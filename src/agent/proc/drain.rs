//! The pipe reader the supervisor never has to join.
//!
//! Split out of `src/agent/proc.rs`. A child's descendant can inherit a write
//! handle and outlive the child, so a reader joined unconditionally would stall
//! the supervisor for as long as that orphan lives. Each stream accumulates into
//! a shared buffer that is snapshotted after a bounded grace instead, and the
//! abandoned reader exits on its own when the last write handle closes.
//!
//! **What a read that fails, rather than ends, does.** `Read::read` on the pipe
//! answers `Ok(0)` for end of stream and `Err` for a read that did not happen.
//! Only `ErrorKind::Interrupted` is retried: it is the one error the trait
//! defines as non-fatal, and on Unix it is a signal delivered to the reader
//! thread by a handler installed without `SA_RESTART`. Every other error ends
//! the reader and is reported by [`Drain::collect`] as [`DrainError::Read`],
//! never read as end of stream: a transcript cut short by a failed read is not
//! a complete transcript, and handing it over as one would present an
//! inspection that did not happen as the negative answer, "nothing more was
//! written". A `Read` implementation that misreports its count and a reader
//! thread that panics are named the same way ([`DrainError::OverlongRead`],
//! [`DrainError::ReaderPanicked`]) rather than becoming a shorter transcript.
//!
//! **Ownership (§6).** The buffer and the limit flag are held twice, by the
//! supervisor through the [`Drain`] and by the reader thread, with lifetimes
//! neither controls: the supervisor may abandon a reader whose writer has not
//! closed, and the reader may finish before the supervisor looks. That is the
//! multi-owner lifecycle `Arc` is for. The `Mutex` protects one invariant, that
//! the buffer holds at most `limit` bytes and the flag is set whenever a byte
//! was dropped for it, and its critical section is one append. Once the reader
//! has let go, [`Drain::collect`] owns the buffer outright and copies nothing.
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

use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

