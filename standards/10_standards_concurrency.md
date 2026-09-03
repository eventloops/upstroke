## 10. Concurrency and async code

Concurrent code has a written protocol — in types, rustdoc or an adjacent comment — naming the owner
of shared state, the linearization point, the valid transitions, winner and loser behaviour, and
cleanup after failure or cancellation.

- Correctness never depends on start order, sleeps, or one worker usually finishing first.
- No check-then-act against shared state: the atomic filesystem, lock, channel or CAS operation is
  what arbitrates.
- Prefer message passing or a single owner to shared mutable state (§6). A lock keeps its invariant
  and acquisition order explicit and its critical section small. Never hold a synchronous guard
  across `.await`, and never run blocking filesystem or process work on an async executor thread
  without an explicit blocking boundary.
- Queues, fan-out, retries and spawned work are bounded; backpressure is part of the API.
- Every spawned thread's `JoinHandle` is joined and a worker panic becomes a defined outcome; a
  detached thread nobody observes is the worker that dies without a report. The same applies to
  task handles when the async scheduler lands.
- Cancellation leaves no published partial result, orphaned child process, leaked worktree or
  permanently claimed capacity; an operation that is not cancellation-safe says so in its API and
  shields the critical region.
- Externally observable ordering is deterministic or documented as unordered.
- `Send` and `Sync` come from ordinary safe fields; a manual unsafe implementation needs §11's proof.

Concurrency tests force the disputed interleaving with barriers, channels or controlled fakes; a
stress loop or a sleep may supplement such a test but cannot be its only oracle.

Enforced by: rustc `Send`/`Sync` and borrow checking; deterministic tests; review for protocol,
bounds and cancellation.
