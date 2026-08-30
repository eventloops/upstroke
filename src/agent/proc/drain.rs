//! The bounded pipe drain: read a child's stdout and stderr into shared
//! buffers that can be snapshotted without joining the reader threads.
//!
//! This is the half of process supervision that has nothing to do with
//! processes. It reads bytes from something that implements [`Read`], keeps at
//! most a caller-supplied number of them, records whether it had to drop any,
//! and hands back what arrived within a caller-supplied grace. Nothing here
//! spawns, signals, waits on or contains a child; the supervisor in the parent
//! module owns all of that and calls in here twice per run.
//!
//! **Why the buffer is shared and the thread is not joined.** A pipe read
//! blocks until the last *write* handle closes, and on both platforms a handle
//! can outlive the process the funnel started: a Windows grandchild that
//! inherited it, or a Unix orphan that escaped its group. Joining such a reader
//! is waiting on a process nobody owns. So the reader accumulates into an
//! `Arc<Mutex<Vec<u8>>>`, and [`Drain::collect`] waits only for the grace it
//! was given before snapshotting the buffer and walking away. An abandoned
//! reader owns its own handle and exits when the last writer closes.
//!
//! **Why the limit is enforced by the reader and not by the supervisor.** The
//! supervisor notices a run has exceeded its output allowance by polling
//! [`drain_limit_exceeded`], and it can only do that if the readers have kept
//! reading — a reader that stopped at the limit would fill the pipe buffer and
//! block the child, which is the deadlock this whole module exists to avoid.
//! So the readers drain past the limit forever and simply stop *retaining*.
//! Bytes above the allowance are counted as dropped and discarded.
//
// PROCESS FUNNEL child module. `src/agent/proc.rs` is in the funnel section of
// `effects/allowlist.toml` and opens with an inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`.
// A Rust lint level is scoped by the MODULE TREE and not by the file, so this
// file would inherit all three in silence -- that is `PR6-LANEF-004`, measured
// twice in the Container subtree and answered for the Process funnel's first
// out-of-line child in `src/agent/proc/test_support/readiness.rs`.
//
// This module reaches no denied primitive at all: pipes arrive as an opaque
// `R: Read`, so it names no `std::process::Command`, opens no file and prints
// nothing. It therefore states the STRONGEST posture rather than the inherited
// one, and needs no `effects/allowlist.toml` row of its own -- a denial is not
// an allowance and nothing here has to be reviewed as an exception.
// `runner::container::tests::every_child_module_of_the_container_funnel_states_\
// its_own_lint_level` is the census that requires this attribute to exist, and
// it derives its domain from the funnel list rather than from a written-out set
// of files, which is why this file joined that domain by being created.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long to keep draining pipes after the process is gone. Normally EOF is
/// immediate; the grace only caps the pathological case of an orphaned
/// grandchild still holding a write handle.
pub(super) const DRAIN_GRACE_EXIT: Duration = Duration::from_secs(2);
pub(super) const DRAIN_GRACE_KILL: Duration = Duration::from_millis(500);
/// Per stream. Readers continue draining after this point so the child cannot
/// block on a full pipe while the supervisor notices and terminates its tree.
pub(super) const OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// A pipe reader whose buffer can be snapshotted without joining the thread,
/// so an orphan holding the write end can never stall the supervisor.
pub(super) struct Drain {
    buf: Arc<Mutex<Vec<u8>>>,
    limited: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

impl Drain {
    pub(super) fn start<R: Read + Send + 'static>(mut pipe: R, limit: usize) -> Self {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&buf);
        let limited = Arc::new(AtomicBool::new(false));
        let reader_limited = Arc::clone(&limited);
        let handle = thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut guard = match writer.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        let remaining = limit.saturating_sub(guard.len());
                        let retained = remaining.min(n);
                        guard.extend_from_slice(&chunk[..retained]);
                        if retained < n {
                            reader_limited.store(true, Ordering::SeqCst);
                        }
                    }
                }
            }
        });
        Self {
            buf,
            limited,
            handle,
        }
    }

    fn limit_exceeded(&self) -> bool {
        self.limited.load(Ordering::SeqCst)
    }

    /// Wait up to `grace` for EOF, then snapshot whatever arrived. A reader
    /// abandoned here exits on its own when the last write handle closes.
    pub(super) fn collect(self, grace: Duration) -> (String, bool) {
        let deadline = Instant::now() + grace;
        while !self.handle.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if self.handle.is_finished() {
            let _ = self.handle.join();
        }
        let snapshot = match self.buf.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        (
            String::from_utf8_lossy(&snapshot).into_owned(),
            self.limited.load(Ordering::SeqCst),
        )
    }
}

