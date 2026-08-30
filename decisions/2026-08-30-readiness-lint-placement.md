# 2026-08-30 — a per-site `#[expect]` may stand where a module-level allow did

**Decision.** `decisions.effect_site_inventory.mechanism` (2) permits an
allowance of a governed lint "only as module-level attributes in files listed in
`effects/allowlist.toml`". That is amended, narrowly: a **per-site
`#[expect(<governed lint>, reason = …)]`** may stand below module level, in a
file whose `effects/allowlist.toml` row records the lint **and** the exact number
of such annotations, and only where the file **denies** that same lint at module
level. Nothing else about the rule moves. A per-site `#[allow]` is still refused,
an unrecorded `#[expect]` is still refused, and a file-scope allowance is still
the only other permitted shape.

Applied to one file today: `src/agent/proc/test_support/readiness.rs`, whose
blanket `#![allow(clippy::disallowed_methods)]` becomes a file-scope deny plus
six per-site expectations.

## Why the module-level rule was written, and why this does not weaken it

The rule exists because of `PR6-LANEF-004`. A Rust lint level is scoped by the
**module tree**, not by the file, so an out-of-line child of a funnel silently
inherits the funnel's allowance: a `ContainerRuntime::start` planted in a child
of `src/runner/container.rs` passed `cargo clippy --all-targets --all-features
-- -D warnings`, measured twice. The repair was to make every such module state
its own level, and to require that any *allowance* be a module-level attribute
in a reviewed row — because an allowance buried on a function is indistinguishable
from an allowance nobody reviewed.

A per-site `#[expect]` is not the shape that finding was about, and it is
strictly **narrower** than the module-level allow it replaces:

- The file-scope allow is a claim about the whole file. Every later denied call,
  in any function, arrives under it. The per-site expectation covers one
  statement.
- `expect` is checked in both directions. An expectation that goes unfulfilled is
  `unfulfilled_lint_expectations`, which is a warning, which is an error under
  the `-D warnings` the gate runs with. An `allow` says nothing when the thing it
  permits stops happening.
- The module-level **deny** is what the expectation narrows. Without it the
  expectation would be decorating an inherited allowance rather than carving an
  exception out of a denial, and the census refuses that case: the amendment
  requires the deny.

So the compiler owns the count in both directions — a seventh denied call is a
build error, a vanished one is a build error — where before, the count lived only
in prose and in a census reading the file with needles.

## Measured, not assumed

- Deleting `readiness.rs`'s `#![allow(clippy::disallowed_methods)]` changed
  **nothing** at `cargo clippy --all-targets --all-features -- -D warnings`,
  because `src/agent/proc.rs`'s own allow reaches the file through the module
  tree. The allowance bought no diagnostic; it was a governance statement, and it
  was the wrong statement to make.
- With the file-scope deny and the six expectations in place, the same command is
  clean — which is only possible if all six expectations are **fulfilled**, and
  therefore only if the lint really does fire at all six sites.
- A seventh denied call planted in the file is an error; removing a call under an
  expectation is an error. Both were run as mutations against the same command.

## What enforces it

- `effects::tests::every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist`
  carries the amendment. The below-module-level exception applies only to an
  `expect`, with a reason, of a lint the file denies at module level, in a file
  whose row records a non-zero `expect_sites`; the recorded count is compared
  with the count found, so an annotation appearing or vanishing has to pass
  through the row.
- `effects::tests::the_readiness_expectations_are_per_site_and_both_records_say_so`
  keeps the file's prologue, the six annotations and the row saying the same
  thing.
- `runner::container::tests::every_child_module_of_the_container_funnel_states_its_own_lint_level`
  is unchanged and still refuses a funnel child that states neither a denial nor
  a recorded allowance. `readiness.rs` now states a denial for all three governed
  lints.
- `runner::container::tests::the_readiness_allowance_names_the_paths_it_is_written_against`
  is unchanged. It was the authority on which primitives the file reaches; it is
  now a second, independent reading beside the compiler's.

## Rejected

- **Leave the blanket allow and keep only the arithmetic census.** Rejected: the
  census is a set of needles derived from `clippy.toml`, and it can only see what
  `clippy.toml` names. The compiler sees resolution — a renamed import, a
  re-export, a function value — which is the whole reason `mechanism` (1) denies
  by path rather than by spelling.
- **Widen the rule to any per-site `#[allow]`.** Rejected: `allow` is not checked
  when it stops being needed, so a per-site allow is a module-level allow with a
  smaller blast radius and the same silence. The asymmetry between `allow` and
  `expect` is the entire reason this amendment is safe.
- **Amend `DESIGN.md`.** Not applicable: the sentence amended is a decision
  packet's, not the design's, and `DESIGN.md` says nothing about lint-attribute
  placement.

## Links

- Amends `decisions.effect_site_inventory.mechanism` (2).
- Constrained by [2026-08-24](2026-08-24-pr3-layer-freeze-charter.md): this is a
  governance-record change plus a comment-and-attribute diff in one test-support
  file, and it changes no production behaviour, no API and no schema.
