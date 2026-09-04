---
id: SWEEP-BIJECTION-007
severity: P3
disposition: deferred
category: correctness
pr: 146
reviewed_sha: 943ae61dc61c579a3b03744c8994a1ce81a9acf8
location: src/topology/effects/registry.rs:279
provenance: pre_existing
first_bad: SWEEP-BIJECTION-004
guard: queue row 23, `src/topology/effects/registry.rs`
---

## Failure sequence

`BijectionFailure` now names its site and its phase with the typed values, and
`InvalidEntry` carries the `RegistryError` itself instead of that error's rendered text. One
type short of complete: every variant of `RegistryError` carries `site: String` and most carry
`phase: String`, both built by `EffectSiteId::name()` and `EntryPhase::to_string()` at the
point of failure.

So a test that wants to assert *which* entry the format refused, having matched
`BijectionFailure::InvalidEntry { error: RegistryError::WrongFaultRow { site, .. }, .. }`, is back
to comparing `site` with a string literal — the surface this pull request removed one level up.
A site name misspelt in such a literal is a test that silently matches nothing, and
`.any(...)` over a failure list turns that into an assertion that passes for the wrong reason as
soon as any other failure of the same variant is present.

Nothing is wrong today: `RegistryError`'s producers all derive the string from the typed value,
and no test compares one against a literal. What is wrong is that the door this pull request closed
in `bijection.rs` is still open in the file next to it.

## What the change that takes this up should do

`src/topology/effects/registry.rs` is queue row 23. Its sweep should carry `site` as
`EffectSiteId` and `phase` as `EntryPhase` through `RegistryError`, keeping `String`
only where the value really is a document's own words — `found` and `expected` resume actions,
and the `&'static str` class names, which are already static. Both types implement `Display`,
so the `#[error]` messages do not change. The call sites are `validate_entry`,
`FaultRegistry::insert` and the format's tests.
