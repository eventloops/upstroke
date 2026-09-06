# `src/agent/proc.rs`

Extended notes for [`src/agent/proc.rs`](../../../src/agent/proc.rs).

[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/agent/proc.rs).

The code defines current behavior. These notes preserve contracts and implementation
history. Search each backticked heading fragment separately in the source.

References below to `decisions.*` and `INV-18` use retired v0.2 planning identifiers.
They record implementation history and do not add current requirements.
[DESIGN.md](https://github.com/eventloops/upstroke/blob/master/DESIGN.md#retired-records)
is the living design authority.

## Module

Subprocess supervision: run a command, feed stdin, drain both pipes
concurrently (required on Windows — a full pipe buffer deadlocks a child
that is still writing), and enforce a wall-clock timeout. The synchronous
runner remains until the Tokio scheduler arrives in v0.2.

Windows subtleties this module owns: `.cmd` shims (npm installs) mean the
direct child is `cmd.exe`, so every invocation is placed in a private Job
Object before its suspended primary thread is allowed to execute. Closing
that handle kills ordinary descendants even when the direct child exits
successfully or Upstroke is terminated. Explicit cleanup uses the same job
and a bounded wait; it never shells out to a PID-based tree walker. Any
process that inherited a pipe handle must not be able to stall the drain.
Parent endpoints use nonblocking I/O. After a bounded grace, collection
releases and joins each worker before taking its captured output.

Unix subtleties are the mirror image: each invocation gets an isolated
process group so a timeout can kill every member, but that isolation
also stops terminal interrupts reaching the child automatically. A tiny
process-wide signal monitor below preserves inherited ignored and custom
handlers,
coordinates SIGINT/SIGTERM/SIGHUP/SIGQUIT termination, and proxies terminal
suspension/continuation. It waits out any spawn-registration race, blocks
launches across a suspension transition, and uses a descriptor-scrubbed
guard process to close the last signal-to-stop race. A separate cleanup
reaper survives even an uncatchable Upstroke SIGKILL. Together the monitor and
reaper stop and clean every active process group before ownership is
released. A host runner does not claim to contain code that deliberately
leaves that group with `setsid`/`setpgid`; the external/container runner
described in DESIGN.md is the boundary for hostile or daemonising repository
code. Pretending otherwise would require racy process-table inference on
macOS, where there is no unprivileged descendant-containment primitive.
Within the host-runner contract, run ownership cannot be handed to a resume
-- or appear suspended -- while an isolated agent group is running.

## `#![allow(`

PROCESS FUNNEL: this module is in the **funnel section** of
`effects/allowlist.toml`. Its effectful entries take ProcessSite by value;
`runner::host` constructs commands and passes both Spawn and Terminate sites
into this supervision boundary. `decisions.effect_site_inventory.mechanism` (2).

## `mod hooks;`

The observation and injection surface, out of line in `proc/hooks.rs`.

Private module, explicit re-exports: `crate::agent::proc::SpawnHooks` and
`crate::agent::proc::NoHooks` keep the exact paths and the exact
visibilities they had inline, and the two appliers keep theirs -- visible in
`proc` and its descendants, which is what a private item of `proc` was.
[`memoised_outcome`] stays here: it is the funnel's own no-degraded-mode
contract rather than part of the injection surface, `windows_job` reaches it
by that name, and the arm it decides is exercised on every platform.

## `use self::hooks::apply;`

Each applier is reached only from the arm that has containment points to
apply an answer at: the four Unix ones inside the bounded supervision entry
point below, and `windows_job`'s three. An ungated import here is an unused
one on the other platform, which `-D warnings` makes an error.

## `pub(crate) fn memoised_outcome<T>(memo: &Result<T, String>) -> Result<(), String> {`

What a **memoised** one-shot establishment reports to a caller.

A `OnceLock` holding a `Result` has exactly two arms and one of them is not
otherwise reachable in a test: the coordinator joins one ambient job for its
whole life, so a process that memoised a success can never observe a failure
and a process that memoised a failure never got a coordinator. Every ambient
failure this suite can build is the *injected* one, which fires strictly
before the memo is consulted — so `Err(_) => Ok(())` here left the whole
suite green while a Windows coordinator whose `CreateJobObjectW` failed
carried on into `run`/`resume` with no ambient kill-on-close job at all: the
degraded mode `crash_reconstruction` forbids ("no degraded mode; deferred")
and `expected_failures_refusals[1]` requires a startup refusal for
(`PR5-CORRECTNESS-010`).

Generic and platform-independent **so that arm can be executed on any
machine**. The value it decides about is Windows-only; the decision is not,
and a decision only one platform can test is a decision one platform never
tests.

### Errors

The memoised diagnostic, verbatim — the caller renders it into the refusal,
so a *fresh* message here would name something that did not happen.

## `pub(crate) fn memoised_outcome<T>(memo: &Result<T, String>) -> Result<(), String> {`

Unix has no ambient job and therefore no production caller; the test in
`proc/tests.rs` is the only one there, and running it there is the point.
`dead_code` is not a governed lint (`effects::GOVERNED_LINTS`), so this is
outside the allow-placement scan rather than an exception to it.

## `pub struct ProcessOutput` › `pub code: Option<i32>,`

Exit code if the process exited normally; `None` when killed for a
timeout/output limit or terminated by a signal.

## `pub struct ProcessOutput` › `pub duration: Duration,`

Wall clock from spawn to process exit (not including pipe drain).

## `pub struct ProcessOutput` › `pub output_limited: bool,`

The child exceeded the bounded stdout or stderr capture allowance and
its owned process tree was terminated.

## `const DRAIN_GRACE_EXIT: Duration = Duration::from_secs(2);`

How long to keep draining pipes after the process is gone. Normally EOF is
immediate; the grace only caps the pathological case of an orphaned
grandchild still holding a write handle.

## `const OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;`

Per stream. Readers continue draining after this point so the child cannot
block on a full pipe while the supervisor notices and terminates its tree.

## `struct ProcessTree {`

A direct child plus the platform primitive that owns its ordinary
descendants. Keeping ownership beside `Child` prevents a successful wait
from accidentally bypassing tree settlement.

## `impl ProcessTree` › `fn finish_direct_exit(&mut self) -> Result<(), UpstrokeError> {`

The direct child has already exited. Windows descendants remain job
members, so terminate and observe the job empty before returning its
status. Unix process-group settlement is owned by `termination`.

## `pub fn run_with_timeout_at(`

Run `command` through the site-authoritative process funnel, with its
containment sub-effect points observable.

`spawn_site` and `terminate_site` are validated before any process effect;
timeout and output-limit cleanup carry the validated termination site into
the platform primitive. `stdin_data` is bytes because a
[`crate::runner::CommandSpec`] carries bytes.
Output collection has a bounded post-exit grace. An escaped descendant
holding a writer can leave partial stdout or stderr; [`ProcessOutput`]
does not expose whether either reader reached EOF.

### Errors

Spawn failure, supervision failure -- among them a reader thread the OS
refused, a stream whose read failed rather than ended, or a stdin write
failure other than the child's broken-pipe refusal, or a fault the observer
injected. Pipe worker panics are supervision failures too.

## `let mut termination = termination::Supervisor::begin(terminate_site)?;`

Enter before `spawn`: if an interrupt arrives in the narrow interval
between creating the child and learning its pid, the signal monitor
waits for this registration rather than terminating Upstroke first and
orphaning the new process group.

## `apply(`

`Spawn.ReaperStarted`: "fork of the per-invocation reaper, which takes
its shared cleanup hold R28". `begin` returning Ok is exactly that
having happened, and nothing else in this function can have happened
yet.

## `apply(`

`Spawn.PreExecPgidAndRegister`. Two coordinates, and they are not the
same one:

* The **operation** is in the forked child before `exec` — `setpgid(0,0)`
  and the reaper registration, in `termination::Supervisor::prepare`'s
  `pre_exec` closure. That is where the packet puts it ("in the child
  before exec") and where it is.
* The **injection** is here, parent-side, immediately after `spawn`
  returns `Ok`. This point's only declared mode is `Kill`
  (`SubEffectPoint::modes`), a kill is a *coordinator* death, and the
  packet's claim for it — "a coordinator kill after any of these leaves a
  group the reaper settles while holding R28" — is true only once the
  child exists and its group does. A kill delivered inside the forked
  child would end the fork, not the coordinator, and would leave no group
  at all. An observer hook cannot run there in any case: after `fork` in a
  multithreaded process only async-signal-safe calls are permitted, and
  every real observer locks and allocates. The packet contemplates
  exactly this: "these are parent-side **or** pre-exec points the harness
  controls".

Fired unconditionally, because `spawn` returning `Ok` *is* the evidence
the closure ran: `std` reports a `pre_exec` error through the child's
CLOEXEC status pipe and returns `Err`. The kernel oracle
(`child_leads_its_own_group`) is a second, independent witness and lives
in the tests — as a guard here it could only ever produce a false
negative, silently dropping the point for a child that left its own group
after `exec` (DESIGN.md:398-402 puts such a process outside host
guarantees; it does not make it invisible).

## `apply(hooks.point(SubEffectPoint::Exec), SubEffectPoint::Exec)?;`

`Spawn.Exec`: `Command::spawn` reports a failed `execvp` through its own
CLOEXEC status pipe and returns `Err`, so reaching here is the exec
having succeeded.

## `drop(termination);`

Drop the pre-exec reaper first: it still has an anchor pinning this
child's group identity and will kill every member before returning.

## `apply(`

`Spawn.Registered`: "parent-side registration".

## `let stdin_bytes = stdin_data.to_vec();`

Feed stdin from its own thread: the child may not read stdin until it
has written output, and this thread must not block the pipe drains.

## `let _ = pipe.write_all(&stdin_bytes);`

A child that exits without reading stdin breaks the pipe; that
is its prerogative, not an error.

## `if let Err(error) = termination.finish() {`

Leave the exited leader as a zombie until cleanup completes:
its PID pins the PGID, so no unrelated group can reuse the
numeric id between observation and the final signal.

## `let stdin_deadline = Instant::now() + grace;`

Bounded like the read drains: a prompt larger than the pipe buffer plus
an orphan holding the read end would otherwise block write_all forever
and hang the supervisor past its own timeout. Abandoning the thread is
safe — it owns its handle and exits when the last reader closes.

## `mod drain;`

The pipe reader, out of line in `proc/drain.rs`, with the predicate the
supervisor asks it for. Both are visible in `proc` and its descendants,
exactly as they were when they were private items of this file.

## `fn kill_tree`

Kill the whole process tree. Killing only the direct child is not enough
when it is a `cmd.exe` shim: the real agent process would survive, keep
running, and keep the pipes open.

## `pub(crate) fn child_leads_its_own_group(pid: u32) -> bool {`

Whether `pid` leads its own Unix process group: the decision of one
`observe_child_group` record, for callers that want only the answer.

The independent witness that `Spawn.PreExecPgidAndRegister`'s operation ran
in the forked child. Asks the kernel, not this crate: the child leads its
own group exactly when the pre-exec closure's `setpgid(0, 0)` ran, and a
process's group at death is the group it was given before `exec`, because
neither `sh` nor this test binary moves itself afterwards.

Test-only on purpose: as a production guard it could only ever *withhold*
the point, never add information (see the comment at the injection
coordinate).

## `pub(crate) struct GroupObservation {`

One look at a child's process group, with the lifecycle around it, so a
false answer carries its own explanation. In order taken: `exited_before`
(`waitid` with `WEXITED | WNOHANG | WNOWAIT`, the supervisor loop's own
question, which reaps nothing), `group` (`getpgid`, or the errno it failed
with), on macOS `zombie_group` (`proc_pidinfo` with
`PROC_PIDT_SHORTBSDINFO` and the non-zero argument that asks for an
exited, unreaped record, or its errno), then `exited_after`. `Display`
renders the whole record, and every assertion that used to print
`[false]` prints it.

### `pub(crate) fn leads_own_group(&self) -> bool {`

`getpgid` answered: it leads its group when the answer is its own pid. On
XNU an exited, unreaped child does not answer: `proc_exit` moves the
process onto the zombie list and marks it invisible to `proc_find`, which
`getpgid` uses, so the call fails with `ESRCH` while the pid is still
pinned by the zombie. Linux keeps answering for a zombie. Measured on
the runner CI uses (macOS 26, `macos-26-arm64` image 20260831.0337.3,
run 34001563243, job 101401235904, at `65d3df5`): "pid 16222: before the
look exited, unreaped; getpgid failed: No such process (os error 3);
proc_pidinfo(PROC_PIDT_SHORTBSDINFO, zombie) answered pgid 16222 status
5; after the look exited, unreaped" — status 5 is `SZOMB`. So on macOS
alone, `ESRCH` falls through to the exited record, whose `pbsi_pgid` is
the `p_pgrpid` the process died with; any other errno, or no record, is
`false`. `a_child_left_in_this_processs_group_never_answers_for_its_own`
holds the fall-through honest: the record of a child nothing moved names
this process's group, not its own.

The fall-through reads that record only for this process's own dead
child. `exited_before` must be `Ok(true)` and `pbsi_status` must be
`SZOMB`, or the answer is `false` whatever group the record names.
`PR173-DARWIN-FALL-THROUGH-ACCEPTS-A-LIVE-RECORD`: without those two the
decision was `pbsi_pgid == pid` on whatever record came back, and the
non-zero third argument of `proc_pidinfo` only ENABLES the zombie lookup
— the call asks `proc_find` first (`bsd/kern/proc_info.c`), so it can
answer for a live process. A child that missed containment, exited and
was reaped by an embedding host's wildcard wait (DESIGN §15 names that
host) leaves a pid that XNU may reuse; `getpgid` then answers `ESRCH`
for the stranger holding it, the record is the stranger's, and a
stranger that leads group `pid` read as containment for a child this
process no longer has. `waitid(P_PID, ..., WNOWAIT)` answers only for
this process's own child and the zombie it answers for pins the pid, so
`exited_before == Ok(true)` is what a reused pid cannot produce; `SZOMB`
is what a live record cannot. Every other platform and every other
errno are unchanged.

This is the standing macOS red `W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-
FINGERPRINT`: `every_role_reaches_the_containment_points_of_this_platform`
looks at the child in `child_created`, right after `spawn` returns, and a
child that has already run to exit by then read as "not leading its own
group" — every sighting on one of the three roles whose child is this test
binary running no test, none on the two whose child is `sh -c 'exit 0'`.
The observation was wrong, not the containment: the pre-exec `setpgid`
had run, or `spawn` would have returned `Err`.

### `pub(crate) fn had_exited_before_the_look(&self) -> bool {`

Whether the child was already an exited, unreaped zombie when `group` was
asked. A test that wants to witness the exited case asserts this first, so
a child that outlived the look cannot pass it for the wrong reason.

## `pub(crate) fn observe_child_group(pid: u32) -> GroupObservation {`

Takes the record above, in the order its fields are listed. A pid that no
`pid_t` or `id_t` can carry yields a record of `EINVAL`s with pid `-1`,
which leads nothing.

## `fn zombie_group_answer(pid: libc::pid_t) -> Result<ZombieGroupAnswer, i32> {`

The macOS exited-record query. The non-zero third argument of
`proc_pidinfo` is what `group_has_non_zombie_members` already passes: it
selects `proc_find_zombref`, which finds a process that has started
exiting and not yet been reaped. A short read is `EIO`, a failed one its
errno.

## `pub(crate) fn await_exit_without_reaping(pid: u32, budget: Duration) -> Result<(), String> {`

Blocks until the child has exited, leaving it a zombie for its owner to
reap: `waitid(P_PID, pid, WEXITED | WNOWAIT)` on a helper thread, the
result handed back over a channel with `recv_timeout(budget)`. The exit
itself is the signal, so no test sleeps to guess at it; the budget bounds
a wedged child, not a healthy one. On a timeout the thread stays in its
wait until the caller kills and reaps the child, which ends it with
`ECHILD`; its `send` then finds no receiver, and that discarded result is
the whole of its failure path. The one caller of that path kills, waits
and panics with the budget.

## `fn exited_unreaped(pid: libc::id_t) -> std::io::Result<bool> {`

The body `child_exited_unreaped` had, by pid, so the supervisor loop and
`observe_child_group` ask the kernel the same question the same way.

## `mod ambient;`

The ambient Job Object and the reclaim scope, out of line in
`proc/ambient.rs`. Private module, explicit re-exports: every path under
`crate::agent::proc::` that named one of these before the split names the
same item now, at the same visibility.

## `mod windows_job` › `pub(super) struct Job {`

A non-inheritable Job Object configured before any supervised code can
run. The OS closes this handle on abrupt conductor death, and
KILL_ON_JOB_CLOSE then terminates every ordinary descendant.

## `mod windows_job` › `fn real_create_job() -> HANDLE {`

The real `CreateJobObjectW`, as [`Job::create`] passes it.

## `mod windows_job` › `fn real_configure_job(`

The real `SetInformationJobObject`, as [`Job::create`] passes it.

## `mod windows_job` › `fn real_terminate_job(handle: HANDLE) -> i32 {`

The real `TerminateJobObject`, as [`Job::terminate_and_wait`] passes it.

## `mod windows_job` › `fn real_query_accounting(`

The real `QueryInformationJobObject`, as the accounting callers pass it.

## `impl Job` › `fn create_with(`

[`Job::create`] over the two Win32 calls it makes.

The same reason `create_ambient` takes its assignment call: on a
working machine `CreateJobObjectW` and `SetInformationJobObject`
always succeed, so both failure branches are unreachable in every
real test and either could be inverted with the whole suite green —
while `crash_reconstruction`'s "if the ambient job cannot be
**created** or joined the write command refuses at startup" and
INV-18's "refusal before any effect if the ambient job cannot be
established" silently stopped holding. The join had a seam; these
two did not, which made the guarantee asserted for one third of the
sentence that states it.

`configure` is handed the limit structure rather than a raw
pointer, so a test can also read what is being asked for:
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the whole fail-safe, and a
job configured with any other flag would still return success here.

## `impl Job` › `return Err(io::Error::last_os_error());`

`job` drops here, closing the handle: an unconfigured job is
not a job this process may keep.

## `impl Job` › `pub(super) fn terminate_and_wait_with(`

[`Job::terminate_and_wait`] over the Win32 calls it makes.

DESIGN.md:402 — "Direct-child success and timeout both terminate
and **boundedly observe that job empty**". Both halves of that
sentence are unobservable from outside on a working machine: a real
job empties immediately, so an implementation that skipped the
observation entirely, and one that observed without a bound, both
return promptly and leave nothing behind for a test to see. The
accounting seam is what makes "observe" and "bounded" separate
facts.

## `impl Job` › `pub(super) fn active_processes_with(`

How many processes the job still holds, over the Win32 call that
answers.

R22's release is "released on exit, timeout kill, cancel, or
shutdown (private Job Object / process group)", and this is the only
thing that reports whether the release happened. A query error read
as "empty" would report a job settled while it still held a live
member, so the error branch is the accounting, not an aside — and it
is unreachable without a seam.

## `impl Job` › `pub(super) fn contains(&self, pid: u32) -> Option<bool> {`

Whether `pid` is a member of **this** job, asked of the kernel.

The Windows counterpart of `child_leads_its_own_group`, and
test-only for the same reason: an independent oracle for the
private job's identity, not a production guard. `IsProcessInJob`
answers from the process table, so it cannot agree with a spawn path
that never assigned anything.

## `mod windows_job` › `pub(super) fn real_assign_to_job(job: HANDLE, process: HANDLE) -> i32 {`

The real `AssignProcessToJobObject`, as [`spawn_suspended_in_job`]
passes it.

## `mod windows_job` › `pub(super) fn job_contains(job: HANDLE, pid: u32) -> Option<bool> {`

Whether `pid` is a member of the job `job`, asked of the kernel.

See [`Job::contains`]; this is the same query for a handle a test
captured through the assignment seam rather than for a live [`Job`],
because constructing a second `Job` over the same handle would close it
on drop.

## `mod windows_job` › `pub(super) fn spawn_suspended_in_job_with(`

[`spawn_suspended_in_job`] over the two Win32 steps that come after
creation.

Both always succeed on a working machine, so the two cleanup branches
that follow them — terminate the private job, kill the child, wait for
it — are unreachable in every real test, and R22's "created as an
ambient-job member, so a coordinator death at any spawn sub-step incl.
the create-suspended prefix terminates it" was asserted for the ambient
job and not for the spawn path's own recovery.

`assign` is also what makes the `PrivateJobAssigned` coordinate
checkable: it hands a test the private job's handle at the instant the
assignment is made, so the hook can be measured against the operation it
is named for rather than against the other hooks.

## `mod windows_job` › `if let Err(error) = apply_io(`

`Spawn.CreatedSuspended`: "the child is already an ambient-job
member". This is the window the ambient job exists to close -- a
coordinator killed here leaves a suspended process that no private
job owns -- so it is where the kill injection goes.

## `mod windows_job` › `let assigned = assign(job.handle, child.as_raw_handle() as HANDLE);`

The primary thread is still suspended, so candidate code cannot
create an escaping child between process creation and assignment to
the job.

## `mod windows_job` › `static AMBIENT: OnceLock<Result<AmbientJob, String>> = OnceLock::new();`

The coordinator's ambient kill-on-close Job Object.

A process-wide singleton, and never dropped: `OnceLock` in a `static`
has no destructor, so the handle survives to process exit. That is the
requirement, not an accident -- the coordinator is itself a member, so
closing this handle terminates the coordinator.

## `mod windows_job` › `struct AmbientJob(HANDLE);`

The ambient job's handle, held for the life of the process.

A separate type from [`Job`] and not merely a second value of it,
because the two have opposite ownership rules. `Job` is owned by the
thread supervising one invocation and its `Drop` is load-bearing --
closing it is how a timeout settles the tree. This one is shared by
every thread and must never be closed, so it has no `Drop` at all.

## `mod windows_job` › `pub(super) fn join_ambient() -> Result<(), String> {`

Create the ambient job and put this process in it, once.

The memo is decided by [`super::memoised_outcome`] rather than by a
`match` here, because that arm is unreachable in this process once
either answer has been taken. See its documentation.

## `mod windows_job` › `pub(super) fn poison_ambient_for_tests(message: &str) -> bool {`

Memoise an ambient **failure** before anything has joined, so a test
process can carry a real one through [`join_ambient`].

Spends the process's one ambient cell, so it belongs only in a
subprocess helper. Returns whether the cell was still free.

## `mod windows_job` › `fn create_ambient(assign: impl Fn(HANDLE, HANDLE) -> i32) -> Result<AmbientJob, String> {`

The body of [`join_ambient`], over the assignment call it makes.

`assign` is a parameter for one reason: `AssignProcessToJobObject`
returns a Win32 `BOOL`, where **zero is failure and every other value
— including `-1` — is success**, and on a working machine it always
returns success. So the branch that reads it is unreachable in every
real test, and `if joined == 0` could be `if joined == -1` with the
whole suite green while `crash_reconstruction`'s "if the ambient job
cannot be created or joined the write command refuses at startup"
silently stopped holding.

Not memoised, and it does not touch [`AMBIENT`]: a test may call this
with a refusing `assign` without spending the process's one ambient
job.

## `mod windows_job` › `fn create_ambient_with(`

[`create_ambient`] over the job it creates as well as the assignment.

`crash_reconstruction` names two failures and this slice's contract
names them together — "ambient job cannot be **created** or joined
(Windows) → write command refuses at startup with a diagnostic". The
join half had a seam and the creation half did not, so the branch that
turns a failed `CreateJobObjectW` or `SetInformationJobObject` into a
refusal was unreachable: `create_ambient` could have returned a disabled
job and continued, and the whole suite would have stayed green while the
coordinator ran with no ambient job at all.

## `mod windows_job` › `return Err(format!(`

`job` drops here, closing the handle: a kill-on-close job
with no members terminates nothing.

## `mod windows_job` › `let job = std::mem::ManuallyDrop::new(job);`

Joined. From here the handle must outlive every `Drop` in this
process, because closing it terminates this process.

## `mod windows_job` › `pub(super) fn ambient_established() -> bool {`

Whether the ambient job has been established in this process.

## `mod windows_job` › `pub(super) fn ambient_contains(pid: u32) -> Option<bool> {`

Whether `pid` is a member of this process's ambient job.

`None` when no ambient job has been established, the process cannot be
opened, or `IsProcessInJob` itself fails. The kernel answers, so this is
an oracle independent of the spawn path it checks.

## `mod windows_job` › `struct OpenHandle(HANDLE);`

A borrowed process handle with query and synchronise rights.

## `fn process_alive` › `return false;`

The pid was reused: whatever is running under it now is not the
process the caller asked about.

## `mod windows_job` › `pub(super) fn primary_thread_suspend_count(process_id: u32) -> io::Result<u32> {`

How many outstanding suspends the child's primary thread carries.

The Windows counterpart of `child_leads_its_own_group`: an oracle for
"is this child still suspended" that asks the kernel rather than the
crate, so the `CreatedSuspended`, `PrivateJobAssigned` and `Resumed`
coordinates can be measured against the operations they name instead of
against each other. `SuspendThread` returns the count *before* its own
increment and the matching `ResumeThread` puts it back, so the
observation leaves the child exactly as it found it.

Test-only, like the Unix one and for the same reason: as a production
guard it could only ever withhold a point it cannot add information to.

## `mod windows_job` › `fn primary_thread(process_id: u32) -> io::Result<Snapshot> {`

A suspend/resume handle on `process_id`'s primary thread.

## `mod tests` › `fn the_ambient_join_reads_a_win32_bool_the_way_win32_defines_one() {`

`AssignProcessToJobObject` answers with a Win32 `BOOL`: **zero is
failure and every other value is success**, `-1` included.

Every real assignment on a working machine succeeds, so the branch
that reads this value is unreachable in an ordinary test and
`if joined == 0` could become `if joined == -1` with the suite
green — while an actual refusal (an outer job with UI restrictions,
a job the process may not join) was read as success and startup
returned `Ok` holding an ambient job with no members. The
coordinator would then take workspace effects and spawn children
that no ambient job owns, which is the whole of INV-18's host
portion.

The expected mapping is Win32's, written here, not read from the
code under test.

## `fn the_ambient_join_reads_a_win32_bool_the_way_win32_defines_one` › `for value in [1_i32, -1, i32::MIN, i32::MAX] {`

Every other value is success. Each of these creates a real job
object this process is deliberately *not* a member of; the
handle is left open exactly as the real ambient one is, and a
kill-on-close job with no members terminates nothing.

## `mod tests` › `fn the_ambient_job_refuses_when_it_cannot_be_created_or_configured() {`

The other two thirds of the sentence the join test covers.

`expected_failures_refusals[1]` is "ambient job cannot be
**created** or joined (Windows) → write command refuses at startup
with a diagnostic", and INV-18's host portion is "refusal before any
effect if the ambient job cannot be **established**". Establishing
is three Win32 calls, not one: `CreateJobObjectW`,
`SetInformationJobObject`, `AssignProcessToJobObject`. Only the last
had a seam, so the first two could each have been ignored — an
ambient job that was never created, or one created without
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and therefore with no fail-safe
at all — with the suite green.

Both failures are unreachable on a working machine, which is why
they need the seam rather than a fixture.

## `fn the_ambient_job_refuses_when_it_cannot_be_created_or_configured` › `let configured = Cell::new(false);`

A job that cannot be created is not configured and not joined.

## `fn the_ambient_job_refuses_when_it_cannot_be_created_or_configured` › `let refused = create_ambient_with(`

A job that cannot be configured is refused, not kept: without
KILL_ON_JOB_CLOSE the ambient job terminates nothing on
coordinator death, which is the whole of INV-18's host portion.

## `mod tests` › `fn every_job_this_module_creates_is_configured_to_kill_on_close() {`

What `SetInformationJobObject` is actually asked for.

`KILL_ON_JOB_CLOSE` is the mechanism DESIGN.md:402 names — "abrupt
conductor death closes its non-inheritable handle and lets the
kernel terminate ordinary descendants" — and a job configured with
any other limit flag would still return success. The expected flag
and the expected structure size are Win32's, written here rather
than read back from the call under test.

## `mod tests` › `fn a_failed_accounting_query_is_never_read_as_an_empty_job() {`

An accounting error is an error, never an empty job.

R22 releases the host process "on exit, timeout kill, cancel, or
shutdown (private Job Object / process group)", and
`QueryInformationJobObject` is the only thing that reports whether
that release happened. Reading a failed query as zero would report a
job settled while it still held a live member — the accounting
saying "released" over a resource that is not.

## `fn a_failed_accounting_query_is_never_read_as_an_empty_job()` › `for reported in [0_u32, 1, 7] {`

And a query that answers is believed, whatever it answers.

## `mod tests` › `fn cleanup_polls_the_accounting_until_the_job_is_empty() {`

Cleanup **observes** the job empty; it does not assume it.

DESIGN.md:402 — "Direct-child success and timeout both terminate and
boundedly observe that job empty". A real job empties by the first
query, so an implementation that skipped the loop entirely is
indistinguishable from this one on any real tree. The accounting
responses here are chosen, not observed: 1, 1, 0.

## `mod tests` › `fn cleanup_gives_up_on_a_job_that_never_empties() {`

And the observation is **bounded**.

A job that never reports empty must produce a diagnostic within the
documented two seconds rather than pinning a supervisor thread for
the life of the process. The bound is asserted from outside, on a
worker thread, so an unbounded loop fails this test with a named
message instead of hanging the whole binary.

## `fn child_exited_unreaped` › `Ok(matches!(`

WEXITED should filter non-terminal transitions, but Darwin can leave a
stopped/continued record observable around job-control delivery. Never
turn such a record into permission for the reaper to SIGKILL the group.

## `mod termination {`

Process-wide Unix termination coordination.

A signal handler may only perform async-signal-safe work. It therefore
stores the first terminating signal in an atomic and returns. A detached
monitor thread owns the locks, termination, and job-control forwarding,
then restores the default disposition and re-raises a terminating signal.
`spawning` closes the otherwise unavoidable race between `Command::spawn`
and pid registration; the monitor cannot terminate or suspend the parent
while it is nonzero.

## `mod termination` › `const GUARD_POLL_SLICE_MS: libc::c_int = 250;`

The guard's wait is sliced rather than open-ended, so that every slice
can end in a `getppid` check the way `reaper_loop`'s 10 ms slice does.
`poll` on a Darwin FIFO reports data and never the last writer's close,
which `fn a_helper_that_has_already_exited_ends_the_acknowledgement_wait_at_end_of_file`
measured: a conductor's death closes the guard's command and wake pipes
without waking a guard that waits with no timeout, and on a macOS host
that left one orphaned guard per run until the machine restarted.
Reparenting is the liveness signal that holds on both platforms, and a
slice is what lets the guard ask for it. The value is the cadence the
stopping probe already ran at, so that pulse is unchanged and an idle
guard costs four `getppid` calls a second.

## `mod termination` › `const REAPER_RESUME_STABLE_POLLS: u8 = 50;`

The job-control guard briefly continues only Upstroke every 250 ms while
probing for a PID-directed termination. The cleanup reaper must not
mistake that internal pulse for an operator resume and continue agents.
Genuine SIGCONT is forwarded immediately by the monitor; this bounded
fallback exists for host-owned signal policies the monitor preserves.

## `mod termination` › `const HELPER_READY_BUDGET: Duration = Duration::from_secs(2);`

How long a forked helper is given to acknowledge that it has started.

Master's value, named rather than repeated at each wait. This budget
is deliberately unchanged: row
`PR125-CLOSE-MACOS-READY-RED-CAUSE-UNKNOWN` records that a larger one
did not help at the exact head that fails. The reason it could not
help is now known and is `wait_readable`'s section below: on Darwin
the wait never saw the helper end, so every early death cost the whole
budget, whatever the budget was. The budget bounds a helper that is
still running and silent; a helper that has ended is seen at once.

## `mod termination` › `const HELPER_ABORT: u8 = 0x71;`

The first byte of a helper's setup-failure report, distinct from every
READY, OK, FAIL and guard command byte. A helper that cannot finish its
setup writes one `SETUP_FAILURE_FRAME_LEN`-byte frame on the
acknowledgement pipe it already owns, then ends with status 1 as it
always did. The frame is this marker, the `SetupStep` that failed, the
position of the lease it was working on, and the errno the call left. The frame is one
`write` of fewer than `PIPE_BUF` bytes, so it arrives whole or not at
all, and it is data: a reader that cannot see the pipe close (Darwin,
below) still wakes for it.

## `struct State` › `spawning: usize,`

Supervisors that entered before spawn but have not registered a pid.

## `struct State` › `groups: Vec<RegisteredGroup>,`

Active isolated process groups. A signal lease pins the numeric
identity until the monitor has delivered its snapshot's signal, so
`finish` cannot reap the leader and expose that id for reuse first.

## `struct State` › `terminating: bool,`

Set by the monitor before it kills groups. No later spawn may begin.

## `struct State` › `suspending: bool,`

Set before a suspend snapshot and cleared only after continuation.
New launches wait outside the lock for the complete transition.

## `struct Guard` › `_command_keepalive_fd: libc::c_int,`

Keep one parent-side reader open so a guard crash turns the next arm
into an acknowledgement EOF instead of delivering SIGPIPE from an
async signal handler that writes the command pipe.

## `pub(super) fn finish(&mut self) -> Result<(), UpstrokeError>` › `self.phase = Phase::Finished;`

`cleanup` consumes and closes the reaper's raw descriptors.
Change phase first so an error return followed by Drop can never
transact on—or close—descriptor numbers another thread may
already have reused.

## `mod termination` › `pub(super) fn registered_groups() -> Vec<i32> {`

The process groups the parent supervisor currently has registered.

Test-only, and an oracle rather than a guard: `Spawn.Registered` is
"parent-side registration", and the only way to ask whether that
happened before the point fired is to read the state it writes.

## `fn drop(&mut self)` › `let _ = self.finish();`

`finish` normally runs while the direct child is still
an unreaped zombie, pinning the process-group identity.
Error unwinding remains fail-closed through the same
external reaper rather than trusting a recycled PGID;
a failure consumes the reaper and arms process exit.

## `fn install() -> Result<Arc<Mutex<State>>, String>` › `if policy.handles_termination(signal) {`

Preserve every launcher-owned policy. POSIX carries SIG_IGN
across exec (`nohup` relies on it), while an embedding host may
have installed a custom in-process handler before calling us.

## `fn install() -> Result<Arc<Mutex<State>>, String>` › `if policy.job_control {`

Preserve every host-owned stop disposition. Each remaining default
terminal stop is proxied, and the policy check above guarantees the
matching default SIGCONT can release the isolated groups again.

## `fn prepare_monitor_signal_mask` › `unsafe {`

An embedding host may have blocked SIGCONT on the thread that first
called Upstroke, and new threads inherit that mask. SIGCONT still wakes
a stopped process when blocked, but its handler cannot run, so the
isolated agent groups would remain stopped forever. Give only the
private monitor thread an unblocked SIGCONT; every host thread keeps
its original mask.

## `mod termination` › `let already_recorded = PENDING_TERMINATION.load(Ordering::SeqCst);`

A stopped process cannot execute a caught termination handler. The
external guard periodically resumes only Upstroke so this handler can
inspect/deliver a PID-directed pending signal; supervised agent
groups remain stopped. With no such signal, stop again from inside
the handler before returning to ordinary parent code.

## `mod termination` › `fn arm_fail_closed_termination(site: &[u8]) {`

Arm fail-closed termination with `SIGTERM`, and say so on fd 2.

Every fallback that gives up on a private helper comes through here so
the process about to die names the site first. `C-004`: the macOS test
harness SIGTERMed itself for a day without a diagnostic — libtest
captures the failing test's panic and the raise pre-empts its report —
and four CI deaths were read backwards as a group-kill that never
happened. One raw `write` survives both, and it is async-signal-safe,
which the guard-probe handler needs. Only the site that wins the arm
writes; a later fallback that finds a termination already pending is
not the cause and says nothing.

## `fn monitor(state: Arc<Mutex<State>>) -> !` › `if !stop_groups(&groups) {`

SIGSTOP cannot be caught or ignored, so a vendor process
cannot keep spending while its visibly foreground Upstroke
parent is suspended. SIGCONT below releases the same groups.

## `fn monitor(state: Arc<Mutex<State>>) -> !` › `SUSPEND_ARMED.store(true, Ordering::SeqCst);`

The guard remains runnable while Upstroke is stopped. It
serializes a late continuation/termination with the actual
SIGSTOP and acknowledges only after a genuine resume. That
closes the final flag-check-to-stop interval.

## `fn monitor(state: Arc<Mutex<State>>) -> !` › `if PENDING_TERMINATION.load(Ordering::SeqCst) != 0 {`

A terminating signal wins over suspension. Do not stop the
parent after its monitor has already been asked to tear down.

## `fn monitor(state: Arc<Mutex<State>>) -> !` › `match guard.stop_parent() {`

The external guard sends SIGSTOP while this monitor is
blocked on its acknowledgement pipe. Therefore the next
instruction cannot run until a real later SIGCONT; queuing a
self-signal here would allow cleanup to race ahead of the
kernel's process-wide stop.

## `impl Reaper` › `fn register_raw(self, pgid: libc::pid_t) -> bool {`

Register from `Command::pre_exec`, where allocation and Rust locks
are forbidden. Launches are serialized, so the shared one-byte
acknowledgement belongs to this registration frame.

## `fn cancel(self)` › `arm_fail_closed_termination(`

The parent does not know whether pre_exec registered a group
before spawn failed. Arm ordinary fail-closed termination;
the independently polling reaper will observe reparenting
and complete any registered cleanup without trusting EOF.

## `impl Reaper` › `fn abandon(self) -> HelperEnd {`

Give up on a reaper that has not said READY.

No agent has been spawned and no group registered — `prepare` runs
only after `begin` has returned — so there is nothing for this
reaper to settle and nothing to fail closed about. Kill it and reap
it; the launch fails with an ordinary error. Arming process-wide
`SIGTERM` here guarded a state that cannot exist yet, and on macOS,
where a forked helper's startup runs long under load, it killed the
test harness with no diagnostic (`C-004`).
Returns what the kill and the wait answered, for the failure
message. The order and the calls are master's; only the two
results are kept rather than discarded.

## `impl Reaper` › `fn close_and_wait_reporting(self) -> (libc::pid_t, libc::c_int, libc::c_int) {`

`close_and_wait`, keeping what the final
`waitpid` answered: the pid it returned or `-1`, the errno it left
in that case, and the status it filled otherwise. The loop, the
descriptors it closes and the order are unchanged.

## `fn spawn_reaper() -> Result<Reaper, String>` › `let exit_before_ready = std::env::var("UPSTROKE_TEST_HELPER_EXIT_BEFORE_READY")`

A helper that ends before it writes READY, so the failure path is
driven with no clock in it at all: the parent's wait ends on the
pipe's end-of-file rather than on its budget, whatever the
scheduler does with either process. `UPSTROKE_TEST_REAPER_READY_
DELAY_MS` above drives the same path through the budget instead,
and depends on the child outrunning it.

Until the Darwin wait was rebuilt on `select` (`wait_readable`), the
first sentence was true on Linux and false on macOS: CI's macOS leg
spent the full two seconds per helper on this seam and reported the
same exit status afterwards, which is how the defect was found
(`a_helper_that_never_acknowledged_reports_what_ending_it_answered`
took ~4 s on macOS against ~6 ms on Linux in run 33987067020).

## `fn spawn_reaper() -> Result<Reaper, String>` › `let containers = container_scope_for_a_new_reaper();`

Rendered BEFORE the fork, like `cleanup_paths` above and for the same
reason: the reaper may not allocate. `None` is the ordinary state of
every run today — nothing selects a container Runner until PR12 — and
costs the reaper nothing at all.

## `fn spawn_reaper() -> Result<Reaper, String>` › `if unsafe { libc::setpgid(0, 0) } != 0 {`

A separate process group is the crucial boundary: an
uncatchable kill of Upstroke's foreground job must not also kill
the process that owns its final agent cleanup.

**The child's call is the only one.** Until PR #172 the parent also
called `setpgid(pid, pid)` right after the fork, "to close the parent's
race with the child-side setpgid; either call may win". On Darwin the
two calls racing on the same new group make the child's fail with
`EPERM` about once in five to twenty thousand forks. Measured on the
macOS runner CI uses (macOS 26.6.2, run 33992718199 on branch
`scratch/darwin-fifo-eof-experiment`, `exp/setpgid_race.c`): with the
parent's call, the child's `setpgid(0, 0)` returned `EPERM` in 1 to 4 of
every 20,000 forks across nine tallies, and never in 120,000 forks
without it; Linux returned no `EPERM` in 60,000 with the parent's call.
The first READY failure carrying the setup report (CI run 33992302665
on PR #172, `engine::tests::second_reviewer_spawn_failure_settles_worker_and_first_review_evidence`)
named exactly this step: "the reaper reported that moving into its own
process group failed: Operation not permitted (os error 1)", after a wait
of 24 µs. The parent's call bought nothing the handshake does not
already give: `begin` returns only after READY, and the child writes
READY only after its `setpgid` succeeded, so the reaper is in its own
group before any agent exists in either shape. In the window between the
fork and the child's call the reaper is still in the parent's group, and
a kill of that group then takes the reaper with it; no agent exists yet
to be left behind, and the launch fails at the READY wait. The
child-side call remains checked, and its failure is reported by step
and errno.

## `fn spawn_reaper() -> Result<Reaper, String>` › `let mut delay_left = ready_delay_ms;`

Test subprocesses can hold READY back past the parent's deadline
so the late-reaper path is driven deterministically.

## `fn spawn_reaper() -> Result<Reaper, String>` › `let how = describe_ready_wait("reaper", wait, &cleanup_paths);`

How the wait ended, in the message: the helper's own report of the
step that refused, the pipe closing with no report, the budget
elapsing with nothing on the pipe, or the wait itself failing. The
report is decoded with the lease paths this launch rendered before
the fork, so the lease a refused `open` or `flock` names is the path,
not a position.

## `fn spawn_reaper() -> Result<Reaper, String>` › `let end = describe_helper_end(reaper.abandon());`

The teardown is master's, unchanged and in master's order; what
it answered becomes the diagnostic. Nothing is asked of the pid
before it, so this adds no window in which a number could be
reaped elsewhere and reused (row
`PR125-CLOSE-PID-IDENTITY-UNDER-A-HOST-WILDCARD-WAITER`). The
helper's report and the pipe's close arrive on the pipe, which the
parent already owned and was already reading, so neither asks the
kernel anything about the pid either.

## `fn install_reaper_dispositions() -> bool` › `if !scrub_private_helper_dispositions() {`

This child never executes embedding-host code. Remove every
inherited callback before clearing its signal mask; SIGCHLD is
restored to default immediately below because the reaper owns
the stopped anchor's wait lifecycle.

## `fn install_reaper_dispositions() -> bool` › `let mut child_action: libc::sigaction = std::mem::zeroed();`

A library host may own SIGCHLD and reap children from its
handler. The private reaper must not inherit that callback (or
SA_NOCLDWAIT): either can consume the stopped anchor before the
reaper's blocking waitpid observes it.

## `mod termination` › `fn lock_cleanup_paths(paths: &[std::ffi::CString]) -> Result<(), SetupFailure> {`

Take the shared cleanup hold on every active lease, or say which lease
and which call refused, with the errno it left. The failure is
returned rather than reported here because the caller owns the
acknowledgement descriptor.

## (end of `fn lock_cleanup_paths(paths: &[std::ffi::CString]) -> Result<(), SetupFailure>`)

Deliberately leave this independently opened descriptor live.
Process exit releases its shared lease after cleanup completes.

## `mod termination` › `let polled = unsafe { libc::poll(&mut command, 1, 10) };`

Poll even before registration, and never for longer than one slice.
EOF is not a parent-liveness signal on Darwin, for a stronger reason
than the one this comment used to give: `poll(2)` on a Darwin FIFO
reports data and never the last writer's close (`wait_readable`'s
section has the mechanism and the measurement), so a parent that dies
with nothing in flight is invisible to this poll however long it
waits. A fork-only descendant of the parent retaining the writer
would have the same effect on any platform. Reparenting is
authoritative and lets this fork-only helper settle independently;
the ten-millisecond slice is what makes the `getppid` check run.

## `fn cleanup_reaper_group` › `unsafe {`

Signal the kernel-owned group identity first. Even if the platform's
membership scanner subsequently becomes unavailable, no owned
process can keep running or spending while cleanup waits fail-closed.

## `fn cleanup_reaper_group` › `let mut delay_left = cleanup_delay_ms;`

Test subprocesses can widen the otherwise tiny post-crash window so
the reaper-owned cleanup lease is asserted deterministically.
Release builds always pass zero and pay no delay.

## `fn cleanup_reaper_group` › `while group_has_non_zombie_members(pgid) != Some(false) {`

The stopped anchor pins the PGID until it becomes our unreaped
zombie. Only release the reaper-owned run-cleanup lease once every
member of that exact group is either gone or a non-running zombie.

## `impl Guard` › `fn stop_parent(self) -> Option<bool> {`

Returns `Some(true)` only after the guard sent SIGSTOP and this
process subsequently resumed. `Some(false)` means a concurrent
continue/termination cancelled the stop before it was issued.

## `mod termination` › `struct HelperEnd {`

What ending a helper that never acknowledged its startup actually
returned.

This asks the kernel nothing it was not already going to be asked. The
teardown sends one `SIGKILL` and takes one `waitpid`, exactly as it
did before this existed and in the same order; all this does is keep
the two answers instead of discarding them, which is what §7 asks of a
signal whose result the caller depends on and what row
`PR125-CLOSE-DISCARDED-KILL-RESULT` asks for.

**Nothing here is a claim about which process the number named.** A
pid cannot be tied to the helper that was forked with it while an
embedding host may reap this process's children — that is the open
design question of row
`PR125-CLOSE-PID-IDENTITY-UNDER-A-HOST-WILDCARD-WAITER`, and no
observation the parent can make settles it. So these are the words for
what two system calls answered, and a reader draws the same inference
from them that they could draw from the calls themselves: no more.

## `struct HelperEnd` › `kill_errno: libc::c_int,`

`0` if `kill` reported delivery, otherwise the errno it left.

## `struct HelperEnd` › `waited: libc::pid_t,`

The pid `waitpid` returned, or `-1`.

## `struct HelperEnd` › `wait_errno: libc::c_int,`

The errno `waitpid` left when it returned `-1`.

## `struct HelperEnd` › `status: libc::c_int,`

The status `waitpid` filled when it returned a pid.

## `mod termination` › `fn describe_helper_end(end: HelperEnd) -> String {`

The words a failure message carries for a [`HelperEnd`].

Two outcomes are worth separating and this separates them: a child
that was **still there** and was killed, and one that had **already
ended** before the signal — either by exiting on its own, which its
exit status then names, or by being gone from the process table
altogether. A helper that exits before READY does so through one of
its own `_exit(1)` paths, so a status is the difference between "it
failed setting itself up" and "it was still working when we gave up".

## `mod termination` › `enum SetupStep {`

The steps a helper takes before READY that can refuse, in the order it
takes them: the reaper installs its signal dispositions, moves into its
own process group, opens each active cleanup lease and takes the shared
lock on it; the guard installs its dispositions and forks its probe.
Closing the inherited descriptors is not here because `close` on a
number that is not open is not a failure the loop reads. The byte
encoding is explicit rather than `as`-cast so that a report from a
helper built at another revision decodes to `None`, never to a
different step.

## `mod termination` › `fn report_setup_failure_and_exit(ack_fd: libc::c_int, failure: SetupFailure) -> ! {`

The child side of the report. One `write` of the frame, then
`_exit(1)`: the exit status the parent has always seen from a helper
that failed its own setup is unchanged, and the frame is what names
the step. The write is best-effort because nothing better exists at
that point, the process is ending either way, and its failure is
observable: the parent reads the pipe closing with no report and says
so, which is the shape the `UPSTROKE_TEST_HELPER_EXIT_BEFORE_READY`
seam drives and `helper_ready_failure_helper` pins. Async-signal-safe:
`write` and `_exit` only, on a frame built by value.

## `mod termination` › `enum AckRead {`

What one bounded read of an acknowledgement byte answered. The four
answers are the four things a caller does differently: act on the
byte, treat the helper as gone, treat it as silent, or report the
wait's own failure. `Option<u8>` collapsed the last three, and the
READY failure messages could then say only "waited 2s of 2s" for a
helper that had been dead since the first millisecond.

## `mod termination` › `fn read_guard_ack(fd: libc::c_int, timeout: Duration) -> AckRead {`

One byte within `timeout`, or the reason there was none. The deadline
is `started.elapsed()` against `timeout`, not `Instant + Duration`,
which panics on overflow; an interrupted wait or read is retried
against the time left.

## `mod termination` › `fn wait_readable(fd: libc::c_int, timeout: Duration) -> Readiness {`

Block until `fd` has a byte to read or has no writer left, or until
`timeout`. Two implementations, because the two platforms' channels
differ: Linux builds the helper pipes with `pipe2(O_CLOEXEC)` and
`poll` reports both data and hangup on a pipe; Darwin has no `pipe2`,
so `create_cloexec_pipe` builds the channel from a FIFO to get an
atomic close-on-exec open, and **`poll(2)` on a Darwin FIFO never
reports the last writer's close**.

The mechanism, from XNU. `poll` is implemented over kqueue, and a
kqueue `EVFILT_READ` knote on a FIFO vnode goes through `vn_kqfilter`
to the generic vnode filter, whose readable test is
`vnode_readable_data_count`: for a FIFO that is `fifo_charcount`, the
number of bytes queued, and nothing else. A writer's `close` runs
`fifo_close`, which marks the FIFO's read socket `SS_CANTRCVMORE` and
wakes the *socket's* selinfo; it posts nothing to the vnode's knotes,
and a re-evaluation would count zero bytes anyway. `select` reaches the
FIFO through `fifo_select`, which asks the read socket `soreadable`,
and that test includes `SS_CANTRCVMORE`. A blocking `read` on the FIFO
sees the close too, through `fifo_read`'s writer count. So data wakes
`poll`, `select` and `read`; a hangup wakes `select` and `read` only.

Measured rather than reasoned, on the macOS runner CI uses (macOS
26.5.2, xnu-12377.121.10, run 33989492728 on branch
`scratch/darwin-fifo-eof-experiment`, `exp/fifo_eof.c`): with the
channel built exactly as `create_cloexec_pipe` builds it and a forked
child that closes its end and exits, `poll` returned 0 after the full
3 s timeout with the child long collected; `select` on the same FIFO
returned readable at once and the following `read` returned 0; `poll`
on a `pipe(2)` returned `POLLIN|POLLHUP` at once; and a child that
wrote one byte woke `poll` on the FIFO at once. Linux passed all five
at once.

What it cost before this: every helper that ended before READY held
its launch for the whole `HELPER_READY_BUDGET` and then read as
"having already exited with status 1", which is the fingerprint row
`PR125-CLOSE-MACOS-READY-RED-CAUSE-UNKNOWN` carries and the reason a
ten-second budget at PR #125's head waited ten seconds. It also cost
the exit-before-READY test four seconds on every macOS run.

Why `select`, and why this symbol. `select` is the one wait primitive
Darwin gives a FIFO that reports hangup. Its `fd_set` is fixed at
`FD_SETSIZE` (1024) descriptors, and a descriptor number in a process
that has opened many files exceeds it (the test binary does routinely);
the plain `select` symbol refuses such an `nfds` with `EINVAL`. Darwin
provides the same call without that limit under the symbol the header
binds when `_DARWIN_UNLIMITED_SELECT` (or the default `_DARWIN_C_SOURCE`)
is defined, `select$DARWIN_EXTSN`, which takes a caller-sized bit array;
the `libc` crate binds the limited one, so the extern here names the
unlimited one directly. The bit array is `slot / 32 + 1` words with one
bit set, which is every bit below `nfds`, and the call may write the
time left back into the `timeval`, which is why it is passed by
mutable reference and not reused. `kqueue` is not an alternative: it is
what `poll` is built on and carries the same filter. Replacing the FIFO
with a `pipe` would give `poll` its hangup but lose the atomic
close-on-exec open that `create_cloexec_pipe`'s section explains, and
that is a wider change than this one.

Neither implementation reads the descriptor's `revents` or set
membership beyond "ready": a descriptor that is invalid or in error is
reported by the `read` that follows, with the errno it leaves.

## `mod termination` › `fn await_ready(fd: libc::c_int, ready: u8, budget: Duration) -> ReadyWait {`

The READY handshake from the parent's side, with every way it can end
named. A `HELPER_ABORT` first byte is followed by the rest of its frame,
read against what is left of the same budget; a frame that does not
complete, or does not decode, is `TruncatedReport` rather than a guess
at a step. A byte that is neither READY nor the marker is reported as
the byte, because the guard and the reaper share this function and a
wrong-helper byte would otherwise read as silence.

## `mod termination` › `fn describe_ready_wait(`

The words the failure message carries for a `ReadyWait`, lowercase and
without a trailing period so that the message's `; ` joins read as one
sentence. The lease paths are the parent's rendering of the same list
the child locked, so `index` names the same file on both sides.

## `mod termination` › `fn acknowledged(fd: libc::c_int, expected: u8, timeout: Duration) -> bool {`

Whether `expected` arrives on `fd` within `timeout`, skipping any other
byte first. A reaper that came up after its READY deadline has that
stale byte queued ahead of its CANCEL acknowledgement; judging the first
byte alone failed a cancel the reaper had accepted (`C-004`). The pipe
closing, the budget elapsing and the wait failing are all `false`; a
flood of unexpected bytes after the deadline ends at the first read
against a zero remainder (row
`PR125-CLOSE-FLOODED-CANCEL-UNBOUNDED-BY-THE-FINAL-LOOK`).

## `fn spawn_guard` › `let how = if wait == ReadyWait::Ready {`

The guard's READY is a byte and then its probe's pid; a guard that said
READY and then ended before the pid is told apart from one that never
said READY. The guard takes no cleanup lease, so its report is
described against an empty lease list.

## `fn spawn_guard` › `let exit_before_ready = std::env::var("UPSTROKE_TEST_HELPER_EXIT_BEFORE_READY")`

A helper that ends before it writes READY; see the same seam in
`spawn_reaper`. Read before fork: the multithreaded child may call
only async-signal-safe primitives.

## `fn spawn_guard` › `let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };`

Resolve the descriptor ceiling before fork: sysconf may take libc
locks, whereas the multithreaded child may call only async-safe
primitives until it enters the guard loop.

## `fn spawn_guard` › `if !install_guard_dispositions(policy) {`

Replace inherited host callbacks and clear the inherited mask as
the first child-side action. Descriptor scrubbing can be long on
high-limit hosts; no signal in that window may run host code or
leave the only wake relay blocked.

## `fn guard_loop` › `let [first, second, third, fourth] = probe_pid.to_ne_bytes();`

Bounding the wait below modified `guard_loop`, and `standards/SWEEP.md`'s
activation rule puts §6 and §7 on the whole body of a function a change
modifies while `src/agent/proc.rs` is still queued rather than swept. So
the body's panic surface went with the change: the READY frame is built
by destructuring instead of `ready[0] = …` and `ready[1..]`, the two poll
entries are read through `let [command_poll, wake_poll] = poll_fds;`
instead of `poll_fds[0]` and `poll_fds[1]`, and the bytes read are walked
with `iter().take(read)` instead of `&buffer[..count as usize]`, over a
length from `usize::try_from`. `cargo clippy -W clippy::indexing_slicing
-W clippy::unreachable` reports no site of either construct in this
function or in the test below. Nothing else in the body changed.

## `mod termination` › `let polled = unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, GUARD_POLL_SLICE_MS) };`

Both parent relays and guard-directed foreground signals make a
descriptor readable, so there is no atomic-check-to-poll window.
While the parent is SIGSTOPped, a signal sent only to its PID
cannot run a caught handler. Periodically resume only the parent;
its SA_SIGINFO SIGCONT handler recognizes this guard as sender,
delivers any pending Upstroke-owned termination, or immediately
re-stops. Agent groups remain stopped throughout.

Every timeout, armed or idle, first asks whether the recorded parent is
still this process's parent, and a guard that has been reparented ends
there; the probe byte is written only while armed and stopping, at the
cadence it always had. `const GUARD_POLL_SLICE_MS` above is why the
timeout is finite at all.

## `mod termination` › `wake = false;`

ARM is an epoch boundary. Signals observed before
it are already represented in the parent's
atomics and checked after this acknowledgement;
retaining them would spuriously continue a later
stop. A signal racing after this clear is caught
by its ordered command or wake-pipe record.

## `mod termination` › `if unsafe { libc::getppid() } != parent {`

PID reuse must never redirect a late stop to an
unrelated process. Reparenting proves the
original Upstroke process is gone.

## `mod termination` › `if unsafe { libc::kill(parent, libc::SIGSTOP) } != 0 {`

The parent is blocked reading this ack pipe. The
stop is queued before the acknowledgement write,
so it cannot return to userspace until a later
SIGCONT has genuinely resumed it.

## `mod termination` › `if unsafe { libc::getppid() } != parent {`

PID reuse must never redirect a late guard wake to an
unrelated process. Reparenting proves the original Upstroke
process is gone even if its numeric pid has been reused.

## `mod termination` › `fn scrub_private_helper_dispositions() -> bool {`

Remove every embedding-host callback from a fork-only helper.

Signal numbers are sparse and platform-specific. `sigaction` reports
EINVAL for holes, uncatchable signals, and values above the platform's
range, so a fixed upper bound avoids non-portable NSIG APIs while still
covering Linux real-time and BSD/macOS signals. Asynchronous signals
are ignored so a broadcast cannot disable cleanup; synchronous faults
retain their ordinary fatal behavior.

## `fn install_guard_dispositions(policy: SignalPolicy) -> bool` › `unsafe {`

The guard stays in the foreground process group but cannot join the
stop: it ignores SIGTSTP and records every transition that must wake
a parent already stopped by the guard. SIGSTOP itself targets only
the parent pid.

## `fn install_guard_dispositions(policy: SignalPolicy) -> bool` › `if !scrub_private_helper_dispositions() {`

Scrub before deliberately clearing the inherited mask. Only this
guard's narrow supervision surface is installed below.

## `fn install_guard_dispositions(policy: SignalPolicy) -> bool` › `let mut empty: libc::sigset_t = std::mem::zeroed();`

A library host may have blocked these signals on the thread
that first invoked Upstroke. The guard is an isolated relay, not
host code: clear its inherited mask so it can always wake a
parent that it previously stopped.

## `fn install_guard_dispositions(policy: SignalPolicy) -> bool` › `if libc::signal(libc::SIGTSTP, libc::SIG_IGN) == libc::SIG_ERR`

Job-control callbacks and defaults belong to the embedding
parent when Upstroke cannot safely proxy the pair. The private
guard must neither run fork-copied host code nor stop itself.

## `fn install_guard_dispositions(policy: SignalPolicy) -> bool` › `if libc::signal(`

Custom callbacks belong to the embedding parent. Never
run a fork-copied callback against the guard's private
memory; translate it into the same self-pipe wake as a
default Upstroke-owned termination signal instead.

## `fn close_inherited_fds` › `if close_ranges_except(keep) {`

The fork must not retain the run lock, event file, pipes, or secrets.
Linux close_range keeps this bounded even when RLIMIT_NOFILE is in
the millions. Older kernels and other Unix hosts retain the
syscall-only per-descriptor fallback.

## `fn close_ranges_except(keep: &[libc::c_int]) -> bool` › `if next_keep != Some(first) {`

`first == kept` is an empty range. Saturating `kept - 1`
would turn the fd-zero case into 0..=0 and close the descriptor
we were explicitly asked to preserve.

## `fn verify_group_scanner() -> Result<(), String>` › `let deadline = std::time::Instant::now() + Duration::from_secs(2);`

Process enumeration can race an unrelated process exiting. Retry a
bounded realistic interval, but refuse before launching an agent
when either cleanup enumeration or parent-state observation is
persistently absent (for example a Linux container without a
mounted/readable procfs).

## `fn group_has_non_zombie_members(pgid: i32) -> Option<bool>` › `LinuxStatSnapshot::Invalid => {`

Permission failures and malformed snapshots remain
fail-closed. Only a kernel-confirmed vanished PID is
safe to skip as ordinary process churn.

## `fn group_has_non_zombie_members(pgid: i32) -> Option<bool>` › `1,`

Apple only searches the zombie table for BSD-info
flavors when this argument is non-zero. Without it an
exited group member is indistinguishable from an
incomplete snapshot and cleanup must wait forever.

## `fn group_has_non_zombie_members(pgid: i32) -> Option<bool>` › `return None;`

A disappearing target-group pid is resolved by the next
complete snapshot. Never turn an incomplete observation
into permission to release the cleanup lease.

## `fn create_cloexec_pipe` › `let template = std::env::temp_dir().join(format!(".upstroke-pipe-{}-XXXXXX", unsafe {`

Darwin has no pipe2. Build the anonymous-equivalent channel from a
FIFO inside an atomic, private mkdtemp directory: each endpoint is
opened with O_CLOEXEC in the syscall that creates its descriptor,
then the name and directory are removed before this function returns.

## `fn set_nonblocking(fd: libc::c_int) -> bool` › `unsafe {`

Signal handlers may write this descriptor. Nonblocking mode makes a
dead or unresponsive guard fail closed instead of wedging Upstroke in
async-signal context.

## `fn groups_are_quiescent(groups: &[i32]) -> bool` › `let entries = match std::fs::read_dir("/proc") {`

`/proc/<pid>/stat` is a kernel interface and remains available on
distributions such as NixOS that intentionally have no `/bin/ps`.
It observes every descendant in the process group, not only the
direct child that Upstroke can wait on.

## `fn groups_are_quiescent(groups: &[i32]) -> bool` › `continue;`

Processes can disappear between directory enumeration and
the read. A still-live target group is caught by either
another member or the kill(0) completeness check below.

## `fn parse_linux_process_stat(stat: &str) -> Option<(i32, u8)>` › `let tail = stat.get(stat.rfind(')')? + 1..)?.trim_start();`

The parenthesized command may itself contain spaces and `)` bytes;
the final close parenthesis is the only reliable field boundary.

## `fn groups_are_quiescent(groups: &[i32]) -> bool` › `let output = match std::process::Command::new("/bin/ps")`

`/bin/ps` is a fixed base-system interface on macOS; no
repository-controlled PATH entry can substitute for it.

## `fn quiescent_snapshot_is_complete` › `if unsafe { libc::kill(-*pgid, 0) } == 0`

A group that disappeared between SIGSTOP and the snapshot is
already quiescent. Any other result means `ps` failed to account
for a still-live member, so do not stop the parent yet.

## `mod termination` › `struct ReaperContainers {`

-----------------------------------------------------------------------
The container half of the orphan window — ST-16 (d)
-----------------------------------------------------------------------

## `mod termination` › `struct ReaperContainers {`

The `docker` argument vectors, rendered before any fork.

A reaper is a `fork`-only child of a multithreaded process: after the
fork it may call only async-signal-safe functions, so it can neither
format a filter nor allocate an argv. Every byte it will ever need is
therefore built here, on the parent side, exactly as `spawn_reaper`'s
`cleanup_paths` are — and a `CString`'s buffer does not move when the
struct that owns it does, so the pointer array stays valid.

## `struct ReaperContainers` › `_ps: Vec<std::ffi::CString>,`

Kept alive for the pointers in `ps_argv`.

## `struct ReaperContainers` › `ps_argv: Vec<*const libc::c_char>,`

NULL-terminated `argv` for `docker ps …`.

## `mod termination` › `static CONTAINER_SCOPE: OnceLock<`

The scope every reaper started from now on inherits, or `None`.

A reaper already running keeps the scope it was forked with; there is no
channel for handing one a new one, and inventing a wire frame for it
would put a variable-length message into a protocol whose frames are five
bytes.

## `mod termination` › `const REAPER_PS_BUFFER: usize = 8192;`

The fixed listing buffer. A `--no-trunc` id is 64 bytes plus a newline,
so this is **126 containers per listing** — and a reaper cannot grow a
buffer, so the number of *rounds* is what has to be unbounded. It was
`8`, which made the buffer size a silent ceiling of 126 x 8 = **1,008
containers**: a coordinator dying with 1,009 of them left one behind and
reported the same success it reports on a clean machine.

## `mod termination` › `const REAPER_DOCKER_TICKS: usize = 3_000;`

The ceiling on one `docker` invocation, in 10 ms ticks.

`determinism` forbids sleeps in tests and this is not one: it is the
fail-safe that keeps a wedged daemon from holding R28 — the shared
cleanup hold the next coordinator waits on — for ever. A reaper that
waited without a bound would convert "docker is hung" into "no run on
this machine can ever start again".

## `mod termination` › `pub(super) fn set_container_reclaim_scope(`

Arm or disarm the container scope. See
[`super::set_container_reclaim_scope`].

## `mod termination` › `if let Some(scope) = scope {`

Rendered here so a scope that cannot be turned into argv is refused
by the caller that set it, rather than silently doing nothing inside
a reaper that has no error channel.

## `mod termination` › `fn resolve_reaper_program(program: &std::path::Path) -> Result<PathBuf, UpstrokeError> {`

The absolute program the reaper will `execv`, resolved **before** the
fork.

**`execv` does not search `PATH`. Only `execvp` does** — and `execvp` is
not on the POSIX async-signal-safe list, so a reaper (a `fork`-only child
of a multithreaded process) may not call it. A bare `docker` handed to
`execv` therefore resolves against nothing at all: the listing child
`_exit(127)`s, the pipe carries no bytes, and the reaper reports exactly
the same success it reports on a clean machine. Measured, not reasoned —
it is what shipped, and the only fixture used an absolute stub.

So the search happens here, on the parent side, in ordinary code with an
error channel: the same discipline that renders every other byte the
reaper needs before the fork. `execvp`'s own rule is mirrored exactly —
a name containing a `/` is a path and is used verbatim (that is what
`execv` already does correctly); a name with no `/` is searched for on
`PATH`, and one `PATH` cannot resolve is **refused** rather than handed
to a child that has no way to say so.

[`crate::util::find_program`] is the resolver deliberately: it is the
one `runner::container::DockerCli::available` asks when it decides the
runtime is present, so the reaper execs the binary the rest of the engine
means by `docker`.

## `mod termination` › `fn render_container_argv(`

The argument vectors for `scope`, or why they cannot be built.

## `mod termination` › `let argument = if index == 0 {`

`argv[0]` is the resolved path too, so the program the child execs
and the program it reports itself as are one string in every one
of the three invocations — `ps` here, `kill` and `rm` from
`containers.program` directly.

## `mod termination` › `fn container_scope_for_a_new_reaper() -> Option<ReaperContainers> {`

What a reaper about to be forked should carry.

## `mod termination` › `fn reclaim_labeled_containers(containers: &ReaperContainers) {`

Kill and remove every labeled container of the dead coordinator.

`T-CONTAINER.resume_action`: "on Unix the cleanup reaper performs
**kill/rm** earlier when the coordinator dies". Only kill and rm: the
Git view and the intent record are removed by the next write command's
census, which is why every step of `runner::container::reclaim` is
idempotent and tolerant of already-gone.

Every call here is async-signal-safe: `fork`, `execv`, `pipe`, `dup2`,
`open`, `close`, `poll`, `read`, `waitpid`, `kill`, `_exit`.

## `fn reclaim_labeled_containers(containers: &ReaperContainers)` › `let mut buffer = [0_u8; REAPER_PS_BUFFER];`

Two fixed buffers and no round counter. The loop ends on one of three
conditions, and none of them is a container count:

1. the listing is **empty** — everything this selector names is gone;
2. the listing is **byte-identical to the previous round's** — the
   runtime answered with exactly what it answered before, so the
   `kill`/`rm` of that round removed nothing and another round would
   repeat it. This is the real form of the guard the round count was
   standing in for ("a runtime that keeps reporting the same
   container cannot hold R28 for ever"), and it is both tighter (it
   fires on the second round rather than the eighth) and not a
   ceiling on how much work a healthy runtime may be given;
3. no complete id was parsed out of a non-empty listing.

Termination without a count: the selector names one **dead**
incarnation, so nothing can add to the set while this runs — the only
process that creates containers under that incarnation label is the
coordinator that died. The set is therefore finite and non-growing,
each round either shrinks it or answers identically, and every
`docker` invocation inside a round is itself bounded by
[`REAPER_DOCKER_TICKS`].

Stopping on (2) is not a silent give-up: what it leaves behind is a
labeled container the runtime will not remove, and that is exactly
the residue the **next write command's census** is required to refuse
over — `refusal_condition`'s "a dead owner's or dead incarnation's
labeled container that cannot be observed terminated blocks
admission". The reaper closes the window early; the census is what
makes failing to close it loud.

## `fn reclaim_labeled_containers(containers: &ReaperContainers)` › `buffer[index] = 0;`

NUL-terminate the id where it lies. Nothing is allocated and
nothing is copied; the buffer is this frame's own.

## `fn reclaim_labeled_containers(containers: &ReaperContainers)` › `let remove: [*const libc::c_char; 6] = [`

`--volumes`, exactly as `DockerCli::remove` issues it
(`PR6-ACCT-006`). An image declaring `VOLUME` gets one
**anonymous** volume per container, and `docker rm`
without this leaves one behind for every container the
reaper removes: measured, 29 leaked from a single run of
this suite through the ordinary path before
`PR6A-ANONYMOUS-VOLUMES-LEAK` put the flag there. Those
volumes are R26 — created by `docker create` as part of
the container, referable by nothing else — and once the
reaper has removed the container the following
intent-only census has no handle on them at all, so this
is the *only* point at which they can be reclaimed.
`--volumes` removes anonymous volumes and **never a named
one**, so it cannot touch R20 (measured on docker 29.7.2:
a mounted named volume survives `rm --force --volumes`).

## `mod termination` › `fn list_labeled_containers(containers: &ReaperContainers, buffer: &mut [u8]) -> usize {`

Run `docker ps …` and read its ids into `buffer`, returning how many
bytes arrived.

## `fn list_labeled_containers` › `if fds[1] != 1 && libc::dup2(fds[1], 1) < 0 {`

The reaper closed every inherited descriptor including 0, 1
and 2, so `pipe` may well have handed back fd 0 and fd 1
themselves. Move the write end onto stdout only when it is
not already there, and never close the descriptor that IS
stdout: doing so leaves `docker ps` writing to a closed fd,
the listing empty, and nothing reclaimed — with the reaper
reporting exactly the same success it reports on a clean
machine. Measured, not reasoned: it is what happened.

## `mod termination` › `fn spawn_docker(program: *const libc::c_char, argv: *const *const libc::c_char) {`

`docker <verb> <id>`, output discarded, bounded.

## `mod termination` › `unsafe fn quiet_standard_descriptors() {`

Give the exec'd `docker` real standard descriptors.

The reaper closed every inherited descriptor including 0, 1 and 2, so
without this a `docker` that opened a file would be handed **fd 1 or fd
2** for it and would then write its output or its diagnostics into that
file. `/dev/null` on whichever of the three is still free is the
cheapest way to make the numbers mean what they mean.

A descriptor that is **already** open is left alone, which is what keeps
this from undoing the listing child's pipe on fd 1.

## `unsafe fn quiet_standard_descriptors()` › `ensure_standard_descriptor(0, libc::O_RDONLY);`

In this order: `open` returns the lowest free descriptor, so
filling 0 first is what lets 1 and 2 land where they are asked
for without a `dup2` at all.

## `mod termination` › `unsafe fn ensure_standard_descriptor(target: libc::c_int, flags: libc::c_int) {`

Open `/dev/null` onto `target` unless something is already there.

## `mod termination` › `fn read_bounded(fd: libc::c_int, buffer: &mut [u8]) -> usize {`

Read until EOF, the buffer is full, or the ceiling is reached.

## `mod termination` › `fn reap_bounded(pid: libc::pid_t) {`

Wait for one `docker`, and kill it rather than hold R28 for ever.

## `mod termination` › `fn settle_after_coordinator_death(`

What a reaper does when its **coordinator has died**: settle the group,
then close the container half of the orphan window.

Separate from the [`REAPER_CLEANUP`] path on purpose, and this is the
distinction the whole extension turns on. `REAPER_CLEANUP` and
[`REAPER_CANCEL`] are the **live** coordinator asking for its invocation
to be settled; killing its labeled containers there would kill the
containers of a coordinator that is still spending through them, which is
`authoritative_state`'s "a live incarnation's containers must not be
touched" — the opposite of what this exists for.

## `mod tests` › `fn the_reapers_cleanup_hold_is_shared_between_overlapping_invocations() {`

R28 is a **shared** hold, and one run has more than one reaper.

`resource_accounting.rows[R28].resource` — "a surviving Unix cleanup
reaper's shared `cleanup.lock` hold (**one per reaper**; a reaper
may outlive the coordinator while it settles its process groups)".
Narrowing this `flock` to `LOCK_EX` would let the first reaper of a
run take the hold and refuse every later one — the second concurrent
invocation failing to start at all — and nothing observed it,
because no test ran two overlapping invocations and inspected their
holds.

