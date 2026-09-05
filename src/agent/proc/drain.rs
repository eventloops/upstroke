//! Bounded output capture with a joined nonblocking pipe worker.
//!
//! The reader retains at most the byte limit while draining excess output.
//! WouldBlock parks for a finite interval; consecutive Interrupted results
//! have a fixed retry bound. Other read errors and invalid counts publish a
//! typed failure. The caught region includes pipe teardown, so success follows
//! close and a panic becomes ReaderPanicked.
//!
//! The supervisor and worker share capture and limit state because the
//! supervisor inspects output while the child runs. Capture's mutex protects
//! bytes and their published verdict together and never spans a pipe operation.
//! The worker's separate release flag arbitrates cancellation. At the grace,
//! collection releases and joins the worker before taking its capture. Drop
//! also releases and joins. A live escaped writer cannot make a nonblocking
//! poll wait for more input. Only observed EOF means ended; release means the
//! retained prefix is partial even when the worker finishes before collection.
//!
//! The owned pipe boundary excludes arbitrary blocking Read implementations.
//! Worker settlement needs scheduling and finite local operations, never an
//! external peer's cooperation. Normal collection returns every published
//! failure after settlement. An uncollected drop joins and records its failure
//! in the invocation's retained FailureReport instead of losing the error on
//! an early return. A caller retains that observer before starting a sibling.
//!
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::fmt;
use std::io::ErrorKind;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::pipe_io::PollRead;
use super::worker::{FailureReport, POLL_INTERVAL, Worker};
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
    /// Reads were interrupted `interruptions` times in a row — one more than
    /// [`INTERRUPTED_RETRIES`] — with no byte between them, and the reader
    /// gave up.
    #[error("reading {stream}: interrupted {interruptions} times in a row")]
    Interrupted {
        stream: Stream,
        interruptions: usize,
    },
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

/// What [`Drain::collect`] hands back.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Captured {
    /// The bytes as text, decoded lossily where they are not UTF-8 (the limit
    /// can cut a multi-byte character).
    pub(super) text: String,
    /// Whether any byte was dropped for the limit.
    pub(super) limited: bool,
    /// Whether the reader observed EOF. False means release stopped polling
    /// before EOF, so these bytes are a partial prefix. Collection joins the
    /// worker before returning either outcome.
    pub(super) ended: bool,
}

/// The retained output before text decoding. Consumers of byte-oriented
/// subprocess protocols must inspect `limited` and `ended` before accepting
/// the bytes as a complete response.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct CapturedBytes {
    /// The retained bytes in their original encoding, including invalid UTF-8.
    pub(super) bytes: Vec<u8>,
    /// Whether any byte was dropped for the byte limit.
    pub(super) limited: bool,
    /// Whether EOF was observed. Release leaves this false even if the
    /// released worker finishes before collection takes its capture.
    pub(super) ended: bool,
}

/// What the reader has captured and, once it has finished, its verdict.
#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    /// `None` while the reader is still reading. Written once, under the
    /// lock, at the moment the reader decides.
    verdict: Option<Result<ReadEnd, DrainError>>,
}

/// Why a reader stopped without a read failure. Releasing a reader does not
/// prove that the stream ended, even when it stops before collection finishes.
enum ReadEnd {
    Eof,
    Released,
}

/// Proof that a failure has been published into the capture. Only
/// [`publish_failure`] makes one, so a reader that returns it has written its
/// verdict and there is nothing left for the thread to publish.
struct Published;

fn publish_failure(capture: &Mutex<Capture>, error: DrainError) -> Published {
    // A poisoned capture is one a reader panicked while holding, and this is
    // the only reader; the bytes in it are intact.
    capture
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .verdict = Some(Err(error));
    Published
}

/// A nonblocking pipe reader released and joined on collection or drop.
pub(super) struct Drain {
    stream: Stream,
    capture: Arc<Mutex<Capture>>,
    limited: Arc<AtomicBool>,
    worker: Worker,
    report: FailureReport,
}

