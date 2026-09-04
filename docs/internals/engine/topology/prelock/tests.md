# `src/engine/topology/prelock/tests.rs`

Extended notes for [`src/engine/topology/prelock/tests.rs`](../../../../../src/engine/topology/prelock/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `struct Inventory {`

A runtime that answers from a fixed inventory and records every question.

It performs no effect of its own: the four effectful methods return
canned values, which is why a module that may not *call* a
`ContainerRuntime` primitive may still implement the trait.

## `struct Ids;`

Fixed identities, so an assertion can name a literal.

## `struct Scratch {`

A scratch directory that **owns** its tree.

The predecessor was a `fn scratch(&str) -> PathBuf`: it created the
directory and handed back a path nothing owned, so every invocation left
its root in the temp directory forever — on the ordinary exit, on an
early return, and on the unwind a failing assertion starts. On this
project's build box a directory leaked per test is inode exhaustion,
which `df -h` reports as 72% full while every write fails — and the leak
is not hypothetical: 5050 `upstroke-prelock-*` roots had accumulated in
the temp directory by 2026-08-30, and five runs of this module after the
repair added none.

Both ends go through the run-directory funnel because this file is a
`TOPOLOGY_MODULE`: `std::fs::create_dir_all` and every `std::fs` removal
are denied in it, tests included. `RunDir.RemovePublicHusk` is the one
recursive delete a test here can reach — it removes a directory's
children and then the directory — because `RunDir.RemovePrivateHusk`
takes a [`crate::rundir::PrivateHalfProof`], and a pre-lock scratch root
is not the two-halves shape that mints one.

The naming is the predecessor's, unchanged: the pid and the thread id
keep two live fixtures apart, and reclamation is what this type adds.

## `impl Scratch` › `fn path(&self) -> &Path {`

The authorized private root a test hands to [`check`].

## `fn drop(&mut self)` › `assert!(`

A failed reclamation is the leak this type exists to prevent, so
it is reported rather than discarded — but never while a panic is
already travelling. A second panic out of a destructor aborts the
process, which would replace the test's own failure with an abort
and lose the report that says what actually broke.

## `fn a_host_selection_resolves_host_v1_and_carries_its_digest() {`

A host run resolves `host-v1`, digests it, and mints its identities —
and the digest is the one the marker will carry.

## `fn the_pre_lock_checks_leave_no_residue() {`

The pre-lock checks leave nothing behind — not the run directory, not
the private half, not a lock file, not a container.

## `fn the_pre_lock_checks_leave_no_residue()` › `for question in runtime.asked() {`

Every runtime interaction was a read.

## `fn the_container_inspections_run_in_order_and_the_first_failure_ends_them() {`

The four container inspections happen in `run_creation`'s order, and the
first failure ends it.

## `fn the_container_inspections_run_in_order_and_the_first_fai…` › `let unreachable = Inventory::default();`

An unreachable runtime never reaches the image question.

## `fn an_absent_credential_volume_refuses() {`

An absent credential volume refuses, and the digest is never computed.

## `fn a_container_selection_without_a_runtime_refuses() {`

A `Container` selection with no runtime seam refuses rather than
silently proceeding as though the inspection had passed.

## `fn a_private_root_that_is_not_a_real_directory_refuses_before_any_inspection() {`

A private root that is not there refuses read-only, before anything
else is asked.

## `fn a_private_root_that_is_not_a_real_directory_refuses_befo…` › `let absent = root.path().join("absent");`

The root the check is given is a child that was never created; the
guard still owns the scratch tree that child was named under.

## `fn the_authorized_private_root_is_canonical() {`

The recorded root is **canonical**, so the locator the marker carries and
the expectation the census computes are the same value.

## `fn a_scratch_root_is_reclaimed_on_every_exit_including_an_unwind() {`

Every exit reclaims the scratch tree — the ordinary one and the unwind,
which is the exit a failing assertion in any test above takes.

The panic hook is deliberately **not** silenced for the second half.
The hook is process-global and this suite runs in parallel, so a test
that takes it, installs a no-op and restores it can interleave with
another doing the same and leave the process with a no-op hook for good
— every later panic anywhere in the suite losing its message and
backtrace. The few lines this prints cost less than that.

## `fn a_scratch_root_is_reclaimed_on_every_exit_including_an_u…` › `create_private_dir(&path.join("nested"), &mut NoHooks).expect("a child of the root");`

A tree rather than a bare directory: the guard reclaims what a
test left under its root as well as the root itself.

## `fn a_scratch_root_is_reclaimed_on_every_exit_including_an_u…` › `let recorded = Mutex::new(None);`

The path is recorded from inside the closure rather than re-derived
here: re-deriving it would copy `Scratch::new`'s naming rule, and a
witness that agrees with a rule it restates proves nothing about it.

## `fn a_scratch_root_is_reclaimed_on_every_exit_including_an_u…` › `assert!(!path.is_dir(), "a deliberate failure, mid-test");`

The shape of a real failure: an assertion about the run that does
not hold, raised with the guard still in scope.

## `fn a_scratch_root_that_cannot_be_reclaimed_is_reported_rather_than_discarded() {`

A reclamation that fails is **reported**, not discarded.

`Drop` cannot return, so the alternative to reporting is silence — and
silence here is the same leak the guard exists to close, with nothing to
say it happened. The tree is reclaimed out from under the guard through
the very funnel the guard would use, so the removal it then attempts
fails for a real reason rather than an injected one, and the panic that
carries the report is caught here rather than failing this test.

## `fn scratch_unwind_with_a_failed_reclamation_child() {`

The child half of
[`a_failed_reclamation_during_an_unwind_does_not_abort_the_process`].

It drives the one corner of the guard's cross-product the two witnesses
above cannot reach: a reclamation that **fails** while a panic is
**already travelling**. `raii-reported` covers failure without an
unwind and `raii-unwind` covers an unwind without a failure; only both
at once reaches the `std::thread::panicking()` half of the assertion,
and only there does the alternative — a second panic out of `Drop` —
abort the process rather than fail a test.

Everything asserted here is asserted **in this process**, so the parent
needs no channel back: reaching the end of this body at all is the
claim, and the harness's own result line is how the parent reads it.

## `fn scratch_unwind_with_a_failed_reclamation_child()` › `remove_public_husk(root.path(), &mut NoHooks).expect("the tree reclaims early");`

Reclaimed out from under the live guard, through the very funnel
the guard will use, so the removal it attempts while unwinding
fails with `NotFound` for a real reason rather than an injected
one — no fault hook, no permission trick, no timing.

## `fn scratch_unwind_with_a_failed_reclamation_child()` › `let message = caught`

Reached at all only because the destructor did not panic a second
time: a panic out of `Drop` during this unwind aborts, and an
aborted process runs no assertion and prints no result line.

## `fn a_failed_reclamation_during_an_unwind_does_not_abort_the_process() {`

A reclamation that fails **while a panic is already travelling** does
not panic a second time: the process survives it, and the primary panic
is still the one that arrives.

Measured **from outside the process that makes the observation**, which
is forced. A second panic out of a destructor mid-unwind aborts, and an
abort takes the whole test binary — so an in-process witness for this
corner would have to survive its own subject. The child is the witness;
this is the frame that reads its exit.

The child is spawned **through the host Runner**, not through
`std::process::Command`: `std::process::Command` is on the effect
denylist and `src/engine/topology/**` may not reach it even in tests.
The Runner is the funnel that owns `Process.Spawn`, which is exactly the
rule — the same spawn `recover::tests::kill_during_recovery_repeats_recovery`
and `create::tests::spawn_and_wait` already use.

**Both assertions are load-bearing, and neither alone is enough.**
`abort()` takes the process before the harness prints anything about the
test, so an aborted child emits no `test result:` line — but a child
whose filter matched *nothing* also exits 0 and prints `ok. 0 passed`,
which a bare exit-code assertion would read as success. Requiring the
zero exit **and** `ok. 1 passed` separates the three outcomes: aborted,
selected-and-passed, and selected-nothing-at-all.

## `fn a_failed_reclamation_during_an_unwind_does_not_abort_the…` › `env: Vec::new(),`

Nothing to pass: the child derives its own scratch root from
the temp directory and its own pid, so the two processes
cannot collide and there is no state to hand over.

## `fn a_failed_reclamation_during_an_unwind_does_not_abort_the…` › `assert_eq!(`

`stderr` rather than the whole `ProcessOutput`: the child's stdout
carries its backtrace, and a failure report that buries the one line
that names the cause — `panic in a destructor during cleanup` — under
fifty frames of it is a report nobody reads.
