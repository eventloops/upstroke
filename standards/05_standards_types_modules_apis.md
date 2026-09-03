## 5. Types, modules, and APIs

**State.**

- Parse and validate untrusted or weakly typed input once; internal code consumes validated enums,
  newtypes and structs rather than rechecking strings and integers.
- An enum, not string tags or a set of booleans, encodes alternatives with different meaning. A
  boolean parameter whose call site does not explain itself becomes a named enum or options struct.
- Fields stay private where construction or mutation must preserve an invariant; provide validated
  constructors and operations instead of exposing the representation.
- `Option<T>` is absence, `Result<T, E>` is failure, `Result<Option<T>, E>` when both are possible.
  Never collapse failure into absence.
- Identifiers, paths, timestamps, durations and capacity quantities get dedicated types wherever a
  mix-up is possible.
- Narrowing and sign changes use checked conversions; an `as` cast needs a nearby invariant that
  proves the range. Arithmetic chooses checked, saturating, wrapping or failing behaviour
  deliberately; floating-point input rejects non-finite values before it is ordered, budgeted or
  persisted. Release builds keep `overflow-checks = true`.
- No wildcard arm over a closed domain enum where a new variant should force a decision at each
  site. Wildcards are fine over intentionally open external input.

**API shape.**

- Default to private; `pub(crate)` for crate-internal collaboration; `pub` only for a supported
  external contract. The crate is published, so a public API change is assessed for SemVer impact
  in review (`cargo semver-checks` is the intended mechanism once a release leg runs it).
- A function performs one coherent operation at one abstraction level. Many parameters, especially
  repeated booleans or options, point to a request or configuration type.
- Borrow (`&str`, `&Path`, slices) when the callee does not retain ownership; take or return owned
  values when data is stored, transferred to another task, or deliberately detached.
- Return values rather than mutating caller-provided output parameters.
- Implement `Debug`, `Clone`, `Eq`, `Hash`, iterators and conversions when their semantics are
  honest. `From` is infallible; validation belongs in `TryFrom` or a named fallible constructor.

**Traits.** Introduce a trait when multiple implementations exist or are part of the accepted
design, callers are generic over a real behavioural contract, runtime heterogeneity is required, or
the trait isolates a real effect boundary — never to mirror a class/interface pair. Traits are
cohesive, object-safe only when trait objects are needed, and explicit about observable semantics.
Before creating a shared abstraction compare the full invariants, not the syntax: callers that
differ in atomicity, ownership, error handling, ordering or durability are not the same operation.

Enforced by: rustc and targeted Clippy lints where they apply; review for the rest.