/// The reader's loop. A failure is published here, under the lock, at the
/// point of decision, and `Err(Published)` says so. A successful return names
/// whether EOF or release stopped the loop; the caller publishes that outcome
/// once the pipe is closed.
fn read_into<R: PollRead>(
    stream: Stream,
    pipe: &mut R,
    limit: usize,
    capture: &Mutex<Capture>,
    limited: &AtomicBool,
    released: &AtomicBool,
) -> Result<ReadEnd, Published> {
    let mut chunk = [0u8; 8192];
    let mut interrupted = 0_usize;
    loop {
        if released.load(Ordering::SeqCst) {
            return Ok(ReadEnd::Released);
        }
        let outcome = pipe.try_read(&mut chunk);
        let cancelled = released.load(Ordering::SeqCst);
        let read = match outcome {
            // A real failure is classified before cancellation. Cancellation
            // stops further bytes/retries, not an error this finite poll saw.
            Err(source)
                if !matches!(
                    source.kind(),
                    ErrorKind::Interrupted | ErrorKind::WouldBlock
                ) =>
            {
                return Err(publish_failure(
                    capture,
                    DrainError::Read { stream, source },
                ));
            }
            Ok(read) if read > chunk.len() => {
                return Err(publish_failure(
                    capture,
                    DrainError::OverlongRead {
                        stream,
                        reported: read,
                        capacity: chunk.len(),
                    },
                ));
            }
            _ if cancelled => return Ok(ReadEnd::Released),
            Ok(0) => return Ok(ReadEnd::Eof),
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                interrupted = 0;
                thread::park_timeout(POLL_INTERVAL);
                continue;
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {
                interrupted += 1;
                if interrupted > INTERRUPTED_RETRIES {
                    return Err(publish_failure(
                        capture,
                        DrainError::Interrupted {
                            stream,
                            interruptions: interrupted,
                        },
                    ));
                }
                continue;
            }
            Err(source) => {
                return Err(publish_failure(
                    capture,
                    DrainError::Read { stream, source },
                ));
            }
        };
        interrupted = 0;
        let Some(bytes) = chunk.get(..read) else {
            return Err(publish_failure(
                capture,
                DrainError::OverlongRead {
                    stream,
                    reported: read,
                    capacity: chunk.len(),
                },
            ));
        };
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
    pub(super) fn start<R: PollRead>(
        stream: Stream,
        pipe: R,
        limit: usize,
    ) -> Result<Self, DrainError> {
        let capture = Arc::new(Mutex::new(Capture::default()));
        let limited = Arc::new(AtomicBool::new(false));
        let reader = (Arc::clone(&capture), Arc::clone(&limited));
        let worker = Worker::spawn(stream.thread_name(), move |released| {
            let (capture, limited) = reader;
            // A second handle on the capture stays outside the caught
            // region, so a panic verdict can be published after the
            // region's own handle has gone with the unwind.
            let after_unwind = Arc::clone(&capture);
            // The pipe is read, then dropped, inside the caught region:
            // a panic anywhere in the loop or in the pipe's own drop is
            // this reader's verdict, written beside its captured bytes.
            // Collection and Drop both join this worker. A failure has
            // already been published by the loop when it returns
            // `Err(Published)`; end of stream is published only now, with
            // the pipe closed, so nothing can still panic after `Ok`.
            let outcome = catch_unwind(AssertUnwindSafe(move || {
                let mut pipe = pipe;
                let ended = read_into(stream, &mut pipe, limit, &capture, &limited, released);
                drop(pipe);
                (ended, capture)
            }));
            match outcome {
                Ok((Ok(ended), capture)) => {
                    capture
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .verdict
                        .get_or_insert(Ok(ended));
                }
                Ok((Err(Published), _capture)) => {}
                Err(_payload) => publish_panic(&after_unwind, stream),
            }
        })
        .map_err(|source| DrainError::Start { stream, source })?;
        Ok(Self {
            stream,
            capture,
            limited,
            worker,
            report: FailureReport::default(),
        })
    }

    /// Whether a byte has been dropped for the limit so far. The supervisor
    /// asks this while the child runs, to terminate a tree that exceeded its
    /// allowance without waiting for the run's timeout.
    pub(super) fn limit_exceeded(&self) -> bool {
        self.limited.load(Ordering::SeqCst)
    }

    /// Retain a secondary-error observer before another worker can fail.
    pub(super) fn failure_report(&self) -> FailureReport {
        self.report.clone()
    }

    /// Wait up to `grace` for EOF or failure, then release and join the worker.
    /// The kernel polls are nonblocking, so settlement does not wait for an
    /// escaped writer. Scheduling can delay a join; the grace bounds waiting
    /// for peer activity, not the operating system's scheduling latency.
    ///
    /// Only a published EOF sets ended. Release returns the retained prefix
    /// with ended=false. The text API retains its lossy UTF-8 conversion.
    ///
    /// # Errors
    ///
    /// Read failure, interruption exhaustion, an invalid count, or a worker
    /// or pipe-teardown panic. A joined worker's failure is never inferred
    /// from a missing EOF or discarded because teardown was still pending.
    pub(super) fn collect(self, grace: Duration) -> Result<Captured, DrainError> {
        let CapturedBytes {
            bytes,
            limited,
            ended,
        } = self.collect_bytes(grace)?;
        let text = String::from_utf8(bytes)
            .unwrap_or_else(|invalid| String::from_utf8_lossy(invalid.as_bytes()).into_owned());
        Ok(Captured {
            text,
            limited,
            ended,
        })
    }

    /// Collect with the same grace, limit, and EOF semantics as [`Self::collect`],
    /// preserving the retained bytes instead of decoding them as text.
    ///
    /// # Errors
    ///
    /// The same stream-specific failures as [`Self::collect`], in place of
    /// bytes that a caller could mistake for a complete response.
    pub(super) fn collect_bytes(self, grace: Duration) -> Result<CapturedBytes, DrainError> {
        self.collect_with_wait(grace, || thread::sleep(Duration::from_millis(20)))
    }

    /// The collection protocol, with its polling wait supplied separately so
    /// tests can observe a pending reader and force the disputed ordering.
    fn collect_with_wait(
        mut self,
        grace: Duration,
        mut wait: impl FnMut(),
    ) -> Result<CapturedBytes, DrainError> {
        let stream = self.stream;
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
            wait();
        }
        // Nonblocking polls and finite parking allow release plus a join
        // without waiting for an escaped descendant to act on the pipe.
        let torn_down = self
            .worker
            .settle()
            .then_some(DrainError::ReaderPanicked { stream });
        // A reader that has let go leaves the capture owned here, and it is
        // moved out; one still running keeps its handle, and the supervisor
        // takes a copy of what has arrived and the verdict, if any, that came
        // with it.
        let (bytes, verdict) = {
            let mut capture = self.capture.lock().unwrap_or_else(PoisonError::into_inner);
            (std::mem::take(&mut capture.bytes), capture.verdict.take())
        };
        let ended = match (verdict, torn_down) {
            (Some(Err(error)), _) => return Err(error),
            (_, Some(error)) => return Err(error),
            (Some(Ok(ReadEnd::Eof)), None) => true,
            (None | Some(Ok(ReadEnd::Released)), None) => false,
        };
        Ok(CapturedBytes {
            bytes,
            limited: self.limited.load(Ordering::SeqCst),
            ended,
        })
    }
}

