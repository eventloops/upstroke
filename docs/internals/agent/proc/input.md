# `src/agent/proc/input.rs`

Extended notes for [`src/agent/proc/input.rs`](../../../../src/agent/proc/input.rs).

## `const INTERRUPTED_RETRIES: usize = 64;`

Consecutive interruptions retried before a signal storm is a failure.

## `pub(super) enum FeedError {`

Why stdin supervision failed, with the operation retained in the error.

## `struct Published;`

Proof that the loop already published its failure before pipe teardown.

## `publish_failure` › `*verdict.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err(error));`

The worker is the only publisher. Poison means it panicked while
publishing, and collection still needs to observe its defined failure.

## `pub(super) struct Feeder {`

One stdin writer whose post-exit wait is bounded even if a descendant kept
the read end open. Dropping this value releases the worker too.

## `pub(super) fn start<W: PollWrite>(pipe: W, bytes: Vec<u8>) -> Result<Self, FeedError> {`

Move the input and pipe to a writer thread. A broken pipe accepts the
child's refusal of unread input; other published failures are returned
by [`Self::collect`].

### Errors

[`FeedError::Start`] when the OS refuses the thread. The closure and
pipe are dropped on that path, so the caller can settle the child tree.

## `start` › `let outcome = catch_unwind(AssertUnwindSafe(|| {`

Keep the shared verdict outside the caught region so it
survives both a write panic and a panic closing the pipe.

## `pub(super) fn failure_report(&self) -> FailureReport {`

Retain the invocation's observer before another pipe setup can fail.

## `pub(super) fn collect(mut self, grace: Duration) -> Result<(), FeedError> {`

Wait for a verdict up to the post-exit grace, then release the writer.
A still-pending write belongs to an escaped reader after the child has
exited; delivery of that remaining input is best-effort. Any verdict
published before collection is inspected even during pipe teardown.

### Errors

A published write failure other than `BrokenPipe`, excessive consecutive
interruptions, an invalid write count, or a worker panic. A known
failure is never discarded merely because the thread is not finished.

## Module

Bounded stdin delivery with a joined nonblocking pipe worker.

The worker owns its endpoint and input bytes. Supervisor and worker share
a verdict because collection may inspect progress before the worker ends.
No mutex spans a pipe operation. Failure is published at its decision;
success follows pipe close, and write/teardown panics become typed failures.
WouldBlock uses finite parking, with release plus unpark before every join.

BrokenPipe means the child declined remaining input. Other write failures
and excessive consecutive interruptions are supervision errors. At the
post-exit grace, release ends further attempts and collection joins before
returning. Delivery of remaining input is best-effort after child exit.
Dropping an uncollected feeder joins and records its failure for the
invocation's retained FailureReport, including early supervisor returns.
