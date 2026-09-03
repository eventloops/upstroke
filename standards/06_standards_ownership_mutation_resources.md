## 6. Ownership, mutation, and resources

- Every mutable resource has one identifiable owner, and mutation goes through the narrowest API
  that preserves its invariants.
- Prefer immutable values and explicit state transitions.
- RAII owns files, locks, temporary directories, child processes and every other resource; cleanup
  happens on early return, error, panic unwinding and cancellation as far as the platform allows.
  A guard or resource-owning type beats a `start`/`finish` pair whose second half can be skipped.
- Do not leak references or guards across layers to avoid defining ownership; return an owned
  result or a purpose-built guard whose lifetime is the actual protocol.

**Shared ownership, locks and clones are exceptions, not conveniences.**

- `Rc` and `Arc` MUST NOT be used to avoid deciding who owns a value. Shared ownership is allowed
  only where the design has a real multi-owner lifecycle — a value that must outlive each of its
  holders individually — and the type or an adjacent comment says what that lifecycle is. Prefer a
  single owner with borrowed views, and prefer moving a value into the task that needs it.
- `Mutex` and `RwLock` MUST NOT guard state that a single owner or message passing can hold. A lock
  is allowed only with a documented protected invariant, an explicit acquisition order where more
  than one lock exists, and a small critical section (§10). `Arc<Mutex<T>>` shared between tasks is
  the shape to justify, never the default.
- `clone()` MUST NOT be used to satisfy the borrow checker. A clone is correct when an owned
  snapshot, a transfer to another thread or task, or a small independent value is the intended
  semantics. A clone of anything larger than a handle is visible at a boundary or explained beside
  the call. Prefer `Copy` types, borrows, and moving the original.

These three rules bind the code a change adds or rewrites, now. The existing tree predates them and
is being brought up to them file by file; `standards/SWEEP.md` records which files have been swept
and states the one activation rule for unswept files (the lines a change introduces or modifies,
and any function whose body it modifies).

Enforced by: review; `standards/SWEEP.md` for the transitional clause.
