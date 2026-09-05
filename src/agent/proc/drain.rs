//! The pipe reader the supervisor never has to join.
//!
//! Split out of `src/agent/proc.rs`. A child's descendant can inherit a write
//! handle and outlive the child, so a reader joined unconditionally would stall
//! the supervisor for as long as that orphan lives. Each stream accumulates into
//! a shared capture that is taken after a bounded grace instead; a reader still
//! running then is released, and it exits at its next return from `read` — the
//! orphan's next write, or the last write handle closing.
//!
//! **What a read that fails, rather than ends, does.** `Read::read` on the pipe
//! answers `Ok(0)` for end of stream and `Err` for a read that did not happen.
//! Only `ErrorKind::Interrupted` is retried, and only [`INTERRUPTED_RETRIES`]
//! times in a row: it is the one error the trait defines as non-fatal — on
//! Unix, a signal delivered to the reader thread by a handler installed
//! without `SA_RESTART`, which the binary never installs but an embedding
//! host's preserved handler can be — and a reader that sees nothing else is in
//! a signal storm, not a retry (this crate has measured `read_to_end` retrying
//! it five million times). Every other error ends the reader and is reported by
//! [`Drain::collect`] as [`DrainError::Read`], never read as end of stream: a
//! transcript cut short by a failed read is not a complete transcript, and
//! handing it over as one would present an inspection that did not happen as
//! the negative answer, "nothing more was written". A `Read` implementation
//! that misreports its count and a reader thread that panics are named the
//! same way ([`DrainError::OverlongRead`], [`DrainError::ReaderPanicked`])
//! rather than becoming a shorter transcript.
//!
//! **How the reader reports (§10).** The reader's verdict — complete, or which
//! failure — is written into the shared capture, under the same lock as the
//! bytes, before the thread ends, and a panic is caught on the thread and
//! written the same way. So the verdict and the bytes are read together and
//! cannot disagree, and a failure decided before the grace expires is seen
//! whether or not the thread has been scheduled off since. The `JoinHandle` is
//! joined whenever the thread has finished; a reader still blocked in `read`
//! when the grace expires is not joined, because it cannot be interrupted, and
//! that is the whole reason this module exists.
//!
//! **Ownership (§6).** The capture, the limit flag and the release flag are
//! held twice, by the supervisor through the [`Drain`] and by the reader
//! thread, with lifetimes neither controls: the supervisor may release a reader
//! whose writer has not closed, and the reader may finish before the supervisor
//! looks. That is the multi-owner lifecycle `Arc` is for. The `Mutex` protects
//! one invariant, that the capture holds at most `limit` bytes, the flag is set
//! whenever a byte was dropped for it, and a verdict, once written, is the
//! verdict of those bytes; its critical sections are one append and one
//! verdict. Once the reader has let go, [`Drain::collect`] owns the capture
//! outright and copies nothing.
//!
//! `PR6-LANEF-004`: it states its own lint level rather than inheriting the
//! funnel's `#![allow]`, and denies all three governed lints. No
//! `effects/allowlist.toml` row: a denial needs none. `Drain::start` is this
//! module's own name and `thread::Builder::spawn` is not
//! `std::process::Command::spawn`; a segment-matching scan flags both and
//! neither is a denied primitive.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::fmt;
use std::io::{ErrorKind, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

/// How many times in a row an interrupted read is retried before the reader
/// gives up. A read interrupted this often with no byte between the
/// interruptions is a signal storm, not a retry.
pub(super) const INTERRUPTED_RETRIES: usize = 64;

/// Which of the child's two output streams a reader is reading. A closed set,
/// so the thread name built from it cannot carry a NUL and nothing can be
/// passed that is not one of the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    /// Linux keeps fifteen bytes of a thread name; both fit.
    fn thread_name(self) -> &'static str {
        match self {
            Self::Stdout => "drain-stdout",
            Self::Stderr => "drain-stderr",
        }
    }
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        })
    }
}