`flock` holds belong to the open file description, so two calls here
are exactly two independent holders, which is what a second reaper
is. The expected behaviour is `flock(2)`'s, not this function's:
shared holds coexist and both exclude the exclusive side.

## `extern "C" fn reap_child_transitions(_: libc::c_int)` › `let _ = unsafe { libc::kill(child, libc::SIGKILL) };`

Keep a broken implementation from leaking the consumed,
permanently stopped anchor after this regression fails.

## `mod tests` › `fn reaper_handshake_helper() {`

Subprocess entry for the two `C-004` handshake checks.

In a fresh process because a regression of either arms process-wide
`SIGTERM`, which must fail this one test rather than the harness
running it — the exact shape `C-004` was.

## `fn reaper_handshake_helper()` › `let started = Instant::now();`

The parent holds READY back past the 2 s deadline, and past the
further 2 s a cancel would then have waited, through
`UPSTROKE_TEST_REAPER_READY_DELAY_MS`. The launch must fail
promptly, arm nothing, and leave no child behind.

## `fn reaper_handshake_helper()` › `let command = create_cloexec_pipe().expect("a stand-in command pipe");`

A CANCEL acknowledged behind a stale READY is a cancel that
succeeded: a reaper that came up late queues READY ahead of its
OK, and judging the first byte alone failed it.

