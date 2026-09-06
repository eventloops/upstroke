# `src/agent/proc/drain.rs`

Extended notes for [`src/agent/proc/drain.rs`](../../../../src/agent/proc/drain.rs).

[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/agent/proc/drain.rs).

The code defines current behavior. These notes preserve contracts and implementation
history. Search each backticked heading fragment separately in the source.

## Module

Bounded output capture with a joined nonblocking pipe worker.

The reader retains at most the byte limit while draining excess output.
WouldBlock parks for a finite interval; consecutive Interrupted results
have a fixed retry bound. Other read errors and invalid counts publish a
typed failure. The caught region includes pipe teardown, so success follows
close and a panic becomes ReaderPanicked.

The supervisor and worker share capture and limit state because the
supervisor inspects output while the child runs. Capture's mutex protects
bytes and their published verdict together and never spans a pipe operation.
The worker's separate release flag arbitrates cancellation. At the grace,
collection releases and joins the worker before taking its capture. Drop
also releases and joins. A live escaped writer cannot make a nonblocking
poll wait for more input. Only observed EOF means ended; release means the
retained prefix is partial even when the worker finishes before collection.

The owned pipe boundary excludes arbitrary blocking Read implementations.
Worker settlement needs scheduling and finite local operations, never an
external peer's cooperation. Normal collection returns every published
failure after settlement. An uncollected drop joins and records its failure
in the invocation's retained FailureReport instead of losing the error on
an early return. A caller retains that observer before starting a sibling.

## `pub(super) struct Drain {`

A nonblocking pipe reader released and joined on collection or drop.

## `impl Drain` › `pub(super) fn collect(self, grace: Duration) -> Result<Captured, DrainError> {`

Wait up to `grace` for EOF or failure, then release and join the worker.
The kernel polls are nonblocking, so settlement does not wait for an
escaped writer. Scheduling can delay a join; the grace bounds waiting
for peer activity, not the operating system's scheduling latency.

Only a published EOF sets ended. Release returns the retained prefix
with ended=false. The text API retains its lossy UTF-8 conversion.

### Errors

Read failure, interruption exhaustion, an invalid count, or a worker
or pipe-teardown panic. A joined worker's failure is never inferred
from a missing EOF or discarded because teardown was still pending.

## `pub(super) const INTERRUPTED_RETRIES: usize = 64;`

How many times in a row an interrupted read is retried before the reader
gives up. A read interrupted this often with no byte between the
interruptions is a signal storm, not a retry.

## `pub(super) enum Stream {`

Which of the child's two output streams a reader is reading. A closed set,
so the thread name built from it cannot carry a NUL and nothing can be
passed that is not one of the two.

## `fn thread_name(self) -> &'static str {`

Linux keeps fifteen bytes of a thread name; both fit.

## `pub(super) enum DrainError {`

Why a stream could not be captured whole. Each variant names the stream
it is about.

## `fmt` › `Start {`

The OS refused a thread for the reader. Nothing was read, and the pipe
was closed with the closure that held it.

## `fmt` › `Read {`

A read failed with something other than `Interrupted`. The reader
stopped there; the bytes before it are not handed over as a transcript.

## `fmt` › `Interrupted {`

Reads were interrupted `interruptions` times in a row — one more than
[`INTERRUPTED_RETRIES`] — with no byte between them, and the reader
gave up.

## `fmt` › `OverlongRead {`

The pipe's `Read` claimed more bytes than the buffer it was handed
could hold, which no bytes can substantiate.

## `fmt` › `ReaderPanicked { stream: Stream },`

The reader thread panicked, so its transcript is not complete.

## `pub(super) struct Captured {`

What [`Drain::collect`] hands back.

## `fmt` › `pub(super) text: String,`

The bytes as text, decoded lossily where they are not UTF-8 (the limit
can cut a multi-byte character).

## `fmt` › `pub(super) limited: bool,`

Whether any byte was dropped for the byte limit.

## `fmt` › `pub(super) ended: bool,`

Whether EOF was observed. Release leaves this false even if the
released worker finishes before collection takes its capture.

## `pub(super) struct CapturedBytes {`

The retained output before text decoding. Consumers of byte-oriented
subprocess protocols must inspect `limited` and `ended` before accepting
the bytes as a complete response.

## `fmt` › `pub(super) bytes: Vec<u8>,`

The retained bytes in their original encoding, including invalid UTF-8.