/// Why a stream could not be captured whole. Each variant names the stream
/// it is about.
#[derive(Debug, Error)]
pub(super) enum DrainError {
    /// The OS refused a thread for the reader. Nothing was read, and the pipe
    /// was closed with the closure that held it.
    #[error("starting the {stream} reader thread: {source}")]
    Start {
        stream: Stream,
        source: std::io::Error,
    },
    /// A read failed with something other than `Interrupted`. The reader
    /// stopped there; the bytes before it are not handed over as a transcript.
    #[error("reading {stream}: {source}")]
    Read {
        stream: Stream,
        source: std::io::Error,
    },
    /// Reads were interrupted [`INTERRUPTED_RETRIES`] times in a row with no
    /// byte between them, and the reader gave up.
    #[error("reading {stream}: interrupted {retries} times in a row")]
    Interrupted { stream: Stream, retries: usize },
    /// The pipe's `Read` claimed more bytes than the buffer it was handed
    /// could hold, which no bytes can substantiate.
    #[error("the {stream} reader was told {reported} bytes filled a {capacity}-byte buffer")]
    OverlongRead {
        stream: Stream,
        reported: usize,
        capacity: usize,
    },
    /// The reader thread panicked, so its transcript is not complete.
    #[error("the {stream} reader thread panicked")]
    ReaderPanicked { stream: Stream },
}

/// What the reader has captured and, once it has finished, its verdict.
#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    /// `None` while the reader is still reading. Written once, under the
    /// lock, before the reader's thread ends.
    verdict: Option<Result<(), DrainError>>,
}

/// A pipe reader whose capture can be taken without joining the thread, so
/// an orphan holding the write end can never stall the supervisor.
pub(super) struct Drain {
    stream: Stream,
    capture: Arc<Mutex<Capture>>,
    limited: Arc<AtomicBool>,
    /// Set by [`Drain::collect`] when it gives up waiting. A reader that sees
    /// it at its next return from `read` stops reading and closes the pipe.
    released: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

/// The reader's loop: what it captured is in `capture`, and its verdict is the
/// return value, which the caller writes into `capture` under the lock.
fn read_into<R: Read>(
    stream: Stream,
    pipe: &mut R,
    limit: usize,
    capture: &Mutex<Capture>,
    limited: &AtomicBool,
    released: &AtomicBool,
) -> Result<(), DrainError> {
    let mut chunk = [0u8; 8192];
    let mut interrupted = 0_usize;
    loop {
        let read = match pipe.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {
                interrupted += 1;
                if interrupted > INTERRUPTED_RETRIES {
                    return Err(DrainError::Interrupted {
                        stream,
                        retries: INTERRUPTED_RETRIES,
                    });
                }
                continue;
            }
            Err(source) => return Err(DrainError::Read { stream, source }),
        };
        interrupted = 0;
        if released.load(Ordering::SeqCst) {
            // The supervisor has taken the capture and moved on; these bytes
            // are an orphan's, and closing the pipe is what ends it.
            return Ok(());
        }
        let Some(bytes) = chunk.get(..read) else {
            return Err(DrainError::OverlongRead {
                stream,
                reported: read,
                capacity: chunk.len(),
            });
        };
        // A poisoned capture is one a reader panicked while holding, and this
        // is the only reader; the bytes in it are intact.
        let mut guard = capture.lock().unwrap_or_else(PoisonError::into_inner);
        let remaining = limit.saturating_sub(guard.bytes.len());
        let retained = remaining.min(bytes.len());
        guard.bytes.extend(bytes.iter().take(retained));
        if retained < bytes.len() {
            limited.store(true, Ordering::SeqCst);
        }
    }
}