## `mod tests` › `fn a_late_reaper_fails_its_launch_without_arming_termination() {`

`C-004`: a reaper that misses its READY deadline is an ordinary
failed launch, and a CANCEL acknowledged behind a stale READY is a
cancel that succeeded. Neither arms process-wide termination.

## `fn linux_close_range_fd_zero_helper()` › `let closed = unsafe { libc::close(libc::STDIN_FILENO) };`

The Rust test harness may reopen a missing standard descriptor
during startup, so close it at the final isolated point before
the pipe that must receive fd zero.

## `fn a_launch_cannot_enter_after_the_suspend_snapshot()` › `claim_launch(&waiting).expect("launch after resume");`

Exercise the production launch gate without fabricating a
second independent cleanup-reaper registry. Production has
one shared registry and serializes helper creation through
this claim; constructing a private Supervisor here violates
that invariant and makes Darwin FIFO inheritance part of a
synchronization test that never intended to cover it.

## `fn a_zombie_only_group_is_quiescent_for_cleanup()` › `let scan_deadline = std::time::Instant::now() + Duration::from_secs(2);`

An unrelated process can disappear between `/proc` enumeration
and its stat read, making one conservative scanner snapshot
unknown. Cleanup retries that state; this regression must model
the same contract rather than requiring an unrealistically
quiescent runner on its first snapshot.