## `struct Capture {`

What the reader has captured and, once it has finished, its verdict.

## `fmt` › `verdict: Option<Result<ReadEnd, DrainError>>,`

`None` while the reader is still reading. Written once, under the
lock, at the moment the reader decides.

## `enum ReadEnd {`

Why a reader stopped without a read failure. Releasing a reader does not
prove that the stream ended, even when it stops before collection finishes.

## `struct Published;`

Proof that a failure has been published into the capture. Only
[`publish_failure`] makes one, so a reader that returns it has written its
verdict and there is nothing left for the thread to publish.

## `publish_failure` › `capture`

A poisoned capture is one a reader panicked while holding, and this is
the only reader; the bytes in it are intact.

## `fn read_into<R: PollRead>(`

The reader's loop. A failure is published here, under the lock, at the
point of decision, and `Err(Published)` says so. A successful return names
whether EOF or release stopped the loop; the caller publishes that outcome
once the pipe is closed.

## `read_into` › `Err(source)`

A real failure is classified before cancellation. Cancellation
stops further bytes/retries, not an error this finite poll saw.

## `pub(super) fn start<R: PollRead>(`

Read `pipe` on its own thread until end of stream, keeping the first
`limit` bytes and counting the rest as dropped.

The reader keeps reading past the limit so the child cannot block on
a full pipe while the supervisor notices [`Drain::limit_exceeded`] and
terminates its tree.

### Errors

[`DrainError::Start`] when the OS refuses the thread — `EAGAIN` at the
process's thread limit, or no memory for a stack — which a coordinator
under resource exhaustion can meet. Nothing has been read; `pipe` is
closed.

## `start` › `let after_unwind = Arc::clone(&capture);`

A second handle on the capture stays outside the caught
region, so a panic verdict can be published after the
region's own handle has gone with the unwind.

## `start` › `let outcome = catch_unwind(AssertUnwindSafe(move || {`

The pipe is read, then dropped, inside the caught region:
a panic anywhere in the loop or in the pipe's own drop is
this reader's verdict, written beside its captured bytes.
Collection and Drop both join this worker. A failure has
already been published by the loop when it returns
`Err(Published)`; end of stream is published only now, with
the pipe closed, so nothing can still panic after `Ok`.

## `pub(super) fn limit_exceeded(&self) -> bool {`

Whether a byte has been dropped for the limit so far. The supervisor
asks this while the child runs, to terminate a tree that exceeded its
allowance without waiting for the run's timeout.

## `pub(super) fn failure_report(&self) -> FailureReport {`

Retain a secondary-error observer before another worker can fail.

## `pub(super) fn collect_bytes(self, grace: Duration) -> Result<CapturedBytes, DrainError> {`

Collect with the same grace, limit, and EOF semantics as [`Self::collect`],
preserving the retained bytes instead of decoding them as text.

### Errors

The same stream-specific failures as [`Self::collect`], in place of
bytes that a caller could mistake for a complete response.

## `fn collect_with_wait(`

The collection protocol, with its polling wait supplied separately so
tests can observe a pending reader and force the disputed ordering.

## `collect_with_wait` › `let torn_down = self`

Nonblocking polls and finite parking allow release plus a join
without waiting for an escaped descendant to act on the pipe.

## `collect_with_wait` › `let (bytes, verdict) = {`

A reader that has let go leaves the capture owned here, and it is
moved out; one still running keeps its handle, and the supervisor
takes a copy of what has arrived and the verdict, if any, that came
with it.

## `fn publish_panic(capture: &Mutex<Capture>, stream: Stream) {`

The panic verdict, written by the reader's thread after its caught region
unwound; `capture` here is the handle the thread kept outside that region.

## `pub(super) fn drain_limit_exceeded(stdout: &Option<Drain>, stderr: &Option<Drain>) -> bool {`

Whether either stream has dropped a byte for its limit; a stream that was
not captured at all has not.

## `const BOUND: Duration = Duration::from_secs(10);`

The bound on every wait below. It bounds a wedged reader, never a
healthy one: nothing here takes more than a few hundred milliseconds.

## `enum Step {`

What a scripted pipe answers to each `read`, in order. An exhausted
script is end of stream.

## `struct Scripted {`

A pipe that answers from a script. If `hold_drop` is set, dropping it
— which the reader does inside its caught region, after a failure has
been published and before end of stream is — blocks until the test
sends or drops the sender, so a test can hold the thread there. If
`panic_on_drop` is set, dropping it panics instead.