/// Why a stream could not be captured whole. Each variant names the stream
/// it is about, as the label [`Drain::start`] was given.
#[derive(Debug, Error)]
pub(super) enum DrainError {
    /// The OS refused a thread for the reader. Nothing was read, and the pipe
    /// was closed with the closure that held it.
    #[error("starting the {stream} reader thread: {source}")]
    Start {
        stream: &'static str,
        source: std::io::Error,
    },
    /// A read failed with something other than `Interrupted`. The reader
    /// stopped there; the bytes before it are not handed over as a transcript.
    #[error("reading {stream}: {source}")]
    Read {
        stream: &'static str,
        source: std::io::Error,
    },
    /// The pipe's `Read` claimed more bytes than the buffer it was handed
    /// could hold, which no bytes can substantiate.
    #[error("the {stream} reader was told {reported} bytes filled a {capacity}-byte buffer")]
    OverlongRead {
        stream: &'static str,
        reported: usize,
        capacity: usize,
    },
    /// The reader thread panicked, so its transcript is not complete.
    #[error("the {stream} reader thread panicked")]
    ReaderPanicked { stream: &'static str },
}

/// A pipe reader whose buffer can be collected without joining the thread,
/// so an orphan holding the write end can never stall the supervisor.
pub(super) struct Drain {
    stream: &'static str,
    buf: Arc<Mutex<Vec<u8>>>,
    limited: Arc<AtomicBool>,
    handle: thread::JoinHandle<Result<(), DrainError>>,
}

impl Drain {
    /// Read `pipe` on its own thread until end of stream, keeping the first
    /// `limit` bytes and counting the rest as dropped.
    ///
    /// `stream` labels the stream in every diagnostic and in the thread's
    /// name; nothing is decided on it. The reader keeps reading past the limit
    /// so the child cannot block on a full pipe while the supervisor notices
    /// [`Drain::limit_exceeded`] and terminates its tree.
    ///
    /// # Errors
    ///
    /// [`DrainError::Start`] when the OS refuses the thread. Nothing has been
    /// read; `pipe` is closed.
    pub(super) fn start<R: Read + Send + 'static>(
        stream: &'static str,
        mut pipe: R,
        limit: usize,
    ) -> Result<Self, DrainError> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&buf);
        let limited = Arc::new(AtomicBool::new(false));
        let reader_limited = Arc::clone(&limited);
        let handle = thread::Builder::new()
            // Linux keeps fifteen bytes of a thread name; this fits.
            .name(format!("drain-{stream}"))
            .spawn(move || {
                let mut chunk = [0u8; 8192];
                loop {
                    let read = match pipe.read(&mut chunk) {
                        Ok(0) => return Ok(()),
                        Ok(read) => read,
                        Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                        Err(source) => return Err(DrainError::Read { stream, source }),
                    };
                    let Some(bytes) = chunk.get(..read) else {
                        return Err(DrainError::OverlongRead {
                            stream,
                            reported: read,
                            capacity: chunk.len(),
                        });
                    };
                    // A poisoned buffer is one a reader panicked while holding,
                    // and this is the only reader; the bytes in it are intact.
                    let mut guard = writer.lock().unwrap_or_else(PoisonError::into_inner);
                    let remaining = limit.saturating_sub(guard.len());
                    let retained = remaining.min(bytes.len());
                    guard.extend(bytes.iter().take(retained));
                    if retained < bytes.len() {
                        reader_limited.store(true, Ordering::SeqCst);
                    }
                }
            })
            .map_err(|source| DrainError::Start { stream, source })?;
        Ok(Self {
            stream,
            buf,
            limited,
            handle,
        })
    }

    /// Whether a byte has been dropped for the limit so far. The supervisor
    /// asks this while the child runs, to terminate a tree that exceeded its
    /// allowance without waiting for the run's timeout.
    pub(super) fn limit_exceeded(&self) -> bool {
        self.limited.load(Ordering::SeqCst)
    }

    /// Wait up to `grace` for the reader to finish, then hand back what it
    /// captured: the bytes as text, decoded lossily where they are not UTF-8
    /// (the limit can cut a multi-byte character), and whether any byte was
    /// dropped for the limit.
    ///
    /// The reader finishes at end of stream, which is every write handle
    /// closed, or on a failure. A reader still running when the grace expires
    /// is abandoned rather than joined: it holds only its own handles and
    /// exits when the last writer closes, and what comes back is the bytes
    /// that had arrived when the copy was taken.
    ///
    /// # Errors
    ///
    /// A reader that finished on a failure reports it here in place of a
    /// transcript: [`DrainError::Read`] for a read that failed with anything
    /// but `Interrupted`, [`DrainError::OverlongRead`] for a count the buffer
    /// could not hold, [`DrainError::ReaderPanicked`] for a panic. The bytes
    /// captured before the failure are not returned with it; a transcript
    /// with a hole is not offered as one without.
    pub(super) fn collect(self, grace: Duration) -> Result<(String, bool), DrainError> {
        let started = Instant::now();
        while !self.handle.is_finished() && started.elapsed() < grace {
            thread::sleep(Duration::from_millis(20));
        }
        if self.handle.is_finished() {
            match self.handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
                Err(_payload) => {
                    return Err(DrainError::ReaderPanicked {
                        stream: self.stream,
                    });
                }
            }
        }
        // A reader that has finished has dropped its handle on the buffer, so
        // the buffer is owned here and moved out; one still running keeps its
        // handle, and the supervisor takes a copy of what has arrived rather
        // than waiting for the rest.
        let captured = match Arc::try_unwrap(self.buf) {
            Ok(owned) => owned.into_inner().unwrap_or_else(PoisonError::into_inner),
            Err(shared) => shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        };
        let text = String::from_utf8(captured)
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

    use super::{Drain, DrainError, drain_limit_exceeded};

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
        Delay(Duration),
    }

    struct Scripted(VecDeque<Step>);

    impl Scripted {
        fn new(steps: impl IntoIterator<Item = Step>) -> Self {
            Self(steps.into_iter().collect())
        }
    }

    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            loop {
                match self.0.pop_front() {
                    None => return Ok(0),
                    Some(Step::Bytes(mut bytes)) => {
                        let rest = bytes.split_off(bytes.len().min(buf.len()));
                        if !rest.is_empty() {
                            self.0.push_front(Step::Bytes(rest));
                        }
                        for (slot, byte) in buf.iter_mut().zip(&bytes) {
                            *slot = *byte;
                        }
                        return Ok(bytes.len());
                    }
                    Some(Step::Interrupted) => {
                        return Err(io::Error::from(io::ErrorKind::Interrupted));
                    }
                    Some(Step::Fail(kind)) => return Err(io::Error::new(kind, "scripted failure")),
                    Some(Step::Overlong) => return Ok(buf.len() + 1),
                    Some(Step::Panic) => panic!("scripted reader panic"),
                    Some(Step::Delay(delay)) => thread::sleep(delay),
                }
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
    /// again, and the announcement is sent from the reader's own thread.
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
            "stdout",
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
        Drain::start("stdout", Scripted::new(steps), limit).expect("a reader thread")
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
    fn an_interrupted_read_is_retried_and_the_stream_is_read_on() {
        let drain = start([text("before"), Step::Interrupted, text("after")], 1 << 20);
        assert_eq!(
            drain.collect(BOUND).expect("a complete stream"),
            ("beforeafter".to_owned(), false)
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
                assert_eq!(*stream, "stdout");
                assert_eq!(source.kind(), io::ErrorKind::Other);
            }
            other => panic!("a failed read was reported as {other:?}"),
        }
        assert_eq!(error.to_string(), "reading stdout: scripted failure");
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
                assert_eq!(stream, "stdout");
                assert_eq!((reported, capacity), (8193, 8192));
            }
            other => panic!("an overlong count was reported as {other:?}"),
        }
    }

    #[test]
    fn a_reader_that_panics_is_a_named_failure() {
        let drain = start([text("before"), Step::Panic], 1 << 20);
        match drain.collect(BOUND) {
            Err(DrainError::ReaderPanicked { stream }) => assert_eq!(stream, "stdout"),
            other => panic!("a panicking reader was reported as {other:?}"),
        }
    }

    #[test]
    fn collect_waits_up_to_its_grace_for_the_stream_to_end() {
        let drain = start(
            [
                text("early"),
                Step::Delay(Duration::from_millis(150)),
                text("late"),
            ],
            1 << 20,
        );
        assert_eq!(
            drain.collect(BOUND).expect("a complete stream"),
            ("earlylate".to_owned(), false)
        );
    }

    #[test]
    fn collect_abandons_a_reader_whose_writer_never_closes_once_the_grace_expires() {
        let (drain, chunks, entered) = held(1 << 20);
        entered.recv_timeout(BOUND).expect("the first read");
        chunks
            .send(b"partial".to_vec())
            .expect("the reader is waiting");
        // The second read is announced only after "partial" was appended.
        entered.recv_timeout(BOUND).expect("the second read");

        let grace = Duration::from_millis(100);
        let (done, result) = mpsc::channel();
        let collector = thread::spawn(move || {
            let started = Instant::now();
            let captured = drain.collect(grace);
            let _ = done.send((captured, started.elapsed()));
        });
        // Bounded so a `collect` that joins the reader unconditionally fails
        // this test instead of hanging the harness: the writer is closed on
        // the way out, which lets both threads finish.
        let outcome = result.recv_timeout(BOUND);
        drop(chunks);
        let (captured, waited) = outcome.expect("collect returned within its grace");
        assert_eq!(
            captured.expect("a partial stream is not a failure"),
            ("partial".to_owned(), false)
        );
        assert!(
            waited >= grace,
            "collect returned before its grace: {waited:?}"
        );
        collector.join().expect("the collector thread");
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
}