## `mod tests` › `fn the_reaper_program_is_resolved_to_an_absolute_path_before_the_fork() {`

The program handed to `execv` is **always absolute**, and a bare name
`PATH` cannot resolve is refused where there is still an error channel.

`execv` does not search `PATH`; only `execvp` does, and `execvp` is not
async-signal-safe, so a reaper may not call it. The production spelling
is `runner::container::DOCKER_PROGRAM` — the bare name `docker`
— so a reaper handed that name unresolved lists nothing, reclaims
nothing, and reports success.

Second field held constant: the private root and the incarnation are the
same in every cell, so the only thing that moves is the **spelling of
the program** — bare-and-resolvable, bare-and-absent, and a path.

## `fn the_reaper_program_is_resolved_to_an_absolute_path_before_the_fork` › `let rendered = render_container_argv(&scope("git")).expect("git is on PATH");`

(1) A bare name that `PATH` resolves. `git` rather than `docker`
because the property is about resolution and every machine that
builds this repository has git; `util::find_program_resolves_real_
tools_and_misses_fake_ones` is where that is already relied on.

## `fn the_reaper_program_is_resolved_to_an_absolute_path_before_the_fork` › `let argv0 = unsafe { std::ffi::CStr::from_ptr(rendered.ps_argv[0]) };`

