## 6. Ownership, mutation, and resources

- Each mutable resource MUST have an identifiable owner. Mutation should occur through the
  narrowest API that can preserve its invariants.
- Prefer immutable values and explicit state transitions. Interior mutability, global state, and
  shared mutable ownership require a concrete lifecycle or concurrency reason.
- Do not add `clone()` merely to silence a borrow-checker problem. A clone is appropriate when an
  owned snapshot, task transfer, or small independent value is the intended semantics; expensive
  or non-obvious clones SHOULD be evident at the boundary or explained.
- Use RAII for files, locks, temporary directories, child processes, and other resources. Cleanup
  MUST also occur on early return, error, panic unwinding, and cancellation to the extent the
  platform permits.
- Scope guards and resource-owning types are preferable to paired `start`/`finish` calls whose
  second half can be skipped.
- Do not leak references or guards across layers merely to avoid defining ownership. Return an
  owned result or a purpose-built guard whose lifetime expresses the actual protocol.