pub(super) fn drain_limit_exceeded(stdout: &Option<Drain>, stderr: &Option<Drain>) -> bool {
    stdout.as_ref().is_some_and(Drain::limit_exceeded)
        || stderr.as_ref().is_some_and(Drain::limit_exceeded)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// A pipe that yields what it is sent and reaches EOF only when every
    /// sender is gone.
    ///
    /// The in-process stand-in for the case this module exists for: a write
    /// handle still held by something the funnel does not own, so the reader
    /// blocks and no amount of waiting on it will return. `std::io::pipe` would
    /// say it more directly and is Rust 1.87; this crate's MSRV is 1.85 and CI
    /// pins it.
    struct HeldOpen {
        rx: std::sync::mpsc::Receiver<Vec<u8>>,
        pending: Vec<u8>,
    }

    impl Read for HeldOpen {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.pending.is_empty() {
                let Ok(next) = self.rx.recv() else {
                    return Ok(0);
                };
                self.pending = next;
            }
            let taken = out.len().min(self.pending.len());
            out[..taken].copy_from_slice(&self.pending[..taken]);
            self.pending = self.pending.split_off(taken);
            Ok(taken)
        }
    }

    fn held_open() -> (std::sync::mpsc::Sender<Vec<u8>>, HeldOpen) {
        let (tx, rx) = std::sync::mpsc::channel();
        (
            tx,
            HeldOpen {
                rx,
                pending: Vec::new(),
            },
        )
    }

    /// The bound is the caller's, not this module's.
    ///
    /// `OUTPUT_LIMIT_BYTES` is what the funnel's public entry passes; every
    /// primitive here takes the allowance as an argument, and a suite that only
    /// ever drove the default would not notice a reader that ignored the
    /// argument and used the constant instead. The row at the allowance exactly
    /// is the off-by-one: retaining all of it is not an overrun.
    #[test]
    fn a_drain_retains_the_callers_allowance_and_reports_the_rest_dropped() {
        for (limit, written, retained, limited) in [
            (0_usize, 4_u64, 0_usize, true),
            (3, 4, 3, true),
            (4, 4, 4, false),
            (16, 4, 4, false),
            (16, 0, 0, false),
        ] {
            let drain = Drain::start(std::io::repeat(b'x').take(written), limit);
            let (text, reported) = drain.collect(Duration::from_secs(5));
            assert_eq!(
                (text.len(), reported),
                (retained, limited),
                "limit {limit} over {written} bytes"
            );
        }
    }

    /// A reader that counts what it hands over.
    ///
    /// Without it a drain that stopped at its allowance is indistinguishable
    /// from one that drained past it: both retain exactly the allowance and
    /// both report the overrun. What separates them is how many bytes left the
    /// pipe, so that is what is measured.
    struct Counted<R> {
        inner: R,
        consumed: Arc<AtomicUsize>,
    }

    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let taken = self.inner.read(out)?;
            self.consumed.fetch_add(taken, Ordering::SeqCst);
            Ok(taken)
        }
    }

    /// Above the allowance the reader keeps *reading* and stops *retaining*.
    ///
    /// This is the property the design turns on: a reader that returned at the
    /// limit would leave the pipe full and the child blocked on a write the
    /// supervisor is waiting to outlive. Driven over more than one 8 KiB chunk,
    /// because a single-chunk fixture cannot tell the two apart, and the
    /// overrun is not a multiple of the chunk.
    #[test]
    fn a_drain_past_its_allowance_reads_to_the_end_of_the_pipe() {
        let written = 8192 * 3 + 17;
        let consumed = Arc::new(AtomicUsize::new(0));
        let drain = Drain::start(
            Counted {
                inner: std::io::repeat(b'y').take(u64::try_from(written).expect("a small fixture")),
                consumed: Arc::clone(&consumed),
            },
            100,
        );
        let (text, limited) = drain.collect(Duration::from_secs(10));
        assert_eq!(text.len(), 100, "more than the allowance was retained");
        assert!(limited, "the dropped bytes were not reported");
        assert_eq!(
            consumed.load(Ordering::SeqCst),
            written,
            "the reader stopped at its allowance instead of draining to EOF, which is the \
             full-pipe deadlock this module exists to avoid"
        );
    }

    /// The overrun is observable *before* the snapshot, which is what lets the
    /// supervisor terminate a tree while its pipes are still open.
    ///
    /// [`drain_limit_exceeded`] is polled from the supervision loop, so it has
    /// to answer while the reader is still running. Both arms are driven and
    /// the empty case is asserted, so a predicate that answered `true`
    /// unconditionally could not pass.
    #[test]
    fn the_supervisor_sees_an_overrun_while_the_pipe_is_still_open() {
        assert!(!drain_limit_exceeded(&None, &None));
        for stderr_side in [false, true] {
            let (tx, pipe) = held_open();
            let drain = Some(Drain::start(pipe, 4));
            let (stdout, stderr) = if stderr_side {
                (None, drain)
            } else {
                (drain, None)
            };
            tx.send(b"0123456789".to_vec())
                .expect("the reader is running");
            let mut polls = 0;
            while !drain_limit_exceeded(&stdout, &stderr) && polls < 500 {
                thread::sleep(Duration::from_millis(10));
                polls += 1;
            }
            assert!(
                drain_limit_exceeded(&stdout, &stderr),
                "the overrun was never reported while the pipe was open \
                 (stderr_side={stderr_side})"
            );
            // The writer is still open, so this really is the mid-run answer.
            tx.send(b"more".to_vec())
                .expect("the reader is still running");
            drop(tx);
        }
    }

    /// A reader still holding an open pipe is abandoned at the grace, not
    /// joined: the supervisor returns what arrived rather than waiting on a
    /// handle nobody owns.
    ///
    /// The claim is structural, not a stopwatch. The test *terminating* is what
    /// says `collect` did not join — a join here would never return — and the
    /// send afterwards is what says the reader was left running rather than
    /// having quietly reached EOF, which would make the fixture prove nothing.
    #[test]
    fn collect_returns_at_the_grace_with_a_writer_still_open() {
        let (tx, pipe) = held_open();
        let drain = Drain::start(pipe, OUTPUT_LIMIT_BYTES);
        // Trailing whitespace and a byte no encoding produces: what a child
        // wrote is what is returned, `U+FFFD` for the invalid byte and not one
        // byte else. A snapshot that tidied its edges would lose a newline an
        // agent's protocol depends on.
        tx.send(b"partial \n\xff \n".to_vec())
            .expect("the reader is running");
        let (text, limited) = drain.collect(Duration::from_millis(500));
        assert_eq!(text, "partial \n\u{FFFD} \n");
        assert!(!limited);
        tx.send(b"more".to_vec())
            .expect("the reader thread was joined or gone, so nothing held the pipe open");
        drop(tx);
    }

    /// The graces the funnel passes are two, and they are ordered.
    ///
    /// A killed tree is given less than one that exited, because there is
    /// nothing left to wait for. Pinned as values as well as ordered: a repair
    /// that made both the same would keep the ordering claim true by collapsing
    /// the distinction it is about.
    #[test]
    fn the_two_graces_the_funnel_passes_are_distinct_and_ordered() {
        assert!(DRAIN_GRACE_KILL < DRAIN_GRACE_EXIT);
        assert_eq!(DRAIN_GRACE_EXIT, Duration::from_secs(2));
        assert_eq!(DRAIN_GRACE_KILL, Duration::from_millis(500));
        assert_eq!(OUTPUT_LIMIT_BYTES, 16 * 1024 * 1024);
    }
}