`argv[0]` is the resolved path too, so the string the child execs and
the string it reports itself as cannot drift apart.

## `fn the_reaper_program_is_resolved_to_an_absolute_path_before_the_fork` › `let absent = scope("upstroke-definitely-not-a-real-docker");`

(2) A bare name `PATH` cannot resolve is refused, while there is
still somewhere to report it: a reaper has no error channel.

## `fn the_reaper_program_is_resolved_to_an_absolute_path_before_the_fork` › `assert!(`

And the refusal reaches the caller that arms the reaper, which is the
only place with an error channel. Nothing is installed on this path,
so no other test in this process inherits a scope.

## `fn the_reaper_program_is_resolved_to_an_absolute_path_before_the_fork` › `let rendered = render_container_argv(&scope("/usr/bin/docker")).expect("a path");`

(3) A name carrying a separator is a path and is used verbatim —
exactly `execvp`'s own rule, and what `execv` already does correctly.

## `mod tests` › `fn reaper_stub(tag: &str, script: &str) -> (std::path::PathBuf, ReaperContainers) {`

A scratch directory, a recording `docker` stub, and the rendered
argument vectors that name it.

## `fn reaper_stub` › `let scope = crate::runner::container::census::ReaperContainerScope::new(`

The stub finds its own scratch directory through `dirname $0`,
so nothing about this fixture depends on process-wide state and
two of these may run concurrently.

