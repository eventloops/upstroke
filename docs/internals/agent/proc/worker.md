# `src/agent/proc/worker.rs`

Extended notes for [`src/agent/proc/worker.rs`](../../../../src/agent/proc/worker.rs).

## `pub(super) struct FailureReport(Arc<Mutex<Option<String>>>);`

One bounded secondary failure slot. The worker owner and the invocation
share it so a failure found during Drop reaches the invocation's outcome.

## `pub(super) struct Worker {`

A unique joining owner. The worker shares only its cancellation flag.

## `pub(super) fn spawn(`

Spawn one worker and give it a release flag checked between finite
nonblocking operations. Names come only from the caller's fixed labels.

### Errors
The OS refused the thread; the unstarted closure and its pipe are dropped.

## `spawn` › `let flag = Arc::clone(&released);`

The worker may run concurrently with cancellation by its owner.

## `pub(super) fn settle(&mut self) -> bool {`

Release and join once. True names a panic outside the worker's own
caught operation, which its collector maps to a supervision failure.

## `drop` › `let _panicked_during_abandonment = self.settle();`

Drain and Feeder explicitly settle and report before their Worker
field drops. This final fallback retains joining ownership if a
future caller abandons Worker directly. Drop never adds a panic
while another stack is unwinding.

## `release_token` › `Arc::clone(&self.released)`

Test collection owns cancellation during failed assertions too.

## `pub(in crate::agent::proc) struct JoinedReceiver<T> {`

The test keeps both cancellation and the collector's joining owner.

## Module

Cancellation and joining for a worker whose individual operations cannot
wait for an external peer. The owner releases before joining on every path.
A finite park and the retained unpark token cover the check-to-park race.
Settlement depends on scheduling and finite local operations, not pipe EOF.