impl Drain {
    /// Read `pipe` on its own thread until end of stream, keeping the first
    /// `limit` bytes and counting the rest as dropped.
    ///
    /// The reader keeps reading past the limit so the child cannot block on
    /// a full pipe while the supervisor notices [`Drain::limit_exceeded`] and
    /// terminates its tree.
    ///
    /// # Errors
    ///
    /// [`DrainError::Start`] when the OS refuses the thread — `EAGAIN` at the
    /// process's thread limit, or no memory for a stack — which a coordinator
    /// under resource exhaustion can meet. Nothing has been read; `pipe` is
    /// closed.
    pub(super) fn start<R: Read + Send + 'static>(
        stream: Stream,
        pipe: R,
        limit: usize,
    ) -> Result<Self, DrainError> {
        let capture = Arc::new(Mutex::new(Capture::default()));
        let limited = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let reader = (
            Arc::clone(&capture),
            Arc::clone(&limited),
            Arc::clone(&released),
        );
        let handle = thread::Builder::new()
            .name(stream.thread_name().to_owned())
            .spawn(move || {
                let (capture, limited, released) = reader;
                // Rebound after the shared handles so it is dropped before
                // them: the pipe closes first, and the capture stays shared
                // until the thread has nothing left to do with it.
                let mut pipe = pipe;
                // A panic anywhere in the loop is this reader's verdict, and
                // it is written where the bytes are rather than left for a
                // join that may never happen.
                let verdict = catch_unwind(AssertUnwindSafe(|| {
                    read_into(stream, &mut pipe, limit, &capture, &limited, &released)
                }))
                .unwrap_or(Err(DrainError::ReaderPanicked { stream }));
                capture
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .verdict = Some(verdict);
            })
            .map_err(|source| DrainError::Start { stream, source })?;
        Ok(Self {
            stream,
            capture,
            limited,
            released,
            handle,
        })
    }

    /// Whether a byte has been dropped for the limit so far. The supervisor
    /// asks this while the child runs, to terminate a tree that exceeded its
    /// allowance without waiting for the run's timeout.
    pub(super) fn limit_exceeded(&self) -> bool {
        self.limited.load(Ordering::SeqCst)
    }

    /// Wait up to `grace` for the reader's verdict, then hand back what it
    /// captured: the bytes as text, decoded lossily where they are not UTF-8
    /// (the limit can cut a multi-byte character), and whether any byte was
    /// dropped for the limit.
    ///
    /// The reader's verdict arrives at end of stream — every write handle
    /// closed — or on a failure, and is read together with the bytes under
    /// one lock, so a failure decided before the grace expired is seen even if
    /// the thread has not yet been scheduled off. A reader with no verdict
    /// when the grace expires is released rather than joined: it is blocked
    /// in a `read` nothing can interrupt, and it will stop at its next return
    /// from it, when the orphan next writes or closes. What comes back then is
    /// the bytes that had arrived when the capture was taken, and a failure
    /// the released reader meets afterwards belongs to no transcript. A
    /// reader whose thread has finished is joined; one that is still tearing
    /// down after writing its verdict is not waited for.
    ///
    /// # Errors
    ///
    /// The reader's verdict when it is a failure, in place of a transcript:
    /// [`DrainError::Read`] for a read that failed with anything but
    /// `Interrupted`, [`DrainError::Interrupted`] for a read interrupted
    /// [`INTERRUPTED_RETRIES`] times in a row, [`DrainError::OverlongRead`]
    /// for a count the buffer could not hold, [`DrainError::ReaderPanicked`]
    /// for a panic. The bytes captured before the failure are not returned
    /// with it; a transcript with a hole is not offered as one without.
    pub(super) fn collect(self, grace: Duration) -> Result<(String, bool), DrainError> {
        let started = Instant::now();
        loop {
            let decided = self
                .capture
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .verdict
                .is_some();
            if decided || started.elapsed() >= grace {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        self.released.store(true, Ordering::SeqCst);
        // Joined whenever the thread is done, so a finished reader is reaped.
        // Its verdict, a panic included, is already in the capture; a join
        // that nevertheless reports a panic (one raised while the pipe was
        // being dropped) is a verdict too.
        let mut torn_down = None;
        if self.handle.is_finished() && self.handle.join().is_err() {
            torn_down = Some(DrainError::ReaderPanicked {
                stream: self.stream,
            });
        }
        // A reader that has let go leaves the capture owned here, and it is
        // moved out; one still running keeps its handle, and the supervisor
        // takes a copy of what has arrived and the verdict, if any, that came
        // with it.
        let (bytes, verdict) = match Arc::try_unwrap(self.capture) {
            Ok(owned) => {
                let capture = owned.into_inner().unwrap_or_else(PoisonError::into_inner);
                (capture.bytes, capture.verdict)
            }
            Err(shared) => {
                let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
                (guard.bytes.clone(), guard.verdict.take())
            }
        };
        match (verdict, torn_down) {
            (Some(Err(error)), _) => return Err(error),
            (_, Some(error)) => return Err(error),
            (Some(Ok(())) | None, None) => {}
        }
        let text = String::from_utf8(bytes)
            .unwrap_or_else(|invalid| String::from_utf8_lossy(invalid.as_bytes()).into_owned());
        Ok((text, self.limited.load(Ordering::SeqCst)))
    }
}

/// Whether either stream has dropped a byte for its limit; a stream that was
/// not captured at all has not.
pub(super) fn drain_limit_exceeded(stdout: &Option<Drain>, stderr: &Option<Drain>) -> bool {
    stdout.as_ref().is_some_and(Drain::limit_exceeded)
        || stderr.as_ref().is_some_and(Drain::limit_exceeded)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{self, Read};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{Drain, DrainError, INTERRUPTED_RETRIES, Stream, drain_limit_exceeded};

    /// The bound on every wait below. It bounds a wedged reader, never a
    /// healthy one: nothing here takes more than a few hundred milliseconds.
    const BOUND: Duration = Duration::from_secs(10);

    /// What a scripted pipe answers to each `read`, in order. An exhausted
    /// script is end of stream.
    enum Step {
        Bytes(Vec<u8>),
        Interrupted,
        Fail(io::ErrorKind),
        Overlong,
        Panic,
    }

    /// A pipe that answers from a script. If `hold_drop` is set, dropping it
    /// — which the reader does after writing its verdict, when its closure
    /// returns — blocks until the test sends or drops the sender, so a test
    /// can hold the thread between its verdict and its end.
    struct Scripted {
        steps: VecDeque<Step>,
        hold_drop: Option<mpsc::Receiver<()>>,
    }

    impl Scripted {
        fn new(steps: impl IntoIterator<Item = Step>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                hold_drop: None,
            }
        }
    }

    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.steps.pop_front() {
                None => Ok(0),
                Some(Step::Bytes(mut bytes)) => {
                    let rest = bytes.split_off(bytes.len().min(buf.len()));
                    if !rest.is_empty() {
                        self.steps.push_front(Step::Bytes(rest));
                    }
                    for (slot, byte) in buf.iter_mut().zip(&bytes) {
                        *slot = *byte;
                    }
                    Ok(bytes.len())
                }
                Some(Step::Interrupted) => Err(io::Error::from(io::ErrorKind::Interrupted)),
                Some(Step::Fail(kind)) => Err(io::Error::new(kind, "scripted failure")),
                Some(Step::Overlong) => Ok(buf.len() + 1),
                Some(Step::Panic) => panic!("scripted reader panic"),
            }
        }
    }

    impl Drop for Scripted {
        fn drop(&mut self) {
            if let Some(hold) = &self.hold_drop {
                let _ = hold.recv();
            }
        }
    }

    fn text(bytes: &str) -> Step {
        Step::Bytes(bytes.as_bytes().to_vec())
    }

    /// A pipe whose writer is the test. Each `read` announces itself on
    /// `entered` and then blocks until the test sends a chunk; a dropped
    /// sender is end of stream. The announcement is what lets a test know the
    /// previous chunk has been appended: the reader appends before it reads
    /// again, and the announcement is sent from the reader's own thread. When
    /// the reader stops and drops this, `entered` disconnects.
    struct Held {
        chunks: mpsc::Receiver<Vec<u8>>,
        entered: mpsc::Sender<()>,
    }

    impl Read for Held {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let _ = self.entered.send(());
            match self.chunks.recv() {
                Err(mpsc::RecvError) => Ok(0),
                Ok(chunk) => {
                    assert!(chunk.len() <= buf.len(), "the test sends chunks that fit");
                    for (slot, byte) in buf.iter_mut().zip(&chunk) {
                        *slot = *byte;
                    }
                    Ok(chunk.len())
                }
            }
        }
    }

    fn held(limit: usize) -> (Drain, mpsc::Sender<Vec<u8>>, mpsc::Receiver<()>) {
        let (chunks, receiver) = mpsc::channel();
        let (entered, announcements) = mpsc::channel();
        let drain = Drain::start(
            Stream::Stdout,
            Held {
                chunks: receiver,
                entered,
            },
            limit,
        )
        .expect("a reader thread");
        (drain, chunks, announcements)
    }

    fn start(steps: impl IntoIterator<Item = Step>, limit: usize) -> Drain {
        Drain::start(Stream::Stdout, Scripted::new(steps), limit).expect("a reader thread")
    }

    /// What `collect` returned, and how long it took to.
    type Collected = (Result<(String, bool), DrainError>, Duration);

    /// `collect` on its own thread, so a `collect` that does not return within
    /// `BOUND` fails the test that called this instead of hanging the harness.
    fn collect_elsewhere(drain: Drain, grace: Duration) -> mpsc::Receiver<Collected> {
        let (done, result) = mpsc::channel();
        thread::spawn(move || {
            let started = Instant::now();
            let captured = drain.collect(grace);
            let _ = done.send((captured, started.elapsed()));
        });
        result
    }

    #[test]
    fn collect_returns_what_the_pipe_delivered_before_end_of_stream() {
        let drain = start([text("hello "), text("world")], 1 << 20);
        let captured = drain.collect(BOUND).expect("a complete stream");
        assert_eq!(captured, ("hello world".to_owned(), false));
    }

    #[test]
    fn the_limit_keeps_exactly_limit_bytes_and_reports_only_a_dropped_byte() {
        // Exactly at the limit: nothing dropped, so not limited.
        let exact = start([text("abcd"), text("efghij")], 10);
        assert_eq!(
            exact.collect(BOUND).expect("a complete stream"),
            ("abcdefghij".to_owned(), false)
        );
        // One chunk straddles the limit: kept to the byte, and limited.
        let over = start([text("abcd"), text("efghijkl")], 10);
        assert_eq!(
            over.collect(BOUND).expect("a complete stream"),
            ("abcdefghij".to_owned(), true)
        );
        // A limit of zero keeps nothing and reports the first byte.
        let none = start([text("a")], 0);
        assert_eq!(
            none.collect(BOUND).expect("a complete stream"),
            (String::new(), true)
        );
    }

    #[test]
    fn interrupted_reads_are_retried_up_to_the_bound_and_the_stream_read_on() {
        let mut steps = vec![text("before")];
        steps.extend((0..INTERRUPTED_RETRIES).map(|_| Step::Interrupted));
        steps.push(text("after"));
        let drain = start(steps, 1 << 20);
        assert_eq!(
            drain.collect(BOUND).expect("a complete stream"),
            ("beforeafter".to_owned(), false)
        );
    }

    #[test]
    fn one_interruption_past_the_bound_ends_the_reader_with_a_named_failure() {
        let mut steps = vec![text("before")];
        steps.extend((0..=INTERRUPTED_RETRIES).map(|_| Step::Interrupted));
        steps.push(text("never"));
        let drain = start(steps, 1 << 20);
        match drain.collect(BOUND) {
            Err(DrainError::Interrupted { stream, retries }) => {
                assert_eq!((stream, retries), (Stream::Stdout, INTERRUPTED_RETRIES));
            }
            other => panic!("a signal storm was reported as {other:?}"),
        }
    }

    #[test]
    fn a_byte_between_interruptions_restarts_the_bound() {
        // Twice the bound in total, never the bound in a row.
        let mut steps = Vec::new();
        for _ in 0..2 {
            steps.extend((0..INTERRUPTED_RETRIES).map(|_| Step::Interrupted));
            steps.push(text("x"));
        }
        let drain = start(steps, 1 << 20);
        assert_eq!(
            drain.collect(BOUND).expect("a complete stream"),
            ("xx".to_owned(), false)
        );
    }

    #[test]
    fn a_read_that_fails_is_reported_and_not_read_as_end_of_stream() {
        let drain = start(
            [
                text("before"),
                Step::Fail(io::ErrorKind::Other),
                text("never"),
            ],
            1 << 20,
        );
        let error = drain.collect(BOUND).expect_err("a failed read");
        match &error {
            DrainError::Read { stream, source } => {
                assert_eq!(*stream, Stream::Stdout);
                assert_eq!(source.kind(), io::ErrorKind::Other);
            }
            other => panic!("a failed read was reported as {other:?}"),
        }
        assert_eq!(error.to_string(), "reading stdout: scripted failure");
    }

    #[test]
    fn a_failure_decided_before_the_grace_is_seen_even_if_the_thread_has_not_finished() {
        // The reader fails, writes its verdict, and is then held between the
        // verdict and the end of its thread: the pipe's `Drop` blocks until
        // `release` is dropped, and the pipe is dropped before the reader's
        // handle on the capture, so the capture is still shared while it is
        // held. A `collect` that read the verdict off the thread's completion,
        // or only off a capture it owned outright, would hand the bytes over
        // as complete.
        let (release, hold) = mpsc::channel::<()>();
        let mut pipe = Scripted::new([text("before"), Step::Fail(io::ErrorKind::Other)]);
        pipe.hold_drop = Some(hold);
        let drain = Drain::start(Stream::Stdout, pipe, 1 << 20).expect("a reader thread");
        let result = collect_elsewhere(drain, Duration::from_millis(100));
        let outcome = result.recv_timeout(BOUND);
        drop(release);
        let (captured, _) = outcome.expect("collect returned");
        match captured {
            Err(DrainError::Read { stream, source }) => {
                assert_eq!(stream, Stream::Stdout);
                assert_eq!(source.kind(), io::ErrorKind::Other);
            }
            other => panic!("a failure decided before the grace was reported as {other:?}"),
        }
    }

    #[test]
    fn a_read_that_misreports_its_count_is_a_failure_and_not_a_panic() {
        let drain = start([text("before"), Step::Overlong], 1 << 20);
        match drain.collect(BOUND) {
            Err(DrainError::OverlongRead {
                stream,
                reported,
                capacity,
            }) => {
                assert_eq!(stream, Stream::Stdout);
                assert_eq!((reported, capacity), (8193, 8192));
            }
            other => panic!("an overlong count was reported as {other:?}"),
        }
    }

    #[test]
    fn a_reader_that_panics_is_a_named_failure() {
        let drain = start([text("before"), Step::Panic], 1 << 20);
        match drain.collect(BOUND) {
            Err(DrainError::ReaderPanicked { stream }) => assert_eq!(stream, Stream::Stdout),
            other => panic!("a panicking reader was reported as {other:?}"),
        }
    }

    #[test]
    fn collect_waits_for_the_stream_to_end_and_returns_what_arrived_by_then() {
        // The reader is blocked in its first `read` before `collect` starts,
        // so `collect` has nothing to return until the writer closes.
        let (drain, chunks, entered) = held(1 << 20);
        entered.recv_timeout(BOUND).expect("the first read");
        let result = collect_elsewhere(drain, BOUND);
        // A `collect` that did not wait would be back within this; the real
        // one is still waiting when it expires, and a healthy run cannot fail
        // here because nothing has been sent.
        assert!(
            matches!(
                result.recv_timeout(Duration::from_secs(1)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "collect returned before the stream ended"
        );
        chunks
            .send(b"late".to_vec())
            .expect("the reader is waiting");
        drop(chunks);
        let (captured, _) = result.recv_timeout(BOUND).expect("collect returned");
        assert_eq!(
            captured.expect("a complete stream"),
            ("late".to_owned(), false)
        );
    }

    #[test]
    fn collect_releases_a_reader_whose_writer_never_closes_once_the_grace_expires() {
        let (drain, chunks, entered) = held(1 << 20);
        entered.recv_timeout(BOUND).expect("the first read");
        chunks
            .send(b"partial".to_vec())
            .expect("the reader is waiting");
        // The second read is announced only after "partial" was appended.
        entered.recv_timeout(BOUND).expect("the second read");

        let grace = Duration::from_millis(100);
        let result = collect_elsewhere(drain, grace);
        // Bounded so a `collect` that joins the reader unconditionally fails
        // this test instead of hanging the harness: the writer is closed on
        // the way out, which lets both threads finish.
        let outcome = result.recv_timeout(BOUND);
        let (captured, waited) = match outcome {
            Ok(returned) => returned,
            Err(timeout) => {
                drop(chunks);
                panic!("collect outlived its grace: {timeout:?}");
            }
        };
        assert_eq!(
            captured.expect("a partial stream is not a failure"),
            ("partial".to_owned(), false)
        );
        assert!(
            waited >= grace,
            "collect returned before its grace: {waited:?}"
        );

        // Released: the reader's next return from `read` ends it, so the
        // writer's next chunk is followed by the pipe being dropped — the
        // announcement channel disconnects — and not by another read.
        chunks
            .send(b"orphan".to_vec())
            .expect("the reader is waiting");
        assert!(
            matches!(
                entered.recv_timeout(BOUND),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "a released reader read again instead of stopping"
        );
    }

    #[test]
    fn limit_exceeded_is_visible_while_the_reader_still_runs() {
        let (drain, chunks, entered) = held(4);
        entered.recv_timeout(BOUND).expect("the first read");
        chunks
            .send(b"12345".to_vec())
            .expect("the reader is waiting");
        entered.recv_timeout(BOUND).expect("the second read");
        assert!(
            drain.limit_exceeded(),
            "five bytes into a four-byte allowance"
        );
        let stdout = Some(drain);
        assert!(drain_limit_exceeded(&stdout, &None));
        assert!(drain_limit_exceeded(&None, &stdout));
        assert!(!drain_limit_exceeded(&None, &None));

        drop(chunks);
        let Some(drain) = stdout else {
            panic!("the drain was placed in the option two statements ago");
        };
        assert_eq!(
            drain.collect(BOUND).expect("a complete stream"),
            ("1234".to_owned(), true)
        );
    }

    #[test]
    fn bytes_that_are_not_utf8_are_decoded_lossily_rather_than_dropped() {
        let drain = start([Step::Bytes(vec![b'a', 0xff, b'b'])], 1 << 20);
        assert_eq!(
            drain.collect(BOUND).expect("a complete stream"),
            ("a\u{FFFD}b".to_owned(), false)
        );
    }

    #[test]
    fn the_stderr_label_names_the_thread_and_the_error() {
        let drain = Drain::start(
            Stream::Stderr,
            Scripted::new([Step::Fail(io::ErrorKind::BrokenPipe)]),
            1 << 20,
        )
        .expect("a reader thread");
        let error = drain.collect(BOUND).expect_err("a failed read");
        assert_eq!(error.to_string(), "reading stderr: scripted failure");
        assert_eq!(Stream::Stderr.thread_name(), "drain-stderr");
        assert_eq!(Stream::Stdout.thread_name(), "drain-stdout");
    }
}