## `mod tests` › `fn logged(dir: &std::path::Path, verb: &str) -> Vec<String> {`

Every line of the stub's log whose first word is `verb`, in order.

## `mod tests` › `fn the_reaper_performs_as_many_rounds_as_the_machine_needs() {`

The reaper performs **as many rounds as the machine needs**, not a
fixed number of them.

The listing buffer is fixed at [`REAPER_PS_BUFFER`] and a
`--no-trunc` id is 65 bytes with its newline, so one listing holds
**126** ids. A round count of 8 therefore made the reaper's reach a
silent **1,008** containers: a coordinator dying with 1,009 left one
behind and the reaper reported the same success it reports on a
clean machine. Twelve rounds is more than eight and few enough to
run in a fraction of a second; what it measures is that the count is
gone, not that the count is twelve.

Second field held constant: exactly one container is listed in every
round, so the number of ids per listing cannot be what ends the
loop — only the number of rounds moves.

## `fn the_reaper_performs_as_many_rounds_as_the_machine_needs()` › `let expected: std::collections::BTreeSet<String> =`

The expected ids come from the stub's own rule, written out here
rather than read back from what the reaper did.

## `mod tests` › `fn the_reaper_settles_more_containers_than_one_listing_holds() {`

A listing **larger than the buffer** is not silently truncated: the
ids that did not fit are settled by a later round.

