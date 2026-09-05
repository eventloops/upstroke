# `src/engine/topology/prelock.rs`

Extended notes for [`src/engine/topology/prelock.rs`](../../../../src/engine/topology/prelock.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

O01 — the read-only pre-lock checks of a fresh schema-4 write command.

`decisions.workspace_candidates.run_creation`, first clause:

> every write command first performs its read-only pre-lock checks and only
> then takes the physical worktree lock … the pre-lock checks of a fresh run
> are: config validation (PR1); resolution of the `RunnerPolicy` from
> `[runner]` config **by read-only inspection** … with `runner_policy_sha256`
> computed over its canonical serialization — an unreachable runtime, an
> absent image reference, or an absent volume **refuses here, before any
> lock or other effect**; generation of the coordinator incarnation id
> (per-process ULID) and the run id (**no effect**).

Three properties, and each is a separate thing to get wrong:

* **Order.** Resolution precedes id generation, and both precede
  `Lock.AcquireWorktree`. This module cannot take a lock — it holds no
  repository path and calls nothing that opens one — so the ordering is a
  fact about what it can reach rather than about where its call site sits.
* **No residue.** Nothing here creates a directory, a file, a lock, a
  container or a process. The one filesystem call is
  [`std::fs::canonicalize`], which is read-only, and the one runtime
  interaction is [`resolve_container`]'s four inspections, every one of which
  is a read.
* **Unforgeable output.** [`PreLockChecked`] has private fields, lives alone
  in a module of its own, derives no `Clone`, `Copy` or `Default`, and has
  exactly one constructor — [`check`]. Every later prefix consumes it by
  value, so "the run id, the incarnation and the policy P0–P8 use are the
  ones resolved before the lock" is a type rather than a comment.

### What this module deliberately does **not** do

**Config validation.** `run_creation` names it first among the pre-lock
checks and it is PR1's: [`crate::engine::preflight::validate_inputs`] is
`config::load_captured` plus the plan analysis, and both write commands
already run it as their first statement. Re-running it here would perform a
second read of the plan and the config, which is exactly what
`preflight::Validated`'s doc calls out as proving nothing ("a snapshot beside
an independent read proves nothing about what was validated"). What this
module takes instead is config's own *product*, a [`RunnerSelection`], which
no caller can obtain without having parsed a configuration.

**The worktree lock.** `startup.rs` owns O02. A module that both refused
before the lock and took it would make "before" unobservable.

## `mod checked {`

The witness, alone in a module, the way [`crate::rundir`]'s `ownership` is.

Field privacy is what makes [`check`] the only constructor: no code inside
`prelock` or outside it can name these fields, so a `PreLockChecked` in
anybody's hand is one that came out of the pre-lock sequence. It derives no
`Clone` (two copies would be two runs claiming one id), no `Copy` and no
`Default`.

## `mod checked` › `pub struct PreLockChecked {`

The read-only pre-lock checks, performed.

Carries exactly what the publication prefixes need and nothing they can
re-derive: the resolved policy (P3b's owner record and P6's
`run_started`), its digest (P1's marker and every container intent), the
run id and the incarnation (the public directory name, the marker, the
owner record, `run_started`, and every container name), the pid the
marker records, and **the authorized private root**, which is the
containment boundary every later locator is checked against.

## `impl PreLockChecked` › `pub(super) fn new(`

The one constructor, callable only from [`super::check`]'s module.

## `impl PreLockChecked` › `pub fn run_id(&self) -> &str {`

The run id this command will create.

## `impl PreLockChecked` › `pub fn incarnation(&self) -> &IncarnationId {`

This coordinator process's incarnation.

## `impl PreLockChecked` › `pub const fn pid(&self) -> u32 {`

The pid `.creating` records.

## `impl PreLockChecked` › `pub const fn runner_policy(&self) -> &RunnerPolicy {`

The policy resolved **before the worktree lock**, in full.

## `impl PreLockChecked` › `pub fn runner_policy_sha256(&self) -> &str {`

`runner_policy_sha256` of [`Self::runner_policy`].

## `impl PreLockChecked` › `pub fn private_root(&self) -> &Path {`

The authorized private root `R`, canonical.

## `impl PreLockChecked` › `pub fn private_dir(&self) -> PathBuf {`

`<R>/runs/<run_id>` — the locator the marker records and the only
private path this run is authorized to write.

## `pub struct PreLock<'a> {`

What the pre-lock checks read. Every field is an input; none is a handle to
anything this module could write through.

## `pub struct PreLock<'a>` › `pub selection: &'a RunnerSelection,`

`[runner]`, as PR1's config parse produced it.

## `pub struct PreLock<'a>` › `pub runtime: Option<&'a dyn ContainerRuntime>,`

The container runtime, for a `Container` selection.

`None` is a machine with no runtime seam wired, and a `Container`
selection against it refuses here rather than proceeding as if the
question had been asked and answered.

## `pub struct PreLock<'a>` › `pub private_root: &'a Path,`

The private root this command is configured with — explicit
`--private-root`, else [`crate::rundir::default_private_root`].

## `pub struct PreLock<'a>` › `pub ids: &'a dyn IdSource,`

Where the run id, the incarnation and the pid come from.

## `pub fn check(inputs: &PreLock<'_>) -> Result<PreLockChecked, UpstrokeError> {`

Perform the read-only pre-lock checks, in the packet's order.

1. The authorized private root: canonicalized, and refused when it is not an
   existing real directory. `run_creation` records the marker's `private_dir`
   "as a canonical path", and a root that is not there yet cannot be
   canonicalized — so the requirement is stated here, read-only, rather than
   discovered at P3a where the answer would already have cost a lock.
   [`crate::workspace_manager::WorkspaceManager::derive`] requires the same
   thing of the same directory.
2. The `RunnerPolicy`, by read-only inspection, and its digest.
3. The run id and the incarnation, which perform no effect at all.

### Errors

[`UpstrokeError::Refused`] when the private root is not a real directory,
when a `Container` selection has no runtime seam, or when the container
inspection refuses — an unreachable runtime, an image reference absent from
the runtime (there is no implicit pull), or an absent credential volume.
[`UpstrokeError::Io`] when the root cannot be canonicalized for any other
reason.

## `pub fn check(inputs: &PreLock<'_>) -> Result<PreLockChecked…` › `let run_id = inputs.ids.run_id();`

Last, and only once every refusal above has passed: `run_creation` puts
id generation after resolution and annotates it "(no effect)".

## `fn authorized_private_root(configured: &Path) -> Result<PathBuf, UpstrokeError> {`

`R`, canonical, or a refusal naming what is wrong with it.

Canonicalizing the **root** rather than `<root>/runs/<run_id>` is forced:
the run directory does not exist during the pre-lock checks and creating it
to canonicalize it would be the residue this phase is defined not to leave.
`rundir::prove_private_half_ownership` resolves the same way from the other
side — it canonicalizes `<R>/runs` and joins the basename — so the locator
this produces and the expectation the census computes are the same value.

## `fn authorized_private_root(configured: &Path) -> Result<Pat…` › `let canonical = std::fs::canonicalize(configured).map_err(|source| UpstrokeError::Io {`

`symlink_metadata` first, then canonicalize: a private root reached
through a link is legitimate (`/tmp` on macOS, a mounted home), and the
reparse-point refusal `startup_census` states is about the chain *below*
the runs directory. What is refused here is a root that is not a
directory at all — a file or a device would canonicalize happily and then
fail at P3a, after the lock.
