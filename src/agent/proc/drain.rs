//! Extended notes: `docs/internals/agent/proc/drain.md`

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
