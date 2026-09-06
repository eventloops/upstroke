# `src/validate/render.rs`

Extended notes for [`src/validate/render.rs`](../../../src/validate/render.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

What the preview looks like: the echo lines `run` fills a [`Report`] with,
the per-task row, and the table [`Report::render`] delegates to.

Split out of `super` unchanged. [`Report::render`] stays an inherent method
on the parent — it is the rendering surface every caller and the wrapper
census know — and its body is [`report`] here, which is the only thing that
moved.

**The denial is restored rather than inherited.** `super` carries
`#![allow(clippy::disallowed_methods)]` because `write_normalized_json`
writes the normalized plan with `fs::write`, and a module-level allow reaches
every child of the module it sits in. Nothing here writes anything — every
function returns a `String` and the sink is always one of its own — so that
allowance has no business extending here, and this line is what stops it.
That is also what keeps this file out of `effects/allowlist.toml`: an
allowance is what that file records, and this module takes none.

## `pub(super) fn review_echo(plan: &ReviewPlan) -> String {`

Who judges the work (§11.2–§11.3), for the preview.

Resolved against the adapters this build ships, not against binaries found
on PATH: `validate` and `--dry-run` execute nothing (§18), so they cannot
probe. Pre-flight is where a named reviewer has to prove it can actually
run — and where a missing one either warns or refuses. The line says so,
because a preview that reads as a promise is worse than one that reads as a
plan.

## `pub(super) fn capacity_echo(`

§13's capacity block, for a command that executes nothing.

`validate` and `--dry-run` **do not probe** (§18): every figure here comes
from files — the pools file, and the latest run's event log in this
repository. That is a real distinction rather than a technicality, and the
block says which side of it each line is on, because `upstroke capacity` shows
strictly more by being allowed to spawn the vendors' CLIs.

The same reason the review line says "if installed": a preview that reads as
a promise is worse than one that reads as a plan.

## `if let Some(binding) = second_opinion {`

§11.3: a second reviewer is a per-task routing decision like any other,
so it belongs in the column that shows what this task's paths bought it.

## `pub(super) fn report(report: &Report) -> String {`

The body of [`Report::render`].
