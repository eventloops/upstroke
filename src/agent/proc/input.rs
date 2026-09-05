//! Bounded stdin delivery with a joined nonblocking pipe worker.
//!
//! The worker owns its endpoint and input bytes. Supervisor and worker share
//! a verdict because collection may inspect progress before the worker ends.
//! No mutex spans a pipe operation. Failure is published at its decision;
//! success follows pipe close, and write/teardown panics become typed failures.
//! WouldBlock uses finite parking, with release plus unpark before every join.
//!
//! BrokenPipe means the child declined remaining input. Other write failures
//! and excessive consecutive interruptions are supervision errors. At the
//! post-exit grace, release ends further attempts and collection joins before
//! returning. Delivery of remaining input is best-effort after child exit.
//! Dropping an uncollected feeder joins and records its failure for the
//! invocation's retained FailureReport, including early supervisor returns.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::io::{self, ErrorKind};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::pipe_io::PollWrite;
use super::worker::{FailureReport, POLL_INTERVAL, Worker};
use thiserror::Error;

/// Consecutive interruptions retried before a signal storm is a failure.
const INTERRUPTED_RETRIES: usize = 64;

/// Why stdin supervision failed, with the operation retained in the error.
#[derive(Debug, Error)]
pub(super) enum FeedError {
    #[error("starting the stdin feeder thread: {0}")]
    Start(#[source] io::Error),
    #[error("writing stdin: {0}")]
    Write(#[source] io::Error),
    #[error("writing stdin: interrupted {interruptions} times in a row")]
    Interrupted { interruptions: usize },
    #[error("the stdin writer reported {reported} bytes from a {remaining}-byte buffer")]
    OverlongWrite { reported: usize, remaining: usize },
    #[error("the stdin feeder thread panicked")]
    WriterPanicked,
}

type Verdict = Mutex<Option<Result<(), FeedError>>>;

/// Proof that the loop already published its failure before pipe teardown.
struct Published;

fn publish_failure(verdict: &Verdict, error: FeedError) -> Published {
    // The worker is the only publisher. Poison means it panicked while
    // publishing, and collection still needs to observe its defined failure.
    *verdict.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err(error));
    Published
}

/// One stdin writer whose post-exit wait is bounded even if a descendant kept
/// the read end open. Dropping this value releases the worker too.
pub(super) struct Feeder {
    verdict: Arc<Verdict>,
    worker: Worker,
    report: FailureReport,
}

fn write_input<W: PollWrite>(
    pipe: &mut W,
    mut remaining: &[u8],
    verdict: &Verdict,
    released: &AtomicBool,
) -> Result<(), Published> {
    let mut interruptions = 0;
    while !remaining.is_empty() {
        if released.load(Ordering::SeqCst) {
            return Ok(());
        }
        let outcome = pipe.try_write(remaining);
        let cancelled = released.load(Ordering::SeqCst);
        match outcome {
            Err(error)
                if !matches!(
                    error.kind(),
                    ErrorKind::BrokenPipe | ErrorKind::Interrupted | ErrorKind::WouldBlock
                ) =>
            {
                return Err(publish_failure(verdict, FeedError::Write(error)));
            }
            Ok(0) => {
                return Err(publish_failure(
                    verdict,
                    FeedError::Write(io::Error::from(ErrorKind::WriteZero)),
                ));
            }
            Ok(written) if written > remaining.len() => {
                return Err(publish_failure(
                    verdict,
                    FeedError::OverlongWrite {
                        reported: written,
                        remaining: remaining.len(),
                    },
                ));
            }
            _ if cancelled => return Ok(()),
            Ok(written) => {
                let Some(rest) = remaining.get(written..) else {
                    return Err(publish_failure(
                        verdict,
                        FeedError::OverlongWrite {
                            reported: written,
                            remaining: remaining.len(),
                        },
                    ));
                };
                remaining = rest;
                interruptions = 0;
            }
            Err(error) if error.kind() == ErrorKind::BrokenPipe => return Ok(()),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                interruptions = 0;
                thread::park_timeout(POLL_INTERVAL);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {
                interruptions += 1;
                if interruptions > INTERRUPTED_RETRIES {
                    return Err(publish_failure(
                        verdict,
                        FeedError::Interrupted { interruptions },
                    ));
                }
            }
            Err(error) => return Err(publish_failure(verdict, FeedError::Write(error))),
        }
    }
    Ok(())
}

impl Feeder {
    /// Move the input and pipe to a writer thread. A broken pipe accepts the
    /// child's refusal of unread input; other published failures are returned
    /// by [`Self::collect`].
    ///
    /// # Errors
    ///
    /// [`FeedError::Start`] when the OS refuses the thread. The closure and
    /// pipe are dropped on that path, so the caller can settle the child tree.
    pub(super) fn start<W: PollWrite>(pipe: W, bytes: Vec<u8>) -> Result<Self, FeedError> {
        let verdict = Arc::new(Mutex::new(None));
        let worker_verdict = Arc::clone(&verdict);
        let worker = Worker::spawn("stdin-feeder", move |released| {
            // Keep the shared verdict outside the caught region so it
            // survives both a write panic and a panic closing the pipe.
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let mut pipe = pipe;
                let result = write_input(&mut pipe, &bytes, &worker_verdict, released);
                drop(pipe);
                result
            }));
            match outcome {
                Ok(Ok(())) => {
                    worker_verdict
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .get_or_insert(Ok(()));
                }
                Ok(Err(Published)) => {}
                Err(_payload) => {
                    worker_verdict
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .get_or_insert(Err(FeedError::WriterPanicked));
                }
            }
        })
        .map_err(FeedError::Start)?;
        Ok(Self {
            verdict,
            worker,
            report: FailureReport::default(),
        })
    }

    /// Retain the invocation's observer before another pipe setup can fail.
    pub(super) fn failure_report(&self) -> FailureReport {
        self.report.clone()
    }

    /// Wait for a verdict up to the post-exit grace, then release the writer.
    /// A still-pending write belongs to an escaped reader after the child has
    /// exited; delivery of that remaining input is best-effort. Any verdict
    /// published before collection is inspected even during pipe teardown.
    ///
    /// # Errors
    ///
    /// A published write failure other than `BrokenPipe`, excessive consecutive
    /// interruptions, an invalid write count, or a worker panic. A known
    /// failure is never discarded merely because the thread is not finished.
    pub(super) fn collect(mut self, grace: Duration) -> Result<(), FeedError> {
        let started = Instant::now();
        loop {
            if self
                .verdict
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
                || started.elapsed() >= grace
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let panicked = self.worker.settle();
        let outcome = self
            .verdict
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match (outcome, panicked) {
            (Some(Err(error)), _) => Err(error),
            (_, true) => Err(FeedError::WriterPanicked),
            (Some(Ok(())) | None, false) => Ok(()),
        }
    }
}

