# `src/agent/proc/ambient.rs`

Extended notes for [`src/agent/proc/ambient.rs`](../../../../src/agent/proc/ambient.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The coordinator's ambient kill-on-close Job Object (INV-18), and the
process-wide reclaim scope its cleanup reapers read.

Split out of `src/agent/proc.rs`. Every Win32 call this makes is made for it
by `windows_job`, which stays in the parent beside the private
per-invocation job it owns; the reclaim scope is handed to `termination`,
which stays there too. What lives here is the process-wide half of
containment -- established once, held for the life of the process -- as
against the per-invocation half the parent supervises.

`PR6-LANEF-004`: it states its own lint level rather than inheriting the
funnel's `#![allow]`, and denies all three governed lints. No
`effects/allowlist.toml` row: a denial needs none.

## `use crate::topology::effects::{InjectionMode, SubEffectPoint};`

Both the appliers and the two mode/point vocabularies are consulted only by
the Windows join below; on every other platform this module answers `Ok`
without reaching a containment point at all, so an ungated import here would
be an unused import under the `-D warnings` the Windows leg runs with.

## `use super::windows_job;`

The Win32 half of every call below is made by the parent's `windows_job`,
which stays there beside the private per-invocation job it owns.

## `pub fn join_ambient_job(hooks: &mut dyn SpawnHooks) -> Result<(), UpstrokeError> {`

Join the coordinator's ambient kill-on-close Job Object (INV-18).

> on Windows every host child is a member of the coordinator's ambient
> kill-on-close Job Object from creation

enforced by "ambient job joined at write-command startup (refusal
otherwise)". Idempotent: the job is a process-wide singleton established
once and held for the life of the process, because a handle that is ever
closed deliberately would terminate every member — including, since the
coordinator joins it too, the coordinator.

This closes the window the private per-invocation job cannot: between
`CreateProcess` and `AssignProcessToJobObject` the child belongs to no
private job, so a coordinator killed in that window used to leave a
suspended stub with no owner. A child created by an ambient-job member is a
member at creation, so there is no such window.

On Unix this does nothing and says so: containment there is the isolated
process group and the per-invocation reaper, and the packet declares
`AmbientJobJoined` a Windows point
(`decisions.effect_site_inventory.containment_sub_effects`). The hook is not
consulted here either — recording a Windows containment point as executed
on a Unix host would let a Linux CI cell claim Windows coverage.

### Errors

[`UpstrokeError::Refused`] with a diagnostic when the job cannot be created
or joined. The caller refuses the write command before any effect.

## `pub(super) fn join_ambient_job_with(`

[`join_ambient_job`] over an explicit join step.

The parameter exists because the real one cannot fail twice: `join_ambient`
memoises its answer in a process-wide `OnceLock` — it must, since the
coordinator joins exactly one ambient job for its whole life — so a test
binary that has ever joined successfully can never again observe a failure,
and one that observes a failure can never join. The suite's only ambient
failure was therefore the *injected* one, which fires **before** this step
and so proves nothing about what this function does with a real error: the
call could be `let _ = windows_job::join_ambient();` and every test would
still pass while `run` and `resume` dispatched with no ambient job at all.

### Errors

[`UpstrokeError::Refused`] carrying `join`'s own diagnostic.

## `apply(`

The error-return coordinate is *before* the join: the point's error
contract is "failure refuses the write command", so an injected failure
stands in place of establishing the job rather than following a job that
was in fact established. A refusal here leaves no ambient job, no child,
and nothing to reclaim.

## `apply(`

The kill coordinate is *after* it, because that is where the point's own
claim is true: "a coordinator kill after any of these leaves no host
process (the ambient handle closes and the kernel terminates the stub or
tree)". Injected before the join there would be no handle to close, and
the observation would sit on the wrong side of the sub-effect it names.

## `pub fn join_ambient_job(_hooks: &mut dyn SpawnHooks) -> Result<(), UpstrokeError> {`

See the Windows implementation. On Unix this is a no-op that returns `Ok`.

### Errors

Never on Unix.

## `pub const AMBIENT_REFUSAL_PREFIX: &str = concat!(`

The opening words of every ambient-job refusal, so a caller and a test can
recognise one without matching on a whole sentence.

## `pub const AMBIENT_REFUSAL_SIMULATED: &str = concat!(`

The tail of the refusal an injected join failure produces.

## `pub fn process_alive(pid: u32, creation_time: u64) -> bool {`

Whether the process `pid` created at `creation_time` is still running.

The pid alone is not an identity — Windows reuses pids — so both halves are
checked, and "running" is `WaitForSingleObject` timing out rather than an
exit code, because a job-terminated process's exit code is not ours to
predict. A pid that cannot be opened, or that opens onto a process created
at another time, is not this process.

## `pub fn process_creation_time(pid: u32) -> Option<u64> {`

When the process `pid` was created, as a raw FILETIME, or `None` if it
cannot be opened.

## `pub fn ambient_job_established() -> bool {`

Whether this process has joined its ambient Job Object.

## `pub fn child_in_ambient_job(pid: u32) -> Option<bool> {`

Whether `pid` is a member of this process's ambient Job Object, or `None`
when there is no ambient job or the process cannot be opened.

INV-18's claim, asked of the kernel: "every host child is a member of the
coordinator's ambient kill-on-close Job Object from creation".

## `pub(crate) fn poison_ambient_for_tests(message: &str) -> bool {`

Memoise an ambient establishment **failure**, before anything has joined.

Test-only, and it spends this process's one ambient cell — so it belongs
only in a subprocess helper. It exists because the failure it plants is the
one no machine can produce on demand: `CreateJobObjectW` and
`AssignProcessToJobObject` succeed on a working Windows host, and the memo
means a process only ever sees one answer. Returns whether the cell was
still free.

## `pub fn set_container_reclaim_scope(`

Arm this process's cleanup reapers to kill the coordinator's labeled
containers when the coordinator dies, or disarm them with `None`.

`decisions.admission_and_leases.permits.os_matrix`, in full:

> Linux and macOS (`cfg(unix)`): the cleanup reaper survives coordinator
> death, settles the dead coordinator's process groups while holding R28,
> and **additionally kills the dead coordinator's labeled containers**,
> closing the orphan window; Windows: **no reaper**; … containers are
> reclaimed at the **next write-command start** (orphan window until then;
> documented; a portable watchdog is deferred).

So this is a **no-op on Windows**, and that is the documented half rather
than an omission: [`crate::runner::container::orphan_window`] is the value
that says so and `runner::container::tests::windows_orphan_window_documented`
is what asserts the platform and the code agree.

The scope is read **before** the fork, by every reaper started after this
call: a reaper already running keeps the scope it was started with, because
it is a `fork`-only child that cannot be handed anything afterwards.

### Errors

Whatever building the argument vectors returns — on Unix, a scope whose
rendered strings carry an interior NUL.
