## 5. Types, modules, and APIs

### Representing state

- Parse and validate untrusted or weakly typed input once. Internal code SHOULD consume validated
  enums, newtypes, and structs rather than rechecking strings and integers.
- Use an enum when alternatives have different meaning or behaviour. Do not use string tags or a
  collection of booleans to encode a state machine.
- Avoid boolean parameters whose call sites do not explain themselves. Use a named enum or options
  struct when the value selects behaviour.
- Keep fields private when construction or mutation must preserve an invariant. Provide validated
  constructors and operations; do not expose a representation and ask callers to behave.
- Use `Option<T>` for legitimate absence and `Result<T, E>` for failure. Use
  `Result<Option<T>, E>` when both are possible; never collapse failure into absence.
- Do not encode domain identifiers, paths, timestamps, durations, or capacity quantities as
  interchangeable strings or numbers when a dedicated type can prevent a mix-up.
- Use checked conversions for narrowing or signedness changes. An `as` cast is acceptable only
  when it is lossless by type or a nearby invariant proves the range. Arithmetic MUST choose
  checked, saturating, wrapping, or failing behaviour deliberately; floating-point input MUST
  reject non-finite values before it is ordered, budgeted, or persisted. Release builds keep
  `overflow-checks = true`, so an overflow nobody chose fails loudly instead of wrapping.
- Avoid wildcard arms over closed domain enums when a new variant should force a decision at each
  call site. Wildcards remain appropriate for intentionally open external inputs.

### API shape and visibility

- Default to private. Use `pub(crate)` for crate-internal collaboration and `pub` only for a
  supported external contract.
- A function SHOULD perform one coherent operation at one abstraction level. Many parameters,
  especially repeated booleans or optional values, indicate that a request/configuration type may
  better express the contract.
- Borrow inputs (`&str`, `&Path`, slices, references) when the callee does not retain ownership.
  Accept or return owned values when data is stored, transferred to another task, or deliberately
  detached from the input lifetime.
- Return useful values rather than mutating caller-provided output parameters.
- Implement common traits (`Debug`, `Clone`, `Eq`, `Hash`, iterators, and conversions) when their
  semantics are honest and useful. `From` is infallible; validation belongs in `TryFrom` or a
  named fallible constructor.
- New or changed public APIs MUST be assessed for SemVer impact — the crate is published, so this
  is a live contract. `cargo semver-checks` becomes the named mechanism when a release-workflow
  leg runs it; the assessment is review-only until then. Public types SHOULD remain open to
  compatible evolution only when that flexibility is deliberate; do not add `#[non_exhaustive]`
  reflexively.

### Traits and abstraction

Introduce a trait when at least one of these is true: multiple implementations exist or are part
of the accepted design; callers are generic over a meaningful behavioural contract; runtime
heterogeneity is required; or the trait isolates a real effect boundary. Do not introduce a trait
solely to mirror an object-oriented class/interface pair.

Traits MUST be cohesive, object-safe only when trait objects are needed, and explicit about
observable semantics. Prefer generics for static composition and trait objects for genuine runtime
selection. Avoid public blanket implementations that foreclose future implementations.

Before creating a shared abstraction, compare the full invariants—not just similar syntax. If two
callers differ in atomicity, ownership, error handling, ordering, or durability, they are not yet
the same operation.