impl Drop for Feeder {
    fn drop(&mut self) {
        let panicked = self.worker.settle();
        let error = self
            .verdict
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .and_then(Result::err);
        match (error, panicked) {
            (Some(error), true) => self.report.record(format!(
                "{error}; stdin worker also panicked during settlement"
            )),
            (Some(error), false) => self.report.record(error),
            (None, true) => self.report.record(FeedError::WriterPanicked),
            (None, false) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{FeedError, Feeder, INTERRUPTED_RETRIES};

    const BOUND: Duration = Duration::from_secs(10);

    enum Step {
        Bytes(usize),
        Fail(io::ErrorKind),
        Zero,
        Overlong,
        Panic,
    }

    struct Scripted {
        steps: VecDeque<Step>,
        written: Vec<u8>,
        report: mpsc::Sender<Vec<u8>>,
        drop_entered: Option<mpsc::Sender<()>>,
        hold_drop: Option<mpsc::Receiver<()>>,
        panic_on_drop: bool,
    }

    impl Scripted {
        fn new(steps: impl IntoIterator<Item = Step>) -> (Self, mpsc::Receiver<Vec<u8>>) {
            let (report, bytes) = mpsc::channel();
            (
                Self {
                    steps: steps.into_iter().collect(),
                    written: Vec::new(),
                    report,
                    drop_entered: None,
                    hold_drop: None,
                    panic_on_drop: false,
                },
                bytes,
            )
        }
    }

    impl super::PollWrite for Scripted {
        fn try_write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            match self.steps.pop_front().unwrap_or(Step::Bytes(bytes.len())) {
                Step::Bytes(count) => {
                    let count = count.min(bytes.len());
                    self.written.extend(bytes.iter().take(count));
                    Ok(count)
                }
                Step::Fail(kind) => Err(io::Error::from(kind)),
                Step::Zero => Ok(0),
                Step::Overlong => Ok(bytes.len() + 1),
                Step::Panic => panic!("scripted stdin write panic"),
            }
        }
    }

    impl Drop for Scripted {
        fn drop(&mut self) {
            let _ = self.report.send(std::mem::take(&mut self.written));
            if let Some(entered) = &self.drop_entered {
                let _ = entered.send(());
            }
            if let Some(hold) = &self.hold_drop {
                let _ = hold.recv_timeout(BOUND);
            }
            assert!(!self.panic_on_drop, "scripted stdin teardown panic");
        }
    }

    fn collect_elsewhere(
        feeder: Feeder,
        grace: Duration,
    ) -> super::super::worker::testing::JoinedReceiver<Result<(), FeedError>> {
        super::super::worker::testing::JoinedReceiver::spawn(
            feeder.worker.release_token(),
            move || feeder.collect(grace),
        )
    }

    #[test]
    fn partial_writes_and_bounded_interruptions_preserve_every_input_byte() {
        let mut steps = vec![Step::Bytes(1)];
        for _ in 0..2 {
            steps.extend((0..INTERRUPTED_RETRIES).map(|_| Step::Fail(io::ErrorKind::Interrupted)));
            steps.push(Step::Bytes(1));
        }
        let (pipe, written) = Scripted::new(steps);
        let bytes = vec![0, 0xff, b'a', b'b'];
        let feeder = Feeder::start(pipe, bytes.clone()).expect("a writer thread");
        feeder.collect(BOUND).expect("all input was written");
        assert_eq!(
            written.recv_timeout(BOUND).expect("the delivered bytes"),
            bytes
        );
    }

    #[test]
    fn only_broken_pipe_is_accepted_as_the_child_declining_input() {
        for kind in [
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
        ] {
            let (pipe, _written) = Scripted::new([Step::Fail(kind)]);
            let feeder = Feeder::start(pipe, vec![b'x']).expect("a writer thread");
            let outcome = feeder.collect(BOUND);
            if kind == io::ErrorKind::BrokenPipe {
                assert!(outcome.is_ok(), "the child declined input: {outcome:?}");
            } else {
                match outcome {
                    Err(FeedError::Write(error)) => assert_eq!(error.kind(), kind),
                    other => panic!("write failure {kind:?} became {other:?}"),
                }
            }
        }
    }

    #[test]
    fn zero_and_overlong_writes_return_errors_without_retrying_or_panicking() {
        let (zero, _written) = Scripted::new([Step::Zero]);
        let feeder = Feeder::start(zero, vec![b'x']).expect("a writer thread");
        match feeder.collect(BOUND) {
            Err(FeedError::Write(error)) => assert_eq!(error.kind(), io::ErrorKind::WriteZero),
            other => panic!("zero progress became {other:?}"),
        }

        let (overlong, _written) = Scripted::new([Step::Overlong]);
        let feeder = Feeder::start(overlong, vec![b'x']).expect("a writer thread");
        assert!(matches!(
            feeder.collect(BOUND),
            Err(FeedError::OverlongWrite {
                reported: 2,
                remaining: 1
            })
        ));
    }

    #[test]
    fn stdin_interruption_retries_stop_after_the_documented_bound() {
        let (pipe, _written) = Scripted::new(
            (0..=INTERRUPTED_RETRIES).map(|_| Step::Fail(io::ErrorKind::Interrupted)),
        );
        let feeder = Feeder::start(pipe, vec![b'x']).expect("a writer thread");
        match feeder.collect(BOUND) {
            Err(FeedError::Interrupted { interruptions }) => {
                assert_eq!(interruptions, INTERRUPTED_RETRIES + 1);
            }
            other => panic!("a signal storm became {other:?}"),
        }
    }

    #[test]
    fn write_and_pipe_teardown_panics_are_supervision_errors() {
        for panic_on_drop in [false, true] {
            let steps = if panic_on_drop {
                vec![]
            } else {
                vec![Step::Panic]
            };
            let (mut pipe, _written) = Scripted::new(steps);
            pipe.panic_on_drop = panic_on_drop;
            let feeder = Feeder::start(pipe, vec![b'x']).expect("a writer thread");
            assert!(matches!(
                feeder.collect(BOUND),
                Err(FeedError::WriterPanicked)
            ));
        }
    }

    #[test]
    fn a_stdin_failure_is_observed_while_pipe_teardown_is_still_pending() {
        let (release, hold) = mpsc::channel();
        let (entered, failed) = mpsc::channel();
        let (mut pipe, _written) = Scripted::new([Step::Fail(io::ErrorKind::Other)]);
        pipe.drop_entered = Some(entered);
        pipe.hold_drop = Some(hold);
        let feeder = Feeder::start(pipe, vec![b'x']).expect("a writer thread");
        failed
            .recv_timeout(BOUND)
            .expect("the published failure precedes teardown");
        assert!(!feeder.worker.is_finished(), "the test holds teardown");
        assert!(matches!(
            feeder.verdict.lock().expect("verdict").as_ref(),
            Some(Err(FeedError::Write(_)))
        ));
        drop(release);
        let result = collect_elsewhere(feeder, Duration::ZERO);
        let outcome = result.recv_timeout(BOUND);
        match outcome.expect("collection did not join the held writer") {
            Err(FeedError::Write(error)) => assert_eq!(error.kind(), io::ErrorKind::Other),
            other => panic!("the published failure became {other:?}"),
        }
    }

    struct Held {
        wakes: mpsc::Receiver<io::Result<usize>>,
        entered: mpsc::Sender<()>,
        announced: bool,
    }

    impl super::PollWrite for Held {
        fn try_write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            if !self.announced {
                let _ = self.entered.send(());
                self.announced = true;
            }
            match self.wakes.try_recv() {
                Ok(answer) => {
                    self.announced = false;
                    answer
                }
                Err(mpsc::TryRecvError::Empty) => Err(io::ErrorKind::WouldBlock.into()),
                Err(mpsc::TryRecvError::Disconnected) => Err(io::ErrorKind::BrokenPipe.into()),
            }
        }
    }

