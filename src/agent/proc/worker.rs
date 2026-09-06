//! Extended notes: `docs/internals/agent/proc/worker.md`

//! Cancellation and joining for a worker whose individual operations cannot
//! wait for an external peer. The owner releases before joining on every path.
//! A finite park and the retained unpark token cover the check-to-park race.
//! Settlement depends on scheduling and finite local operations, not pipe EOF.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Clone, Default)]
pub(super) struct FailureReport(Arc<Mutex<Option<String>>>);

impl FailureReport {
    pub(super) fn record(&self, error: impl std::fmt::Display) {
        let message = error.to_string();
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(message);
    }

    pub(super) fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

pub(super) struct Worker {
    released: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    pub(super) fn spawn(
        name: &'static str,
        work: impl FnOnce(&AtomicBool) + Send + 'static,
    ) -> io::Result<Self> {
        if name.as_bytes().contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pipe worker name contains NUL",
            ));
        }
        let released = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&released);
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || work(&flag))?;
        Ok(Self {
            released,
            handle: Some(handle),
        })
    }

    pub(super) fn settle(&mut self) -> bool {
        self.released.store(true, Ordering::SeqCst);
        match self.handle.take() {
            Some(handle) => {
                handle.thread().unpark();
                handle.join().is_err()
            }
            None => false,
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _panicked_during_abandonment = self.settle();
    }
}

#[cfg(test)]
pub(super) mod testing {
    use super::*;
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};

    impl Worker {
        pub(in crate::agent::proc) fn release_token(&self) -> Arc<AtomicBool> {
            Arc::clone(&self.released)
        }

        pub(in crate::agent::proc) fn is_finished(&self) -> bool {
            self.handle.as_ref().is_none_or(JoinHandle::is_finished)
        }
    }

    #[test]
    fn a_worker_panic_is_observed_by_its_joining_owner() {
        let mut worker =
            Worker::spawn("panic-fixture", |_| panic!("finite worker panic")).expect("worker");
        assert!(
            worker.settle(),
            "joining observes the worker's panic outcome"
        );
        assert!(!worker.settle(), "the handle was consumed exactly once");
    }

    #[test]
    fn a_nul_name_is_a_typed_startup_refusal() {
        let result = Worker::spawn("invalid\0name", |_| {});
        assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::InvalidInput));
    }

    pub(in crate::agent::proc) struct JoinedReceiver<T> {
        receiver: Receiver<T>,
        collector: Option<JoinHandle<()>>,
        release: Arc<AtomicBool>,
    }

    impl<T: Send + 'static> JoinedReceiver<T> {
        pub(in crate::agent::proc) fn spawn(
            release: Arc<AtomicBool>,
            collect: impl FnOnce() -> T + Send + 'static,
        ) -> Self {
            let (done, receiver) = mpsc::sync_channel(1);
            let collector = thread::spawn(move || {
                let _receiver_was_dropped = done.send(collect());
            });
            Self {
                receiver,
                collector: Some(collector),
                release,
            }
        }

        pub(in crate::agent::proc) fn recv_timeout(
            &self,
            wait: Duration,
        ) -> Result<T, RecvTimeoutError> {
            self.receiver.recv_timeout(wait)
        }
    }

    impl<T> Drop for JoinedReceiver<T> {
        fn drop(&mut self) {
            self.release.store(true, Ordering::SeqCst);
            if let Some(collector) = self.collector.take() {
                let joined = collector.join();
                if !thread::panicking() {
                    assert!(joined.is_ok(), "collector thread panicked");
                }
            }
        }
    }
}
