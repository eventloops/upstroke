## 10. Concurrency and async code

The codebase is synchronous at adoption. Requirements that specifically mention `.await` are N/A
until the v0.2 async scheduler lands; the ownership, arbitration, cancellation, ordering, and
bounded-work rules apply to current threads and subprocesses now.

Concurrent code MUST have a written protocol visible in types, API documentation, or an adjacent
comment. The protocol identifies the owner of shared state, the atomic/linearization point, valid
state transitions, winner and loser behaviour, and cleanup after failure or cancellation.

- Correctness MUST NOT depend on task start order, sleeps, or one worker usually finishing first.
- Do not use check-then-act against shared state. Use the atomic filesystem, lock, channel, or CAS
  operation that actually arbitrates the transition.
- Prefer message passing or single-owner coordination over shared mutable state. When a lock is
  necessary, keep its protected invariant and acquisition order explicit and its critical section
  small.
- Never hold a synchronous lock guard across `.await`. Blocking filesystem or process work MUST
  not occupy an async executor thread without an explicit blocking boundary.
- Bound queues, fan-out, retries, and spawned work. Backpressure and overload behaviour are part of
  the API, not tuning details.
- Join every spawned thread's `JoinHandle` and turn a worker panic into a defined outcome. A
  detached thread whose failure nobody observes is the worker that dies without a report. The
  same rule applies to task handles when the async scheduler lands.
- Cancellation MUST leave no published partial result, orphaned child process, leaked worktree, or
  permanently claimed capacity. If an operation is not cancellation-safe, its API MUST make that
  constraint explicit and shield the critical region.
- Externally observable ordering MUST be deterministic or explicitly documented as unordered.
- Prefer deriving `Send` and `Sync` from ordinary safe fields. Manual unsafe implementations
  require the same proof discipline as any other unsafe code.

Concurrency tests SHOULD force the disputed interleaving with barriers, channels, or controlled
fakes. A stress loop or a sleep may supplement such a test but cannot be its only oracle. When the
v0.2 scheduler lands, interleaving-sensitive protocols SHOULD also run under a model checker such
as Loom where its modelled primitives reach the code; until a named repository gate exists, that
is a triggered review requirement, exactly as with Miri in §11.