This is the other half of the 126 x 8 arithmetic, and the two are a
product: a fixture that varied only the number of rounds would not
notice a buffer that dropped what it could not hold, and one that
varied only the number of ids would not notice a round count. 130 is
the smallest number that crosses the boundary — 130 x 65 = 8,450
bytes into an 8,192-byte buffer — so the first listing is cut inside
an id, which is the case a parser is most likely to get wrong.

Second field held constant: the stub removes exactly what it is
told to remove and invents nothing, so the only thing that moves
between rounds is how much of the set is left.

## `mod tests` › `fn a_runtime_that_keeps_answering_the_same_listing_ends_the_loop() {`

A runtime that keeps answering with the **same listing** ends the
loop on the second round, and does not repeat for ever.

This is the guard the round count used to be, in its real form. The
reaper holds R28 — the shared cleanup hold the next coordinator waits
on — so a loop that could not end would turn "docker is wedged" into
"no run on this machine can ever start again".

Second field held constant: the id set, which never changes; only
the number of times the reaper is willing to ask about it moves.

## `fn a_runtime_that_keeps_answering_the_same_listing_ends_the_loop` › `let (dir, rendered) = reaper_stub(`

The stub answers with the same two ids twice and then with
nothing. The third listing is what makes this fixture finite: an
implementation with **no** no-progress guard still terminates
here, and is caught by the counts rather than by a hang, because
a test that measures a missing bound by hanging measures nothing.

