# `src/route.rs`

Extended notes for [`src/route.rs`](../../src/route.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Routing resolution (DESIGN.md §10) and the binder preview.

Chains stay abstract tiers; the binder normally resolves them at attempt
time against live capacity. Step 1 has no capacity engine, so every rung
carries a catalog-derived example binding tagged `preview` (or the pinned
binding tagged `pin`).

## `#![allow(clippy::disallowed_methods)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub enum ChainSource {`

Why a rung sits where it does in the chain.

## `pub fn resolve(task: &Task, cfg: &Config) -> ResolvedChain {`

Resolve a task's escalation chain: config baseline, then blast-radius
floors, then the designer's advisory `tier=`, then the binding `min=` clip.

## `pub fn resolve(task: &Task, cfg: &Config) -> ResolvedChain` › `for ov in &cfg.overrides {`

Blast-radius floors: a matching override raises the start. Blast radius
beats nominal difficulty (§10.2). An override carrying only a
`second_opinion` has no floor to apply and is handled by the reviewer
(§11.3), not here.

## `pub fn resolve(task: &Task, cfg: &Config) -> ResolvedChain` › `if let Some(tier) = task.suggested_tier {`

`tier=` is advisory: it becomes the chain start only if it outranks the
current start. An annotation that merely agrees with a blast-radius
floor must not take credit for it — the override is what binds, and the
preview has to say so (§10.2: blast radius beats nominal difficulty).

## `pub fn resolve(task: &Task, cfg: &Config) -> ResolvedChain` › `if !raised {`

Agreeing with a silent default still counts as the designer's
decision; agreeing with an override does not — the override is what
holds the start up, and removing the annotation would not lower it.

## `pub fn resolve(task: &Task, cfg: &Config) -> ResolvedChain` › `if let Some(min) = task.min_tier {`

`min=` is binding: clip everything below it.

## `fn raise_start(tiers: &mut Vec<(Tier, ChainSource)>, floor: Tier, source: ChainSource) ->…`

Drop rungs below `floor`; if that empties the chain, the floor itself
becomes the only rung. Returns whether anything changed, relabeling the new
start when it did.

## `mod tests` › `fn hermetic() -> (PathBuf, PathBuf) {`

A directory with no config and one empty pools file, built once.

Every test here routes through it, and it used to be rewritten on every
call at a path shared by *every process on the machine* — not even
pid-scoped, so a second `cargo test` binary truncated it under this
one's readers. The content is identical for every caller, so there was
never anything to rewrite.

## `fn hermetic() -> (PathBuf, PathBuf)` › `let empty = dir.join("no-pools.toml");`

A real, empty pools file: an explicit pools path that does not
exist is a hard error, and `None` would read the operator's own.

## `fn advisory_tier_raises_or_relabels_but_never_lowers()` › `let mut equal = task(TaskKind::Design);`

Equal to the baseline start: relabeled as the designer's decision.

## `fn advisory_tier_raises_or_relabels_but_never_lowers()` › `let mut lower = task(TaskKind::Design);`

Below the baseline start: advisory is ignored.

## `fn path_floor_raises_start_with_override_source()` › `let mut unmatched = task(TaskKind::Fix);`

Non-matching paths keep the full default chain.

## `fn path_floor_raises_start_with_override_source()` › `let mut agreeing = task(TaskKind::Fix);`

An annotation agreeing with the override must not take credit for
a floor the override is holding up.
