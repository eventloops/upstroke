# `src/agent/proc/hooks.rs`

Extended notes for [`src/agent/proc/hooks.rs`](../../../../src/agent/proc/hooks.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The funnel's observation and injection surface: the trait a watcher
implements, the do-nothing observer production passes, and the two appliers
that turn an answer back into control flow.

Split out of `src/agent/proc.rs`, whose supervision primitives consult these
at every containment step. Nothing here starts, signals or waits on a
process: it decides what the caller must do next and returns.

`PR6-LANEF-004`: a Rust lint level is scoped by the module tree and not by
the file, so this file would inherit the funnel's `#![allow]` of the three
governed lints without saying so. It denies all three instead, and carries no
`effects/allowlist.toml` row -- a denial needs none; only an allowance does.

## `pub trait SpawnHooks {`

The parent-side containment steps of one spawn, told to whoever is watching.

`decisions.effect_site_inventory.containment_sub_effects`: "the process
funnel exposes hook points for platform containment steps, each a
Topology/Shared site with documented residue and recovery". PR3 declared the
eight points ([`SubEffectPoint`]); PR4 makes them execute, and this is the
interface through which they do.

Production passes [`NoHooks`], which answers [`Injection::Proceed`] to
everything and costs a virtual call per containment step. The ST-07 subset
passes [`crate::runner::HarnessHooks`], which records into PR3's
`HookHarness` and returns whatever the suite armed.

## `pub trait SpawnHooks` › `fn point(&mut self, point: SubEffectPoint) -> Injection;`

The funnel reached `point`. The answer says what it must do there.

## `pub trait SpawnHooks` › `fn point_mode(&mut self, point: SubEffectPoint, mode: InjectionMode) -> Injection {`

The funnel reached `point`, at the coordinate that mode's fault belongs
at.

A point whose two modes fire at two coordinates cannot be consulted
once. `Spawn.AmbientJobJoined` is the one:
`containment_sub_effects` gives it an error contract — "failure refuses
the write command" — which stands *in place of* establishing the job, so
it is consulted **before** the join; and it gives it the kill claim "a
coordinator kill after any of these leaves no host process (the ambient
handle closes …)", which is only true **after** the join, because before
it there is no handle to close.

The default answers with [`Self::point`], so an observer that does not
distinguish modes behaves exactly as it did.

## `pub trait SpawnHooks` › `fn child_created(&mut self, _pid: u32) {}`

The funnel created a child and has not yet contained it.

Called between `CreateProcess`/`fork` and the next containment step, so
an observer that is about to inject a kill can record the identity that
must not survive the coordinator. The Windows stub test needs the pid
*and* the creation time, because Windows reuses pids, and only the
funnel knows the pid before it dies.

## `pub struct NoHooks;`

What production passes: nothing is armed and nothing is recorded.

## `pub(super) fn apply(injection: Injection, point: SubEffectPoint) -> Result<(), UpstrokeEr…`

Do what a hook answered.

[`Injection::Kill`] aborts. Not `panic!` and not `std::process::exit`:
the whole claim under test is that a coordinator which dies **without
running any cleanup** still leaves no host process, and both of those run
destructors — including the one that closes the very job handle whose
close-on-death is the mechanism.

## `pub(super) fn apply_io(injection: Injection, point: SubEffectPoint) -> std::io::Result<()…`

[`apply`] for the funnel steps whose only declared mode is `Kill`.

`SubEffectPoint::modes` gives every containment point except
`AmbientJobJoined` kill mode alone, because the packet gives only the
ambient join an error contract to return through ("failure refuses the
write command"). An `Error` here can therefore only come from a hand-written
observer, and it is surfaced as a spawn failure rather than silently
ignored.