## `fn a_runtime_that_keeps_answering_the_same_listing_ends_the_loop` › `assert_eq!(`

Two listings: the first is acted on, the second is recognised as
the same answer and ends the loop **there** — the third listing,
which the stub is willing to answer, is never asked for. Each
container was attempted exactly once, so nothing is retried
against a runtime that has already refused to remove it.

## `mod tests` › `fn a_helper_ending_is_described_by_what_the_kill_and_the_wait_answered() {`

The words a READY failure carries are the words for what `kill`
and `waitpid` answered, and the outcomes a reader needs told apart
are told apart: a helper that had already ended itself, one that
was still there and was killed, and one the parent could not
signal or could not collect.

An exit status is the difference that matters. A helper that ends
before READY does so through one of its own `_exit(1)` paths, so
"already exited with status 1" says it failed setting itself up,
where "killed by signal 9" says it was still working when the
parent gave up. Nothing here infers which process the number
named; that is row
`PR125-CLOSE-PID-IDENTITY-UNDER-A-HOST-WILDCARD-WAITER`.

Witnessed against two mutations: the `WIFSIGNALED` and
`WIFEXITED` arms swapped, and `kill_errno` replaced by a constant
`0`. Each fails a named case below.

## `fn a_helper_ending_is_described_by_what_the_kill_and_the_wait_answered` › `let exited_one = 1 << 8;`

`waitpid` fills a status word, so the fixtures are built the
way the kernel builds them rather than by the code under test.

## `mod tests` › `fn helper_ready_failure_helper() {`

Subprocess entry for the READY-failure message at both call sites.

In a fresh process for the reason `reaper_handshake_helper` is:
both helpers install process-wide signal dispositions in their
children and publish descriptors in process-wide statics.

`UPSTROKE_TEST_HELPER_EXIT_BEFORE_READY` makes each helper end
itself before writing READY, so the parent's wait ends on the
pipe's end-of-file. **There is no clock in this test**: no sleep,
no budget to outrun and no elapsed time asserted, so no scheduling
outcome can make a correct implementation fail it (row
`PR125-CLOSE-SCHEDULER-BOUND-TIMING-TESTS`).

## `fn helper_ready_failure_helper()` › `assert!(`

The seam's exit code, reported through the wait the teardown
was already taking. This is the whole diagnostic: the child
had ended itself, and the message says with what.

## `mod tests` › `fn a_helper_that_never_acknowledged_reports_what_ending_it_answered() {`

Both READY failures carry the elapsed wait, the budget, the
descriptor ceiling, how the wait ended and what ending the helper
answered.

Witnessed against four mutations, each of which fails it:
`close_and_wait_reporting` returning a zero status rather than the
one `waitpid` filled (the message loses the exit status),
`descriptor ceiling {open_max}; ` deleted from either message, and
`wait_readable` polling on macOS (the wait ends at the budget and the
message says so, not "closed with no report"). The last is the one
this test could not see before it asserted how the wait ended: the
message shape was the same after two seconds as after two
milliseconds.

## `mod tests` › `fn a_helper_that_has_already_exited_ends_the_acknowledgement_wait_at_end_of_file() {`

The regression test for the Darwin wait, and the one that fails on the
tree before it: `read_guard_ack` answered `TimedOut` after the whole
budget for a writer that had been collected before the wait began.
Deterministic by construction: the child is reaped first, so its
descriptors are closed before the parent looks, which is the shape
row `PR125-CLOSE-SCHEDULER-BOUND-TIMING-TESTS` asks for. On Linux it
passes before and after; the platform the defect is on is the one that
evaluates it (§11).

## `mod tests` › `fn a_guard_whose_conductor_is_gone_ends_while_its_pipes_stay_open() {`

The regression test for the guard's idle wait, and the one that fails on
the tree before it: with `poll(…, -1)` the guard was still there when the
test's collection budget ran out and had to be killed, which its message
reports as the status the kill produced.

The fixture is the defect's own shape rather than a clock. The guard's
recorded parent is a pid this test forked and *collected*, so `getppid`
can never answer it again, and the test holds every command and wake
writer open for the whole wait, so no end-of-file can end the guard and
the reparenting check is the only thing that can. The end is observed by
collecting the child, not by timing the guard's own work (row
`PR125-CLOSE-SCHEDULER-BOUND-TIMING-TESTS`). Every wait the test takes is
bounded: READY through `await_ready` on `HELPER_READY_BUDGET`, the same
call and budget `spawn_guard` uses, so a guard that stalls before saying
READY is reported rather than waited on forever; and the collection
through `wait_for_lifetime_target`. A guard the test cannot account for —
one that never said READY, or one still waiting when the collection
budget runs out — is ended with SIGKILL and collected, never by closing
those writers: on Darwin that close is exactly what `poll` does not
report, so closing them first would make the collection unbounded on the
platform the defect is on. Both platforms evaluate this: holding the writers open
withholds the hangup Linux would otherwise report, so the check under
test is the same one on each (§11).

The fixture guard scrubs its inherited descriptors before entering the
loop, as `spawn_guard`'s child does. The harness runs these tests in one
process and in parallel, so a child that kept copies of another test's
pipe write ends would hold them open for the whole wait and withhold the
end-of-file that test is measuring.

## `mod tests` › `fn a_reaper_refused_its_cleanup_lease_says_which_lease_and_why() {`

End to end through `spawn_reaper` with a real run lock and cleanup
scope, so the lease path the reaper reports is the one the conductor
registered. The exclusive `flock` the test holds is on its own open
file description; the child's `close` of the inherited copy does not
release it, and the child's own `open` and `LOCK_SH | LOCK_NB` refuse
at once, so no scheduling outcome reaches the assertion. This is also
the only test that drives `report_setup_failure_and_exit` in a real
child; the frame's encoding is pinned separately without a fork.

## `pub(crate) mod test_support` › `pub(crate) fn run_with_timeout(`

Test-only convenience entry. Production passes both sites explicitly.

## `pub(crate) mod test_support` › `pub(crate) mod readiness;`

**Out of line, in `proc/test_support/readiness.rs`.** The primitives are
~440 lines of protocol that three modules' fixtures depend on, and they
were the only thing in this file with no relationship to subprocess
supervision. Moving them gives them their own module doc and their own
stated lint level -- a Rust lint level is scoped by the module tree
rather than by the file, so an out-of-line child inherits this file's
`#![allow]` unless it says otherwise, which is `PR6-LANEF-004`.

The path does not change: this declaration keeps
`crate::agent::proc::test_support::readiness` resolving exactly as it
did, for `workspace`, `rundir` and the witnesses below.

**Declared without a `#[cfg(test)]` of its own**, because it inherits one:
`test_support` above carries it, so the file is compiled only under
`cfg(test)` and `effects::census_domain` resolves it as a whole-file test
module through that inline ancestry rather than through an attribute
written here.

## `memoised_outcome` › `pub stdout: String,`

Captured stdout, decoded lossily as UTF-8. An inherited writer that
outlives the post-exit grace can leave this partial; this public result
has no flag proving that EOF was observed.

## `memoised_outcome` › `pub stderr: String,`

Captured stderr with the same grace and decoding policy as `stdout`.

## `run_with_timeout_and_limit` › `let mut termination = termination::Supervisor::begin(terminate_site)?;`

Enter before `spawn`: if an interrupt arrives in the narrow interval
between creating the child and learning its pid, the signal monitor
waits for this registration rather than terminating Upstroke first and
orphaning the new process group.

## `run_with_timeout_and_limit` › `apply(`

`Spawn.Registered`: "parent-side registration".

## `run_with_timeout_and_limit` › `drop(command);`

Spawn borrows configured Stdio. Drop our copies of the child's pipe
ends now, or they would suppress EOF and conceal declined stdin.
The spawn-error closure above still has Command for its diagnostic.

## `run_with_timeout_and_limit` › `apply(hooks.point(SubEffectPoint::Exec), SubEffectPoint::Exec)?;`

`Spawn.Exec`: `Command::spawn` reports a failed `execvp` through its own
CLOEXEC status pipe and returns `Err`, so reaching here is the exec
having succeeded.

## `run_with_timeout_and_limit` › `drop(termination);`

Drop the pre-exec reaper first: it still has an anchor pinning this
child's group identity and will kill every member before returning.

## `run_with_timeout_and_limit` › `let input_error = |error: input::FeedError| UpstrokeError::Agent {`

Feed stdin from its own thread: the child may not read stdin until it
has written output, and this thread must not block the pipe drains.
The copy transfers owned input to a worker joined before this call ends.

## `run_with_timeout_and_limit` › `{`

A missing pipe worker can stall the child. Treat any
worker-start refusal as a supervision failure and settle
the tree; dropping successful workers releases them too.

## `run_with_timeout_and_limit` › `if let Err(error) = termination.finish() {`

Leave the exited leader as a zombie until cleanup completes:
its PID pins the PGID, so no unrelated group can reuse the
numeric id between observation and the final signal.

## `run_with_timeout_and_limit` › `if let Some(feeder) = stdin_feeder {`

A descendant retaining stdin cannot keep a nonblocking poll waiting.
Collection releases and joins, then observes every returned failure.

## `run_with_timeout_and_limit` › `let collect = |drain: Option<Drain>| -> Result<(String, bool), UpstrokeError> {`

The public agent result retains the bounded partial-output policy.
An escaped writer may remain open after the grace. Cancellation
ends polling and joins the worker, but does not prove EOF. Complete
binary consumers use collect_bytes and inspect ended and limited.

## `run_with_timeout_and_limit` › `finish_pipe_reports(outcome, reports)`

All worker owners have settled, including early error exits. Each slot
contributes at most one secondary diagnostic to the returned outcome.

## `mod input;`

The stdin worker's error publication and bounded post-exit lifecycle.

## `fn spawn_sigchld_target() -> (libc::pid_t, std::os::fd::OwnedFd) {`

The parked target owns only its lifetime pipe. A setup failure or
parent death closes the writer and ends it even before a reaper
has registered its separate process group.

## `fn wait_for_lifetime_target(target: libc::pid_t) -> Option<libc::c_int> {`

Poll a child owned by the isolated lifetime helper. That helper
installs SIG_DFL for SIGCHLD and has no other waiter or child.

## `sigchld_target_setup_failure_helper` › `let _reaper = spawn_reaper().expect("forced reaper startup refusal");`

The outer test makes the reaper exit without READY. This
forces startup refusal regardless of process scheduling.

## Pipe startup failure reporting

A failed pipe or worker startup retains its original error while settling the
registered process. Reaper, kill and wait failures accompany that primary
failure. A successful wait proves the direct child was reaped and makes a
racing kill refusal irrelevant. If wait fails, both kill and wait errors are
reported. Deferred worker reports append through `WithCleanup`, preserving
the primary type and avoiding a second agent-error prefix around it.
