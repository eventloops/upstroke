# Standing finding ledger

Every review finding across every slice, with its disposition and whether it has recurred.
Accumulative and append-only. This file is **an input to every review**, not a record written
after one.

Per-PR ledgers in pull-request bodies stay as they are — `validate-pr-body.sh` enforces them and
they bind that PR's merge. This file is their union, in the repository, so a reviewer can read what
has already been settled before spending effort re-deriving it.

## Why this exists

Reviewers re-raise settled matters because nothing tells them a matter is settled. On slice PR3, six
concurrent lenses returned 196 findings and two independent skeptics killed 114 of them — a 58%
noise rate, some of it re-litigating questions already answered in a previous slice's ledger, which
lived in a pull-request body no reviewer was given.

## The authority rule

**The implementer holds the disposition.** A reviewer may not overturn one.

A reviewer *may* **append a challenge** to a settled entry, and should when it has something the
original disposition did not consider. A challenge is only admissible with **new evidence**:

- a **concrete failure sequence** the disposition did not address, and
- a **surviving mutation** — a specific edit the current suite would not catch.

A restatement of the original finding is not a challenge and should not be filed. Neither is a
preference: where the design is frozen, an equally valid alternative is not a defect.

Challenges go in §3. The implementer adjudicates them and either revises the disposition — appending
a new row, never editing the old one — or records why the challenge fails. **The middle ground is
the implementer's call.** That is deliberate: the implementer has read the frozen packet for that
slice and carries the consequence of getting it wrong.


## The boundary rule — for every review, not just gates

**A boundary you would have drawn elsewhere is not a defect when the design is frozen.**

Every fix draws a boundary somewhere, and a boundary can always be measured against *some* sentence.
A reviewer who does not separate "the packet forbids this" from "I would have drawn the line
elsewhere" will generate findings indefinitely, because each repair creates fresh boundaries to
object to. On PR3 that loop ran for three consecutive rounds.

**The test is a single question: can you quote a *live* packet passage that the current behaviour
fails to satisfy?**

- **Yes → a defect**, even if the implementer drew the boundary deliberately and documented it.
  `PR3-ST14-006` is the worked example: round 5 asserted the deferred-state legal transition only
  *below* the trace ceiling and said so in a comment, but `decisions.bounded_census.coverage_assertions`
  says **every** state with a Deferred task has at least one legal next transition. No exception
  exists in the sentence, so the finding stands.
- **No → not a defect.** `PR3-ST07-014`'s general half is the worked example the other way: the
  reviewer asked for a cumulative durable prefix per site and phase, but
  `fault_injection_registry.structure` keys entries by `EffectSiteId × phase × order × injection
  mode` and nothing else. A cumulative prefix is not a function of that key. The repair declined it,
  gave that reason, and made the boundary an executable test. Correct.

**"Live" is load-bearing.** The packet carries fourteen generations of disposition history inline and
superseded rationale reads exactly like specification. `*_verification_dispositions`,
`finding_dispositions[].rationale` and `v4_`..`v15_` keys are history; `decisions.*`, `invariants`
and `transaction_fault_matrix` are live.

**And say which you found.** A documented, counted, bounded boundary is not a concealed gap. Round
5's ceiling skip carried its rationale in a comment, counted the skipped states, and asserted
`deferred_states > at_ceiling` so the skip could not grow silently. The finding was still right — but
"narrower than required" and "hidden defect" are different things, and reporting the first as the
second misdirects the repair.

## Recurrence

The schema already tracks it and it must be used:

- `Provenance: fix_regression` means this finding is a *regression of a previous fix*.
- `First bad / prior ID` names the earlier finding it recurs from.
- `Regression or documented guard` names the test that now prevents it.

A finding whose `First bad / prior ID` is populated has happened before. Two occurrences of the same
class is a signal about the method, not about the slice — `PR1-ORDER-001-ABA` is the worked example:
a sound finding whose *fix* had a hole, caught only by a later independent pass.

## 1. Settled — do not re-raise without new evidence

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR3-LIMITS-SCHEDULING | P3 | ae9e9da+ / src/topology/events.rs:429 | TopologyLimits omits max_per_agent and max_per_pool -> claimed to break the durable trace contract | introduced_by_feature | correctness | — | rejected: live decisions.resource_accounting names both "process-lifetime ephemeral scheduler state", and ephemeral state does not belong in a durable event; canonical_trace_projection says what a comparison ignores, not what the log contains | rejected |
| PR3-PATHSET-WIRE-KEY | P3 | ae9e9da+ / src/topology/paths.rs:194 | a serde alias lets an alternate PathSet payload key deserialize, and no test rejects it | introduced_by_feature | compatibility | — | rejected as stated: the packet never names PathSet or freezes its payload key, so the demanded refusal has no live basis. The underlying gap — encoding compared only against itself — is repaired under the wire-pinning class | rejected |
| PR3-SATISFIES-ORDER | P3 | ae9e9da+ / src/topology/fold.rs:2306 | satisfies compared as a sorted set would accept a reordered closure | undetermined | correctness | — | rejected: no live packet passage requires satisfies to be ordered, and decisions.repairs says nothing about it. Production is stricter than required (Vec equality is order-sensitive); the authoritative fixture closure is single-element so no test can distinguish a relaxation. Forward note, not a defect | rejected |

| PR3-CENSUS-SKELETON-SCOPE | P2 | ae9e9da+ / src/topology/census.rs | 11 of 28 catalogue mutations survive against the ST-14 census skeleton | introduced_by_feature | correctness | — | **DISPOSITION OVERTURNED.** Confirmation 3 deferred all eleven wholesale to PR10. Confirmation 4 found that unsound and it is: the frozen contract's proof_tests names "ST-14 skeleton incl. **totality**, the pre-budget_exceeded prefix, the **deferred-state legal-transition assertion**, and the **fast relations**" — so nine of the eleven attack obligations PR3 itself owes. Repaired in round 5 | fixed |
| PR3-CENSUS-ATTRIBUTION | P3 | ae9e9da+ / src/topology/census.rs | repair round 2 attributed the accepted/refused movement (8423->7751, 254413->255085) to "the six new refusals" | undetermined | docs-contract | — | the numbers are credible but the attribution is FALSE: the census does not generate attempt_interrupted, merge_verification_unavailable or question_answered. Recorded so the wrong cause is not carried forward; the orchestrator had already written the attribution into its ledger as fact | rejected |
| PR4-CONF-001 | P1 | 3fcb360+ / src/runner/host.rs:223 | DESIGN.md:260 names `HOME`, `PATH` and credential locations role-scoped; `reserved_values` scopes only credentials, and `the_values_host_v1_does_not_scope_by_role_are_counted` positively *required* `PATH`/`HOME`/`USERPROFILE` to take exactly one value across the five roles — so implementing the sentence's plainest reading failed a test | introduced_by_feature | docs-contract | — | **DISPOSITION REVISED, not reversed.** The prior basis ("one machine, one user") was a rationale, not a passage, and the reviewer is right that a count cannot separate "the packet forbids this" from "narrower than I would have drawn". `host-v1` still supplies one value, now decided by three live passages, each forbidding a different part of a per-role value: DESIGN.md:263 ("Probe and execution compose the **same** base, mounts, reserved values, and overlay") over {`probe(<agent>)`, `implement`, `review`}; `decisions/2026-08-12-merge-queue-execution-topology.md:331-333` ("gate-shell/program availability is checked inside the same boundary") over {`probe(shell)`, `gate`}; and :341-342 ("Host runner behavior remains available and honestly provides no OS boundary around gate code"), which is why a per-role `HOME` for gate code would assert an isolation this host does not have while the credential *location* can honestly be withheld. The value is the base's because :321-322 says "the host base starts from the Upstroke process environment". Catalogue entries `PR4-CORE-016`/`-017` describe the shipped behaviour and are answered by those passages, not by the count. Test replaced by `the_reserved_values_every_role_gets_are_the_host_boundarys_own`, which asserts the pairings and the base-derived values and names the passage in each failure | fixed |
| PR4-CONF-003 | P1 | 3fcb360+ / src/engine/mod.rs:56, :118 | the frozen **public** engine facade never established the ambient job: `run_harness`/`resume_harness` built a `HostRunner` and entered the write coordinator directly, so a downstream crate calling `engine::run_with` or `resume_with` was a coordinator with no ambient job — a kill after `CreateProcessW` and before private-job assignment left the suspended stub alive (INV-18), and an ambient creation/join failure could not produce `expected_failures_refusals[1]`'s startup refusal, because establishment was never attempted on that path | introduced_by_feature | correctness | PR4-CONF-002 (same class: a guarantee proved for the entry point that was looked at) | **Accepted — a production defect, not a test gap.** Repaired by class rather than by instance: containment is now a capability. `runner::host::Contained` has a private field and is minted only by `contain_write_command()` after `proc::join_ambient_job` returns `Ok`; `coordinator::run_harness_inner_on` and `resume::resume_harness_inner_on` — the two write-coordinator entries — take `&Contained`, so **no** entry point, present or added later, can reach a spawn without having established containment first. Deleting the establishment is a compile error rather than a silent regression. The census is on the class: `engine::tests::every_public_write_coordinator_entry_point_establishes_containment` reads the six `pub fn` names out of `engine/mod.rs` itself, crosses them against the table of calls, and asserts each establishes exactly once (per-thread count, plus real ambient membership on Windows); `no_read_only_public_entry_point_establishes_containment` asserts the other six establish none. Ordering is a runtime fact too, on both platforms: `a_facade_run_refuses_before_any_effect_when_containment_fails` and its resume twin. The reconciliation's "every write command establishes it before any dispatch arm can run" was true of CLI dispatch and was generalised to every write coordinator; the CLI-only boundary is gone | fixed |
| PR4-CONF-004 | P1 | 3fcb360+ / src/runner/host.rs:2912 | the all-role containment grid hand-built its `Implement` and `Review` requests with `agent: None` and a *gate* identity, while production sends `agent: Some(<adapter>)` with a worker/review identity — so a `HostRunner::run` selecting `NoHooks` when `matches!(role, Implement \| Review) && agent.is_some()` ran every real worker and reviewer with no containment hooks and no fault injection, with the suite green | introduced_by_feature | correctness | PR2/PR3 correlated-fixture class (§4) | **Accepted.** Every role in the grid is now built by the builder production uses for it, and there are five: `shell_probe_request`, `agent::probe_request`, and the three added here — `runner::{worker_request, review_request, gate_request}` — which `engine::attempt`, `gates::ShellGate::check` and `review::run_review` now call instead of assembling a literal. `every_production_runner_request_is_built_by_its_roles_builder` censuses the tree so a sixth construction point has to be classified. Hostility is asserted as distinct-value counts (`the_role_grid_sends_the_shapes_production_sends`: 5 roles, 5 identities, 3 bound / 2 not, and `agent.is_some() == role.is_slotted()` per request, which is R3's rule rather than the fixture's). Witness: the mutation kills `every_role_reaches_the_containment_points_of_this_platform` and `a_fault_armed_at_any_containment_point_stops_any_role`; restoring the old fixture shape under the same mutation makes both pass again and fails the new count test instead | fixed |
| PR4-CONF-002 | P1 | 3fcb360+ / src/runner/mod.rs:242, src/runner/host.rs | every runtime containment observer built its request with `ExecutionRole::Gate`, so `HostRunner::run` passing `NoHooks` when `matches!(&request.role, ExecutionRole::Probe(_))` left both contract-named probe paths emitting no containment-hook evidence and un-fault-injectable, with the whole suite green | introduced_by_feature | correctness | PR4-SPAWN-SITE-PROBE-CONTEXT | **Accepted.** The count in `the_spawn_site_files_every_role_under_one_context_and_the_count_says_which` proves the site/context mismatch exists; it never proved the hooks execute on those roles, and §2's entry has been corrected to stop claiming it did. Runtime proof added for all five roles rather than the two named, because a suppression keyed on any single role is the same defect: `every_role_reaches_the_containment_points_of_this_platform` (points, packet order, and on Unix the kernel's answer that the pre-exec containment operation ran) and `a_fault_armed_at_any_containment_point_stops_any_role` (5 roles x 4 Unix / 3 Windows points). The site *variant* stays deferred to PR6/PR7 — it is `src/topology/effects.rs`, PR3's and frozen | fixed |
| PR4-CONF-005 | P1 | 3fcb360+ / src/runner/host.rs:682 | `contain_write_command` — the mint the frozen public facades (`engine::run_harness`, `resume_harness`) and `src/main.rs::dispatch` all reach — took no observer and used the real `windows_job::join_ambient`, which memoises, so **no test could drive its failure branch**. `let _join_outcome = proc::join_ambient_job(&mut NoHooks); Ok(Contained::new())` left the whole suite green: Linux cannot make the join fail at all, the guest's success paths still mint, and every simulated failure went through `HostRunner::start_write_command` or a closure injected at `engine::run_contained` instead. A Windows coordinator would then dispatch with **no ambient job**, and `expected_failures_refusals[1]`'s startup refusal could not be produced | introduced_by_feature | correctness | PR4-CONF-003 (same class: a containment guarantee proved for the entry point that was looked at) | **Accepted — a proof gap, not a behaviour defect; production was already correct.** The observer is now a parameter on `contain_write_command` and `start_write_command`, threaded exactly as `proc::run_with_timeout_hooked`'s already is and for the same stated reason (no machine here can make the real join fail); production passes `NoHooks` at every call site. `runner::host::tests::the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing` arms a refusal at `Spawn.AmbientJobJoined` and asserts the diagnostic reaches the caller (`ambient`, `INV-18`, `No process was spawned`), that nothing past the join was reached and no child exists, and that `containment_establishments()` did not move — then that the same call on its success path *does* mint, and that the unit-returning CLI entry refuses too. The class is closed by count as well as by case: `runner::tests::write_command_containment_has_one_join_site_and_one_mint` pins **one** `proc::join_ambient_job` call and **one** `Contained::new()` in the production region of the tree, and `Contained`'s constructor is private to `runner::host`, so no other module can mint one. Witness (Windows guest, `UPSTROKE_WIN_TAG=r6mut`): the mutation fails the named test with `must refuse: Contained(())`; the `start_write_command` variant fails it with `must refuse too: ()`; both pass on Linux, which is the invariant rather than a gap — `join_ambient_job` is a no-op there and does not consult the observer, deliberately, so a Linux cell cannot claim Windows coverage | fixed |
| PR4-CONF-006 | P1 | 3fcb360+ / src/runner/host.rs:2985 | every request in the role grid carried `stdin: Vec::new()`, while production's worker (`engine/attempt.rs:174`) and reviewer (`review.rs:695`) always carry the adapter's prompt — so `let selected = if request.command.stdin.is_empty() { &mut **hooks } else { &mut NoHooks };` in `HostRunner::run` ran **every real worker and reviewer** with no containment hooks and no fault injection while every hook and fault grid stayed green | introduced_by_feature | correctness | PR4-CONF-004 (same class, one field over: a fixture constant production never sends) | **Accepted.** The field list was re-derived from `RunnerRequest` and `CommandSpec` themselves rather than from intuition, which turned up two more constants of the same kind: every request ran the **recorded shell** although production's three agent-bound roles always run a located CLI, and every request carried `SHELL_PROBE_TIMEOUT` although production gives each role its own. All three are now production's own value per role — `agent_cli_command`/`shell_command`, the adapter prompt for worker and reviewer, and the five public timeout constants — and `the_role_grid_sends_the_shapes_production_sends` asserts all nine varying fields as distinct-value counts with the partitions checked against each other (the agent probe is bound and runs a CLI and still carries no prompt, so payload and binding cannot be mistaken for one field). The **identity** and **agent-binding** axes turned out to be larger than the five-role grid can express and are closed by two tests of their own: `every_production_invocation_identity_reaches_the_containment_points` (the shapes production builds that the grid never sends — `AttemptRole::ReviewReask(n)`, non-zero gate/pass indices, non-zero probe ordinals) and `every_shipped_agent_binding_reaches_the_containment_points` (all three ids in `CREDENTIAL_LOCATIONS`, where the grid names only `claude-code`). `runner::tests::every_production_command_spec_payload_is_classified` is the tripwire for the next one: it censuses every production `.stdin(`/`.env(` so a call site that starts populating a spec field must be classified before the grids can stay silent about it. Witness: each of the five mutations — keyed on stdin, on the program, on the timeout, on the identity, on the agent id — kills a named test (the first three kill `every_role_reaches_the_containment_points_of_this_platform` and `a_fault_armed_at_any_containment_point_stops_any_role`; the timeout one kills five tests) | fixed |

| PR4-CONF-010 | P1 | b1864dd / src/runner/host.rs:508 | `slice_contract.proof_tests[8]` names `host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing` **verbatim**, and that identifier was not in the tree. The CI-fix round renamed and decomposed it into separately-tested layers after Windows CI showed its child-`PATH` oracle invalid — the right diagnosis — but the decomposition lost the composition the contract requires, so `match run_shell_probe(self, …) { Err(e) if workspace.exists() && e.to_string().contains("os error 2") => Ok(()), o => o }` in `HostRunner::shell_probe` survived the whole suite: the positive case succeeds, the missing-*workspace* case has `workspace.exists() == false`, one case calls `runner.run` directly and the stub cases call the free `run_shell_probe` | fix_regression | correctness | PR4-CI-ENVIRONMENT-ASSUMPTIONS (the CI fix whose decomposition dropped it) | **Accepted.** The contract-named test is restored and composes all three conditions at once — an existing workspace, a recorded shell that is missing, and the call going through `HostRunner::shell_probe`. The CI fix's insight is kept: the absence is **constructed**, not hoped for. `pwsh` is probed from a child process whose entire `PATH` is one directory this suite created and asserts is empty, because one of the two `PATH`s std consults on Windows is the **process's** and a process cannot rewrite its own for one test without racing the binary. The helper asserts its premises before it asserts the claim — `PATH` is that one empty directory; on Windows `pwsh.exe` is in none of the three directories the search reaches whatever `PATH` says; the workspace exists before and after — so a premise that stops holding fails loudly instead of passing for the wrong reason. `PATH` is *replaced*, never removed: an absent `PATH` sends `execvp` to the confstr default `/bin:/usr/bin`, and the CI image really does ship `/usr/bin/pwsh`. Witness: the review's mutation verbatim fails `host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing` on Linux and on the guest. The gate that would have caught it is now in `phase9.sh`: it reads the frozen packet's own `proof_tests`, checks every bare-identifier entry is present in `src/`, and prints how many it checked and how many it skipped as prose | fixed |
| PR4-CONF-009 | P1 | b1864dd / src/runner/host.rs:541 | every agent role in the role grid runs `std::env::current_exe()` — an `.exe` — and the only `.cmd` this suite executed at runtime was `agent::bin::tests::a_batch_shim_runs_and_receives_its_argument`, which calls `build_command(&spec).output()` and bypasses `HostRunner` and its hooks entirely. So `if request.command.program.to_ascii_lowercase().ends_with(".cmd") { &mut NoHooks } else { &mut **hooks }` in `HostRunner::run` left **every real Windows agent CLI** running with no containment observation and no fault injection while the whole grid stayed green — and npm-installed agent CLIs on Windows *are* `claude.cmd`, `codex.cmd`, `copilot.cmd` | introduced_by_feature | correctness | PR4-CONF-006 (same class, one field over: a fixture constant production never sends) | **Accepted, and the process failure around it is recorded separately in §4.** The program shape is now its own axis, the way the identity and agent-binding axes already are, because the five-role grid asserts `programs.len() == 2` as *production's* rule (a bound process runs its agent's CLI, an unbound one runs the recorded shell) and a third program in it would assert something production never sends. `every_production_program_shape_reaches_the_containment_points` runs every shape this platform's production can produce through `HostRunner` under a witness and then under a fault at every containment point — Windows: a native `.exe`, a `.cmd` shim, a `.cmd` shim whose path contains a space (`C:\Users\John Smith\npm\copilot.cmd`, verbatim from `bin.rs`'s own fixture), a `.bat` shim, and the recorded shell's bare `PATH`-resolved name; Unix: the native executable, a shebang script, a shebang script whose path contains a space, and the bare name. Two axes varied independently and asserted as counts: the kind of file, and whether the path needs quoting. `the_cli_roles_of_the_grid_run_a_shim_shaped_program_through_the_funnel` then carries the shim shape through **all three** roles that run a CLI, each built by that role's own production builder, so a suppression keyed on the pair has nowhere to be green. Shapes enumerated and excluded, with the reason: a shell **builtin** is never a `CommandSpec.program` (`ShellKind::builtins` are command starters inside the shell's `-c` string), and a non-Unicode program path is `PR4-PROGRAM-PATH-NOT-UNICODE`, an owner question. Witness: the review's `.cmd` mutation verbatim fails the shape test on the guest; the space-keyed half of the same mutation fails it on Linux | fixed |
| PR4-CONF-008 | P1 | b1864dd / src/main.rs:282 | `run()`'s wiring closure returns `Result<(), _>`, so `|| { let _ = start_write_command(&mut NoHooks); Ok(()) }` fabricates success one level above the seam `PR4-CONF-005` closed. `dispatch` is driven with injected failures and `start_write_command` is driven with one on the guest, but nothing drove `run()`'s **composition** of the two — leaving `upstroke run … --dry-run` succeeding on a Windows host whose ambient job could not be established, against the slice `scope` ("refusal with diagnostic if it cannot") and `expected_failures_refusals[1]` | introduced_by_feature | correctness | PR4-CONF-005 (the same claim one level out) | **Accepted. The round-6 deferral of this finding was invalid and is withdrawn** — see the struck row in §2. `run` is now a single delegating expression over `run_wired(command, hooks)`, which threads the **observer** rather than the join closure, so `start_write_command` is inside the function under test instead of inside its untestable caller. Three tests, and each kills a different mutation shape: `the_cli_write_path_runs_the_real_containment_step` asserts `containment_establishments()` moves by exactly one on the write path and not at all on the read-only one, on both platforms (kills `|| Ok(())`); `a_cli_write_command_refuses_when_the_real_containment_step_refuses` arms a refusal at `Spawn.AmbientJobJoined` and asserts the CLI refuses with that diagnostic and never reaches its arm (kills `let _ = …; Ok(())`, on the platform where the join can fail — `join_ambient_job` is a no-op on Unix and does not consult the observer, the same boundary `PR4-CONF-005` records); and `the_cli_wires_the_real_containment_step_into_dispatch` reads `src/main.rs` and asserts the step is named exactly once in the code region and that neither `run` nor `run_wired` constructs an `Ok` of its own, which kills both swallow shapes **including the one in `run` itself**, on every platform. That census strips `//` comments before counting and asserts the strip worked, because `run_wired`'s doc comment quotes the mutation verbatim — `PR4-CENSUS-COMMENT-ORACLE`, handled rather than tripped over | fixed |

| PR5B-R28-COORDINATOR-WITNESSED | P1 | ff0490a+ / src/rundir.rs:1838 (the worktree-lease refusal), :1944 (the run-lock exclusive probe) | `PR4-R28-NEXT-COORDINATOR-UNWITNESSED` recorded two withheld-catalogue mutations surviving the whole suite because **no test starts a coordinator while a surviving reaper actually holds R28**: `PR4-WIN-073` turns the `cleanup::is_held` / exclusive-probe would-block branch from refusal into continuation, and `PR4-WIN-074` replaces the immediate refusal with a polling loop that waits for the hold to release and then continues. Both leave two engines able to overlap on one worktree while a reaper is still settling agent process groups | undetermined | correctness | PR4-R28-NEXT-COORDINATOR-UNWITNESSED (PR4 filed it out of its own scope and named this slice as owner) | **Accepted and closed by evidence.** `rundir::tests::a_surviving_reaper_hold_refuses_the_next_coordinator_until_released` spawns a **real second process** that opens `cleanup.lock` and takes the **shared** hold a reaper takes (`LOCK_SH`, per R28's "a surviving Unix cleanup reaper's *shared* cleanup.lock hold"), then drives a coordinator against it. Four assertions, and the two mutations die on different ones: `WorktreeLock::acquire_in` must refuse **and name the run** (kills `-073`, whose continuation would return `Ok`); the hold must **still be held when the refusal returns** (kills `-074`, whose polling loop can only return once the hold is gone — a state assertion, not a timing one, with an elapsed bound as a second signal); `RunLock::acquire`'s exclusive probe must refuse too, which is R28's *other* named observation point; and both must succeed once the reaper is reaped, so the test cannot pass by refusing everything. The run it uses is deliberately a **husk**, which is where the second half of this entry is: the scan used to walk `list_runs`, and PR5 makes that reader return committed directories only — so keeping it would have hidden precisely the holds it exists to observe, since the run whose reaper is still settling is the run that died before its log committed. The scan now walks `run_dir_names`, and the test asserts `classify_run_dir` calls the directory a `Husk` and `list_runs` returns nothing before it starts the reaper. Unix-only, deliberately: `cleanup::is_held` is `#[cfg(unix)]` and returns `false` on Windows, so a Windows cell would claim coverage it cannot have | fixed |
| PR5B-CLASSIFIER-TERMINATOR-UNTESTED | P1 | ff0490a+ / src/rundir.rs:909 | `classify_run_dir` is `Committed` **iff a newline-terminated** valid first-line `run_started`, and the twenty-shape grid did not test the terminator. The shape that was *about* it — `torn-first-line` — truncated the last 8 bytes, which removes the newline **and** breaks the JSON, so it refused on the parse. Measured: `first_committed_line` rewritten to `.position(\|b\| *b == b'\n').unwrap_or(window.len())` — treating end-of-file as end-of-line — survived all 20 shapes and the whole suite. A run whose log was killed mid-first-line would then have been classified `Committed`, listed, and resumed from a record no writer ever finished | introduced_by_feature | correctness | the PR2/PR3/PR4 `bounded_grid` class — a fixture varying two things at once, so it refuses for the wrong reason; fourth consecutive slice | **Accepted, found by this lane's own mutation run, fixed the same round.** New shape `complete-first-line-with-no-newline`: a complete, valid, parseable `run_started` whose only defect is the missing terminator, expected `Husk`. It isolates the terminator because it varies *only* the terminator — the JSON is byte-identical to the `committed` shape's. `every_publication_prefix_classifies_as_the_packet_names_it`'s class counts move 5/15 → **5/16**, so the grid cannot shrink back silently. Witness: the mutation above now fails that test; it passed before | fixed |
| PR5B-PUBLICATION-ATOMICITY-UNPROVEN | P1 | ff0490a+ / src/rundir.rs:474 | `proof_tests[6]` requires "atomic marker, owner-record and commit-record publication tests", and `a_kill_between_stage_and_rename_leaves_only_the_tmp` does not prove atomicity. It kills at the publication site's `Before` phase, where a rename and a copy-then-delete have **both done nothing**, so the assertions hold identically for either. Measured: `publish` rewritten as `fs::copy` + `fs::remove_file` survived that test and the whole suite. Copy-then-delete truncates the destination and then fills it, so a death inside it leaves a **partial** published record where `T-RUNSTART` requires either the old one or the new one — and for `committed.json` a partial record is one the ownership proof reads to decide whether a private half may be deleted | introduced_by_feature | correctness | the same class as `PR4-CONF-005`: a branch no test could drive, green because the harness could not reach it | **Accepted, found by this lane's own mutation run, fixed the same round.** `RunDirSite::sub_effects()` is empty for every site in the frozen inventory, so there is no coordinate *inside* the primitive to place a fault at, and the discriminator has to be an observable that survives a **successful** publication. `publication_replaces_the_name_rather_than_writing_through_it` hard-links a sentinel file to the destination, publishes, and asserts the sentinel's bytes are untouched: `fs::rename` re-points the directory entry, `fs::copy` opens that same file through the link and overwrites it. Run for all three publications (marker, owner record, commit record). Portable by construction and needing no `st_ino` — Windows does not expose `MetadataExt::file_index` on stable Rust — and confirmed executing on the guest. **Residual limit, stated rather than papered over:** the suite now proves publication *is* a rename and that a kill *before* it leaves only the `.tmp`; it proves nothing about a kill *during* `fs::rename` and cannot while the inventory is frozen, so that step rests on the filesystem's own rename atomicity | fixed |

## 2. Open — carried deliberately, with an owner

| ID | What | Owner | Why it is open |
|---|---|---|---|
| PR5-VERIFY-CLAUSE-NARROWER-THAN-STATED | `slice_contract.proof_tests[8]` says each of **eight** synthetic residue elements "classifies Internal, **fails `Worktree.Verify`**, and forced removal succeeds", and `command_internal_sub_effects` says the same of its synthetic evidence. For **two** of them — `UnreferencedObject` and `TemporaryObjectFile` — the suite asserts `Worktree.Verify` **passes**, and the implementation (`element_breaks_quiescence`, `src/workspace_manager.rs:2802`) says so on purpose. Twelve of the frozen 24 (site, element) pairs satisfy the clause and twelve cannot | project owner — **for the G2 erratum list** | **The behaviour is right and the sentence is over-general; recorded because an unrecorded live sentence the behaviour does not satisfy is a defect until an owner rules.** Both elements live in the *shared object store*, are R27 ("Git's"), and are left by ordinary Git use — every amended commit leaves an unreferenced object. A `Worktree.Verify` that consulted the object store would fail on essentially every worktree in every real repository, and `decisions.workspace_candidates.generation` requires a quiescent worktree to be **reusable**; forcing the clause would make `OpenNoAttempt` reuse impossible and the tabled recovery non-convergent. Measured rather than argued (PR5-CONF-006, Fable PR5-CONF-003): Sol predicted a survivor and the flip of `element_breaks_quiescence` is **KILLED** — the partition is pinned hard in both directions. What it is pinned *against* is the implementation's own `const fn`, which is the reason this row exists rather than a repair: the suite cannot both hold the packet's sentence and hold the behaviour. **Not repairable in this slice** — the alternative is failing quiescence for every innocent worktree — and the erratum wanted is one clause on `proof_tests[8]` naming the two object-store elements as exceptions |
| PR3-ATTEMPT-SHAPE | Whether `AttemptSettlement` can represent the frozen atomic `attempt_finished` incl. the allowance decision | project owner | Turns on whether `finding_dispositions[].design_changes` and `transaction_fault_matrix` impose field requirements on event shapes. `decisions.tests_acceptance.seam_tests[14]` is live and names `attempt_finished{Retained, Retry{resume:true}}`. Forward constraint on PR7/PR11  **RULED 2026-08-25, and the concrete form is sharper than the line above — appended, not rewritten.** The question is not whether `AttemptSettlement` has room; it is **whether the allowance decision is derived or carried**. `attempt_finished` records `SettlementTransition` (`Succeeded`, `Retry`, `Escalated{rung}`, `Deferred{defers,reason}`, `Parked{question}`, `Failed{halts_run,reason}`) and **nothing saying whether the attempt consumed one of the rung's `attempts_per`**, while the schema-4 fold carries no `attempts_on_rung` — `GenerationFold.attempts` is the highest attempt *number started*, which restarts at an escalation. The legacy engine keeps `LadderState` in memory and never replays it; a resume has only the log. **Owner ruling: DERIVED, not explicit.** The wire does not change — a recorded conclusion beside the recorded fact it derives from is an internal-disagreement channel inside one event, which is `predicted_region`'s disease relocated into the wire. **One named total function over `SettlementTransition`, engine-side this slice**, on the ladder's side because `next_step` is its sole consumer, pinned by the one-implementation census; the house template is `GenerationLease::expected` ("Total, and the whole of the rule"). Relocating the rule onto the vocabulary type, and whether the fold should validate allowance on replay the way `check_attempt_started` validates a binding, are **G2-pass items** — no new logic enters `src/topology/**` this slice. **Live citations, per cell, and they are thin**: `transaction_fault_matrix[7]` (T-FAILED) `durable_state` names the "allowance decision" among what becomes durable and its `resume_action` ends "**never re-decide**"; `transaction_fault_matrix[2]` (T-ATTEMPT) gives interruption — "append `attempt_interrupted` (unknown spend, **allowance refunded**...)"; and `decisions.coordinator_integration.dispositions` gives the only "**no attempt burned**" in the packet — measured, one occurrence in every live key — for an Infrastructure→Deferred **merge verification**, which is an analogy to the attempt path rather than a statement about it. `Retained`, `Retry`, `Escalated` and `Failed` have **no direct citation** and are implemented on the owner's stated presumption that they spend. **`Parked` the repository cannot decide**, and it returns as a follow-up owner question rather than being hard-coded either way  **`Parked` resolved 2026-08-25 by legacy precedent, which overrides the proposed default.** The check asked for — can the legacy engine park an attempt, and what does its ladder count — has an operative answer, so `invariants_preserved[1]` decides it. `ladder::next_step` reaches `AskHuman` by **four** paths that do not agree: `NeedsHuman` (*"the code was never judged, so **nothing is spent** and nothing escalates"*); `ReviewInputTooLarge`/`ReviewInputOpaque` (*"The worker ran, so the attempt **is spent** and must stay in the ledger"*); an outage at `max_defers`, whose sibling comment refuses to "burn attempts on a run that never got a verdict"; and chain exhaustion, reached only once `attempts_on_rung >= attempts_per`, so the retries already spent them. **So a park never spends *by being a park*.** The legacy rule is that an attempt spends iff **the worker ran and produced work to judge** — `NeedsHuman` is the agent declining to work, an outage is no completed run, and `ReviewInput*` is a completed run whose diff could not be judged, which still spends. **Consequence for the derivation, and it refines part 2 of the ruling**: the total function cannot key on `SettlementTransition` alone, because `Parked` is not one cell — it is four, separated by `AttemptRecord.failure`. It stays derived and the wire still does not change, because `attempt_finished` carries **both** the record and the settlement, so a replaying resume has everything the function needs. The function is total over the *event*, not over the transition. **G2 erratum stands**: the packet states none of this — its only attempt-path allowance citations are interruption and, by analogy, a merge-verification deferral — and the pass should give it the cell  **G2 ERRATUM TEXT, 2026-08-25 — the exact wording the pass should carry into the packet, so the erratum inherits the rule and not four examples.** The rule: *"An attempt spends one of its rung's `attempts_per` iff the worker ran and produced work to judge."* And the design's own words for the cell that decides it, from `engine::attempt::review_failure`: *"§12: the reviewer declined to judge and asked for a person. That is **not a rejection of the code**, so it **must not spend an attempt or escalate** — it parks the task and asks."* The second is the citation the first is derived from, and both belong in the packet: the rule alone would let a later reader re-litigate the boundary, and the citation alone would leave them to induce the rule from one case. Landed as `ladder::spends_allowance`, total over `FailureKind` — the exhaustive match immediately caught two variants the author had not seen, `Interrupted` and `Declined`, which a default arm would have answered silently in the direction that costs an operator a rung. `Interrupted` is the one cell the packet already states, and it agrees: T-ATTEMPT's "allowance refunded" and the variant's own doc ("hands the task back to the scheduler still on the same rung") are two independent sources with one answer |
| PR5-MACOS-CLIPPY-NEVER-RUN | `cargo clippy` still runs on **no macOS runner**, so the five `#[cfg(target_os = "macos")]` regions in the crate are outside the effect denylist's reach on every job CI runs — the Windows half of exactly this hole is `PR5-CONF-014`, repaired this round by the `lint (windows)` job. The denylist is rustc-resolved, so it denies precisely what the compiler compiled | project owner / the slice that next opens `.github/workflows/ci.yml` | **Measured, and the measurement is why the gate was not simply added.** Cross-compiled clippy from this Linux box is **clean at `-D warnings` for both darwin targets** — `cargo clippy --target x86_64-apple-darwin` and `--target aarch64-apple-darwin`, `--all-targets --all-features`, rc=0 (`logs/repair3/macos-cross-clippy.log`, `macos-arm-cross-clippy.log`). That is evidence and not a native run: this project has no macOS guest, and the standing rule here is that a mutation quoted in a review is a Linux mutation until it has run on the platform it is about. Adding an unmeasured gate is how `PR5-CONF-014` got a red CI in the first place. **One thing the cross-run did find and this row carries forward**: both darwin targets emit `warning: \`libc::pipe2\` does not refer to a reachable function` — a denied path that resolves on Linux and not on macOS, which is the "a denial that enforces nothing" class `clippy.toml`'s own header warns about, and which `every_denied_path_this_host_can_resolve_does_resolve` cannot see from a Linux host |
| PR5-ANSWER-MODULE-COLUMN | `effect_sites.json` ships `"module": "src/interaction.rs"` for `Answer.StageWrite`, `Answer.PublishRename` and `Answer.Ingest`; the `AnswerSite::` literals are at `src/rundir.rs:899`, `:912` and `:934` and nowhere else. The column is `EffectSiteId::module()`, generated from `src/topology/effects.rs` | PR6/PR7 implementer (the slice that next opens `src/topology/effects.rs`) | **The artifact's claim is corrected; the column is not, and cannot be from here.** `effects/funnel-modules.json` is generated beside `effect_sites.json` from the tree's own answer, carries every site and names the three that disagree, and is compared byte-for-byte — so a gate report now carries the correction alongside the claim. The column itself lives in a file frozen under the owner ruling of 2026-08-20, and moving the three funnel bodies to satisfy it is the other thing a slice may not do: they close over `rundir`'s private `funnel`/`RunDirHooks`, and `mechanism` (2) is the packet's own placement. Sol ruled this a low defect (`PR5-CONF-018`) and Fable a preference; the disagreement is over whether a false `module` column matters when enforcement is unchanged, and it is narrow either way — both files are allowlisted funnel modules and `interaction.rs`'s delegations are denied as wrappers |
| PR3-RUNNER-DIGEST | The packet contradicts itself: `decisions.task_registry.validation_at_fold` requires the container image digest "when Container"; `INV-23` has it "when reported" | project owner | A Container run whose runtime reports no manifest digest is legitimate under one reading and refused under the other. PR3 implemented INV-23 consistently across A1 and A2 and said so per refusal |
| PR3-REG-001-CONDITIONAL | `A3-REG-001` is equivalent *for the current inventory*, because every constructible site exposes zero or one observable order | PR4-PR10 implementer | It becomes live debt the moment any site exposes more than one observable order. Conditional debt, not closed |
| PR3-BEFORE-PHASE-SCOPE | Before-phase rows name the site's own artifact, not the transaction's whole durable prefix — so `Worktree.Add/Before` is empty although R9 already holds the intent | PR7–PR10 implementer | Chosen deliberately by repair round 4, documented on the type and asserted as a test so it reads as a decision rather than an omission. The repair itself names it as the largest remaining place a finding could live, in either direction |
| PR3-COMMIT-AUTHORSHIP | PR3's commit will be authored `Cameron Lambert <cameronlambert84@gmail.com>` (the repo-local git config) while the five commits beneath it on `codex/parallelism-design` are `upstroke <upstroke@upstroke.local>` | project owner | Cosmetic and unenforced: no CI gate checks authorship and CONTRIBUTING has no sign-off requirement. The repo already carries four identities in normal use (Cameron Lambert 72, upstroke 46, t 46, GitHub noreply 14). Left as configured rather than silently changed; overriding is one `git -c` flag if preferred |
| PR3-CONTAINER-START-ROW | `Container.Start → Present` is the least obvious row in the semantics table | PR6/PR7 implementer | Flagged by repair round 4 as the row most worth a second opinion |
| PR3-FRAMEWORK-SILENT-1 | Non-releasing removals leave `rows: []` — the packet fixes the pruning case (R27) but says nothing about removals with no objects to release | PR7–PR10 implementer | Derived by applying the pruning reading: the row that accounted for what was removed no longer holds it. After stays distinguishable from Before by artifact (`Removed` vs `Nothing`) and by action |
| PR3-FRAMEWORK-SILENT-2 | Read-only sites' After phase leaves nothing | PR7–PR10 implementer | Derived from the packet's "performs no effect", not stated by it |
| PR3-FRAMEWORK-SILENT-3 | `Container.Stop` is `Referenced` (only `Remove` ends a container); `Lock.ProbeCleanupExclusive` is `Referenced` | PR7–PR10 implementer | R17 accounts for the hold while held and is process-local OS state the kernel releases at death |
| PR3-FRAMEWORK-SILENT-4 | `Event.OpenLog`'s `Create` and `TruncateTornTail`: kill → `NextOpenConverges`, error-return → `RefuseResumably` | PR7–PR10 implementer | The packet elaborates only `SyncPrefix`, giving one action in both modes; this table gives one action in both modes by the same shape |
| PR3-FRAMEWORK-SILENT-5 | Windows and Unix containment kills get distinct actions (`AmbientHandleTerminates`, `ReaperSettlesGroup`) though the packet's residue answer is "none" for both | PR7–PR10 implementer | The mechanisms the packet states are different, and a table that merged them would survive a swap |
| PR3-REPORT-DOUBLE-NAME | `RunDir.WriteReport` and the `Report` group both name `report.json`, so ST-07 will demand two hook executions for one write | project owner | Found by A3, implemented as written and reported |
| PR4-SPAWN-SITE-PROBE-CONTEXT | `Process.Spawn` is one site with one adjacency (`After(AttemptStarted)`) and one fault row (`T-ATTEMPT`), but PR4 routes five roles through it and two — `Probe(Shell)` and `Probe(Agent)` — are `RunnerPreflight`, ordered at **P4**, before P6's `run_started`. A crash prefix at a probe spawn is effect-before-`run_started` (T-RUNSTART fresh, T-RESUME on resume) while the site it is filed under says event-before-effect in T-ATTEMPT. ST-07 evidence over `Process.Spawn` therefore does not cover the probe prefixes | PR6/PR7 implementer | **Cannot be repaired in this slice.** The site enum, its adjacency and its fault row are `src/topology/effects.rs` — PR3's, frozen at review — and a probe context would be a *new variant* of an inventory `decisions.effect_site_inventory` enumerates. Raised as `PR4-SEAMS-001`. What is deferred is the **site variant** — a probe-specific semantic context, its adjacency and its fault row — and that stays deferred. `runner::tests::the_spawn_site_files_every_role_under_one_context_and_the_count_says_which` transcribes the site's adjacency and fault row from PR3, classifies all five roles, and asserts that exactly **2** spawn outside the context the site names, so the gap cannot grow without failing. **That count is not a discharge of the hook obligation and this entry no longer claims it is** (corrected in round 4, `PR4-CONF-002`): counting that two roles fall outside the site's declared context proves the mismatch exists; it does not prove the containment hooks execute on those roles, and a `HostRunner::run` passing `NoHooks` for `Probe(_)` left the whole suite green. The hooks *firing on both probe paths, observed and fault-injected at runtime*, is PR4's by `scope` and `proof_tests[3]`, was never deferrable, and is now held for all five roles by `runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` and `runner::host::tests::a_fault_armed_at_any_containment_point_stops_any_role`. Recorded here because that test's own doc comment says this file carries it **OWNER RULING, 2026-08-20: the frozen files stay frozen.** PR4 does not change `src/topology/effects.rs` or DESIGN.md:222. This is an **accepted deviation**, not an open question and not a defect to be repaired in this slice: the repair requires editing a file an earlier slice froze, and a slice may not quietly redesign what it implements. **Revisit at G2** if it is raised repeatedly there. Under the authority rule this is now settled — a reviewer may still append a challenge in §3, but only with evidence the ruling did not consider, and 'a live passage is violated' is not new evidence: that is the fact the ruling was made about. |
| PR4-REG-001-STILL-EQUIVALENT | `PR3-REG-001-CONDITIONAL` becomes live debt the moment any site exposes more than one observable order | PR4–PR10 implementer | **Re-checked, still conditional.** The same test asserts `Process.Spawn.observable_orders() == [EventBeforeEffect]` — one order — so the order-free registry key stays equivalent for the one site this slice uses. Not closed; re-measured |
| PR4-R28-NEXT-COORDINATOR-UNWITNESSED | `src/rundir.rs`'s next-coordinator cleanup-hold check is unwitnessed from both ends. Two withheld-catalogue mutations survive the whole suite: `PR4-WIN-073` turns the `cleanup::is_held` / exclusive-probe would-block branch from refusal into continuation (`src/rundir.rs:383-396`, `:713-747`), and `PR4-WIN-074` replaces the immediate refusal with a polling loop that waits for the hold to release and then continues. Neither is caught, because **no test starts a coordinator while a surviving reaper actually holds R28** | PR5–PR7 implementer (the slice that owns `rundir`) | **Out of PR4's scope, deliberately.** Packet keys: `decisions.resource_accounting.rows[R28].lifecycle.held` and `invariants[17].recovery` (INV-18). PR4's `slice_contract.owned_resources` names **R22, R4 and RunnerPolicy** and its `scope` does not include `src/rundir.rs`, so the refusal these two attack belongs to another slice's ledger. What PR4 does own of R28 is the *reaper's* side, and that is now witnessed: `agent::proc::termination::tests::the_reapers_cleanup_hold_is_shared_between_overlapping_invocations` pins the hold as shared (`PR4-WIN-072`), and `agent::proc::tests::every_unix_containment_point_is_measured_against_its_own_operation` asserts that at `Spawn.ReaperStarted` an exclusive probe of the live lease is already refused. Recorded rather than dropped so the coordinator half is visible as owed |
| PR4-DESIGN-ROLE-SCOPED-ENV | **A wording ambiguity inside one paragraph of DESIGN.md.** :260 says the runner *"supplies role-scoped `HOME`, `PATH`, and credential locations"*; :262-264, three lines later, says *"Probe and execution compose the **same** base, mounts, reserved values, and overlay, so pre-flight certifies the environment that will actually spend."* Probe and execution are **different roles**, so a per-role `HOME` or `PATH` value makes pre-flight certify an environment the attempt will not run in — the second sentence constrains how the first must be read | project owner | Raised by the independent final confirmation as `PR4-CONF-001`, which read :260 alone. PR4 resolved it by scoping **credential locations** by role while `HOME`/`PATH` stay the host boundary's own, and grounded that in :263, packet :331-333 and :341-342 — the only reading that satisfies both sentences. Two pre-existing tests already enforced the second sentence by name, so the alternative reading would have required deleting a guard on the passage it implements. **Recorded rather than closed** because the ambiguity is in the source document, not in the code, and the same shape as `PR3-RUNNER-DIGEST`. If the owner reads :260 as requiring per-role values, PR4's disposition is the thing to revisit, and it is a design change rather than a repair |
| PR4-PROGRAM-PATH-NOT-UNICODE | **A conflict between two frozen passages, not a bug in a function.** DESIGN.md:222 freezes `struct CommandSpec { program: String, … }`. `bin::Invocation::spec` therefore refuses a resolved agent-binary path that is not valid Unicode — legal on Unix, where a path is bytes — where pre-PR4 `Command::new(&self.path)` carried the `PathBuf` through unchanged and that installation ran. So `invariants_preserved[1]` ("legacy engine behavior unchanged") is **unsatisfiable given the frozen shape**: the value cannot be represented at all, and both available behaviours fail. The alternative, `to_string_lossy`, replaces each invalid byte with `U+FFFD`, so the runner spawns a path that names *nothing* and the run dies at `execvp`/`CreateProcess` pointing at a path the operator never wrote | project owner | Raised by the third independent final confirmation as `PR4-CONF-007`. **Cannot be repaired inside PR4**: the repair that restores the old behaviour is widening `CommandSpec.program` to an `OsString`, and that is a change to DESIGN.md:222, not to `Invocation::spec`. The slice chose to fail **at the boundary that cannot represent the value** — naming the path, saying why, and never mistakable for a missing installation — rather than at the spawn; the function's own doc comment records that choice and its rejected alternative. `agent::bin::tests::a_program_path_a_string_cannot_carry_is_refused_by_name` documents the chosen behaviour and was deliberately **not** changed in repair round 6: changing it would be resolving an owner question inside a repair round. Third packet-level conflict of this slice, alongside `PR3-RUNNER-DIGEST` and `PR4-DESIGN-ROLE-SCOPED-ENV`, and the same shape as both — the resolution is a design decision **OWNER RULING, 2026-08-20: the frozen files stay frozen.** PR4 does not change `src/topology/effects.rs` or DESIGN.md:222. This is an **accepted deviation**, not an open question and not a defect to be repaired in this slice: the repair requires editing a file an earlier slice froze, and a slice may not quietly redesign what it implements. **Revisit at G2** if it is raised repeatedly there. Under the authority rule this is now settled — a reviewer may still append a challenge in §3, but only with evidence the ruling did not consider, and 'a live passage is violated' is not new evidence: that is the fact the ruling was made about. |
| PR4-ADAPTER-RESOLVES-ON-THE-HOST | Adapters resolve the agent CLI on the coordinator host and put the absolute host path in `CommandSpec.program`, so a boundary with its own filesystem is never asked what it has | PR6 implementer | **Ruled hardening, not a defect** — the full entry, its live passages and what breaks at PR6 are in the hardening-rule table below. Listed here too because §4's rule is mechanical: a round that names a surviving mutation and does not repair it files it here in the same commit, with an owner. The live-passage test is `agent::built_program_tests::an_adapters_program_is_the_coordinator_hosts_and_the_boundary_supplies_none` |
| ~~PR4-MAIN-WIRING-UNWITNESSED~~ | — | — | **DEFERRAL WITHDRAWN as invalid; repaired in round 8.** See `PR4-CONF-008` in §1. The round-6 deferral rested on *when the finding arrived* relative to that round's fixed scope — a **process** reason. Scope was never the issue: PR4's `scope` names "on Windows the process joins one ambient kill-on-close Job Object at write-command startup (refusal with diagnostic if it cannot)" and `expected_failures_refusals[1]` names the refusal, and the CLI is the entry point they describe. A process reason cannot defer a contract obligation, and this row no longer claims it can |
| PR5-CAPACITY-NOT-A-TOPOLOGY-RESOURCE | Whether agent-model **capacity** — the provider window a worker or reviewer spends against — is a resource the parallelism topology *brokers*, or ambient state it discovers by failing. Today it is ambient. The three ceilings are parsed, validated and carried (`src/config.rs:439-447`), and two of them already say "acted on by the topology engine", but none of them is a *provider* budget: they bound how many attempts run at once, not how much window remains to spend. The only capacity feedback the engine has is retrospective — `capacity::retire_signals` marks a pool exhausted **after** an attempt came back `RateLimited` (`src/capacity.rs:376`), and the ladder then defers without spending an attempt (`ladder::rate_limits_defer_without_spending_an_attempt`). Nothing admits work *against* a budget, and no topology row models a permit | project owner | **Deferred to PR11 deliberately, not overlooked.** Three reasons, in ascending weight. (1) **The packet is frozen and the freeze is the method.** A capacity permit is a new row in a frozen contract. The owner ruling of 2026-08-20 held the line on `PR4-PROGRAM-PATH-NOT-UNICODE` and `PR4-DESIGN-ROLE-SCOPED-ENV` — two findings that violate *live passages* — rather than edit a frozen file. Amending the packet for a finding that violates no live passage, while those two stay accepted deviations, is the inconsistency a reviewer sees first. (2) **There is nothing yet to model it in.** PR11 is where the coordinator brokers concurrency; a permit is that same shape, so building one before the broker exists means inventing a second mechanism PR11 must then reconcile or discard. The ledger already places it there: `PR3-LIMITS-SCHEDULING`'s disposition rests on live `decisions.resource_accounting` naming `max_per_agent` and `max_per_pool` "process-lifetime ephemeral scheduler state" — a permit is that same kind of state, so it belongs in the scheduler PR11 builds and not in the frozen durable contract. (3) **The data to specify it does not exist.** DESIGN.md:656 (§23.2) records what the first real runs measured, and the capacity side of that is a single usage-limit event across five slices — not a distribution a fault row can be written against. **What is worth doing before PR11, and touches nothing frozen:** (a) make capacity exhaustion *distinguishable in the record at the launcher* — inside the engine `FailureKind::RateLimited` is classified and durable, but an agent invoked outside it that hits a provider limit and one that dies leave the same trace, which is why ruling limits out after the PR4 deaths took a transcript grep rather than a query; (b) carry provider identity as configuration rather than an ambient credential file — needed anyway for the cross-vendor reviewer, and the same seam `PR4-DESIGN-ROLE-SCOPED-ENV` names from the environment side (`CREDENTIAL_LOCATIONS`, DESIGN.md:260). Both produce the measurement (3) is missing, so PR11 can specify against evidence instead of intuition. Forward constraint on PR11, carried the way `PR3-ATTEMPT-SHAPE` is |

| PR5-C-FSYNC-UNOBSERVABLE | **Deleting the `sync_all()` call in `events::log::sync_log_file` is undetectable by any test on this machine.** An fsync has no user-space observable effect: the ledger entry the suite reads would still be written, the byte length would still be the filesystem's own answer, and only a power loss could tell the difference. Every `SyncPrefix` test therefore proves that the funnel *reached* the sync and *recorded* it, not that the data reached the platter | PR7–PR11 implementer (the slice that owns the two-crash proof) | **Carried, not hidden.** The residual boundary is stated on the function itself (`src/events/log.rs:934`) rather than left for a reviewer to discover, and the mitigation that *is* possible is taken: the sync and its ledger entry are **one call**, because with them written as two statements a mutation that moves the `SyncPrefix` consult to *between* them puts the injection after the syscall and before the only thing that can see it — measured surviving the suite. Fused, the only place the consult can move to is after the record, where `an_injected_sync_failure_at_open_names_syncprefix_and_hands_out_no_handle` kills it. The packet names the test that would close this for real — `transaction_fault_matrix[T-PREPARED].test`'s `unsynced_merge_prepared_two_crash_barrier_before_cas_then_power_loss_keeps_log_and_ref_agreeing` — and it needs a coordinator, a CAS and a simulated power loss, none of which are PR5's |
| ~~PR5-R1-PROCESS-START-CENSUS-UNSTRIPPED~~ | — | — | **CLOSED by PR7's census repair.** All four whole-tree censuses — `every_production_runner_request_is_built_by_its_roles_builder`, `every_production_process_start_is_classified`, `write_command_containment_has_one_join_site_and_one_mint` and `every_production_command_spec_payload_is_classified` — now count over `effects::production_code`, which blanks comments **and** string literals, so a doc comment naming a needle can no longer change an expected number. Every expected count was re-derived over code: `src/agent/proc.rs`'s `run_with_timeout` row went 8 → 5 (three of the eight were doc comments, so deleting two sentences bought a real ninth entry point), and the `src/effects.rs` row was deleted outright because its only `Command::new(` is inside a `DENIAL_FIXTURES` string. The class remains `PR4-CENSUS-COMMENT-ORACLE`; what closed this instance was moving the blanking into the shared region rather than into each census |
| PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN *(re-scoped: `externally_reachable_fns` only)* | `effects::production_region` cuts a file at its **first** `#[cfg(test)]`, so a test-only item placed among production items removes every item below it from the **wrapper-classification** domain — silently, and `mechanism` (3)'s "every pubfn of a legacy or shared module is classified" would then be true of a domain nobody drew. Measured: adding `Invocation::at` inside `impl Invocation` took five of `src/agent/bin.rs`'s functions out of the census. **Scope as of PR7:** `effects::externally_reachable_fns` and the three censuses in `src/runner/container/exec.rs` are what still read the truncating region; the four whole-tree censuses no longer do | PR7+ implementer (the slice that owns `effects::externally_reachable_fns`) | **Two of three parts closed; the third is what this row now is.** (1) The instance is repaired: the constructor lives in a `#[cfg(test)] impl` block below every production item, so `src/agent/bin.rs` is whole again, and the shrink was **loud** — `every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified` reported the five functions as "invented". (2) The *prohibition* half is closed: PR7 gave the four whole-tree censuses `effects::production_code`, which removes each `#[cfg(test)]` **item** in place instead of truncating, so a mid-file test item no longer takes the rest of the file out of those, and `effects::tests::every_production_region_that_stops_early_stops_at_a_module` pins by name the ten files whose truncating region still stops at something that is not a module. (3) What is **not** closed: `externally_reachable_fns` still calls `production_region`, so those same ten files have a classification domain that ends at their first `#[cfg(test)]`, and six modules have an empty one. Moving it to `production_code` re-derives every classification entry by hand and is a change to the generated inventories, which PR7 does not own |
| PR5-C-LEGACY-APPEND-ERROR-CENSUS | `production_effect` promises "the legacy engine's handling of a returned append error is unchanged — it reports and stops". `events::log::tests::the_legacy_engine_reports_and_stops_on_a_returned_append_error` proves it as a **source census** (the error branch returns, emits nothing, and the engine has exactly one append call site), not as a behavioural test | PR7 implementer, or whichever lane plumbs an observer through `engine::Harness` | **Boundary stated rather than hidden**, on the test's own doc comment. The legacy engine opens its own `EventLog` through `EventLog::open` and takes no observer, so no test can make one of its appends fail without threading hooks through `engine::Harness` — a file PR5 lane C does not own and a change with reach far beyond this claim. What *is* checkable locally is the property the promise rests on: the error branch returns and appends nothing, so the handle poisoning this slice adds is unobservable to it. A behavioural version becomes cheap the moment the coordinator takes an `EventHooks`, which is what the append-error protocol needs anyway |
| PR5-R2-WIN-NON-SURROGATE-REPARSE | `PR5-WORKSPACE-006`. `validate_execution_root_chain`'s Windows arm checks the raw `FILE_ATTRIBUTE_REPARSE_POINT` attribute as well as `FileType::is_symlink()`, and only the raw check covers the **non-surrogate** tags — dedup, placeholder, LX symlink, appexec. Every fixture builds its reparse point with `cmd /C mklink /J`, and Rust's `is_symlink()` answers true for `IO_REPARSE_TAG_MOUNT_POINT` because a junction is a name-surrogate tag, so omitting the attribute check is behaviour-neutral for the only shape any fixture constructs. Measured twice: the mutation SURVIVED both the pre-repair and the post-repair guest runs, with both junction tests running and passing | PR6/PR7 implementer (the slice that next owns Windows containment) | **Carried because the distinguishing fixture cannot be built by the guest's test user.** Two of the four non-surrogate tags need a privilege it lacks (dedup and placeholder are filesystem-feature reparse points, not user-creatable), and the other two need WSL or an app-execution alias installed on the runner. A fixture that faked the attribute would be testing the fixture. What holds today is the surrogate half, on both platforms, by `a_junction_below_the_private_root_refuses_the_execution_root` and `a_managed_base_or_private_root_that_is_itself_a_link_refuses_before_any_effect`. The live passage is `slice_contract.expected_failures_refusals[0]` — "symlink/junction on the chain" — which names exactly the shape that *is* covered |
| PR5-R2-SNAPSHOT-INPUT-COMMIT-DEAD | `PR5-WORKSPACE-024` and `PR5-WORKSPACE-025`. `SnapshotInput::Commit` is constructed **nowhere** in the tree, so `create_integration_snapshot`'s "check out the proposal or head commit and create no object" arm never executes and turning it into an unconditional commit-tree synthesis changes nothing any test runs; and `add_snapshot` has two callers in two different tests with two different fixtures, so no fixture ever holds a gate snapshot and a reviewer snapshot alive **together** and `ExactSnapshotStore::create` caching one snapshot for every role and attempt is invisible. `SnapshotName::review` is constructed nowhere either | PR6/PR7 implementer (the slice that first requests two snapshots) | **Carried: the caller does not exist yet, and inventing one inside a repair round is inventing the orchestration.** Both entries need a *second live request* — an integration snapshot from a proposal commit, and a gate snapshot plus a reviewer snapshot alive at once across two attempts — which is the gate/review orchestration PR5's `scope` stops before. The live passages are `workspace_candidates.snapshots`: "integration snapshots check out the proposal or head commit and create no object" and "one snapshot for the gate set and one fresh snapshot per reviewer, never reused across roles or attempts". Recorded rather than dropped so the first slice that builds a reviewer snapshot knows it inherits an unmeasured claim |
| PR5-R2-IDUNREAD-BEFORE-THE-PARSE | `PR5-WORKSPACE-045`. `commit_tree` consults `IdUnread` before parsing the child's printed id, and the three `IdUnread` tests all run against a child that succeeds and prints a well-formed id — so moving the point *after* the parse changes nothing they can see | PR6/PR7 implementer | **Carried: not constructible through the funnel.** The distinguishing fixture is a commit-tree child that writes its object and then prints a **malformed** id, and the child is real `git commit-tree`, which always prints a valid one. Nothing stubs the child or injects its stdout, and adding a stdout seam to a production Git invocation to test the ordering of a hook is a larger change than the claim. The live passage is `effect_site_inventory.identity`'s R27 clause. What *is* held is that the point fires exactly once, before `After`, and that a kill there leaves a GC-owned object nothing adopts |
| PR5-R2-WORKTREE-LOCK-RETENTION | `PR5-RUNDIR-070`. The physical worktree lock is taken before the startup census and held for the whole run (`coordinator.rs:93` fresh, `resume.rs:108` on resume, both `let _worktree_lock = …` to end of scope). Dropping the guard immediately after the census is invisible: the two lease tests take a competing lease **first** and then check the run refuses, which exercises acquisition, not retention | PR6/PR7 implementer (the slice that can pause a run) | **Carried: the killing assertion needs a paused run and nothing in the suite pauses one.** "While run A is paused after census but before termination, a second write command for run B in the same physical worktree is refused; it succeeds only after run A releases its guard" needs a run held open across a second command — a coordinator seam PR5 does not own. `run_creation`'s "only then takes the physical worktree lock … holding it across the startup census and the whole run" is the live passage. Same shape as `PR4-R28-NEXT-COORDINATOR-UNWITNESSED`: a lifetime claim about a guard, unwitnessed because no fixture holds two coordinators |
| PR5-R2-LEGACY-ENGINE-APPEND-FAILURE | `PR5-EVENTS-054` and `PR5-EVENTS-055`. `Run::emit` swallowing an `EventLog::append` error into `self.warnings` and returning `Ok(())`, and deleting the partial-report construction from `drain_and_report`'s error branch, both survive the whole suite — because no test ever makes a legacy append fail **inside a live `Run`**. Every append-failure fixture operates on an `EventLog` directly | PR7 implementer, or whichever lane plumbs an observer through `engine::Harness` | **Carried, and it is the behavioural half of `PR5-C-LEGACY-APPEND-ERROR-CENSUS` above.** The engine opens its own `EventLog` through `EventLog::open` and takes no observer, and its run directory is created with a generated run id, so neither an injected failure nor a prepared path (a `/dev/full` symlink, which is what made `PR5-EVENTS-044` measurable in the Event lane) can be aimed at it from outside. The live passage is `production_effect` — "the legacy engine's handling of a returned append error is unchanged: it reports and stops" — and the source census that stands in for it is already filed. Both become cheap the moment the coordinator takes an `EventHooks` |
| PR5-R2-OBJECT-GROUP-TAKES-NO-SITE | `PR5-WORKSPACE-048`. All six Object-group APIs hard-code their `ObjectSite` internally — `candidate_stage`, `candidate_write_tree`, `snapshot_commit_tree`, `candidate_commit_tree`, `proposal_cherry_pick`, `repair_materialize` — while the Ref group takes `site: RefSite`. `manager` says every effect "goes through typed funnel APIs that take a typed site", so the asymmetry is real rather than an artefact of the measurement, and no compile fixture probes it | project owner | **Carried: widening six public signatures and every caller is a design change, not a repair-round edit.** Recorded as `NOT_PRESENT` by the re-measurement — there is no parameter to delete — but the absence is the finding. The tree already owns the mechanism that would prove it: `rundir.rs`'s `build_refusals()` compiles six fixtures against this crate's rlib and asserts rustc's own error **codes** (E0061, E0308, E0451/E0603/E0063, E0599, E0382) against a control that must compile. It has no Object-group case because there is nothing yet to refuse. If the owner reads `manager` as requiring the parameter, the repair is mechanical and the harness is waiting |
| PR7-WRAPPERS-EMPTY-DOMAIN | `effects::externally_reachable_fns` consults the truncating `production_region`, so for `engine/{attempt,coordinator,resume}.rs` and three siblings cut at a `#[cfg(test)] use` the **classification domain is empty**. Production `pub`-declared functions in classified modules are unclassified — 40 externally-reachable names and 20 `pub` fns across the six modules, and a **working bypass was demonstrated**: a `pub(super) fn` below the cut, called from a live topology module, passes clippy and the whole suite | project owner — **the post-v0.2 pass over PR3's layer** | **Carried: the repair is shared enforcement machinery whose blast radius is every classified module**, which is the shape that made PR5 round 7 a revert. `mechanism` (3)'s guarantee that a topology module cannot reach an effect through a legacy wrapper **does not hold** today, and that is a live-passage failure, not hardening — it is recorded here rather than repaired because the change is to the classifier every other module's enforcement depends on, and PR7 already spent two rounds on this file. Recorded **with its measurement and its bypass** so the next slice inherits evidence rather than a rumour. This is the **fourth and fifth** occurrence of `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN` (§4): PR7 repaired the two census instances by giving `production_code` a comment-and-string blanker, and this one is the same root cause in the function the blanker does not serve |
| PR7-NARROWED-SURFACE-19-UNCALLED | **Nineteen items in `engine::topology` have no caller at all — not in production, not in a test — and `pub` was what kept the compiler from saying so.** Narrowing `engine::topology` to `pub(crate)` (the frontier review of `75da796`, finding 1) made rustc report **328 items** dead in a lib build, which is what `production_effect = "none"` means and is silenced by `#![cfg_attr(not(test), allow(dead_code))]` in `engine/topology.rs` and `engine/assembly.rs`. **Nineteen survive that gate**, being dead in the *test* build too, and each now carries its own `#[allow(dead_code)]` naming this row: `attempt.rs` `key`; `candidate.rs` `base`; `emit.rs` `discharging`, `wrote_nothing`; `seams.rs` `harness` ×2; `startup.rs` `into_parts`, `lock` ×2, `locked`; `recover.rs` `reader` ×3, `owner` ×2, `bytes` ×2; `run.rs` `PartlyImplemented`, `owes`, `warnings`, `defer_round`. Counted at `610106b` | **PR8/PR12, or whichever slice next opens these modules** | **This is the slice's own most-recurrent class, found by the compiler rather than by a reviewer, and two entries were already known.** `pr7/STATE.md` records "`PartlyImplemented` has no inhabitants", and S5 round 6 recorded that a doc cited `LoopBranch::owes`, "which has zero call sites" — both stood because the `pub` surface made every item externally reachable in principle, so `dead_code` never fired. Seven review rounds and a withheld mutation catalogue did not find the other seventeen; one visibility change did. **Not deleted here, and the reason is that each is a judgement.** Several are typestate accessors that exist so a proven value can be taken apart (`bytes`, `owner`, `reader`, `into_parts`) and the tree argues for keeping some of them on their own docs; `PartlyImplemented` is a variant the ladder may yet construct. Deleting nineteen items across seven files at the end of a repair round, each needing its own reading, is the shape PR5's round 7 was reverted for. **What is enforced meanwhile**: the allows are per item, not per module, so a *new* uncalled item is still an error at `-D warnings`, and this row is the list a future reader diffs against |
| PR7-MACOS-PROCESS-GROUP-FLAKE | **`runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` fails intermittently on `test (macos-latest)` and nowhere else**, asserting *"review: the child did not lead its own process group, so the pre-exec containment step did not run for this role"* — `left: [false]`, `right: [true]`, at `src/runner/host.rs:5565` **as it stands at `75da796`**. **Measured over the last 20 CI runs on this branch, 13 of which completed a macOS job: 11 success, 2 failure** — and then a **third sample at the same sha**: `gh run rerun 32999498916 --failed` re-ran only the failed jobs of `75da796`'s run, without a push, and `test (macos-latest)` **passed**, taking the run to 9/9 success. So the tally is 12 success / 2 failure over 14 completed macOS jobs, and one head has now produced both outcomes with the tree byte-identical, which is what makes it a flake rather than a defect in the head. Both failures are this test, at `cca1276` (14:00) and `75da796` (18:24) on 2026-08-26; every other completed macOS job on the branch back to `f6ed9f1` passed. The Linux, Windows and both other MSRV legs pass in the same runs, and the guest and this box never reproduce it. **Not caused by the diff it appeared on**: `d17bcf2..75da796` is `reviews/FINDINGS.md`, a new review record, and two doc-comment stampings in `run.rs` and `run/tests.rs` — nothing that can reach a process group | **project owner / whichever slice next opens `src/runner/host.rs`** | **Recorded with its rate rather than described, and not chased in this slice.** The assertion is that the spawned child leads its own process group after the pre-exec step; a macOS runner under load losing that for one role out of a grid, twice in thirteen, is either a real race in `pre_exec` ordering or a runner-side artifact, and **this session cannot tell those apart** — it has no macOS host, and the two observations are CI logs. **What a repair would need first** is a way to reproduce: a macOS runner the slice can drive, or a CI job that runs this one test in a loop and reports a rate. Adding either is out of scope here and neither is a `src/` change. §12 is the precedent for carrying a flake with numbers; `PR5-MACOS-CLIPPY-NEVER-RUN` in §2 is the standing observation that this project has no macOS host at all, which is the same gap one gate over |
| PR7-WIN-READ-RACING-BOUND-TOO-SHORT | **A production retry bound in `container.rs::read_racing` is too short on Windows, and the concurrent-census race tests are how it shows.** *Retitled 2026-08-26 on the frontier review's judgement: this row said "flake" and the reviewer's answer is that it "describes a real production retry-bound defect in `read_racing`, not merely a flaky test — calling it a flake understates the category". That is right, and the distinction is not cosmetic: a flake is triaged by re-running, a bounded retry that is too short for its platform is triaged by changing the bound. The tests are the symptom; the bound is the defect.* `container.rs`'s `read_racing` returns `Ok(None)` for `NotFound` — the Unix answer when a competing reclaimer removed the record — and for **any other** IO error spins `RACING_ACCESS_ATTEMPTS = 64` times on `std::thread::yield_now()` before letting the error escape. Its own doc reasons *"clears when the winner's own call returns, so this is a handoff rather than a wait, and `yield_now` is what it costs"*, which holds where the window is one syscall. On Windows a competing open returns **`PermissionDenied` (os error 5)** for as long as the winner holds the handle — its whole open/read/close cycle — and under full-suite load on a 16-vCPU guest, 64 yields can fit inside that. **Measured at `d17bcf2`, four full-suite guest runs: two green, two red, and two *different* tests** — so it is a class, not one flaky test. (1) `concurrent_reclaimers_converge`: `the resuming incarnation refused instead of converging: Err(Io { path: …\\upstroke-census-converge-9-2440-ThreadId(4407)\\containers\\upstroke-…intent, source: Os { code: 5, kind: PermissionDenied } })`. (2) `a_fresh_and_a_resuming_census_race_one_container_and_converge`, panicking at the `a racer refused instead of converging` assertion in `a_fresh_and_a_resuming_census_race_one_container_and_converge` (`census/tests.rs:4618` **at `4247255`** — named as well as cited, because a line number is a claim about a version). **The errno for (2) was not captured at the time** — `win-iter.sh` writes every run to one `/tmp/win-iter.log` and the next run overwrote it — so (2) stood as a presumption of the same cause rather than a measurement of it. **It is now measured.** At `049342c` (2026-08-27), the fourth full-suite run of that head reproduced (2) exactly — `[round 6] a racer refused instead of converging: failed to read …\\…intent: Access is denied. (os error 5)` at `census/tests.rs:4618` — with the log preserved this time, at `~/tactus-artifacts/pr7/win-failure-049342c-run4-iter.log`. **`PermissionDenied`, the same errno as (1) and (3): the presumption is discharged and both tests are one cause.** The capture is owed to `win-full.sh` copying the iteration log per run, a one-line fix made after this row's own evidence loss happened a second time, an hour earlier in the same session. In isolation `concurrent_reclaimers_converge` passes **3 of 3**, so it is load-dependent. **Third occurrence, at `8a163fd` (2026-08-26): the same test, the same `"refused instead of converging"` assertion at `census/tests.rs:1623`, on the first full-suite guest run; it passed in isolation immediately after and the full-suite re-run was green (1669 + 10, 0 failed).** **Fourth occurrence, at `049342c` (2026-08-27): the same test, the same assertion at `census/tests.rs:1623`, on the first of three full-suite guest runs — `the resuming incarnation refused instead of converging: … Os { code: 5, kind: PermissionDenied, message: "Access is denied." }`, quoted from a read of `/tmp/win-iter.log` during the run and **not preserved on disk**, because the two runs after it overwrote the shared log. That is the same evidence loss this row already records for occurrence (2), repeated; `win-full.sh` now copies the iteration log per run, which does not help this one. The quote is at `~/tactus-artifacts/pr7/win-failure-049342c.md` with its provenance stated. The two runs after it were green (1687 + 0 across three binaries), and a fourth run reproduced the row's *other* test.** At `049342c` the rate was **2 red of 4**, one of each test. Cumulative guest rate: **5 red of 10 full-suite runs**, across three heads and two distinct tests — every failure the same assertion, and now every *captured* errno the same `PermissionDenied`. The rate is carried here rather than smoothed away — it is a bounded retry that is too short for its platform, and it is triaged by changing the bound | **PR6's owner, or whichever slice next opens the Container funnel** — `read_racing` arrived in `919a728` (PR6 lane C), so this is `pre_existing` for PR7 | **Carried with its repair fork stated, rather than repaired here, and the reason is precedent.** This is production code on a first-class target and the bound is deliberate, so the fork is the owner's to pick, not mine to guess at the end of a repair round.  The retry policy is documented production behaviour in a funnel, its bound is deliberate (*"Bounded rather than timed, for the reason `TERMINATION_OBSERVATIONS` is: a wait with no bound turns 'this path cannot be removed' into 'this write command never returns'"*), and a late change to shared concurrency infrastructure is the exact shape PR5's round 7 was reverted for. **What a repair would have to decide**, so the next owner does not re-derive it: whether `PermissionDenied` joins `NotFound` as an immediate *already-gone* answer (it is not the same claim — a permission error can be a real one), or whether the spin becomes a short bounded backoff, which keeps the bound the doc argues for while making each attempt cost more than a yield. **The measurement to demand of any repair** is the one above run to a rate: this is roughly 2 in 4 full-suite runs and 0 in 3 isolated ones, and a repair that is only measured in isolation has not been measured. §12 is the precedent for carrying a flake with its numbers rather than a description |
| PR7-SCRATCH-FIXTURE-LEAK | `src/rundir.rs`'s `scratch` calls `remove_dir_all` at **creation**, keyed by `{tag}-{pid}` — §16 records it in full. PR7 is the slice that pays for it: the suite grew from 1385 tests to **1644**, and the leak scales with the suite | project owner / whichever slice owns shared test infrastructure | **Carried, unchanged in disposition from §16 and now with a second measurement.** The build box reached **19% of 58.5M inodes**; sweeping leaked fixture directories returned it to **12%** — on the order of **4.1 million inodes** that were leaked test fixtures, roughly a third of everything in use. `df -h` read 31% throughout. Held out of this slice for the reason §16 gives — the repair is a judgement call across 60+ call sites in shared test infrastructure, the PR5-round-7 shape — and mitigated out of tree by a sweeper with a 30-minute age floor so it cannot race a running suite. **PR7 raises the urgency rather than the difficulty**: parallel execution multiplies the fixture count per wall-clock hour. **2026-08-26: on Windows this stopped being a disk problem and became a correctness one.** The guest suite at `5e309a0` returned **16 failures** — fourteen in `engine::topology::emit::tests` and two in `settle::tests` — every one of them `assert!(bytes.is_empty(), "a fresh run has no prefix")` at `emit/tests.rs:324`. The same guest, minutes later, was **green at `040a100`** (1651 + 10, 0 failed), so it is not a regression in the diff. `emit/tests.rs`'s `run_paths` keys its scratch on `{tag}-{pid}-{n}` and **Windows recycles pids**: `%TEMP%` held **11,395** leaked `upstroke-*` directories, and grouping the `upstroke-emit-*` ones by their pid component gave six previous processes with 25-34 directories each. A run that draws a recycled pid finds its "fresh" fixture already populated and fails on the emptiness assertion. Sweeping `%TEMP%` to zero and re-running the same head is the control. **What this changes about the row**: the Linux symptom is inode exhaustion and is mitigated out of tree by a sweeper; the Windows symptom is a **fresh-run fixture that is not fresh**, it is indistinguishable from a real defect in the reviewed head, and no sweeper prevents it — the fix is that a fixture root includes something a recycled pid cannot supply, or removes its own directory at creation the way `rundir::scratch` does. Recorded here rather than repaired for the reason the row already gives: 60+ call sites in shared test infrastructure |
| PR7-P3A-CREATOR-RETAINS | A creator that errors at exactly P3a has no owner record, so `prove_private_half_ownership` mints no `PrivateHalfProof`; the creator therefore removes **neither** half, and the startup census retains and reports both. The packet's deletion boundary is satisfied, but an operator sees two retained directories where the failing step created one usable pair | PR7/PR12 implementer | **Accepted risk, and the alternative is worse.** ST-19 tables this shape as content-free by ordering — nothing has been written into either half at P3a — and `creator_error_at_p3a_retains_both_halves_and_reports_them` covers both windows, so the behaviour is asserted rather than incidental. Removing the retention needs a second constructor for `PrivateHalfProof`, and that type's **single-constructor property is compile-fail-tested**: the proof exists precisely so that no path can delete a private half without having proved it owns it. Trading a compile-time guarantee for a tidier failure directory is the wrong direction, and the retained pair is reported, not silent |
| PR7-CREATEINTEGRATION-ORDER-BACKWARDS | `src/topology/effects.rs:1696` says `RefSite::CreateIntegration => Adjacent::Before(DurableEvent::RunStarted)`, and `Adjacent::Before` is documented three lines above as *"the effect is designed to be durable **before** the append is"*. `decisions.pr_sequence[8].slice_contract.side_effect_vs_event_ordering` says **"run_started before integration ref"**, and P8 creates the ref after P6 appends. The registry states this site's order axis backwards | project owner — **the post-v0.2 pass over PR3's layer** | **Carried by owner ruling, 2026-08-24: recorded clearly and revisited once v0.2 is complete.** Not cosmetic — `Adjacent` "decides `EffectSiteId::observable_orders`, which is what the registry's order axis ranges over", so for a `fault_row: t_runstart` site the fault-injection registry demands evidence for `effect_before_event`, an ordering the production code never produces, and never demands `event_before_effect`, the one it does. **Why nothing caught it:** the only test over the value is `the_observable_orders_are_the_ones_the_adjacency_admits`, which checks that `observable_orders` agrees with `adjacent` — a function used as its own oracle, §4's class, so it is green for either value. Measured: flipping the token fails exactly two tests, `effects::tests::the_checked_in_effect_sites_json_is_what_the_enums_generate` and `topology::effects::tests::every_site_carries_the_row_fault_row_scope_and_adjacency_the_design_gives_it`, both transcriptions of the same claim. The edit is one token; the consequence is that G2 evidence for this site is owed against the other order. `src/topology/effects.rs` is the file `ff0490a` names by name |
| PR7-FOLD-ACCESSORS-IN-PR3-LAYER | `src/topology/fold.rs` is **+1196 / −13 at `2378c83`** (`git diff <merge-base>...HEAD --numstat -- src/topology/fold.rs`). **Twice restated, and the second time by a reviewer rather than by me.** It read +628/−0 and "nine accessors" until 2026-08-24, then +777/−11; the frontier review of `75da796` measured +1196/−13 and observed that a disclosure row whose own number is stale is the disclosure failing — twice over, since the correction that fixed the first staleness introduced the second. **The number now carries the sha it was taken at**, per §22's rule, because that is the only form of it that does not decay: this file grows whenever the slice adds a fold test, and a figure with no sha reads as current forever. Disclosed here rather than left for a reviewer to find, because it is PR3's file and the slice is large enough that a footprint this size can stop being visible to the person making it | project owner — **adjudicated 2026-08-24, see §3**; the deferred work is the G2 PR3-layer pass | **Accepted as a disclosed deviation through `3362f65`.** Measured split at head: **561 lines of tests**, **152 comment and blank lines** in the production region, and **64 lines of production code**. That code is **eleven `pub fn` readers** — `ready`, `ready_retry`, `pipeline_held`, `pipeline_reservable`, `structurally_admissible`, `integration_admissible`, `run_is_ending`, `backoff_pending`, `predicted_region`, `frozen_rung_binding`, `questions_open` — nine of them one-line delegations to an existing private `RunState` predicate with a poison guard, plus **one line of changed behaviour**: `&& self.pipeline_reservable()` in `integration_admissible`, which is `PR7-INTEGRATION-NO-ENTITLEMENT`'s repair. The **11 deletions** are not behaviour either: four are one re-wrapped `use` block, and seven are the body of the *test* helper `frozen_binding`, which repeated the reader's composition and now delegates to it — so the reader sits under the whole existing attempt corpus. No variant added, no type widened, nothing else deleted, which is not the shape `ff0490a` forbade. **`frozen_rung_binding` is deliberately half of the fold's rule**: it returns the frozen rung's binding and not the human-override arm, because no override is constructible while the answer-ingest branch is unimplemented and because `matches_override` checks only agent, model and effort — leaving `tier` and `pinned` for a caller to choose unchallenged. Collapsing it to a full delegation is **W2 of the pass**. It is also the **last fold reader outside that pass**: the standing rule this slice proposed was rejected |
| TASK-DISPATCHED-REGION-UNVALIDATED | **The fold accepts any predicted region a `task_dispatched` carries.** `check_dispatched` matches on `(&dispatched.lease, entry.lineage)` — the lease's **shape** only, Predicted-versus-InheritedLineage and their pairing with the entry — and never compares the `paths` inside `LeaseGrant::Predicted` against `predicted_region(entry)`. `apply_dispatched` then grants **whatever region the event carried**: `LeaseGrant::Predicted { paths } => (GenerationLease::Own, Some(paths.clone()))`. So the fold admits a dispatch on one region and the lease table holds another, and the lease table's is the one every later overlap check consults. **The asymmetry is the finding**: one event over, `check_attempt_started` refuses a divergent *binding* with `FoldError::BindingMismatch` — a refusal present since PR3 — so the same class of disagreement is caught for the binding and not for the region | project owner — **the G2 PR3-layer pass, W1** | **Recorded, not repaired: the repair is a fold-side refusal, and `src/topology/**` is closed to this slice by the 2026-08-24 adjudication (§3).** Measured on this box at `3c09f6e`, 2026-08-24, both halves run: with the divergent derivation restored **and** the regression assertion removed, the full suite is **1661 passed / 0 failed** — every gate, census and fold test is indifferent; with the assertion restored, **exactly one test fails**, `the_driver_takes_over_from_the_recovery_order_and_steps`. **That is the whole of the protection, and it is a convention rather than a guarantee.** `84a3978` made the driver read `TopologyFold::predicted_region` instead of deriving its own, which fixes the instance; the class stays open, because nothing stops the next caller — or a later slice's second writer — from constructing a `task_dispatched` the fold will accept and the lease table will honour. The class fix is `check_dispatched` comparing the carried region against the one it admitted on, exactly as `check_attempt_started` already does for the binding. Live at the first width above `max_parallel = 1`, where two tasks holding non-overlapping-by-construction regions edit the same files; invisible below it |
| PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE | **A one-shot environmental measurement scheduling a race.** `sampled_git_add_and_write_tree_child_kills_every_residue_classified_and_recovered` measures one `git` duration in a probe worktree, then aims all sixteen kills as fractions of it — sleep `budget * (run+1)/9`, kill. The probe is the **first** invocation in a fresh worktree, so it pays for a cold filesystem cache and, on Windows, an antivirus scan of files it has just seen created. Its number is therefore inflated relative to the runs it schedules, every kill lands after its child has exited, and the harness samples the residue its commands left when they **finished**. No seed, and the whole variance is one measured duration | **fixed in PR7** | **Fixed in slice, not carried, and the reason is the answer to "does it block merge-readiness": an intermittently red required leg is not a gate — it trains re-running reds, which is how a real regression hides.** Two occurrences, `test (windows-latest)` at `b07b8cc` and its re-run, on a commit that changed **one line of a Markdown file**; `3362f65` was green on the same leg. The assertion was right both times — it refuses to pass vacuously when nothing died — so the defect was the schedule, never the code under test. **Repair, O(1) and not a per-run re-measure** (which would double git invocations in a ~700s leg to fix a defect living in the first one): discard a warm-up probe and take the **median of the next three**, keeping the fractional schedule; and make the test assert its premise — at least one kill landed mid-run — recalibrating from the durations the runs **actually took** and retrying **once**, bounded, before failing hard. `KillableGitChild::exited` was added for that, because wall time to the reap includes the scheduled sleep and would report an over-long schedule back as the duration it should have been: **measured, the first version of this fix inherited exactly the error it existed to correct.** Guard: the vacuity refusal is unchanged and now states what it has already ruled out. Mutations — an inflated probe (×50) is rescued by the retry; `KillableGitChild::kill` made a no-op still fires the vacuity refusal, so self-healing cannot mask a kill that does not land. **Evidence on the platform that failed**: 10 consecutive guest runs, 10 pass / 0 fail, 21.9–22.5s with no outlier. A Linux-only green would have closed this falsely — it did: the first repair passed on Linux and failed on the guest, killing at 40.3ms against a 48.5ms rung because the poll broke out early and killed there || PR7-CANDIDATE-TREE-UNVERIFIED | **A resume has no recorded tree to check the candidate against.** DESIGN.md §15 has `candidate_prepared` record the complete attempt/base/commit/tree identity "so resume adopts only the judged object", and the event does carry `tree_sha` — but `TaskFold`'s `PreparedCandidate` keeps `candidate`, `base_sha` and `paths`, so by the time `recovery_for` classifies an unfinished promotion the tree is gone. `verify_object` therefore checks what survives replay: that the object is a **commit** and that its parent is the generation's recorded base. A commit with the right parent and a **different tree** passes | project owner — **the G2 PR3-layer pass**, alongside `TASK-DISPATCHED-REGION-UNVALIDATED` | **Recorded rather than repaired, because closing it is a fold field and `src/topology/**` is closed to this slice by the 2026-08-24 adjudication (§3).** This row exists because the repair it accompanies stopped short of the claim rather than overstating it: `SETTLE-CANDIDATE-OBJECT-NOT-VERIFIED` found `verify_object` asking only `object_exists`, which is `cat-file -e <sha>^{}` and so answers **true for a tree or a blob**, and the repair added the parent comparison and its two witnesses (`promotion_refuses_an_object_that_is_not_the_judged_candidate`, both mutation halves killed). What the parent check cannot see is a same-parent, different-tree object, and no reachable path produces one today — the only writer of a candidate commit is `write_candidate_commit`, from the judged tree. It becomes live the moment a second writer exists, which is the same width `TASK-DISPATCHED-REGION-UNVALIDATED` names. The repair is one field on `PreparedCandidate` and one comparison, and it is cheap **in the pass** and a frozen-file change here. **FIXED 2026-08-26.** The frontier re-review of `c2c0294` raised it as finding B with the argument that carried `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4` — a ledger disposition does not amend the sole living authority — and the owner ruled: **Class B, per-instance approval granted**, quoted with its measured split in §3. `PreparedCandidate` retains `tree_sha` (**+20/−0** on the frozen file: 18 doc lines, 2 of code), `verify_object` compares the commit's tree against it, and `promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged` builds a real same-parent different-tree commit, asserts both pre-existing checks pass on it so the refusal cannot be an earlier one, and asserts the refusal with no queue position taken and no candidates ref created. Nothing serde-visible moves. The prediction in this row — that it "becomes live the moment a second writer exists" — is no longer load-bearing: the check does not depend on who wrote the commit |
| PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4 | **§11.4's accumulated brief cannot survive a resume, because schema 4's wire has nowhere to put it.** The legacy schema-3 events carry it outright — `LadderRetry` and `LadderEscalated` each hold `summary` **and** `detail`, and `Progress::feedback` is rebuilt by replaying them. Schema 4 records `attempt_finished{AttemptRecord, SettlementTransition}`, and `FailureRecord` is `{kind, origin, reason}` with **no detail**, while no `SettlementTransition` variant has a feedback field at all. So the gate-log tail and the reviewer's `required_changes` — the two things §11.4 exists to send back — are process-local in schema 4 and in no other schema. A run that crashes mid-ladder resumes and tells its next worker nothing about the attempts before the crash | project owner — **the G2 pass**, with `TASK-DISPATCHED-REGION-UNVALIDATED` and `PR7-CANDIDATE-TREE-UNVERIFIED` | **Recorded, not repaired: the repair is a wire field and `src/topology/**` is closed to this slice by the 2026-08-24 adjudication.** This row is the half that the in-process repair could not reach. S5 round 2 found (`contract`, `seams`, `attempt`, independently) that the driver accumulated §11.4's brief inside `Retained`, which `settle::retry` produces only for a resumable same-rung retry **with** a session — so every escalation and every sessionless retry, meaning every Copilot attempt (`DESIGN.md:452`), dispatched with an empty brief even *within one process*. That half is fixed: the brief is per **task**, every judged failure adds to it, and both dispatch arms read it. What is left is the durability, and it is a real behaviour difference from the engine schema 4 replaces — worth stating plainly rather than as a footnote, because `invariants_preserved[1]` is the standing rule and this is the one place the new engine is **less** capable than the old one. **The shape of the fix is already decided by precedent, which is why it is cheap in the pass and a frozen change here**: `attempt_finished` already carries the record beside the settlement, so a `detail: Option<String>` on `FailureRecord` is a pure addition of exactly the `#[serde(default)]` kind `AttemptRecord::pool` and `AttemptRecord::usage` already are — a log written before it folds to the state it always did, and `SCHEMA_VERSION` does not move. **FIXED 2026-08-26, measured at `bd3b9cd`.** The frontier review of `75da796` held that a ledger disposition cannot waive a live passage and the owner agreed: fork 1 was authorised as **Class C** with its ceremony (`decisions/2026-08-26-durable-retry-feedback.md`, and the per-instance approval in §3). `FailureRecord` carries `detail`, `classify::attempt_record` writes it, and the driver's brief is now derived from the log by `Brief::replay` rather than accumulated by the live path alone — one step, two callers, applied as it will be read back (§15's "one fold, not two"). Four witnesses, each with a mutation that kills it and leaves the others green; the table is in §22b |
| PR7-STEP-D-LINEAGE-ARM-UNWITNESSED | **Recovery step (d) handles `LeaseDisposition::LineageHeld` and no test can reach that arm.** Catalogue entry `PR7-PIPELINE-008` adds `if lease == LineageHeld { continue; }` to `settle_interrupted`'s loop and the whole suite stays green. The loop is **already correct** — this is a coverage gap, not a defect | PR8 implementer (the slice that gives the merge queue a repair to spawn) | **Carried with a condition sharper than the one the catalogue implies, and measured.** `LineageHeld` is produced only by `GenerationLease::InheritedLineage`, which only a **repair task** holds, and a repair task exists only after a `task_spawned` carrying `Origin::MergeRepair`. Measured over `effects::production_code`: the only `TaskSpawned {` constructions in the tree are the frozen layer's own definitions (`topology/events.rs`, `topology/fold.rs`) and `engine/topology/scaffold.rs`, which is `#[cfg(test)]`. **No production path in this slice spawns a repair**, so the arm is unreachable by construction rather than by width — PR8's merge queue is what makes it live, not PR11's parallelism. **Why it is carried rather than witnessed**: the fixture would have to seed a `task_spawned` whose `FrozenSpawn.entry` is a registry entry derived outside the fold — the scaffold's `spawn_repair` reads the live registry to build one, and `Damage::extra` is assembled before any fold exists. That is a different construction from the sibling gap `PR7-PIPELINE-010`, which **was** repaired in-slice this round (`Damage::two_tasks`, `steps_d_and_e_reach_every_generation_not_the_first`) because it was the loop-versus-first shape a second task settles |
| R3-SEAMS-006-ATT003-REPAIRED-POSTHOC | **Refuted as described, with a residual question that is not the same claim.** Sol's independent `seams` read, round 3: "a first reviewer whose Runner returns an error -> `run_review` reports `invocations: 0` -> the post-hoc loop performs no registration or cancellation", concluding R4 is not held on the error path. **Inspected `src/review.rs:786-797`, the `runner.run(&request)` match arm inside `run_review`'s invocation loop** — the item, the file and the lines, per §4's refutation rule. That arm does **not** return `Err`: it returns `Ok(unavailable_after_error("review process failed", error, cost, invocation - 1, last_path))`. So `judge` receives an outcome, the reconciliation **does** run, and it registers `invocation - 1` = 0 for a first pass. The described mechanism — an `Err` bypassing the loop — does not occur | project owner, if the residual is worth a row of its own | **The residual, stated separately because it is a different claim and I nearly repaired the wrong one.** `unavailable_after_error`'s `invocation - 1` is "how many invocations *completed*", and a Runner error means none did — but the Runner may have **spawned** a process before failing. Whether a spawned-and-unreportable process belongs in the ledger is a real question about `permits.protocol`'s "registered exactly once"; it is not the question Sol asked, and the answer is not obviously yes, since registering one that never started is the opposite failure and is the reason the reconciliation is post-hoc at all. **What was almost shipped**: an error arm in `judge` registering and cancelling the pass, written against Sol's description before its reachability was checked. It compiled, the suite stayed green, and a witness built for it **failed** — `judge` returned `Ok` — which is what surfaced the refutation. Reverted rather than kept: an arm whose reachability is unestablished is the same defect as a function with no production caller, filed one commit earlier as this slice's most recurrent class |
| PR7-G2-W1-SUCCESS-IGNORES-THE-FROZEN-PLAN | **A candidate is admitted whose configured primary reviewer never ran.** `AttemptRecord::is_successful` asks `failure.is_none()` and `all()` over the passes *present on the record*, and never compares them with the task's frozen `FrozenReviews`. A `candidate_prepared` carrying a lone passed `second-opinion` — or an empty list — is therefore successful at the door: the fold charges the rung, enters `Promoting`, and permits `task_candidate_created` for a tree no required reviewer approved. Found by the `cfa1be8` review, round 6, as its first P1 | project owner — **the G2 PR3-layer pass, W1** | **Recorded, not repaired.** The repair is a fold-side check taking `(record, frozen)`, because the predicate needs the plan and `AttemptRecord` does not carry it — a fourth Class B change to a door already holding three per-instance approvals, proposed at the end of a sixth repair round. The standing stop condition forbids exactly that. Round 6 did fix the *outcome* half — `Failed` and `Unavailable` are refused, with witnesses that kill their mutations — so what is open is the *presence* half. §22e |
| PR7-G2-W1-RETAINED-ARM-UNGUARDED | **The settlement door's `Retained` arm asks neither question the `Closed` arm asks.** It checks the epoch and stops, so a current-epoch retained settlement may carry a record with `failure: None`, every review passing, and an attempt number that is not the envelope's. `AttemptRecord::is_failed` has **no caller anywhere in the tree**. `scaffold.rs:1293` already emits a retained record with `failure: None` and no reviews, so the missing check is demonstrated in-tree. Every one of round 6's four new refusal witnesses constructs `Closed`, which is why the arm is undriven. Found by the `cfa1be8` review, round 6 | project owner — **the G2 PR3-layer pass, W1** | **Recorded, not repaired**, and its first decision is a design question rather than a mechanical fix: a retained attempt is *unsettled*, so requiring its record to say "failed" may be the wrong assertion — the alternative is that a retained record makes no success claim at all, and `is_failed` is deleted rather than given a caller. That choice also disposes of the unused-public-API half of round 6's finding 4. §22e |
| PR7-G2-W1-PROBE-PAIR-NOT-OBLIGED | **Handing a probe the ledger and slots as arguments does not oblige it to use them.** An implementation may run its processes through a pair of its own and let creation's closing balance inspect the supplied one, which is the same disagreement `PR7-RR4-BALANCE-CHECKED-AGAINST-THE-WRONG-LOCKS` described one shape earlier. `ContainerProbes` already ignores both arguments while running a real shell process. Deleting `ledger()`/`slots()` from the trait was correct and is kept; what is false is the **signature-level guarantee**, which `create.rs`'s own doc retracts and then restates two paragraphs later — the fourth assertion of a claim refuted three times | project owner — **the G2 PR3-layer pass, W1** | **Recorded, not repaired, and the claim is retracted without replacement.** The structural repair is for the caller to build the registration wrapper from its own pair and hand the probe that, so there is nothing else to register through — a change to the pre-flight seam, which is not a thing to attempt at the end of round 6. **What is true today, without a guarantee attached**: `RunnerProbes` is production's only implementation, it uses the pair it is handed, the balance reads that same pair, and the three implementations that ignore the arguments are test doubles. §22e |
| PR7-R4-CLAIMS-UNVERIFIED | **Eight claims written into commit messages and doc comments of the round-3 repairs are false, and each is one `grep` from disproof.** Round 4 — five lenses over the six commits `0cd2001..040a100`, scoped to that diff alone — returned **27 findings, every one inside it**, on a head green on Linux (1702/0), the Windows guest (1651+10) and CI (10/10). The eight: (1) `an_ending_run_reaches_closure` cited as an existing test whose scoping gap justified a new witness — **the test does not exist**, the name occurs once, in that doc comment; (2) the pool census described as asserting "what actually failed" — it inspects `attempt.rs`/`settle.rs` while the defect was `pool: None` in `run.rs`, and restoring the pre-repair state leaves the whole suite green; (3) "no driver fixture can reach the arm", given as the structural reason a source census was necessary — `the_retaining_incarnation_retries_in_place` reaches it; (4) `AttemptPlans::pool_for` said to give the pool rule "one production implementation" — `capacity::pool_for` has three call sites in `assembly.rs`; (5) the ending witness said to cover "**every** arm" — three of six; (6) the pre-clean repair presented as complete — one of its two callers; (7) the packet-clause census said to have "would have caught… `Spend::replay`" — not among its eleven entries; (8) a fixture said to make two behaviours "not both pass" — its implementer and reviewer share `AGENT`, so both pass, and the mutation measured as killed died for the wrong reason | project owner — **the claims protocol a fresh session carries** | **Recorded as a ledger correction, not repaired by history surgery.** The commit messages are pushed history and the owner's instruction is that they are corrected here, citing the table, exactly as `80a141b`'s false refutation was. The full table with per-claim citations is `~/tactus-artifacts/pr7/s5/r4/FALSIFICATION-TABLE.md`; the raw lens outputs are beside it. **Three confirmed code defects accompany the claims and are open**: `expected_refs`'s census entry is satisfied by a substring collision (all four `expected_refs(` matches in `workspace_manager.rs` are `refuse_unexpected_refs(`; genuine calls zero); the pre-clean fix is half-applied, leaving the stranger-killing path live at `census/tests.rs:3645`; and `an_ending_run_offers_no_work_from_any_arm` covers three of six arms with `Integrate` in the gap. **What is not in doubt**: rounds 1-3 closed real defects — the E6 promotion stall, a resumed run that forgot its spend, and a path traversal from plan-authored input where the legacy engine sanitised and the extraction did not — and those repairs are behaviourally sound. Round 4 challenged the *claims about* several witnesses, not the fixes beneath them. **The pattern, stated once**: prose asserted at the moment of writing became the evidence for the work it described, and nothing earlier in the chain checks a claim made in a commit message — which is the artifact a reviewer trusts most. **The table itself is now in this file, verbatim, as §19**, with each of the eight disproofs re-run at `cca1276` and its command recorded beside its result — including one place the table over-reached, corrected there under the same rule |


## 3. Challenges to settled entries

A reviewer appends here; the implementer adjudicates. New evidence only — a failure sequence the
disposition did not address, and a mutation the current suite would not catch.

*(See §2 for the mechanism working in the other direction: **PR3's** second confirmation was asked a
direct question about scope and answered it as a disposition, which is now settled in §1. This is a
claim about the PR3 round, not about PR4's second confirmation, whose two findings —
`PR4-CONF-003` and `PR4-CONF-004` — were both accepted and repaired in round 5.)*

### 2026-08-28 — `BRIDGE-FROZEN-LINT-ATTRIBUTE`, per-instance Class B approval

**The owner's ruling, quoted:**

> **RULED — Class B per-instance approval, granted by this message:** the
> `#[expect(clippy::expect_used)]` attribute on `src/topology/effects.rs` stands. That
> file is one of the two the 2026-08-20 ruling froze BY NAME, so this carries full
> ceremony.

Raised by the `lints` lens of the five-lens review of `bdd64f5`
(`~/tactus-artifacts/pr34/review-lints.md`, finding 1), which was correct on the point
the bridge got wrong: the touch is not Class A's additive reader and this pull request is
not the chartered pass, so the class is arguable — and an arguable class is **Class B
until ruled otherwise**, which requires per-instance approval *before* landing. The
bridge's own reasoning, that the freeze binds feature slices and a master merge is not
one, is not an exemption the 2026-08-20 ruling grants. Deferring the question to the G2
pass would have been too late, because this lands first.

**Why the file matters more than "somewhere under `src/topology/`".** The 2026-08-20
ruling froze **two named things**, and `src/topology/effects.rs` is one of them. This is
not the directory-wide reading; it is the explicit one.

**What changed, measured at the commit that carries this text.**

| file | +/− | what |
|---|---|---|
| `src/topology/effects.rs` | **+4/−0** | one `#[expect(clippy::expect_used, reason = …)]` attribute on the statement `let hook = phase.hook_phase()`, carrying that call's existing message. No statement, signature, type or behaviour changes. |

**The annotation is honest, and that was audited rather than asserted.** The `lints`
lens verified the reason is true: `required` is constructed only from `Before`, `After`
and `Point`; all three map to `Some(HookPhase)`; `Residue` and `NoExecution` cannot enter
the loop, and the mapping has a focused test. The `expect` is a tripwire for a future
programmer defect, not a currently reachable panic. The lens found no reachable failure
suppressed by it.

**Why the alternatives lost.** Refactoring the `expect` away is a larger edit to the same
frozen file, and a behaviour-adjacent one. A module-level `allow` would have to live in
`effects/allowlist.toml`, weakening the mechanism the whole governed-effect system rests
on. Leaving it unannotated fails `clippy -D warnings` on the integration branch, because
master's `[lints.clippy]` denies `expect_used` — so the branch could not pass its own
gate.

### 2026-08-28 — `PR5-MACOS-CLIPPY-NEVER-RUN` fired, and is carried

Its owner clause names the slice that next opens `ci.yml`. The master merge carries a
`ci.yml` change, so the trigger has fired. **It is recorded here and NOT repaired in this
pull request**: adding a macOS Clippy job to a merge-only bridge is scope creep, and the
bridge's whole argument is that it changes nothing but what the merge forced.

The `lints` lens supplied the concrete escape this row previously described only in the
abstract. No Clippy job compiles the macOS-only production regions of
`src/agent/proc.rs` — `last_errno`, `group_has_non_zombie_members`,
`process_is_stopped`, `create_cloexec_pipe`, `clear_nonblocking`, and the non-Linux
`groups_are_quiescent`. Ubuntu Clippy configures them out, the new Windows Clippy job
configures them out, and macOS runs tests and MSRV but not Clippy. So:

> add an `.expect()` inside macOS `create_cloexec_pipe`; if no test executes that branch,
> every required check passes and a production panic the standard prohibits ships.

The lens checked and found **no currently denied call** in those regions, so the hole is
open but unoccupied. The repository already records it at `src/effects/tests.rs:1352`.
Carried, with an owner, rather than left silent.

### 2026-08-27 — `candidate_prepared` is the sole successful settlement, per-instance Class B approval

**The owner's ruling, quoted:**

> **Finding 1 ruled: CONFORM — no supersession.** `candidate_prepared` is the sole
> successful settlement for a candidate-producing attempt, as the 2026-08-12 record and
> DESIGN state; the driver stops emitting `attempt_finished` for those attempts. The
> slice's own doc that blessed dual emission is corrected — not the record. Class B on the
> frozen fold, per-instance approval granted with ceremony: settlement counting moves to
> the sole event, and every settlement-counting witness is re-derived against the new
> invariant — one settlement per candidate-producing attempt, crash prefixes per DESIGN's
> enumerated resume cases — never patched to pass.

Raised by the frontier review of `bf927f3` as its first P1. The authority is
`decisions/2026-08-12-merge-queue-execution-topology.md`: *"`candidate_prepared`: the
**sole** successful settlement for an attempt that produces a candidate … ;
`attempt_finished` is not also emitted for that attempt."*

**The doc that reinterpreted it, now corrected rather than the record.** `settle_succeeded`
argued that INV-07 was *"about which event records the candidate, not about which event
settles the attempt"*. It was not; the record answers that in the same sentence.

**What changed in the frozen file — `src/topology/fold.rs`, +152/−81** (31 doc, 69 comment,
1 blank, **51 lines of code**), and the code is four things:

| | |
|---|---|
| `check_attempt_finished` | refuses `Closed{Succeeded}` outright — the strict door, so the dual pattern is unrepresentable rather than tolerated |
| `check_candidate_prepared` | requires `InFlight`, where it required `Promoting`; the old requirement *forced* the pair the record forbids |
| `apply_candidate_prepared` | performs the settlement — `class = Promoting` — in the same block that records the candidate |
| `check_lease_disposition` | loses its `survives` parameter: every caller now passes a closing generation, and the surviving case moved to `CandidatePrepared::lease_effect`, which `check_candidate_prepared` already matches against the entry's lineage |

**The strict door was chosen over tolerance, as the ruling directed**, and it is reachable:
schema 4 has no external writers (`src/engine/mod.rs` is `pub(crate) mod topology`), so no
log this build did not write can carry the shape. `Spend::replay`'s per-attempt
deduplication is **deleted** — it existed only to survive the duplicate, and a filter that
outlived the shape it was written for would keep a second reading of "one settlement per
attempt" alive beside the fold's, free to disagree.

**One invariant now holds by construction, and it is what closes the review's sequence.**
`class = GenerationClass::Promoting` appears at exactly one place in the fold, inside the
block that sets `candidate = Some(record)`. So **a promoting generation always has a
recorded candidate** — and erratum **E6**'s window, a `Promoting` generation with no
candidate record, cannot occur.

That window was the review's attack: crash between the settlement and the append,
substitute the pin, and `complete_promotions` rebuilt a `candidate_prepared` from whatever
the pin pointed at — deriving tree, message and paths from that commit, so the tree check
added on 2026-08-26 could not catch it, because recovery itself recorded the tree.
**`complete_promotions`, `promoting_without_candidate`, the `Recovered::promoted` field and
the pin-absent refusal are all removed**, because their premise is unreachable. The same
prefix is now a pin with no candidate record — orphan residue, which
`candidate::recovery_for` prunes while settling the attempt interrupted.

**Witnesses re-derived, not patched — and that claim was false when written.**

> **Corrected 2026-08-27.** Roughly twenty-five witnesses failed on the invariant change.
> The ones named below were genuinely re-derived. But `Journal::settle_succeeded`, the
> candidate suite's shared settlement helper, was turned into an **explicit no-op** and left
> at its call sites so the fixtures reaching it would pass without being touched. That is
> patching a shared helper, which is what this sentence claimed had not been done, and the
> round-4 review of `09f9a99` said so.
>
> The real re-derivation is done: the helper and all **seven** call sites are removed, and
> each fixture's sequence is now `task_dispatched → attempt_started → candidate_prepared`
> with no settlement between them. They assert the invariant rather than tolerating it —
> making `apply_candidate_prepared` stop promoting the generation fails **five** of them
> (`pin_pruned_after_promotion`, `promoting_completed_at_run_end`,
> `a_pin_left_by_an_interrupted_promotion_is_pruned_by_the_closure_procedure`,
> `worktree_removal_idempotent_after_candidate_created`,
> `kill_after_candidate_prepared_appends_candidate_created_once`), which they could not have
> done while a no-op stood in for the step.

Each of the named witnesses was re-derived against the invariant, and the diff is
`+75/−390` in `recover/tests.rs` alone:

* `candidate_prepared_is_the_sole_successful_settlement` replaces
  `a_successful_settlement_promotes_the_generation_and_keeps_its_region` — three claims:
  the settlement lands on `candidate_prepared`, a `succeeded` `attempt_finished` is refused
  whatever else is true, and a promoted generation may not then prepare, so **neither order
  of the old pair can be written**.
* `a_candidate_is_prepared_by_the_generation_whose_attempt_is_in_flight` replaces
  `…whose_attempt_succeeded`, which asserted the *opposite* of the new first claim.
* `a_prepared_pin_without_a_candidate_record_is_orphan_residue` replaces three E6
  convergence tests. Same crash, the other expectation: the attempt settles interrupted and
  **no `candidate_prepared` is invented**.
* `a_settlement_records_the_disposition_its_holding_admits` enumerated the one surviving
  lease disposition; it now asserts `succeeded` is refused for **every** disposition, which
  is stronger than the row it replaces.
* Three ordering witnesses lose exactly one `Event.Append`, and the count is the assertion:
  `pin_pruned_after_promotion`, `the_driver_carries_an_accepted_attempt_through_the_candidate_sequence`,
  and the branch's durable-kind list — three appends in the candidate sequence, not four.
* The census's explored traces are one step shorter, so
  `an_overlapping_region_is_explored_and_changes_a_transition_answer`'s differing index is
  regenerated from the shorter trace rather than the assertion being loosened.

> **Appended 2026-08-27, under this same approval — no new one needed, because its own
> sentence mandates the change.** The approval reads *"settlement counting moves to the sole
> event"*, and it did not: `apply_settlement` kept the `attempts_on_rung` increment inline
> and `apply_candidate_prepared` never charged. **A successful attempt spent nothing** — a
> first-attempt success left the rung at zero — and the round-4 review of `09f9a99` found
> it. The suite was green, and the allowance census went on finding its one write site
> because a write site nothing calls still counts as one.
>
> `RunState::charge_allowance` is now the single write and **both** settlement appliers
> reach it: one derivation, not a duplicated increment, because two increments are the two
> rules `the_rungs_allowance_is_counted_in_one_production_place` exists to forbid. That
> census now also counts **calls** to the core and expects two, so a settlement that stops
> charging is a failing census rather than a silent undercount.
>
> **Split for this appendix: +127/−11 on the frozen file** — 44 doc, 6 comment, 7 blank and
> **70 lines of code**, most of it the witness below.
> `a_successful_attempt_charges_its_rung_live_and_on_replay` drives a first-attempt and a
> second-attempt candidate success — they fail differently, one going 0 → 1 and the other
> landing on top of a failure's charge — and compares the live count against a replay of the
> same bytes. Removing the successful settlement's charge fails **both** the witness and the
> census.
>
> **Split for the doors appendix, 2026-08-27 at `584f77e`: +262/−17 on the frozen file**
> — 49 doc, 22 comment, 16 blank and **175 lines of code**, of which the production change is
> nine: `check_candidate_prepared`'s `prepared.attempt.is_successful()`,
> `check_attempt_finished`'s `finished.record.is_successful()` and its envelope comparison,
> and the two refusals they raise. The remaining 166 are fixtures and the four witnesses.
> `src/events/mod.rs` takes **+30/−0** for the predicate pair itself — 20 doc, 8 code, 2
> blank.
>
> **Why this is the same approval and not a new one.** The sentence granted above is
> "settlement counting moves to the sole event, and every settlement-counting witness is
> re-derived against the new invariant". A door that admits a settlement whose own record
> says the attempt failed is not enforcing that invariant, it is enforcing a proxy for it —
> `failure.is_none()` on one door and nothing on the other. `AttemptRecord::is_successful` is
> the invariant stated once, and both doors ask it: the same "one derivation, not two" the
> allowance charge needed, applied to the definition the charge is conditioned on. The
> fixtures moved with it because they had to — the positive premises satisfied the review
> clause vacuously, and a premise that passes for the wrong reason is not a re-derivation.

**Two of the re-derivations were caught by the compiler rather than by care**, and both are
worth naming. `cargo` reported a binding that no longer needed `mut` — which meant the
"Promoting" case of `a_generation_is_closed_only_from_an_open_class_with_no_attempt` was
asserting about an *in-flight* generation while calling itself the promoting one. And the
`survives` parameter went constant, which is how the moved lease rule was found rather than
lost.

### 2026-08-26 — `PR7-CANDIDATE-TREE-UNVERIFIED`, per-instance Class B approval

**The owner's ruling, quoted:**

> **RULED — Class B, per-instance approval granted:** `PreparedCandidate` retains the
> event's `tree_sha`; adoption verifies the commit's tree equals the recorded tree and
> refuses otherwise. Nothing serde-visible moves; this conforms to DESIGN:410 rather than
> amending it.

Raised by the frontier re-review of `c2c0294` as finding B, and carried in §2 before that.
The reviewer's argument is the one that carried finding 2 and was accepted then: a ledger
disposition records a decision, it does not amend the sole living authority.

**What changed, with the split measured at the commit that carries this text.**

| file | +/− | what |
|---|---|---|
| `src/topology/fold.rs` | **+20/−0** | **18 doc lines and 2 lines of code**: `pub tree_sha: CommitSha` on `PreparedCandidate`, and `tree_sha: prepared.tree_sha.clone()` in `apply_candidate_prepared`. No variant, no type widened, nothing deleted. |
| `src/engine/topology/candidate.rs` | +194/−8 | `PromotingCandidate.tree`, the comparison in `verify_object`, the divergent-tree fixture, and the witness. Not frozen. |
| `src/workspace_manager.rs` | +31/−0 | `commit_tree_sha`, the sibling of `commit_parent` and deliberately its shape. Not frozen. |
| `effects/wrappers.toml` | +1/−0 | the new reader classified `effect_free`, which the effects census requires and caught. |

**Nothing serde-visible moves.** `CandidatePrepared::tree_sha` has been on the wire since
schema 4 was defined; this is the fold keeping what it already reads. No event kind, field,
type or serde attribute changes, and `events::SCHEMA_VERSION` is untouched. A log written
before this folds to the same state, with one more field of it retained.

**It conforms to `DESIGN.md`:410 rather than amending it.** That passage requires
`candidate_prepared` to record "exactly one complete attempt/base/commit/tree identity …
so resume adopts only that exact shape". The record already did; the fold dropped the tree,
so adoption checked existence and parent and a commit with the recorded parent and a
different tree passed. `candidate.rs`'s own comment recorded that gap and called closing it
"its own decision" — which is exactly what this approval is.

**Witnesses, and the mutation each dies to.**

| mutation | tree witness | fold-value witness |
|---|---|---|
| *(none — baseline)* | ok | ok |
| the tree comparison is removed (the pre-repair state) | **FAILED** | ok |
| the fold retains `base_sha` in that field instead | ok | **FAILED** |

`promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged` builds a real commit
on the recorded base with a real different tree, asserts that **both** pre-existing checks
pass on it — the object exists, its parent is the base — so the refusal cannot be an
earlier one firing, and then asserts the refusal, that no queue position was taken, and
that no candidates ref was created.

**The second column is a gap the battery found, not a mutation expected to survive.** That
witness constructs its `PromotingCandidate` by hand, so it proves the *check* and not the
*value checked against*: the fold retaining the wrong sha left it green. The assertion now
lives on the recovered promotion in
`a_pin_left_by_an_interrupted_promotion_is_pruned_by_the_closure_procedure`, which is the
only path production takes. Same shape as finding A's second row, one subsystem over — a
witness that bypasses the step it is about.

### 2026-08-26 — `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4`, per-instance Class C approval

**One additive wire field, authorised by the owner on 2026-08-26 against the
measurement that forced it.** Raised by the frontier review of `75da796` as finding 2.
The durable form of the authorization is `decisions/2026-08-26-durable-retry-feedback.md`,
which carries the measurement, the exact shape, the compatibility argument for leaving
`SCHEMA_VERSION` at 3, the passages it serves, and what was rejected.

**What changed.** `crate::events::FailureRecord` gains
`#[serde(default)] pub detail: Option<String>` — what §11.4 sends back to the next
attempt, which `ladder::AttemptFailure::feedback` already unified from both of §11.4's
named sources. `classify::attempt_record` copies it across. Nothing else on the wire
moves; `ReviewRecord` needs nothing, because the reviewer's `required_changes` is
already rendered into `AttemptFailure::feedback` at classify time.

**Why it could not be avoided.** Rebuild-on-resume was the preferred fork and the
grep above closes it: `reason` is the human-facing summary, `ReviewPassOutcome` is
three states with no text, and a resumed run could therefore reconstruct *that* an
attempt failed and nothing about what the next one must do differently.

**Why the frozen files it touches are the ones they are, with the numbers.** Derived
from the staged tree at the commit carrying this text:

| file | +/− | what |
|---|---|---|
| `src/events/mod.rs` | +33/−0 | 21 doc lines and 3 comment lines on the new field, its `#[serde(default)]`, its declaration, and 7 `detail: None,` initializers — 1 production (`Dangling::event`, an attempt nothing judged) and 6 in `mod tests` |
| `src/topology/events.rs` | +1/−0 | one `detail: None,` in `mod tests` |
| `src/topology/fold.rs` | +1/−0 | one `detail: None,` in `mod tests` |
| `src/topology/registry.rs` | +2/−0 | two `detail: None,` in `mod tests` |

The four topology lines are mechanical: the tree does not compile without them, they
carry no comment and no behaviour. They are disclosed here rather than
left to be found: the authorization's scope condition is "no other frozen-file change
rides along", and four one-line fixture updates are the smallest form that still
compiles. The alternative — a `Default` impl or a constructor on `FailureRecord` — is a
larger change to the same wire.

**Not a schema move.** Additive, optional, and the strict door (`refusals[24]`) reports
input keys a record does not claim back, never output keys a record adds.
`a_log_predating_the_detail_field_folds_and_resumes` strips the key from a real
fixture's bytes and resumes the run from the result.

### 2026-08-25 — `PR7-FOLD-OPEN-NO-ATTEMPT`, per-instance Class A approval

**Disclosure row for a twelfth fold reader, filed in the commit that adds it.**
Raised by S5 round 1 as `LOOP-RECOVERED-DISPATCH-DEADLOCK`.

**What changed.** `src/topology/fold.rs` gains `open_no_attempt(key) -> Option<GenerationId>`
— a lookup over [`TopologyFold::task`]'s own state, returning the id of the generation whose
class `apply` already recorded. It decides nothing and derives nothing.

**Why a reader and not a change to `ready`.** `ready` requires `task.open().is_none()`, and
that is **correct**: a task with an open generation is not *ready to be dispatched*. The
continuation is a different question about the same task, and answering it inside `ready`
would make one predicate mean two things. The predicate is fold-owned either way, which is
why this is a reader rather than a driver-side scan of `task().generations`.

**Why it could not be avoided.** `transaction_fault_matrix[T-DISPATCH].resume_action` is
"verify the worktree at the recorded base ... or remove it with force and recreate it ...
**continue attempt (no spend repeats)**". Recovery step (g) recreated those worktrees and
nothing started an attempt in them: `ready` excluded the task, `ready_retry` wants
`RetainedIdle`, and no branch could select it. The run stalled with its only pipeline
entitlement held by a generation nothing could drive, falling through to a closure this
build refuses. `dispatch::resume_open_no_attempt` had no production caller — the design was
waiting for this one.

**Not a new branch.** `eligibility_order` names "eligible integration precedes ready_retry
precedes **new** ordinary dispatch", and a continuation is not a new dispatch. It is the
ready-dispatch branch reaching the same attempt over ground that already exists, so
`LoopBranch` is still the packet's seven.

**A candidate erratum, reported rather than chosen.** `eligibility_order` is silent on where
a continuation sits relative to `ready` and `ready_retry`. At `max_parallel = 1` the question
cannot arise: `T-DISPATCH`'s `authoritative_state` is "entitlement derived from the open
generation", so an open generation holds the run's only entitlement and nothing else is
selectable — an existing test already asserts "`OpenNoAttempt` holds a pipeline entitlement".
At a wider pipeline the two can coexist and the packet will have to say which wins.

**Witnessed in both halves**, per the fold-field class above:
`the_loop_continues_an_attempt_recovery_recreated` fails when the reader never answers
**and** when the selector ignores it — and in both cases the failure is the deadlock itself,
the loop falling through to a closure it refuses.

**Neighbour docs checked.** The reader sits between `frozen_rung_binding` and
`predicted_region`; both still carry their own doc blocks. That check is here because this
file has lost a doc block to an inserted item twice.

**Delegation target named.** `recover::open_no_attempt`'s class check now delegates to this
reader; its repair refusal stays where it is, because that is recovery's policy and not the
fold's.

### 2026-08-25 — `PR7-APPEND-REPORT-READABLE-UNDISCHARGED`, partially repaired

**A guarantee I asserted that was not true as written.** The commit that moved obligation
(3) to the caller claimed the append-error report is "unreachable while invocations still
run, as a compile error". S5 round 1's `emit` lens found the hole: `EmitFailure` and
`EmitError` both implemented `Display` by delegating to the token's, so
`failure.to_string()` — the thing every `?` path does on its way to an operator — rendered
the entire report without discharging anything.

**Repaired for the path that matters.** `EmitFailure::Undischarged` and
`EmitError::AppendFailed` now render only what a caller may know before discharging: that
an entered append failed, and at which site. The outcome, the cause and the creator
disposition arrive with `AppendError`, which still requires the ledger.

**Residue, named rather than closed.** `UncancelledAppend` itself still implements
`Display`. Removing it is the complete fix and it ripples into six `emit` tests that assert
the report's operator text directly; doing that hastily is the "a fix that introduced a new
defect" class, which this project has paid for five times. It is round-2 work, and until
then the honest claim is narrower than the one the earlier commit made: **the count and the
discharge cannot be skipped; the prose can still be read by a caller that destructures the
error deliberately.**

### 2026-08-25 — `PR7-FOLD-LADDER-POSITION`, per-instance Class B approval

**Disclosure row for a frozen-file change, filed in the commit that makes it.** Raised by
S5 round 1 as five findings from three lenses — `loop` ×2, `settle` ×2, `contract` — which
is one defect seen from three directions.

**What changed.** `src/topology/fold.rs`: `TaskFold` gains `rung: u32` and
`attempts_on_rung: u32`. The rung is assigned from `SettlementTransition::Escalated { rung }`,
which the packet defines as the rung an escalation climbs *onto*; the counter increments at
the `attempt_started` arm that already wrote `generation.attempts`, and resets on escalation
because the allowance is per rung. Both read through the **existing** `TopologyFold::task`
reader — no new reader.

**Why it could not be avoided.** The fold *validates* `attempt_started.rung` against the
frozen ladder and then discards it: `GenerationFold` has no rung, and `TaskFold` had no
ladder position at all. Meanwhile `SettlementTransition::Retry | Escalated` closes the
generation and **does not set the task's state**, so the task stays `Pending` and the
ready-dispatch branch selects it again — at a rung nobody could read. The driver assumed
`rung 0, attempts_on_rung 1`, and I had justified both as "properties of the branch".

**That justification holds only for a task that has never been attempted.** For any task
past its first generation it is wrong twice: an escalated task is dispatched on rung 0
forever and never reaches the tier its chain escalated it to, and `next_step` always sees
the first attempt of the allowance, so the task retries forever and never escalates at all.
Neither shows up as a wrong number — only as a run that behaves differently after a restart.

**Why the fold owns it.** The same reason as [`PR7-FOLD-DEFERS-ACCUMULATOR`]: a ladder
position survives a resume and a process-local tally does not. Witnessed in both halves —
`a_ladder_position_is_derived_by_replay_and_not_assumed` for the accumulation (fails at
`left: 0, right: 1` when the escalation arm is removed) and
`the_driver_spends_the_allowance_the_log_records` for the read (the driver settles `Retry`
instead of parking, "and the task retries forever", when `ladder_position` is replaced by
the old constants). The second witness exists because the first mutation of the read
**survived**: the fold half being witnessed says nothing about the driver reading it.

**Why the contract owes it.** `pr_sequence[8].scope` names "failed/interrupted/deferred
settlements" and the "same-generation retry path"; `permitted_transitions` names
"Pending -> dispatched generation -> attempt"; and the fold itself returns an escalated task
to `Pending`. A build that dispatches it at the wrong rung is not implementing that
transition.

**What did not change.** No event, no serialization, no transition, no refusal. The fold
holds the position and only the position; `attempts_per` and the chain stay in
`ladder::LadderPolicy`, read from the frozen entry.

**A second defect found while witnessing it.** The park question quoted
`plan.attempt` — this *generation's* attempt number — where the human needs the task's
spend on the rung. After two attempts a park said "1 attempt(s)". Fixed in the same commit,
and asserted by the same test.

### 2026-08-25 — `PR7-FOLD-DEFERS-ACCUMULATOR`, per-instance Class B approval

**Disclosure row for a frozen-file change, filed in the commit that makes it.**

**What changed.** `src/topology/fold.rs`: `TaskFold` gains a `defers: u32` field, set from the
settlement's own number at the `SettlementTransition::Deferred` arm that already handled the
transition. Read through the **existing** `TopologyFold::task` reader — no twelfth reader. One
private setter, `set_defers`, assignment not increment.

**Why it could not be avoided.** `ladder::next_step` reads `LadderState::defers` on exactly one
branch: an outage defers while `defers < max_defers` and parks at it. Schema 4 had no reader for
that count anywhere. `SettlementTransition::Deferred { defers }` was written into the log and the
fold never accumulated it: `TaskFold` had no such field, and `TaskState::Deferred` is a unit
variant. The legacy engine keeps the count in `state.progress[index].defers`, which is in-memory
schema-3 state; a schema-4 run derives everything by replay.

**Why the fold owns it rather than the driver.** A process-local tally agrees with the log on every
reading except the one after a resume, and then it reads zero while the log holds three — so a run
that had already spent its allowance would defer a fourth time, a fifth, and never park. That is
`PR7-REGION-SECOND-DERIVATION`'s shape with a resume-shaped fuse: two derivations of one number,
agreeing until they do not. `a_deferral_count_is_derived_by_replay_and_not_by_a_process_local_tally`
is the witness, and it fails at `left: 0, right: 3` when the accumulation is removed.

**Why the contract owes it.** `pr_sequence[8].scope` is "failed/interrupted/**deferred**
settlements"; `permitted_transitions` names "failed (Retained | Closed | **Deferred**)" and
"Deferred -> Pending via defer_wait_elapsed"; `durable_events` lists `defer_wait_elapsed`. **T-FAILED
is in this slice's `gating` and `replay_recovery` ranges**, its `durable_state` reads "Deferred marks
the task Deferred", and two of its named proof tests —
`deferred_task_woken_by_defer_wait_elapsed_or_resume` and
`deferred_task_does_not_block_halted_or_budget_exceeded_closure` — cannot pass without a deferred
settlement existing to wake. The backoff branch was already live and nothing could produce a
deferral for it to wake.

**What did not change.** `max_defers` stays policy, in `ladder::LadderPolicy`, read from
`run_started(4).limits`. The fold holds the count and only the count. No event, no serialization, no
transition, and no other reader moved.

**Measured split.** The fold change and this row land together, with the suite at 1667/0 and the
witness green; the driver consuming the reader and deleting its refusal is the commit after.

**The driver's read is witnessed too, and it was not at first.** The value is load-bearing only on
the outage branch, so replacing `TopologyRun::deferrals_recorded`'s expression with a constant zero
once left the whole suite green — measured, and named in that function's own doc rather than left
silent. `the_driver_settles_an_outage_from_the_folds_deferral_count` closes it: an agent whose CLI
reports a rate limit, one deferral already durable in the fixture's log, and the settlement asserted
to record `defers: 2`. The mutation now fails at `left: [1, 1], right: [1, 2]` — which is exactly
the failure mode, a run that records `1` forever and never parks.

### 2026-08-24 — the PR7 unfreeze challenge, adjudicated

**Challenge.** `reviews/2026-08-24-unfreeze-challenge-request.md`, filed by the PR7 implementer
against the 2026-08-20 ruling carried on `PR4-SPAWN-SITE-PROBE-CONTEXT` and
`PR4-PROGRAM-PATH-NOT-UNICODE`. It argued that the ruling's two named things —
`src/topology/effects.rs` and `DESIGN.md:222` — do not cover a **public reader** that delegates to
logic already in a frozen file, and proposed a standing rule making such readers always permissible.
Its new evidence was `PR7-REGION-SECOND-DERIVATION`: a private, load-bearing derivation
(`fold::predicted_region`, which `dispatch_lease_check` uses to decide a task is `ready` at all), a
second derivation written in the engine to avoid touching that file, and the two disagreeing — the
fold admitting a dispatch on `src/alpha` while the log recorded `src/alpha/*.rs`, a prefix that
overlaps nothing. Shipped green in `199dc1d`; repaired in `84a3978`.

**Adjudicated by the project owner, 2026-08-24**, after an independent adversarial review of
`3c09f6e`. Three parts:

1. **The footprint is accepted**, as a **disclosed deviation**, through `3362f65` — the ten readers,
   the `pipeline_reservable` conjunct, and the eleventh reader. It stays. See
   `PR7-FOLD-ACCESSORS-IN-PR3-LAYER` in §2 for the measurement.
2. **The standing rule is rejected.** "Readers to frozen files are always fine" does not become
   policy. `frozen_rung_binding` is the **last fold reader outside a dedicated pass**.
3. **A freeze charter replaces the ad-hoc reading**, landing as
   `decisions/2026-08-24-pr3-layer-freeze-charter.md`, with the work itself scheduled as
   `proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md` — a slice that runs **after PR7 merges and before
   PR8**. Every further `src/topology/**` change goes there, including the three findings §2 already
   carries for it and `TASK-DISPATCHED-REGION-UNVALIDATED` below.

**What this settles for the rest of PR7.** No further edits to `src/topology/**` or `DESIGN.md:222`
from this slice. A lane that finds it needs a twelfth reader or a new derivation **stops and writes
the need up as an owner question** — it does not work around it with a driver-side re-derivation,
which is the defect `84a3978` repaired and this slice's dominant class.

**Why the challenge succeeded on evidence and failed on rule.** The evidence cleared the authority
rule's bar: a concrete failure sequence the 2026-08-20 disposition did not address, and a mutation
that demonstrably survived the whole suite. What it did not establish is that a *category* of change
is safe. One slice is one data point, and the reviewer's strongest objection stands — that eleven new
public methods is a claim about `TopologyFold`'s original surface, and answering it by widening the
surface one reader at a time is the redesign the ruling forbids, performed slowly.

## 4. Recurrence watch

Classes seen more than once. Two occurrences is a signal about the method.

| Class | Occurrences | Where | What it says |
|---|---|---|---|
| **A surviving mutation named in a round's own prose and carried nowhere durable** | **2** | `PR4-CONF-009` (round 6 §349-352 named the `.cmd` suppression verbatim, did not repair it, and did not file it); `PR4-CONF-008` (round 6 named the `main.rs` wiring hole, filed it, and then deferred it for a process reason) | **A measurement taken and not triaged is worse than one never taken, because it is evidence you already hold that you chose not to act on.** Round 6 found both of round 8's repairable findings and shipped neither. One was named only in a report that lives outside the repository, so the next reviewer had to re-derive it from the source; the other was filed here but deferred on "it arrived after this round's scope was fixed", which is not a reason the contract recognises. **The rule adopted, and it is mechanical:** a repair round that names a surviving mutation in prose and does not repair it must append it to §2 *in the same commit*, with an owner and a live-passage test — and a deferral must quote the passage that makes it out of scope, never the round's own schedule. A finding whose only home is a report file has no home. The reports themselves are not in the repository; this table is |
| **A boundary drawn narrower than the packet's sentence** | 2 | `PR3-ST14-006` (round 5's trace-ceiling skip); `PR3-ST07-014` (round 4's site-artifact scope) | **Distinct from a fix that introduces a defect, and it should not be counted as one.** In both cases the round documented the boundary, gave a reason, and made it observable — round 5's skip carries its rationale in a comment, counts the skipped states, and asserts `deferred_states > at_ceiling` so the skip cannot grow silently. The finding is still real where a live packet sentence says otherwise (`coverage_assertions` says *every* state), but the failure mode is "narrower than required", not "concealed". **A reviewer must distinguish the two, or every fix generates a finding forever** — each fix draws a boundary and a boundary can always be measured against some sentence |
| **A fix that introduced a new defect** | **5** | `PR1-ORDER-001-ABA` (PR1); `PR3-ST07-011` and `-012` (PR3 round 3); **`PR7-BLANKER-DESYNC`**, **`PR7-HUSK-BRICKS-RESUME`** and **`PR7-RETRY-ATBASE-UNGUARDED`** (PR7 round 2, all three from round 1's repairs) | **The strongest argument for the independent final confirmation, and the reason round count is itself a risk.** PR1's was a fix *specification* with a hole. PR3's were fixes structurally right and wrong at the boundary: `semantics(Before)` returned empty rows, so the framework refused a packet-correct `[R9]` entry and accepted a false empty one — the exact inversion of its purpose. Guard adopted for round 4: for every change, state what the *new* code could get wrong and write the test that catches it. **PR7 tripled the class in one slice**, and its worst case landed in a census's own blanker, where a desync failed *open* and hid forged code from every instrument with a zero-byte region delta: when a repair's subject is an instrument, the question is not "does it still detect" but "how does it behave when its own parser loses sync" |
| Tests satisfied by a correlated field rather than the named one | 11 (PR2) + 11 (PR3/A1) + **2 (PR4)** | PR2 registry tests; A1 fixtures; `PR4-CONF-004`'s role grid (`Implement`/`Review` hand-built with `agent: None` and a gate identity); `PR4-CONF-006`'s role grid again, one field over (`stdin: Vec::new()` on every request, plus the recorded shell and one timeout for all five roles) | Fixtures must vary every independently meaningful field independently; assert hostility as distinct-value **counts**, not prose. **PR4 adds the structural half of the guard**: where production has a builder per role, a fixture that writes its own request is the defect, so the builders are now the only construction points and a census says so. **`PR4-CONF-006` says that half is not enough.** Rounds 4 and 5 each swept the fields their author enumerated and the next confirmation found the one nobody listed — a builder fixes role, binding and identity, and leaves everything it is *handed* to the fixture. The guard adopted in round 6 is the one already required for transcription: derive the field list from the **type**, not from intuition, and report every field marked covered or not, because mutation witnessing cannot detect a dimension nobody varied for the same reason it cannot detect an omitted field |
| **A guarantee proved for the variant that was looked at** | **4** | `PR4-CONF-002` (`Probe(_)` roles got `NoHooks`); `PR4-CONF-003` (the public facade entries never established containment); `PR4-CONF-005` (the production mint's failure branch unreachable); **`PR5-C-APPEND-SITE-GRID`** (two of three schema-4 append sites never driven) | **Distinct from the correlated-fixture class below, and it needs its own guard.** There the fixture had the right shape and the wrong *values*; here the fixture is right and the *domain* is short — one role, one entry point, one site — and the missing cells are the ones the author already believed were the same. Reading the code and concluding "the site is not consulted anywhere in this function" is exactly the reasoning that failed in `PR4-CONF-002`. **The guard is mechanical and is now used in three places:** derive the domain from the **type** (`EventSite::ALL`, `sub_effects()`, `modes()`, `TOPOLOGY_APPEND_SITES`), drive **every** member, and assert per member that the evidence came back **under that member's own name** — a coordinate recorded under the wrong site is the same defect wearing a passing test |
| **The thing that was supposed to prove it never ran** | **2** | `PR4-CONTRACT-NAMED-PROOF-TEST-DELETED` (a contract-named proof test deleted; twelve gates and three CI platforms green because none read the contract); **`PR5-C-DOCTEST-FIXTURES-NEVER-RAN`** (three `compile_fail` fixtures for a contract-named build refusal, none of which any gate executes, because `--all-targets` excludes doctests) | **A green suite says nothing about a test that is not in it.** Both were found by asking *which command runs this?* rather than *does this pass?* — and in both the answer was "none", with every gate green. The rule adopted: **when a claim's only evidence is a fixture, name the command that executes it and check that the command is one CI runs.** For build refusals specifically, `compile_fail` doctests are documentation; the executable proof has to live in a run target, and it has to include a positive control, or a broken toolchain invocation makes every fixture "refuse" |
| **A source census fooled by a comment** | **5** | `PR4-CENSUS-COMMENT-ORACLE` (`every_production_process_start_is_classified` counts literal occurrences, so a doc comment changes an expected number); round 5's second census on the same mechanism (`every_production_runner_request_is_built_by_its_roles_builder`); **`PR5D-CI-COMPONENT-CENSUS-COMMENT-ORACLE`** (a census for the substring `clippy` in a CI job, satisfied by the nine-line comment explaining why the `components: clippy` line exists — so deleting the line left it green); **`PR7-CENSUS-BLANK-COMMENTS`** and **`PR7-CENSUS-PROSE-COUNTED`** (a `strip_comments` that removed `//` only, and counting censuses that conflated code with prose) | **The class has now cost a real hole, not just a measurement hazard.** PR4's two are recorded as hardening because the expected count is independently derivable; PR5D's was a *defect*: the census's whole subject was "does the job install the compiler these fixtures need", and it answered yes to a comment. **The guard, from PR4-CONF-008's `run_wired` census and now used a third time:** strip comments before counting, **assert the strip removed something**, and where possible assert on structure (a line that starts with `components:` and contains `clippy`) rather than on a substring anywhere in the file. Any census over a file format that has comments — Rust, YAML, TOML — is in this class. **PR7 adds two, and they are the reason the class is now a shared helper rather than a habit**: a block comment *or a string literal* collapsed a whole production region (live in this crate at the reviewed SHA), and a counting census over unblanked text meant **deleting prose bought a real call**. `effects::production_code` is now the one implementation, and it blanks cfg-test *items* in place rather than truncating, because cutting the file at the first attribute is the same defect wearing a parser |
| **An enforcement artifact no gate validates** | **2** | **`PR5-C-DOCTEST-FIXTURES-NEVER-RAN`** (three `compile_fail` fixtures no command executes); **`PR5D-UNRESOLVED-DENIAL-IS-A-WARNING`** (a `clippy.toml` denial whose path does not resolve enforces nothing, and clippy says so with a bare `warning:` that `-D warnings` does **not** escalate — measured; for a path whose crate is not linked, it says nothing at all) | **Sibling of "the thing that was supposed to prove it never ran", one level out: there the *test* did not run, here the *rule* does not bind, and both are green.** The rule adopted: **an artifact that enforces something must itself be checked by something that runs.** For a denylist that means proving every entry resolves — with a control that injects a typo, because a probe that silently lints nothing reports an empty set and passes |
| **An element of a packet-named sequence with no implementation at all** | **2** | **`PR7-RECOVERY-STEP-G-MISSING`** (`recovery_order` names steps (a0)–(i); the implementation runs every one but (g), and the function step (g) would call has zero production callers); **`PR7-NO-TOPOLOGY-RUN`** (`engine` and `selection` both name the driver; no such type exists and every top-level entry point is reachable only from its own tests) | **Omission has nothing to mutate, and this class is what that costs at the level of a step rather than a field.** PR3 learned it for event fields — *"mutation witnessing cannot detect omission; transcription slices need a reconciliation table against the packet's named enumerations"* — and the lesson was applied to fields and never to sequences. All 117 named tests passed, every gate was green, and two per-lane review rounds read the lanes that existed. Both were found by asking **which command runs this?** rather than *does this pass?* — the same question that found `PR4-CONTRACT-NAMED-PROOF-TEST-DELETED` and `PR5-C-DOCTEST-FIXTURES-NEVER-RAN`, one level further out: there the test did not run and the rule did not bind; here the **step does not exist**. **The guard, and it is mechanical:** a slice whose contract names an ordered sequence carries a test that enumerates the sequence *from the packet's text* and asserts exactly one implementation per element — presence, not correctness. A step absent, or present twice, fails it, which would also have caught this slice's duplication findings |
| **`git checkout <path>` discarding uncommitted work while mutation-testing** | **2** | both in PR7's session, both while restoring after an armed mutation: the `predicted_region` narrowing in `engine/topology/run.rs`, and the classification delegation in `engine/attempt.rs`. Recovered from a `cp` snapshot the first time and by re-running the scripted edits the second | **Two occurrences is a method signal, not a person's memory, so it is a rule here rather than a resolution.** Mutation testing means deliberately breaking the tree and putting it back, and `git checkout <path>` puts back *the committed* version — which silently discards every uncommitted change to that file, including the work the mutation was testing. It is worst exactly when it is most tempting: mid-experiment, on a file you have just edited heavily. **The rule: snapshot the file before arming any mutation (`cp <file> $TMP/<name>.orig`) and restore from that snapshot. `git checkout <path>` is forbidden while uncommitted work exists anywhere in the tree.** A `git stash` is not the escape either — it moves the problem to a stack whose entries are easy to lose track of across a long session |
| **An item inserted into a file re-targeting the doc comment above it** | **11 at `51cfc01`, derived not maintained — see the cell** | both in PR7's session: two documented fold readers inserted above `questions_open` took its doc block (`bb68cf6`); the `ReviewPasses` trait inserted into the middle of `AttemptPlan`'s doc block, splitting one sentence across two items and shipping that way at the green head `9fcca67`. **Four more found by S5 round 2's `seams` and `contract` lenses, all introduced by the round-1 repair diff**: `TopologyRun::commitment_digest` took `fold`'s block *and its `#[must_use]`* (`6a21be6`, the same session that filed this class); `Spend::new`'s block landed on `run_total`, leaving the constructor the driver calls undocumented; `fn attempt`'s block landed on `const FIRST_ATTEMPT` (`1de76cf`) and **two of its sentences then went false** — it cited `LoopBranch::owes`, which has zero call sites, and argued at length that attempt and rung are *not* read from the fold, which is now the opposite of the code; `Started::into_parts`'s block and `#[must_use]` went to `into_handle`, whose `# Errors` section then described a `Result` the signature does not return. One further site, `BarrierHeld::fold`, was recorded here as **refuted** on inspection and **that refutation was wrong** — corrected 2026-08-26 after round 3's `emit` lens re-raised it. The inspection checked `BarrierHeld::fold()`, the **accessor method**, which is correctly documented; the finding was about the `fold` **field**, where `recover.rs:802` **as it stood at `80a141b`** carried "The fold built from exactly those bytes, and no others." is followed by `events` and its four doc lines, so all five `///` lines attach to `events` and the `fold:` field is undocumented. **A field and its accessor are two items with the same name.** The commit that recorded the false refutation (`80a141b`) is pushed history and is corrected here rather than rewritten. So the class stands at **8** occurrences, not 7, and a refutation of mine reduced the count — which is the more useful half of this entry. **The count, derived, because a count in a recurrence table is a verification claim**: two fold readers above `questions_open` + `ReviewPasses`/`AttemptPlan` + four from S5 round 2 + `BarrierHeld::fold` + the `production_sources` insertion below = **9**. The cell read **6** while the derivation said 8; corrected to 8 on 2026-08-26 (S5 round 4, `R4-SEAMS-004`) **by the same commit that committed the ninth occurrence and named it in this cell** — so the number was one behind its own prose again, which is `R4-SEAMS-004` reintroduced in the commit that repaired it. Found by S5 round 5's `attempt` and `settle` lenses independently (`PR7-R5-ATT-004`, `R5-SETTLE-005`). **The rule the second occurrence adds**: a count and the prose beside it are edited in one motion, and a sentence that adds an instance edits the number in the same diff hunk. **And the rule the third and fourth add, which is that the first rule does not work.** S5 round 6 found the cell at **9** while the head carried **11**: `765a2f7` committed occurrence 10 (`OFFERS_WORK` and `OFFERS_NO_WORK` between `fn arm_label` and its doc block) and occurrence 11 (`production_calls`, `Call` and `whole_file_test_modules` between `declared_whole_file_test_modules` and its doc block, in the very module that exists to hold shared census machinery) — and `8e48dd1`, the commit that corrected the cell to 9, was its child. That is three consecutive corrections each made by a commit whose own diff added occurrences: 6 when the prose said 8, 8 by the commit that committed the ninth, 9 by a commit whose parent committed the tenth and eleventh. `R6-SETTLE-006`. **So the column stops being a maintained number.** It now reads *derived at a named sha*, because a maintained count in this project has been wrong three times out of three, and a reader deciding whether a class warrants an instrument would rather have a number with a date on it than a number that looks current. Both new occurrences are repaired at `51cfc01`+, each by moving the inserted items rather than the prose. **The mechanical rule that follows**, and it is the one that would have prevented all four: an insertion's anchor is the **start of the target item's doc block**, never its `fn` line — and every one of these was made by a script anchored on the signature, and the §4 row below was orphaned from this table by a blank line, so the newest rule binding every reviewer rendered as a paragraph rather than as a row (`R4-SEAMS-005`) | **There IS a free detector, and it was in hand and misread.** A split strands the previous item's attributes onto the new one; when both carry `#[must_use]`, rustc emits *"unused attribute … attribute also specified here"*. That warning fired at `run.rs:637` **as that file stood at `bb68cf6`** during the very session that filed this class, and was resolved by deleting the **newly written** attribute — silencing the one signal that says a block was split. **Measured 2026-08-26, both directions**: plant a split where the inserted item also carries `#[must_use]` → the warning fires, and CI runs clippy at `-D warnings`, so it is a build failure rather than a rule; plant one where the inserted item carries no attribute → **silent**, the stranded attribute simply applies to the new item. So the detector covers the attribute-collision half and nothing else. **A second free detector, found 2026-08-26 by committing occurrence 9 of this class while repairing it.** Three helper `fn`s were inserted between `runner::tests::production_sources` and its doc block, whose last line is a `*` list item; `cargo clippy --all-targets --all-features -- -D warnings` refused with `error: doc list item without indentation` at the first line of the inserted doc — `-D clippy::doc_lazy_continuation`. So the detector fires whenever the **stranded** block's last line is a list item and the inserted item carries a doc comment of its own, which is a different half from the `#[must_use]` collision and a much more common shape. It does not fire when the inserted item has no doc comment. **The rule that follows**: an anchor for an insertion is the start of the target item's *doc block*, not its `fn` line — and this occurrence was caught by a gate rather than by the ceremony, which is the third time that has been true for this class. **The rules, in order of cost:** (1) an *"attribute also specified here"* warning on an item you just inserted is the **previous** item's attribute, stranded — never resolve it by deleting the one you wrote, look up; (2) after inserting any item, read the **rendered** neighbourhood, not the diff — the ceremony already said "neighbour doc-attachment checked" and it did not save the author of occurrence 3, who checked the diff; (3) a doc block whose last sentence does not terminate is the tell. **And a fourth, which occurrences 3-6 add:** a re-targeted block does not merely point at the wrong item, it stops being maintained — nobody edits a doc they cannot see is attached to what they are changing, so the sentences rot into false claims. Two of the four had already done so |
| **A mutation whose anchor `cargo fmt` had moved, reported as a surviving mutation** | **2** | PR7's session: test anchors taken from an unformatted file after `cargo fmt` reflowed them, and — in the candidate-sequence lane — a `str.replace` mutation whose multi-line anchor `cargo fmt` had since rewrapped, so the replace matched nothing, the tree built unchanged, and the test passed | **A mutation that does not apply and a mutation that survives are the same observation and opposite conclusions.** The second occurrence was read as "the sequence is unwitnessed" and nearly bought a rewritten test for a defect that did not exist; the tell was that the *first* attempt reported survival on a test whose assertion visibly covered the mutated events. **The rule: a mutation script asserts its own anchor matched (`assert old in t`) before writing, and a surviving mutation is re-run once with the assertion in place before it is believed.** Taking anchors from the formatted file is necessary and not sufficient — `cargo fmt` runs again between arming and measuring |
| **An accumulator's witness proves the accumulation and not the read** | **4** | `PR7-FOLD-DEFERS-ACCUMULATOR` (the fold-level replay witness was green while replacing `TopologyRun::deferrals_recorded` with a constant zero left the whole suite green); `PR7-FOLD-LADDER-POSITION` (same shape, same day: the escalation-arm mutation died instantly, the `ladder_position` mutation survived); **`PR7-SPEND-REPLAY-UNREAD`** — `Spend::replay` had **no production caller at all**, so every resume handed the run its whole budget back, found by S5 round 2's `seams` and `loop` lenses **one commit after this class was filed**; **`PR7-LADDER-POSITION-RUNG-HALF`** — the `rung` half of `ladder_position`'s own reader, still unwitnessed after the repair this class was filed from, found by round 2's `settle` lens | **The two halves are different claims and only one of them was ever being tested.** A replay witness asserts *the value is derived by replay and survives a resume*; it cannot see the driver ignoring the value, because the driver is not in it. It *looks* like coverage of the feature and is cited as such. **The rule: any accumulator rebuilt by replay that a driver consumes carries two witnesses — one that it is derived by replay, and one that the driver's behaviour changes when it does.** The second is written by replacing the driver's reader with a constant and requiring a *named* test to fail; a fixture that cannot make the value observable (the deferral count needed a *prior* deferral in the log; the ladder position needed a *spent* allowance; the spend needed a **non-zero** cost) has not tested the read at all. **Re-scoped 2026-08-26, and the re-scoping is the lesson.** This was filed as "a **fold field's** witness…", and the narrow name is what let occurrence 3 through: `Spend` is not literally a fold field, it is a driver-side accumulator rebuilt by `Spend::replay` — it *behaves* as one, and the author of the repair skipped the prescription on that distinction. Occurrence 4 is worse and settles it: the narrow name also let through **half of a named instance**, since `PR7-FOLD-LADDER-POSITION`'s repair witnessed `attempts_on_rung` and not `rung`. A class whose own filed instance is still partly open was not a class, it was an example |
| A function used as its own expected-value oracle | 5 (PR3/A1) | `RunnerContract::kind`, `VerificationRecord::passed`, `GitPath::from` | Expected values come from the packet's text or an independent table, never from the function under test |
| A grid bounded short of its required domain | 8 (PR3/A1) | upgrade totality `to<=6`, reader selection, `is_topology_schema` | State what bounds each grid and why that bound is sound |
| Omitted packet-required fields | 7 (PR3/A1) | `RunStarted4.integration_ref`, `.execution_root` | **Mutation witnessing cannot detect omission.** Transcription slices need a reconciliation table against the packet's named enumerations |
| **A refutation that inspected the wrong item of that name** | **1** | `BarrierHeld::fold`, round 2: the finding named the **field**, the refutation inspected the **accessor method**, found it correctly documented, and recorded "refuted" in a commit message and in §4. Round 3's `emit` lens re-raised it with `git blame` and it was right — the field's doc block had been taken by an inserted `events` field | **A refutation is a claim, and it was the only claim in this ledger nobody re-derived.** Every *finding* here carries a failure sequence and a mutation; the refutation carried neither, and it silently reduced a recurrence count — which is worse than a missed finding, because the count is what decides whether a class gets an instrument. **The rule: a refutation must name which item it inspected, and must check every item carrying the name.** A field and its accessor, a method and a free function, a type and its module: same identifier, different items, and `grep` for the bare name finds all of them where a reader looking for "the" definition finds one. The cheap form is to quote the line number and the item kind in the refutation itself, so the next reader can tell what was actually looked at |
| **A command quoted as evidence becomes part of its own input** | **4** | all four introduced by this session's own claims-protocol commits, 2026-08-26: `select.rs` quoting `an_ending_run_reaches_closure` (the grep then reports a hit for a test that does not exist — the exact thing the sentence denies); `run.rs` quoting `fn drive`; `emit.rs` quoting `cancel_all_running(`; `run/tests.rs` quoting `fn the_retaining_incarnation_retries_in_place`. Each doc says a count and each count is now one higher than the doc claims, because the doc is in `src/**/*.rs` and the command is `grep -rn … --include='*.rs' src/` | **The documentation half of `PR4-CENSUS-COMMENT-ORACLE`, and it arrives with the claims protocol rather than despite it.** The protocol says a verification claim carries the command that verified it; writing that command into the tree makes the tree a different tree. A census that counts prose was the first half and was closed by `effects::production_code`; this is the same defect where **the reader** is the instrument, and no blanking helps because a person running the quoted command sees the raw file. **The rule**: a command quoted as evidence inside the tree is written in a form that stays true under being quoted — append `| grep -v '///'`, or restrict the path to one the doc does not live in — and the doc says the filter is load-bearing rather than tidy. Found by re-running my own four quotes before round 5 did; all four repaired in the same session that introduced them |

## The hardening rule

**A finding that strengthens a guarantee beyond what the frozen packet requires is not a defect.**
It is recorded here as managed debt and scheduled, not repaired in the slice that surfaced it.

The test, applied per finding:

- **Defect** — a live `decisions.*`, `invariants` or `transaction_fault_matrix` passage says the
  current behaviour is *wrong*. There is a concrete failure sequence against the packet, and a
  mutation the suite does not catch. Repair it in-slice.
- **Hardening** — the current behaviour satisfies the packet, and the finding proposes a *stronger*
  property: a runtime check promoted to compile time, an invariant asserted from a second direction,
  a guarantee the packet never asked for. Record it here with an owner and a slice; do not repair it
  in the slice that surfaced it.

Two reasons this is the right cut. Round count is itself a risk — each repair round rewrites tests
and inverts assertions, and every rewrite is a chance to encode a defect as an expectation. And the
project already has the precedent: `ae9e9da` shipped naming PR2's remaining test-sufficiency debt in
its commit message rather than grinding a sixth round, and the handover records that as the right
call.

**Applies from PR3's third confirmation onward.** Authorised by the project owner, 2026-08-18.

| ID | Finding | Packet says current behaviour is wrong? | Disposition |
|---|---|---|---|
| PR4-INVOCATION-CONSTRUCTIBLE | `InvocationId` is a `pub` enum, so `InvocationId::Probe { .. }` can be constructed directly, bypassing `InvocationId::probe` and yielding a value `parse` later refuses. The domain is closed by validation, not by construction | **No.** `decisions.admission_and_leases.permits.invocation_identity` requires the identity to carry one of three enumerated shapes and to be *"deterministic in the sequential substrate"*. Every value the constructors produce satisfies both, and rendering is injective over the tuple. No live passage requires invalid states to be unrepresentable | **Hardening**, owner: PR7 implementer (the slice that assigns identities for real). Promoting a runtime check to compile time is this rule's worked example. Raised by the correctness lens as claim 10, outside the 27 findings |
| PR4-CENSUS-COMMENT-ORACLE | `runner::tests::every_production_process_start_is_classified` is a source-text census that counts literal occurrences per file, so a doc comment mentioning `run_with_timeout` changes an expected number. Three catalogue mutations were first recorded KILLED on comment deletion rather than on their own point | **No.** The packet asks for the site census; it does not specify a parser that excludes comments. The expected count is independently derivable by hand, which is what the no-self-oracle rule requires | **Hardening**, owner: PR5–PR7 implementer. Recorded because it is also a *measurement* hazard: any future catalogue run against this suite must re-apply surgically when a mutant dies only on the census test. Round 5 added a second census on the same mechanism (`every_production_runner_request_is_built_by_its_roles_builder`), so this hazard now covers two tests |
| PR4-ADAPTER-RESOLVES-ON-THE-HOST | `ClaudeCodeAdapter::probe` (`src/agent/claude.rs:75`) and `build` (`:135`), and both siblings, resolve the agent CLI on the **coordinator host** — `locate()` before the Runner is asked anything — and serialise the resulting absolute host path into `CommandSpec.program`. Two consequences: an agent present inside the selected Runner boundary but absent on the host is refused at pre-flight *without the Runner ever being asked*; and an agent present on both at different paths yields a spec carrying a machine-specific program that names nothing at the boundary which executes it. Neither is constructible in PR4 | **No.** DESIGN.md:117's "it does not decide where the process runs" is the *boundary* choice, and an adapter makes none: it is handed a `&dyn Runner`, never names or constructs one (`runner::tests::every_production_process_start_is_classified` asserts that by count), while the same sentence makes the adapter the thing that knows "an official CLI". DESIGN.md:216 and :612 require the probe to **run through** the runner, which it does, and :612 states the failure they exist to prevent: "Probes run through that same runner, **or pre-flight could certify a host CLI/version different from the one the attempt executes**." In PR4 pre-flight and the attempt share one cached resolution and one `HostRunner` whose boundary **is** the coordinator host, so that sentence has no constructible counter-instance here; PR4's `non_goals[0]` is "container runner". This is a ruling, not an assumption: the current behaviour satisfies every live passage, and resolving behind the Runner is a *stronger* property | **Hardening**, owner: **PR6 implementer**. Raised by the frontier review of `4631a3f` as `PR4-CONF-012`. **PR6 is where it becomes a defect, and by its own scope**: "probe-through-runner … inside a container from the recorded image id", and "shell/CLI availability observed **only** by the RunnerPreflight probe spawns". What newly breaks there: (a) the *normal* container case — CLIs pinned in the image, none on the coordinator host (DESIGN.md:612's "an image with version-pinned CLIs") — is refused at pre-flight before the runtime is asked anything, so a correctly configured container run cannot start; (b) where both have the CLI at different paths, every spec carries the host's path and each spawn fails inside the container pointing at a path the operator never wrote; (c) `Caps.version` certifies the host's CLI while the attempt runs the image's — :612's sentence, exactly. The repair is that the program stops being a host path: either `CommandSpec.program` carries the bare CLI name and the runner resolves it against the environment it composes (DESIGN.md:222's `program: String` already admits it, and `codex::locate` already tests candidates *through* the runner), or the Runner grows a resolution call the adapter asks. `agent::built_program_tests::an_adapters_program_is_the_coordinator_hosts_and_the_boundary_supplies_none` pins today's behaviour against a boundary the test invents rather than against this machine's filesystem, so PR6's change fails it **by name** rather than silently |
| PR4A-SPAWN-WITHOUT-AMBIENT | A `HostRunner` constructed outside any coordinator — `HostRunner::new().run(&request)` in a downstream crate, or `examples/probe.rs` — spawns on Windows with no ambient job, so a kill between `CreateProcessW` and private-job assignment leaves a suspended stub. `HostRunner::run` could refuse when no ambient job exists | **No.** INV-18 is scoped to "the **coordinator's** ambient kill-on-close Job Object", and `crash_reconstruction` anchors establishment at "at process start every **write command**". A caller that is not a write coordinator is outside both sentences — which is why `connect` and `capacity` spawn agent CLIs without one and are named and counted as doing so (`main::tests::the_commands_that_spawn_outside_a_run_are_named_and_counted`, `engine::tests::no_read_only_public_entry_point_establishes_containment`). After round 5 every *coordinator* entry point establishes containment and cannot reach the coordinator without the proof, so what remains is protecting arbitrary Runner construction | **Hardening**, owner: PR7/PR12 implementer. Raised by the second independent final confirmation alongside `PR4-CONF-003`, which was the defect half of the same area and was repaired in round 5 |

## 5. Fixed — recorded so recurrence is visible

A fixed finding is not a closed subject. It is recorded here with the guard that now prevents it, so
a later reviewer can tell a *new* defect from a *returning* one, and so a class that keeps coming
back is visible as a fact rather than a feeling.

| ID | Slice | What | Guard that now prevents it | Class |
|---|---|---|---|---|
| PR3-WINDOWS-VERIFIED | PR3 | Whether the fault-seam platform axis behaves correctly *at runtime* on Windows — carried as a known-unknown while this box was Linux-only, because `cfg!(windows)` is false here and both sides of every platform pin move together | **Closed by evidence, not by a fix.** Attestation of record is CI: `test (windows-latest)` on head `288194f` reports **815 passed, 0 failed, 8 ignored** (Linux 850/9; the 35-test gap is the platform-gated set), and `msrv (Rust 1.85, windows-latest)` is green at 55s. From 2026-08-18 a Windows Server 2025 KVM guest on this box also runs the suite locally via `phase9.sh`'s `win-test` gate, so Windows regressions are now catchable before a push — but the guest is the iteration loop, not the attestation. CI remains the record | host-platform unverifiable locally |
| PR3-RUNSTARTED-FIELDS | PR3/A1 | `RunStarted4` omitted `integration_ref` and `execution_root`, both named by the packet in two independent passages | reconciliation of every event's fields against the packet's named lists | omitted packet-required field |
| PR3-STRICTNESS-RECURSION | PR3/A1 | `refusals[24]` not enforced recursively — 32 of 69 types carried `deny_unknown_fields`; `Answer4`, reachable from `question_answered`, did not | unknown-field injection at every reachable object path (384 paths) | recursive strictness |
| PR3-TOPOLOGY-PREDICATE | PR3/A1 | `is_topology_schema` compared with `>=`; `fold.rs:808` gates schema-4 admission on it, so a schema-5 run would be admitted | domain widened past the adjacent pair | bounded grid |
| PR3-UPGRADE-DOMAIN | PR3/A1 | upgrade-totality grid crossed destinations only to 6, so a guard bounded at 6 passed all 669 tests | grid extended past the implementation-chosen bound | bounded grid |
| PR3-SELF-ORACLE | PR3/A1 | completeness grid computed its expected contract/kind relation by calling `RunnerContract::kind()` — oracle and result moved together | expected values from the packet's text or an independent table | self-oracle |
| PR3-WIRE-PINNING | PR3/A1 | every serialization test consumed self-produced canonical JSON, so any symmetric rename survived | encoding pinned against independently written payloads | encoding compared to itself |
| PR3-FOLD-001..006 | PR3/A2 | six fold defects: blank committed lines skipped rather than refused; `max_defers` off-by-one; `binding_override` never checked against the frozen `HumanBinding`; `attempt_interrupted` leaving a generation open against `T-ATTEMPT`; `CandidatePrepared` unbound to the successful attempt; a second candidate silently overwriting the first | per-finding witnesses, each finding's own surviving mutation now dying | fold identity and refusal |
| PR3-BLOCKED-TRANSITIVE | PR3/repair2 | `blocked_tasks` walked the task list once in key order on "keys refer only backwards" — true for repairs, false for plan-ordered originals | fixed-point iteration; three-task chain witness | found while writing a witness for another finding |
| PR3-ST07-001..005 | PR3/A3 | five framework defects where the shipped implementation *was* a withheld catalogue mutation | each entry re-measured KILLED against the repaired tree | framework self-reference |
| PR4-CI-ENVIRONMENT-ASSUMPTIONS | PR4 | Three tests asserted an environmental property rather than the behaviour they named, and passed on every machine this box has. `the_legacy_engine_routes_every_process_through_the_runner` compared two `PathBuf`s — macOS symlinks `/var`→`/private/var`, Windows CI returned the **8.3 short name** `RUNNER~1`, and the separators differed. `kill_tree_settles_the_whole_unix_group_before_it_returns` counted bytes from a non-blocking read instead of draining to EOF, so **one byte of anything on the child's stderr** read as a live writer forever — macOS emits such a byte and Linux does not. `host_shell_probe_…_fails_when_shell_missing` hid `pwsh.exe` by emptying the *child's* `PATH`, but `CreateProcess` searches the **parent's**, and the guest passed it only because it has no `pwsh` installed — **the right answer for the wrong reason** | `util::same_path` asks the filesystem rather than comparing strings, and every path-equality assertion in the slice goes through it; the pipe oracle drains to EOF; the missing-shell case no longer depends on what is installed. **CI attests all three at `4bb996ca4c1a77137f49978624b0f9881fd8df6e`: ubuntu 959/0/14, windows 933/0/16, macos 954/0/13** | environment assumption in a test |
| PR4-CONTRACT-NAMED-PROOF-TEST-DELETED | PR4 | A slice deleted one of its own contract-named proof tests and nothing local noticed. `slice_contract.proof_tests[8]` names an identifier verbatim; a repair round renamed it while fixing a genuinely invalid oracle, and the orchestrator's twelve gates, three CI platforms and count checks all stayed green because none of them read the contract | **A gate, not a test.** `phase9.sh` now reads `decisions.pr_sequence[N].slice_contract.proof_tests` from the frozen packet, treats each entry whose first token is a snake_case identifier as an obligation, and fails if any is absent from `src/`. Prose entries ("environment composition fixtures") are skipped, and the gate prints **how many it checked and how many it skipped** so a silently-empty check is impossible — a zero-checked run fails. A slice that deletes or renames one of its own proof obligations now fails locally rather than at a frontier review | contract obligation unenforced by any gate |
| PR4-MACOS-IS-MEASURABLE | PR4 | The Windows catalogue recorded `PR4-WIN-056` and `-075` as unmeasurable because *"CI adds no macOS runner"*. **That was false** — `ci.yml` has run `os: [windows-latest, ubuntu-latest, macos-latest]` in both matrices throughout — and the belief cost a real defect: the macOS-only `kill_tree` failure sat undetected through six repair rounds and three independent confirmations, and was found by the first CI push | **Closed by evidence, not by a fix.** macOS is a measured platform on every push. A future slice must not record a macOS property as unmeasurable; `os_matrix` states the reaper invariant for **all Unix**, and CI can hold it | platform wrongly believed unmeasurable |
| PR5-C-DOCTEST-FIXTURES-NEVER-RAN | PR5/C | `expected_failures_refusals[9]` opens with "a schema-4 append outside the Event funnel **does not compile**", and lane C discharged it with three `compile_fail` doctests carrying error codes (`src/events/log.rs:265` E0616, `:278` E0308, `:1021` E0451). **`cargo test --all-targets` does not run doctests** — `--all-targets` is `--lib --bins --tests --benches --examples` and the doc target is not among them — and `.github/workflows/ci.yml:52` runs exactly that command. Every gate this project runs was green on three fixtures that had never executed once. This is strictly worse than the failure the contract warns about ("green whether it failed for the intended reason or a typo"): the fixtures were green for **no** reason | `events::log::tests::every_declared_build_refusal_fails_for_the_reason_it_declares` reads the fenced blocks **out of the doc comments** (so the executed and documented fixtures are one text, and cannot drift) and compiles each with `rustc` against the crate's own rlib inside the lib test target. It pins the reason three ways a bare "it did not build" cannot: a **positive control** must compile first, so a mis-wired `--extern` cannot make every fixture "refuse"; each fixture's emitted **set** of `error[EXXXX]` codes must equal exactly `{declared}`, so a typo (E0425/E0432/E0599) fails; and the **count** is pinned at three with three distinct codes. **General to the project, not to this lane**: any `compile_fail` fixture added anywhere is invisible to CI unless something in a run target compiles it | a fixture no gate executes |
| PR5-C-APPEND-SITE-GRID | PR5/C | The contract names three schema-4 append sites — `Event.AppendFirst`, `Event.Append`, `Event.AppendInformational`. Every grid drove `Event.Append` (and the legacy site). `AppendFirst` and `AppendInformational` appeared **only in refusal cells**, refused because the line's kind did not match, never exercised as accepting sites. `if matches!(site, EventSite::Append \| EventSite::LegacyAppend)` around the point consults in `write_committed` passed the entire suite | `append_site_lines()` builds a line of each site's own kind — a real `RunStarted4`, a `defer_wait_elapsed`, a `pool_exhausted` — keyed against `TOPOLOGY_APPEND_SITES` so a site added later has no line and says so. Four tests cross every site, and the point grid asserts each coordinate is offered **under its own site's name**. Witness: the mutation above now fails `every_append_point_is_offered_in_every_mode_the_frozen_inventory_declares` with `` `Event.AppendFirst` never offered `Written` in Kill mode `` | a guarantee proved for the variant that was looked at |
| PR5-C-FOLD-PATH-UNCENSUSED | PR5/C | `INV-02`'s stable-prefix portion makes the barrier "the **only** fold source for a topology write command", and nothing asserted it. A second, barrier-free `pub fn fold_without_barrier(path, inputs)` beside `establish_stable_prefix` passed every test | `events::log::tests::the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold`: a crate-wide census requiring `TopologyFold::replay(` and `TopologyFold::parse_log(` to appear **exactly once** in production, both inside `establish_stable_prefix`. It carries its own control (`TopologyFold` is named in the production half of exactly three files), because a census whose regions collapse counts zero and reads as "nobody does this" | invariant stated in prose, asserted nowhere |
| PR5-C-KILL-MODE-NEVER-EXECUTED | PR5/C | `effect_site_inventory.scope` requires every parent-side sub-effect point to be "observed **executed** at least once by the suite in every injection mode the point supports", and `fault_injection_registry.structure` tables kill entries for `Written` (two shapes), `Synced`, `Create`, `TruncateTornTail` and `SyncPrefix`. Lane C had asserted only that the funnel *offers* those coordinates. No test had ever let one fire | A subprocess helper (`events::log::tests::event_funnel_kill_helper`, the idiom `src/agent/proc.rs` already uses) and three tests: `every_kill_point_the_inventory_declares_has_a_case_and_no_case_is_invented` derives the point set from `EventSite::ALL × sub_effects() × modes()` and pins six cells over five points; `a_kill_at_each_open_point_leaves_the_shape_the_packet_tables`; `a_kill_at_each_append_point_leaves_the_shape_the_packet_tables`. The child's death is checked, not assumed — not `success()`, no `panicked at` on stderr, and on Unix `signal() == SIGABRT` | "supports injection" proved as reachability, never as execution |
| PR5-C-PRODUCTION-SOURCES-HANDLIST | PR5/C | `runner::tests::production_sources()` cut each file at its first **inline** `#[cfg(test)]` and then excluded exactly one whole-file test module **by name** (`src/engine/tests.rs`). A file the crate declares as `#[cfg(test)] mod tests;` has no inline marker to cut at, so the whole of it counted as *production*. The moment lane C added `src/events/log/tests.rs` and `premove.rs`, `every_production_process_start_is_classified` and `every_production_command_spec_payload_is_classified` both failed — and had they instead been *silently* satisfied, two censuses whose whole purpose is "every production process start is classified" would have been measuring test code | `whole_file_test_modules()` derives the exclusion set from the `#[cfg(test)] mod <name>;` declarations themselves, with a control assertion that `src/engine/tests.rs` is in the derived set (a derivation that found nothing would silently count every test file as production — the failure it replaces). Witness: making the derivation return an empty set fails four `runner::tests` | a census exclusion maintained by hand |

### What the recurrence data says so far

- **Fixture/oracle defects recur across slices** — 11 in PR2, 11 again in PR3/A1, in different code
  by different authors. That is a property of the method, and it is why hostility is now asserted as
  distinct-value **counts** and why a function may not be its own oracle.
- **Omission does not recur — it had never been looked for.** Mutation witnessing cannot detect a
  field that was never written, so no previous slice would have caught it either. The guard is a
  reconciliation table, not a better test.
- **Nine of PR3's fourteen second-round findings were predicted before the code existed** by withheld
  mutation catalogues. Authoring a catalogue and not measuring it is spend with no yield; measuring
  one and not triaging the survivors is worse.

## 6. PR5 lane A (WorkspaceManager primitives and the Object group) — found and fixed in-slice

Recorded here rather than only in `pr5/A-report.md`, because a finding named in a report and not
carried into this file is a finding lost — PR4's round 6 named a `.cmd` gap in its own prose,
dropped it, and the next reviewer found it.

| ID | What | How it was found | Guard that now prevents it | Class |
|---|---|---|---|---|
| PR5A-ADD-WITHOUT-INTENT | `WorkspaceManager::add_worktree` did not require the slot's durable intent to exist. `slice_contract.invariants_introduced[1]` is "worktree and snapshot intents synced **before** the add", but `WriteIntent` and `Add` are separate sites — each carries its own hooks, and the cancellation clause is stated per clause — so no single funnel body ordered them and the ordering was a caller's obligation nothing checked. A schema-4 caller in PR7–PR10 dropping the `write_intent` call would have got a successful add and a worktree invisible to `reclaim_intents`, which walks intents: the exact leak `enforcement_domains.external_physical` writes the intent to prevent ("a durable per-owner recovery record in its row, reclaimed at process start (never 'empty')") | own-code audit of the contract's `invariants_introduced` against the funnel bodies, before any witness was written | `Refusal::AddWithoutIntent` — the add funnel refuses when `intent_path(slot)` is not a file, so the ordering is a property of the primitive rather than of its callers. `workspace_manager::tests::an_add_without_a_durable_intent_refuses_and_leaves_nothing_registered` covers all three add sites (`Worktree.Add`, `Worktree.AddStaging`, `Snapshot.Add`, asserted as three distinct `add_site()` values), proves nothing is created or registered on refusal, and then proves the *reason*: with the guard, `reclaim_intents` finds every worktree the manager created | invariant stated in the contract, enforced by nobody |
| PR5A-SLOT-VALIDATION-ONE-SITE | `Slot::validate` — the containment refusal for slot names — ran in `write_intent`, `intents` and `add_worktree` only. `Slot`'s fields are `pub`, so the name is caller data at every entry point: `candidate_stage`, `candidate_write_tree`, `proposal_cherry_pick`, `repair_materialize`, `verify_worktree`, `remove_worktree`, `remove_intent` and `changed_paths` each turned the slot into a working directory and ran `git add -A`, `git write-tree`, `git cherry-pick` or `git diff` in it. A key carrying separators and `..` puts that working directory outside the execution root | own-code audit; the existing test `a_slot_name_that_could_escape_the_root_refuses` varied the hostile **name** across six values and held the **primitive** fixed at one — the `bounded_grid` shape recorded three times on this project | a private `slot_target` helper validates before returning any slot path, and every slot-taking primitive goes through it. `workspace_manager::tests::every_slot_taking_primitive_refuses_a_hostile_slot_name` crosses 8 distinct escape mechanisms (asserted as a distinct-value count, one per mechanism, including a Windows `\` separator a POSIX-only check misses) against a primitive list **derived by scanning this module's own signatures** for `pub fn`s taking `slot: &Slot` and returning a `Result`, so a new slot-taking primitive with no arm fails the test by name | bounded grid (varied the value, fixed the axis) |
| PR5A-STAGE-LOCK-UNTESTED | `after_reference_present` treats a surviving `index.lock` as proof that `git add -A` did not publish its blobs, and the whole suite stayed green with that check deleted. The fixture's `Internal` state for `Object.CandidateStage` left the edit **unstaged**, so the unstaged-changes half of the after phase already answered "absent" and the lock was never the discriminator. The state that distinguishes them is reachable: a second `git add` killed on an already-clean worktree | mutation witness `stage_lock_discriminator`, re-measured against the **whole** suite after it survived its own named test — a survivor triaged rather than filed | the `CandidateStage` arm of `observed_three_classes` now stages through the real funnel *before* planting the lock, so `index.lock` is the only thing making the state `Internal`. Re-measured: the mutation is killed | confounded discriminators in a fixture |
| PR5A-FORCED-REMOVAL-NAME-OVERCLAIMED | `forced_removal_clears_every_administrative_residue_and_is_idempotent` planted six residue files from a hand-written array and omitted the `locked` marker Git holds for the whole of an interrupted `worktree add` — the one element that *blocks* reclaim, since `git worktree prune` skips a locked entry. Deleting `remove_worktree`'s clearing of it left that test green; four other tests killed it, so the suite held, but the test's own name claimed coverage it did not have | mutation witness `forced_removal_lock`, which survived its named filter and was then re-measured against the whole suite | the element list is now `ResidueElement::ALL` — PR3's frozen enum — matched exhaustively, with the two object classes explicitly skipped as R27 rather than administrative, and the planted count asserted at `ALL.len() - 2`. A new element in the frozen enum fails to compile here | enumeration written by hand instead of derived from the type |

## 7. PR5 lane D (the compile-time enforcement layer) — found and fixed in-slice

Recorded here rather than only in `pr5/D-report.md`, because a finding named in a report and not
carried into this file is a finding lost — PR4's round 6 named a `.cmd` gap in its own prose, dropped
it, and the next reviewer found it.

| ID | What | How it was found | Guard that now prevents it | Class |
|---|---|---|---|---|
| PR5D-VISIBILITY-CHECK-DUPLICATED | `effects::externally_reachable_fns` decides the **domain** of the wrapper classification, so a function it cannot see is a function nobody has to classify. Its visibility test was written **twice** — once for the bare `pub` / `pub(crate)` / `pub(super)` case and once inside the modifier-stripping fallback for `pub const fn` / `pub unsafe fn` — and breaking the `pub(crate)` arm of the first copy left the **whole 1077-test suite green**, because the second copy still caught it. Two hand-maintained lists of three strings disagree eventually, and the one that disagreed silently would have been this one: a `pub(crate) fn` that stopped being seen would silently leave the classification domain, and `mechanism` (3)'s "every pubfn … is classified" would be true of a domain nobody drew | this lane's own mutation run: `the-parser-misses-pub-crate` **survived** the whole suite and was then triaged rather than filed as "probably covered" | one `declares_visibility` helper, called once (`src/effects.rs`). The mutation now fails `effects::tests::the_reachable_fn_parser_finds_each_shape_this_tree_uses`, which asserts the parser's answer over seven accepted shapes and three refused ones, as a written-out `Vec` rather than a count | a hand-maintained list kept in two places |
| PR5D-CI-COMPONENT-CENSUS-COMMENT-ORACLE | `effects::tests::the_workflow_that_runs_these_tests_installs_the_compiler_they_need` is the test that answers *which command runs the build-refusal fixtures?* — the rule adopted from `PR5-C-DOCTEST-FIXTURES-NEVER-RAN`. It asserted that the `test` job's YAML **contains the substring `clippy`**. The `components: clippy` line it was checking carries a nine-line comment saying why the component is there, and that comment contains the word — so **deleting the line left the test green**, and the fixtures would have stopped running in CI with the test that exists to prevent exactly that still passing | this lane's own mutation run: `ci-stops-installing-clippy` **survived** | the YAML's comments are stripped before the census (`#` to end of line), the strip is asserted to have removed something, a **control** asserts the strip removed the ledger id the comment names, and the assertion is now on a line that both starts with `components:` and contains `clippy`. Measured: the mutation now dies | `PR4-CENSUS-COMMENT-ORACLE`, **third occurrence** — and the first in a test whose whole subject is a comment-bearing config file |
| PR5D-UNRESOLVED-DENIAL-IS-A-WARNING | **A `disallowed-methods` path that does not resolve enforces nothing, and no gate this project runs can tell.** Measured on clippy 0.1.97: an unresolvable path produces a bare `warning: \`std::fs::wrrite\` does not refer to a reachable function` which **`-D warnings` does not escalate** — the gate exits 0. A path whose *crate* is not linked (every `windows_sys::` entry, on a Unix host) produces **no diagnostic at all**. So a typo anywhere in an 87-entry denylist would silently delete a denial, and the two lists that can never be checked on one host are the platform-specific ones | asked *which command checks this?* of the artifact rather than of the code, before writing any of it | `effects::tests::every_denied_path_this_host_can_resolve_does_resolve` strips every `allow-invalid` from a copy of `clippy.toml`, runs `clippy-driver` over a probe linked against `upstroke` **and every dependency rlib** — so the `libc::` and `upstroke::` entries are really resolved rather than silently skipped — and asserts the unresolvable set equals exactly the declared host-conditional set, with a **control** that injects `std::fs::wrrite` and requires the notice to appear. `allow-invalid` is spent on exactly three entries, asserted as a written-out set. The Windows half is covered by `every_platform_conditional_denial_names_something_real` and by a measured `cargo clippy --target x86_64-pc-windows-msvc` run in which **nine of the twelve `windows_sys` denials fire on real code**. Measured: `denylist-typoes-a-path` dies | an enforcement artifact no gate validates |

### The withheld-mutation measurement

29 mutations, each an exact single-occurrence replacement asserted to have applied (a mutation that
does not apply is a **failed** witness, not a skip). Driver `pr5/mutate-D.py`, logs
`pr5/logs/D/mutations/`. **29 of 29 killed, 0 survivors, 0 vacuous**, after the two survivors above
were repaired and three anchors were corrected — including one that had to be corrected *for the
reason it died*: placing an `#![allow(…)]` after the first item is `error: expected outer attribute`,
so the mutation was dying on a syntax error rather than on the placement scan. "Green whether it
failed for the intended reason or a typo", one level up, inside the witness itself.

## 8. PR5 lane D — carried, with an owner

| ID | What | Owner | Why it is open |
|---|---|---|---|
| PR5D-FUNNEL-RETURNS-A-COMMAND | `runner::host::build_command` is `pub(crate)` and **returns a `std::process::Command`** to the rest of the crate. `decisions.effect_site_inventory.mechanism` (2) reviews each funnel module "to perform effects only inside site-taking APIs **and never to return writable handles**", and `src/runner/host.rs` is in that list by name. A `Command` is the writable handle for R22 | PR6/PR7 implementer (the slice that owns `src/runner/**`) | **A live passage the current shape fails, and therefore a defect by the boundary rule — but not one PR5 may repair.** `src/runner/**` is frozen under the owner ruling of 2026-08-20, and the repair is architectural: `agent::proc` and `agent::bin` consume the `Command` `build_command` hands out, so removing it means moving spawn construction inside the funnel. **The mitigation that is available is taken**: `upstroke::runner::host::build_command` is on the denylist, so every caller must be an allowlisted module — which forced `src/agent/bin.rs` into the enumerated legacy section, where it is visible as debt rather than invisible as convenience. The allowlist entry for `src/runner/host.rs` states the residual rather than claiming the clause is satisfied |
| PR5D-PROCESS-FUNNEL-TAKES-NO-SITE | `decisions.effect_site_inventory.identity`: "**every effectful funnel API takes its group's site by value**, and the funnel itself calls hook(Before, site) -> primitive -> hook(After, site)". PR4's process funnel does neither: `HostRunner::run` threads a `SpawnHooks` observer and consults the eight containment sub-effect points by name, and `ProcessSite` appears in the production half of the tree **nowhere** — `Process.Spawn` and `Process.Terminate` are the only two claimed sites in the inventory that no funnel names. Measured by `effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`, which accepts *both* shapes the other lanes built (the variant literal and the site-as-parameter) and still finds these two | PR6/PR7 implementer | **A shape gap, not a coverage one, and the distinction is load-bearing.** The hooks fire, and PR4's grids drive all eight containment points on both platforms under witness and under fault (`runner::host::tests::every_role_reaches_the_containment_points_of_this_platform`, `a_fault_armed_at_any_containment_point_stops_any_role`). What is missing is the site *travelling with the call*, which is what makes `effect_sites.json`'s `module` column true of `Process.*`. `src/runner/**` is frozen; the repair is PR4's funnel signature |
| PR5D-ROW-MAPPING-REFUSAL-UNFIXTURED | `expected_failures_refusals[7]` is "a site without a row mapping **fails to compile**", and there is **no fixture in the tree for it**. The refusal is structural and real — `EffectSiteId::row()` and each group's `row()` are `const fn` matches over their own variants with no wildcard, so a variant added without a row is `error[E0004]` — but nothing executes that claim, and the other four build refusals each have a fixture that pins its reason | PR6/PR7 implementer (the slice that next edits `src/topology/effects.rs`) | **Cannot be fixtured from here.** The fixture has to add a variant to a frozen enum in a frozen file, which the owner ruling of 2026-08-20 forbids; a fixture that added the variant in a *separate* crate would be testing its own enum, not this one. Recorded rather than claimed: `reconciliation-D.md` §B says "not a fixture, and this row says so" instead of pointing at a test that does not exist. One line for whichever slice next opens that file |
| PR5D-MSVC-CLIPPY-NEVER-RUN | **`cargo clippy` has never run against the Windows target on this project**, so every `#[cfg(windows)]` line in the crate is unlinted. `ci.yml`'s `lint` job runs on `ubuntu-latest` only; the local gate set runs `cargo check --target x86_64-pc-windows-msvc` — `check`, not `clippy`. Running `cargo clippy --target x86_64-pc-windows-msvc --all-targets --all-features -- -D warnings` from this box found two things at once: a real `disallowed_types` violation in `src/main.rs`'s `#[cfg(windows)]` test region (repaired in-slice by widening that file's recorded allow — the union-over-platforms case the allowlist header predicts), and a **pre-existing** `error: items after a test module` at `src/agent/proc.rs:1097` that no gate has ever seen | project owner / PR6–PR7 implementer | **The `main.rs` half is repaired; the `proc.rs` half is not, deliberately.** Clearing `clippy::items_after_test_module` means moving ~250 lines of Windows-only code above an inline `mod tests` in `src/agent/**` — a reordering with no behavioural content, in a file this slice does not own, that would put a large diff in a lane whose `production_effect` is "none in behavior". **Adding the gate is therefore also deferred**, because a gate that fails on arrival is not a gate. What this slice does instead is *measure* it and record the numbers: with the unrelated lint suppressed, the three denial lints are **clean** on the msvc target and **nine of the twelve `windows_sys` denials fire on real code** — which is the evidence the Windows half of the denylist is not decorative. **CLOSED by repair round 3 (`PR5-CONF-014`).** Both halves it deferred are done: `src/agent/proc.rs`'s `items after a test module` is cleared by moving that `#[cfg(test)] mod tests` to the end of `mod windows_job` (a pure reordering, no behavioural content, and `cargo fmt` clean), and `ci.yml` gained a `lint (windows)` job running `cargo clippy --all-targets --all-features -- -D warnings` on `windows-latest`, required by `merge-gate`. Verified on the Windows Server 2025 guest: clippy rc=0 on the repaired tree. `effects::tests::a_windows_runner_runs_the_effect_denial_gate_and_the_merge_gate_requires_it` pins the job, its `needs` entry and its place in the aggregate's required-gate loop, and each of the three was witnessed dying to its own mutation. The **macOS** half of the same hole is new row `PR5-MACOS-CLIPPY-NEVER-RUN` |
| PR5D-TOOLBOX-DISCARDS-CLIPPY-OUTPUT | **`~/bin/upstroke-build` silently discards the stderr of every command it runs.** Line 85 is `exec {slotfd}>"$lock" 2>/dev/null`; an `exec` with only redirections applies them to the *current shell*, so `2>/dev/null` permanently rebinds the wrapper's stderr, and the `exec`ed cargo inherits it. `upstroke-build cargo clippy … > log 2>&1` therefore produces an **empty log**: the exit code survives, every diagnostic is lost. The same is true of `cargo +1.85.0 check` and of `cargo test`'s compile errors. The evidence it already left: `pr5/gates-merged/clippy.log`, `fmt.log` and `msrv.log` are all **0 bytes**, and this lane's first two builds reported `exit=101` with a zero-byte log | project owner (the box's tooling, not the tree) | **Not a defect in the tree and not repairable in it.** Recorded because the ledger is the union of what has been learned, and because the failure mode is the one this project keeps paying for: a gate whose *result* is trustworthy and whose *evidence* is empty. The workaround used throughout this lane is `--message-format=json`, which puts diagnostics on **stdout**, or `CARGO_TARGET_DIR=<the private pool slot> cargo …` run directly. A one-character fix exists (`exec {slotfd}>"$lock" 2>&-` is not it; the redirection wants to be scoped to the `exec` alone, e.g. by testing the lock in a subshell) but the file is outside this repository |
| PR5D-PROOF-TESTS-COUNT | `prompts/reconciliation-obligation.md` §C says "the contract's `proof_tests`, **all nine**". `decisions.pr_sequence[6].slice_contract.proof_tests` has **ten**. `reconciliation-A.md` §C repeats the nine, and `A-report.md` §7 says "none of this lane's **nine** `proof_tests` entries begins with a snake_case identifier". The tenth is `proof_tests[9]`, the Event-funnel row — lane C's | recorded, no owner needed | **Trivial and recorded anyway**, because the obligation file's own rule is "if this file's list and the packet's disagree, **the packet wins and you say so**", and an undercount nobody re-derived is how `PR3-RUNSTARTED-FIELDS` shipped. `reconciliation-D.md` §C carries ten rows |

## 9. PR5 repair round 1 — the nineteen findings of the three-lens review

Three independent lenses read a `.git`-stripped snapshot of the merged PR5 tree and returned 20
findings with zero preferences; two independent skeptics judged every one, prompted to kill it and
defaulting to refuted; one was refuted by the orchestrator against the packet. The **19** below are
what survived, and all 19 are repaired here.

**Most were test-sufficiency findings, and the distinction was kept.** "After the surviving mutation,
X happens" is not "the code is wrong today" — it is "the suite cannot tell". **Fifteen of the nineteen
are repaired with tests alone and no production edit.** Of the other four: **two are production
defects** — `PR5-CORRECTNESS-002` (a valid `run_started` past the cap hidden from every reader) and
`PR5-CORRECTNESS-005` (a detected rename's old endpoint missing from the region) — and **two took a
behaviour-identical seam** so the arm under test could be reached at all: `proc::memoised_outcome`
(`-010`) and `codex::locate_in` (`-008`). Every row below says which it is. Every row below names the
finding's **own** mutation and the test that now dies under it; the measurement is
`pr5/logs/repair1/mutations/` (driver `mutate-r1.py`, **20 mutations: 19 killed, 1 inert by design,
0 survivors, 0 build failures** — a clean full run against the final tree).

| ID | Severity | Location | What the finding was | Repair | Mutation → the test that now dies |
|---|---|---|---|---|---|
| PR5-CORRECTNESS-002 | medium | `src/rundir.rs:895` | **A production defect.** `FIRST_LINE_CAP` was a *classification* bound: a valid newline-terminated `run_started` of 1,048,577 bytes classified `Husk`, so every reader hid a committed run. `sequential_substrate.startup_census` defines `Committed` from "its first newline-terminated line is a valid `run_started`" and states **no size exception**; the boundary test derived both padded fixtures from the constant, so it moved with it | The bound is now a **performance** constant, renamed `FIRST_LINE_WINDOW`. `first_line` tries the window, and on a miss scans for the newline in a fixed 64 KiB buffer (`newline_offset_from`) before re-reading exactly the line it found. A file with **no** newline is still bounded — in 64 KiB rather than 1 MiB, strictly better than before — and a long valid line is no longer hidden | `1 << 20` → `1 << 19` is now **inert**, which is the claim: `C002-window-halved` SURVIVED and that is the expected result. The *defect* is witnessed by `C002-window-is-a-cap-again` (the fall-back returns `None`), which kills `classification_does_not_depend_on_the_probe_window`, `a_complete_first_line_with_no_terminator_is_a_husk_at_every_length` and `the_probe_returns_the_lines_exact_bytes_on_both_paths` |
| PR5-CORRECTNESS-003 | high | `src/workspace_manager.rs:545` | Test sufficiency. `refuse_unreal_directory` is the **only** check either leaf gets — `reparse_point_below` walks components *under* its anchor and `canonical_prefix` resolves a link rather than refusing it — and every existing fixture planted its link *below* the private root, where `refuse_reparse_points` catches it | `a_managed_base_or_private_root_that_is_itself_a_link_refuses_before_any_effect` drives **all three** call sites (derive/base, derive/private-root, revalidate/base, the last by replacing an already-derived base with a link to itself) on both platforms via a `plant_directory_link` helper that makes a POSIX symlink or a `mklink /J` junction, and asserts the premise both ways: `symlink_metadata` says reparse point, `metadata` says real directory | `fs::symlink_metadata` → `fs::metadata` (`C003-follow-the-link`) kills it |
| PR5-CORRECTNESS-004 | critical | `src/runner/policy.rs:170` | Test sufficiency. Two Container policies differing only by `codex -> creds_a` vs `creds-a` could produce one `runner_policy_sha256`, so a marker digested from the first and an `owner.json` holding the second would pass `prove_private_half_ownership`'s digest conjunct. INV-23 makes the record "execution identity", compared exactly | Two tests. `field_writes_its_values_bytes_and_transforms_nothing` pins `f(s) = <byte-length>:<bytes>;` written from the module's own grammar over 20 hostile values — one assertion that kills *any* transformation inside `field`. `a_normalisable_difference_in_any_string_position_moves_the_digest` crosses **11** normalisations against **all five** string positions (reference, image id, image digest, volume key, volume value) = 55 cells | `value.replace('_', "-")` (`C004-underscore-to-hyphen`) kills both |
| PR5-SEAMS-002 | high | `src/runner/policy.rs:170` | Same site, different normalisation: `value.trim()` collapses `creds` and `creds  ` | Same two tests; the whitespace pairs are three of the eleven | `let value = value.trim();` (`S002-trim-the-field`) kills both |
| PR5-CORRECTNESS-005 | high | `src/workspace_manager.rs:2083` | **A production defect.** `changed_paths` ran `git diff --cached --name-only -z base`. Rename detection is Git's **default** since 2.9, and `--name-only` prints a detected rename's destination alone — **measured on git 2.43**: staging `src/auth.rs -> archive/auth.rs` printed `archive/auth.rs` and nothing else. `path_policy.actual` requires "`--name-status`; **both rename endpoints**", and the missing old endpoint is the one another owner may hold a lease on, so two overlapping edits could be admitted at once | The invocation is `--name-status -M -z` (`-M` explicit, so the records do not depend on the operator's `diff.renames`), and `decode_changed_paths` parses the record grammar: a status field, then one path or — for `R`/`C` — two. An unrecognised status is `PathSet::RepoWide`, never a shorter list, which is also what a reversion to `--name-only` now produces. `every_change_kind_reaches_the_region_including_both_rename_endpoints` drives real Git over four change kinds and asserts Git really detected an `R100`; `both_endpoints_of_a_rename_or_copy_record_reach_the_region` and `an_unparsable_status_record_is_repo_wide_and_never_shorter` (7 shapes) hold the decoder | `--diff-filter=AM` (`C005-diff-filter-AM`) and the reversion to `--name-only` (`C005-name-only-again`) both die |
| PR5-CORRECTNESS-006 | high | `src/events/log.rs:621` | Test sufficiency. The legacy differential **normalises every `ts`** before comparing files and compares returned *bodies* only, so a moved writer stamping `1970-01-01T00:00:00Z` was invisible — while `status` renders that field and `export` copies it into attempt timestamps | `the_legacy_append_stamps_the_clocks_answer_at_every_entry_point` asserts the returned **and persisted** `ts` lies between two clock reads bracketing the append, at both legacy entry points (`append`, `append_hooked`), with a control that this machine's clock does not itself read as the epoch. The differential now checks the same window for **both** writers | `event.ts = "1970-01-01T00:00:00Z"` (`C006-epoch-timestamp`) kills it and the differential |
| PR5-SEAMS-003 | medium | `src/events/log.rs:621` | The same mutation from the seams lens | The same repair | The same witness |
| PR5-CORRECTNESS-007 | high | `src/gates.rs:217` | Test sufficiency. `expected_failures_refusals[2]`: a spawn failure is "returned error; **no halting settlement is synthesized**". No erroring Runner had ever reached the Gate role — every gate test runs a real `HostRunner` | `a_gate_whose_process_never_ran_returns_the_error_and_synthesizes_nothing` drives a `ScriptedRunner` returning the `UpstrokeError::Agent{"failed to spawn …"}` that `agent::proc` really produces, through **both** layers: `ShellGate::check` returns it, and `run_all` propagates it rather than returning `Ok(Some(GateFailure))`, stops after the first gate, and writes no evidence file | the `Err(error) => Ok(GateResult::Fail)` match (`C007-spawn-error-is-a-fail`) kills it |
| PR5-CORRECTNESS-011 | high | `src/gates.rs:233` | Test sufficiency, same seam: a gate whose child exceeded the capture bound but exited 0 (`code: Some(0), output_limited: true`) authorising the task. Gate tests covered pass/fail/timeout only, on a real runner that cannot produce `output_limited` on demand | `a_shell_gate_maps_every_supervision_result_the_way_the_contract_says`: 3 exit codes × both flags = **12** cells, expectations written as **literals per row** rather than re-derived from the branch order — because the two rows that matter are exactly the ones a re-derivation would get wrong in the same direction (`Some(0)` with a flag set). 1 pass, 11 fails, asserted as counts, plus the request really being Gate-role and unbound | deleting the `output_limited` block (`C011-output-limited-gate-passes`) kills it |
| PR5-CORRECTNESS-008 | high | `src/agent/codex.rs:466` | Test sufficiency. `every_preflight_process_has_its_own_ordinal` asserts the hand-written table `probe_ordinal::ALL`, which holds only the **declared** ordinals; the six strict-config probes' ordinals are **computed**. `Resume => Fresh.index()` left six processes carrying three identities, against `invocation_identity`'s "unique per process" and INV-20 | Stop asking the table, ask the requests. `the_six_config_parser_probes_are_six_distinct_identities` drives `validate_effort_config_key` against a `RecordingRunner` that answers each surface the way a working `codex` does, and asserts 6 requests, 6 distinct identities, 3 of them on the resumed surface, all inside the table's reserved block. The sibling computed site gets `every_binary_resolution_candidate_carries_its_own_identity`, driven through a new `locate_in(runner, cache, names)` seam so it neither spends the process's memoised resolution nor depends on `codex` being installed; the premise (≥2 candidates on this machine) is asserted, never skipped. `Invocation::at` is the test-only constructor that makes both possible | `Self::Resume => Self::Fresh.index()` (`C008-resume-reuses-fresh-ordinals`) kills it |
| PR5-CORRECTNESS-009 | high | `src/runner/host.rs:528` | Test sufficiency. `supplies_credentials` names **three** roles; the actual-child parity test held **one pair** (`Probe(Agent(claude-code))` vs `Implement`). Stripping the binding for `Review` alone left every child-level comparison green, so pre-flight would certify a credential location the spending process does not have (DESIGN.md:258-264) | `every_credential_supplied_role_composes_one_environment_per_binding` takes its domain from `ExecutionRole::all()` filtered by `supplies_credentials`, crosses it with **all three** `CREDENTIAL_LOCATIONS` bindings (9 children), and asserts three sentinels — base-only, overlay-only, and the credential key itself, which only the binding can restore because composition strips reserved keys — before asserting the three environments are equal | the `Review`-filtered binding (`C009-review-loses-its-binding`) kills it |
| PR5-CORRECTNESS-012 | high | `src/runner/host.rs:550` | Test sufficiency. `invariants_preserved[0]` is "output capture … unchanged". **Every** grid in `host.rs` sends `args = ["--exact", NO_SUCH_TEST]`, so the *argument vector* was an axis nobody varied — `PR4-CONF-006`'s class one field further over — and no grid inspected the output at all | `the_runner_returns_the_childs_whole_output_for_every_production_request_shape`: 16 cells over three axes — role (built by production's builder), agent binding (all three), and each adapter's **real** argument vector from its own `pub fn build_args`, fresh and resumed, so `exec`, `-p` and the bare-prompt form all appear. A shim emits three JSONL lines and two stderr lines; the assertion is byte equality per stream, with "the *first* line survived" named separately because "the last line survived" is what a truncating runner would still pass | the codex/`exec`/Implement-or-Review truncation (`C012-codex-stdout-truncated`) kills it |
| PR5-CORRECTNESS-013 | high | `src/agent/codex.rs:934` | Test sufficiency. Claude and Copilot had direct flag-to-status parser tests; Codex's `output_limited` fixtures exercised its strict-config *preflight validators*, so no test had ever parsed an output-limited Codex execution. A truncated, supervisor-terminated transcript could authorise the task | `agent::tests::every_adapter_maps_every_supervision_result_the_same_way`: domain from `ADAPTERS`, 6 supervision shapes × 3 adapters = **18** cells, expectations as literals, and the stdout is a **success** payload in each adapter's own answer shape — so a parser that ignored the flags would report `Completed` rather than failing for the wrong reason | deleting the `output_limited` block (`C013-codex-output-limited-ignored`) kills it |
| PR5-CORRECTNESS-014 | medium | `src/agent/codex.rs:940` | The same gap one branch down: a timed-out Codex worker reported `AgentError` instead of `Timeout`, which is a distinct ladder input | The same grid; two of its six shapes are timeouts | deleting the `timed_out` block (`C014-codex-timeout-ignored`) kills it |
| PR5-CORRECTNESS-015 | high | `src/events/log.rs:626` | Test sufficiency. DESIGN.md:406: "it applies the event **as it will be read back** rather than as constructed". Every body in the differential and every append fixture was **lossless over the wire**, so returning the constructed event instead of the round-tripped one was invisible | A lossy body — `attempt_finished` with `duration = 1,500,123 µs`, which `duration_ms` writes as `1500` — is now the third of the differential's four bodies, and `the_legacy_append_returns_the_event_a_replay_of_this_log_yields` asserts the returned event equals what `crate::events::read_all` — the **reader**, not this writer — produces, at both entry points | `Ok(written)` → `Ok(event)` (`C015-return-the-constructed-event`) kills both |
| PR5-SEAMS-004 | low | `src/events/log.rs:403` | Test sufficiency. The differential open grid varies the log's **bytes**, and a failing open is a property of the **path** — so all thirteen shapes open successfully and the legacy error *variant* was unasserted. `UpstrokeError::Io` carries the `std::io::Error` a caller can match `kind()` on; `UpstrokeError::EventLog` carries a string and loses it | `a_legacy_open_that_fails_fails_the_way_the_pre_move_writer_did` varies the path over four failing shapes (absent parent, path is a directory, read-only file, read-only file with a torn tail), takes its expectation from the **oracle** (`std::mem::discriminant` equality with `PremoveEventLog`), asserts positively that the oracle's variant is `Io` so a both-sides mutation is still caught, compares the rendered errors with the two directories folded away, and requires ≥2 cells to have really failed — a machine that can open all four is recorded, never silently skipped | the `UpstrokeError::EventLog` mapper (`S004-legacy-open-error-variant`) kills it |
| PR5-FIDELITY-001 | medium | `src/agent/bin.rs:95` | Test sufficiency, in two places. `every_production_command_spec_payload_is_classified` counted `.stdin(` and `.env(` **method calls**, so the two spec *constructors*' struct-literal `env: Vec::new()` was a production payload site the census could not see; and no test compared a probe's overlay with a work command's | The census grows a third column counting struct-literal `env:`/`stdin:` initialisers (with the comment strip asserted to have removed something), and `src/agent/bin.rs` and `src/gates.rs` become enumerated rows. `a_command_specs_payload_does_not_depend_on_its_arguments` then says what those sites *produce*: over 13 of production's own argument vectors — every adapter's `--version`, every adapter's `build_args` fresh and resumed, Codex's strict-config shape — `Invocation::spec`'s payload is one value, and `ShellKind::spec`'s is empty across all five dialects | the `--version`-keyed overlay (`F001-probe-only-overlay`) kills it |
| PR5-SEAMS-001 | high | `src/agent/proc.rs:119` | Test sufficiency. `effect_site_inventory.scope` requires every point "observed **executed** … in every injection mode the point supports", and **every** containment point declares `Kill`. Nothing had ever let one fire: the reach tests arm nothing and the fault grid injects `Injection::Error`, deliberately, because an abort would take the test binary with it | `a_kill_armed_at_any_containment_point_actually_kills` — the sibling of the events lane's kill grid, and the same idiom: a subprocess helper (`spawn_funnel_kill_helper`, the one new ignored entry on Linux) over `per_spawn_points()`, with the child's death **checked** — not a clean exit, no `panicked at` on stderr, and on Unix `signal() == SIGABRT` | `Injection::Kill => Ok(())` (`S001-kill-does-not-kill`) kills it |
| PR5-CORRECTNESS-010 | high | `src/agent/proc.rs:1042` | Test sufficiency **plus a seam that had to exist**. `crash_reconstruction` forbids a degraded mode; `AMBIENT`'s memoised `Err` arm was unreachable in any test, because a process that memoised a failure never gets a coordinator and one that memoised a success can never fail. `Err(_) => Ok(())` there left `contain_write_command` minting `Contained` with no ambient job | The decision moved out of the Windows-only value and into `proc::memoised_outcome<T>`, which **every platform compiles and Linux can test** — a decision only one platform can test is a decision one platform never tests. `a_memoised_establishment_failure_reaches_every_later_caller` runs everywhere and asserts the memoised diagnostic comes back verbatim. The end-to-end half is Windows-only and deliberate, as `PR4-CONF-005`'s is: `poisoned_ambient_helper` seeds `AMBIENT` with an `Err` in a subprocess and asserts `join_ambient_job`, `contain_write_command` (with `containment_establishments()` unmoved) and `start_write_command` all refuse | `Err(_message) => Ok(())` (`C010-memoised-error-becomes-ok`) kills the platform-independent test on Linux |

### What the new code could get wrong, and what catches it

The guard adopted after `PR1-ORDER-001-ABA` and `PR3-ST07-011/-012`, applied to the three production
changes this round makes:

| Change | What it could get wrong | What catches it |
|---|---|---|
| `rundir::first_line`'s two-pass probe | An off-by-one in the fall-back's newline offset — one byte short truncates the closing brace, one byte long splices the newline into the JSON, and **both refuse on the parse**, so `Husk` would look like a correct answer for the wrong reason | `the_probe_returns_the_lines_exact_bytes_on_both_paths` asserts the returned **bytes**, not the verdict, on the window path and the scan path, with a second event after the first line so "read to EOF" and "read to the newline" are different answers |
| the same | The no-newline case regressing to an unbounded read — the reason the window existed | `a_log_with_no_newline_at_all_is_a_husk_however_long_it_is` drives 16 windows of newline-free bytes through the classifier, and the scan is a fixed `SCAN_CHUNK` stack buffer that cannot grow |
| `decode_changed_paths`'s record grammar | Reading a bare path as a status field and returning a **shorter** region — which is what it now sees if the invocation ever reverts to `--name-only` | `an_unparsable_status_record_is_repo_wide_and_never_shorter`'s first cell is exactly that input, and repo-wide overlaps everything, so the unparsable direction refuses rather than admits |
| `proc::memoised_outcome` | Being bypassed — a later edit could `match` in `join_ambient` again and leave the helper dead | It is `pub(crate)` with one production caller, classified `effect_free` in `effects/wrappers.toml`, and on Windows the end-to-end helper asserts the refusal reaches `contain_write_command` rather than only the helper |

### Two process notes this round leaves behind

* **`PR4-CENSUS-COMMENT-ORACLE`, fourth occurrence, in the safe direction.** A doc comment added to
  `codex::locate_in` mentioning `run_with_timeout_hooked` broke
  `runner::tests::every_production_process_start_is_classified`, which counts literal occurrences and
  does **not** strip comments. It failed loudly rather than passing, so it cost a rewording rather
  than a hole — but it is the fourth time, and the guard the ledger already adopted (strip comments,
  assert the strip removed something) is not yet applied to that census. Filed in §2.
* **A `#[cfg(test)]` item placed among production items silently shrinks the wrapper-classification
  domain.** `effects::production_region` cuts a file at its **first** `#[cfg(test)]`, so adding
  `Invocation::at` inside `impl Invocation` took five of `src/agent/bin.rs`'s functions out of the
  domain `mechanism` (3) is asserted over. Measured, not theorised — the test named them as
  "invented". The constructor now lives in a `#[cfg(test)] impl` block below every production item,
  with the reason on it.


## 10. PR5 repair round 2 — the repair-diff review and the 48 catalogue survivors

Two independent bodies of evidence: a `max`-effort review scoped to **what round 1 changed**
(`pr5/review-repair-diff.json`), and the re-measurement of all 210 withheld catalogue entries against
the repaired tree, with the 59 survivors re-measured and 48 still surviving in nine named causes
(`pr5/remeasure-survivors.json.md`). Full round report: `pr5/repair2-report.md`.

Counts, measured rather than quoted, summed across all three test binaries:
**Linux 1128 / 0 / 21** (1120 lib + 8 bin), **Windows guest 1098 / 0 / 24** (1088 + 10), from
1099 / 0 / 20 and 1072 / 0 / 23. One new ignored entry on each platform, the same one:
`rundir::tests::endless_log_classification_helper`.

### The two repair-diff findings

| ID | Severity | What it was | Repair | Mutation → the test that now dies |
|---|---|---|---|---|
| PR5-RD-001 | medium | **A production defect round 1 introduced.** Removing `FIRST_LINE_CAP` as a classification bound was right — it hid committed runs over 1 MiB — but the replacement never terminated. `newline_offset_from` looped until a read returned `Ok(0)`, which `/dev/zero` never does, so a public run directory whose `events.jsonl` is a symlink to one was never classified and the write command held the worktree lock for ever, against `startup_census`'s requirement that **every** entry be classified before a write command proceeds. Round 1's report claimed "a log with no newline is now bounded at 64 KiB"; that bounded one stack buffer while the loop ran to EOF | **The read is bounded, never the answer.** `first_line` takes its budget from `fstat` on the handle it is about to read — the file's own length — so a regular file is read in full however large it is and a device, fifo or socket declares zero and is a `Husk`. Termination is now a property of the loop: every branch that reads spends at least one byte of a finite budget, and the one branch that spends nothing (`Interrupted`) is named in the doc and is not something a regular file produces. The same non-termination in `std::fs::read` is fixed in the Event lane too — all four log reads in `src/events/log.rs` go through `util::read_file_bounded` | an unconditional `read_to_end` in `first_line` kills `a_run_directory_whose_log_never_ends_is_still_classified` (a real `/dev/zero`, in a subprocess, on a 20 s deadline); a `newline_offset_from` that carries its budget and never spends it kills `the_first_line_probe_spends_its_budget_and_stops`, which asserts the probe read **exactly** the budget and runs on the Windows guest too |
| PR5-RD-002 | high | The kill grid's domain was a hand-written list. `per_spawn_points()`'s Windows branch named `CreatedSuspended`, `PrivateJobAssigned` and `Resumed` and omitted `Spawn.AmbientJobJoined`; the helper also ran `start_write_command(&mut NoHooks)` **before** installing `KillAtPoint`, so the one call that reaches the ambient join could not receive a kill. `effect_site_inventory.scope` requires every point observed executed in every mode it supports and `SubEffectPoint::modes` gives the ambient join both — and it had executed in Kill mode **zero** times across six guest runs while round 1's report claimed the grid covered it | The domain is now **derived from the frozen enum**: `containment_points()` reads `Process.Spawn`'s own `sub_effects()` and each point's own `platform()`, so a point added later is covered by construction. `per_spawn_points()` is that set minus `STARTUP_POINTS`, and `the_startup_and_per_spawn_domains_partition_this_platforms_points` asserts the two partition it. The helper arms a startup point **on the startup call**. `KillAtPoint` became mode-aware, which is a defect the repair would otherwise have introduced: `point_mode` defaults to `point`, and the ambient join is consulted at two coordinates, so a mode-blind hook would have aborted at the *error-return* coordinate — before there is a handle to close — and the grid would have passed while witnessing a coordinate the packet does not name | **Witnessed on the guest**, which is where it had to be. `proc.rs:646` mutated to consult `point_mode(AmbientJobJoined, Kill)`, discard the answer and return `Ok` fails `a_kill_armed_at_any_containment_point_actually_kills` on Windows Server 2025 with `AmbientJobJoined: the helper exited cleanly, so the kill never fired` |

Round 1's report was also wrong that `-M` makes records independent of `diff.renameLimit` (it does
not; no repair follows, because the conservative D+A output still retains both paths) and that
`a_log_with_no_newline_at_all_is_a_husk_however_long_it_is` catches an unbounded read (it cannot —
one finite regular file reaches EOF under every implementation, including the one that never
returned). That test's doc comment now says so itself.

### The 48 survivors, by cause

**38 repaired, 3 ruled not-a-defect, 7 carried into §2** (38 + 3 + 7 = 48). The nine `target-absent` entries were
`NOT_PRESENT` rather than `SURVIVED` and are ruled separately in the round report; eight are
not-a-defect (two structurally impossible by design, one covered a layer down by DefId, five outside
PR5's scope by the packet's own words) and one — `PR5-WORKSPACE-048` — is carried in §2.

| Cause | Entries | Ruling |
|---|---|---|
| `no-sync-ledger` | 3 | **Repaired.** `util::DurabilityLedger`, reached through a defaulted `durability_ledger()` on `EffectHooks` and `RunDirHooks`, gives the workspace and run-directory lanes the instrument the Event lane already had. Each sync is fused with its ledger entry, and the **rename** is in the trace because the claims are orderings. The residual boundary — deleting `sync_all` *inside* the fused helper — is stated, not claimed closed |
| `event-ledger-too-narrow` | 7 | **Repaired.** The same ledger on `EventHooks`, covering the append's `write_all`/`flush`/`sync_data` and the open's truncation, which `synced` never saw. `PR5-EVENTS-044` needed a **real** primitive failure and now has one: `/dev/full`, which is only openable because of `PR5-RD-001`'s bounded read |
| `correlation-never-broken` | 3 | **Repaired.** HEAD is moved off the recorded value before each primitive runs, and the tests assert the two readings really differ in that fixture |
| `unreachable-behind-an-earlier-guard` | 3 | **Ruled one by one, and the group's shared cause holds for one of the three.** `-028` (`--no-deref`) is genuinely unreachable behind `refuse_symbolic` — **not a defect** — but the guard's own coverage drove two of the three primitives it protects and now drives all three. `-030` and `-031` are **real gaps, repaired**: neither needs a guard bypassed, only a third SHA substituted before a CAS, and a symbolic ref that resolves to the expected object |
| `the-assertion-exists-and-its-oracle-leaks` | 6 | **Repaired**, and this is the project's dominant defect class across five slices. A `position()` first-match over a **first-observation** log (worse than the review knew: the second occurrence is not recorded at all, so a mark would not have worked either) → a fresh harness with the count asserted; a marker recording `/nowhere` → two shapes whose marker names the private half beside them; an `After` hook that fires once the directory is gone → a real failed removal partway through; and `to_string().contains(point.name())` satisfied by a scratch path named after the point → a message accessor, point-free scratch names, and backtick-quoted matching because `Written` is a prefix of `WrittenFull` |
| `equivalent-mutants` | 2 | **NOT A DEFECT, verified in the code rather than accepted.** Conjunct 2 (`rundir.rs:1446`) and conjunct 3 (`:1454`) have already forced `marker.run_id == basename` and `marker.repo_key == repo_key` before the owner disagreements are built at `:1517`, so the substitution is a no-op on every input, error message included. The controls settle coverage in the other direction: deleting either conjunct fails `every_conjunct_of_the_ownership_proof_refuses_on_its_own` |
| `the-distinguishing-shape-is-never-built` | 23 | **16 repaired, 7 carried** (`PR5-R2-WIN-NON-SURROGATE-REPARSE`, `-SNAPSHOT-INPUT-COMMIT-DEAD` ×2, `-IDUNREAD-BEFORE-THE-PARSE`, `-WORKTREE-LOCK-RETENTION`, `-LEGACY-ENGINE-APPEND-FAILURE` ×2 in §2). Each carried one needs something this round cannot honestly build: a reparse tag the guest's test user cannot create, a caller that does not exist, a `git commit-tree` that prints a malformed id, a paused run, or an append injector reachable from inside a live `Run` |
| `a-row-mapping-that-compiles-away` | 1 | **Repaired.** `effects::tests::no_site_enums_row_mapping_has_a_wildcard_arm` — a source census over the frozen inventory's production region asserting no `row()` body carries a `_ =>` arm, with the number of bodies scanned asserted so a census pointed at the wrong file fails rather than passes vacuously |

### What this round's new code could get wrong

Thirteen mutations, each restored from a byte copy with the restore verified by sha256 and the number
of tests that actually ran asserted — a filter matching nothing exits 0 and reads exactly like a
survivor. **Twelve died; one survived and that is the correct result** (the `first_line` mutation run
against a test that measures `first_line_within`), reported rather than dropped.

Two hazards are recorded rather than only fixed:

* **`KillAtPoint` would have witnessed the wrong coordinate.** Arming a two-coordinate point without
  a mode gate aborts at the earlier coordinate and the grid still passes. That is the same shape of
  false witness as the omitted point itself, one layer in, and it is why the guest run rather than
  the Linux run is the evidence for `PR5-RD-002`.
* **A new test placed between an existing test's `#[test]` and its `fn` left a duplicated attribute.**
  Linux was green — the lint is warn-by-default — the test was registered **twice**, and the Linux
  count read one higher than it should have. The **guest** failed the build under `-D warnings`.
  Fixed, and the corrected count is the one reported. Fourth consecutive round in which the platform
  nobody looked at held the defect.

## 11. PR5 — a frozen file changed, and why that is not a breach of the ruling

`src/topology/registry.rs` is modified by this slice (**+56 / −15**). `src/topology/**` is slice PR3's
code, and the owner ruled on 2026-08-20 that **the frozen files stay frozen**. So this is recorded here
rather than left for a reviewer to find and spend a finding on.

**It is not the shape the ruling was made about.** The ruling answers a slice that wants to *redesign*
what it implements — the two accepted deviations in §1 both wanted a frozen production change to make a
repair possible. This is the opposite: a change PR5 could not avoid without violating the packet.

Measured, by restoring the frozen version of that one file and running CI's own gate
(`cargo clippy --all-targets --all-features -- -D warnings`):

| | |
|---|---|
| gate result | **fails, rc 101**, four `disallowed_methods` errors |
| sites | `registry.rs:3371` `create_dir_all`, `:3372` `write`, `:3378` `write`, `:3396` `remove_dir_all` |
| the escape the packet forbids | `decisions.effect_site_inventory.mechanism` (2): the legacy allowlist *"never contains a topology module (src/topology/\*\*, src/runner/\*\*, src/workspace_manager.rs, src/engine/topology.rs)"* |
| scope of the change | **test fixture only** — the three hunks begin at 3359 / 3433 / 3443; `#[cfg(test)]` is at line **898** |

So lane D turning on the packet-required denial made a *pre-existing* fixture uncompilable, the packet
forecloses the allowlist escape by name, and routing the fixture through the funnels was the only
conforming option. Production code in that file is untouched.

**Recorded as a forced consequence, not a deviation, and not carried debt.** If a later reviewer finds
that the fixture could have been left alone, relocated, or written another way — or that something
outside `#[cfg(test)]` in fact changed — that is new evidence and overturns this entry.

### A process note: a review is only as fresh as its snapshot

The repair-diff review read a snapshot taken at 10:55 and finished at 11:41; the repair it informed
landed at 13:00. That sequencing was correct — it reviewed round 1 *for* round 2 — but nothing in the
driver distinguished it from the failure mode where a confirmation reads code the last repair already
replaced. The S11 driver now fingerprints the snapshot against the live tree and **refuses** on a
mismatch. Cheap, and this project has already lost one max-effort review to a head that moved.

## 12. A pre-existing flake, measured rather than described

`agent::proc::tests::pid_directed_termination_kills_a_suspended_tree_without_continue` failed once
during repair round 3, in one of roughly 25 full-suite runs, and in no run before or after it — neither
final platform run, nor the twelve mutation measurements around it.

**Measured after the round**: six further consecutive full-suite runs on Linux, all
**1140 / 0 / 21, rc=0**. So the observed rate is **one failure in ~31 runs (~3%)**, not zero and not
common.

**It is not this slice's.** The only change PR5 makes to `src/agent/proc.rs` is a reordering — moving a
`#[cfg(test)] mod tests` to the end of `mod windows_job` to clear `clippy::items_after_test_module`,
which `PR5-CONF-014`'s new Windows clippy gate required. That reorder was verified to be **pure**: the
sorted line multiset of the file is byte-identical before and after (`fad0db6f…089f7` both sides), at
the same 7245 lines, with zero differing lines. Source order does not determine test execution order,
so the move cannot reach a process-timing test's behaviour.

**Carried, not repaired, and here is the consequence to plan for.** A 3% per-run failure gives a
meaningful chance of an intermittent red on any given CI run, and CI runs the suite on three platforms.
A red on this test after a push is **this flake until proven otherwise** — check the failing test name
before treating it as a regression, and re-run rather than repairing forward. Owner: the slice that
next opens `src/agent/**`; it is legacy supervision code that PR5's `production_effect` ("none in
behavior") does not touch.

This project has shipped a one-in-six flake before that CI attested green three times, which is why the
rate is written down here as a number instead of as "occasionally".

## 13. PR5 repair round 7 — reverted, and why the defect it fixed is safer than the fix

Round 7 repaired `PR5-RD-002`: a kill inside `git worktree add`'s registration leaves
`.git/worktrees/<slot>/commondir` **zero-length**, Git treats a zero-length read as a failed one, and
`git worktree list --porcelain -z` then fails with `fatal: … : Success` — `strerror(0)`, an errno never
set — taking down the **whole** enumeration. `remove_worktree` errored instead of converging. Measured
1 in 18 clean-tree runs. Real, and correctly diagnosed: an *absent* `commondir` is rc=0 because Git
falls back to the default common directory, so the file whose content is semantically identical to its
own absence is the one that is fatal.

**The repair was reverted whole. It bought convergence by weakening a packet-required refusal.**

Round 7 introduced:

```rust
fn enumerated_worktree_paths(&self) -> Result<Vec<PathBuf>, UpstrokeError> {
    match self.worktree_records() {
        Ok(records) => Ok(records.into_iter().map(|r| r.path).collect()),
        Err(_) => self.registration_worktree_paths(),   // silent fallback
    }
}
```

`revalidate` runs its **containment check** over that list. The `Err(_)` arm swallows *any* enumeration
failure and substitutes a directory scan that **skips entries it cannot read** — absent, zero-length, or
non-UTF-8 `gitdir`. So containment can be checked against a list shorter than Git's, and an execution
root inside an omitted worktree passes. That is
`expected_failures_refusals[1]` — *"root inside a repository worktree or worktree inside root"* — made
silently weaker. `contained()` does not restore it: it proves the target is under `execution_root`, never
that `execution_root` was legitimate.

**The direction decides it.** The defect fails **closed** — `remove_worktree` returns `Err` and deletes
nothing. The repair fails **open** — recursive deletion of the slot checkout, removal of a registration's
`locked` file, and repository-global `git worktree prune`, on an authorization it can no longer
establish. A cross-family state-space review put it exactly: *"an earlier `Err` becomes destructive
progress."* One of its five newly reachable states is an execution root inside an omitted repository
worktree, where create/reclaim/delete can write or delete **inside the user's own checkout**.

A 1-in-18 test flake is not worth a path that can delete outside the authorized root.

### Carried, with owners

* **`PR5-RD-002` — recovery does not converge on a zero-length `commondir`.** Live passages:
  `proof_tests[8]` (*"every observed residue … recovers"*), `cancellation` (*"a
  registered-but-unpopulated worktree is pruned"*), `proof_tests[1]`. Closing it requires restoring
  containment authorization *before* widening what removal proceeds on — the two must be solved
  together, which is why round 7's ordering failed. Reverting is not a repair; the residue still does
  not converge, it merely fails safe.

  **Owner clause restated 2026-08-27, and it is now explicit rather than file-triggered.** It read
  *"the slice that next opens `src/workspace_manager.rs`"*. PR7 opens that file — **+491/−22** against
  its integration base — so the clause fired, and it fired on **incidental contact**: this slice's
  contribution to it is `commit_tree_sha`, a 31-line read-only derivation added for the candidate-tree
  repair, with no workspace-lifecycle work in it at all. A clause that any edit satisfies names an
  owner who did not choose the work, and recording PR7 as an owner-who-declined would leave the row
  dangling with a name on it.

  > **Owner: the slice that next changes the worktree removal or residue-recovery path in
  > `src/workspace_manager.rs`.** Touching the file is not the trigger; changing that path is.
  > **A repair requires a macOS reproduction path first** — see the occurrence below, where the
  > platform's own `strerror(0)` rendering differs from the one this ledger recorded. Like every open
  > row, it is re-ruled at the G2-gate full-ledger audit.

  Carried as a **rated platform-residue row**, beside `PR7-WIN-READ-RACING-BOUND-TOO-SHORT` and
  `PR7-MACOS-PROCESS-GROUP-FLAKE`. The three share a shape: real production behaviour, reachable only
  under load or a kill, measured rather than described, and repaired by a slice that owns the
  subsystem rather than by the slice that happened to be red.

  **Occurrence, 2026-08-27, `327cce3`, `test (macos-latest)`** — the first observed on CI for this
  branch:

  ```
  workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered FAILED
  panicked at src/workspace_manager.rs:9530: forced removal converges: Git { message:
    "git worktree list --porcelain -z failed …: fatal: failed to read
     .git/worktrees/kalpha-g0/commondir: Undefined error: 0" }
  ```

  **`Undefined error: 0` is macOS's `strerror(0)`.** This ledger recorded the glibc rendering,
  `Success`, and §13's recognition guide tells a reader to match on that word — so the macOS
  occurrence of this row does not match the string the row tells you to look for. Both are errno 0 on
  a read that returned no bytes, which is the actual signature.

  **Rate.** PR5 measured **1 in 18** clean-tree runs, sampled locally on Linux. On CI this is the
  first occurrence in **41** concluded runs of this branch's CI workflow, each of which ran a macOS
  leg; the other four concluded failures were three of `PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE`
  (fixed in PR7) and one of `PR7-MACOS-PROCESS-GROUP-FLAKE`. Re-running the failed job at the same sha
  was green, and **that is why the rate is written down here**: a re-run replaces the run's conclusion,
  so `gh run list` no longer shows this failure at all. A rate not recorded when observed is a rate
  destroyed by the re-run that clears it.
### If one of these fires before G2, this is what it looks like

**Symptom.** A write command (`run`, `resume`) fails at start, in `reclaim_intents`. Git's own
enumeration is what breaks first, so the error names a registration file rather than anything Upstroke
owns — `fatal: failed to read .git/worktrees/<slot>/commondir: Success` is the observed shape on
glibc, and `Success` is `strerror(0)` rather than a real errno. **On macOS the same errno
renders as `Undefined error: 0`** — observed 2026-08-27 at `327cce3`, and recorded because a
reader matching on the word `Success` would not recognise this row on the platform it fired on.
The signature is errno 0 on a read that returned no bytes, not either string. Every production call site propagates with `?`
(`src/workspace_manager.rs:1433`, `:1816`); **nothing panics and nothing is deleted**, so the repository
is intact and the failure is a refusal, not damage.

**Manual recovery.** Remove the affected registration directory — `rm -rf
<common-git-dir>/worktrees/<slot>` — and re-run. `git worktree prune` alone may not clear it, because a
`locked` file left by the same interrupted registration is exactly what prune skips.

**How to recognise it is this and not a regression.** The residue is only reachable by a kill landing
inside `git worktree add`'s registration window, so it follows a crash or interrupt rather than a clean
run, and the slot is one that was mid-creation. If a *clean* run produces it, that is new and is not
this row.

* **`PR5-RD-003` and the other uncovered neighbours.** A kill in that registration window can also leave
  `gitdir` absent, zero-length, partial, or containing valid **non-UTF-8** Unix path bytes — where both
  scans use `read_to_string` and silently skip the entry, so the residue does not converge even with
  round 7's fix applied. Nine neighbours were examined; **three are uncovered.** The round-7 review ruled
  these **in-slice rather than a ledger row** and ruled that `PR5-RD-003` is *not* the only one. They are
  carried here only because the slice is landing; they are not settled.

### Two process notes this round leaves behind

* **A review filed this defect as unfixable on a false premise.** The round-6 review carried it as a
  ledger row because *"the owner ruling keeps those files frozen"* — but `src/workspace_manager.rs` is
  untracked and created by this slice; the freeze covers `src/topology/**` and `DESIGN.md:222` only. The
  outcome it reached was right and its reasoning was wrong, which is the combination that survives review
  unchallenged. **Check a freeze claim against the file's actual status, not against the word "frozen".**
* **Cross-family review earned its cost here, and a single family would not have.** The executing
  reviewer was measuring whether the fix *worked*; the read-only reviewer was reasoning about what the
  fix *cost*. Only the second question found this, and it needed no execution to answer — the mechanism
  is visible in the control flow. Two families, two extensions, one shared core.

## 14. A history rewrite invalidates every reviewed-SHA reference in the PR ledger

Stripping the Claude co-author trailers required rewriting three commits, which changed the SHA of
those and every commit after them. The rewrite itself was safe and verified — root trees byte-identical,
only messages changed — and the working tree was preserved through a `reset --soft`, so nothing was
lost in the repository.

**What was not anticipated: the PR body pins findings to reviewed SHAs.** `pr-policy.yml` validates
that each ledger row's reviewed SHA is available, and two of the referenced commits — `59bef93` and
`cdb1952` — were at or after the earliest rewritten commit. Both became orphaned on the remote the
moment the branch was force-pushed, and eighteen ledger rows pointed at them.

The remapping was unambiguous because the rewrite changed no content: each orphaned commit has exactly
one commit in the new history with an **identical tree**, so `59bef93 -> bc07139` and
`cdb1952 -> 1a9cb20` were derived by tree equality rather than by matching subjects, which could
collide. Every SHA in the body was then re-checked as an ancestor of the head before the edit.

**The rule.** Before force-pushing a branch that has an open PR, grep the PR body for 40-hex SHAs and
check each against the rewritten history; remap by tree identity. The failure is silent at push time —
the push succeeds, CI goes green, and only the policy gate notices, several minutes later and in a log
whose first twenty lines are the ledger table it is complaining about rather than the complaint.

Related: the same rewrite left four other worktree branches based on pre-rewrite commits, which was
foreseen and communicated. It is only the *PR-body* references that were missed, because they are data
in a place `git` does not look.

### `PR5-CAPACITY-NOT-A-TOPOLOGY-RESOURCE` — the measurement its row was waiting for

The row was filed noting the evidence to specify a permit did not exist: *"a single usage-limit event
across five slices, which is not a distribution a fault row can be written against."* PR5's final day
produced one. Recorded here so the PR11 implementer inherits numbers rather than an anecdote.

**Three exhaustion events in one working day, all of them on the Anthropic side** — a Max-20x plan
on a 5-hour rolling window. **The OpenAI provider (`codex`/`gpt-5.6-sol`) was never rate-limited or
exhausted at any point in this slice**, across four review stages including two multi-megabyte
`max`-effort runs. That asymmetry is the reason the free-lane split works: a `codex` stage costs
nothing against the ceiling that actually binds. Do not read `429`/`RateLimited` strings in the
`codex` logs as throttling — those are Upstroke's own capacity source being reviewed.

* A `max`-effort scoped review (`claude-fable-5`) was **killed mid-flight** with no verdict written. It
  held no write tools and read a static diff, so nothing was lost but the wall-clock — relaunching after the reset re-ran it from a
  clean start. **An implementation round in the same position loses its worker's context**, not its
  edits: the tree keeps the work and a hand-written resume contract recovers the rest.
* `claude -p` **exits** on exhaustion. It does not sleep and retry. Three implementation lanes stopped
  this way earlier in the slice and each needed a resume contract; none resumed itself.
* The **failure is silent at the wrapper**. A background job's exit code is its wrapper's, not the
  worker's: one review reported success while the worker had returned rc=1 with `You've hit your session
  limit`. Reading the wrapper's code instead of the worker's is how a killed review gets recorded as a
  finished one.

**Burn is dominated by effort tier and context size, not by wall-clock or worker count.** Measured the
same day: a single `claude-opus-5` worker at `xhigh` ran at roughly **0.07 %/min**; a `claude-fable-5`
reviewer at `max` at roughly **0.5 %/min** — about seven times the rate for one worker of nominally the
same shape. An orchestrator session's own overhead grows with its transcript, because every tool call
re-sends the accumulated context; a session carried across a completed slice pays for that slice on
every subsequent turn. Two estimates made from wall-clock alone that day were wrong by 6x in one
direction and then by 7x in the other, which is the argument for a permit rather than a heuristic.

**What this does not settle.** Whether a permit belongs in the frozen contract is still the open
question the row states, and `decisions.resource_accounting` still calls per-agent and per-pool limits
process-lifetime ephemeral scheduler state. Nothing here argues for amending the packet before PR11
brokers concurrency; it argues that when PR11 does, the distribution exists.

## 15. The catalogue re-measured against the shipped code

A passing suite proves the tests pass; only re-applying the catalogue proves they still **detect**. The
210-entry catalogue was measured at 10:32–10:54 on 2026-08-21, and two production-changing repair rounds
landed after it (13:00 and 16:30). This re-runs every entry that previously died — the 151
`KILLED`/`KILLED_BY_TYPES` plus the 38 survivors ruled *repaired*, **189 that must die** — against the
tree that actually shipped.

**Status: 160 of 190 measured. 152 killed. Five survivors carried below, one resolved, five
`TARGET_MOVED`, three unmeasurable. Thirty entries still running.**

### Resolved: `PR5-EVENTS-006` is not a regression

Its killing assertion is Windows-only — *"an append-only `FILE_APPEND_DATA` handle lacks
`FILE_WRITE_DATA`"* — which has no Unix analogue, so the mutation survives on Linux **by construction**.
Measured on the guest it dies: `rc=101`, `1093 passed / 11 failed`, panicking in
`a_torn_tail_is_truncated_on_open_with_a_warning_at_both_open_sites`. A Windows entry measured on Linux
proves nothing, in either direction.

### The five that need adjudication, and the question that decides them

| entry | target | was |
|---|---|---|
| `PR5-RUNDIR-030` | `prove_private_half_ownership`, the commit-record absence conjunct | KILLED |
| `PR5-EVENTS-020` | `prove_prefix_stable` equality oracle | KILLED |
| `PR5-WORKSPACE-068` | `force_remove_residue` | KILLED |
| `PR5-WORKSPACE-070` | `ResidueSamplingHarness::record_sample` | KILLED |
| `PR5-EVENTS-051` | legacy `EventLog::append` flush step | SURVIVED, then **repaired** and witnessed dying by round 2 |

**Every one of their killing assertions is still present in the tree** — the legacy I/O trace at
`src/events/log/tests.rs:1306`, the reread-instability tests at `:2594` and `:2683`,
`unreachable_objects(&fixture.base).expect("fsck")` at `src/workspace_manager.rs:6810`, and *"an
unclassifiable residue is durable state no tabled action recovers"* at `:7585`. So **no repair deleted a
test.** That leaves exactly two possibilities per entry, and they are not the same finding:

1. **The assertion was narrowed** so it no longer distinguishes the mutated behaviour — a real
   regression in detection power.
2. **The re-expressed mutation is not the original.** The catalogue records mutations as *prose*, so the
   re-measurement re-implemented each one; a differently-expressed mutation can be an **equivalent
   mutant**, unkillable by construction and never the same test. `recatsub.py`'s own header names this
   as *"the most expensive possible false positive"* for this exercise.

`PR5-RUNDIR-030` leans hard toward (2): the production check is byte-identical to catalogue time
(`fs::symlink_metadata(locator.join(COMMIT_RECORD)).is_ok()`, line 1359 then, 1564 now) and every
fixture still writes `b"{}"`, so nothing that could have dissolved it changed. The other four sit in
`events/log.rs` and the residue harness, which rounds 3–6 worked heavily, so (1) is live for those.

**Settling it is mechanical and bounded**: compare each re-expressed patcher against what the entry's
prose actually specifies, and where they agree, bisect the assertion. **Owner: G2.** Do not carry these
forward as "five regressions" — that is the claim this exercise exists to avoid making without evidence.

### Also outstanding

* **Five `TARGET_MOVED`** — `sync_surviving_prefix`, `publish_json_atomically`, `add_task_worktree`,
  `write_worktree_intent`. A repair relocated the code the mutation names; each needs re-expressing
  against the new site before it means anything. Recorded rather than counted as dead.
* **Three Windows entries never measured** — `PR5-WORKSPACE-003`, `-034`, `-059` are on the guest
  manifest and were not run. Given `PR5-EVENTS-006`, an unmeasured Windows entry is a genuine gap.
* **Three unmeasurable** — one `WONT_COMPILE`, one `NO_VERDICT`, one recorded without a diff.

### Two harness defects this run exposed

* **No timeout on `cargo test`.** `PR5-RUNDIR-069` rewrites `is_running()` to
  `return lock_file(public).exists()`, so anything waiting on a run waits for ever: the mutation **hangs**
  the suite rather than failing it. The batch then never advances and the job is killed with no verdict —
  which reads as nothing rather than as a kill. Now bounded at 900s and recorded as `KILLED_BY_HANG`,
  because a non-terminating suite *is* detection, just not the kind the parser understood. The harness
  guarded every *silent* failure — anchors asserted, restores sha256-verified, unrecognised trees refused
  — and none of those guards is reached by a hang.
* **`recat-batch.sh` ignores its second argument.** It writes to stdout and expects the caller to
  redirect. Passing a log path *and* redirecting elsewhere sends every verdict to the void while the run
  looks healthy.

### Final result — 190 of 190, and two corrections to the interim entry above

**184 of 190 still die.** 174 `KILLED` plus 10 that fail to compile — and that second group is a **kill,
not an unmeasurable**: `WONT_COMPILE` now is `KILLED_BY_TYPES` then, the same outcome under a different
label, because those mutations are caught by the type system. The interim entry above counted them as
unmeasurable, which was wrong.

**The repairs demonstrably work: 37 of the 38 survivors ruled *repaired* now die**, measured against the
code that shipped rather than against the tree they were written on. That is the strongest single result
of the exercise and it was not visible from any other instrument.

**Two entries resolved by platform, in opposite directions.** Measuring the same entry on both platforms
is what separated them:

* `PR5-EVENTS-006` — Linux `SURVIVED`, guest `KILLED`. Its assertion is about an append-only
  `FILE_APPEND_DATA` handle lacking `FILE_WRITE_DATA`, which has no Unix analogue, so surviving on Linux
  is **correct**. Not a regression.
* `PR5-WORKSPACE-003` — Linux `KILLED`, guest `SURVIVED`. The reverse: a real Windows-side survivor that
  the Linux run would have reported as fine. `WorkspaceManager::repo_key`.

Neither could have been settled on one platform, and three Windows entries were nearly left unmeasured.

### The six that need adjudication. Owner: G2.

| entry | target | was | note |
|---|---|---|---|
| `PR5-RUNDIR-030` | `prove_private_half_ownership` | KILLED | production and fixtures byte-identical to catalogue time |
| `PR5-EVENTS-020` | `prove_prefix_stable` equality oracle | KILLED | |
| `PR5-WORKSPACE-068` | `force_remove_residue` | KILLED | |
| `PR5-WORKSPACE-070` | `ResidueSamplingHarness::record_sample` | KILLED | |
| `PR5-EVENTS-051` | legacy `EventLog::append` flush step | SURVIVED | **the one repair of 38 that did not take** |
| `PR5-WORKSPACE-003` | `WorkspaceManager::repo_key` | KILLED | **Windows only** — Linux kills it |

Every one of their killing assertions is still in the tree, so no repair deleted a test. Each is either a
**narrowed assertion** (real detection loss) or an **equivalent mutant** from re-expressing prose — two
different findings, and calling them six regressions without settling which would be the mistake this
exercise exists to prevent.

### A third harness defect, and it is the same shape as the other two

A guest entry whose `win-iter` invocation fails returns `rc=6` with **no verdict**, and the batch records
`exit=0` and moves on. Three Windows entries were "measured" that way and produced nothing; the `RESULT`
line carries the `rc`, so it is visible to a reader, but a batch that reports success while measuring
nothing is how an unmeasured entry becomes a counted one. With the unbounded `cargo test` and the ignored
second argument, all three defects share one shape: **the harness guards every way of producing a wrong
number and none of the ways of producing no number at all.**

### Capacity: the constraint is a rate against a rolling window, not a volume

Observed 2026-08-22, and it changes what a permit would have to model. On the day PR5 exhausted its
5-hour window three times — killing one `max`-effort review mid-flight — the **weekly** allowance stood
at **97% remaining**. The aggregate was never close to spent.

So the resource that actually refuses work is a **rate over a short rolling window**, and the ceiling
that never binds is the long one. A permit modelled as a budget drawn down over a slice would have
reported ample capacity at every moment work was being refused. What it would have to model instead is
the window: how much has been spent in the last five hours, by whom, at what effort tier — and
`decisions.resource_accounting`'s existing framing of per-agent and per-pool limits as
*process-lifetime ephemeral scheduler state* is closer to that shape than a durable row would be.

Two practical consequences already paid for on PR5: pacing matters more than total (four concurrent
`max` reviewers exhaust a window that the same four run sequentially would not), and a worker killed by
the window is not short of budget — it is early, and the same work succeeds unchanged after the reset.

## 16. PR6 — what nine reviews and a withheld catalogue found

The container Runner slice ran four per-lane reviews and five whole-slice lenses, and measured a
**193-entry mutation catalogue** authored from the frozen packet alone before any container code existed
and withheld from every implementer. **136 of the 138 applicable entries were killed (98.6%)**; both
survivors were repaired. Nine reviews produced **71 findings**, every one carrying a mutation the
reviewer applied and measured.

This section records what generalises. The per-finding detail is in the slice's own reports.

### The one that mattered most, and why the suite could not see it

`expected_failures_refusals[5]` is *"gate write outside mount fails"*, and DESIGN.md:610 calls confining
gate-executed repository code the first thing a container uniquely buys. **A Gate could write outside
every declared mount**: Docker received the role bind mounts but no read-only root filesystem, so
`sh -c 'printf owned >/outside-role-mount'` exited **0** into the writable container layer.

The test *"explicitly permits container-layer writes and checks only host bytes."* It proves **"the host
is unharmed"** — which is true, and which the orchestrator quoted approvingly as kernel-level evidence.

> **A test can prove a true, weaker statement indefinitely while the stated guarantee is false.**

When replacing an assertion, check that the new one is *the contract's claim* and not a neighbouring true
one. Repaired by `--read-only`, with the assertion now on the write failing.

### The witnessing rule this slice had to learn

Lane F witnessed **16** mutations — more rigour than any lane on this project — and an independent
review refuted **three** of the claims they supported. Each mutation deleted the **mechanism together
with its observable**:

| mutation as written | what it proved | minimal mutation | result |
|---|---|---|---|
| delete `fsync_file` **and** its `Synced` trace record | the **record** is asserted | delete the fsync, keep the record | whole suite passes |
| `expect_site` always `Ok` | **`write_intent`'s** guard is asserted | delete only `start_container`'s | passes |
| run reclaim twice | **idempotence** | two reclaimers actually racing | not constructible in any fixture built |

> **Delete the mechanism and leave every observable in place.** If the suite still passes, the test is
> asserting the observable rather than the mechanism — the self-oracle shape wearing a witness's clothes.

This is the rule after *"a fixture that lands green having never been seen red is not coverage"*, and it
is now in the slice's `repair-common.md`.

### `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`, three times in one slice

`effects::production_region` cuts a source at its **first** `#[cfg(test)]`. In PR6 it defeated, in order:

1. **lane A's R20 census**, which concatenated the container sources and called `production_region` once,
   so the first test boundary truncated every module appended after it — while its positive control still
   fired, *on the truncated domain*;
2. **the orchestrator's own witness** of a guard repair, where a duplicate literal planted *below* a
   `#[cfg(test)]` was correctly not seen, and the conclusion "the guard did not fire" was one step from
   weakening a good guard;
3. **`PR6-ACCT-002`**, reporting the same census *still* scanning only the first file.

Lane F had warned lanes A and C about this by name in its own report. It recurred anyway.

> **A positive control proves a census can see something. It does not prove the census sees the domain it
> names.**

### One defect arrives wearing several names

Five lenses named one defect three times — `PR6-CORRECTNESS-004` = `PR6-RECOV-001`; `PR6-CONV-001` =
`PR6-CORRECTNESS-009`; `PR6-ENUM-004` = `PR6-CORRECTNESS-003`. The orchestrator partitioned repairs by
**finding id**, verified the partition was disjoint (it was), and sent **one defect to two lanes**, which
solved it two incompatible ways interleaved through one function. That diff was discarded and re-run
rather than hand-merged, because twelve interleaved regions in a 1639-line diff is how PR5's round 7
became a revert.

> **Partition repair work by the code path a finding touches, not by the identifier a lens assigned it.**

Independent lenses agreeing is the signal this process exists to produce. It also means the same defect
arrives several times under different names, and only reading the *location* tells you so.

### A bare name given to something that does not resolve bare names — twice

* **Windows**: `CommandSpec.program` carrying `claude` into `CreateProcessW`, which appends `.exe` and
  ignores `PATHEXT` — so every npm-installed agent CLI failed to spawn (`PR6D-001`).
* **Unix**: the cleanup reaper passing `docker` to `execv`, which does not search `PATH` at all — so no
  labeled container was ever reclaimed after a coordinator death (`PR6-LANEC-002`).

Different subsystems, different platforms, different reviewers, one shape. The second has a real
constraint behind it: the reaper runs post-`fork`, pre-`exec`, and `execvp` is not async-signal-safe while
`execv` is. Repaired by resolving in the parent and handing the absolute path down.

### Three defects, three platforms, one oracle each

| defect | the only place it was visible |
|---|---|
| the bare-name repair breaking npm `.cmd` CLIs | the Windows guest |
| `launch` mounting the Git view **after** `create`, so no container with a view could start | real Docker |
| `SIGSTOP` landing before the supervised worker's first write | macOS CI |

None was visible from the others and none from reading; twelve green local gates said nothing about any
of them. The first was **predicted** by a catalogue entry naming a function that did not exist
(`HostRunner::resolve_program`) and then measured on the guest.

### The test suite leaks a temp directory per fixture, and it exhausted the build box

Found while re-measuring the catalogue, when the box stopped being able to create files with **237 GB
free**. Inodes were at **100%** — 58,466,304 used, 0 available. `/tmp` held **1,639,765** `upstroke-*`
fixture directories, enough that the directory entry itself was 114 MB.

The mechanism is one line, in `src/rundir.rs`:

```rust
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("upstroke-rundir-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
```

The `remove_dir_all` runs at **creation**, not at drop, and the name is keyed by `{tag}-{pid}`. It makes
a fixture idempotent *within* one process. It never removes anything after a test, and the next test
process has a different pid and therefore a different name, so the previous run's directories are not
merely left behind — they are unreachable by that cleanup for the rest of the machine's life.

Measured on the shipping tree, immune to a concurrent cleanup by counting only entries newer than a
marker: **66 tests executed, 117 fixture directories left behind.** A full run of the current 1385-test
suite leaks on the order of 2,400. The box had accumulated 1.6 million.

`pre_existing` — the helper dates to `dc56475` (2026-08-09) and PR6 changes `src/rundir.rs` in zero
files. It is recorded here rather than repaired here because it is project-wide and outside this slice's
contract, and because the repair is a judgement call: a `Drop` guard is the obvious fix, but tests that
deliberately inspect a fixture after a panic would lose their evidence.

> **A cleanup that runs at setup is not a cleanup. It is a retry.** The distinction is invisible while
> disk is measured in bytes and fatal once it is measured in inodes.

Two operational notes for whoever picks this up. `df -h` reports the box healthy at 72% while every
write fails — only `df -i` shows it. And the failure does not announce itself as a disk problem: it
surfaced as four mutation entries aborting mid-measurement.

### Deterministic container names plus end-of-test cleanup is not enough

Two real-Docker tests went red on the shipping head — `real_docker_census_reclaims_a_dead_owner_and_
spares_a_live_one` and `real_docker_the_daemon_holds_exactly_the_specs_mounts_and_a_read_only_root` —
while the only change in that commit was a documentation edit. Both passed in isolation minutes later
under `UPSTROKE_REQUIRE_DOCKER`, so a skip would have failed.

The cause was four containers left over from an earlier run this session that had been **SIGKILLed** when
the box exhausted its inodes: two `Exited (137)`, two still `Created`. Their names were the deterministic
ones these tests recreate, so `docker create` hit a name conflict.

Both tests already clean up after themselves — a `cleanup` closure on every exit path in one, a
`LeaveNoResidue` RAII guard in the other. Both are correct, and neither can help: **no in-process
cleanup runs when the process is SIGKILLed.**

The fix is a pre-clean — reclaim the names before using them — and the idiom is only correct here because
the names are *deterministic*:

> **A pre-clean removes the previous run's residue exactly when the name recurs.** Keyed by something
> unique per process — a pid, a ULID — it can never name anything an earlier run created, and it
> degrades into an unconditional retry that cleans nothing. `src/rundir.rs`'s `scratch` is the second
> shape; these container fixtures are the first.

Recorded as **debt, not repaired in this slice.** The tests hold what they claim; robustness to SIGKILL
is a guarantee beyond their contract, the trigger was an operator-caused disk exhaustion that has been
cleared, and an unreviewed change to shared test infrastructure is how PR5's round 7 became a revert.
It should be repaired before PR7, where parallel runs make a stranger container the normal case rather
than the aftermath of a crash.

## 17. PR7 — what two review rounds found, and the two things no review was positioned to find

The `TopologyRun` slice ran four per-lane reviews on the implementation and three on the repairs
themselves, every finding carrying a mutation the reviewer applied and measured. Round 1 produced
**8 HIGH, 18 MEDIUM, 12 LOW, 6 census findings (3 CRITICAL) and 3 debt items**; round 2, whose
subject was the five repair commits, produced **1 CRITICAL, 1 HIGH, 6 MEDIUM and 5 LOW**. Severity
fell between rounds, which is the shape a converging slice has.

This section records what generalises. Per-finding detail is in the slice's own reports.

### The defects lived between the lanes, not inside them

Nine lanes each built a correct component. What the reviews found was almost entirely **duplication
across lane boundaries**: three implementations of the one append-error protocol, two publicly
reachable `BarrierHeld` constructors, two censuses of the run directory where the reclaiming one had
no production caller, and two modules implementing O24's retry rule that **disagreed** — `settle.rs`
correctly closing a `RetainedIdle` generation, `attempt.rs` destroying its cumulative tree and
recreating it at base.

Every lane's own tests passed. They had to: each lane tested the implementation it wrote.

> **A per-lane review cannot see a defect whose two halves are in two lanes.** Partitioning a review
> by code path is still right — PR6 paid 3565 discarded lines to learn that partitioning by finding
> id is not — but the partition has to be *covered by a pass that owns the seams*, or duplication is
> structurally invisible: every reviewer reads a correct implementation of the rule, and no reviewer
> reads both.

The mechanical guard adopted: **for every clause of the packet, count the implementations.** Not
"does this module implement it correctly" but "how many modules implement it at all". Two is a
finding regardless of whether both are right, because two can drift and one cannot.

### A fix that introduced a new defect — three times in one slice

`PR7-BLANKER-DESYNC` (P0), `PR7-HUSK-BRICKS-RESUME` (P1) and `PR7-RETRY-ATBASE-UNGUARDED` (P1) were
all introduced by round 1's repairs and caught by round 2. §4's class stood at 2 occurrences across
PR1–PR3; this slice alone triples it, and it is the strongest evidence the project has that **a
repair round is the most dangerous code in a slice**.

The worst of the three deserves its own note, because of *where* it landed. Repairing a census that
could be fooled by a comment meant writing a blanker, and the blanker recognised a char literal by a
fixed two-byte lookahead. `'é'` closes at `i+3`. Scanning resumed **on** its closing quote, read it
as an opening one, desynced, and the item-end matcher then overshot and **failed open by blanking to
end of file** — hiding forged code from every census, with `fmt`, `clippy -D warnings` and the whole
suite green, and a **zero-byte** region delta so no floor, per-file or aggregate, could see it.

> **When a repair's subject is an instrument, the repair inherits the instrument's blast radius.**
> The failure mode to look for is not "does it still detect" but "how does it behave when its own
> parser loses sync" — and the answer must be *fail loud in the direction of the census*, never
> return a larger region than it can account for. Both give-up paths now return `start`, not
> `bytes.len()`.

### Two tests of mine that asserted `!false`

`PR7-POISON-TEST-VACUOUS` and `PR7-BUNDLE-TEST-PARTIAL` were both instruments **the orchestrator
wrote**, and both passed while asserting nothing. The poison test's fixture was a dependency chain in
which nothing was admissible even unpoisoned, so four of its five guards were satisfied by the
fixture rather than by the poison: deleting them individually left the suite green. The bundle test
drove five hook families and asserted three.

This is the standing rule — *a green suite proves tests pass, not that they still detect* — firing on
the work of the person holding the rule. The guard is the one already recorded: **kill each guard
individually and require a distinct failure message for each**, which also proves each predicate was
independently true before the mutation.

### The two things no review was positioned to find

Both were found by asking *which command runs this?* rather than *does this pass?* — §4's rule, and
the second time on this project it has caught something a mutation catalogue structurally cannot.

**Recovery step (g) is absent.** `decisions.sequential_substrate.recovery_order` lists step (g),
"recreate `OpenNoAttempt` worktrees at their bases (through `Worktree.Verify` or forced recreate)".
`run_recovery_order` implements a0, a, a1, census, (b), (c), (f)-as-refusal, (d), (e) and (h). There
is no (g), and no comment marking its absence deliberate — unlike (b) and (f), which carry their
rationale. `dispatch::resume_open_no_attempt` is written, documented and tested, with **zero
production callers**. Two separate findings had already circled the symptom (an `OpenNoAttempt`
generation that nothing advances, at the only width production creates) without either reviewer
identifying that the packet has a step for exactly that case.

**`TopologyRun` does not exist.** The same decision says "`src/engine/topology.rs` TopologyRun drives
schema 4 at max_parallel = 1 synchronously" and "schema 4 always runs TopologyRun". `create_run`,
`run_recovery_order`, `select`, `dispatch` and `close_at_run_end` are each reachable **only from
their own tests**; nothing outside `select.rs` so much as matches on `Step`. The slice built every
component of the run and never assembled the run.

**Measured after the fact, and it is the sharpest number this slice produced.** Six withheld
catalogues were authored from the packet alone, by readers forbidden to open `src/engine/topology/`:
265 entries, every `packet_basis` resolving to a live key. **93 of them — 35% — are written against
`TopologyRun`, its loop, or the production `EventEmitter`**, naming methods like
`TopologyRun::run_fresh` and `TopologyRun::initialize_slots`. Six independent readers, none of whom
had seen the implementation, all assumed the driver existed, because the specification describes one
and nothing in the specification hints that it was skipped. A third of the catalogue is
**unapplicable** until the driver is written, which is also the cleanest available measure of how much
of the slice's obligation surface the omission accounts for.

Neither is detectable by any technique this project currently runs. A mutation catalogue measures
whether existing code is pinned; **omission has nothing to mutate.** A per-lane review reads the
lanes that exist. The 117 named tests all pass — they are per-boundary tests, and a driver that
sequences boundaries is not a boundary. Every gate is green.

> **The census that was missing is the one over the packet's own enumerations.** PR3 learned this for
> event fields — *"mutation witnessing cannot detect omission; transcription slices need a
> reconciliation table against the packet's named enumerations"* — and the lesson was applied to
> fields and never to **steps**. `recovery_order` names its steps (a0) through (i) in one sentence;
> `loop` names its branches in one sentence. Both are enumerations. Neither had a test that read the
> packet's list and asserted the implementation covers it.

The guard adopted, and it is mechanical: **a slice whose contract names an ordered sequence must
carry a test that enumerates the sequence from the packet's text and asserts one implementation per
element.** Not per-element correctness — presence. A step that is absent, or present twice, fails it.

### A process note: the reconciliation instrument needs its own control

The orchestrator's named-test checklist reported 117 of 117 present. Re-derived from the packet, the
eleven gated rows name **115 unique tests across 117 mentions**, of which **114 are present** — the
one absent is annotated *"(with T-PREPARED)"*, a row PR7 does not gate. The figures agree in
substance, and both intermediate instruments were wrong: the checklist file lost two names in
extraction, and a later re-check reported three spurious absences because its character class was
`[a-z0-9_]+` and the packet's names contain capitals that `clippy::non_snake_case` forbids in Rust.

> **A reconciliation table is a census, and every rule this project has learned about censuses
> applies to it.** Derive it from the source of truth each time rather than from a copy; give it a
> positive control; and when it reports a discrepancy, confirm the discrepancy before acting on it.

### The build slot pool poisons concurrent agents across worktrees

A reviewer measured `upstroke-build` handing it a slot whose test binary had been compiled with
`CARGO_MANIFEST_DIR` pointing at **a sibling reviewer's worktree**, while cargo reported *"Finished
in 0.01s"*. Confirmed with `strings` on the binary. Earlier in the same slice the same mechanism
produced 53 phantom failures at a green commit and **masked a real compile error**.

Isolated worktrees are not sufficient: **they isolate the source, not the target.** `upstroke-build`'s
premise — one target dir per concurrent build — holds only while the concurrent builds come from one
source path. Until it is fixed, an agent building in a worktree must touch every source file before
its first gate run and must not trust a sub-second "Finished". This entry is the record — the fix is
to key the slot on the source path as well as the slot index, and it belongs to whoever next touches
the build-box tooling.

### A review that shares a tree is not four reviews

Round 1's four reviewers mutation-tested the same checkout. One caught another's fault injection
mid-flight and reported it as a defect. Both were transient and the tree was sound, but the exposure
was real and the cost is silent: a reviewer measuring against a tree another reviewer is mutating
cannot distinguish its own control from someone else's attack. Round 2 gave every reviewer its own
worktree, which is now the rule.

### The effect-site registry describes the system slightly differently from how it behaves

Three findings this slice surfaced are the same shape, and the shape is worth naming because none of
them is a bug in the behaviour. `src/topology/**` is PR3's, and it holds the *vocabulary* the rest of
the system is checked against — effect sites, their rows, their fault rows, their adjacency, and the
classification domain the enforcement layer ranges over. When that vocabulary and the code drift, the
code is usually right and the vocabulary is usually the thing that was written once and never
re-measured:

- **`Ref.CreateIntegration`'s order axis is backwards.** The registry says the effect precedes
  `run_started`; P8 creates the ref after P6 appends it, and the slice contract says so.
- **Six modules have an empty classification domain**, so a `pub(super) fn` below a `#[cfg(test)] use`
  is reachable from a topology module and passes every gate — demonstrated, not theorised.
- **Three of the enforcement layer's own censuses could pass while scanning nothing**, which PR7
  repaired in its own files but which leaves `externally_reachable_fns` still consulting the
  truncating region.

> **A registry that is checked only against itself is a self-oracle at the scale of a subsystem.**
> `the_observable_orders_are_the_ones_the_adjacency_admits` asserts that two functions in one file
> agree; it is green for either value of the thing they agree about. Every one of these findings was
> found by comparing the registry to a **live packet sentence** or to a **running program**, and none
> by any test the tree contains.

**Owner ruling, 2026-08-24: recorded clearly, not repaired here, and revisited once v0.2 is complete.**
The reasoning is the one `ff0490a` already stands on — a slice may not quietly redesign what it
implements — extended by the observation that these are not independent one-token fixes. They share a
cause, and repairing them one slice at a time means three separate unreviewed edits to the layer every
other module's enforcement depends on, which is the shape that made PR5's round 7 a revert. Section 2
carries all three under one owner so the pass that takes them finds them together.

### The test emitter is a fourth implementation of the append path

`EventEmitter` had **one implementation in the whole tree** before PR7's driver existed, and it was
`#[cfg(test)]`: `scaffold::FoldedEmitter`. Same root cause as the missing driver — the seam was
written for a caller nobody built.

And it does not call `emit::emit`. It re-implements the append: round-trip, `plan_transition`,
`append_topology_hooked`, `apply_delta`. So it runs **none of the append-error protocol's five
obligations** — no explicit poison, no reservation cancellation, no in-flight invocation
cancellation, no reopen, no present/absent/undetermined report. Every dispatch, attempt, settle and
candidate test drives through it.

Measured rather than argued: transplanting `FoldedEmitter`'s shape into the production `RunEmitter`
leaves the fold **unpoisoned** on an armed append failure, and
`the_production_emitter_reaches_the_append_error_protocol` goes red on exactly that.

> **A test double that re-implements the thing under test is not a double, it is a second
> implementation** — and this slice found three others of the same protocol in production code. The
> production emitter is a forwarder and deliberately nothing else, so there is one implementation and
> `emit::emit` is it.

**Recorded, not repaired.** It is `#[cfg(test)]`, so no shipped behaviour is wrong; what is wrong is
that the pipeline's protocol coverage is thinner than the suite's size suggests. Routing
`FoldedEmitter` through `emit::emit` is a change to test infrastructure every topology test depends
on, which is the shape PR5's round 7 was reverted for. Owner: the slice that next touches the
scaffold.

### And the two emitters observe different funnels

`FoldedEmitter`'s `EventHooks` is a `TimelineEvents`, which records each `(site, phase)` into the
ordering timeline **and** the harness. The shared bundle's `events` family is a bare
`HarnessEventHooks`, which records only into the harness.

So an append made through the scaffold is visible to a timeline ordering assertion and an append made
through `emit::emit` is not — **two observation surfaces for one kind of event, decided by which
emitter ran.** Nothing is broken today, because each test reads the observer its own path populates;
what is not possible is writing a timeline-ordering assertion about the recovery path.

This is why `EventEmitter::emit` taking `hooks` as a parameter is not by itself a guarantee. It makes
the bundle the caller's choice — as every Git funnel in this tree already does — and
`every_family_of_the_harness_bundle_records_into_the_same_harness` is the assertion that the choice
was right. The repair is to give the shared bundle the timeline wrapper, not to take it from the
scaffold. Same owner as above.

## 18. PR7 — the legacy engine's command assembly moved, and why that is not a behaviour change

`src/engine/attempt.rs` and `src/gates.rs` are the **legacy** engine's — the path that ships today,
that `upstroke run` drives for schemas 1–3, and that PR7's contract touches only by promising not to
disturb. This slice moved code out of both. Recorded here rather than left for a reviewer to find,
the same way §11 recorded PR5's frozen-file change.

### What moved

| from | to | what |
|---|---|---|
| `engine/attempt.rs::run_attempt` | `engine::assembly::WorkerAssembly::command` | permissions → `TaskRun` → `AgentAdapter::build` → stdin payload |
| `gates.rs::ShellGate::check` | `gates::ShellGate::command` | `(shell.spec(&cmd), timeout)` |

Both call sites now delegate. Neither expression changed: same inputs, same order, same adapter
calls.

### Why

Two engines need the same answer. The legacy one assembles a command **at the moment of use**; the
schema-4 driver needs the same sets **up front**, because an `AttemptPlan` is a value it appends
`attempt_started` from. Assembling twice is this project's dominant defect class, and this slice paid
for it directly: two derivations of a task's predicted region, disagreeing on every glob, shipped
green in `199dc1d` and were repaired in `84a3978`.

> **The finding that scoped this work: minting was never duplicated.** The crate has exactly two
> production `CommandSpec` constructors — `gates::ShellKind::spec` and `agent::bin::Invocation::spec`
> — and both already document themselves as the single place. All six other mints are `#[cfg(test)]`.
> What was about to be duplicated is the **selection of their inputs**: which prompt, which
> permissions file, which timeout, which profile. So the extraction is scoped to input selection, and
> `a_command_is_assembled_in_one_production_place_per_role` is scoped to it too.

### The neutrality evidence

The contract for PR7 names **no** legacy-behaviour clause — unlike PR4's, whose
`invariants_preserved[1]` was "legacy engine behavior unchanged". What it names is
`production_effect: none (TopologyPreview selector only)`, which a change to the legacy path's
behaviour would breach just as surely. So the evidence matters more, not less, for the absence of a
clause to cite.

1. **A whole-tree census reported the move and nothing else.**
   `every_production_command_spec_payload_is_classified` counts every production call site that
   populates a `CommandSpec` payload, per file. It failed on this change with exactly one difference
   — `src/engine/attempt.rs: (1,0,0)` becoming `src/engine/assembly.rs: (1,0,0)` — and **every other
   row identical**. A move that had altered a payload would have moved a number, not a filename.
2. **The request census still holds, and was widened.**
   `every_production_runner_request_is_built_by_its_roles_builder` asserts that
   `engine/attempt.rs` and `engine/coordinator.rs` never construct a `RunnerRequest`;
   `engine/assembly.rs` is now asserted absent from it too. The command says *what* to run and the
   request says the role, the boundary and the identity — one module doing both would be a call site
   free to choose its own role, which `ExecutionRole::is_slotted` and `host::supplies_credentials`
   are derived from.
3. **The full suite: 1662 passed, 0 failed**, against 1661 before, the one addition being the new
   census itself.

### The reviewer needed no move at all — it needed a narrowing

The third role was expected to be the hard one, and the expectation was wrong in an instructive way.

`review::run_review` is already engine-agnostic. It is a `pub fn` over a caller-supplied `ReviewCx`,
a `&dyn Runner` and a `ReviewInvocations { pass, reask }` — **the caller supplies both identities**,
and the workspace, settings and reviews directories too. It returns a `ReviewOutcome` carrying the
result, the **cost**, the invocation count and the transcript path: everything `ReviewRecord` needs
and an exit code cannot give. The re-ask loop, the per-invocation prompt and the verdict parsing are
all inside it.

So the machinery was never legacy-shaped. It was **shared-capable and never shared** — the same
shape as everything else this slice has found: built, documented, and waiting for a caller nobody
wrote.

**One thing did block reuse, and it was a parameter that asked for too much.** `ReviewCx` took an
`&ir::Task` to reach three fields — `title`, `body`, `acceptance` — which `materialize_prompt` is the
only thing in that path to read. The schema-4 driver holds a `FrozenTaskSpec` from the frozen
registry and no `ir::Task` anywhere, so sharing would have meant **synthesising** one: inventing an
id, a kind and a dependency list the reviewer never reads. A conversion that fabricates fields is
free to drift from the plan it claims to represent, and improvising assembly inputs is the specific
thing this work was told not to do.

`ReviewSubject { title, body, acceptance }` is what the path reads, and `ReviewCx` now asks for that.
The same narrowing `OpenGeneration` made for the rebuild family, for the same reason, and with the
same result: the frozen-layer question disappears rather than being answered.

Preservation: the suite is **1662 / 0 before and after** — no test added, removed or changed
behaviour — and no `CommandSpec` census moved, because no mint or call site did. The effect
classifier did fire, on the new `ReviewSubject::of` being an unclassified externally-reachable fn of
a classified module; it is classified `effect_free` in the same commit, which is the enforcement
layer working rather than an obstacle.

### What is deliberately not finished here

- **The reviewer's command is still assembled in `review.rs`**, and
  `a_command_is_assembled_in_one_production_place_per_role` carries that as a **non-zero row with a
  reason** rather than an exemption. It does not extract by lifting one expression: the re-ask loop
  builds a different prompt per invocation — full prompt, `REASK_PROMPT`, or both — against a
  resumable session. It moves in its own commit, and until then the duplication is a number in a test
  rather than a sentence in a review.
- **The scaffold's worker command is still synthetic.** Its gate plan is now built through
  `ShellGate::command` — the `frozen_binding` precedent, where a fixture repeating a production
  composition kept a fifth copy alive — but re-pointing the worker needs an `AgentAdapter` in the
  shared topology scaffold, which every topology test uses. That is the change PR5's round 7 was
  reverted for, and it belongs with the commit where the driver introduces an adapter seam anyway.


## 19. PR7 S5 round 4 — eight unverified claims, corrected in the ledger

`PR7-R4-CLAIMS-UNVERIFIED` in §2 is the row; this is the evidence behind it, in the
repository rather than in a session artifact a reviewer of the pull request cannot open.

**Round 4 was five lenses over six commits of round-3 repairs** (`0cd2001..040a100`) and
nothing else. It returned 27 findings, every one inside that diff. Eight of them are not
defects in code: they are **claims written into those commits' messages and doc comments,
asserting a verified property, that are false** — each one `grep` from disproof, each
written in the same commit as the work it describes.

**The correction mechanism is this section, not a history rewrite.** The commit messages
are pushed history. This project already corrected `80a141b`'s false refutation the same
way, and the alternative — a tired session rebasing published commits — is the worse of
the two failure modes.

**The standing rule this produced**, adopted 2026-08-26 and binding on every later commit
in this repository:

> **The claims protocol.** Any commit-message or doc assertion of a *verified* property —
> "single authority", "every arm", "would have caught X", "test T asserts Y" — carries the
> command that verified it and its result beside the claim, or the claim is not made.
> Intent-language is free; verification-language pays evidence.

### The round-4 falsification table, verbatim

Reproduced from `~/tactus-artifacts/pr7/s5/r4/FALSIFICATION-TABLE.md`
(sha256 `30e2134f6f8f76f9ff265a17a593aeb17dbe40acaf9377fc519f8099d952adee`). The only
change is heading depth, so the document nests under this section:
`sed -e 's/^# /### /' -e 's/^## /#### /'`, and
`diff <(sed 's/^#\+ //' SOURCE) <(sed 's/^#\+ //' NESTED)` is empty.

### PR7 S5 round 4 — the falsification table

**Round 4's subject was six commits of round-3 repairs** (`0cd2001..040a100`), read by
the five lenses that produced the findings those commits closed. It returned **27
findings**, every one inside that diff, on a head verified green on all three legs
(Linux 1702/0, Windows guest 1651+10, CI 10/10).

`seams` 5 · `attempt` 5 · `contract` 6 · `loop` 6 · `settle` 5.
Three P1s were reached independently by three lenses each.

#### The eight claims

Each was written into a commit message or a doc comment **in the same commit as the work
it describes**, and each is one `grep` from disproof. This is the finding of the round;
the code defects below are ordinary by comparison.

| # | Claim, as written | Reality | Where |
|---|---|---|---|
| 1 | *"`an_ending_run_reaches_closure` already asserts this, and asserts it only where nothing else is live"* | **The test does not exist.** The name appears once in the whole tree — inside this doc comment. A scoping gap was described in an invented test, and the new witness's justification rests on it | `select.rs:1568` |
| 2 | the census *"asserts the property over the two construction sites — which is what actually failed, a literal `None` where the other arm named an authority"* | It inspects `attempt.rs` and `settle.rs`. The defect was `pool: None` in **`run.rs`**'s `RetryRequest`, a file the census does not read. Both inspected literals already named an authority **before** the repair. Restoring the pre-repair state leaves the whole suite green | `79cd9c8` |
| 3 | *"no driver fixture can reach the arm"*, given as the structural reason a source census was necessary | `the_retaining_incarnation_retries_in_place` (`recover/tests.rs:5488`) drives `step` twice in one process and reaches it. The behavioural witness said to be impossible was available | `79cd9c8` |
| 4 | `AttemptPlans::pool_for` exists *"so the pool rule has one production implementation"* | `capacity::pool_for` has **three** call sites in `assembly.rs` — the seam itself, the plan builder, and the reviewer profile added in the same batch. The seam method is called only from `run.rs` | `79cd9c8`, `assembly.rs:300/328/440` |
| 5 | the ending witness *"asserts it over **every** arm with that arm's precondition satisfied"* | Three of six. `Integrate`, `Backoff` and `HardBlock` are absent | `aee0432` |
| 6 | the pre-clean repair, presented as complete | `preclean_names` has two callers. `exec.rs` was scoped to the build slot; `census/tests.rs` still carries fixed `REPO_KEY_A`/`REPO_KEY_B`. **The stranger-killing path is still live there** | `aee0432` |
| 7 | the census *"would have caught the E6 stall, both findings above, and `Spend::replay`"* | `Spend::replay` is not among its eleven entries. It would not have | `cf7bdb5` |
| 8 | the pool fixture is *"named … and bound to the reviewer's agent, so a plan that inherited the implementer's pool and one that looked up the reviewer's own cannot both pass"* | The fixture's implementer and reviewer share `AGENT` (`claude-code`), so both lookups return the same pool and **both pass**. The mutation measured as "killed" died because the pool became *empty*, not wrong | `b44040a` |

#### The three confirmed code defects

Distinct from the claims: these are things the tree does wrong, all introduced by the
round-3 repairs, all measured.

1. **`expected_refs`'s census entry is satisfied by a substring collision.** In
   `workspace_manager.rs`, `expected_refs(` matches four times and **all four are
   `refuse_unexpected_refs(`**. Genuine calls: zero. So
   `every_packet_named_recovery_action_has_a_production_caller` proves one of its own
   eleven entries by accident, and the needle `format!("{name}(")` will do the same for
   any future entry whose name is another's suffix.
2. **The pre-clean fix is half-applied.** `census/tests.rs:3645` still calls
   `preclean_names` with fixed-key names, so `PR7-R3-CONTRACT-001`'s class — a helper
   that kills a concurrent run's live container by a name both runs share — remains live
   on that path.
3. **`an_ending_run_offers_no_work_from_any_arm` covers three of six arms.** `Integrate`
   is a work-offering arm and is not among them, so the guard's coverage is half what the
   witness's own name and doc assert.

Also open, and dependent on (1): the packet-clause census additionally counts
**test** callers as production ones, because `effects::production_code` blanks
`#[cfg(test)]` *items* and an out-of-line test file (`attempt/tests.rs`, zero `#[cfg(test)]`
attributes) has nothing to blank. Measured by `seams` and `attempt` independently.

#### What is NOT in doubt

Rounds 1–3 found and closed real defects, including two P0/P1 liveness bugs — the E6
promotion stall and the resumed run that forgot its spend — plus a path traversal from
plan-authored input. Those repairs are behaviourally sound and independently witnessed;
round 4 challenged the **claims about** several of their witnesses, not the underlying
fixes. The head is green on Linux, the Windows guest and CI.

#### The pattern, stated once

Prose asserted at the moment of writing became the evidence for the work it described.
The review layer caught it — round 4 did exactly what it was scoped to do — but only
because five lenses were aimed at six commits of my own repairs. Nothing earlier in the
chain checks a claim made in a commit message, and the claim is the artifact a reviewer
trusts most.

### Re-verified at `cca1276`, with the command beside each result

The table is round 4's. This is what re-running its disproofs found on the head this
correction lands at — the protocol applied to the correction itself, because a
falsification table asserting eight verified properties is exactly the artifact the
protocol exists for.

| # | Command | Result | Verdict |
|---|---|---|---|
| 1 | `grep -rn 'an_ending_run_reaches_closure' --include='*.rs' src/ \| wc -l` | `1` — the sole occurrence is `select.rs:1568`, the doc comment that cites it | **Confirmed.** The test does not exist |
| 2 | read `both_attempt_started_arms_take_their_pool_from_an_authority`'s `SITES` (`run/tests.rs:359`) | the two entries are `src/engine/topology/attempt.rs` and `src/engine/topology/settle.rs`; the repaired literal is `run.rs:1124` | **Confirmed.** The census does not read the file the defect was in |
| 3 | `grep -rn 'fn the_retaining_incarnation_retries_in_place' --include='*.rs' src/` | `recover/tests.rs:5488` | **Confirmed.** The fixture said to be impossible exists |
| 4 | `grep -n 'crate::capacity::pool_for' src/engine/assembly.rs` | lines `300`, `328`, `440` | **Confirmed.** Three call sites, not one |
| 5 | read `an_ending_run_offers_no_work_from_any_arm`'s `cases` (`select.rs:1593`) | three: continuation, ready dispatch, ready retry. `select` offers work from five arms — `Integrate`, `Retry`, `Dispatch`, `Backoff`, `HardBlock` | **Confirmed.** Three of six cases; `Integrate`, `Backoff` and `HardBlock` absent |
| 6 | `grep -rn 'preclean_names(' --include='*.rs' src/ \| grep -v 'pub(crate) fn'` | `exec.rs:6262` and `census/tests.rs:3645` | **Confirmed.** One of two callers was scoped |
| 7 | the census's `CLAUSES` (`recover/tests.rs:7138`), 11 entries | `prune_orphan_pin`, `refuse_unexpected_refs`, `expected_refs`, `complete_promotions`, `finish_promotions`, `recreate_open_no_attempt`, `settle_interrupted`, `close_retained_idle`, `ensure_recorded_integration_ref`, `refuse_unimplemented_terminals`, `resume_open_no_attempt` | **Confirmed.** `Spend::replay` is not among them |
| 8 | read `scaffold.rs:105` and `:192` | the implementer's rung-0 binding is `(claude-code, alpha-Mid-model)`; the primary reviewer's is `(claude-code, opus)`. `review::passes_for` rebinds only on **exact `(agent, model)` equality**, so it does not fire, and the pass keeps agent `claude-code` | **Confirmed.** Reviewer and implementer resolve the same pool, so both behaviours pass |

**And one place the table itself over-reached, corrected here under the same rule.**
Claim (1) of round 4's *code defects* — not of its eight claims — says the substring
collision means the census "proves one of its own eleven entries by accident". The
collision is real:

```
$ grep -rn 'expected_refs(' src/workspace_manager.rs
src/workspace_manager.rs:2045:    pub fn refuse_unexpected_refs(
src/workspace_manager.rs:5711:            .refuse_unexpected_refs(namespace, std::slice::from_ref(&mine))
src/workspace_manager.rs:5725:                .refuse_unexpected_refs(namespace, std::slice::from_ref(&mine))
src/workspace_manager.rs:5787:                .refuse_unexpected_refs(namespace, std::slice::from_ref(&mine))
```

but the entry is **not** satisfied only by it. A boundary-aware search finds a genuine
production caller:

```
$ grep -rnP '(?<![A-Za-z0-9_])expected_refs\(' --include='*.rs' src/ | grep -v '/tests\.rs:'
src/engine/topology/recover.rs:1732:  let expected = …::expected_refs(&run_id, fold);
src/engine/topology/candidate.rs:916:  pub fn expected_refs(run_id: &str, …
src/engine/topology/candidate.rs:2074, 2082, 2303, 2314, 2904, 3003, 3012, 3024
```

**The elision in the first draft of this row was itself the defect, and the
number that replaced it was too.** The draft showed the first line and an
ellipsis; the correction said "ten lines"; a reviewer running it one commit later
gets **thirteen**, because `765a2f7` moved `production_calls` into
`src/effects.rs` and its doc block names the needle three times.
`PR7-R6-CONTRACT-003` / `PR7-R6-ATT-002`.

**So this row states the reading and not the count.** `grep -v '/tests\.rs:'`
removes the out-of-line test files and does nothing about an **in-file**
`#[cfg(test)] mod tests`, which is where `candidate.rs`'s calls live, nor about a
doc comment naming the function. Exactly one hit is a call outside test
configuration — `recover.rs`'s, inside `run_recovery_order` — and the reading that
decides that is `effects::production_code`'s, not `grep`'s, which is what the
census uses and why the census is the evidence rather than the transcript.

**The rule this gives, and it is round 6's finding in one line**: a raw count over
the tree is a claim about a version of the tree. It decays on the next commit, it
decays *silently*, and it decays fastest for the needles this project writes
about — every doc comment that names one moves it. State the property; put the
number in a test.

`recover.rs:1732` is the call `cf7bdb5` added, in production code, and it satisfies the
entry on its own merits. So the defect is that **the needle is unsound**, not that this
entry is hollow: `format!("{name}(")` will silently satisfy any future entry whose name is
a suffix of another identifier, and the failure is latent rather than present. Repaired at
its class boundary rather than at the instance — see the commit that carries
`a_call_census_needle_is_not_satisfied_by_a_longer_name_ending_in_it`.

### A process note: the first suite run of this session reported a failure that was not there

`an_ending_run_offers_no_work_from_any_arm` failed at `cca1276` on the first
`upstroke-build cargo test --all-targets --all-features` of the session, with
`Dispatch { continuing: true }` — the exact shape of the `PR7-R3-LOOP-001` defect
`aee0432` repaired. It passed at `040a100` in a fresh worktree. No source change between
those two commits touches `select.rs`.

It was **a poisoned build slot** — §17's *"The build slot pool poisons concurrent agents
across worktrees"*, one occurrence further on, and with §17's own signature. The second line
of that run's log is:

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.01s
```

`Compiling` appears **zero** times in it. So nothing from `/srv/tactus` was built and the
suite ran a binary already sitting in `slot2`. `cargo clean` in that slot, then the same
command, gives **1694 lib + 8 bin passed, 0 failed, 32 ignored** — green.

**What is proved and what is inferred**, because this section is about that distinction.
Proved: the run compiled nothing, and the same working tree compiles green. Inferred: the
binary was one of round 4's five reviewer worktrees', carrying a `select.rs` mutation left
in the artifacts — the failure has a mutation's shape, those five are the only builds this
pool saw between `040a100` and now, and all five trees are clean at `040a100`, so no
mutation survives in any *source*. §17's confirmation is
`strings <target>/debug/deps/<crate>-<hash> | grep -oE '/[^ ]*<worktree>[^ ]*'`, and it is
**not available here**: the `cargo clean` that fixed the slot destroyed the binary that
would have named its manifest dir. Diagnose before cleaning is the third rule, and it is
recorded because it was not followed.

§17's rule is *"an agent building in a worktree must touch every source file before its
first gate run"*. Two things this occurrence adds to it:

1. **The poisoning outlives the round.** All five reviewer worktrees were clean and idle
   when this failure appeared; nothing was building. It is the artifacts that persist, not
   the concurrency, so the rule extends past the round that caused it.
2. **It reaches the main tree, not only the worktrees.** §17's occurrence was a reviewer
   handed a sibling's binary. This was `/srv/tactus` itself, and what it produced was a
   *phantom failure in the one test this session was about to repair* — which is the
   direction that costs an hour. The other direction hides a real defect and costs more.
   **A suite run that follows a review round is cleaned, not trusted.**


## 20. PR7 S5 round 4 — the disposition of all 27, and what is carried past it

§19 is the eight false claims. This is every finding of that round with where it
went, and the backlog behind it. **Twenty-six of the 27 are closed in-slice; one
is carried with an owner.** Each closure names the commit that carries its
witness, and every mutation round 4 measured as surviving has been re-run against
the repaired tree and killed.

### The 27, by lens

| id | sev | disposition |
|---|---|---|
| `PR7-R4-LOOP-001` · `PR7-R4-CONTRACT-001` · `R4-SEAMS-002` · `PR7-R4-ATTEMPT-004`(a) | P1 | **Repaired, `59cde4d`.** The census read `attempt.rs`/`settle.rs`; the literal `None` was in `run.rs`. Restoring it left the whole suite green — re-measured here. `the_retaining_incarnation_retries_in_place` now seeds a pool and asserts it on **both** `attempt_started` appends, which is the behavioural witness `79cd9c8` said was unavailable |
| `PR7-R4-ATTEMPT-001` · `PR7-R4-CONTRACT-002` · `R4-SEAMS-001` · `PR7-R4-SETTLE-001` | P1/P2 | **Repaired, `21f1de0` and `faf0158`.** Three holes in one census: the substring collision (`expected_refs(` ⊂ `refuse_unexpected_refs(`), the fourteen out-of-line test files reading as production, and three unrelated items named `settle_interrupted`. Closed by a boundary-aware needle, a test-file skip with a control that it is in force, and a per-entry **call form**. Each name must now also be defined. All three of round 4's mutations re-run and killed |
| `PR7-R4-LOOP-002` · `PR7-R4-SETTLE-002` · `PR7-R4-CONTRACT-006` | P2 | **Repaired, `5a08f19` and `faf0158`.** The ending witness drove three of six arms while its doc said every. All six now, with `arm_label` total over `Step` so a seventh is a compile error, and `a_halted_run_offers_no_work_from_the_arms_that_rest_on_the_guard` pins the guard's other disjunct — round 4 measured `&& halted_at().is_none()` surviving the whole suite twice |
| `PR7-R4-LOOP-003` | P3 | **Repaired, `5a08f19`.** `an_ending_run_reaches_closure` does not exist; the real predecessors are named |
| `PR7-R4-LOOP-006` | P3 | **Repaired, `21f1de0`.** The census's doc now states what it does **not** cover, `Spend::replay` first among them |
| `PR7-R4-ATTEMPT-002` · `PR7-R4-SETTLE-003` | P2 | **Repaired, `59cde4d`.** The reviewer-pool fixture's implementer and reviewer shared an agent, so both behaviours passed. It binds `REVIEW_AGENT` now and asserts that premise before the claim, so it cannot degrade back |
| `PR7-R4-ATTEMPT-003` · `PR7-R4-SETTLE-004` | P2 | **Repaired, `59cde4d`.** `capacity::pool_for` had three call sites in `assembly.rs`; the two copies were character-for-character the seam's body and now go through it. `the_frozen_pool_table_is_read_through_one_seam` holds the count at one |
| `PR7-R4-ATTEMPT-004`(b) | P2 | **Repaired, `59cde4d`.** "No driver fixture can reach the arm" — one does, and it is now the witness |
| `PR7-R4-CONTRACT-003` | P2 | **Repaired, `6f71b64`.** The pre-clean's second caller. `preclean_names` now refuses a name that is not this build slot's, so a third caller cannot repeat it |
| `PR7-R4-ATTEMPT-005` · `PR7-R4-CONTRACT-004` · `R4-SEAMS-003` | P2/P3 | **Repaired, `faf0158`.** The stem census took its value to the first comma and matched field initializers only, so it could not see `coordinator.rs:537` — the **live legacy path**, where dropping the sanitiser left the whole suite green |
| `PR7-R4-CONTRACT-005` · `PR7-R4-SETTLE-005` | P3 | **Repaired, `faf0158`.** The allowance census's needle missed `+=` |
| `PR7-R4-LOOP-005` | P3 | **Repaired, `faf0158`.** `RunAs`'s doc said "fresh generation"; the continuation path is not one |
| `R4-SEAMS-004` · `R4-SEAMS-005` | P3 | **Repaired, `faf0158`.** A §4 count cell that contradicted its own row, and a §4 row orphaned from the table by a blank line |
| `PR7-R4-LOOP-004` | P3 | **Carried — see below.** `Closure(NotEnding)` on the ending path |

### The one carried, and why

**`PR7-R4-LOOP-004`: `select` can return `Closure(DerivedOutcome::NotEnding)`.**
`RunState::derived_outcome` returns `NotEnding` whenever a generation blocks run
end, and an `OpenNoAttempt` generation does — which is exactly the fold the
ending guard was written for. `Step::Closure`'s own doc says "run-end closure is
due, with the outcome the fold derives", so the value contradicts itself, and
`checkpoint` then refuses with "closure derives NotEnding" to the operator of a
run that is in fact budget-stopped.

**Owner: the slice that implements closure — PR8/PR10.** Carried rather than
repaired for two reasons, both stated so the next slice does not have to
re-derive them. The behaviour is masked here: this build refuses run-end closure
outright (`checkpoint_refusals`), so no run acts on the value. And the repair is a
choice this slice has no standing to make — either closure closes the open
generation first and re-derives, or `derived_outcome` learns to answer for a run
that is ending with work still open. The second changes a `src/topology/**`
reader and the first is closure's own shape. What is owed with it is the
diagnostic: whatever PR8 chooses, an operator told "closure derives NotEnding"
about a budget-stopped run is being told the wrong thing.

### Round 3's carried P2/P3s, confirmed against the tree

Six were confirmed still true and repaired in `9b6fef1` — `PR7-R3-EMIT-003`,
`-004`, `-005`, `PR7-R3-CONTRACT-006`, `-007` and `PR7-R3-LOOP-003`, the last of
which was a measured surviving mutation on `loop`'s branch order. The rest are
carried, each with what would close it:

| id | why it is carried |
|---|---|
| `PR7-R3-ATTEMPT-002-REVIEWERS-TAKE-NO-SLOT` | A review pass reaches the Runner through the `ReviewPasses` seam with a raw `&dyn Runner`, so it takes no slot. **R3 is "assertion only" at `max_parallel = 1`** and this slice ships that width, so nothing can over-subscribe. It becomes live with PR11's parallelism, and the repair is a seam change — the reviewer path taking the same `SlotAssertion` — not a line. Owner: **PR11** |
| `PR7-R3-ATTEMPT-003-RESIDUE-DISCARD-UNREACHED` | The snapshot worktree's ephemeral commit is reachable after a coordinator death mid-attempt, and nothing discards it. Owner: **the slice that owns snapshot reclaim**. Carried because the repair needs a reclaim path this slice does not have, and because the residue is inside the run's own private root |
| `PR7-R3-ATTEMPT-004-NO-TRANSCRIPT-NO-GATE-LOG` | §11.1's feedback is intact — `judge` builds the gate tail and the retry is told — but nothing on the schema-4 path writes `transcripts/<stem>-<attempt>.json`, so the **operator-facing** evidence the legacy engine wrote is absent. A real capability gap, not a defect in what exists. Owner: **project owner, for the G2 erratum list**, with `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4` in §2, which is the same shape one artifact over |
| `PR7-R3-EMIT-006-DEFER-ROUND-IS-A-BACKOFF-ROUND` | **CLOSED 2026-08-26 by per-instance approval for the comment-only edit**, carrying the erratum text staged here. `DeferWaitElapsed4.round` was documented as "Which sleep this was, **counted across the run**"; it now says "consecutive waits where deferred work was the only runnable work". **The wire is unchanged** — no field, type or serde attribute moves, and `events::SCHEMA_VERSION` is outside the file and untouched — and the reason it was not left for the G2 pass is that a reviewer-facing wire doc must not carry a known falsehood into the frontier review. Precedent: §11. **Two things the edit found that the staged text did not say**, both measured before the doc was written: the sole production construction is `settle::Deferral::wait`, reached from `TopologyRun::step` alone, and it writes `self.round` **after** incrementing — so the value is one-based and the sequence a reader sees is **1, 2, … 12, 1**, not 1, 2, 3. "Counted across the run" therefore reads a *later* sleep as an earlier one, which is sharper than "imprecise". And `the_defer_backoff_doubles_caps_and_resets` asserted the accumulator's reset and **not** the value the event carries — §4's "an accumulator's witness proves the accumulation and not the read", at four occurrences, applying to the field whose doc was being corrected. The recorded sequence is now asserted, so the wire doc has a witness rather than a reading. The neighbour doc-attachment check was performed: `waited_ms` above is undocumented before and after, the struct's own block still attaches to `DeferWaitElapsed4`, and nothing below is stranded |
| `PR7-R3-SETTLE-LADDER-POSITION-RUNG-HALF` | The `rung` half of `ladder_position`'s accumulator, filed in §4's "an accumulator's witness proves the accumulation and not the read" row at 4 occurrences. Owner: **PR8** |
| `PR7-R3-CONTRACT-004-UNRESOLVED-INDEX-REFUSAL-UNREACHABLE` | `expected_failures_refusals` names "empty-diff **and unresolved-index** attempt failures"; the empty-diff half is produced and named, the unresolved-index half has no fixture that reaches it. Owner: **project owner**, as a G2 erratum question — whether the clause is this slice's at all |
| `PR7-R3-SETTLE-CAND-OBJ-REFUSAL-UNREACHABLE` | **Closed by `cf7bdb5`**, and confirmed here: `refuse_unexpected_refs` has a production caller in `run_recovery_order` (`recover.rs:1735`) and `expected_refs` derives its entitlement at `:1732`. Recorded rather than dropped, because the round-3 report predates the repair |

### Round 2's carried items, unchanged

`PR7-PIPELINE-008` (§2, `PR7-STEP-D-LINEAGE-ARM-UNWITNESSED`) and
`PR7-PIPELINE-014` are unchanged in disposition and unchanged in evidence: the
first is unreachable until PR8's merge queue spawns a repair, measured over
`effects::production_code`; the second is a "held across" claim that needs a
paused run, which is `PR5-R2-WORKTREE-LOCK-RETENTION`'s shape.

### `R3-SEAMS-006`'s residual

Unchanged, and it is in §2 as `R3-SEAMS-006-ATT003-REPAIRED-POSTHOC`. The claim
as described is refuted with the item and lines inspected; the residual — whether
a Runner-**spawned**-but-unreportable process belongs in the invocation ledger —
is a real `permits.protocol` question and is the owner's.

### One consolidation this round's repairs leave behind

`every_packet_named_recovery_action_has_a_production_caller` skips out-of-line
test files by file stem; `runner::tests::production_sources` does the same job
through `effects::census_domain::declared_whole_file_test_modules`, which derives
the set from the crate's own `#[cfg(test)] mod …;` declarations and asserts it
found at least thirteen. **Two idioms for one rule**, and the second is the
better one. Not unified here because the neighbouring census in the same file —
`the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle`
— uses the file-stem skip too, so the change is one edit across both plus a
decision about where the shared walk lives. `R4-SEAMS-001` named the seam and it
is right. Owner: **whichever slice next opens `effects::census_domain`**.


## 21. PR7 S5 round 5 — the protocol audited by running it

Round 5's subject was not a module. It was **every verification-language claim
the seven commits `cca1276..5e309a0` make**, and every prompt required a
`claims_rechecked` array: the claim, the command the reviewer ran, what it
printed, and a verdict of `reproduced` / `contradicted` / `unverifiable`.

Five lenses returned **28 findings and 106 re-run claims**, with **77** items in
`checked_and_clean` — `loop` 3/16/14, `attempt` 7/18/13, `settle` 6/19/13,
`contract` 6/38/22, `seams` 6/15/15. That ratio is the
result: most of what a confirming round over a repair diff finds, once the
repairs are written under the claims protocol, is claims — and they are cheap to
check and cheap to fix, which is the point of writing them down.

### What it contradicted, and where each went

| what | disposition |
|---|---|
| Four doc comments quoting a `grep` whose count is one higher than the doc claims, because the doc is in `src/**/*.rs` and the command searches `src/` | **Already repaired at `c01a844`**, found by re-running my own quotes before the round did. §4 carries it as a class: *a command quoted as evidence becomes part of its own input* |
| The census skip covers fourteen of the **seventeen** whole-file test modules the crate declares — `scaffold`, `premove` and `fake` are not called `tests.rs` | **Repaired, `765a2f7`.** One resolver, `census_domain::whole_file_test_modules`. §20 had filed the consolidation as tidiness one commit earlier. **The commit message and this row both said "four call sites" and there are five** — the fifth is in the witness the same commit added, `effects/tests.rs`, which is the one place a reader would look to check the claim. Counted **at `d17bcf2`**: `events/log/tests.rs`, `effects/tests.rs`, `runner/mod.rs`, `recover/tests.rs` ×2. `PR7-R6-ATT-010`, and the same class as every other number in this section |
| "A seventh arm cannot be left out of the coverage assertion" — the compile error fires, the coverage assertion does not | **Repaired, `765a2f7`.** `every_label_the_arm_classifier_returns_is_classified` reads `arm_label`'s own match body and requires every label it returns to be classified |
| `receiver_writes` sees five of Rust's ten compound assignment operators; `task.attempts_on_rung \|= 1;` leaves the census green | **Repaired, `765a2f7`.** The enumeration is the language's now |
| The pool-table census's needle is the literal `capacity::pool_for(`, so `use crate::capacity::pool_for;` + a bare call is invisible — both spellings live in this tree | **Repaired, `765a2f7`.** Free calls through the shared `production_calls`, with controls in both directions |
| `EmitState`'s doc says "**Five** borrows … obligations are each a statement about one of these five"; four fields, and obligation (3) moved to the caller | **Repaired, `765a2f7`** |
| `slot_repo_key` claims "every container name in a Docker-gated test"; `runner::container::tests` names gated containers with bare literals | **Repaired, `765a2f7`**, along with the guard's inertness when `CARGO_TARGET_DIR` is unset |
| The stem census's rationale promises it survives a fourth assembler; its control is now an equality that a fourth assembler fails | **Repaired, `765a2f7`** — both facts stated, with which one is deliberate |
| `run.rs:615` — correct **at `9b6fef1`** — cited in a correction that made its own paragraph thirteen lines longer, so the number pointed into the correction itself | **Repaired, `765a2f7`.** Named rather than cited by line |
| `AttemptContext::start`'s historical note appended **after** its `# Errors` heading, so rustdoc renders it as part of the error contract | **Repaired, `765a2f7`** |
| The `expected_refs` transcript quoted one line and an ellipsis; the command prints ten | **Corrected in §19 above**, with the ten and the reading that decides them |
| The §4 occurrence count read 8 while the same row's prose named a ninth | **Corrected in §4 above**, to 9, with the rule that a count and its prose are edited in one motion |

### The two claims that are false and are corrected here rather than repaired

**Two of the eleven recorded restore-hashes do not re-derive**, and the ledger is
where that is corrected because the commit messages are pushed history.

```
$ git show 5a08f19:src/engine/topology/select.rs | sha256sum
4cf6f9a2adbb084c…      recorded in that commit's message: 1171ccee…
$ git show 21f1de0:src/engine/topology/recover/tests.rs | sha256sum
1e370f188739f51e…      recorded in that commit's message: d93f24e5…
```

The other nine reproduce exactly — `fake.rs 3de2161c`, `census/tests.rs b594df57`,
`assembly.rs 5d03e2ff`, `coordinator.rs bc7222cd`, `recover.rs 5e667625` among
them, each checked against the commit whose message records it.

**The cause is `cargo fmt`, and it is the same trap this tree already records
twice for mutation anchors.** In both cases the pristine copy was hashed
*before* the final `cargo fmt`, and fmt then reflowed the file. The hash was true
of the restore it verified at that moment and is false of the committed file, so
a reader checking it against the commit finds a mismatch and cannot tell a
sloppy record from a failed restore.

> **The rule.** A restore hash is taken **after** the last `cargo fmt`, so it is
> a hash of the committed content, or it says what it is a hash of and when.
> "Verified by hash" with a number a reader cannot re-derive is worth less than
> no number: it looks checkable and is not.

**And a fourth and fifth, of a different shape, found by round 6.** `765a2f7`
records `run.rs 94b066db…` and `3a91626` records `runner/mod.rs 6881666c…`. Both
are **the parent's blob**:

```
$ git show 765a2f7:src/engine/topology/run.rs    | sha256sum   035a2045…
$ git show 765a2f7~1:src/engine/topology/run.rs  | sha256sum   94b066db…
$ git show 3a91626:src/runner/mod.rs             | sha256sum   407af8ba…
$ git show 3a91626~1:src/runner/mod.rs           | sha256sum   6881666c…
```

**Each message is literally true and each is useless to a reader.** They say
"verified by hash against its **pre-mutation copy**", and that is exactly what the
number is: the file as it stood before the mutation, which equals the parent's
blob whenever the restore is the last thing that happens to that file. A further
edit after the restore — a doc correction, in both cases — makes the commit's
blob differ, and §21 above tells the reader to check `git show <sha>:<path>`.
A claim whose stated method and whose recommended verification disagree is not
evidence, whatever it was to the author. `PR7-R6-CONTRACT-008`, `PR7-R6-ATT-007`.

**A third occurrence, in the commit that wrote that rule's own round.**
`765a2f7` records `run.rs 94b066db…`; the committed file hashes `035a2045…`. The
restore was real and the hash was true of it — `run.rs` was then edited once more,
to name the slot-assertion field instead of citing a line, and the message kept
the earlier number. Found by re-deriving my own three hashes before round 6 could,
which is the third time in this session that running one's own claims has been
cheaper than being told.

**So the rule as stated is not enough, because it asks a person to remember a
step at the moment they are finishing.** The mechanical form, and what this
project should use from here:

```
$ git add -A && git show :src/path/file.rs | sha256sum
```

The **staged** content is by definition what the commit will carry, so a hash
taken there cannot drift, and the reader re-derives it with
`git show <sha>:<path> | sha256sum`. A hash that means "the working tree at the
moment I restored it" is a note to oneself; a hash of the staged blob is
evidence.

### Two things that are unverifiable by construction, named so they are not mistaken for verified

- **The falsification table's `sha256`** in `21c5735`. Its source is a session
  artifact outside the repository, so no reviewer of this pull request can
  re-derive it. What *is* checkable, and was checked, is that the nested copy is
  internally consistent with the stated transformation.
- **§19's process note** about the poisoned build slot. `cargo clean` destroyed
  the binary that would have named its manifest dir; §19 already separates what
  that leaves proved from what it leaves inferred, and round 5's independent
  reading agrees the support is indirect.

### What round 5 checked and found sound

Across the five lenses, **77 items** in `checked_and_clean`. The three P1s of
round 4 that this session repaired — the unwitnessed retry pool, the clause
census's collisions, the pre-clean's second caller — were each re-driven with
round 4's own mutations and each is killed.


## 22. PR7 S5 round 6 — the crop is entirely claim-drift, and that is the convergence signal

Round 6 read the four commits that answered round 5 (`c01a844~1..8e48dd1`) with the same
five lenses and the same protocol. It returned **50 findings, 112 re-run claims, 95 clean
items** — `loop` 10/21/18, `attempt` 10/26/17, `settle` 12/18/24, `contract` 9/23/17,
`seams` 9/24/19.

Those three totals are counted from the five lens reports, which are session artifacts
outside this repository, so they are **unverifiable by construction here** — the same
disposition §21 gives round 5's. What *is* in the repository and checkable is the table
below: eleven defects, each with the command that finds it and the repair that closes it.
The diff range is the stamp on the rest.

**Fifty findings, eleven distinct defects.** Each lens reached most of them independently,
which is what the count measures. The eleven:

| # | defect | repaired |
|---|---|---|
| 1 | §21 cited **`e1e6841`** nine times as round 5's repair commit — nine occurrences of the string, counted at `8e48dd1`. That object was **dangling** when observed at `d17bcf2`, and being unreachable it may be garbage-collected, after which even this row's evidence stops resolving: `git cat-file -e e1e6841` answered yes then and need not later. It is — the commit was amended into `765a2f7` and §21 was written against the pre-amend sha | repointed, all nine |
| 2 | `recover/tests.rs:**5488**` quoted as terminal output; `765a2f7` inserted nineteen lines above it and 5488 is now a blank line | the item is **named**, and the line number is gone |
| 3 | §19's corrected transcript says "**ten lines**"; at the reviewed head the command prints **thirteen**, because `765a2f7` moved `production_calls` into `effects.rs` and its doc names the needle three times | the row states the **reading**, not the count |
| 4 | Two restore hashes are **the parent's blob**: `run.rs 94b066db` at `765a2f7`, `runner/mod.rs 6881666c` at `3a91626` | corrected in §21, with why each message is literally true and still useless |
| 5 | "one resolver and **four** call sites" — there are **five**; the fifth is in the witness the same commit added | corrected |
| 6 | "`fn drive` … and **nothing in `src/engine/`**" — two of the three hits *are* under `src/engine/`, and the command quoted beside the clause says so | clause removed, the true statement put in its place |
| 7 | `OFFERS_WORK`/`OFFERS_NO_WORK` inserted between `fn arm_label` and its doc block — **occurrence 10** of §4's class | consts moved below `arm_label` |
| 8 | `production_calls`, `Call` and `whole_file_test_modules` inserted between `declared_whole_file_test_modules` and its doc block — **occurrence 11**, in the module that exists to hold shared census machinery | moved above the doc block |
| 9 | `cancel_all_running`'s doc quotes a raw hit count that read 3, then 4, then 5 across three commits, each time correctly | the count is gone; the stable claim stays |
| 10 | The seventeen-modules witness asserts what the **resolver returns** and nothing about whether a census calls it — the defect `3a91626` repaired for two censuses, reproduced one commit later | the control moved **into** the resolver, where no caller can miss it |
| 11 | `OFFERS_NO_WORK` membership was untied to behaviour: moving a work label into it satisfies the census and drops that arm from the coverage requirement | pinned by name, with the reason |

### What round 6 says that rounds 4 and 5 did not

**Nothing it found is behaviour.** Not one of the fifty is a defect in what a run does. The
whole crop is *claim-drift*: a line number, a count, a hash or a sha that was **true when
written and false one commit later**. Round 4's crop was prose asserted without checking;
round 5's was witnesses that did not witness; round 6's is evidence that decays.

**And it decays silently, fastest, for exactly the things this project writes about.** A
count over the tree for a needle moves whenever any doc comment names that needle — so the
act of documenting the count is what invalidates it. Three of the eleven are that.

> **The rule.** A doc comment or a ledger row states a **property**; a **measurement** goes
> in a test, or is stamped with the sha it was taken at. Line numbers, raw `grep` counts and
> hashes of anything but a staged blob are claims about a version of the tree, and this
> session produced eleven of them in four commits while trying to be careful.

§4's Occurrences column is the first thing changed under it: it now reads *derived at a
named sha* rather than a maintained number, because a maintained count in this project has
been wrong three times out of three — each time corrected by a commit whose own diff added
occurrences.

### A process note the guest driver cost this round

`pr6/drivers/win-iter.sh` writes every run's full output to a single
`/tmp/win-iter.log`, and the wrapper keeps only the summary lines. So the **second**
of two intermittent Windows failures lost its errno: the next run in the same loop
overwrote the log before it was read, and `PR7-WIN-READ-RACING-BOUND-TOO-SHORT` records
that failure's cause as a presumption rather than a measurement because of it.

The driver is the owner's script and is recorded rather than edited. What a caller
can do in the meantime is copy `/tmp/win-iter.log` beside each run's summary before
starting the next — which is what this session should have done from the first red
run, and is the same class as §17's "an intermittent failure you cannot name is one
you cannot attribute".

### The stopping condition, and the honest reading for the owner

The finding counts are 27, 28, 50 and **not falling**. The distinct-defect counts are
roughly 11, 11 and 11 — flat. But the *kind* has narrowed to one thing, and it is the kind
that a rule can close rather than a repair: rounds 4 and 5 each found defects in the code
or in what holds the code; round 6 found none, and every item is a citation that aged.

**Zero-admissible is therefore not reached and should not be declared.** A seventh round
over these repairs would find the citations *this* round's repairs introduce — the pattern
is three for three. What breaks it is not another round but the rule above, and the owner's
call is whether the slice ships with the rule adopted and this crop closed, or whether an
instrument round runs until a round returns nothing.


## 22b. The frontier review's own findings, and what §22's rule did not cover

`reviews/2026-08-26-pr7-frontier-review-75da796.md` is the record; this is what it
changed here.

**Four unversioned false property claims survived every round, in production doc
comments, and §22's rule did not reach them** — because the closing sweep that §23
rests on scoped itself to *the prose two commits added*, and these predate those
commits. Corrected where they stand, with the reviewed sha beside each:

| where | said | is |
|---|---|---|
| `settle.rs`, `Settled::spent_attempt` | an outage deferral spends none "and every other settlement spends one"; the fold derives the count from `attempt_started` | **five** kinds spend nothing — `NeedsHuman`, `NoChain`, `Interrupted`, `Declined` and the outage deferral — and `apply_settlement` derives it from `attempt_finished` |
| `candidate.rs` module doc | "nothing here is a production path yet … the coordinator that will call them is the rest of PR7" | that coordinator **arrived in this slice**: `TopologyRun::promote_candidate` and `recover::finish_promotions` call six of these functions outside `#[cfg(test)]`. What keeps the effect "none" is `pub(crate)`, not the absence of callers |
| `emit.rs` module doc | obligation (3)'s ledger side "is this module's" | `bcc5c2f` moved it to the caller; `UncancelledAppend`'s own note in the same file has said so since |
| `engine/mod.rs`, `pub mod topology` | the visibility guards compile-fail fixtures | the same doc admitted no such fixture exists. Repaired by narrowing, not by rewording |

**The rule that follows, and it is a correction to §22 rather than an addition
to it.** §22 says a measurement carries the sha it was taken at. That is
necessary and it is not sufficient: three of the four above carry no number at
all. They are **property** claims that were true when written and were falsified
by later commits in the same slice — the `candidate.rs` one by the very
coordinator this slice added. A property claim decays exactly like a
measurement, and nothing in this project re-reads one after the commit that
made it false.

> A doc comment that says what *another part of the tree* does is a claim about
> that part, and it ages when that part changes. The sha-stamp rule covers
> numbers; for properties the only instruments are a census that ties the
> sentence to the code, or a reviewer.

Two of the four now have the first kind: `pub mod ` is forbidden in the engine
facade by `the_engine_facade_exposes_exactly_the_items_the_packet_enumerates`,
and the allowance rule has `ladder::spends_allowance` as its single authority
with the doc pointing at it. The other two are prose against prose.

### `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4` — fixed, and the fifth false claim it turned up

Measured at `bd3b9cd`, repaired in the commit carrying this text.

The frontier review's finding 2 held that a ledger disposition cannot waive a live
passage, and it is right. The row below was a **recorded deviation** from DESIGN §11.4
and had been since the slice opened. The preferred repair was rebuild-on-resume — a
derivation needs no wire change — and it is not reachable:

```
$ grep -c 'required_changes' src/events/mod.rs src/topology/events.rs
src/events/mod.rs:0
src/topology/events.rs:0
$ grep -cE 'log_tail|gate_log|FEEDBACK_TAIL_BYTES' src/events/mod.rs src/topology/events.rs
src/events/mod.rs:0
src/topology/events.rs:0
```

Neither §11.4-named source is on the wire in any form, so the stop-and-ask went up and
the owner authorised fork 1 as **Class C with its ceremony**:
`decisions/2026-08-26-durable-retry-feedback.md`, and §3 below carries the
per-instance approval.

> **Re-run that command at a later head and the first number is 1, not 0.** The field's
> own doc comment quotes "the reviewer's `required_changes` (§11.2)", so the repair moved
> the needle its justification measured. The `bd3b9cd` stamp is what keeps the quotation
> true — §22's rule doing exactly the work it was written for — but the stamp alone would
> leave a reviewer re-running it at HEAD to wonder, so it is said here. Fourth shape of §4's
> self-referential-needle class in this slice, and the first where the *repair* rather than
> the *documentation of a census* moved the count. The same caveat applies to the copy of
> this measurement in the decision record, which is not amended for it: `decisions/` is
> outside the 2026-08-20 exempt path set, and a docs edit there would restart a review
> sequence over a sentence its own sha stamp already makes true.

**What the witnesses assert is delivered content, and each has a mutation that kills
it.** §23's own standard is that a mechanism existing is not the claim; the claim is
that the next worker is *told*. All five run in the same invocation, so the columns are
comparable:

| mutation (the defect's class, re-applied) | crash→same-rung | crash→escalation | write path | older log | live mode |
|---|---|---|---|---|---|
| *(none — baseline)* | ok | ok | ok | ok | ok |
| the resume rebuilds an empty brief — the reviewer's exact sequence | **FAILED** | **FAILED** | ok | ok | ok |
| `classify` writes `detail: None` again | ok | ok | **FAILED** | ok | ok |
| the live loop stops recording from the appended record | ok | ok | ok | ok | **FAILED** |
| the brief keeps only the newest line | ok | **FAILED** | ok | ok | ok |
| `#[serde(default)]` removed | ok | ok | ok | ok | ok |

Columns are `a_crash_does_not_erase_what_the_last_attempt_was_told_to_fix`,
`an_escalation_after_a_crash_carries_the_accumulated_feedback`,
`both_feedback_sources_reach_the_durable_attempt_record`,
`a_log_predating_the_detail_field_folds_and_resumes`, and the pre-existing
`a_retried_worker_is_told_what_the_last_attempt_failed_on`.

**The last row is a finding about the ledger, not about the code.** Removing the
attribute changes nothing: serde's derive already reads a missing `Option<T>` field as
`None`, confirmed by a two-struct probe decoding `{"kind":"gate_failed"}` with and
without it. The attribute is kept — the authorization specifies it and every other
optional field on this wire carries it — but the *backward-compatibility property* is
carried by the field's type, and the decision record says so rather than crediting the
attribute. A claim that survives its own mutation is a claim nothing is holding.

**A fifth false property claim, found by the compiler.** `classify::attempt_record`'s
doc said "the one production construction of an `AttemptRecord`". Adding a field to
`FailureRecord` broke 17 initializers and named a second: `events::Dangling::event`,
the record for an attempt that started and never reported back. Same class as the four
above — a property claim about another part of the tree — and the same correction:
the sentence now carries its qualifier and names the other one.

**And the third occurrence this slice of a doc comment that changes the census reading
it.** The first draft of that correction quoted the initializer and the test-only
attribute literally. A region-cutting census then stopped **inside the doc comment**,
above the construction it was looking for, and reported one production construction
where there are two — the §4 self-referential-grep class, one file further along.
Both corrections and the rule are on the function itself.

## 22c. The re-review of `c2c0294`, and the one finding that was the harness

`reviews/2026-08-26-pr7-frontier-review-c2c0294.md` is the record. Four blocking findings;
three stand, one dismisses. This is what they changed here.

### Finding A — the repair reached the legacy wire, and a census could not have seen it

**Correction to `decisions/2026-08-26-durable-retry-feedback.md`, amended on-branch before
it landed anywhere.** The record's compatibility section claimed *"`report.json` is
unaffected … this change adds no call site to it"*. Measured at `502970d`:

```
$ grep -n 'classify::attempt_record' src/engine/coordinator.rs
844:                data: Box::new(super::classify::attempt_record(
                        failure: result.failure.as_ref(),
$ grep -n 'pub attempts' src/engine/report.rs
83:    pub attempts: Vec<AttemptRecord>,
530:        attempts: records.clone(),
```

`coordinator.rs` is the **live** schema-3 path. It passes an `AttemptFailure` whose
`feedback` holds the gate tail or the reviewer's `required_changes` into the shared
builder, so `detail: failure.feedback.clone()` put the full text on the legacy wire and
into `report.json` — once per failed attempt, duplicating the `ladder_retry` copy, and
reversing the reason `LadderRetry`'s own doc gives for holding it.

**Why every instrument this slice owns missed it, which is the part worth keeping.** "Adds
no call site" was *true*. The change added no **initializer**; it changed what an existing
shared one writes. Every census in this repository counts constructions — that is how the
second `AttemptRecord` construction was found two sections above — and a construction
census cannot see **value flow through a shared builder into a caller nobody read**. §22b
says a property claim about another part of the tree ages and only a census or a reviewer
catches it. This is the sharper case: not a claim that aged, but a claim about a caller
that was never read at all, and no census could have read it.

> A claim that a change does not reach some other engine is a claim about that engine's
> **callers**, not about this change's call sites. The instrument is reading them.

**The repair** is `classify::FeedbackCarrier` — a two-variant choice on `AttemptFacts`
with **no default**, so a caller must decide and a third engine will not compile until
someone does. The compiler named all three existing sites when the field was added.

**Witnesses, and the mutation that kills each.** All run in one invocation:

| mutation | legacy wire | schema-4 live | schema-4 crash | write path |
|---|---|---|---|---|
| *(none — baseline)* | ok | ok | ok | ok |
| the legacy caller asks for `AttemptRecord` (finding A, re-applied) | **FAILED** | ok | ok | ok |
| the schema-4 caller asks for `LadderEvent` | ok | **FAILED** | ok | ok |
| the `match` collapses to an unconditional write | **FAILED** | ok | ok | ok |
| `#[serde(default)]` becomes `skip_serializing_if` | ok | ok | ok | ok — but the **strict door** witness FAILS |

Columns: `the_legacy_wire_and_report_carry_no_feedback_on_the_attempt_record`,
`a_retried_worker_is_told_what_the_last_attempt_failed_on`,
`a_crash_does_not_erase_what_the_last_attempt_was_told_to_fix`,
`both_feedback_sources_reach_the_durable_attempt_record`.

**The second row is a witness gap this battery found, not a mutation that was expected to
survive.** The crash witnesses seed a log directly, so they assert what a resume does with
a `detail` already present and cannot tell whether a live schema-4 settlement writes one.
Pointing the driver at the legacy carrier left every test in the file green. The live
driver test now reads its own log and asserts a settled failure carries the text.

**And the legacy witness is a fixture comparison, not a self-transform.** The expected
bytes are the bytes `610106b` — the commit before the field existed — actually wrote for
the same gate-failure scenario, captured by running it there:

```
"failure":{"kind":"gate_failed","origin":"worker","reason":"gate `needs-test` failed: …"}
```

Three keys. The test asserts that stripping `,"detail":null` from what this build writes
leaves exactly those three, with those values — and that the strip *fires*, so an absent
key cannot pass vacuously.

**One residual difference is stated rather than hidden.** `detail` serializes as an
explicit `null`, so a legacy `failure` object gains that one key.
`skip_serializing_if = "Option::is_none"` would remove it and **breaks schema 4's strict
door**: an input carrying `"detail":null` decodes to `None`, re-encodes to nothing, and the
door reports a key the record did not claim back, refusing every failed attempt's
settlement. That was an argument in the decision record and is now a measurement — and the
door's own precondition test stays green under the attribute, because its fixture's
`AttemptRecord` has `failure: None` and contains no `FailureRecord` at all.
`an_explicit_null_detail_survives_the_strict_door` is that case, one record deeper, in a
file this exception may touch.

### Finding B — a resume could adopt a tree nothing judged

**RULED Class B**, per-instance approval granted 2026-08-26 and quoted in §3 with the
measured split. `PreparedCandidate` retains the event's `tree_sha`; `verify_object`
compares the commit's tree against it and refuses otherwise. `DESIGN.md`:410 is conformed
to, not amended; nothing serde-visible moves.

The residue was documented in `candidate.rs`'s own comment — *"A commit with the recorded
parent and a different tree would still pass here. Recorded rather than approximated —
closing it is a fold field and therefore its own decision."* The decision is the approval,
and the comment now says what the check does instead of what it cannot do.

**What the two findings share, and it is the thing worth carrying out of this round.**
Finding A's second mutation and finding B's second mutation are the same defect in two
subsystems: **a witness that bypasses the step it is about.**

| the witness | what it drove | what it therefore could not see |
|---|---|---|
| the schema-4 crash witnesses | a log seeded with a `detail` already present | whether a *live* settlement writes one — the driver asking for the legacy carrier left them green |
| `promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged` | a `PromotingCandidate` built by hand | whether the *fold* retains the right sha — retaining `base_sha` in that field left it green |

Both were found by re-applying a mutation and watching nothing fail, and both are closed
by moving the assertion onto the value production actually computes: the driver's own log
in the first case, the recovered promotion in the second.

> A witness that constructs the input to the step under test proves the step. It does not
> prove that anything upstream produces that input. When the defect being repaired is
> *upstream of the check*, the witness has to start further back.

### Finding C — five false property claims, and the one that is now a test

Corrected where they stand. Measured at `4809cd4`.

| where | said | is | what holds it now |
|---|---|---|---|
| `settle.rs`, `Settled::spent_attempt` | "**five kinds** spend nothing" | **13 shapes, spanning 7 kinds**: a `FailureShape` is a `(kind, origin)` pair, and `spends_allowance` dispatches on both — four kinds outright, plus `RateLimited` and `ReviewUnavailable` at any origin and `Timeout` at `FailureOrigin::Reviewer`, all three taken by `FailureShape::is_outage` before the match runs. **This cell said "seven shapes", which was the third wrong statement of the same number** (after "every other settlement spends one" and "five kinds"): seven is the *kind* count standing in for the shape count, which is the exact substitution the row was written to correct | `ladder::tests::exactly_thirteen_failure_shapes_spend_no_allowance`, which reads the variants out of the enum's own source and asserts both numbers |
| `events/mod.rs`, `FailureRecord::detail` | "the one production construction of an `AttemptRecord`" | two; `InterruptedAttempt::event` (`src/events/mod.rs:1040`) is the other. **This cell said `Dangling::event`, a type that does not exist** — the same invented name §22b records, left standing in the table that corrects invented names | the qualifier, and the census in §22b |
| `recover/tests.rs`, the old-log witness | after an older log "the brief is simply empty" | one line per failure, carrying its summary with `detail: None` | three assertions on the rebuilt brief's actual content |
| `recover.rs`, step (f) | "PR7 implements neither terminal" | `finish_promotions` calls `append_candidate_created`; the refusal is the *integration* half only | the sentence now says which half |
| `run.rs`, `park_question` | `task.rung + 1` "is the same quantity the legacy `BTreeSet<tier>` computes" | not for a chain naming one tier twice — `ChainSummary.tiers` is a `Vec<Tier>` nothing deduplicates, so `["small", "small"]` is 2 here and 1 there | the claim is narrowed and the divergence is stated, with which answer is right for the sentence being built |

**The `settle.rs` one has been wrong three times, and that is why it stopped being
prose.** It first said an outage deferral spends none "and every other settlement spends
one" — off by six. Round 6 corrected it to "five kinds", which reads the outage arm as one
kind when it is three shapes, and counts kinds when the authority dispatches on
`(kind, origin)`. A fourth restatement is a fourth chance to be wrong, so the number is now
counted from `spends_allowance` itself over every pair, the seven are named, and the one
pair where the origin decides — `Timeout` — is asserted in both directions.

**Where the wrong answer comes from is worth recording.** `spends_allowance`'s last match
arm reads `Timeout | RateLimited | … | ReviewUnavailable => true`, and all three of those
are unreachable there for the origins the outage guard already took. A reader who checks
the arm gets the wrong answer and has checked. That is the shape of a doc that is wrong
twice about code nobody misread.

> A number in a doc comment that a function can compute should be computed by a test, not
> restated by a person. The third restatement is the signal, not the first.

### A local gate that existed and was never run

`upstroke-pr-policy` failed at `e85f348` on five ledger rows, every one of them a location
or an identifier that does not exist at the sha it cites — the class this round was
repairing, in the artifact describing the repair:

```
PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4 prevention identifier is not tracked at exact head:
  #[serde(default)] detail: Option<String>
PR7-FR-005 location line 2300 exceeds reviews/FINDINGS.md at 75da796 (2050 lines)
PR7-FR-006 location does not exist at its reviewed SHA:
  75da796 / reviews/2026-08-26-pr7-frontier-review-75da796.md
PR7-RR-D  location does not exist at its reviewed SHA:
  c2c0294 / reviews/2026-08-26-pr7-frontier-review-c2c0294.md
PR7-FR-006 prevention identifier is not tracked at exact head: headRefOid
```

Line 2300 was read out of the *current* file for a claim about a 2050-line one, and both
review records were cited at shas that predate the commits adding them.

**The gate was runnable locally the whole time and I was running the wrong one.**
`bash .github/scripts/test-pr-ledger-evidence.sh` tests the *validator* — it passes
whatever the body says. The check CI performs is

```
cat <body> | bash .github/scripts/validate-pr-ledger-evidence.sh <exact-head-sha>
```

which resolves every cited `path:line` at its stated reviewed sha and every backticked
identifier at the head. Run against the body it caught all five, including two the CI run
had not reached.

> A `test-*.sh` gate proves the validator works. It says nothing about the artifact.
> Validating the artifact means running `validate-*.sh` against the artifact.

That is the same shape as §22c's rule one layer up: the instrument was pointed at the
mechanism instead of at the thing the mechanism judges.

### Two more of the same class, both found before a reviewer saw them

**The base-mismatch guard refused correct diffs.** `502970d` compared the pull request's
file list against the diff's `+++ b/` headers. A deletion writes `+++ /dev/null`, so a
deleted file never appears there and the guard would have refused a correct diff for any
pull request that removes one:

```
--- name-only ---     --- old rule (+++ b/) ---   --- new rule (diff --git b-side) ---
added.txt             added.txt                   added.txt
gone.txt                                          gone.txt
keep.txt              keep.txt                    keep.txt
```

It now reads the b-side of each `diff --git a/X b/Y` header, which `git diff --name-only`
agrees with for modify, delete **and** rename. Verified both directions: exact match on
this pull request's 66 files, and it still refuses the wrong-base diff at 182.

The guard had been tested by pointing it at the bad diff and watching it refuse. **That
proves it rejects and says nothing about whether it accepts what it should** — §22c's rule
in the tool that enforces §22c.

**And a claim written in this round's own repair.** The comment explaining the
fold-value assertion said the recovered `PromotingCandidate` "is the only one production
ever uses". Production builds two: `promote` returns one carrying `judged.tree_sha`, and
`recovery_for` builds one from the fold. The distinction that matters is not which is used
but which can be *wrong* — `promote`'s tree is the value it has just written into the
event, so the comparison there is a tautology, while the fold's has been through a
serialization, a replay and an `apply`. The comment now says that instead.

Found by grepping this round's own added prose for verification-language — "the one",
"only", "every", "never" — and checking each against the tree. That is the cheapest
instrument for this class and it belongs in the repair loop, not in the review.

### Round 3's second P1 — the pin binds to the record

`recovery_for` read the prepared pin as `pin.is_some()` and never compared its target to
the commit `candidate_prepared` recorded; `reclaim_after_creation` re-read the target and
deleted **that** value expected-old — a compare-and-swap comparing the ref to itself, which
cannot fail.

So a pin moved from the recorded `C` to some `X` after the settlement left a resume
promoting `C`, appending `task_candidate_created`, and then removing the substituted pin on
the way out. It **succeeded**, and it deleted the one ref that evidenced the substitution.
`DESIGN.md` §15 says the opposite: *"Any substituted or symbolic pin … refuses while
preserving evidence."*

**Both halves now bind to the record.** No new refusal kind: `Refusal::RefAtAnotherSha` is
`T-CAND-REF`'s "ref present at another SHA" and the pin is a ref present at another sha —
the refusal inventory is packet-enumerated, so its message was widened to name the two refs
it serves rather than a variant added.

**The orphan-pin path is deliberately not bound.** With no `candidate_prepared` there is no
recorded commit to bind to, and DESIGN says a pin without a successful settlement "is
orphan residue and is removed without dereferencing symbolic refs". The binding applies
exactly where a record exists to bind to.

**Witness, and the mutation each half dies to.**
`a_substituted_prepared_pin_refuses_and_leaves_the_evidence` reaches the boundary honestly —
commit, pin, `candidate_prepared` — then moves the pin to a real sibling commit and asserts
three things, because "refuses while preserving evidence" is three things: it refuses; the
error names **both** shas, so the substitution is legible from the message alone; and the
pin is still at the impostor, the candidates ref was never created, and nothing was
appended.

| mutation | the witness |
|---|---|
| `recovery_for` reads the pin as `is_some()` again | **FAILED** |
| the prune deletes whatever target it finds again | **FAILED** |

Deliberately **not** a different-tree commit: the sibling shares the judged tree, so the
2026-08-26 tree check cannot catch it and the pin's own binding is what must. A witness
that used a divergent tree would pass on the other repair's account.

### Round 3's third P1 — one probe accounted where ten processes ran

Fresh creation registered a single `probe(agent, 0)` identity around the whole adapter
call and handed the adapter the **raw** `Runner`. A current Codex probe runs ten Runner
requests — version, two help probes, six strict-config probes, the model catalog — so
ordinal 0 was accounted and 1 through 9 were absent.

**Ten, derived rather than quoted.** The review stated the figure and this round restated
it once before checking; the standing rule for this round is that a prose count is computed
or sha-stamped, so it is computed here, at `bcfd1bf`:

| where | requests |
|---|---|
| `codex::probe` directly | **4** — version, fresh `exec` help, `exec resume` help, model catalog |
| `validate_effort_config_key`, called from it | **6** — 2 surfaces (`Fresh`, `Resume`) × (1 unknown-key control + 2 efforts), one `runner.run` per `run_config_parser_probe` |
| | **10** |

**The failure is a wrong row, not a missing one.** With the version probe at ordinal 0
succeeding and a help probe at ordinal 1 failing, the creation ledger recorded **ordinal 0
cancelled**: the identity of the process that *succeeded*, with no record of the one that
failed. `permits.protocol` asks for "registered/completed/cancelled exactly once" per
invocation, and R3's subject is a process.

**Resume already had the answer, and had written down why.**
`preflight::Registering` wraps the Runner and registers each request, and its doc reads:
*"One place, so that 'each a registered invocation' is true of a process an adapter built
as much as of one this module built."* Fresh creation was the other place. It is now
`pub(super)` and both paths use it, so the sentence is true.

**Three things fell out of moving the boundary, and each was a real consequence rather
than tidying.**

1. **P4's own register/slot/settle calls are gone.** They *were* the wrong boundary; the
   wrapper does it per process.
2. **`Request::ledger` and `::slots` became shared locks.** They were `&mut`, and leaving
   them so would have given `create.rs` a *second* ledger: its end-of-module
   `ledger.balances()` would have read an empty one and passed vacuously. One ledger, held
   by both, or the check is theatre.
3. **The R4 half moved out of view, so it is now asserted.** P4 used to acquire and release
   each pair itself, which made "every pair released" visible in this module's code. The
   balance check now tests `slots.held().is_none()` beside it — otherwise `Request::slots`
   would have been an unread field, which the compiler said outright.

**Witness and mutation.** `the_creation_ledger_accounts_every_probe_process` drives an
adapter that runs two processes against a runner that refuses the second, and asserts
`(completed, cancelled) == (1, 1)` — naming `(0, 1)` as the pre-repair reading in the
failure message, so the two accounts are told apart rather than a count being asserted in
isolation. Handing the adapter the raw runner again fails it.

**And the shipped claim it falsified is corrected where it stands.**
`reviews/2026-08-25-pr7-g2-evidence.md` §8 said "every worker, gate, review, re-ask and
probe process carries a typed `InvocationId` … registered exactly once and settled exactly
once". That was false for fresh creation when written and stayed false until now; the
correction is in that file, sha-stamped, and says so — including that it was a reviewer
that found it and not the artifact's own evidence.

### Round 3 on finding A — the behaviour was repaired and the claim about it was not

The round-3 review confirmed `FeedbackCarrier` works: the legacy caller chooses
`LadderEvent`, schema 4 chooses `AttemptRecord`, the feedback no longer reaches the legacy
record or `report.json`, and the strict-door argument for the `"detail":null` residual is
sound. It then found that **the witness did not do what three artifacts said it did**.

The commit message, the PR body and
`decisions/2026-08-26-durable-retry-feedback.md` all said the test *compares against the
bytes `610106b` wrote*. It did not. The captured fixture appeared only as **elided prose**
in a doc comment — `"reason":"gate \`needs-test\` failed: …"` — and the assertions were three
key names plus `reason.starts_with(...)`. A changed reason **suffix** passed.

Two repairs, and the second is the one worth keeping.

**1. The fixture is a constant and the comparison is `assert_eq!` on the bytes.**
`PRE_CHANGE_FAILURE` holds the exact `"failure"` object captured at `610106b`; the test
strips `,"detail":null` and compares byte for byte. Its failure message says to
**re-capture the fixture** if a newer git rewords its pathspec error, rather than to loosen
the comparison — a fixture that may be quietly relaxed is not a fixture.

**2. `is_null()` cannot tell an explicit null from an absent key**, and both halves used
it. `serde_json` returns `Value::Null` for a missing key, so the assertion answered *true*
for a record whose `detail` had stopped serializing altogether — which is a different wire,
and the one schema 4's strict door refuses. Both halves now assert
`object.get("detail") == Some(&Value::Null)`: present, and null.

| mutation | the witness |
|---|---|
| the reason gains a suffix — exactly what `starts_with` could not see | **FAILED** |
| `skip_serializing_if` makes the key absent rather than null | **FAILED** (and it also fails the strict-door witness) |

The second row is the measurement that the old assertion was vacuous in a reachable
direction: under that mutation `failure["detail"].is_null()` was `true` and the test passed.

> A claim in three artifacts that no test makes is worse than no claim, because the
> artifacts are what a reviewer reads first. The repair is to make the test hold the claim,
> not to weaken the claim to what the test happened to check.

### Round 3 on finding C — a count of the wrong thing, and a guard that was not one

Two defects in one repair, both found by the `bf927f3` review.

**The number counted kinds while the doc named shapes.** A `FailureShape` **is** a
`(kind, origin)` pair; `spends_allowance` takes one, and `FailureShape::is_outage` reads the
origin for `Timeout`. So the shape count and the kind count are different numbers —
**13 and 7** — and the previous repair's doc said "seven shapes … not a `FailureKind`
count" while its test collapsed the pairs into a `BTreeSet` of kind names and asserted 7.
The doc and the test disagreed with each other as well as with the authority.

That sentence has now been wrong four times: "every other settlement spends one" (off by
six) → "five kinds" (the outage arm covers three, not one) → "seven shapes" (that is the
kind count) → **13 shapes spanning 7 kinds**, which is what the authority answers. Six of
the seven kinds contribute two shapes each and `Timeout` contributes one, because `Timeout`
is the only kind whose answer depends on the origin.

**And the guard the previous repair described did not exist.** Its comment read *"a new
variant between them fails this list to compile"* — of a 14-element **array literal**,
which compiles perfectly well while an enum grows past it. The same comment was also
inverted: it named `Interrupted` first and `Declined` last, and the enum begins at `NoChain`
and ends at `Interrupted`.

**Two mechanisms replace it, failing in different directions.**

| | catches |
|---|---|
| `every_failure_kind` reads the variant names out of `ladder.rs` between the enum header and its closing brace | a variant that exists but nobody added to a list |
| `kind_of_name` maps each name to a value through an **exhaustive `match`** | a variant that exists but has no value here — the crate stops building |

Both were exercised rather than asserted. Breaking the parse so it finds nothing produces
*"the source read found 0 variants … the parse is broken, not the enum"*; dropping a variant
from the mapping's candidate list produces *"`Interrupted` is a variant of `FailureKind`
that this mapping does not name"*. A source-reading test that silently reads nothing is the
failure mode that matters here, and it now refuses instead.

**The invented constructor name is corrected in both files.** `events::Dangling::event` names
no type; it is `InterruptedAttempt::event`. The name was fabricated in the *correction of a
false claim about that very constructor*, in `classify.rs` and `events/mod.rs` — one round
after a fabricated sha and one before a fabricated test name in `fold.rs`. Every
backticked type and function name added in this round has since been checked to resolve
against the tree.

> Three fabricated identifiers in three consecutive rounds — a sha, a type, a test — all in
> prose written *about* accuracy. The check is mechanical and cheap: grep each backticked
> name for a definition before committing. It is now part of the repair loop.

### Round 4's second P1 — the accounting could be checked against the wrong locks

`Request` carried its own ledger and slots beside a `&dyn Probes` that carried another
pair, and nothing required them to be the same. Probes over locks A, request over empty
locks B: P4 runs through A, creation's closing assertion reads B, finds it vacuously
balanced, and the refusal an operator reads reports no leaked registration whatever A holds.

**Fixed by making the second pair unrepresentable.** The pair lives on the `Probes` seam —
`fn ledger()`, `fn slots()` — and `Request` has none. One owner, and no second for a caller
to supply. That is a compile-time property, so no test demonstrates it; what the tests
demonstrate is that the check reads a **populated** ledger.

**Two witnesses, because one of them cannot discriminate on its own.**

`the_append_error_balance_reads_the_ledger_the_probes_used` drives `create_run` to the
forced first-append error through the **production** `RunnerProbes` — not a recording
double, which registers nothing and would leave an empty ledger that balances for the wrong
reason. Its premise assertion refuses exactly that: `completed() > 0` before `balances()`.
That premise fired on the first draft, which used the double, and is why this test uses the
real probes.

But a balanced run cannot tell the two ledgers apart: an empty one balances too. So
`a_leaked_probe_registration_is_reported_by_the_append_error` drives a `Probes` that
registers an invocation and never settles it, and asserts the refusal **does** carry
"still holds a registered invocation".

| mutation | balanced witness | leaked witness |
|---|---|---|
| the balance check reads a ledger other than the probes' | ok | **FAILED** |

The first column is the measurement that the balanced case is not a witness for this
property at all — which is worth recording, because the round-3 witness was of exactly that
kind and looked sufficient.

### Round 4's third P1 — the successful settlement did not require success

The 2026-08-27 Class B change made `candidate_prepared` the sole **successful** settlement
and `check_candidate_prepared` validated attempt number, base, parent and lease — and
mentioned `failure` nowhere. So a `candidate_prepared` whose embedded `AttemptRecord`
carries `failure: Some(GateFailed)` was accepted, promoted the generation, and was carried
to `task_candidate_created`: a task durably queued as a successful candidate whose own
authoritative evidence says a gate failed.

**The one condition that made the event *successful* was the one condition not enforced**,
in the change that made it the successful settlement. The fold is the authority against
malformed, reconstructed and faulty future writers — not only against this build's driver,
which happens to supply a passing record — and that is the whole argument for a checked
fold.

`prepared.attempt.failure.is_none()` is now required, refused as `InconsistentRecord`
rather than a new variant, because the inventory is packet-enumerated and "the event
disagrees with the record it cites" is exactly this kind (P1-2's rule, applied again).

**It also earns a property the driver had been assuming.** `Brief::replay` walks settlements
and takes a `candidate_prepared` record to carry no feedback — true because it carries no
failure, which until now nothing checked.

`a_candidate_prepared_whose_record_failed_is_refused` drives the review's five steps, and
asserts its own premise first — the same event with a passing record **is** accepted — so
the refusal is about the failure and not about anything else in the fixture. It then asserts
nothing moved: the generation is still `InFlight` with no candidate.

| mutation | the witness |
|---|---|
| the door stops requiring success | **FAILED** |

### Round 4's docs finding — and the identifier check that was too weak

**Five production comments still prescribed the settlement the fold now refuses.** The
2026-08-27 ruling changed the code and not the prose around it: `candidate.rs`'s and
`attempt.rs`'s module headers, `run.rs`'s candidate-sequence doc, `settle.rs`'s lease note,
and `recover.rs`'s continuation doc all described `attempt_finished(succeeded)` between the
pin and `candidate_prepared` — two of them as the thing that *makes* the generation
`Promoting`, which is now the opposite of true. All five are rewritten to the ruled
semantics, each saying what it used to say and why that is wrong.

**The fourth fabricated identifier.** `CandidateRecovery::SettleInterrupted` — a struct with
a `settles_interrupted: bool` field and no such associated item. And `events::Dangling::event`,
reported corrected "in both files" in round 3, survived in
`decisions/2026-08-26-durable-retry-feedback.md` — the **immutable** artifact, and the one a
reader reaches first. Three places, and I checked two.

**The check that was supposed to prevent this was too weak, and the two names show how.**
Round 3's rule was that a backticked name must *occur* in the tree. `Dangling` occurred —
in the prose that invented it. `CandidateRecovery` occurs, so the fabricated associated item
would have passed on its prefix. Occurrence is not definition.

`~/tactus-artifacts/pr7/drivers/idcheck.sh` now requires a **definition site**: a Rust item
(`fn`/`struct`/`enum`/`trait`/`type`/`const`/`static`/`mod`), an enum variant, a struct
field, or — for an event kind, which has no Rust item — its wire name in one of the two
vocabularies.

**It took two corrections of its own, and the first is the instructive one.** The check
first resolved a path by its **leaf**, and on that rule `events::Dangling::event` *passes*:
`event` is defined all over the tree. It would have accepted the exact fabricated path it
was written to catch, and I only found that by running the control instead of assuming it.
It now checks **every segment** and names the one that fails. Second: its scope excludes
`reviews/**`, because a review record and this ledger have to be able to name a fabricated
identifier in order to record that it was fabricated — an unresolved name there is the
artifact doing its job.

Controls, both run: `events::Dangling::event` is refused at segment `Dangling`, and
`CandidateRecovery::SettleInterrupted` at segment `SettleInterrupted`. A run over this
round's `src/**` and `decisions/**` is clean.

It also flagged something worth keeping: **`complete_promotions` and `settle_succeeded`**,
narrated in new comments as history. They are deleted, so they do not resolve — and
formatting a deleted item as a code path is what implies it still exists. They are now plain
prose, and the check has **no exceptions**.

**And the "never patched to pass" claim was false.** `Journal::settle_succeeded` was made an
explicit no-op and left at its call sites so the fixtures reaching it would pass untouched.
The helper and all **seven** call sites are now gone — `git grep -c '\.settle_succeeded()'
5ccc8f5^ -- src/` returns `7`, and the round-5 record's own count of "nine" was the fourth
uncomputed number this branch published. Each fixture's sequence is
`task_dispatched → attempt_started → candidate_prepared`. They assert the invariant rather
than tolerating it: making `apply_candidate_prepared` stop promoting fails **five** of them,
which a no-op standing in for the step made impossible. Re-measured at `23958c3` after this
round's fixture changes — deleting `generation.class = GenerationClass::Promoting` from
`apply_candidate_prepared` fails **20** tests suite-wide, of which `grep -c
'engine::topology::candidate::tests'` over the failure list returns **5**. The round-3 claim is corrected
where it stands, in the §3 appendix that made it.

### Round 4's P2 — four body claims the tree contradicted

Each corrected where it stands, and the second is the one that mattered.

**The head stamp.** Validation read *"Local, at `327cce3` — the head this body describes"*,
seven commits behind and predating every repair the review was reading. Scope and Review
evidence had been updated and Validation had not — the same section-by-section drift the
one-declared-basis rule exists to stop, one section over.

**"No event kind, serialization, or transition changed" was false twice**, and the honest
statement is longer than the false one:

* the legacy schema-3 `failure` object gains `"detail":null` — a **constant, content-free**
  key, since the legacy carrier is `ladder_retry` and the record's copy is always `None`
  there. No reader's behaviour changes; the growth is one null per failed attempt. This
  branch's own byte witness against `610106b` is what makes that precise rather than
  asserted;
* the accepted schema-4 **transition shape** changed, and this one is **forward-only**: a
  log this head writes settles a success with `candidate_prepared` alone, and the
  immediately preceding build's fold required `Promoting` first and would refuse it.

The second costs nothing today — schema 4 has no external writers and no shipped command
writes it — but "revert and every old log still reads" is exactly what a rollback claim
rests on, and for schema-4 logs written *by this head* it does not hold. Disclosed rather
than reasoned away.

**The G2 stamp named the wrong commit.** The correction was stamped `8f0e605`, which touches
no `create.rs`; the creation repair is `35aaf8e`, one commit later. A sha stamp exists so a
reader can go and look, and one that points at a commit without the change is worse than
none. Corrected to `35aaf8e`, saying what it first said.

**"A substitution refuses without touching anything" was too strong.** True on the
`recovery_for` window. On the late window — substitution after the candidates ref and
`task_candidate_created` are written — `reclaim_after_creation` refuses and preserves the
pin, but those two effects have already landed. The security property holds on both windows;
the absolute claim held on one, and the body now says which.

### The self-audit sweep over the reviewer's not-read list

**The reviewer publishes next-round targets every round.** Its coverage declaration names
what it did not read, which is the cheapest convergence available: read them first, fix what
they hold, and the next review spends its budget elsewhere. Run before round 5, at `021edf7`,
over the files round 4 declared unread — **7,337 lines across seven files**: `startup.rs`
and its tests, `scaffold.rs`, `seams.rs`, `run/tests.rs`, `dispatch.rs` and its tests.

**What it found was all in the instruments, not the code.**

*The identifier check had two more defects, either of which would have produced a false
verdict.* Run over whole files rather than a diff, it flagged eighteen `std::` paths: it
skipped the `std` segment and then asked this repository to define `fs`, `process`,
`SystemTime`. A path rooted in the standard library is now skipped whole. **That flaw was
live in the committed check** and would have fired on the first future round whose added
prose mentioned `std::fs::write`.

It then flagged one real bare mention — `SystemTime::now` in `seams.rs`, where the *subject*
of a grep-checkable rule is deliberately unqualified. **The rule was audited and holds**:
nothing under `src/engine/topology/**` or `src/topology/**` calls it outside comments, and
`util::rfc3339_utc_now` is where the clock is read. Left as prose, because qualifying it
would break the grep needle the rule is about, and the diff-scoped gate never sees the line.

**What it checked in the code and found sound**, each verified rather than read past:

| claim | verified |
|---|---|
| `seams.rs`: "five effect-hook families" | `TopologyHooks` exposes exactly five — effects, rundir, events, container, spawn |
| `startup.rs`: calls step (a) and "does not reimplement it" | `run_startup_census` called at `startup.rs:676`, defined in `runner/container/census.rs`; no container-runtime or label scan outside comments |
| `dispatch.rs`: `closing_disposition` reads the lease rule rather than restating it | it calls `expected(false)`, and is still correct after `check_lease_disposition` lost its `survives` parameter |
| `run/tests.rs`: the pool seam's "only caller was `run.rs`" | historical narration, sha-stamped, and it states what its census cannot see |
| `scaffold.rs`: `durable_at_spawn` is "the only oracle O23 has" | carries its own measurement — the test stayed green with the append moved after the spawn until the field existed |

**And nothing in the sweep set was staled by rounds 3 and 4.** No reference to the removed
successful `attempt_finished`, the deleted convergence, `Recovered::promoted`,
`attempts_on_rung` or the settlement move appears in any of the seven files — established by
grep rather than by reading, so the absence is a measurement.

**What this sweep cannot see, stated rather than left to be found.** It checks identifiers,
countable claims, and claims about other modules that are greppable. It does not check
witness *quality* — whether a test drives the step it names or constructs its input — which
is the class that produced findings in rounds 2, 3 and 4 and has needed a reviewer every
time.


## 22d. The re-review of `b1f54a5`, and what a gate can hold that a reader cannot

Round 5 returned seven findings. **Three were in the fold's doors and the probe seam — the
places round 4's repairs had touched — and four were prose.** The pattern is now five rounds
old and stated plainly: *each round's findings are defects in the previous round's repairs*,
and the prose half of the crop has never once been caught by a person re-reading.

**The doors enforced half a definition each.** `check_candidate_prepared` asked
`failure.is_none()`; `check_attempt_finished` asked nothing beyond refusing `Succeeded`.
A record can carry no failure and still hold a review whose outcome is `Failed` or
`Unavailable` — §11.2 requires every configured pass to pass, and a reviewer that could not
run says nothing about the code — so a rejected attempt was promoted, charged against its
rung allowance and queued as a candidate. `AttemptRecord::is_successful` is now the one
derivation and both doors ask it. This is the third application of "one derivation, not two"
in this slice, after the rung allowance and the settlement counting.

**And the positive premises were vacuous, which is why no test noticed.** `reviews:
Vec::new()` satisfies an `all` over review outcomes because `all` never sees a pass; two
fixtures carried a lone `second-opinion` entry with no primary pass at all. Delete the review
clause from `is_successful` and not one positive witness would have failed. They now build a
complete successful attempt under the frozen plan, with `TaskKey` read as the plan index so
the second opinion is derived from `review_plan` rather than asserted by the fixture.

**"A second pair is unrepresentable" was false for the third round running, and is retracted
rather than restated.** The trait exposed `ledger()` and `slots()`, so any implementor could
return a pair of its own. Those accessors are deleted; `Request` owns the single pair and
passes it as arguments. What made the retraction necessary is worth keeping separately from
the fix: **a property asserted three times and refuted three times is not a property, and the
fourth assertion is the defect.**

**Two dead public methods could write an arbitrary path.** `commit_identity`, documented as a
read and classified `effect_free`, ran `git show --output=<interpolated>`. Both are deleted
with their `effects/wrappers.toml` entries rather than repaired by validating the argument:
neither had a caller, so a check would have kept a dead escape alive behind it.

### What is now gated rather than re-read

Two censuses, because the prose class has survived five rounds of people looking at it.

`drivers/deleted-mechanisms.sh`, in the pre-push loop, over seven retired names. It asserts
two things, since "zero occurrences" is not the invariant: **zero as code**, and every
surviving mention accompanied by deletion language — the tombstone comments that tell a reader
where a function went are worth keeping, and a gate demanding literal zero would delete the
signpost.

**Its three wrong widths are the record's actual content**, because each was a plausible
design that measurement refuted:

| width | why it looked right | what it missed |
|---|---|---|
| the line containing the claim | `grep` is line-based and so is every other check here | doc comments wrap. *"without `attempt_finished(Succeeded)` the generation never / reaches `Promoting`"* is two innocent lines — and is the exact sentence round 5 named |
| ±3 lines of a joined comment run | a tombstone's "deleted" sits near the name | a long block's ruling citation is 20 lines from its first line |
| the whole joined run | a block that says "deleted" is a tombstone | a block that corrects itself in paragraph 2 was then licensed to assert the thing in paragraph 5 — which is precisely the shape round 5 found |
| **the claim's own sentence, plus the next** | — | this is the width at which a tombstone and an assertion differ |

The second is §22's own rule, applied without exception: **every count in prose carries the
command that computes it.** `settle_succeeded` is seven, not nine. `Admitted` excludes three
of `Step`'s eight variants, not two of seven — found by the extended sweep, and now held by
`every_step_variant_is_admitted_or_refused_and_the_split_is_five_three`, whose `match` has no
wildcard arm, so a ninth variant does not compile until someone says which side it falls on.
A doc comment cannot enforce a count; that is the whole reason the count kept being wrong.

**What the sweep found and deliberately did not repair.** Every dead `pub fn` it reports is
pre-existing on the merge base rather than added here, and the two remaining `--flag={}` git
arguments take a `rev-parse` OID and a branch name behind a `--` terminator — neither is the
`commit_identity` class. Both are recorded as debt rather than widened into this slice.

Measured at `5a442db`.


## 22e. Round 6, the stop condition, and the three items that go to G2

Round 6 of `cfa1be8` returned **CHANGES_REQUIRED** with three P1s: two in the fold's
settlement doors and one in the probe coupling. The standing stop condition names exactly
those and it has fired, so **there is no round 7**. The three are dispositioned to the G2
pass below, the residue is dispositioned with reasons, and the merge decision goes to the
owner on that basis rather than being repaired into another round.

**Why the condition was set where it was, restated because the numbers now support it.**
Six rounds, six CHANGES_REQUIRED. Each round's crop was dominated by defects in the previous
round's repairs. The three areas the condition names are the three that have recurred in
every round since the fourth, and round 6 shows why re-attempting them in place does not
converge: **each repair was correct about the instance it was shown and wrong about the
class**, and the next round found the class one step over.

### The three, and what each actually is

| # | what round 6 found | what the previous rounds' repairs got right, and what they missed |
|---|---|---|
| 1 | `AttemptRecord::is_successful` never consults the task's **frozen `FrozenReviews`**. It asks `failure.is_none()` and `all()` over *the passes that happen to be present*, so a record carrying a lone passed `second-opinion` — or an empty list — is "successful". A `candidate_prepared` whose primary reviewer never ran is admitted, charged, and promoted | Round 6 fixed the *outcome* half: a pass recorded `Failed` or `Unavailable` is now refused, with witnesses. It did not fix the *presence* half. The round's own fixture comment says the lone-second-opinion shape satisfied the predicate — **and only the fixture was repaired.** Every new witness changes an existing pass's outcome; none removes a configured pass |
| 2 | The repair sits inside the `Closed` arm only. The **`Retained` arm** checks the epoch and nothing else, so a current-epoch retained settlement may carry a record with `failure: None`, all-passing reviews, and an attempt number that is not the envelope's. `is_failed` has **no caller at all** | The `Closed` arm is genuinely fixed and its four witnesses drive it. `Retained` was never in view: every new refusal witness constructs `Closed`, and `scaffold.rs` already emits a retained record with `failure: None` and no reviews, which is the missing check demonstrated in-tree |
| 3 | Passing `ledger` and `slots` as **arguments** does not oblige an implementation to use them. An implementor can run its processes through its own pair and let the closing balance inspect the supplied one. `ContainerProbes` already ignores both arguments while running a real shell process | Deleting `ledger()`/`slots()` from the trait was correct and is kept. But the doc **retracts the compile-time claim and then restates it two paragraphs later** — "there is no second pair … a property of the signature rather than of any implementation" — which is the fourth assertion of a claim refuted three times. The production `RunnerProbes` is coherent; the guarantee is not signature-level |

**The common shape, which is the finding worth carrying forward.** In all three, a repair
established the property *for the path the previous review walked* and left the sibling path
untouched: the `Closed` arm and not `Retained`; the outcome of a present pass and not the
presence of a configured one; the removal of an accessor and not the obligation to use what
replaces it. **A door is not fixed until every arm through it asks the same question**, and
"one derivation, not two" — which this slice applied three times — is necessary and not
sufficient: one derivation asked on one of two paths is still one path unguarded.

### Recorded as G2-pass work items

Fold-door semantics are already the G2 PR3-layer pass's, W1, by the same assignment
`TASK-DISPATCHED-REGION-UNVALIDATED` carries in §2 — a fold-side refusal recorded rather than
repaired because `src/topology/**` is closed to this slice beyond its per-instance approvals.
These three join it, with the repair each needs stated so the next owner does not re-derive it.

| ID | owner | the repair, stated | why not here |
|---|---|---|---|
| `PR7-G2-W1-SUCCESS-IGNORES-THE-FROZEN-PLAN` | G2 PR3-layer pass, W1 | `check_candidate_prepared` compares the record's passes against the task's `FrozenReviews` — every configured pass present **and** passed — rather than `all()` over whatever is present. The predicate needs the plan, which `AttemptRecord` does not carry, so this is a fold-side check taking `(record, frozen)` and not a method on the record | A third Class B change to `src/topology/fold.rs` in one slice, on a door already carrying three per-instance approvals, at the end of a sixth repair round. The stop condition exists to prevent exactly that |
| `PR7-G2-W1-RETAINED-ARM-UNGUARDED` | G2 PR3-layer pass, W1 | The `Retained` arm asks the same two questions the `Closed` arm asks — the record's attempt equals the envelope's, and the record's claim matches the settlement's kind — with `is_failed` acquiring the caller it currently lacks, or being deleted if the arm's answer is that a retained record makes no success claim at all. **That question is the repair's first decision and it is not obvious**: a retained attempt is unsettled, so "failed" may be the wrong assertion to require | Same door, same slice, same reason. And the semantic question above is a design decision, not a mechanical fix |
| `PR7-G2-W1-PROBE-PAIR-NOT-OBLIGED` | G2 PR3-layer pass, W1 | Make the obligation structural rather than documentary: the registration wrapper is constructed **by the caller** from its own pair and handed to the probe as the only thing it can register through, so an implementation has nothing else to register into. Then the claim is about the type a probe receives rather than about arguments it may ignore | The claim is now **retracted and not replaced**, which is the honest state. A fourth attempt to phrase the guarantee is what the stop condition forbids; a structural change to the pre-flight seam at the end of round 6 is what it forbids more |

**What is true today, for the record and without a guarantee attached**: `RunnerProbes` is
production's only `Probes` implementation, it uses the pair it is handed, and the closing
balance reads that same pair. The three doubles that ignore the arguments are tests. Nothing
in production constructs a second pair. That is a property of the code as written, not of the
signature, and it is written down here so no one has to re-derive it from a doc comment that
has been wrong four times.

### The residue, dispositioned

| finding | severity | disposition | reason |
|---|---|---|---|
| 4. `src/events/mod.rs +91/−0` classified wholly Class C, when 30 of those lines are the Class B predicate pair, and `is_failed` is public and unused | P2 | **Accepted, and it is the scope rule** | The reviewer is right that the numerical total re-derives and the *semantic* classification does not. An unused public method is scope this change did not need. It is not repaired here because the repair is either "add the caller", which is G2 item 2, or "delete `is_failed`", which is a frozen-layer edit in the round the stop condition closed. Carried into item 2, whose first decision is precisely whether the `Retained` arm wants that caller |
| 5. Neither new gate supports its claims: `deleted-mechanisms.sh` uses a ±3-line window for the six names while only its one prose regex is sentence-level; its code scan strips from the first `//`, so `let _ = "removed //"; settle_succeeded();` evades both halves; it advertises a `--selftest` it does not implement. `idcheck.sh` resolves qualified paths segment-by-segment, so the very identifier it was extended to catch — `TopologyFold::charge_allowance` — still passes, and pass 1 is not green over the repair range | P2 | **Accepted in full. The gates are weaker than the body says, and the body is corrected rather than the gates** | Every part of this is true and was checkable before it was published. The `--selftest` line is the sharpest: a gate written to catch false claims about mechanisms carried a false claim about itself, in its own header. These are external drivers, not repository code, so no source change is owed — but the PR body's description of them is a claim about evidence and it is now narrowed to what they actually do. **The segment-by-segment hole is the one worth naming**: a path check that resolves each segment independently cannot distinguish `Type::method` from `method defined on some other type`, which is the entire class it exists to catch, so it has never actually caught it |
| 6. Four prose defects in the repairs themselves: the ledger says "`RunState::charge_allowance` does not exist (the method is on `impl RunState`)"; a comment says the helper records `failure: None` while the repaired helper sets `Some`; a new comment publishes "~8 fixtures" with no command against 11 call sites; the Windows paragraph's range and its command disagree | P2 | **Accepted** | The first is a self-contradicting sentence: the fabricated path was `TopologyFold::charge_allowance`, and correcting the path without correcting the sentence left it asserting that the *correct* name does not exist because it is on the impl it is on. The rest are the same class the round was meant to close, produced by the repairs that closed it |
| Self-found before the verdict, not in the review: `create.rs`'s balance comment said it reads the pair "through the probes, which own them", and the leak witness's doc said "the probes' own ledger" — both describing the arrangement the round's own deletion removed | — | **Fixed at `96a4ed4`, held unpushed** | Found while auditing the probe coupling against the stop condition. Committed locally and deliberately not pushed: a push invalidates a review in flight. It is also the limit of the new gate, stated plainly — the gate's vocabulary is seven retired *names*, and "the probes own the pair" is a retired *arrangement* with no retired identifier in it |

**What round 6 confirmed sound**, quoted because a review that only lists failures is not a
measurement: the successful-candidate allowance charges live and on replay; the three new
`Closed`-settlement refusals drive the door they name; the five stale mechanisms named by
round 5 are corrected or deleted; `commit_identity` and `changed_paths_between` and their
wrapper entries are gone; `+1916/−186`, `+91/−0` and the seven call sites all re-derive; no
decision record was edited; and there is no added panicking `unwrap`/`expect`, no non-binary
`anyhow`, and no non-`std::path` path handling.


## 22a. A driver that fails silently on a diff this size

Recorded here because it was found while launching the frontier review and it would have
produced a verdict on nothing.

`~/bin/review-pr.sh` fetches the change it reviews with `gh pr diff <n> > "$work/pr.diff"`.
For this pull request that command **fails**:

```
$ gh pr diff 31 --repo eventloops/upstroke
could not find pull request diff: HTTP 406: Sorry, the diff exceeded the maximum
number of lines (20000)
```

The slice is **53,464 diff lines / 2.42 MB across 59 files** (`git diff <merge-base>...HEAD`
at `75da796`), and GitHub's API refuses a diff over 20,000 lines. The script runs under
`set -uo pipefail` **without `-e`**, so the failure is not fatal: it writes a **zero-byte**
`pr.diff`, prints `diff: 0 lines`, and pipes a prompt containing no change at all to the
frontier model — which would answer, plausibly and uselessly, `VERDICT: PASS`.

**A gate that fails by default teaches people to ignore it** — the script's own comment says
exactly that about a timeout it once had. This is the same failure one input over, and it
fails in the *passing* direction, which is worse.

The script is the owner's and is recorded rather than edited. What a caller must do until it
is fixed: assemble the diff locally, and **check it is non-empty before believing a verdict**.

## 23. S5's convergence claim, narrowed to what was measured — and withdrawn as a merge claim

> **Narrowed 2026-08-26, after the frontier review of `75da796` returned
> CHANGES_REQUIRED.** What this section originally said — that S5 converged — was
> a claim about **the slice**, and the sweep it rests on measured **the prose two
> commits added**. Those are not the same scope, and the review found four
> unversioned false property claims and a witness-validity failure outside the
> swept region. §22b carries them.
>
> The narrow claim below is what the evidence supports and is all that is now
> asserted: **the in-house rounds converged on the region they read.** They are
> not a merge gate and never were — `MAINTAINING.md` makes the frontier review
> that, and it is the instrument that found what six rounds did not.

**The in-house rounds converged on what they read**, and the word is scoped,
because an unscoped one would be a claim this project has spent six rounds
learning not to make — and made anyway, here, until a reviewer measured it.

### What "admissible" meant

A finding was admissible if it was one of three things:

1. **behaviour** — a run doing something a live packet passage, an invariant, a fault-matrix
   row or the code's own stated guarantee forbids;
2. **witness-validity** — a test that does not hold the property it names, including one
   scoped to the instance rather than the class, or to an instrument rather than its use;
3. **an unversioned false claim** — a verification-language assertion that was false at the
   head it shipped on.

### Where each stands

| class | round 4 | round 5 | round 6 |
|---|---|---|---|
| behaviour | 3 | **0** | **0** |
| witness-validity | — | 2 (both P1) | **0** |
| unversioned false claims | 8 | 12 | 11 |

**Behaviour has been zero for two rounds. Witness-validity is zero this round.** The third
class did not fall — but it stopped being *unversioned*: every one of round 6's eleven was
a citation that was **true when written and stale one commit later**, which is a different
defect from a claim that was never true, and it is the one §22's rule governs:

> A doc comment or a ledger row states a **property**; a **measurement** goes in a test, or
> is stamped with the sha it was taken at.

From `d17bcf2` on, evidence in this repository is rule-governed rather than checked round by
round. That is what converged: not "no more findings", but **no more findings of a kind a
round is the right instrument for**.

### The closing act

A mechanical compliance sweep over round 6's own repair diff and the ledger row after it —
`d17bcf2` and `4247255`, no lenses — asking of every prose claim in them only whether it is
test-borne or carries its sha. `reviews/2026-08-26-pr7-s5-closing-sweep.md` is the result in
full: 24 hex tokens, 5 `file:line` citations and every numeric claim, checked one at a time.

**Nine stampings, no moves, no repairs, no retractions.** All eleven sha256 blob hashes
re-derive; twelve of thirteen commit references are ancestors and the thirteenth is the
*subject* of a finding rather than a citation; two counts were already test-borne
(`seventeen` modules, `ten` operators) and stay in tests; three totals that come from
session artifacts are now named unverifiable-by-construction rather than quoted as facts.

### What the frontier review then found, and what that says about the definition

Round 6 reported zero in the first two admissible classes. The frontier review, over
the same head, found **one witness-validity failure** — a two-rung exhaustion telling
the operator "1 attempt(s) across 1 rung(s)", with no topology test asserting `rung(s)`
at all — and **four unversioned false claims**. Both are categories this section
declared empty.

**The definition was not wrong; the coverage behind it was.** Six rounds read repair
diffs — each round the previous round's changes — and the four false claims and the
`rungs_spent` constant all predate the region any of them read. A round scoped to a
repair diff cannot find a defect in code the repair did not touch, and six such rounds
in sequence never widen. That is the honest limit, and it is why the sequence ends with
an independent reader over the whole slice rather than a seventh round.

### What rides with it

`PR7-WIN-READ-RACING-BOUND-TOO-SHORT` in §2 is **open, measured, and owned** — two of four
full-suite guest runs red at `d17bcf2`, two different tests, one captured errno and one
presumption, `pre_existing` from `919a728`. It is not a loose end being smuggled past a
convergence claim: a reviewer meets it as numbers, with what a repair would have to decide
and what measurement to demand of it. Carrying a flake with its rate rather than a
description is §12's precedent.

`PR7-R4-LOOP-004` (`Closure(NotEnding)` on the ending path) remains carried to PR8/PR10 per
§20, and round 6's `contract` lens re-examined that disposition and found it sound.

### The head

`d17bcf2` is the head with a **complete, uncancelled CI run** — nine jobs, all success,
including `test (windows-latest)`, `test (macos-latest)` and all three MSRV legs — because
the push after it was held while it executed. Everything past it is this section, the sweep
record and the stampings it produced.