    fn held() -> (Feeder, mpsc::Sender<io::Result<usize>>, mpsc::Receiver<()>) {
        let (wakes, receiver) = mpsc::channel();
        let (entered, calls) = mpsc::channel();
        let feeder = Feeder::start(
            Held {
                wakes: receiver,
                entered,
                announced: false,
            },
            vec![b'x'; 4],
        )
        .expect("a writer thread");
        (feeder, wakes, calls)
    }

    #[test]
    fn releasing_stdin_at_the_grace_stops_the_next_write_or_interruption() {
        for wake in [Ok(1), Err(io::Error::from(io::ErrorKind::Interrupted))] {
            let (feeder, wakes, entered) = held();
            entered.recv_timeout(BOUND).expect("the writer is blocked");
            let collected = collect_elsewhere(feeder, Duration::ZERO);
            let outcome = collected.recv_timeout(BOUND);
            let stopped = wakes.send(wake).is_err();
            drop(wakes);
            outcome
                .expect("the grace bounded collection")
                .expect("remaining input after child exit is best-effort");
            assert!(stopped, "the writer was joined before collection returned");
            assert!(
                matches!(
                    entered.recv_timeout(BOUND),
                    Err(mpsc::RecvTimeoutError::Disconnected)
                ),
                "a released writer attempted another write"
            );
        }
    }

