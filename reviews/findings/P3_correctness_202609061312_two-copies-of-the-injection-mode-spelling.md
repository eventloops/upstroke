---
id: SWEEP-HARNESS-001
severity: P3
disposition: deferred
pr: TBD
category: correctness
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/vocab.rs:493
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects/vocab.rs` (queue row 26), with `src/topology/effects/registry.rs` (row 23) as the second call site
---

## Failure sequence

`InjectionMode` has no `Display`, so the two types that render a mode each carry
their own copy of the mapping: `HookPhase`'s `Display`
(`src/topology/effects/harness.rs`, `Kill => "kill"`, `ErrorReturn =>
"error-return"`) and `EntryPhase`'s `Display` (`src/topology/effects/registry.rs`,
the same two arms, character for character). They are two hand-maintained
statements of one spelling, in two files, and the spelling is precisely what a
reader compares across the two: a `BijectionFailure::Unobserved` names its phase
as a `HookPhase` ("`{site}` was never observed executing its `{phase}` hook")
and every `RegistryError` names its phase as an `EntryPhase`, so a coordinate a
suite has to line up is quoted by one impl in one message and the other impl in
the next -> a change to either arm, in either file, leaves the other and the two
messages disagree about what the same mode is called, with nothing failing. The
tree already refuses this shape one module up: `EffectSiteId::all` is derived
from the groups' `ALL` slices because "two hand-maintained lists of seventy sites
would disagree eventually, and the one that disagreed silently would be this
one".

Measured at this head: the two impls agree, and the agreement is guarded on one
side only. Mutating `harness.rs`'s arm to `error_return` fails
`a_hook_phase_renders_as_the_name_a_failure_quotes_it_by` (added by this sweep)
and `a_point_and_a_mode_are_one_coverage_coordinate_and_not_two_axes`; no test
of `topology::effects::` compares the two renderings with each other, so a
mutation applied to *both* arms at once is green.

## What the change that takes this up should do

Give `InjectionMode` the `Display` its spelling belongs to, in `vocab.rs` beside
`ALL` and `name()`-shaped accessors of its neighbours, and have both `HookPhase`
and `EntryPhase` write `"{point}/{mode}"` through it — one authority, two
readers, as `SubEffectPoint`'s own `Display` already is for the other half of the
coordinate. `harness.rs` is swept, so its arm is a one-line edit inside a swept
file; `registry.rs` is queue row 23 and `vocab.rs` row 26, and either sweep can
take it as long as it takes both call sites. A test that renders one `HookPhase`
and the `EntryPhase` of the same coordinate and asserts the two strings equal is
what keeps them one spelling afterwards.
