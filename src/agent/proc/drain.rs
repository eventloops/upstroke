//! Extended notes: `docs/internals/agent/proc/drain.md`

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

pub(super) const INTERRUPTED_RETRIES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
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

#[derive(Debug, Error)]
pub(super) enum DrainError {
    #[error("starting the {stream} reader thread: {source}")]
    Start {
        stream: Stream,
        source: std::io::Error,
    },
    #[error("reading {stream}: {source}")]
    Read {
        stream: Stream,
        source: std::io::Error,
    },
    #[error("reading {stream}: interrupted {interruptions} times in a row")]
    Interrupted {
        stream: Stream,
        interruptions: usize,
    },
    #[error("the {stream} reader was told {reported} bytes filled a {capacity}-byte buffer")]
    OverlongRead {
        stream: Stream,
        reported: usize,
        capacity: usize,
    },
    #[error("the {stream} reader thread panicked")]
    ReaderPanicked { stream: Stream },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Captured {
    pub(super) text: String,
    pub(super) limited: bool,
    pub(super) ended: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CapturedBytes {
    pub(super) bytes: Vec<u8>,
    pub(super) limited: bool,
    pub(super) ended: bool,
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    verdict: Option<Result<ReadEnd, DrainError>>,
}

enum ReadEnd {
    Eof,
    Released,
}

struct Published;

fn publish_failure(capture: &Mutex<Capture>, error: DrainError) -> Published {
    capture
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .verdict = Some(Err(error));
    Published
}

pub(super) struct Drain {
    stream: Stream,
    capture: Arc<Mutex<Capture>>,
    limited: Arc<AtomicBool>,
    worker: Worker,
    report: FailureReport,
}

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
            let after_unwind = Arc::clone(&capture);
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

    pub(super) fn limit_exceeded(&self) -> bool {
        self.limited.load(Ordering::SeqCst)
    }

    pub(super) fn failure_report(&self) -> FailureReport {
        self.report.clone()
    }

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

    pub(super) fn collect_bytes(self, grace: Duration) -> Result<CapturedBytes, DrainError> {
        self.collect_with_wait(grace, || thread::sleep(Duration::from_millis(20)))
    }

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
        let torn_down = self
            .worker
            .settle()
            .then_some(DrainError::ReaderPanicked { stream });
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

fn publish_panic(capture: &Mutex<Capture>, stream: Stream) {
    capture
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .verdict
        .get_or_insert(Err(DrainError::ReaderPanicked { stream }));
}

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

    const BOUND: Duration = Duration::from_secs(10);

    enum Step {
        Bytes(Vec<u8>),
        Interrupted,
        Fail(io::ErrorKind),
        Overlong,
        Panic,
    }

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

    enum Feed {
        Bytes(Vec<u8>),
        ReleaseThenInterrupt(std::sync::Arc<super::AtomicBool>),
    }

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

    type Collected = (Result<Captured, DrainError>, Duration);

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
        let exact = start([text("abcd"), text("efghij")], 10);
        assert_eq!(
            exact.collect(BOUND).expect("a complete stream"),
            ended("abcdefghij", false)
        );
        let over = start([text("abcd"), text("efghijkl")], 10);
        assert_eq!(
            over.collect(BOUND).expect("a complete stream"),
            ended("abcdefghij", true)
        );
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
        entered.recv_timeout(BOUND).expect("the second read");

        let grace = Duration::from_millis(100);
        let result = collect_elsewhere(drain, grace);
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