impl Drop for Drain {
    fn drop(&mut self) {
        let panicked = self.worker.settle();
        let error = self
            .capture
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .verdict
            .take()
            .and_then(Result::err);
        match (error, panicked) {
            (Some(error), true) => self.report.record(format!(
                "{error}; {} worker also panicked during settlement",
                self.stream
            )),
            (Some(error), false) => self.report.record(error),
            (None, true) => self.report.record(DrainError::ReaderPanicked {
                stream: self.stream,
            }),
            (None, false) => {}
        }
    }
}

/// The panic verdict, written by the reader's thread after its caught region
/// unwound; `capture` here is the handle the thread kept outside that region.
fn publish_panic(capture: &Mutex<Capture>, stream: Stream) {
    capture
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .verdict
        .get_or_insert(Err(DrainError::ReaderPanicked { stream }));
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
    use std::io;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{Captured, Drain, DrainError, INTERRUPTED_RETRIES, Stream, drain_limit_exceeded};

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
    /// — which the reader does inside its caught region, after a failure has
    /// been published and before end of stream is — blocks until the test
    /// sends or drops the sender, so a test can hold the thread there. If
    /// `panic_on_drop` is set, dropping it panics instead.
    struct Scripted {
        steps: VecDeque<Step>,
        hold_drop: Option<mpsc::Receiver<()>>,
        drop_entered: Option<mpsc::Sender<()>>,
        panic_on_drop: bool,
    }

    impl Scripted {
        fn new(steps: impl IntoIterator<Item = Step>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                hold_drop: None,
                drop_entered: None,
                panic_on_drop: false,
            }
        }
    }

    impl super::PollRead for Scripted {
        fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
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
            if let Some(entered) = &self.drop_entered {
                let _ = entered.send(());
            }
            if let Some(hold) = &self.hold_drop {
                let _ = hold.recv_timeout(BOUND);
            }
            assert!(
                !self.panic_on_drop,
                "scripted panic while dropping the pipe"
            );
        }
    }

    fn text(bytes: &str) -> Step {
        Step::Bytes(bytes.as_bytes().to_vec())
    }

    /// What the test feeds a held pipe: bytes, or an interruption.
    enum Feed {
        Bytes(Vec<u8>),
        ReleaseThenInterrupt(std::sync::Arc<super::AtomicBool>),
    }

    /// A pipe whose writer is the test. Each `read` announces itself on
    /// `entered` and then blocks until the test sends a feed; a dropped
    /// sender is end of stream. The announcement is what lets a test know the
    /// previous chunk has been appended: the reader appends before it reads
    /// again, and the announcement is sent from the reader's own thread. When
    /// the reader stops and drops this, `entered` disconnects.
    struct Held {
        feeds: mpsc::Receiver<Feed>,
        entered: mpsc::Sender<()>,
        announced: bool,
    }

    impl super::PollRead for Held {
        fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.announced {
                let _ = self.entered.send(());
                self.announced = true;
            }
            let feed = self.feeds.try_recv();
            if !matches!(feed, Err(mpsc::TryRecvError::Empty)) {
                self.announced = false;
            }
            match feed {
                Err(mpsc::TryRecvError::Empty) => Err(io::ErrorKind::WouldBlock.into()),
                Err(mpsc::TryRecvError::Disconnected) => Ok(0),
                Ok(Feed::ReleaseThenInterrupt(flag)) => {
                    flag.store(true, super::Ordering::SeqCst);
                    Err(io::ErrorKind::Interrupted.into())
                }
                Ok(Feed::Bytes(chunk)) => {
                    assert!(chunk.len() <= buf.len(), "the test sends chunks that fit");
                    for (slot, byte) in buf.iter_mut().zip(&chunk) {
                        *slot = *byte;
                    }
                    Ok(chunk.len())
                }
            }
        }
    }

    fn held(limit: usize) -> (Drain, mpsc::Sender<Feed>, mpsc::Receiver<()>) {
        let (feeds, receiver) = mpsc::channel();
        let (entered, announcements) = mpsc::channel();
        let drain = Drain::start(
            Stream::Stdout,
            Held {
                feeds: receiver,
                entered,
                announced: false,
            },
            limit,
        )
        .expect("a reader thread");
        (drain, feeds, announcements)
    }

    fn start(steps: impl IntoIterator<Item = Step>, limit: usize) -> Drain {
        Drain::start(Stream::Stdout, Scripted::new(steps), limit).expect("a reader thread")
    }

    fn ended(text: &str, limited: bool) -> Captured {
        Captured {
            text: text.to_owned(),
            limited,
            ended: true,
        }
    }

    /// What `collect` returned, and how long it took to.
    type Collected = (Result<Captured, DrainError>, Duration);

    /// `collect` on its own thread, so a `collect` that does not return within
    /// `BOUND` fails the test that called this instead of hanging the harness.
    fn collect_elsewhere(
        drain: Drain,
        grace: Duration,
    ) -> super::super::worker::testing::JoinedReceiver<Collected> {
        super::super::worker::testing::JoinedReceiver::spawn(
            drain.worker.release_token(),
            move || {
                let started = Instant::now();
                let captured = drain.collect(grace);
                (captured, started.elapsed())
            },
        )
    }

    /// Whether the reader has stopped: its pipe dropped, so the announcement
    /// channel disconnected rather than announcing another read.
    fn reader_stopped(entered: &mpsc::Receiver<()>) -> bool {
        matches!(
            entered.recv_timeout(BOUND),
            Err(mpsc::RecvTimeoutError::Disconnected)
        )
    }

    #[test]
    fn collect_returns_what_the_pipe_delivered_before_end_of_stream() {
        let drain = start([text("hello "), text("world")], 1 << 20);
        let captured = drain.collect(BOUND).expect("a complete stream");
        assert_eq!(captured, ended("hello world", false));
    }

    #[test]
    fn the_limit_keeps_exactly_limit_bytes_and_reports_only_a_dropped_byte() {
        // Exactly at the limit: nothing dropped, so not limited.
        let exact = start([text("abcd"), text("efghij")], 10);
        assert_eq!(
            exact.collect(BOUND).expect("a complete stream"),
            ended("abcdefghij", false)
        );
        // One chunk straddles the limit: kept to the byte, and limited.
        let over = start([text("abcd"), text("efghijkl")], 10);
        assert_eq!(
            over.collect(BOUND).expect("a complete stream"),
            ended("abcdefghij", true)
        );
        // A limit of zero keeps nothing and reports the first byte.
        let none = start([text("a")], 0);
        assert_eq!(
            none.collect(BOUND).expect("a complete stream"),
            ended("", true)
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
            ended("beforeafter", false)
        );
    }

    #[test]
    fn one_interruption_past_the_bound_ends_the_reader_with_a_named_failure() {
        let mut steps = vec![text("before")];
        steps.extend((0..=INTERRUPTED_RETRIES).map(|_| Step::Interrupted));
        steps.push(text("never"));
        let drain = start(steps, 1 << 20);
        let error = drain.collect(BOUND).expect_err("a signal storm");
        match &error {
            DrainError::Interrupted {
                stream,
                interruptions,
            } => {
                assert_eq!(
                    (*stream, *interruptions),
                    (Stream::Stdout, INTERRUPTED_RETRIES + 1)
                );
            }
            other => panic!("a signal storm was reported as {other:?}"),
        }
        assert_eq!(
            error.to_string(),
            format!(
                "reading stdout: interrupted {} times in a row",
                INTERRUPTED_RETRIES + 1
            )
        );
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
            ended("xx", false)
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
        // The reader fails, publishes its verdict at the decision, and is
        // then held between the verdict and the end of its thread: the
        // pipe's `Drop` blocks until `release` is dropped, inside the caught
        // region, with the reader's handle on the capture still alive. A
        // `collect` that read the verdict off the thread's completion, or only
        // off a capture it owned outright, would hand the bytes over as
        // complete.
        let (release, hold) = mpsc::channel::<()>();
        let (entered, failed) = mpsc::channel();
        let mut pipe = Scripted::new([text("before"), Step::Fail(io::ErrorKind::Other)]);
        pipe.hold_drop = Some(hold);
        pipe.drop_entered = Some(entered);
        let drain = Drain::start(Stream::Stdout, pipe, 1 << 20).expect("a reader thread");
        failed
            .recv_timeout(BOUND)
            .expect("the failure was published before pipe teardown began");
        assert!(
            !drain.worker.is_finished(),
            "the test holds teardown after publication"
        );
        assert!(matches!(
            drain.capture.lock().expect("capture").verdict,
            Some(Err(DrainError::Read { .. }))
        ));
        drop(release);
        let result = collect_elsewhere(drain, Duration::ZERO);
        let outcome = result.recv_timeout(BOUND);
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

    struct CancelOnPoll {
        cancel: mpsc::Receiver<std::sync::Arc<super::AtomicBool>>,
        panic: bool,
    }

    impl super::PollRead for CancelOnPoll {
        fn try_read(&mut self, _bytes: &mut [u8]) -> io::Result<usize> {
            match self.cancel.try_recv() {
                Ok(flag) => {
                    flag.store(true, super::Ordering::SeqCst);
                    assert!(!self.panic, "finite reader panic");
                    Err(io::Error::other("finite read failure"))
                }
                Err(mpsc::TryRecvError::Empty) => Err(io::ErrorKind::WouldBlock.into()),
                Err(mpsc::TryRecvError::Disconnected) => Ok(0),
            }
        }
    }

    #[test]
    fn a_read_failure_that_races_release_is_reported() {
        let (cancel, receiver) = mpsc::channel();
        let drain = Drain::start(
            Stream::Stdout,
            CancelOnPoll {
                cancel: receiver,
                panic: false,
            },
            64,
        )
        .expect("reader");
        cancel
            .send(drain.worker.release_token())
            .expect("finite poll");
        let error = drain
            .collect(BOUND)
            .expect_err("release cannot hide a returned read error");
        assert!(matches!(error, DrainError::Read { .. }));
        assert!(error.to_string().contains("finite read failure"));
    }

    #[test]
    fn a_dropped_reader_adds_its_failure_to_the_primary_error() {
        for panic in [false, true] {
            let (cancel, receiver) = mpsc::channel();
            let drain = Drain::start(
                Stream::Stdout,
                CancelOnPoll {
                    cancel: receiver,
                    panic,
                },
                64,
            )
            .expect("reader");
            let report = drain.failure_report();
            cancel
                .send(drain.worker.release_token())
                .expect("finite poll");
            let started = Instant::now();
            while !drain.worker.is_finished() {
                assert!(started.elapsed() < BOUND, "finite reader finished");
                thread::yield_now();
            }
            drop(drain);
            let error = super::super::finish_pipe_reports::<()>(
                Err(crate::error::UpstrokeError::Agent {
                    message: "primary supervision failure".to_owned(),
                }),
                [None, Some(report), None],
            )
            .expect_err("both failures remain observable")
            .to_string();
            assert!(error.contains("primary supervision failure"));
            assert!(error.contains(if panic {
                "panicked"
            } else {
                "finite read failure"
            }));
        }
    }

    #[test]
    fn a_panic_while_the_pipe_is_dropped_is_the_verdict_and_not_a_success() {
        // End of stream, then the pipe's `Drop` panics: end of stream is
        // published only once the pipe is closed, so the panic is what the
        // reader reports.
        let mut pipe = Scripted::new([text("before")]);
        pipe.panic_on_drop = true;
        let drain = Drain::start(Stream::Stdout, pipe, 1 << 20).expect("a reader thread");
        match drain.collect(BOUND) {
            Err(DrainError::ReaderPanicked { stream }) => assert_eq!(stream, Stream::Stdout),
            other => panic!("a panic while dropping the pipe was reported as {other:?}"),
        }
    }

    #[test]
    fn collect_waits_for_the_stream_to_end_and_returns_what_arrived_by_then() {
        // The reader has polled an empty live stream before collect starts.
        // The collector must enter its wait before the test supplies output.
        let (drain, feeds, entered) = held(1 << 20);
        entered.recv_timeout(BOUND).expect("the first read");
        let (waiting, observed_wait) = mpsc::channel();
        let (resume, continue_wait) = mpsc::channel::<()>();
        let result = super::super::worker::testing::JoinedReceiver::spawn(
            drain.worker.release_token(),
            move || {
                drain.collect_with_wait(BOUND, || {
                    let _ = waiting.send(());
                    let _ = continue_wait.recv_timeout(BOUND);
                })
            },
        );
        // This signal comes from inside the pending-reader loop. A collector
        // that skips its wait cannot produce it, regardless of scheduling.
        observed_wait
            .recv_timeout(BOUND)
            .expect("collect waited for the still-open stream");
        feeds
            .send(Feed::Bytes(b"late".to_vec()))
            .expect("the reader is waiting");
        drop(feeds);
        drop(resume);
        let captured = result
            .recv_timeout(BOUND)
            .expect("collect returned")
            .expect("a complete stream");
        drop(result);
        assert_eq!(captured.bytes, b"late");
        assert!(
            captured.ended,
            "the stream ended before collection finished"
        );
        assert!(!captured.limited, "the bytes fit within the allowance");
    }

    #[test]
    fn collect_releases_a_reader_whose_writer_never_closes_once_the_grace_expires() {
        let (drain, feeds, entered) = held(1 << 20);
        entered.recv_timeout(BOUND).expect("the first read");
        feeds
            .send(Feed::Bytes(b"partial".to_vec()))
            .expect("the reader is waiting");
        // The second read is announced only after "partial" was appended.
        entered.recv_timeout(BOUND).expect("the second read");

        let grace = Duration::from_millis(100);
        let result = collect_elsewhere(drain, grace);
        // Bounded so a collect that joins without releasing the reader fails
        // this test instead of hanging the harness: the writer is closed on
        // the way out, which lets both threads finish.
        let outcome = result.recv_timeout(BOUND);
        let (captured, waited) = match outcome {
            Ok(returned) => returned,
            Err(timeout) => {
                drop(feeds);
                panic!("collect outlived its grace: {timeout:?}");
            }
        };
        assert_eq!(
            captured.expect("a partial stream is not a failure"),
            Captured {
                text: "partial".to_owned(),
                limited: false,
                ended: false,
            }
        );
        assert!(
            waited >= grace,
            "collect returned before its grace: {waited:?}"
        );

        // The joined reader has closed its endpoint while this writer is live.
        assert!(feeds.send(Feed::Bytes(b"orphan".to_vec())).is_err());
        assert!(
            reader_stopped(&entered),
            "a released reader read again instead of stopping"
        );
    }

    #[test]
    fn a_released_reader_stops_at_an_interruption_too() {
        let (drain, feeds, entered) = held(1 << 20);
        entered.recv_timeout(BOUND).expect("the first read");
        // Force cancellation between entering a poll and its Interrupted
        // result. The post-poll check must stop before another retry.
        feeds
            .send(Feed::ReleaseThenInterrupt(drain.worker.release_token()))
            .expect("live reader");
        assert!(
            reader_stopped(&entered),
            "release preceded interruption handling"
        );
        let result = collect_elsewhere(drain, Duration::ZERO);
        let outcome = result.recv_timeout(BOUND);
        drop(feeds);
        let (captured, _) = outcome.expect("collect returned");
        assert!(
            !captured.expect("a partial stream is not a failure").ended,
            "the stream was taken as ended while the writer was open"
        );
    }

    #[test]
    fn a_released_reader_cannot_report_eof_before_its_capture_is_taken() {
        let (drain, _feeds, entered) = held(1 << 20);
        entered.recv_timeout(BOUND).expect("the reader is blocked");
        // Force collect's disputed ordering: release first, then let the
        // reader finish before taking its capture. The public collect
        // call cannot pause at that scheduling point, so this test sets
        // its release flag directly. The writer remains open throughout.
        drain
            .worker
            .release_token()
            .store(true, super::Ordering::SeqCst);
        assert!(reader_stopped(&entered), "the released reader stopped");
        let started = Instant::now();
        while !drain.worker.is_finished() {
            assert!(started.elapsed() < BOUND, "the reader published its result");
            thread::yield_now();
        }
        let captured = drain
            .collect(Duration::ZERO)
            .expect("release returns the available capture");
        assert!(
            !captured.ended,
            "release was reported as EOF while the writer remained open"
        );
        assert!(captured.text.is_empty(), "released bytes were not retained");
    }

    #[test]
    fn dropping_a_drain_without_collecting_it_releases_its_reader() {
        // The supervisor's error exits drop a drain they never collect; the
        // reader must not be left draining for the orphan's lifetime.
        let (drain, feeds, entered) = held(1 << 20);
        entered.recv_timeout(BOUND).expect("the first read");
        let dropped = super::super::worker::testing::JoinedReceiver::spawn(
            drain.worker.release_token(),
            move || drop(drain),
        );
        let outcome = dropped.recv_timeout(BOUND);
        drop(feeds);
        outcome.expect("drop settled the nonblocking worker with its writer still live");
        assert!(
            reader_stopped(&entered),
            "a dropped drain's reader read again instead of stopping"
        );
    }

    #[test]
    fn limit_exceeded_is_visible_while_the_reader_still_runs() {
        let (drain, feeds, entered) = held(4);
        entered.recv_timeout(BOUND).expect("the first read");
        feeds
            .send(Feed::Bytes(b"12345".to_vec()))
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

        drop(feeds);
        let Some(drain) = stdout else {
            panic!("the drain was placed in the option two statements ago");
        };
        assert_eq!(
            drain.collect(BOUND).expect("a complete stream"),
            ended("1234", true)
        );
    }

    #[test]
    fn bytes_that_are_not_utf8_are_decoded_lossily_rather_than_dropped() {
        let drain = start([Step::Bytes(vec![b'a', 0xff, b'b'])], 1 << 20);
        assert_eq!(
            drain.collect(BOUND).expect("a complete stream"),
            ended("a\u{FFFD}b", false)
        );
    }

    #[test]
    fn byte_collection_preserves_non_utf8_and_limits_the_original_bytes() {
        let bytes = vec![b'a', 0xff, 0, 0xc3, 0xa9];
        let complete = start([Step::Bytes(bytes.clone())], bytes.len())
            .collect_bytes(BOUND)
            .expect("the original bytes");
        assert_eq!(complete.bytes, bytes);
        assert!(!complete.limited);
        assert!(complete.ended);

        let limited = start([Step::Bytes(bytes)], 4)
            .collect_bytes(BOUND)
            .expect("a bounded byte capture");
        assert_eq!(limited.bytes, [b'a', 0xff, 0, 0xc3]);
        assert!(limited.limited, "the final byte exceeded the allowance");
        assert!(limited.ended, "the limit does not replace EOF");
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
