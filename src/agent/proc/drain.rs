//! The bounded pipe drain: one reader thread per pipe, accumulating into a
//! buffer the supervisor can snapshot without joining it.
//!
//! Split out of [`super`] as its own module because it is the one part of the
//! funnel that supervises a *thread* rather than a process, and nothing in the
//! parent touches its fields — only `start`, `limit_exceeded` and `collect`.
//!
//! **The limits are the caller's, not this module's.** `start` takes the
//! per-stream byte cap and `collect` takes the post-exit grace as arguments, so
//! `DRAIN_GRACE_EXIT`, `DRAIN_GRACE_KILL` and `OUTPUT_LIMIT_BYTES` stay in
//! `super` beside the policy that chose them. This module has no constants.
//!
//! Two properties are load-bearing and neither is visible from the signatures:
//!
//! * **Reading continues after the cap; only retention stops.** `super`'s
//!   `OUTPUT_LIMIT_BYTES` states the reason — "readers continue draining after
//!   this point so the child cannot block on a full pipe while the supervisor
//!   notices and terminates its tree". A reader that returned at the limit
//!   would leave a full pipe buffer and a child blocked in `write`, which is
//!   the deadlock the parent module exists to prevent.
//! * **Collection is bounded, never an unconditional join.** Any process that
//!   inherited the write end can outlive the direct child, so the thread is
//!   waited on for `grace` and then abandoned with its buffer snapshotted. An
//!   abandoned reader owns its handle and exits when the last writer closes.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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

    pub(super) fn limit_exceeded(&self) -> bool {
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