## `enum Feed {`

What the test feeds a held pipe: bytes, or an interruption.

## `struct Held {`

A pipe whose writer is the test. Each `read` announces itself on
`entered` and then blocks until the test sends a feed; a dropped
sender is end of stream. The announcement is what lets a test know the
previous chunk has been appended: the reader appends before it reads
again, and the announcement is sent from the reader's own thread. When
the reader stops and drops this, `entered` disconnects.

## `type Collected = (Result<Captured, DrainError>, Duration);`

What `collect` returned, and how long it took to.

## `fn collect_elsewhere(`

`collect` on its own thread, so a `collect` that does not return within
`BOUND` fails the test that called this instead of hanging the harness.

## `fn reader_stopped(entered: &mpsc::Receiver<()>) -> bool {`

Whether the reader has stopped: its pipe dropped, so the announcement
channel disconnected rather than announcing another read.

## `the_limit_keeps_exactly_limit_bytes_and_reports_only_a_dropped_byte` › `let exact = start([text("abcd"), text("efghij")], 10);`

Exactly at the limit: nothing dropped, so not limited.

## `the_limit_keeps_exactly_limit_bytes_and_reports_only_a_dropped_byte` › `let over = start([text("abcd"), text("efghijkl")], 10);`

One chunk straddles the limit: kept to the byte, and limited.

## `the_limit_keeps_exactly_limit_bytes_and_reports_only_a_dropped_byte` › `let none = start([text("a")], 0);`

A limit of zero keeps nothing and reports the first byte.

## `a_byte_between_interruptions_restarts_the_bound` › `let mut steps = Vec::new();`

Twice the bound in total, never the bound in a row.

## `a_failure_decided_before_the_grace_is_seen_even_if_the_thread_has_not_finished` › `let (release, hold) = mpsc::channel::<()>();`

The reader fails, publishes its verdict at the decision, and is
then held between the verdict and the end of its thread: the
pipe's `Drop` blocks until `release` is dropped, inside the caught
region, with the reader's handle on the capture still alive. A
`collect` that read the verdict off the thread's completion, or only
off a capture it owned outright, would hand the bytes over as
complete.

## `a_panic_while_the_pipe_is_dropped_is_the_verdict_and_not_a_success` › `let mut pipe = Scripted::new([text("before")]);`

End of stream, then the pipe's `Drop` panics: end of stream is
published only once the pipe is closed, so the panic is what the
reader reports.

## `collect_waits_for_the_stream_to_end_and_returns_what_arrived_by_then` › `let (drain, feeds, entered) = held(1 << 20);`

The reader has polled an empty live stream before collect starts.
The collector must enter its wait before the test supplies output.

## `collect_waits_for_the_stream_to_end_and_returns_what_arrived_by_then` › `observed_wait`

This signal comes from inside the pending-reader loop. A collector
that skips its wait cannot produce it, regardless of scheduling.

## `collect_releases_a_reader_whose_writer_never_closes_once_the_grace_expires` › `entered.recv_timeout(BOUND).expect("the second read");`

The second read is announced only after "partial" was appended.

## `collect_releases_a_reader_whose_writer_never_closes_once_the_grace_expires` › `let outcome = result.recv_timeout(BOUND);`

Bounded so a collect that joins without releasing the reader fails
this test instead of hanging the harness: the writer is closed on
the way out, which lets both threads finish.

## `collect_releases_a_reader_whose_writer_never_closes_once_the_grace_expires` › `assert!(feeds.send(Feed::Bytes(b"orphan".to_vec())).is_err());`

The joined reader has closed its endpoint while this writer is live.

## `a_released_reader_stops_at_an_interruption_too` › `feeds`

Force cancellation between entering a poll and its Interrupted
result. The post-poll check must stop before another retry.

## `a_released_reader_cannot_report_eof_before_its_capture_is_taken` › `drain`

Force collect's disputed ordering: release first, then let the
reader finish before taking its capture. The public collect
call cannot pause at that scheduling point, so this test sets
its release flag directly. The writer remains open throughout.

## `dropping_a_drain_without_collecting_it_releases_its_reader` › `let (drain, feeds, entered) = held(1 << 20);`

The supervisor's error exits drop a drain they never collect; the
reader must not be left draining for the orphan's lifetime.