    #[test]
    fn dropping_an_uncollected_feeder_releases_its_worker() {
        let (feeder, wakes, entered) = held();
        entered.recv_timeout(BOUND).expect("the writer is blocked");
        let dropped = super::super::worker::testing::JoinedReceiver::spawn(
            feeder.worker.release_token(),
            move || drop(feeder),
        );
        let outcome = dropped.recv_timeout(BOUND);
        drop(wakes);
        outcome.expect("drop joined the writer with its reader still live");
        assert!(
            matches!(
                entered.recv_timeout(BOUND),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "an abandoned writer attempted another write"
        );
    }

    struct CancelOnPoll {
        cancel: mpsc::Receiver<std::sync::Arc<super::AtomicBool>>,
    }

    impl super::PollWrite for CancelOnPoll {
        fn try_write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            match self.cancel.try_recv() {
                Ok(flag) => {
                    flag.store(true, super::Ordering::SeqCst);
                    Err(io::Error::other("finite stdin failure"))
                }
                Err(mpsc::TryRecvError::Empty) => Err(io::ErrorKind::WouldBlock.into()),
                Err(mpsc::TryRecvError::Disconnected) => Err(io::ErrorKind::BrokenPipe.into()),
            }
        }
    }

    #[test]
    fn a_write_failure_that_races_release_is_reported() {
        let (cancel, receiver) = mpsc::channel();
        let feeder = Feeder::start(CancelOnPoll { cancel: receiver }, vec![b'x']).expect("writer");
        cancel
            .send(feeder.worker.release_token())
            .expect("finite poll");
        let error = feeder
            .collect(BOUND)
            .expect_err("release cannot hide a returned write error");
        assert!(matches!(error, FeedError::Write(_)));
        assert!(error.to_string().contains("finite stdin failure"));
    }

    #[test]
    fn a_dropped_feeder_adds_its_failure_to_the_primary_error() {
        let (cancel, receiver) = mpsc::channel();
        let feeder = Feeder::start(CancelOnPoll { cancel: receiver }, vec![b'x']).expect("writer");
        let report = feeder.failure_report();
        cancel
            .send(feeder.worker.release_token())
            .expect("finite poll");
        let started = std::time::Instant::now();
        while !feeder.worker.is_finished() {
            assert!(started.elapsed() < BOUND, "finite writer finished");
            std::thread::yield_now();
        }
        drop(feeder);
        let error = super::super::finish_pipe_reports::<()>(
            Err(crate::error::UpstrokeError::Agent {
                message: "primary supervision failure".to_owned(),
            }),
            [Some(report), None, None],
        )
        .expect_err("both failures remain observable")
        .to_string();
        assert!(
            error.contains("primary supervision failure") && error.contains("finite stdin failure")
        );
    }
}
