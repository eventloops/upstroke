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
| PR4-PROGRAM-PATH-NOT-UNICODE-CLOSED | **Supersedes the row above; that row is left as written, per this file's append-only rule.** The question — whether a non-Unicode agent path is representable or refused at the boundary — is **closed as not reproducible in production**. | project owner | **Closed, not repaired: there is nothing to repair.** The premise was that a resolved agent-binary path reaches `CommandSpec.program` and is refused there. It cannot. `Invocation::at`, the constructor that takes a path, is `#[cfg(test)]` and says so in its own doc — *"Production's only constructor is `Invocation::named`, whose argument is a bare CLI name"* — and both its call sites are inside test modules. `Invocation::named` takes a `&str`, so `Invocation::spec`'s `to_str()` cannot return `None` for anything production builds. `runner/mod.rs` states that **a `String` was always wide enough** for a bare name, and `runner/host.rs` splits `PATH` with `std::env::split_paths` over an `OsStr` and joins the name to each entry, so a CLI installed under a non-UTF-8 directory is found and executed today and the resolution is never written back into the field. The conflict `CODING_STANDARDS.md` §1 records between `DESIGN.md:222` and §8 dissolves for the same reason: §8 governs paths, and this field holds a name. **`DESIGN.md:222` is unchanged**, the widening scheduled to the G2 pass's W4 is withdrawn, and `a_program_path_a_string_cannot_carry_is_refused_by_name` **stays** — it guards a state production cannot construct, which is what makes adding a path-valued constructor safe later. Reasoning and rejected alternatives: `decisions/2026-08-25-commandspec-program-stays-string.md`. Found by the frontier review of `1de9131`, whose two predecessors had each refuted a different attempt to re-ground the widening. |
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
| PR40-PROGRAM-PUBLIC-ADAPTER-SEAM | **The `CommandSpec.program` closure is scoped to this repository's adapters and does not reach the crate's public construction seam.** `decisions/2026-08-25-commandspec-program-stays-string.md` closes `PR4-PROGRAM-PATH-NOT-UNICODE` on the evidence that `Invocation::at` is `#[cfg(test)]` and `Invocation::named` takes a `&str`, so no adapter *in this repository* puts a path in the field. That audit does not reach `AgentAdapter`, which is public: `src/lib.rs` declares `pub mod agent`, `src/agent/mod.rs:194` declares `pub trait AgentAdapter` whose `build` returns a data-only `CommandSpec`, and `src/engine/mod.rs:83` declares `pub fn run_with(opts, adapters: &dyn AdapterSource)`. Failure sequence: a downstream crate implements `AgentAdapter` -> it is configured with a Unix agent path whose bytes are not valid UTF-8, such as `/opt/agent-\xff/claude` -> `build` must place that path in `program: String` -> `to_str()` returns `None` and the boundary refuses, or `to_string_lossy()` substitutes `U+FFFD` -> the run refuses with a diagnostic naming no missing installation, or the runner spawns a path that names a different file. `agent::bin::tests::a_program_path_a_string_cannot_carry_is_refused_by_name` guards `Invocation` and places no constraint on a direct `CommandSpec` construction | project owner, carried by G2 W4 | **Accepted as real and deferred, not fixed — owner disposition 2026-08-29.** Found by the frontier review of `7cf4f9971e2b4a8712ca7afa11e129c734921173`, verdict CHANGES_REQUIRED. Deferred deliberately: the repair is a decision about whether the public adapter seam may carry a path at all, which is a question about the shipped API rather than a defect in the documents this pull request lands, and W4 is the venue that owns `CommandSpec.program`. **Revisit at G2 W4**, and sooner if an adapter outside this repository is written against `AgentAdapter`, or if a path-valued adapter or configuration input is added. Until then the closure this pull request lands is to be read as scoped to this repository's adapters, never as a statement about the type |
| PR4-PROGRAM-PATH-NOT-UNICODE-CLOSED-NARROWED | **Supersedes `PR4-PROGRAM-PATH-NOT-UNICODE-CLOSED` above; that row is left exactly as written, per this file's append-only rule.** The superseded row says the premise "cannot" hold, and that the `CODING_STANDARDS.md` §1 conflict "dissolves for the same reason: §8 governs paths, and this field holds a name". That is a claim about the type, and it is wider than the evidence. The evidence establishes only that no constructor in this repository puts a path in the field. The boundary is path-capable by contract: `src/runner/host.rs:828` turns on whether `program` is a name for the boundary to resolve rather than a location to use as given, and hands a location to `Command` byte for byte; and the retained test at `src/agent/bin.rs:496` asserts `/usr/local/bin/claude` in `fine.program`. Failure sequence for the uncorrected wording: an implementer reads the standing ledger -> takes "this field holds a name" as the field's contract -> converts a path-valued input with `to_string_lossy()` on the ground that §8 does not govern this field -> a non-Unicode installation is silently renamed rather than refused. The claim that binds is the one `decisions/2026-08-25-commandspec-program-stays-string.md` carries: every route this repository takes puts a bare name in the field, so the conflict has no reachable instance today, and §8 governs the field the moment a path-valued input exists | project owner | **Correction appended rather than applied in place — owner disposition 2026-08-29.** Raised as a P2 by the frontier review of `7cf4f9971e2b4a8712ca7afa11e129c734921173`, which found the final narrowing had reached the decision record, its index entry and the pass proposal but not this file. Closed by this row. Read the two rows together: the disposition of `PR4-PROGRAM-PATH-NOT-UNICODE` is unchanged and remains closed as not reproducible in production, and it is this row's scope statement that binds. See also `PR40-PROGRAM-PUBLIC-ADAPTER-SEAM` |
| PR40-CHARTER-BINDS-A-PROPOSAL | **Two live passages disagree about whether a proposal can bind, and this pull request lands both.** `proposals/README.md:22` states the folder contract — "**DESIGN.md remains the only living authority for product design.** A proposal binds nothing." `decisions/2026-08-24-pr3-layer-freeze-charter.md:169` states the opposite for one proposal: `proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md` "is the pass's plan; it binds when this record lands and cites it". The only `DESIGN.md` edit this pull request makes records sequencing and links to the plan; it does not carry the plan's content. Failure sequence: the G2 pass opens -> one implementer reads `DESIGN.md` as the sole living authority and inherits only that the PR3-layer pass precedes PR8 -> another reads the charter, treats the proposal as binding, and inherits W1 through W10 with their exit criteria -> the two build to different scopes, and each can correctly cite a governing document against the other. Compounding: `decisions/README.md:17` makes a landed record immutable, so after merge the charter's sentence can be superseded by a dated appended section or a successor record but never edited | project owner, carried by the documentation-authority pass | **Accepted as real and deferred, not fixed — owner disposition 2026-08-29.** Found by the frontier review of `7cf4f9971e2b4a8712ca7afa11e129c734921173`, verdict CHANGES_REQUIRED. Deferred deliberately: the repair is a ruling about how a charter may confer authority on a plan, which reaches `proposals/README.md`, `decisions/README.md` and `DESIGN.md` together, and is wider than the documents this pull request lands. **Revisit in the documentation-authority pass**, and sooner if any slice cites the proposal as binding. Until that ruling, `DESIGN.md` governs, the proposal binds nothing, and the charter's sentence is to be read as scheduling the pass rather than as conferring authority on the plan |
| PR7-STD-PRIVATE-ROOT-LEXICAL-COMPARE | The explicit private-root comparison falls back to lexical equality on every canonicalization error (`src/engine/topology/recover.rs`) | project owner | **Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's MUST-tagged fail-closed bullet — *"Security-sensitive comparisons and decisions MUST fail closed on malformed, contradictory, or unavailable evidence; availability fallbacks must not silently grant more authority."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — the two `normalize` calls are independent and each falls back only for the path that failed to canonicalize -> canonicalization of the **recorded** root fails, transiently or on a mount that refuses it, so that side degrades from an identity to a spelling while the explicit root canonicalizes normally -> the comparison is then between a resolved path and a spelling, so an explicit root that merely spells the same is accepted and one that resolves to the same place through a symlink is refused -> the decision reaches the opposite verdict from the one it exists to reach, in the direction of granting rather than refusing, on evidence that was unavailable **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region 1 of 2** — the decision site, the `--private-root` comparison itself, `3e5212d` `recover.rs` 338-342: `5289194ca998e04b98b33aba06400b2abab199a6fdce2c9737693e326f6990c5`. **Region 2 of 2** — its documented rationale and the `normalize` helper it justifies, 460-470: `74cb133ae3a953d0c6a7e7dcf8c25c445203f0cbe52f457c309645e4963b555f`. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too. |
| PR7-STD-OWNER-RECORD-LEXICAL-AUTH | Owner-record public-directory authentication falls back to lexical spelling when canonical evidence is unavailable (`src/engine/topology/recover.rs`) | project owner | **Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's MUST-tagged fail-closed bullet — *"Security-sensitive comparisons and decisions MUST fail closed on malformed, contradictory, or unavailable evidence; availability fallbacks must not silently grant more authority."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — the record is written while a spelling denotes one directory -> that spelling is later retargeted to another, by replacing a symlink or a mount, while the run id, repo key and incarnation still match -> the filesystem then refuses to canonicalize the public directory, so `canonical_display` returns the spelling on both the recorded and the live side -> string equality authenticates the **new** directory against the old directory's owner record, and the disagreement refusal never fires. The retarget step is stated because equal spellings alone do not establish two directories, and an earlier revision of this row jumped straight from the canonicalization failure to the conclusion **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region 1 of 2** — the decision site, the owner-record public-directory check, `3e5212d` `recover.rs` 632-634: `a332d47443baaa6c12f1f74ee47e06a6b654e16a6bbbd731261d1de46971fb75`. **Region 2 of 2** — its documented rationale and the `canonical_display` helper it justifies, 751-758: `1366553bf35fea1422476857aa79e3b8ac7c76e7e77011c01e84efcea7d0abb1`. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too. |
| PR7-STD-PRIVATE-ROOT-NO-CONTAINMENT | The recorded private-root locator is accepted without absolute-path or symlink/reparse-point containment validation (`src/engine/topology/recover.rs`) | project owner | **Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §8's MUST-tagged containment bullet — *"Path containment checks MUST account for `..`, absolute paths, symlinks/reparse points, and platform-specific prefixes as appropriate to the security boundary. Lexical normalization alone does not prove filesystem containment."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§§8–9 filesystem, persistence, and processes** (mechanism: behavioural tests; platform CI; the active effect denylist). Containment is not among the automated parts that row names and this finding cites no test or denial, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — a recorded locator names `<R>/runs/<run_id>` where a component is a symlink out of `<R>` -> `authorized_root` checks components lexically, rejecting `..` and requiring the two trailing names, and resolves no link -> the derived root is accepted -> locking and reclamation operate under a root outside the containment boundary the record was supposed to prove **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region** — `authorized_root` with the doc comment and in-line rationale that justify the lexical check, `3e5212d` `recover.rs` 418-458: `b9fd86d19d22b096130ecdee08427816852a062a59f0a0df64b854e95fd10483`. |
| PR7-STD-QUESTION-PAYLOAD-COMPONENT | The question-payload write boundary interpolates an unvalidated component into an authoritative path (`src/rundir.rs`) | project owner | **Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's opening MUST — *"Code MUST validate them before granting filesystem, process, git, capacity, or state-transition authority"*, persisted run data being named there as a trust-boundary input, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — a component reaches `write_question_payload` from persisted or model-authored input -> it is interpolated into `questions.join(format!("{component}.json"))` with no validation -> a component containing a separator or `..` escapes the questions directory -> a write lands outside the run directory the funnel is supposed to bound **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region** — `write_question_payload` with its doc comment and the join it performs, `3e5212d` `rundir.rs` 819-831: `0f0459b9b9e906df032ec7c988288be68764a99b2f16ee37d4d106c7b60a05f6`. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too. |
| PR7-STD-ANSWER-STAGING-COMPONENT | The answer staging boundary uses an unvalidated component as part of its write path (`src/rundir.rs`) | project owner | **Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's opening MUST — *"Code MUST validate them before granting filesystem, process, git, capacity, or state-transition authority"*, persisted run data being named there as a trust-boundary input, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — a component reaches `stage_answer` from persisted or model-authored input -> it is interpolated into `answers.join(format!("{component}.json.partial"))` with no validation -> a component containing a separator or `..` escapes the answers directory -> the writer-owned residue lands outside the run directory and no reader's ignore rule covers it **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region** — `stage_answer` with its doc comment and the join it performs, `3e5212d` `rundir.rs` 916-927: `fcb6df369b997f817a786bd69da731a676fbd2cac91a53421fc5bc5aea659db9`. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too. |
| PR7-STD-OWNERSHIP-PROOF-UNCANONICAL | The private-half ownership proof falls back to an uncanonicalized public path when canonicalization evidence is unavailable (`src/rundir.rs`) | project owner | **Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's MUST-tagged fail-closed bullet — *"Security-sensitive comparisons and decisions MUST fail closed on malformed, contradictory, or unavailable evidence; availability fallbacks must not silently grant more authority."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — the marker is written while a spelling denotes one public directory -> that spelling is later retargeted to another while the remaining recorded fields still match -> canonicalization of the public directory then fails, so the proof falls back to the uncanonicalized path and renders it lossily -> the comparison is spelling against spelling, the private half is treated as this run's, and the retain-on-disagreement path never fires. The retarget step is stated because equal spellings alone do not establish two directories **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region 1 of 2** — the proof's documented contract through the fallback, the lossy rendering and the disagreement comparison they feed, `3e5212d` `rundir.rs` 1451-1595: `c4256e0a23cc312185222314f3af0f1d1cf353f7c8720cf0e07178d9082bba5f`. **Region 2 of 2** — the **write side's** rationale, which is where the deliberateness of the paired fallback is actually argued, `3e5212d` `create.rs` 1983-1995: `4500bb448c7bd33285f4d72c9d40366334a65098c35e46ccd4d448bf4b0bfd37`. An earlier revision hashed only the read side's fallback, so an edit to either rationale left the digest verifying while this row's documented-site claim became false — the third time a region on this row covered less than the row claims, and the reason every region is now quoted with what it covers. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too. |
| PR7-STD-CONTAINER-LEXICAL-CONFINEMENT | Confinement uses a lexical prefix comparison as its entire filesystem-containment decision (`src/runner/container/exec.rs`) | project owner | **Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §8's MUST-tagged containment bullet — *"Path containment checks MUST account for `..`, absolute paths, symlinks/reparse points, and platform-specific prefixes as appropriate to the security boundary. Lexical normalization alone does not prove filesystem containment."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§§8–9 filesystem, persistence, and processes** (mechanism: behavioural tests; platform CI; the active effect denylist). Containment is not among the automated parts that row names and this finding cites no test or denial, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — a mount source reaches a withheld path through a symlink or a differently-spelled prefix -> `violations` decides containment with `withheld.starts_with(source)` and resolves nothing -> the mount is judged not to hand the container a withheld path -> the container is given the public log or the repository root and the confinement claim is false **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region** — `violations` with its doc comment and the `starts_with` that is the whole check, `3e5212d` `exec.rs` 316-332: `2c7edcd309cca055fb992a25c73504f197fe17a5e5e04fffd611ff4f417e81f4`. |
| PR7-STD-CONTAINER-EXEC-UNBOUNDED | The Docker subprocess primitive has no timeout or cancellation protocol and captures both streams without a pre-allocation bound (`src/runner/container.rs`) | project owner | **Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034`, filed to the sweep work-list as a §9 observation, and routed here from PR41 second review record `reviews/2026-08-28-pr41-frontier-review-788c714.md`, whose first finding identified the misclassification.** It cites **§9's MUST** — *"Every subprocess integration MUST define and test: … timeout, cancellation, and descendant-process cleanup"* — and the same section's requirement that stdout/stderr size behaviour be defined and tested. `CODING_STANDARDS.md` §1 refuses an ad hoc in-code exception for a MUST, and the site's doc comment argues only about **stream separation** — it says nothing about timeout, cancellation or bounds — so there is no rationale to weigh even at SHOULD strength. Enforcement map row **§§8–9 filesystem, persistence, and processes** (mechanism: behavioural tests; platform CI; the active effect denylist); subprocess timeout and capture bounds are not among the automated parts that row names and this finding cites no test or denial, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing**; category **liveness**; failure sequence — `docker logs` hangs or emits unbounded output -> `Command::output` waits with no timeout and allocates complete stdout and stderr buffers before returning -> the runner's later truncation runs only after those vectors exist, so it bounds the log and not the capture -> a container operation blocks the runner indefinitely or drives it toward OOM, and no cancellation path exists to stop it. **Region** — `exec_streams` with the doc comment that is its whole stated rationale, `3e5212d` `container.rs` 1516-1547: `8998739ca68035a8f8e538a3f8c0783835664bbf69ba395d03318322063f6c5f`. The digest is recorded to relocate the site inside a file that has moved; it is not a W10.4 salvage claim and does not exempt this row from re-derivation. **Why it is here and not in the sweep:** the sweep is for SHOULD-level conformance; §1 sends a MUST deviation to an owner, which is the same test that routed the other seven. |
| PR43-MACOS-PROC-SIGNAL-FINGERPRINT | One macOS run of `agent::proc::tests::a_blocked_terminal_signal_still_wakes_a_suspended_host` reached the monitor's terminating path with `PENDING_TERMINATION == SIGTERM` and exited 143 instead of completing cleanly; the writer and reason are unresolved, and no rate has been measured | project owner / the slice that next opens `src/agent/proc.rs`, once a controlled macOS environment can measure it | **Open as an unexplained observation, not classified as a flake or regression.** Durable provenance and the exact matching boundary are in `reviews/2026-08-28-macos-proc-signal-single-failure.md`: test name, assertion site and the status form `exit status: 143` (not signal termination). Run `33162906210`, attempt 1, at `c3e5b20`; one failed and one green attempt are visible, but that opportunistic pair is not promoted to a rate. Several writers can store SIGTERM, including reaper-cleanup and the SIGCONT guard fallback, so a matching red remains unresolved until its writer and reason are established. This row fulfills the record's deferred §2 commitment after the PR #42 serialization boundary; it does not strengthen the record's causal claim. |
| PR43-WINDOWS-TOPOLOGY-KILL-FINGERPRINT | One Windows run produced two topology kill-test failures together: a `git worktree prune` ran outside a repository after snapshot-add kill, and the retry helper exited 101 where the parent required an abort; whether they share a cause is unresolved, and no rate has been measured | project owner / the slice that next opens the Windows topology kill harness | **Open as one unexplained run, not classified as a flake or regression.** Durable provenance, byte-exact assertion sites and the limits of the Windows abort oracle are in `reviews/2026-08-28-windows-topology-kill-single-failure.md`. Run `33169116985`, attempt 1, at `02b7399`; one of three same-source Windows jobs failed, an opportunistic observation rather than a designed rate. Exit 101 identifies a panic but discarded child output cannot show why, and the prune failure does not prove which process removed or invalidated the repository. This row fulfills the companion record's deferred §2 commitment after the PR #42 serialization boundary without merging the two messages into a guessed mechanism. |
| PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT | One Windows (`test (winguest)`) run produced two `engine::topology::settle` failures together, both refusing a log replay with `MalformedEntry { kind: "task_dispatched", key: 0 }` on a predicted-region mismatch: the entry takes the predicted region `` `src/aleph/` `` while the same entry's frozen path hints derive `` `src/aleph` `` — the two strings differ only by the trailing `/`. The writer of the differing spelling is unresolved and no rate has been measured | project owner / the slice that next opens the Windows `engine::topology::settle` harness or the frozen path-hint derivation | **Open as one unexplained run, not classified as a flake or regression.** **The failure.** Run `33779292591`, attempt 1, job `100728982300`, at `a5d1e14`. `engine::topology::settle::tests::retained_generation_not_continued_after_kill` panicked at `src\engine\topology\settle\tests.rs:1807:60` and `engine::topology::settle::tests::kill_after_failed_settlement_rematerializes_question` at `:1764:56`, both on `the log replays`; `test result: FAILED. 1760 passed; 2 failed; 35 ignored`. The fixture declares the region **with** the slash — `task_of("aleph", "src/aleph/", Tier::Mid)` at `settle/tests.rs:100` — and the hints derive it **without**. **It is intermittent on one leg, and the window is stated rather than summarised.** `test (winguest)` concluded **success** in runs `33776180623` (attempt 1, at this same SHA `a5d1e14`), `33774140883`, `33770200867` and `33769228836`, and **failure** in `33779292591` (attempt 1, also `a5d1e14`). So the identical commit passed this leg once and failed it once. Every run named here is `attempt=1`: no rerun-in-place is hiding a conclusion inside a row, which the API reports only at its latest attempt. **All runs at the head were read, not the latest** — reading the latest is how this class hides. **Why PR #104's diff cannot reach it, offered as reasoning that does NOT satisfy a floor.** The pull request's only `src/` changes outside `src/plan/` are in `src/topology/registry.rs`, and that change is **inside `mod tests`**, replacing `fs::read_to_string` of three fixture paths with the compile-time corpus constants; the two failing tests are in `engine::topology::settle` and read no corpus; and the shape is absent from recent failing runs on `master` and on the four sibling refactor branches. **The merge proceeded on the project owner's disclosed decision of 2026-09-03, not because this explanation cleared the red.** A floor is not satisfiable by explanation, and this row exists so that the decision is on the record with its evidence rather than resolved by argument. **What would settle it.** A spelling that passes four times and fails once is nondeterministic path handling on the Windows leg, not a fixed normalisation defect — a fixed one would fail every time. The measurement is: establish which writer produces the frozen path hints for a `task_dispatched` entry and whether its derivation is order- or environment-dependent on Windows, then run that derivation repeatedly on the winguest host against the `src/aleph/` fixture and record the distribution. **The cause is not guessed here.** **Provenance is inline rather than in a companion record**, unlike `PR43-*`, because this append is deliberately confined to `reviews/FINDINGS.md` — `decisions/2026-08-20-review-invalidation-scope.md` makes that one path the exempt set, and a second file would forfeit the exemption and invalidate the pull request's frontier review. |
| CLASS-GATE-STATED-DOMAIN-EXCEEDS-COUNTED-DOMAIN | **A gate whose stated domain is wider than the domain it counts, so it fails open in the passing direction.** Seven instances measured 2026-09-03, in four subsystems, each repaired on its own and none of them found by the repair of another. **(1)** `PR108-CENSUS-DEPTH-FAIL-OPEN` (#108): the `charge_allowance` census read `src/topology/fold/*.rs` and never descended, so a third charge in a grandchild behind a `mod debit;` would double-charge a retained failure at run time while the census still saw two calls and passed; the recheck that missed it was planted **inside** the scanned boundary. **(2)** `PR106-CENSUS-WITNESS-TEXT-DOMAIN` (#106, repaired at `6dc5987`): the witness for "the census cannot read a file the walk did not" extracted the census function's body and banned six reader names, so the assertion's domain was the census's **source text** while its claim was about the files the census reads. A sibling helper scanning a stale eight leaves that body innocent of every needle, and the positive control fires on the witness's own text either way. Implemented and run at both heads: both row tests green at `6d8cdda`, the census red at `6dc5987`. **(3) The repair of (2) reproduced the class one level down**, found by the frontier pass on `6dc5987` the same day: `RowMappingScan`'s fields are `pub(super)` (`src/effects/tests/source_oracles.rs:401`), so a sibling can construct one whose `scanned` is copied from `ProductionModule::files()` while it scans something else. The equality at `:529` then compares the walk against **self-reported metadata** rather than the files actually read, and both row tests stay green with a wildcard `row()` in an unscanned ninth child. **(4)** #116 at `111b045`, review finding 5b: `effects/allowlist.toml`, `src/agent/proc/test_support/readiness.rs` and `src/effects/tests.rs` cite standards §2 for content it does not contain, and the citation passes because the test only checks that both records **mention the new pathname**. **(5)** #106 pass-3 finding 2: the production-module walk's stated domain is the module, and its counted domain is the declarations `scan_module_declarations` can *read* — it inspects an item-position macro invocation's tokens, so a macro defined in a file the walk never reads expands to `mod twelfth;` and the child is never scanned. Reproduced at `62f4ac1`: both row-mapping tests green, no test file touched, the wildcard missed. **(6)** #106 round 3's prose sweep: stated scope "the prose the split falsified", counted scope *categorical* claims only, so a cross-reference — `ReportSite`'s "see this module's worker report" — was outside it and shipped. **(7) The repair of (5) reproduced the class again**, found by the sixth frontier pass on `27c8b2b`: the reconciliation's stated domain is the module, and its counted domain is the module's own DIRECTORY, so a macro expanding to `#[path = "effects_hidden.rs"] mod hidden;` lands the file beside the directory and no walk rooted at it looks there. Reproduced at `27c8b2b`: both row-mapping tests green, no test file touched, the wildcard missed. | project owner | Seven instances in four subsystems in one day, and the class is what they share rather than anything their subsystems do. **Two of them are repairs of earlier ones that reproduced the defect they were fixing** — (3) repairs (2), (7) repairs (5) — and (6) is the *sweep* for a sibling of (2) exhibiting it too. That is the evidence that per-instance repair does not converge here: three of the seven were introduced by the attempt to close another. #106 rounds 4 and 5 closed the instances at (2), (3), (5), (6) and (7), each with a two-sided proof; the class still has no check. What would close it is a stated rule with a check behind it: for any gate, name the domain the **claim** is about and the domain the **assertion** counts, and require them to be the same set of values rather than two descriptions that agree today. No such rule exists in the standards or in this file, and all seven were found by four separate reviews — #108's, #116's, and #106's fourth, fifth and sixth passes — rather than by anything looking for the shape. Filing this is not a disposition on any of the four; each keeps its own. |
| PR101-CFG-ATTR-APPLIED-CFG-INVISIBLE-TO-THE-SCAN | `scan_module_declarations` (`src/effects.rs:2207`) treats a `cfg_attr` as significant **only when its text contains `path`** — `"cfg_attr" if raw.contains("path") => pending_path = true,` at `:2282` — so `#[cfg_attr(all(), cfg(test))] mod hidden_tests;`, which rustc applies as `#[cfg(test)]` and compiles only under test, is read as an **unconditional** declaration and the file it names stays in every census's domain as production. A fixture call in that file then sits inside the production censuses, where it can mask the deletion of a real production call — the exact failure the skip sets exist to prevent | the slice that next widens `scan_module_declarations` in `src/effects.rs` | Predates W1 by months; no W1 or W2 diff touches it. Widening the scan to **decide** `cfg_attr` predicates changes what every census in the crate scans, and a measurement change gets its own review. Already recorded in the tree as a stated limit, with the measurement that established it, in `declared_whole_file_test_modules`' doc comment (`src/effects.rs:2022-2039`) — so a repair must update that paragraph in the same change or leave a comment describing a hole the code no longer has. Full derivation, venue and required evidence: **§43** |
| W1-CLASSIFIED-MODULES-IS-A-HAND-MAINTAINED-ROLL-CALL | `mechanism` (3)'s classification census takes its domain from `CLASSIFIED_MODULES` (`src/effects.rs:968`) — **56 entries at `3af9696` and not one directory prefix** — and `reachable_fns_are_classified` (`src/effects/tests/classification.rs:99`) asserts set equality against it before reading each entry from disk. Its doc comment at `:95` says *"The domain is **derived from the modules**, not listed"*, which is true of the **function-level** domain and not of the **module-level** one: a new production child file is graded only if somebody enrols it by hand. **Twenty-one** `.rs` files sit under a directory whose `.rs` parent is listed and are not themselves listed, and **the last two arrived after this row was written**: M7 split `src/config.rs` into `parse.rs` and `read.rs`, which carry **nine `pub(super)` functions between them** — all new names, none of them previously a `pub*` item of the parent. `declares_visibility` inside `externally_reachable_fns` counts `pub(super)` alongside `pub` and `pub(crate)`, so those nine are exactly the kind of item this census exists to force somebody to classify, and **because their files are not in the roll-call, nothing requires it and nothing fails** | the slice that next changes `CLASSIFIED_MODULES` in `src/effects.rs` | A consequence of the extraction pattern rather than of any one packet, and the fix is a decision about how the list should treat child directories. **The `TOPOLOGY_MODULES` half is already fixed** — `f1918e0` added the `src/workspace_manager/` prefix — and the two lists are matched differently on purpose (`src/effects.rs:903-911`), so the repair for the surviving half is not "add a prefix" but to derive the module domain or to state and execute the roll-call's semantics. The `m3-rundir`, `m5-host` and `m6-proc` splits each enrolled their children and each cited this finding by its working-record key while doing so; **M7 did not, and no gate noticed** — which is the same mechanism seen from the other side. Neither choice can be called wrong, because **the criterion is nowhere stated**: three splits enrolled, one did not, and the tree says only that the list is hand-maintained. That is the finding, and the asymmetry is what sustains it — enrolling a child costs classification rows for every reachable item in it, while not enrolling one costs nothing and is checked by nothing. Full derivation, with the command that reproduces the nineteen: **§43** |
| PR103-CONTAINER-SUBSTRATE-LIST-CHECKS-NAME-ONLY | `every_view_discard_removes_through_the_one_racing_removal` (`src/runner/container/tests.rs:4883`) excludes out-of-line test substrate by name through a `SUBSTRATE` const (`:4888`, six entries), and the only assertion over that list is `assert_eq!(excluded, SUBSTRATE.len(), …)` at `:4931` — a check that each name **is met**, not that each name **is still test substrate**. A listed file that later becomes production-reachable — compiled as a Cargo target, or declared unconditionally by a production parent — stays excluded and nothing notices. Failure sequence: add an `[[example]]` target whose `src_path` is a listed file, give it a `#[cfg(not(test))] main` reaching a governed primitive, and the census skips it | the slice that next changes the container removal census | Byte-identical before and after PR #103 and not activated by it. **A claim this finding used to carry is withdrawn**: it said PR #103 closed the same gap in its own better-guarded list, but #103 was **closed unmerged**, so that list never landed and the comparison has no second term — text describing code that does not exist, which is the failure mode §43 is written against. The two guards (an entry must not be a crate root, and must be a member of `cfg::WHOLE_FILE_TEST_MODULES`) remain the shape of the repair; they are implemented nowhere. Full derivation: **§43** |
| PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY | Two gaps in `census_domain`, each established by a separate frontier pass. **(1) Target kind is discarded**: `CrateRoots` (`src/effects.rs:1742`) keeps a `package_dir` and a `BTreeSet<PathBuf>` and nothing else — its doc comment states the choice, *"Kinds are **not** filtered"* — so a `[[test]]` root, which Cargo compiles with `cfg(test)` on and which can therefore be exclusively test code, is indistinguishable from a `[[bin]]` or `[[example]]` root. **(2) Non-test declarations are ignored**: `declared_whole_file_test_modules` (`:2050`) skips every declaration that is not test-only (`:2076`), so membership proves *"some test declaration resolves here"* and never *"only test declarations reach here"*. The reviewer's sequence: a `#[cfg(test)] mod fixture;` whose file is also declared unconditionally by a binary root that calls it — production-reachable, and invisible to the resolver | the slice that next changes `CrateRoots` or `declared_whole_file_test_modules` in `src/effects.rs`, or W3 when it takes up the deferred registry extraction | Not confined to the closed pull request that found it: **two shipped censuses derive their skip sets from this resolver at `ae2a58f`**, both adopted under `PR7-R5-ATT-001` — **an attestation key carried in the source, not a row in this file**; it resolves at `src/effects/tests/source_oracles.rs:1569`, `src/runner/mod.rs:1456`, `src/events/log/tests.rs:3412` and twice in `src/engine/topology/recover/tests.rs`, and a reader should not look for a ledger row of that name — `runner::tests::production_sources_by_path` (`src/runner/mod.rs:1458`) and the fold census (`src/events/log/tests.rs:3414`) — so both carry the blind spot on `master`. The shape of the repair is recorded so it need not be re-derived: retain target kind, and add a query for "is this path declared unconditionally anywhere in the walk"; neither changes what `whole_file_test_modules` returns, which is what killed #103's round 2. Full derivation: **§43** |
| PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED | Every temporary directory in `src/validate.rs`'s test region is derived from `env::temp_dir().join(format!("upstroke-validate-<tag>-{}", process::id()))` — **predictable**, created with `create_dir_all` (which accepts an existing directory), stored as a bare `PathBuf`, and never reclaimed: **12 `env::temp_dir()` sites, 12 `create_dir_all` lines and 0 `impl Drop`** at `ae2a58f`. `scratch_root` (`:403`) additionally runs `let _ = fs::remove_dir_all(&dir);` (`:405`) against that predictable path before creating it, deleting whatever a previous run or another process left there and discarding the error. `standards/12_standards_tests.md:16` requires *"unique temporary directories with RAII cleanup"* | the slice that next opens `src/validate.rs`'s test region | Byte-identical before and after PR #104 and not activated by it; the reviewer said so explicitly and kept it out of the verdict, which turned on the newly introduced instance — and **that instance no longer exists**, since owner ruling 7 reverted the file to `origin/master` entirely. **The harm is measured, not argued**: the pass-7 reviewer pre-created `$TMPDIR/upstroke-validate-sample-<pid>/foreign-sentinel`, ran `sample_plan_renders_expected_table` against the exact-head binary, and the test **passed** with `sentinel=deleted` and `replacement_plan=present`. Full derivation: **§43** |
| PR104-PRELOCK-SCRATCH-NAME-REPRODUCIBLE-ACROSS-RUNS | `Scratch::new` (`src/engine/topology/prelock/tests.rs:200`) names its root `upstroke-prelock-{tag}-{pid}-{ThreadId}`. Every component resets when the process does, so the name is **reproducible across runs**: a killed run leaves a root behind, and a later run that reuses the pid and gets the same thread id computes the same path. The allocator then **adopts** it silently rather than refusing — `create_private_dir` (`src/rundir.rs:634`) → `create_dir` (`:575`) → `fs::create_dir_all`, which succeeds on an existing directory | the slice that next changes `src/engine/topology/prelock/tests.rs` | Byte-identical across PR #104 and not called by it; the reviewer said so explicitly. **Worth recording because of what it is**: this is the precedent PR #104 was told to copy, on the strength of its measured success against leaking — 5050 `upstroke-prelock-*` roots by 2026-08-30 and none after, recorded in its own doc comment at `:181`. It is a good precedent for **reclamation** and it carries a defect in **allocation**, and the packet that copied it inherited both. Copying a precedent copies its weaknesses. Related: `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED`, the same allocation weakness ten times over in the file that copied this one. Full derivation: **§43** |
| W1-FIXTURES-NOT-RETIRED-W0-AUTH-PART-E-UNFULFILLED | W0-AUTH Part E said: retire `fixtures/` and inline the corpus. **`fixtures/` survives** — `bare-plan.md`, `cyclic-plan.md`, `sample-plan.md`, `steps-plan.md`. What PR #104 as landed did achieve, re-derived at `ae2a58f`: every runtime fixture read **outside** `src/validate.rs` is gone; `src/plan/mod.rs` takes the corpus at compile time through `include_str!` (`:82`, `:87`, `:91`) and `src/plan/markdown.rs` and `src/topology/registry.rs` (`:3123-3125`) consume those constants. **`src/validate.rs` is the one remaining runtime reader, with 10 call sites** of the form `opts("fixtures/<name>.md")`; `cyclic-plan.md` is the one file with no constant and its only consumer is `src/validate.rs:739` | the slice that takes up retirement, which is blocked behind the `src/validate.rs` scratch-directory row | Recorded so an unfulfilled packet clause is not later read as a fulfilled one. It stopped here because `src/validate.rs` is frozen-legacy and every attempt to give its tests a corpus on disk produced a new finding about temporary-directory ownership — five across four repair rounds, then three more at pass 8 — and owner ruling 7 reverted the file rather than ship the ninth. **What is owed**: retirement needs `src/validate.rs`'s tests to stop reading from disk, which is `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED`'s problem for all ten call sites at once. Doing that row first makes retirement straightforward; doing it second is what produced eight passes. Full derivation: **§43** |
| W2-EXPECTED-REFS-COUNT-STALE-AFTER-EXTRACTION | `production_calls`' doc comment (`src/effects.rs:1370`) asserts *"Measured on this tree: `workspace_manager.rs` carries four occurrences of the substring `expected_refs(`"* and then reasons from that number. **The root file carries one**; the other three moved to `src/workspace_manager/tests.rs` when W1 extracted the test region, and every other file under `src/workspace_manager/` carries none. The number is right about the **subsystem** and wrong about the **file it names**, which is why it survived: a reader who recounts across the directory reproduces "four" and moves on | the packet that next holds the pin-maintenance grid lock for its own reasons | Already stale before W2 began — W1's extraction caused it, and no W2 packet causes or worsens it; the steward checked both directions before proposing it. **The repair must not be another count.** Whoever makes it should state the property the sentence exists to make — that a substring needle is satisfied by a longer identifier — rather than re-measure a number the next extraction falsifies again; a count in prose beside a list that moves is the same hazard this row is an instance of. Full derivation, both files, two engines: **§43** |
| PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK | The child-lint census in `src/runner/container/tests.rs` derives its domain by walking each funnel's directory — `const FUNNELS` (`:3146`), then `let arm = walk(&directory);` (`:3170`) — so **a `#[path]` relocation is invisible to it by construction**. M4's repairs are on `master` and are correct: `assert_eq!(with_children, FUNNELS.len())` (`:3183`) and a per-arm `assert!(!arm.is_empty())` (`:3171`), stated over the class. They do not reach this variant. At `ae2a58f` the walk finds **38 children, 16 of them named individually and 22 named by nothing but the walk**; relocating the 22 with `#[path]` less one file kept per arm leaves **20 ungraded with every assertion still green** — union 18 over a floor of 9, `with_children` 5, no arm empty, all 16 named files present | the slice that next changes the child-lint census | Pre-existing at `1cbdccd`; neither M3's nor M4's split activates it or makes it worse, and a mechanism change to a census gets its own review. **By-name pinning is not the answer** — it catches this only if a pinned file happens to be a relocated one, and the pinned count has gone 1 → 6 → 16 across three packets each adding its own. **The prescription, so a repair need not re-derive it: derive the domain from the module declarations rather than from a directory walk**; the repository already holds the pattern in `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`. Derived independently twice, by #110's reviewer and by M4's steward. Full derivation with its command: **§43** |
| W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY | `an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name` (`src/runner/host/tests.rs:7179`) writes an executable through `marker_shim` (`:5329`) and immediately spawns it. In a gate run at `d8f4d13` the spawn failed with `"an empty entry before a real installation: a raw spawn: Text file busy (os error 26)"` — **ETXTBSY**, a concurrently-forking thread in the same process still holding a write descriptor at `execve`. A textbook write-then-exec race under a parallel harness, not a logic error. **Both functions are byte-identical from `1cbdccd` through `ae2a58f`** (`marker_shim` sha256 `f666ed74…`, 701 bytes; the test `098f21e8…`, 4489 bytes), so the race travelled unchanged through the M4, M5 and M6 splits | the slice that next changes `src/runner/host/tests.rs`, or whoever meets the failure again | Pre-existing, not reproducible on demand, and fixing it inside a split packet would put a concurrency change in a refactor's diff. **Both prescriptions this finding has carried are refuted, which is the most useful thing in the row**: `drop` plus `sync_all` closes nothing, because the writer is `std::fs::write` and it already drops its handle; and rename-into-place does not help either, because a `fork` that inherits the descriptor inherits it whatever the path is called. A repair must demonstrate that it addresses **fd inheritance across a `fork` in another harness thread**. Misattributed by construction — the failure lands on whichever test happens to be spawning. Full derivation: **§43** |
| W2-WINDOWS-RACING-REMOVAL-DELETE-PENDING | `racing_removal` (`src/runner/container.rs:1437`) retries a removal `RACING_ACCESS_ATTEMPTS` times — `= 64` at `:404` — then returns `UpstrokeError::Io`. On the Windows guest it exhausts that budget against an R19 view directory under **delete-pending** semantics, at roughly **2%** of runs on a 16-vCPU guest. It is a defect in **production code**, not in the harness or the build box. **It is not concurrency and not Docker**: the guest has no Docker and its jobs never overlap — 123 executions, zero overlaps — so the contention hypothesis this programme carried through W1 is wrong, and this row supersedes every earlier characterisation | project owner, undirected | **Two traps, both pointing at the wrong subsystem.** (1) A `failed to read <path>` message on that path means a **removal** failed: `UpstrokeError::Io` has one `Display` — `#[error("failed to read {}: {source}", .path.display())]` at `src/error.rs:23` — so read, write, create, sync and remove all render identically; the message names the `Display` impl, not the operation. (2) `0123456789abcdef` in those paths is the fixture constant `REPO_KEY_A` (`src/runner/container/census/tests.rs:89`), **not** an unset `CARGO_TARGET_DIR` slot key — dangerous precisely because that hex is the slot-pool trap's visual signature. **How to tell it from a compile break**: three Windows legs failing together is a compile error; `test (winguest)` alone on a `racing_removal` signature is this race. A rerun on this signature is legitimate and disclosed as such — the only one of §43's six CI signatures carrying that licence, and it has it because the mechanism is established. **Raising the 64 is not the fix** and is an infrastructure decision for the owner. Full derivation: **§43** |
| PR110-SITE-CENSUS-MATCHES-EFFECT-SITE-NAMES-BY-SUBSTRING | `every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent` (`src/effects/tests.rs:2625`) decides that a funnel names a site by plain substring containment — `if source.contains(&variant)` at `:2677` — so **a longer variant satisfies a search for a shorter one**: `WorktreeSite::RemoveExecutionRoot` (`src/workspace_manager.rs:747`) satisfies a search for `WorktreeSite::Remove`. Remove the exact shorter literal while keeping the longer one and the census stays green while the removed site goes unnoticed. **The exposure is a class and it is enumerable**: every group's sites share one funnel module, so a within-group prefix collision is a same-file collision, and at `ae2a58f` there are **ten collision pairs over six shorter variants in four groups** — `WorktreeSite::{Add, Remove, RemoveStaging}`, `SnapshotSite::Remove`, `ContainerSite::Remove`, `EventSite::Append` | the slice that next changes the site census in `src/effects/tests.rs` | Pre-existing and **not activated** by #110: all six shorter variants are still present as exact literals — not merely as substrings — in their own funnel modules, measured by counting matches not followed by an identifier byte, so the collisions have nothing to hide yet. Verified by the steward before proposing it and by #110's reviewer independently. **Fix the class, not the pair**: repairing only the collision the reviewer named leaves the other nine. A count #110's body quoted was under-counted by this same weakness and was stripped under ruling 10; **the finding survives that, because the census weakness is independent of whether any body quotes a number.** Same family as `PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY` and `PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK`. Full table of collisions: **§43** |
| PR110-CONTAINMENT-COMMENT-STATES-A-FALSE-GUARANTEE | `src/workspace_manager/containment.rs:83` states *"every deletion **in this subsystem** goes through `WorkspaceManager::contained`, which compares **canonical** paths, so a resolved link cannot carry a removal outside the root."* **It is FALSE, not stale** — recorded in those words deliberately, because "pre-existing, referent updated" reads as a bookkeeping nit and this is a false containment assertion in a security comment. Of the six deletion sites in the subsystem's production region at `ae2a58f`, **one** goes through `contained()`: the checkout removal in `remove_worktree` (`src/workspace_manager.rs:1215-1216`). `remove_intent` (`:842`) reaches `fs::remove_file` after `slot.validate()?` and `self.revalidate()?` with **`contained()` never called**; `remove_execution_root` (`:760`, `:766`) does not call it either, and neither do `remove_worktree`'s own `locked`-file removal (`:1232`) or its admin-tree removal (`:1256`). `contained()` has exactly one production call site in the whole subsystem | the slice that next changes `src/workspace_manager/containment.rs`, `remove_intent`, `remove_execution_root` or `Slot::validate` | **What actually provides the containment, which the comment does not name**: `Slot::validate` (`src/workspace_manager/naming.rs:189`) calls `safe_component` (`:136`). So the subsystem is safe on that path **by a different mechanism than the one documented** — the dangerous case, not the harmless one, because a refactor that weakens `safe_component` or adds a deletion path skipping `validate` will be reading a comment promising a guard that does not run there. The real guard is load-bearing on two of three `Slot` arms (`Staging` holds a `u64` and has nothing to validate, `:192`), so the **documented** guard is the real one on none of them. A **three-state trace** is why nobody caught it: the claim was false at base, made **vacuous** by the split, and false again once the repair widened its referent — so at any single state it reads as either a pre-existing defect or a clean repair. Ruled out of scope by #110's pass-2 reviewer. Full table of the six sites: **§43** |
| W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT | `runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` fails intermittently on macOS with *"`<role>`: the child did not lead its own process group, so the pre-exec containment step did not run for this role"*, and a `test result: FAILED. <n> passed; 1 failed` whose passed-count tracks whichever head it ran on. **Twelve sightings across six branches between 2026-09-01 and 2026-09-03**, every one confirmed by its own `... FAILED` line in its own job log rather than by a mention. **Three are on `master` itself**, and the two earliest — runs `33503020178` and `33535107935`, 2026-09-01 — are at `src/runner/host.rs:5574:13`, the pre-extraction location, so **the failure predates the W2 programme and the `W2-` prefix records when it was found, not when it began**. The rest sit at `src/runner/host/tests.rs:4220:9`, `:4227:9` or `:4229:9`, which is the same assertion relocated by successive splits | project owner / the slice that next opens the pre-exec containment path, once a controlled macOS environment can measure it | **Open as an unexplained observation, not classified as a flake or regression.** Not diff-caused, on the cleanest counterfactual this programme has produced: `c30aca0`'s delta from `9a7fc22` is `reviews/`-only, `9a7fc22` was green (run `33776069960`, attempt 1) and `c30aca0` is red — the same tree with a markdown file added. Independently, #108 does not touch `runner::host` at all. **The failing role varies across three roles** — `probe(claude-code)` six times, `review` four, `implement` twice — **and one run settles what that means**: run `33777752620` is red on both attempts at the identical commit, naming `probe(claude-code)` then `review`. Direct evidence that any role can lose, consistent with a race in the pre-exec `setpgid` path rather than with anything specific to a role. Whether this is a face of `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` is **open**; the signatures differ and they are deliberately not merged on family resemblance, because that row's repair makes the question answer itself — if this shape stops recurring on heads carrying it, it was the same defect. **Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.** Full evidence: **§43** |
| PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT | Two `engine::topology::settle` kill tests fail together on the Windows guest with `the log replays: AlreadyStarted` — `kill_after_failed_settlement_rematerializes_question` at `src\engine\topology\settle\tests.rs:1764:56` and `retained_generation_not_continued_after_kill` at `:1807:60`, `test result: FAILED. 1760 passed; 2 failed; 35 ignored`. Run `33785587535`, attempt 1, job `100749444333`, `test (winguest)`, at `9963fb0` on PR #107; `upstroke-ci` concluded failure on the back of it | project owner / the slice that next opens the Windows `engine::topology::settle` harness | **Open as one unexplained observation, not classified as a flake or regression.** **Its own ID deliberately, and NOT folded into `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT`**: that row is a predicted-region trailing-slash mismatch with `MalformedEntry { kind: "task_dispatched", key: 0 }`, and **the string `src/aleph` appears zero times in this job log** — checked in a local copy. Same module, same leg, same two tests, **different assertion**; folding two fingerprints into one record is how a class stops being countable. Nondeterministic, established by the same head passing twice and failing once, all `attempt=1` so nothing is hidden inside a row: `33784774150` success, `33785587535` **failure**, `33786611538` success — and the red run was started by a **body edit**, not a code change. **Not a regression from the C-004 repair**: a regression would be deterministic and this is not. What would settle it is wider than the path-hint derivation the sibling row names — whether these two tests build their event log deterministically on Windows at all. **Not rerun**; no licence covers this signature. **Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.** Full evidence: **§43** |
| PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT | `workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered` panicked at `src/workspace_manager/tests.rs:5691:10` with `forced removal converges: Git { message: "worktree registration …/.git/worktrees/kalpha-g1 has an empty gitdir" }`, `test result: FAILED. 1806 passed; 1 failed; 35 ignored`. Run `33787330192`, attempt 1, job `100755588011`, `test (ubuntu-latest)`, at `9963fb0` on PR #107 | project owner / the slice that next opens the workspace residue sampler | **Open as one unexplained observation, not classified as a flake or regression.** **A third platform**, and its own ID rather than folded into either Windows row: different platform, different subsystem, different assertion. Nondeterministic — the same commit produced two green runs of this leg in the same hour. Cause unknown: `remove_worktree` handles an empty `commondir` deliberately (`src/workspace_manager.rs:1249-1258`), so whether the sampler is racing that arm or reaching a different empty-gitdir path is the open question and is not answered here. **This is the cheapest of the class's four members to chase**, recorded so the choice is not re-derived: it is the only one on the Linux leg, which this project's build box reproduces directly — no guest and no hosted macOS runner. **Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.** Full evidence: **§43** |
| CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES | **A programme-wide intermittent failure rate in subprocess kill, settle and residue paths.** Members, all observed 2026-09-03: `W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT` (macOS, pre-exec process group); `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT`, already in §2 (winguest, two settle kill tests, trailing slash); `PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT` (winguest, the **same two** tests, `AlreadyStarted`); `PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT` (ubuntu, residue-and-kill, empty gitdir). **Four members, four distinct fingerprints, three platforms, three subsystems.** Sightings per member are enumerated in §43 by reading **every failing test job of every CI run on `master` and the eight W1/W2 branches, per attempt**: the macOS member has **twelve** across six branches spanning 2026-09-01 to 09-03 and fires on `master` itself, and the Windows trailing-slash member has **three**, not the one and two their own entries recorded. This row exists because the sightings were being disclosed as packets happened to meet them, which is exactly the shape that let `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` reach four instances before anyone counted it — and re-deriving the population found every per-member count understated | project owner, undirected | **What is established**: every member is intermittent — the identical commit produced both green and red runs, named per member; every member sits in a subprocess kill, settle or residue path; it spans all three CI platforms, so it is not one bad runner; and it is caused by no one packet — E, M1, M2, M3, M4, M5 and M6 have each shown a member, **and `master` has shown one three times**. **The macOS member predates W2's base commit** `1cbdccd` (2026-09-02T20:55Z) — its earliest sighting is 2026-09-01T11:32:42Z, 33.4 hours before the programme had a base (both stamps UTC) — which no packet-scoped reading survives. **It is not the C-004 repair**: M4's macOS failure at `c30aca0` predates that merge entirely, and a fix cannot cause a failure that happened before it landed. **On the count**: an earlier statement of this class said *five* fingerprints and counted a fifth macOS member that has since been withdrawn as an instance of the fixed row — the count here is derived from the members named above and nowhere else, which is the property that matters. **The lead, recorded as a hypothesis and asserted nowhere**: whether one launch-gate or reaper mechanism underlies members on three platforms is **untested**, and the evidence does not reach it. Repairing one member does not close this row. Full evidence and the two corrections that produced it: **§43** |
| W2-RETIRED-DECISIONS-PATHS-CITED-AND-MISSING | PR #116 retired the `decisions/` directory and left every citation of a file in it naming a path that does not exist. Measured at `3af9696` over the tracked tree by two engines that agree, **with §43's own text excluded because it names dead paths as examples**: **24 distinct `decisions/*.md` paths cited, all 24 missing, 168 occurrences across 53 files** — 131 in `.md`, **26 in `.rs`**, 4 in `.toml`, 4 in `.yml`, 3 in `.sh`. Including §43 the same command returns 25 / 25 / 173, and the twenty-fifth path is one §43 itself introduced. The heaviest single path is `decisions/README.md` at 32. The `.rs` citations are spread over twenty production and test files including `src/engine/classify.rs`, `src/engine/topology/run.rs`, `src/topology/effects/sites.rs` and `src/topology/fold/check_attempt.rs`; `effects/allowlist.toml` and `upstroke.toml` carry two each. **No gate catches it** — `test-docs-consistency.sh` passes at `ac16fff`, `ae2a58f` and `3af9696` | project owner, undirected | **The rules themselves survive; it is the citations that died, and that distinction was verified rather than assumed.** The clean-base merge rule this programme relies on lived in a `decisions/` file that is gone, but is restated in `DESIGN.md` and `.github/pull_request_template.md`, so it is live — checked before being relied on, because a rule cited to a deleted file is exactly the authority that evaporates on inspection. This is the **deletion** form of a class this programme met three times in one day at smaller scale: a change invalidates prose in files it does not touch, and a deletion invalidates every reference to what it deleted **including references in code comments nobody thinks of as documentation**. Not any packet's to repair — a packet fixes the citations in its own body and no more. The durable fix is a gate that resolves cited repository paths; without one the class recurs on the next directory retirement, which is how it arrived. Full derivation with its command: **§43** |
| PR119-MACOS-PROC-SUSPEND-CONTINUE-EXIT-BUDGET | One macOS (`test (macos-latest)`) run failed `agent::proc::tests::terminal_suspend_and_continue_cover_the_isolated_tree` at the **exit wait after `SIGCONT`**, not at the suspension-coverage assertion the test exists for: `wait_for_exit(&mut helper.child, Duration::from_secs(10))` returned `None` and the `expect("continued helper completes normally")` at `src/agent/proc.rs:8202` panicked; `test result: FAILED. 1806 passed; 1 failed; 34 ignored; finished in 242.38s`. The same commit passed the same leg on a rerun in place. No rate has been measured and the test name appears in no earlier record | project owner / the slice that next opens `src/agent/proc.rs`, once a controlled macOS environment can measure it | **Deferred; nondeterministic on that runner, cause not established.** **The failure.** Run `33798030529`, attempt 1, job `100790356980`, at `4094c57973ffd7f76a9868d634296be8b0f9a3f1` (PR #119). A normal assertion failure with a `failures:` section and a `test result:` line, so not the `PR43-MACOS-PROC-SIGNAL-FINGERPRINT` shape (`exit status: 143`, a different test) and not the C-004 SIGTERM shape. **The rerun.** `gh run rerun 33798030529 --failed` re-ran only the failed job at the byte-identical head: attempt 2, job `100810497995`, `test (macos-latest)` **success**, the test `ok`, `1807 passed; 0 failed; 34 ignored; finished in 288.63s`. One failed and one green attempt at one head prove nondeterminism on that runner and nothing else: not the absence of a defect in the head, and not its origin. That pair is not promoted to a rate. **What failed.** Not the suspension assertion the test exists for, which passed, but the test's own ten-second exit budget after `SIGCONT` (`wait_for_exit(&mut helper.child, Duration::from_secs(10))`, `src/agent/proc.rs:8201` at that head), which spans the helper's `SIGCONT` handler, a monitor thread polling every 10 ms, the worker's 50 ms poll of the `finish` file, the helper's own reap of it, and the helper's exit as a `cargo test` harness. On a `macos-latest` runner whose suite took 242 s, a fixed budget over that chain can expire under load; that is a reading of the code, not a measurement, and the cause is not established. **What addresses it: nothing yet.** Nothing in PR #119 touches `src/agent/`, and PR #125 changes only the forked helpers' startup `READY` waits, not this post-continue exit wait or the shutdown chain behind it; a helper can report `READY` promptly, pass the suspension assertions, and still take over ten seconds to exit. The test's exit budget is owed a load-tolerant fix of its own, by whoever next opens `src/agent/proc.rs`, and a measured rate is what would show whether that fix worked. **The guard is this row.** Whether the tests PR #119 adds to the same executable altered the scheduling this budget depends on is not argued either way. |


## 3. Challenges to settled entries

A reviewer appends here; the implementer adjudicates. New evidence only — a failure sequence the
disposition did not address, and a mutation the current suite would not catch.

*(See §2 for the mechanism working in the other direction: **PR3's** second confirmation was asked a
direct question about scope and answered it as a disposition, which is now settled in §1. This is a
claim about the PR3 round, not about PR4's second confirmation, whose two findings —
`PR4-CONF-003` and `PR4-CONF-004` — were both accepted and repaired in round 5.)*

### 2026-08-28 — `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE`, **deferred**

**The finding, and the ruling it overturned.** Six review rounds of PR #34
examined the test that pins the platform Clippy legs into `merge-gate`.
`~/tactus-artifacts/pr34/review-delta6.md` established two escapes it does not
close, and defeated the ruling that had declined to close them.

**The escapes, both concrete.**

1. `- run: echo cargo clippy --all-targets --all-features -- -D warnings`
   satisfies the command check. The job echoes, succeeds, and the aggregate
   passes while Clippy never examines a denied call in that platform's code.
2. The cfg census collects `target_os` names without evaluating `all`/`any`/
   `not`, so `#[cfg(not(any(target_os = "linux", target_os = "macos", target_os
   = "windows")))]` reports all three platforms covered while **no** runner
   compiles the body. The inverse also misfires: `#[cfg(not(target_os =
   "freebsd"))]` would demand a FreeBSD runner for a body specifically excluded
   there, and a plain `let target_os = "android";` is misclassified because the
   scan never confirms it sits in cfg syntax.

**The ruling that was wrong, recorded because the reasoning matters.** An
earlier round declined to close these, arguing from PR #25 that a text checker
over an open-ended surface does not converge. The review rejected that and was
right. PR #25's *withdrawn* half compared prose across an open document set and
had a trusted workflow rerunning the real gates behind it — machinery since
retired. Its *retained* half kept C1–C4 as **equalities and exact pins**. So
PR #25's lesson supports structural equality here; it does not license
repeated `contains` checks. The ruling is withdrawn, in this row and in the
test's own doc comment.

**The bounded repair, as the review specified it.** Parse the workflow as
YAML 1.2 with duplicate-key rejection and compare the relevant mappings
structurally: exact `runs-on`, an exact `run` scalar, absence of job- and
step-level `if` and `continue-on-error`, the exact `needs` set, and exact env
key-to-expression mappings, rejecting unexpected fields. Separately, evaluate
parsed cfg predicates against the finite CI target tuples rather than
collecting names, with a permanent injected control fixture as
`CODING_STANDARDS.md` §12 requires of a census.

**Disposition: DEFERRED, and this row is that disposition rather than a menu.**
The repair needs a YAML parser; this crate has no YAML dependency and no
`[dev-dependencies]` section at all, so it cannot be made without adding one.
Adding a dependency is a judgement about what the crate should carry, and
`DESIGN.md` does not settle it — an earlier draft of this row asserted a
"small trusted surface" thesis and `grep -ci 'trusted surface' DESIGN.md`
returns 0, so that was an owner tradeoff dressed as an established premise. It
is withdrawn.

**Owner:** the slice that next adds a dependency to `Cargo.toml`, or the G2
pass, whichever comes first. **Condition on the deferral:** the test must not
grow further substring predicates in the meantime — the escapes are enumerated
above and in its doc, and adding heuristics to chase them is what six review
rounds established does not work here.

**Why deferring is defensible rather than convenient.** The oracle guards
wiring that is *correct at this head and proven so by execution*: at this head
three distinct platform Clippy jobs — `lint`, `lint (windows)` and
`lint (macos)` — were green in the same run. `lint (macos)` itself runs only on
`macos-latest`; an earlier draft of this row said it "ran green on all three
platforms", which is not a thing a single job can do. `lint (windows)` is green
at this head, and was *not* green throughout: it failed on the first run of this
pull request, over the three annotations the merge had dropped, which the body
records. A weak regression oracle risks a *future* silent change, not a present
defect, and the row names exactly what that change would look like. The
alternative dispositions were considered and rejected: `accepted-risk` overstates
the acceptance, because the repair is specified and intended rather than waived;
and deleting the test would regress `PR5D-MSVC-CLIPPY-NEVER-RUN`, whose Windows
guard this test contains.

**What is not in doubt.** `lint (macos)` exists, is wired into the aggregate,
and **passed on its first run in this repository's history** — the tree is
clean under macOS Clippy by measurement. `PR5-MACOS-CLIPPY-NEVER-RUN` is closed
by that leg. This row is about the strength of the regression oracle guarding
the wiring, not about whether the wiring is correct today.

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
frozen file, and a behaviour-adjacent one. A module-level `#![allow(clippy::expect_used)]` is a Rust
attribute, not an allowlist entry — `effects/allowlist.toml` governs the effect-denial
lints only, and an earlier draft of this row said otherwise. It loses on its own
terms instead: it suppresses the lint for every call in the module rather than
the one that needs it. Leaving it unannotated fails `clippy -D warnings` on the integration branch, because
master's `[lints.clippy]` denies `expect_used` — so the branch could not pass its own
gate.

### 2026-08-28 — `PR5-MACOS-CLIPPY-NEVER-RUN` fired, and is REPAIRED

Its owner clause names the slice that next opens `.github/workflows/ci.yml`. The
master merge carries a `ci.yml` change, so the trigger fired and **this slice was
the named owner**. An earlier draft of this row recorded it as "carried, with an
owner" and declined the repair as scope creep. The delta review of `d46e48f`
rejected that, correctly: the row named no successor owner, no concrete
follow-up, and carried no owner re-ruling authorising another deferral. A
deferral by the named owner, to nobody, is not a disposition.

**The repair is the `lint (macos)` job.** The hole was precise. Ubuntu Clippy
configures out every `#[cfg(target_os = "macos")]` region; the Windows leg
configures out the same; macOS ran tests and MSRV but **no Clippy job at all**.
So a denied call in a macOS-only region — the lens's own example is an
`.expect()` inside the macOS `create_cloexec_pipe` at `src/agent/proc.rs:3870`
— could ship with every required check green.

`ci.yml` gains a `lint (macos)` job mirroring `lint (windows)` exactly, and —
because a dependency whose result nothing inspects is a job that enforces
nothing — it is added to the `merge-gate` aggregate in all three places that
matter: `needs`, the `LINT_MACOS_RESULT` env, and the loop that decides the
aggregate's exit.

**The line number in an earlier draft was wrong**, and the delta review caught
it: `src/agent/proc.rs:3857` is the **Linux** `create_cloexec_pipe`. The macOS
implementation begins at **3870**. The six macOS-only production regions the
lens named are `last_errno`, `group_has_non_zombie_members`,
`process_is_stopped`, `create_cloexec_pipe`, `clear_nonblocking`, and the
non-Linux `groups_are_quiescent`; it verified no currently denied call sits in
any of them, so the hole was open and unoccupied.

**What this row cannot claim until CI reports.** macOS Clippy has never run in
this repository's history. Whether the tree is clean under it is a measurement,
not an assumption, and the first run of the new leg is that measurement. If it
fails, the failure is a finding about the tree and not about this row.
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
| W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM | W1/W2 | On macOS CI the test binary died with `(signal: 15, SIGTERM: termination signal)` and no diagnostic, and the death landed on whichever tests were mid-flight — so it was attributed to innocent tests and read as a flake in a different subsystem each time. **Five instances across four pull requests that touch none of the named subsystems**, two of them visible only in attempt 1 of a rerun-in-place: #97 `9807f48` run `33674393240`; #103 `4517caa` run `33691549623`; #104 `ae59f2d` run `33741105025`; #108 `5b67179` run `33763282946`; #104 `94f8c27` run `33773356014`. **The group-kill hypothesis this finding carried is refuted**: no kill path signals a group that can include the harness, and cargo — in the same process group — survived both deaths and printed its error. What actually happens: the harness's own supervisor arms a **process-wide `SIGTERM`** when a freshly forked cleanup reaper has not said READY within 2 s and then does not acknowledge CANCEL within a further 2 s, and the monitor re-raises it; every container-exec fixture runs `git` through the host runner and every host-runner spawn enters one process-wide launch gate, so a stalled launch froze the module and the arm refused every waiter in one tick — which is why **a named burst with no summary is the signature**, not a counter-signature | **PR #115, merged at `046f17d`**, one file, three changes: the READY-timeout path kills and reaps the late reaper and fails that launch with an ordinary `Err`; `Reaper::cancel` reads until `OK` or EOF instead of judging the first byte; and **every arm site writes one async-signal-safe line to fd 2 naming itself** (`upstroke: fail-closed SIGTERM armed: …`), so the next occurrence is evidence rather than an absence. At `ae2a58f`, after M6's split, `arm_fail_closed_termination` is at `src/agent/proc.rs:2076` with five arm sites at `:1671`, `:2010`, `:2131`, `:2173`, `:2295`. **Guard**: `agent::proc::tests::a_late_reaper_fails_its_launch_without_arming_termination` (`src/agent/proc.rs:4635`), driven through the subprocess helper at `:4562`; **reverting either behavioural change fails it**. Eight-command baseline ALL 8 PASS at `741364b`; CI run `33780942121` green on every leg, macOS `1800 passed; 0 failed`. **What one green run does not prove**: it is consistent with the fix and with luck alike — the evidence that counts is the shape staying absent, and the fd-2 line if it returns. **Recurrence test**: a macOS death carrying that fd-2 line is this row returning; a macOS death without it is something new and gets its own ID | a measurement apparatus that reports about something other than what it was pointed at — the same class as `PR4-CI-ENVIRONMENT-ASSUMPTIONS`. **Two reading errors kept it hidden and both are the same shape**: a rollup shows only the latest run at a SHA, and a run row shows only its latest attempt, so two of the five instances were invisible to every "enumerate the runs" check. **And one categorisation error kept the fifth out**: it was ruled out of this row by searching for the marks of an ordinary failure, finding none, and concluding "not this" — ruling out a signature by the absence of other things instead of by searching for the one line that defines it. See §43 |

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
| PR5D-MSVC-CLIPPY-NEVER-RUN | **`cargo clippy` has never run against the Windows target on this project**, so every `#[cfg(windows)]` line in the crate is unlinted. `ci.yml`'s `lint` job runs on `ubuntu-latest` only; the local gate set runs `cargo check --target x86_64-pc-windows-msvc` — `check`, not `clippy`. Running `cargo clippy --target x86_64-pc-windows-msvc --all-targets --all-features -- -D warnings` from this box found two things at once: a real `disallowed_types` violation in `src/main.rs`'s `#[cfg(windows)]` test region (repaired in-slice by widening that file's recorded allow — the union-over-platforms case the allowlist header predicts), and a **pre-existing** `error: items after a test module` at `src/agent/proc.rs:1097` that no gate has ever seen | project owner / PR6–PR7 implementer | **The `main.rs` half is repaired; the `proc.rs` half is not, deliberately.** Clearing `clippy::items_after_test_module` means moving ~250 lines of Windows-only code above an inline `mod tests` in `src/agent/**` — a reordering with no behavioural content, in a file this slice does not own, that would put a large diff in a lane whose `production_effect` is "none in behavior". **Adding the gate is therefore also deferred**, because a gate that fails on arrival is not a gate. What this slice does instead is *measure* it and record the numbers: with the unrelated lint suppressed, the three denial lints are **clean** on the msvc target and **nine of the twelve `windows_sys` denials fire on real code** — which is the evidence the Windows half of the denylist is not decorative. **CLOSED by repair round 3 (`PR5-CONF-014`).** Both halves it deferred are done: `src/agent/proc.rs`'s `items after a test module` is cleared by moving that `#[cfg(test)] mod tests` to the end of `mod windows_job` (a pure reordering, no behavioural content, and `cargo fmt` clean), and `ci.yml` gained a `lint (windows)` job running `cargo clippy --all-targets --all-features -- -D warnings` on `windows-latest`, required by `merge-gate`. Verified on the Windows Server 2025 guest: clippy rc=0 on the repaired tree. `effects::tests::ci_yml_lexically_names_a_clippy_job_per_platform_and_the_aggregate_lists_it` pins the job, its `needs` entry and its place in the aggregate's required-gate loop, and each of the three was witnessed dying to its own mutation. The **macOS** half of the same hole is new row `PR5-MACOS-CLIPPY-NEVER-RUN` |
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

## 24. 2026-08-29 actionable sweep — workspace registration recovery

This is an append-only disposition of the live §13 workspace-recovery rows, measured from
integration `ff86d29a72ccc23e0d86c6fadabe2aa198ff46b8`. It does not rewrite their historical
evidence.

| stable ID | disposition | exact scope and evidence |
|---|---|---|
| `PR5-RD-002` | **fixed in this slice** | Recovery now reads registration paths byte-safely, revalidates containment of the base, private root, and every linked-worktree administration directory immediately before removal, binds a target only through an exact valid `gitdir`, and can reclaim the deterministic zero-length-`commondir` case. The final repair additionally rereads identity immediately before locked handling and direct empty-`commondir` admin removal. Mutation witnesses cover exact recovery, pre-mutation refusal, containment, idempotence, and non-UTF-8 Unix paths. |
| `PR5-RD-003` | **fixed by owner-authorized convergence semantics** | A valid registration with an empty `commondir` is recoverable. A per-registration `gitdir` that is already absent is treated as already-gone forced-cleanup convergence without inferring or deleting an administration directory; Git prune handles its own metadata. A missing whole worktrees store refuses while the checkout target exists, paths must be absolute and normalized, Windows decoding is strict UTF-8, and identity movement immediately before deletion refuses. |
| `PR7-R3-ATTEMPT-003-RESIDUE-DISCARD-UNREACHED` | **blocked by packet/owner contradiction, no repair applied** | The current packet and tests classify the pre-intent ephemeral commit as Git-owned R27 and require recovery to leave it. Deleting it would contradict that explicit contract. Reclassification requires an owner packet decision. |

The Linux-deterministic registration repair does not claim to resolve the separate controlled-macOS
errno/process fingerprints. Those remain external-measurement blockers. The implementation commit is
`2fc8678c6017031f44dce5d76cb47829a0079dae` with sole repair `e35ea2d4b0ce03a983131b196d9eb4a3548758a3`; publication/review/CI evidence is recorded by the PR
that carries this section.

## 25. 2026-08-29 actionable sweep — process funnel authority

This append-only disposition was measured from integration
`ff86d29a72ccc23e0d86c6fadabe2aa198ff46b8`. It does not rewrite the historical evidence above.

| stable ID | disposition | exact scope and evidence |
|---|---|---|
| `PR5D-FUNNEL-RETURNS-A-COMMAND` | **fixed in this slice** | Production host and shell construction no longer exports a writable `Command`; translation helpers exist only in test-support modules. The denylist/wrapper census is fail-closed and a mutation witness refuses any production command-builder escape. |
| `PR5D-PROCESS-FUNNEL-TAKES-NO-SITE` | **fixed in this slice** | `ProcessSite::Spawn` and `ProcessSite::Terminate` travel by value through the host/process funnel, including termination. The process funnel is classified as a funnel rather than legacy, and the two absent-site rows are removed. Existing containment-point and timeout witnesses plus the complete effects census cover the resulting path. |

Implementation commit `5a460f8a6cf2deae2dc1dd08615d097dce68ea00` passed the focused effects and
process gates; publication, exact-head CI, and bounded review evidence are carried by the PR containing this
section.

### PR #47 bounded-review residue

The sole review of public head `a36f0890fee71286d213ff61bbd15bd6a1a55eef` was triaged once and its sole
repair pass committed as `4119612`. The owner-authorized final dispositions after that pass are:

| stable ID | disposition | exact residue |
|---|---|---|
| `PR47-WINDOWS-TEST-ALLOW-NOT-GOVERNED` | **fixed by owner-authorized mechanical exception at `ca2d702`** | The Windows-only test exception is now module-level, carries the required allowlist marker and justification, and is represented in the frozen allowlist fixture. The exact governed-allow and frozen-allowlist witnesses, native and Windows-target Clippy, MSRV, formatting, docs consistency, and PR policy pass. Full-suite and hosted exact-head evidence remain required before merge. |
| `PR47-PUBLIC-PROCESS-API-REMOVED` | **accepted residual; deferred compatibility concern by owner ruling** | The reviewed production APIs remain absent or signature-incompatible. The owner explicitly accepts the current compatibility-wrapper behavior for this PR and defers the residual concern; the exceptional repair authority is limited to the governed-lint mechanical fix and does not authorize a broader second repair. Preserve this row for a later compatibility-owned slice rather than treating it as a gate blocker for this PR. |

The other review findings are fixed by the delivered pass: Process sites are release-validated and carried
through termination, and the writable-`Command` export guard is structural rather than spelling-based.

## 26. 2026-08-29 G2 W1 — fold-door and probe-pair obligations

This append-only disposition applies the owner-authorized W1 acceptance matrix derived on-box from the
authoritative private packet. Private packet text is not reproduced here. The slice starts from integration
`bc30d7c38e5ed69939315a05bf870a9bf745b139`. Exact-head hosted evidence remains required before these
implementation dispositions become final.

| stable ID | disposition | exact scope and evidence |
|---|---|---|
| `TASK-DISPATCHED-REGION-UNVALIDATED` | **implementation-fixed at `185e392`** | Dispatch validation derives the predicted region from the frozen fold state and refuses an event-carried region that differs. Shape, round-trip, and mutation witnesses exercise the derivation boundary. |
| `PR7-G2-W1-SUCCESS-IGNORES-THE-FROZEN-PLAN` | **implementation-fixed at `c0ec940`** | Candidate success is judged against the task's frozen review plan: configured passes must be present and successful, so a partial or empty recorded pass set cannot satisfy the door. Focused and mutation witnesses cover the plan-presence gate. |
| `PR7-G2-W1-RETAINED-ARM-UNGUARDED` | **implementation-fixed at `46780ea`, review repair `d895dd5`** | The retained settlement arm validates record/envelope identity and now refuses a record whose public predicate says successful, preserving candidate preparation as the sole successful settlement. Red-first sibling-arm and mutation witnesses cover the accepted P1 repair. |
| `PR7-G2-W1-PROBE-PAIR-NOT-OBLIGED` | **implementation-fixed at `6c6cb3d`, review repair `d895dd5`** | Probe registration authority is structural and accounted: the execution seam is sealed, the caller grants the slot/ledger capability, and P4 verifies the granted capability recorded each probe. Red-first cooperative/substitution witnesses and mutation checks cover the accepted P1 repair. |

The four rows are one collision set and one W1 PR. `6c6cb3dabcb8f30ef8ae7e24703eb36ba762b86f` is the
implementation head before the root-owned ledger commit. The single independent review of ledger head
`e0e7ade69b0298a43d26adcea5c1b761c750d376` returned two P1 code blockers and this P2 evidence-wording
defect; root triaged all three once. The same implementor delivered the sole code repair at `d895dd5`; this
ledger correction is root's part of that bounded disposition. There is no second review. Exact public head,
globally serialized full-suite results, and final hosted gates are recorded externally in the phase audit so
the ledger does not make a self-referential commit claim.

## 27. 2026-08-29 actionable sweep — structural CI workflow oracle

This append-only disposition applies to integration
`982474a4dc60ed6291c4f3394b59bdd820edec75`. It does not rewrite the historical evidence above.

| stable ID | disposition | exact scope and evidence |
|---|---|---|
| `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE` | **implementation-fixed at `a5f3706`, review repair `647a70b`** | The platform-gate oracle parses the workflow as YAML 1.2 with duplicate-key rejection. The sole review found six implementation defects: custom/default shell escapes, matrix exclusions, non-injective result stems, unknown cfg values counted as coverage, non-conjoined item/module guards with non-gating forms conflated, and incomplete Rust cfg lexical handling. Root accepted all once; the same implementor's sole repair pins effective shells and full strategy shape, rejects stem collisions, evaluates complete per-invocation cfg valuations, conjoins effective guards, separates `cfg!`/`cfg_attr`, and reads raw/escaped literals. Focused controls cover all six. The owner-authorized parser remains a dev dependency only. No workflow file changed because the repaired oracle found no live wiring defect. |
| `CI-CFG-UNSHIPPED-UNIX-REGION` | **deferred — deliberate unsupported-target residue discovered at `647a70b`** | The repaired effective-guard census proves `src/agent/proc.rs` contains one `unix && !linux && !macos` fallback region compiled by no Linux/macOS/Windows CI invocation. The region preserves compilation on an unshipped fourth Unix family; current project scope names Linux, macOS, and Windows only. The census now requires this exact acknowledged set and fails if it changes. Adding a BSD runner is a platform-scope decision, not an in-slice oracle repair. |

The implementation head before review is `a5f3706e191bc13041ad0e6bf4885833617ef390`. The one
independent exact-head read-only review of ledger head `6622b5f72b124963165926380e843f4d15bd979c`
returned five P1 and two P2 findings; root triaged all seven once. Six implementation findings were repaired
in the sole bounded pass at `647a70b`; the seventh was this ledger evidence correction. There is no second
review. Globally serialized full-suite and final hosted evidence are recorded externally; exact final public
head remains external to avoid a self-referential ledger claim. The historical
`PR5D-MSVC-CLIPPY-NEVER-RUN` test-name citation above is superseded by this structural effective-predicate
census rather than rewritten in place.

## 28. 2026-08-29 terminology correction — the Windows retry-bound row is an intermittent production defect

This append-only section corrects one classification and **rewrites no historical evidence**.

An earlier commit on this branch, `ce4cee15a2267c53a1981b97d7a97514567f0a00`, edited two historical
passages in place: the trailing clause of `PR7-WIN-READ-RACING-BOUND-TOO-SHORT`'s disposition in
§2, and the sentence about the same row in §23's "What rides with it". Both edits were narrow and
changed no disposition, but this file's rule is append-only whatever the width of the edit, and a
correction applied in place leaves a reader unable to see what the row said when it was settled.
**Both passages are restored byte-for-byte to their state at
`859fa6e046d32bcf9775f1e8ac0d90aa89f3f491`**, and the correction they attempted is carried here
instead — the same route `PR4-PROGRAM-PATH-NOT-UNICODE-CLOSED-NARROWED` took, and the same one
§27 used for the superseded `PR5D-MSVC-CLIPPY-NEVER-RUN` citation.

| stable ID | disposition | exact scope and evidence |
|---|---|---|
| `PR7-WIN-READ-RACING-BOUND-TOO-SHORT-TERMINOLOGY` | **terminology corrected; the row's disposition, owner, rate, evidence and repair fork are unchanged** | `PR7-WIN-READ-RACING-BOUND-TOO-SHORT` is an **intermittent production defect**, not a flake. The head of that row already says so — it was retitled 2026-08-26 on the frontier review's judgement that calling it a flake *"understates the category"* — but two sentences kept the older word: the row's own closing clause in §2, *"§12 is the precedent for carrying a flake with its numbers rather than a description"*, and §23's *"Carrying a flake with its rate rather than a description is §12's precedent."* **Both stand exactly as written**; read them with this row. The mechanism is identified, which is what settles the category: `container.rs::read_racing` spins `RACING_ACCESS_ATTEMPTS = 64` yields on any IO error other than `NotFound`, and a competing Windows open returns `PermissionDenied` (os error 5) for as long as the winner holds the handle — its whole open/read/close cycle — which under full-suite load on a 16-vCPU guest can outlast 64 yields. `CODING_STANDARDS.md` §12 rules that a failure with an identified mechanism on a supported platform is a defect at whatever rate it occurs. **Intermittency is the symptom's shape, not the category**: the cumulative 5 red of 10 full-suite guest runs, across three heads and two tests, is what made the defect observable, not what makes it a flake — and the triage is the row's own, change the bound rather than re-run. `PR7-MACOS-PROCESS-GROUP-FLAKE` is **not** reached by this correction: no mechanism has been identified for it, so its provisional flake classification stands under the same §12 rule. |

Provenance: raised as `PBLA-LEDGER-001` by the initial Lane A review of
`ce4cee15a2267c53a1981b97d7a97514567f0a00`, one of four findings root triaged once and accepted.
This section is the whole of the ledger's part in that bounded repair pass. No other row in this
file is altered, and no disposition anywhere in it is reopened.

## 29. 2026-08-30 PR #73 owner adjudication — narrowed residuals and temporary restrictions

This append-only section records the owner's Fable 5 adjudication of the fresh review of
implementation head `0a28c1ab57ae07da151a96393c94f94b14205885`. It rewrites no historical
finding. The owner found PR #73 a measured net improvement with no introduced regression and
authorized these four limitations to remain open/deferred under the recorded restrictions and
follow-up triggers. The separate findings-only commit carrying this section inherits the review
under `decisions/2026-08-20-review-invalidation-scope.md`; both SHAs are recorded in the PR body.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR73-TARGET-INVENTORY-001 | P3 | 0a28c1ab57ae07da151a96393c94f94b14205885 / src/effects/tests.rs:155 | a workspace member or path dependency is added outside the current package -> per-package Cargo inventory omits that member -> scanned_sources never walks its sources -> a governed effect in the member is outside this package census | pre_existing | correctness | scanned_sources src and examples boundary predates PR #72; comparison head 0f05b456fa226f9f83332aa88c152909d8cf850c | Target inventory test and foreign-source-root refusal remain live; true residual is per-package scope; workspace-member and path-dependency additions remain owner events; inventory-pin edits must widen the walk in the same diff; roots-within-walk containment is queued | deferred |
| PR73-LEXICAL-CLOSURE-001 | P2 | 0a28c1ab57ae07da151a96393c94f94b14205885 / src/agent/proc/test_support/readiness.rs:303 | a function-value alias of std::fs::remove_file is added inside the site-6 expected statement -> Clippy emits multiple matching diagnostics in that statement -> the single expectation suppresses all of them -> the six-site expectation count and lexical call census remain green | pre_existing | security-trust | whole-file allowance predates PR #72; per-site expectation placement is at 0a28c1ab57ae07da151a96393c94f94b14205885; comparison head 0f05b456fa226f9f83332aa88c152909d8cf850c | File-level deny plus six single-call expectations are a narrowed improvement; until hardening there is exactly one denied path per expected statement and no aliases or function values under an expectation; narrow the claim to site count and two pub GovernedAllow fields | deferred |
| PR73-LEXER-DIVERGENCE-001 | P3 | 0a28c1ab57ae07da151a96393c94f94b14205885 / src/effects.rs:1096 | a Unicode XID identifier or macro name reaches the ASCII-only scanner -> word and macro_at do not consume the Rust token as one unit -> the scanner can refuse valid code or walk a macro body as source items -> the pinned whole-file test-module oracle is the current backstop | pre_existing | correctness | scanner origin predates PR #72; behavior is identical at comparison head 0f05b456fa226f9f83332aa88c152909d8cf850c | Pinned whole-file inventory catches the measured test-only invention; governed sources remain ASCII outside comments and strings; blanked-view ASCII census is queued; syn and proc-macro2 remain reserved; a third lexer-class recurrence triggers migration adjudication | deferred |
| PR73-LINT-SEMANTICS-001 | P3 | 0a28c1ab57ae07da151a96393c94f94b14205885 / src/effects.rs:2868 | cfg_attr carries a governed lint level -> the direct-attribute reader ignores the conditional attribute -> its reported level can differ from rustc effective lint level -> every_allow or funnel-child census is the current refusing backstop | pre_existing | correctness | direct-attribute reader predates PR #72; identical blindness at comparison head 0f05b456fa226f9f83332aa88c152909d8cf850c | every_allow census, restatement refusal, and rustc fixture table cover the measured active-allow and inactive-deny cases; cfg_attr carrying a governed lint remains forbidden; reader remains direct-attribute-only | deferred |

## 30. 2026-08-31 PR #64 successor slice 1 — token-carried scratch-tree authority

This append-only section records the first successor slice implementing the owner's
2026-08-30 two-token deletion-authority decision. Implementation head
`24830622530ae3998771dcebc39d51811730af2e` is based on green integration
`f7fe2c3be232ea6de98299b6500b6369648a344e`. `PrivateHalfProof` and its twelve
fail-closed conjuncts remain unchanged. The new `ScratchTreeOwnership` exists only
under `cfg(test)`, binds the exact root acquired by an exclusive create, and adds no
production effect site or census row. The root-owned ledger append follows the
implementation commit so its final commit is intentionally recorded outside this
self-referential section.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR64-CLEANUP-003-SCRATCH-PRECLEAN | P2 | 24830622530ae3998771dcebc39d51811730af2e / src/rundir.rs:3612 | a test helper derives a predictable scratch path -> it discards the result of recursive pre-clean -> an occupied root can be deleted before the fixture establishes ownership -> another test or process loses content it owns | pre_existing | correctness | the helper predates PR #64 and is byte-identical at base f7fe2c3be232ea6de98299b6500b6369648a344e | ScratchTreeOwnership authority landed and is witnessed: acquire creates a ULID-named root with a non-recursive exclusive create, refuses occupied or undecidable roots, and never pre-cleans; occupied and undecidable mutation witnesses fail if pre-clean is restored. The legacy scratch helper is not migrated in this head; slice 2 moves its callers onto the new authority | deferred |
| PR64-CLEANUP-003-P5B-SCOPE | P3 | 24830622530ae3998771dcebc39d51811730af2e / src/topology/effects.rs:1997 | the P5b identity says no path deletes the private half without qualification -> the test build gains a second deletion authority -> a reader treats the sentence as covering test-owned scratch trees -> either the sentence or the authorized mechanism appears to violate the other | introduced_by_feature | docs-contract | the unqualified sentence predates PR #64; the second test-only authority is introduced by this successor | The two leased boundary locations now state the run-lifecycle scope and name the sole test-build exception; conjunct 12 and PrivateHalfProof are unchanged. A committed-bearing scratch tree is retained as PossiblyCommitted by the private-half proof and reclaimed only by its scratch token | fixed |
| PR64-CLEANUP-003-RECLAIM-SILENCE | P3 | 24830622530ae3998771dcebc39d51811730af2e / src/rundir.rs:3165 | a guard reclaim fails on a non-unwinding path -> failure is only printed -> the suite remains green while a tree leaks -> repeated failures can exhaust build-host inodes | introduced_by_feature | correctness | a measured implementation mutation replacing the normal-path panic with the unwind arm's report left every other witness green | The normal path raises and names the tree, while an already-unwinding path reports without double panic; dedicated witnesses cover normal failure, suppressed unwind failure, and reclaim on both normal return and unwind | fixed |

The exact implementation full suite ran through the globally serialized build wrapper:
library 1,787 passed with 34 ignored, CLI 8 passed, and the example target had no
tests. The fresh exact-head independent review and hosted checks remain required;
this section grants neither review nor merge authority.

## 31. 2026-08-31 PR #77 exact-head review — bounded scratch-authority repair

This append-only section records the sole independent review of exact head
`7db77d92a7bc7a9d80bea788453acfbf90a0eaa3` and the same-implementor bounded
repair at `aa7a8ff6cf4d31bf76827a2048a504c0693e3269`. It does not rewrite the
slice-1 dispositions in section 30. The earlier Windows gate failure and these
review findings belong to the same reviewed implementation lineage and count as
one unsuccessful exact-head attempt under the standing convergence policy.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR77-SCRATCH-UNWIND-REPORT-PANICS | P2 | 7db77d92a7bc7a9d80bea788453acfbf90a0eaa3 / src/rundir.rs:3159 | an original panic unwinds through the scratch guard -> reclaim fails -> the unwind arm invokes an infallible-printing macro -> stderr reporting fails and panics -> the destructor double-panics and aborts, losing the original diagnosis | introduced_by_feature | correctness | 24830622530ae3998771dcebc39d51811730af2e / PR64-CLEANUP-003-RECLAIM-SILENCE | Repair aa7a8ff uses a fallible reporter whose result is explicitly matched; a deterministic reporter-failure witness proves simultaneous reclaim and report failure remains suppressed during unwind, while the non-unwinding reclaim failure still panics | fixed |
| PR77-SCRATCH-ULID-WITNESS-ABSENT | P2 | 7db77d92a7bc7a9d80bea788453acfbf90a0eaa3 / src/rundir.rs:2991 | ULID generation is replaced by a process ID or constant -> existing fixtures use distinct tags or bypass public naming -> all prior witnesses remain green -> two acquisitions for the same tag can collide instead of producing fresh roots | introduced_by_feature | correctness | 24830622530ae3998771dcebc39d51811730af2e | Repair aa7a8ff adds a same-parent, same-tag witness that requires distinct roots and ULID-shaped basenames; the PID and constant destructive mutations fail only this new witness, proving it closes the prior evidence gap | fixed |
| PR77-DECISION-EFFECT-SITES-PATH | P3 | 7db77d92a7bc7a9d80bea788453acfbf90a0eaa3 / decisions/2026-08-30-test-scratch-tree-ownership.md:14 | the decision cites effects/effect_sites.json -> that tracked path does not exist -> a maintainer cannot follow the claimed unchanged-inventory evidence to its authority file | introduced_by_feature | docs-contract | 24830622530ae3998771dcebc39d51811730af2e | Repair aa7a8ff corrects both references to the tracked root-level effect_sites.json; the artifact blob and its 70-total/14-RunDir census remain unchanged from the base | fixed |

The review explicitly preserved the owner-deferred
`PR64-CLEANUP-003-SCRATCH-PRECLEAN` slice-2 migration as non-blocking residue.
The repaired exact head still requires the globally serialized full suite, fresh
exact-head review, and hosted gates; this ledger append grants neither review nor
merge authority.

## 32. 2026-08-31 PR #64 successor slice 2 — emit scratch fixtures spend token authority

This append-only section records the second successor slice under the owner's
2026-08-30 scratch-tree authority decision and controlling addendum. Implementation
head `6555870ef62bef5f8de5b598496783466646a092` is based directly on green
integration `82874ef70dd4acf074cbf1453e28651d78af4db3`. The root-owned ledger
append follows that implementation commit, so the final reviewed SHA is recorded
in the PR body rather than self-referentially here. The old PR #64 branch remains
historical input only and is not replayed or merged.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR64-CLEANUP-003-SCRATCH-PRECLEAN | P2 | 6555870ef62bef5f8de5b598496783466646a092 / src/engine/topology/emit/tests.rs:276 | an emit fixture derives a predictable process-and-counter root -> it recursively pre-cleans that root while discarding the result -> a recycled process identity or overlapping fixture can make another owner's tree reachable -> setup deletes content before proving exclusive ownership | pre_existing | correctness | PR64-CLEANUP-003-SCRATCH-PRECLEAN / slice-1 section 30 deferred this caller migration | Emit fixtures now acquire a previously nonexistent ULID-named root through ScratchTreeOwnership before any fallible run-tree construction; Arc clones share one non-Clone token owner; the last holder spends the token through remove_scratch_tree. Occupied-root, partial-construction, return, unwind, shared-lifetime, confirmed-absence, normal-failure, suppressed-failure, and Windows drop-order witnesses are live. The five required destructive mutations each kill only its assigned witness. No RemovePublicHusk call, ancestor-targeted production funnel, raw deletion, predictable root, pre-clean, forged proof, or discarded reclaim result remains in emit/tests.rs | fixed |

## 33. 2026-08-31 PR #78 exact-head review — scratch-fixture authority correction

This append-only correction records the sole review of exact head
`17dbf7adbfaefc964ebdedbbcce200350d9ab72a` and the same-implementor repair
`02b64604477c28b3ce24ed86a53cc5c81b916960`. It does not rewrite section 32:
that section incorrectly reused the legacy pre-clean ID for a distinct emit-local
helper. The original `rundir::tests::scratch` hazard remains deferred, while the
emit helper's actual predictable-root and missing-reclamation defect is fixed under
its own stable ID.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR78-EMIT-UNWIND-REPORT-LOST | P2 | 17dbf7adbfaefc964ebdedbbcce200350d9ab72a / src/engine/topology/emit/tests.rs:235 | an ordinary assertion panics -> the last Scratch holder drops -> real reclamation fails -> the failure is parked only in an OwnedTree-local slot -> field destruction drops the sole token and message -> the tree leaks while the original panic reports no cleanup failure | introduced_by_feature | correctness | 6555870ef62bef5f8de5b598496783466646a092 | Repair 02b6460 routes the unwind arm through the scratch subsystem's fallible reporter before touching the witness slot and explicitly suppresses reporter failure; a no-observer witness proves the report remains externally visible, while injected witnesses recover, rearm, release guards, and only then assert | fixed |
| PR78-SCRATCH-REMOVER-SEAM-AUTHORITY | P2 | 17dbf7adbfaefc964ebdedbbcce200350d9ab72a / src/rundir.rs:3081 | any crate test receives a generic remover callback -> it ignores the token path or deletes an ancestor -> it returns success without deleting the token root -> the only token is consumed -> API construction no longer confines reclamation to the owned root | introduced_by_feature | security-trust | 6555870ef62bef5f8de5b598496783466646a092 | Repair 02b6460 keeps Remover, remove_scratch_tree_with, guarded_with, and Reporter module-private and exposes only a pathless refusal operation whose private stateless remover always returns PermissionDenied and whose return type has no success case | fixed |
| PR78-LEDGER-PRECLEAN-FALSE-CLOSURE | P2 | 17dbf7adbfaefc964ebdedbbcce200350d9ab72a / reviews/FINDINGS.md:3600 | section 30 defers the predictable pre-clean in rundir tests -> section 32 attributes that sequence to the distinct emit helper -> the legacy helper remains byte-present -> the reused stable ID is marked fixed -> maintainers receive a false closure of a live deletion hazard | introduced_by_feature | docs-contract | PR64-CLEANUP-003-SCRATCH-PRECLEAN | This append-only section restores the original row to deferred and records the emit-local repair under PR64-EMIT-SCRATCH-PREDICTABLE-LEAK; the PR body uses the same corrected separation | fixed |
| PR64-CLEANUP-003-SCRATCH-PRECLEAN | P2 | 17dbf7adbfaefc964ebdedbbcce200350d9ab72a / src/rundir.rs:3834 | rundir tests derive a predictable process root -> they discard recursive pre-clean failure -> an occupied root can contain another owner's data -> setup removes that data before acquiring token-carried authority | pre_existing | correctness | PR64-CLEANUP-003-SCRATCH-PRECLEAN | The slice-1 token authority and slice-2 emit migration do not change this legacy helper. It remains assigned to the separately bound startup, recover, and create migration follow-up; reopen when that collision set receives an exact-base lease, and require occupied-root preservation plus no pre-clean or discarded cleanup result | deferred |
| PR64-EMIT-SCRATCH-PREDICTABLE-LEAK | P2 | 17dbf7adbfaefc964ebdedbbcce200350d9ab72a / src/engine/topology/emit/tests.rs:96 | an emit fixture derives a process-and-counter root without exclusive acquisition -> fixtures never reclaim the root -> process identity reuse or repeated runs encounter stale content -> tests fail as fresh-run assumptions collide and temporary trees accumulate | pre_existing | correctness | PR7-SCRATCH-FIXTURE-LEAK | Repair 02b6460 acquires a previously nonexistent ULID root before fallible construction, shares one non-Clone token through Arc, and spends it on final drop; 17 original tests retain names and semantics, 9 ownership and failure witnesses pass, and ten repetitions leave the observed temporary-entry count unchanged | fixed |

## 34. 2026-08-31 PR #78 Fable convergence — external unwind-report oracle

This append-only section records the fresh review of exact repaired head
`4099d57b24c8b7d2dd44aeb3e3b24272eacf1a9c`. That review was the lane's
second unsuccessful reviewed attempt, so the standing convergence policy froze
the branch and prohibited another ordinary repair. Fable 5 authored the bounded
convergence repair at `6a217865f978b5319007c55c269fb48e26823dc3` without
reopening the settled scratch-deletion authority or widening the three-file lease.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|---|
| PR78-EMIT-UNWIND-REPORT-ORACLE | P2 | 4099d57b24c8b7d2dd44aeb3e3b24272eacf1a9c / src/engine/topology/emit/tests.rs:2575 | external stderr reporting is removed -> the unwinding arm sets its own delivered record -> the in-process observer and no-observer witnesses still pass -> a failed scratch reclaim again loses its cleanup report while the evidence claims the reporter is fixed | introduced_by_feature | correctness | PR78-EMIT-UNWIND-REPORT-LOST / repair 02b64604477c28b3ce24ed86a53cc5c81b916960 | Fable repair 6a21786 moves delivery observation outside the process: rundir scratch-tree witnesses spawn the exact emit test, capture its real fd 2, assert one correctly shaped report, assert silence for a successful unwind reclaim, and on Linux route fd 2 to /dev/full to prove a real write failure is suppressed without replacing the original panic. Five destructive mutations, including removing the stderr write and removing the reporter call, each turn the assigned oracle red and are absent from the committed tree | fixed |

## 35. 2026-08-31 G2 checkpoint — the full-ledger audit `decisions/2026-08-25-checkpoint-merges.md` ordered

This append-only section is the **full-ledger audit** the checkpoint record
requires before a candidate is cut. It rewrites no historical row and reopens no
disposition. It is measured at candidate head
`50ed8c86ec60164011bfd393066c4c3696d3865b`, and its rule is
**latest-disposition-wins**: where a row in §2 has been repaired, closed or
superseded by a later section, the later disposition is the live one and §2's
text stands as history.

The audit is mechanical. Row counts come from the file, not from a prior
summary, and every count below is reproducible from `reviews/FINDINGS.md` at
this head.

### The recount, and one structural defect it found

§2's table occupies lines 112–176 (line 136 is blank). That is **64 physical
table lines** — but **65 logical rows**, because one line carries two.

**Line 156 joins two ledger rows with `||`.** It holds
`PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE` and then, after a doubled pipe,
`PR7-CANDIDATE-TREE-UNVERIFIED`. Split on `|`, that line has **11 fields where
every other row in the table has 6**. The consequence is not cosmetic: in
rendered Markdown a `||` produces an empty cell and the second row's content
spills into the first row's columns, so **`PR7-CANDIDATE-TREE-UNVERIFIED` is
invisible as a row to anyone reading the rendered ledger**, and any line-based
count of §2 under-counts by exactly one.

This is §4's own shape — a measurement present in the file that no reader is
positioned to act on. It is recorded here rather than repaired in place, because
this file is append-only whatever the width of the edit, and the same route
§27 and §28 took. **The row's disposition is unaffected**: its own text carries
`FIXED 2026-08-26`, and it is dispositioned as repaired below.

### The normalized totals

| Bucket | Rows |
|---|---:|
| §2 logical rows | **65** |
| — struck in place (withdrawn, historical) | 2 |
| **Live rows carried into this audit** | **63** |
| — **repaired**, by a later disposition | 8 |
| — **closed, not owed** | 3 |
| — **carried**, with a named venue, a re-opening trigger and required evidence | 52 |

8 + 3 + 52 = 63. **Every live row lands in exactly one bucket**, which is the
condition the checkpoint record states.

The two struck rows are `~~PR4-MAIN-WIRING-UNWITNESSED~~` (line 134, deferral
withdrawn as invalid, repaired in round 8) and
`~~PR5-R1-PROCESS-START-CENSUS-UNSTRIPPED~~` (line 138, closed by PR7's census
repair). They are already withdrawn in place and are not re-dispositioned here.

### Repaired — 8 rows whose §2 text is now history

| ID | Where the repair is recorded | What discharges it |
|---|---|---|
| `PR5-MACOS-CLIPPY-NEVER-RUN` | §3, 2026-08-28, "fired, and is REPAIRED" | Verified live at this head: `.github/workflows/ci.yml:113` defines `lint (macos)`, and it is wired into the aggregate in all three places — `merge-gate.needs` (`:152`), `LINT_MACOS_RESULT` (`:161`), and the loop that decides the aggregate's exit |
| `PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE` | Its own §2 owner cell reads **fixed in PR7** | Warm-up probe discarded, median of the next three, premise asserted, bounded single retry. Ten consecutive guest runs green |
| `PR7-CANDIDATE-TREE-UNVERIFIED` | In-row, `FIXED 2026-08-26`; per-instance Class B approval in §3 | `PreparedCandidate` retains `tree_sha`; `verify_object` compares the commit's tree against it; a same-parent different-tree commit is refused by a dedicated witness. **This is the row hidden by the `||` join above** |
| `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4` | In-row, `FIXED 2026-08-26, measured at bd3b9cd`; §22b; `decisions/2026-08-26-durable-retry-feedback.md` | `FailureRecord` carries `detail`; the brief is derived from the log by `Brief::replay`. Four witnesses, each with a mutation that kills it and leaves the others green |
| `TASK-DISPATCHED-REGION-UNVALIDATED` | §26, implementation-fixed at `185e392` | Dispatch validation derives the predicted region from the frozen fold state and refuses a divergent event-carried region |
| `PR7-G2-W1-SUCCESS-IGNORES-THE-FROZEN-PLAN` | §26, implementation-fixed at `c0ec940` | Candidate success is judged against the task's frozen review plan; a partial or empty recorded pass set cannot satisfy the door |
| `PR7-G2-W1-RETAINED-ARM-UNGUARDED` | §26, implementation-fixed at `46780ea`, review repair `d895dd5` | The retained arm validates record/envelope identity and refuses a record whose public predicate says successful |
| `PR7-G2-W1-PROBE-PAIR-NOT-OBLIGED` | §26, implementation-fixed at `6c6cb3d`, review repair `d895dd5` | Probe registration authority is structural and accounted; the execution seam is sealed and P4 verifies the granted capability recorded each probe |

#### The W1 double-count, reconciled explicitly

Four rows appear **twice** in this file with opposite dispositions, and a naive
count of open rows counts each of them twice:

| ID | §2 (early) | §26 (later) |
|---|---|---|
| `TASK-DISPATCHED-REGION-UNVALIDATED` | line 155 — "Recorded, not repaired" | line 3473 — implementation-fixed at `185e392` |
| `PR7-G2-W1-SUCCESS-IGNORES-THE-FROZEN-PLAN` | line 160 — "Recorded, not repaired" | line 3474 — implementation-fixed at `c0ec940` |
| `PR7-G2-W1-RETAINED-ARM-UNGUARDED` | line 161 — "Recorded, not repaired" | line 3475 — implementation-fixed at `46780ea` |
| `PR7-G2-W1-PROBE-PAIR-NOT-OBLIGED` | line 162 — "Recorded, not repaired" | line 3476 — implementation-fixed at `6c6cb3d` |

**§26's disposition is the live one for all four.** The §2 text stands as the
record of why each was recorded rather than repaired at the time — the
`src/topology/**` closure of the 2026-08-24 adjudication, and round 6's stop
condition — and it is correct history. It is not an open obligation, and the
audit counts each of these rows once, as repaired.

The residual §22e entries for the same three W1 rows (lines 3268–3270) describe
the repair *shape* that was then implemented in §26. They are superseded by
§26 and are likewise not counted as open.

### Closed, not owed — 3 rows

The `PR4-PROGRAM-PATH-NOT-UNICODE` family is three §2 rows because each closure
and narrowing was **appended as a new row** rather than moving the old one, which
is this file's rule. All three are closed.

| ID | Rationale |
|---|---|
| `PR4-PROGRAM-PATH-NOT-UNICODE` | Closed as **not reproducible in production** by `decisions/2026-08-25-commandspec-program-stays-string.md`. Every production route puts a bare CLI name in `CommandSpec.program`; `DESIGN.md:222` is unchanged and the W4 widening is withdrawn |
| `PR4-PROGRAM-PATH-NOT-UNICODE-CLOSED` | "Closed, not repaired: there is nothing to repair." `Invocation::at`, the only constructor taking a path, is `#[cfg(test)]` and both call sites are in test modules; `Invocation::named` takes a `&str`, so `to_str()` cannot return `None` for anything production builds |
| `PR4-PROGRAM-PATH-NOT-UNICODE-CLOSED-NARROWED` | Owner disposition 2026-08-29: the final narrowing had reached the decision record, its index entry and the pass proposal but not this file. "Closed by this row"; it is this row's scope statement that binds |

**A note the count depends on:** these three are one finding, recorded three
times. Read as three open rows they overstate the ledger's debt by two.

### Carried — 52 rows, each with a venue, a trigger and required evidence

Every carried row already names an owner in §2; none is ownerless. What this
audit adds is a **venue class** with an explicit re-opening trigger and the
evidence a repair must produce, so a row cannot be deferred to nobody — the
failure `PR5-MACOS-CLIPPY-NEVER-RUN` was reopened for.

| Class | Venue / owner | `shrinks_when` | Re-opening trigger | Required evidence for the repair |
|---|---|---|---|---|
| **V1** | The G2 PR3-layer pass | the pass lands and `src/topology/**` reopens under a Class-B approval | the G2 pass opens `src/topology/**` | a fold-side refusal with a red-first witness **and** a mutation the witness kills, both halves recorded |
| **V2** | The project owner, for the G2 erratum list or a ruling | the owner writes the erratum clause or rules the question | the owner takes up the erratum list | the packet clause the ruling amends, quoted in the record, **plus** a test pinning the ruled behaviour |
| **V3** | A file- or behaviour-triggered successor slice — the owner clause names the path *and the change*, not merely opening the file | the named slice changes the named path or behaviour | a slice changes that path/behaviour; **incidental contact does not fire it** (the clause restated 2026-08-27 for `PR5-RD-002`) | a red-first witness on the named path, a killed mutation, and — for the platform rows — a reproduction on the platform that failed, never a Linux-only green |
| **V4** | A numbered future slice implementer (PR8, PR6/PR7, PR7–PR11, …) | that slice opens | the named slice opens | the slice's own per-head ceremony, with the row named in its ledger |
| **V5** | The project owner, undirected | the owner rules | an owner ruling, or new evidence admissible under §"The authority rule" | a concrete failure sequence **and** a surviving mutation, as §"The authority rule" requires of any challenge |
| **V6** | The post-v0.2 pass over PR3's layer | v0.2 completes and the pass opens | the post-v0.2 pass opens | as V1 |

**The bar for every class is the same in one respect**: a deferral by the named
owner, to nobody, is not a disposition. If a trigger fires and the owner declines,
the decline is recorded as a new row with a **new** named successor — never as
silence.

The 52 rows, with their class and the owner text §2 gives them:

| ID | Class | Venue / owner as recorded in §2 |
|---|:---:|---|
| `PR5-VERIFY-CLAUSE-NARROWER-THAN-STATED` | V2 | project owner — for the G2 erratum list |
| `PR3-ATTEMPT-SHAPE` | V5 | project owner |
| `PR5-ANSWER-MODULE-COLUMN` | V3 | PR6/PR7 implementer (the slice that next opens src/topology/effects.rs) |
| `PR3-RUNNER-DIGEST` | V5 | project owner |
| `PR3-REG-001-CONDITIONAL` | V4 | PR4-PR10 implementer |
| `PR3-BEFORE-PHASE-SCOPE` | V4 | PR7–PR10 implementer |
| `PR3-COMMIT-AUTHORSHIP` | V5 | project owner |
| `PR3-CONTAINER-START-ROW` | V4 | PR6/PR7 implementer |
| `PR3-FRAMEWORK-SILENT-1` | V4 | PR7–PR10 implementer |
| `PR3-FRAMEWORK-SILENT-2` | V4 | PR7–PR10 implementer |
| `PR3-FRAMEWORK-SILENT-3` | V4 | PR7–PR10 implementer |
| `PR3-FRAMEWORK-SILENT-4` | V4 | PR7–PR10 implementer |
| `PR3-FRAMEWORK-SILENT-5` | V4 | PR7–PR10 implementer |
| `PR3-REPORT-DOUBLE-NAME` | V5 | project owner |
| `PR4-SPAWN-SITE-PROBE-CONTEXT` | V4 | PR6/PR7 implementer |
| `PR4-REG-001-STILL-EQUIVALENT` | V4 | PR4–PR10 implementer |
| `PR4-R28-NEXT-COORDINATOR-UNWITNESSED` | V3 | PR5–PR7 implementer (the slice that owns rundir) |
| `PR4-DESIGN-ROLE-SCOPED-ENV` | V5 | project owner |
| `PR4-ADAPTER-RESOLVES-ON-THE-HOST` | V4 | PR6 implementer |
| `PR5-CAPACITY-NOT-A-TOPOLOGY-RESOURCE` | V5 | project owner |
| `PR5-C-FSYNC-UNOBSERVABLE` | V3 | PR7–PR11 implementer (the slice that owns the two-crash proof) |
| `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN *(re-scoped: externally_reachable_fns only)*` | V3 | PR7+ implementer (the slice that owns effects::externally_reachable_fns) |
| `PR5-C-LEGACY-APPEND-ERROR-CENSUS` | V3 | PR7 implementer, or whichever lane plumbs an observer through engine::Harness |
| `PR5-R2-WIN-NON-SURROGATE-REPARSE` | V3 | PR6/PR7 implementer (the slice that next owns Windows containment) |
| `PR5-R2-SNAPSHOT-INPUT-COMMIT-DEAD` | V3 | PR6/PR7 implementer (the slice that first requests two snapshots) |
| `PR5-R2-IDUNREAD-BEFORE-THE-PARSE` | V4 | PR6/PR7 implementer |
| `PR5-R2-WORKTREE-LOCK-RETENTION` | V3 | PR6/PR7 implementer (the slice that can pause a run) |
| `PR5-R2-LEGACY-ENGINE-APPEND-FAILURE` | V3 | PR7 implementer, or whichever lane plumbs an observer through engine::Harness |
| `PR5-R2-OBJECT-GROUP-TAKES-NO-SITE` | V5 | project owner |
| `PR7-WRAPPERS-EMPTY-DOMAIN` | V6 | project owner — the post-v0.2 pass over PR3's layer |
| `PR7-NARROWED-SURFACE-19-UNCALLED` | V3 | PR8/PR12, or whichever slice next opens these modules |
| `PR7-MACOS-PROCESS-GROUP-FLAKE` | V3 | project owner / whichever slice next opens src/runner/host.rs |
| `PR7-WIN-READ-RACING-BOUND-TOO-SHORT` | V3 | PR6's owner, or whichever slice next opens the Container funnel — read_racing arrived in 9 |
| `PR7-SCRATCH-FIXTURE-LEAK` | V3 | project owner / whichever slice owns shared test infrastructure |
| `PR7-P3A-CREATOR-RETAINS` | V4 | PR7/PR12 implementer |
| `PR7-CREATEINTEGRATION-ORDER-BACKWARDS` | V6 | project owner — the post-v0.2 pass over PR3's layer |
| `PR7-FOLD-ACCESSORS-IN-PR3-LAYER` | V1 | project owner — adjudicated 2026-08-24, see §3; the deferred work is the G2 PR3-layer pas |
| `PR7-STEP-D-LINEAGE-ARM-UNWITNESSED` | V3 | PR8 implementer (the slice that gives the merge queue a repair to spawn) |
| `R3-SEAMS-006-ATT003-REPAIRED-POSTHOC` | V2 | project owner, if the residual is worth a row of its own |
| `PR7-R4-CLAIMS-UNVERIFIED` | V2 | project owner — the claims protocol a fresh session carries |
| `PR40-PROGRAM-PUBLIC-ADAPTER-SEAM` | V2 | project owner, carried by G2 W4 |
| `PR40-CHARTER-BINDS-A-PROPOSAL` | V2 | project owner, carried by the documentation-authority pass |
| `PR7-STD-PRIVATE-ROOT-LEXICAL-COMPARE` | V5 | project owner |
| `PR7-STD-OWNER-RECORD-LEXICAL-AUTH` | V5 | project owner |
| `PR7-STD-PRIVATE-ROOT-NO-CONTAINMENT` | V5 | project owner |
| `PR7-STD-QUESTION-PAYLOAD-COMPONENT` | V5 | project owner |
| `PR7-STD-ANSWER-STAGING-COMPONENT` | V5 | project owner |
| `PR7-STD-OWNERSHIP-PROOF-UNCANONICAL` | V5 | project owner |
| `PR7-STD-CONTAINER-LEXICAL-CONFINEMENT` | V5 | project owner |
| `PR7-STD-CONTAINER-EXEC-UNBOUNDED` | V5 | project owner |
| `PR43-MACOS-PROC-SIGNAL-FINGERPRINT` | V3 | project owner / the slice that next opens src/agent/proc.rs, once a controlled macOS environ |
| `PR43-WINDOWS-TOPOLOGY-KILL-FINGERPRINT` | V3 | project owner / the slice that next opens the Windows topology kill harness |

### Recurrence classes — §4 reviewed at the same sitting

The checkpoint record requires §4's recurrence classes to be reviewed for
structural guards at the same sitting as the ledger audit. §4 carries **18
classes**. Each is given a guard verdict below:

- **mechanical** — a named artifact in this tree fails if the class recurs;
- **partial** — a mechanism catches part of the class, and the uncovered part is named;
- **convention** — a written rule with no mechanical enforcement.

| # | Class | Occurrences | Guard | The guard, or what is missing |
|---:|---|---|:---:|---|
| 1 | A surviving mutation named in a round's own prose and carried nowhere durable | 2 | convention | §4's own adopted rule: a round that names a surviving mutation and does not repair it appends it to §2 **in the same commit**, and a deferral must quote the passage that makes it out of scope. Nothing enforces this but a reader |
| 2 | A boundary drawn narrower than the packet's sentence | 2 | convention | This file's "boundary rule" preamble |
| 3 | A fix that introduced a new defect | 5 | partial | Repair rounds now require a red-first witness and a killed mutation per repair, which catches the introduced defect **when the repair is witnessed**. It does not catch a defect introduced outside the witnessed seam — which is how three of the five arrived |
| 4 | Tests satisfied by a correlated field rather than the named one | 11 (PR2) + 11 (PR3/A1) + 2 (PR4) | partial | Withheld mutation catalogues, measured per slice. A catalogue is a measurement taken at a head, not a standing gate; a green suite proves tests pass, not that they still detect |
| 5 | A guarantee proved for the variant that was looked at | 4 | convention | Totality by exhaustive match is the house style; no artifact requires it |
| 6 | The thing that was supposed to prove it never ran | 2 | partial | `test-docs-consistency.sh` C3 pins the set of `.github/scripts/test-*.sh` files **equal** to the set the CI lint job invokes, both directions, which closes the shell-gate half. The `compile_fail` fixture half — a fixture no command executes — has no equivalent |
| 7 | A source census fooled by a comment | 5 | mechanical | `every_production_process_start_is_classified` (`src/runner/mod.rs:1540`) and `every_production_runner_request_is_built_by_its_roles_builder` (`:1463`); and §27's replacement of substring matching with a YAML-1.2 structural oracle, `the_workflow_parser_rejects_duplicate_keys_and_reads_on_as_a_string` (`src/effects/tests.rs:1513`) and `the_workflow_shape_oracle_refuses_every_escape_the_ledger_names` (`:1563`) |
| 8 | An enforcement artifact no gate validates | 2 | mechanical | `every_allowlist_entry_carries_its_justification_and_names_a_real_file` (`src/effects/tests.rs:898`) and `every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist` (`:507`) |
| 9 | An element of a packet-named sequence with no implementation at all | 2 | convention | Sequence coverage is read, not gated |
| 10 | `git checkout <path>` discarding uncommitted work while mutation-testing | 2 | convention | A session hazard. The standing mitigation is to restore from a disposable copy, never the live worktree |
| 11 | An item inserted into a file re-targeting the doc comment above it | 11 at `51cfc01`, derived not maintained | convention | Found by derivation at one head; nothing maintains it |
| 12 | A mutation whose anchor `cargo fmt` had moved, reported as a surviving mutation | 2 | convention | Pre-flight the anchors on a disposable copy before the measuring run, which is fail-fast |
| 13 | An accumulator's witness proves the accumulation and not the read | 4 | convention | The rule is to assert the value the event carries, not the accumulator's reset |
| 14 | A function used as its own expected-value oracle | 5 (PR3/A1) | convention | — |
| 15 | A grid bounded short of its required domain | 8 (PR3/A1) | convention | — |
| 16 | Omitted packet-required fields | 7 (PR3/A1) | convention | — |
| 17 | A refutation that inspected the wrong item of that name | 1 | convention | Below the two-occurrence threshold §4 sets for a signal about the method; carried because it is recorded |
| 18 | A command quoted as evidence becomes part of its own input | 4 | convention | All four introduced by the claims-protocol commits of 2026-08-26 |

**Status, stated plainly: 2 of 18 classes are mechanically guarded, 3 are
partially guarded, and 13 rest on convention.** That is the honest shape of the
recurrence defence at this candidate, and it is the number a panel should weigh
rather than the count of classes alone. The two mechanical guards are both in
the effects/census layer, which is the layer that has had the most recurrence
pressure — the guards followed the failures, which is the right order, but it
means the eleven classes with no mechanical guard are the ones that have not yet
cost enough to earn one.

**No class is closed by this audit.** §4 is a watch list; a class leaves it by
being guarded, not by being reviewed.

### Reconciliation against the P1A ledger input

The P1A read-only pass reported **64** early §2 rows, **2** struck, and **four**
stale W1 rows later fixed. Re-derived at `50ed8c86`:

| Quantity | P1A | This audit | Why they differ |
|---|---:|---:|---|
| §2 rows | 64 | **65** | P1A counted physical table lines. Line 156 carries two logical rows joined by `||`; the second, `PR7-CANDIDATE-TREE-UNVERIFIED`, is invisible to a line-based count and to a rendered read |
| Struck rows | 2 | 2 | agrees |
| Rows whose live disposition is *repaired* | 4 | **8** | P1A named the four W1 rows of §26. Four more carry a terminal disposition elsewhere: `PR5-MACOS-CLIPPY-NEVER-RUN` (§3), `PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE` (own owner cell), `PR7-CANDIDATE-TREE-UNVERIFIED` (in-row) and `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4` (in-row, §22b) |
| Rows closed, not owed | — | 3 | the `PR4-PROGRAM-PATH-NOT-UNICODE` family, one finding recorded three times |
| Rows genuinely carried | — | **52** | 65 − 2 struck − 8 repaired − 3 closed |

P1A's figures were correct for what they measured; the deltas are what a
mechanical re-derivation at the candidate head adds, and both are recorded so
neither has to be taken on trust.

### What this audit does not do

- It does **not** attest. The three-model panel is a separate obligation
  (`decisions/2026-08-25-checkpoint-merges.md`), untouched by this section.
- It does **not** run the suite. No claim here depends on a test result produced
  during assembly.
- It does **not** reopen any disposition, and it edits no historical row. The
  `||` defect on line 156 is recorded, not repaired, for exactly that reason.
- It does **not** claim a reviewer reread the candidate diff.

Companion records: `decisions/2026-08-31-g2-checkpoint-promotion.md`,
`reviews/2026-08-31-g2-gate-report.md`,
`reviews/2026-08-31-g2-first-parent-coverage.md`.

## 36. 2026-08-31 G2 checkpoint — the serialized suite result, and what it does not settle

Append-only. This section records one measurement and changes **no disposition**
in §35 or anywhere above it.

Root ran the globally serialized suite at the exact committed candidate head
`50a84acd3ebf5f0ecffc35a7a5b4ea68960310f9`:

| Fact | Value |
|---|---|
| Command | `upstroke-build cargo test --all-targets --all-features` |
| Exit status | `rc=0`, fresh compile |
| Library | 1801 passed, 0 failed, 34 ignored |
| Binary (`main`) | 8 passed, 0 failed |
| Example | 0 tests |
| Docker | live daemon, Docker server 29.7.2; the `real_docker_*` tests used it and passed |
| Platform | Linux, this host only |

§35 said of itself that it ran no suite and that no claim in it depended on a
test result. That remains true of §35. This section is the separate measurement,
recorded beside it rather than folded into it.

**What it settles about the ledger: very little, and that is the honest reading.**

- **No carried row fired.** 0 failed, so no §35 carried row was observed firing
  on Linux in this run.
- **That is not evidence of absence.** A green suite proves the tests passed, not
  that they still detect — the standing lesson behind the withheld-mutation
  catalogues in §4 class 4. None of the 52 carried rows is closed, narrowed, or
  re-dated by this result, and none of their re-opening triggers fired.
- **The platform-rated rows are untouched.** `PR7-MACOS-PROCESS-GROUP-FLAKE`,
  `PR7-WIN-READ-RACING-BOUND-TOO-SHORT` and `PR43-*` are macOS and Windows rows.
  A Linux run cannot observe them, and a Linux green is exactly the false
  closure `PR5-MACOS-CLIPPY-NEVER-RUN` was reopened for. Their rates stand as
  recorded.
- **The intermittent rows keep their rates.** One green run is one observation
  against rates measured over many; §12's precedent is that a rate not recorded
  when observed is a rate destroyed by the run that clears it. Nothing here
  revises a rate in either direction.

**The recurrence classes are unmoved.** §35's verdict — 2 of 18 mechanically
guarded, 3 partial, 13 convention-only — is a statement about what artifacts
exist, not about whether a run passed. A class leaves §4 by being guarded.

**The `||` defect on line 156 is unrepaired**, deliberately, and this section
does not touch it either.

Companion record: `reviews/2026-08-31-g2-gate-report.md`, "The serialized gate run".

## 37. 2026-08-31 G2 checkpoint — the public schema-4 write path, carried

Append-only. This section adds **one carried row** to the ledger under the
owner's binding amendment 1a of 2026-08-31
(`decisions/2026-08-31-inertness-premise-behavioural.md`). It changes no
disposition in §35 or §36 and repairs nothing.

The row exists because the owner ruled that prose in a decision record is not
enough: **the panel must find this triaged in the ledger rather than discover
it.** It is a *carried* row, not a defect report — the behavioural inertness
condition is satisfied at the candidate head, and this row records the exact
shape of what inertness does **not** cover, so nobody later mistakes the
premise for a stronger one.

### The carried row

| ID | What | Owner | Why it is open |
|---|---|---|---|
| `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED` | **A library consumer can durably write schema-4 state through the checked funnel, using public API only, with no write-side activation check.** The path is three explicit topology choices: construct `RunStarted4 { schema: TOPOLOGY_SCHEMA, … }` — **25 fields, all `pub`, no `#[non_exhaustive]`** (`src/topology/events.rs:600`); check it with `TopologyLine::round_trip` (`src/events/log.rs:1242`); open the funnel with `EventLog::open` (`:466`) and commit it with `append_topology(site_for(&body), …)` (`:796`, `:1064`). `append_topology` delegates straight to `append_topology_hooked` (`:809`) and applies no ceiling test; **`TOPOLOGY_ACTIVATION` and `MAX_READABLE_SCHEMA` appear nowhere in `src/events/log.rs`** — activation gates *reading* only. The resulting log is state the same binary's own resume refuses by name, `SchemaRefusal::TopologyLogUnreadable` (`src/topology/schema.rs:338`, raised at `:241`) | project owner — **the PR12 activation slice** | **Carried, not repaired, and the premise it qualifies is unchanged.** Inertness is *behavioural* and holds: production's only `run_started` mint stamps schema 3 (`src/engine/coordinator.rs:164`), no CLI arm reaches the topology coordinator (`engine::topology` is `pub(crate)`, `src/engine/mod.rs:61`), the read ceiling is 3 by four const assertions evaluated in the ordinary build (`src/topology/schema.rs:98-101`), and `check_upgrade_transition` refuses every path into schema 4. **What this row denies is the stronger guarantee, not the premise**: a released library cannot be prevented from *creating* a schema-4 log, and never could — the legacy funnel already accepts any `pub u32` in `RunStarted.schema` (`src/events/mod.rs:315`), and plain `std::fs` binds no downstream crate at all. Log bytes are untrusted input and the code has always treated them so. **Repairing this is out of scope for the promotion by owner ruling**: narrowing `src/topology/` to `pub(crate)` would break `EventSite` in public log signatures and the frozen `compile_fail` doctests pinned to their failure reasons, report the whole topology tree dead under `-D warnings`, and produce a new candidate head that re-runs the suite, the eight gate artifacts and the 66-unit coverage map — to buy a guarantee `std::fs` refutes. A write-side inactivity guard in `append_topology` would strengthen a guarantee beyond PR7's frozen packet, which is managed debt and not an in-slice repair |

### Venue, trigger and required evidence

Recorded in §35's carried-row form so the row is auditable the same way the
other 52 are.

| Field | Value |
|---|---|
| **Class** | V4 — a numbered future slice implementer |
| **Venue / owner** | project owner — the **PR12 activation slice** |
| **`shrinks_when`** | the activation slice lands, **or** a visibility narrowing is scheduled |
| **Re-opening trigger** | PR12 opens, or any slice schedules a narrowing of `src/topology/**` or the event funnel's public surface |
| **Required evidence for the repair** | a write-side refusal with a red-first witness **and** a killed mutation; **plus** an accounting of the legacy funnel's unvalidated `RunStarted.schema` field, because a guard on the schema-4 path alone leaves the schema-3 path accepting any `u32` and the guarantee would still not hold |

### What this row does not do

- It does **not** reopen the inertness condition of
  `decisions/2026-08-25-checkpoint-merges.md`. That condition is behavioural and
  is satisfied at `50ed8c86`.
- It does **not** authorize a visibility change. The owner ruled explicitly that
  no visibility change to the code is authorized in this promotion.
- It does **not** change the §35 totals as they were audited. §35's 52 carried
  rows are the normalization of §2 at the time of the audit; this is a
  **53rd carried row**, opened after it by owner amendment, and is counted
  separately rather than folded back into a table it was not part of.

Companion records: `decisions/2026-08-31-inertness-premise-behavioural.md`,
`reviews/2026-08-31-g2-gate-report.md` ("Inert by default" §4).

## 38. 2026-08-31 PR #80 exact-head review — the full-ledger projection, completed

Append-only. This section repairs the completeness defect the sole review of
`e174d086efc71b8c837ed22e61f29f706ef9dacd` found in §35, and records that
review's four dispositions. It rewrites no historical row.

**§35's audit was not full, and the review was right.** §35 projected §2's 65
logical rows and stopped there, while live deferred rows sat in other sections it
never counted — including **all four `PR73-*` rows** (§29, lines 3539–3542) and
**`PR64-CLEANUP-003-SCRATCH-PRECLEAN`** (§33, line 3617). §35's claim that
"every live row" was categorized was therefore false, and the checkpoint
record's obligation 2 was **overstated as discharged**. It is discharged here.

The omission had teeth: `PR64-CLEANUP-003-SCRATCH-PRECLEAN` is the live sequence
in which the predictable scratch helper meets another process's occupied root,
recursively pre-cleans it before acquiring ownership, and deletes that process's
content. An audit that does not count it is an audit that would let it through.

### The canonical domain, stated before the counts

**A canonical row is a markdown table row in this file whose first cell is a
stable finding ID.** That domain is mechanically enumerable and is what the
projection covers. Derived at this head:

- **284** canonical row instances, over **197 distinct stable IDs**.
- Physical lines carrying two logical rows are split on `||` (line 156), and
  cells naming several IDs with `·` are expanded to one row each.
- **Latest-disposition-wins**: for each ID, the instance at the greatest line
  number is the live one. §35's own restatement table is excluded as a
  restatement, not a source.

**Completeness is asserted over this domain and no wider one.** Owner clauses
that live in prose rather than a table row are *not* canonical rows; four exist
and are named at the end of this section so they are not invisible.

### The projection over all 197 canonical IDs

| Terminal disposition | IDs |
|---|---:|
| **repaired** | 94 |
| **carried** | 75 |
| **settled** (§1, not re-raisable without new evidence) | 17 |
| **closed, not owed** | 9 |
| **struck** (withdrawn in place) | 2 |
| **total** | **197** |

94 + 75 + 17 + 9 + 2 = 197. **Every canonical ID has exactly one terminal
disposition**, and the sum is the enumeration, not an estimate.

Six IDs whose disposition cell carried no mechanical keyword were ruled by hand
against their own prose and are listed for audit:
`PR5D-PROOF-TESTS-COUNT` → closed ("recorded, no owner needed");
`PR7-WIN-READ-RACING-BOUND-TOO-SHORT-TERMINOLOGY` → closed (a terminology
correction, disposition unchanged); and `PR7-R3-ATTEMPT-002-REVIEWERS-TAKE-NO-SLOT`,
`PR7-R3-ATTEMPT-004-NO-TRANSCRIPT-NO-GATE-LOG`,
`PR7-R3-SETTLE-LADDER-POSITION-RUNG-HALF`,
`PR7-R3-CONTRACT-004-UNRESOLVED-INDEX-REFUSAL-UNREACHABLE` → carried.

### The 75 carried rows, by origin

| Originating section | Carried |
|---|---:|
| §2 | 49 |
| §15 — the six that need adjudication | 6 |
| §20 — PR7 S5 rounds 3 and 4 | 5 |
| "The hardening rule" | 4 |
| §29 — PR #73 owner adjudication | 4 |
| §8 — PR5 lane D | 2 |
| §24, §25, §27, §33, §37 — one each | 5 |
| **total** | **75** |

**Why §2 shows 49 here and 52 in §35.** §35 counted §2 rows; this projection
counts *IDs*, and five §2 IDs have a later instance elsewhere that wins:
`TASK-DISPATCHED-REGION-UNVALIDATED`, `PR7-G2-W1-SUCCESS-IGNORES-THE-FROZEN-PLAN`,
`PR7-G2-W1-RETAINED-ARM-UNGUARDED` and `PR7-G2-W1-PROBE-PAIR-NOT-OBLIGED` resolve
to §26 (repaired), and `PR4-ADAPTER-RESOLVES-ON-THE-HOST` resolves to the
hardening rule (carried, counted there). §35's 52 minus the three §2-carried IDs
that move out is 49. The two figures are consistent; they count different things,
and this one is the full-ledger figure.

### The 26 carried rows §35 missed, each with venue, trigger and evidence

| ID | Section | Line |
|---|---|---|
| `PR4-INVOCATION-CONSTRUCTIBLE` | §The hardening rule | L842 |
| `PR4-CENSUS-COMMENT-ORACLE` | §The hardening rule | L843 |
| `PR4-ADAPTER-RESOLVES-ON-THE-HOST` | §The hardening rule | L844 |
| `PR4A-SPAWN-WITHOUT-AMBIENT` | §The hardening rule | L845 |
| `PR5D-ROW-MAPPING-REFUSAL-UNFIXTURED` | §8 | L927 |
| `PR5D-TOOLBOX-DISCARDS-CLIPPY-OUTPUT` | §8 | L929 |
| `PR5-RUNDIR-030` | §15 | L1417 |
| `PR5-EVENTS-020` | §15 | L1418 |
| `PR5-WORKSPACE-068` | §15 | L1419 |
| `PR5-WORKSPACE-070` | §15 | L1420 |
| `PR5-EVENTS-051` | §15 | L1421 |
| `PR5-WORKSPACE-003` | §15 | L1422 |
| `PR7-R4-LOOP-004` | §20 | L2175 |
| `PR7-R3-ATTEMPT-002-REVIEWERS-TAKE-NO-SLOT` | §20 | L2207 |
| `PR7-R3-ATTEMPT-004-NO-TRANSCRIPT-NO-GATE-LOG` | §20 | L2209 |
| `PR7-R3-SETTLE-LADDER-POSITION-RUNG-HALF` | §20 | L2211 |
| `PR7-R3-CONTRACT-004-UNRESOLVED-INDEX-REFUSAL-UNREACHABLE` | §20 | L2212 |
| `PR7-R3-ATTEMPT-003-RESIDUE-DISCARD-UNREACHED` | §24 | L3430 |
| `PR47-PUBLIC-PROCESS-API-REMOVED` | §25 | L3459 |
| `CI-CFG-UNSHIPPED-UNIX-REGION` | §27 | L3494 |
| `PR73-TARGET-INVENTORY-001` | §29 | L3539 |
| `PR73-LEXICAL-CLOSURE-001` | §29 | L3540 |
| `PR73-LEXER-DIVERGENCE-001` | §29 | L3541 |
| `PR73-LINT-SEMANTICS-001` | §29 | L3542 |
| `PR64-CLEANUP-003-SCRATCH-PRECLEAN` | §33 | L3617 |
| `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED` | §37 | L3955 |

Venue, trigger and required evidence for each, in §35's carried-row form:

| Group | Rows | Venue / owner | `shrinks_when` | Re-opening trigger | Required evidence |
|---|---|---|---|---|---|
| **Hardening** | `PR4-INVOCATION-CONSTRUCTIBLE`, `PR4-CENSUS-COMMENT-ORACLE`, `PR4-ADAPTER-RESOLVES-ON-THE-HOST`, `PR4A-SPAWN-WITHOUT-AMBIENT` | the named implementer slice (PR5–PR7, PR7, PR7/PR12) | that slice lands the hardening | the named slice opens | a witness that fails without the hardening, plus a killed mutation. These strengthen a guarantee beyond the frozen packet, so they are **managed debt, not in-slice repairs** |
| **PR5 lane D** | `PR5D-ROW-MAPPING-REFUSAL-UNFIXTURED`, `PR5D-TOOLBOX-DISCARDS-CLIPPY-OUTPUT` | PR6/PR7 implementer; project owner (box tooling) | the fixture becomes constructible; the toolbox stops discarding Clippy output | a slice opens the row-mapping fixture, or the build wrapper is changed | a red-first fixture for the refusal; for the toolbox, a wrapper that surfaces Clippy output — a box-side fix, not a tree fix |
| **§15 adjudication** | `PR5-RUNDIR-030`, `PR5-EVENTS-020`, `PR5-WORKSPACE-068`, `PR5-WORKSPACE-070`, `PR5-EVENTS-051`, `PR5-WORKSPACE-003` | **project owner — G2** | each is adjudicated **narrowed assertion** or **equivalent mutant** | the G2 adjudication sitting | per entry, the decision between a real detection loss and a re-expressed-prose equivalent. `PR5-EVENTS-051` **SURVIVED** and is the one repair of 38 that did not take; `PR5-WORKSPACE-003` is **Windows-only** — Linux kills it, so it needs the guest |
| **§20 PR7 rounds** | `PR7-R4-LOOP-004`, `PR7-R3-ATTEMPT-002-REVIEWERS-TAKE-NO-SLOT`, `PR7-R3-ATTEMPT-004-NO-TRANSCRIPT-NO-GATE-LOG`, `PR7-R3-SETTLE-LADDER-POSITION-RUNG-HALF`, `PR7-R3-CONTRACT-004-UNRESOLVED-INDEX-REFUSAL-UNREACHABLE` | PR8/PR10 (closure); PR8+; project owner for the G2 erratum list | closure is implemented; the merge queue spawns a repair; the erratum is written | PR8 or PR10 opens, or the owner takes up the erratum list | a red-first witness on the arm plus a killed mutation. `PR7-R4-LOOP-004` additionally owes the **diagnostic**: an operator told "closure derives NotEnding" about a budget-stopped run is being told the wrong thing |
| **Blocked** | `PR7-R3-ATTEMPT-003-RESIDUE-DISCARD-UNREACHED` | project owner | the packet reclassifies the pre-intent ephemeral commit | an owner packet decision | **Blocked, not deferrable by an implementer**: the packet and tests classify the commit as Git-owned R27 and require recovery to leave it. Deleting it contradicts an explicit contract |
| **Accepted residual** | `PR47-PUBLIC-PROCESS-API-REMOVED` | project owner — a later compatibility-owned slice | that slice takes the compatibility question | a compatibility slice opens | the owner explicitly accepts the current wrapper behaviour for PR #47; **preserve as residue, not a gate blocker** |
| **Platform scope** | `CI-CFG-UNSHIPPED-UNIX-REGION` | project owner — a platform-scope decision | a fourth Unix family is added to CI, or the region is removed | adding a BSD runner | a platform-scope decision, **not an in-slice oracle repair**. The census already requires the exact acknowledged set and fails if it changes |
| **PR #73 deferred** | `PR73-TARGET-INVENTORY-001`, `PR73-LEXICAL-CLOSURE-001`, `PR73-LEXER-DIVERGENCE-001`, `PR73-LINT-SEMANTICS-001` | project owner — owner-adjudicated deferrals of 2026-08-30 | each named guard is widened: the walk covers workspace members; aliases under an expectation are forbidden; a blanked-view ASCII census lands; `cfg_attr` handling is added | a workspace member or path dependency is added; a third lexer-class recurrence; an inventory-pin edit | the recorded restriction becomes the trigger. All four are `pre_existing` and identical at comparison head `0f05b456`; each keeps its named backstop until repaired |
| **Scratch pre-clean** | `PR64-CLEANUP-003-SCRATCH-PRECLEAN` | project owner — the bound **startup, recover and create migration** follow-up | that collision set receives an exact-base lease | the migration slice opens | occupied-root **preservation**, and **no pre-clean and no discarded cleanup result**. §32 marked this fixed by reusing the ID for a distinct emit helper; §33 corrected that to deferred, and it stays deferred here |
| **Schema-4 write path** | `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED` | project owner — the PR12 activation slice | the activation slice lands, or a visibility narrowing is scheduled | PR12 opens, or a narrowing is scheduled | a write-side refusal with a red-first witness and a killed mutation, **plus** an accounting of the legacy funnel's unvalidated `RunStarted.schema` |

### The four prose owner clauses, named so they are not invisible

Outside the canonical row domain, four paragraphs carry an owner clause with no
table row. They are **not** counted in the 197 and are listed so a later audit
does not rediscover them as omissions: §3's dependency clause (line 237), §12's
pre-existing-flake clause (line 1118), §18's two clauses (lines 1784, 1815), and
§20's `effects::census_domain` clause (line 2241). Giving each a canonical row is
work for the next ledger pass, not for this repair.

### The PR #80 review's own four dispositions

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR80-LEDGER-AUDIT-NOT-FULL | P1 | e174d086efc71b8c837ed22e61f29f706ef9dacd / reviews/FINDINGS.md:3633-3679 | §35 projects only §2's rows -> live deferred rows in other sections are never counted -> the audit claims every live row is categorized -> the checkpoint's obligation 2 is reported discharged while `PR64-CLEANUP-003-SCRATCH-PRECLEAN` and four `PR73-*` rows are untriaged -> the predictable scratch helper pre-cleans another process's occupied root and deletes its content | introduced_by_feature | correctness | 50a84acd3ebf5f0ecffc35a7a5b4ea68960310f9 / §35 | This section projects all 197 canonical IDs across the whole ledger — 94 repaired, 75 carried, 17 settled, 9 closed, 2 struck — with the domain stated before the counts and completeness asserted only over it. The 26 carried rows §35 missed each receive venue, trigger and required evidence | fixed |
| PR80-CHECKPOINT-ORDER-REVERSED | P1 | e174d086efc71b8c837ed22e61f29f706ef9dacd / decisions/2026-08-31-g2-checkpoint-promotion.md:11-23 | the record declares the candidate cut at `50ed8c86` -> the same record admits the gate has not passed and artifacts are missing -> the controlling order (gate and artifacts first, then the cut) is reversed -> an immutable decision is narrowed with no successor ruling; and the audit postdates `50ed8c86`, so that commit cannot carry it | introduced_by_feature | docs-contract | 50a84acd3ebf5f0ecffc35a7a5b4ea68960310f9 | `50ed8c86` is restated as the **pre-assembly baseline**, not a cut candidate; the candidate becomes the integration landing head after this evidence lands and the outstanding artifacts and gates complete. `2026-08-25-checkpoint-merges.md` remains controlling and unnarrowed; the third addendum and the gate report's "What `50ed8c86` is" carry the correction, and the owed artifacts are restated | fixed |
| PR80-COVERAGE-EXEMPTION-INVENTED | P1 | e174d086efc71b8c837ed22e61f29f706ef9dacd / reviews/2026-08-31-g2-first-parent-coverage.md:30 | the map treats `decisions/`, `proposals/`, `docs/`, root Markdown and ignore files as review-exempt -> `2026-08-20-review-invalidation-scope.md` authorises exactly `reviews/FINDINGS.md` -> units with no recorded review are counted as covered -> `59a6830`, which changed a decision plus `reviews/README.md`, is reported mapped | introduced_by_feature | docs-contract | 50a84acd3ebf5f0ecffc35a7a5b4ea68960310f9 | The exemption class is withdrawn. Class **X** now requires the unit's whole diff to be exactly `reviews/FINDINGS.md`, verified per unit (11 units, 12 commits). Class **M** requires every delta file to be byte-identical to the merged-in parent **and** that parent to be an ancestor of `origin/master` (2 units). Residue rises from 7 units to **18 units / 19 commits**, including `59a6830`, three master-forward merges carrying merge-authored conflict resolution, and the 14 pre-PR-regime commits. Totals re-derived: 33 S + 2 B + 2 M + 11 X + 18 R = 66 units, 418 commits | fixed |
| PR80-PANEL-TEXT-STALE | P2 | e174d086efc71b8c837ed22e61f29f706ef9dacd / reviews/2026-08-31-g2-gate-report.md:59-62 | the gate report says panel membership is unsettled -> the same head adopts `2026-08-31-panel-seats.md` -> a current gate artifact asserts two contradictory states -> a reader cannot tell which is live | introduced_by_feature | docs-contract | e174d086efc71b8c837ed22e61f29f706ef9dacd | The gate report now states that membership is **settled and ratified** and that **no seat has run**, naming the three seats with their invocation guards. The promotion record's second addendum carries an in-place supersession marker pointing at its third addendum. No other sentence claims membership is unsettled | fixed |

Companion records: `reviews/2026-08-31-g2-first-parent-coverage.md`,
`reviews/2026-08-31-g2-gate-report.md`,
`decisions/2026-08-31-g2-checkpoint-promotion.md` (third addendum).

## 39. 2026-08-31 PR #80 second exact-head review — the projection made reproducible, and the labels closed

Append-only. This section repairs the three P1 findings the sole review of
`ada79bd76c791a6faac18f850929fbbd8cd7b237` returned, corrects §38's
arithmetic by re-deriving it, and records the review's dispositions. It
rewrites no historical row and reopens no disposition.

### Reading rule for the labels in §§35–37

§35 says "measured at candidate head" (line 3637; likewise its prose at
lines 3847, 3871 and 3882), §36 "at the exact committed candidate head"
(line 3893), §37 "satisfied at the candidate head" (line 3947). Per the
promotion record's third addendum: `50ed8c86` is the **pre-assembly
baseline**, `50a84acd…` the **committed evidence head**, and no candidate
has been cut. Those sections stand as written under this file's append-only
rule; this paragraph is the reading rule a later auditor applies. ("Candidate"
in the product sense — the merge queue's prepared candidates,
`PR7-CANDIDATE-TREE-UNVERIFIED` — is a different word and is untouched.)

### The canonical-row domain, restated so a script can hold it

§38's domain rule was not reproducible as stated: applied literally, its
`||` split fires inside code spans (lines 102, 105 and 3864 carry a literal
`||` in backticks) and its "first cell is a stable ID" admits prose cells
that merely mention an id. The rules that close both holes:

- **R1.** A physical table line is any line whose stripped form starts with
  `|` and is not a separator row.
- **R2.** A line splits into two logical rows at a `||` occurring **outside
  backtick spans**, and only if **both** halves independently satisfy R3.
  Exactly one line in this file splits: line 156.
- **R3.** A logical row is **canonical** iff its first cell — after
  stripping strikethrough, emphasis, backticks, and a trailing parenthetical
  annotation such as `(a)` or `*(re-scoped: …)*` — consists **entirely** of
  one finding id or a `·`-separated list of finding ids, an id matching
  `^[A-Z][A-Z0-9]*(-[A-Z0-9]+)+$`.
- **R4.** A `·` list expands to one instance per id at the same line.
- **R5.** Restatement tables — audit views that re-list rows owned elsewhere
  — are excluded as sources. At this head they are exactly: §35's four view
  tables (lines 3689–3697, 3704–3709, 3727–3731, 3759–3812) and §38's
  missed-rows listing (lines 4067–4094). §38's four-disposition table is a
  **source** (the only instances of its `PR80-*` ids), as is the disposition
  table at the end of this section. This section adds no other id-first-cell
  rows, so it moves the projection by exactly its three new rows.
- **R6.** For each id, the source instance with the greatest line number is
  the live one (latest-disposition-wins).
- **R7.** The winner classifies by section rule, then keyword, then an
  explicit hand-ruled list — every hand ruling named below with its basis;
  nothing is ruled silently.

Section rules: §1 → settled; §5, §6, §7, §9, §10, §26, §31, §34 → repaired;
"The hardening rule", §8, §15 → carried; §2 → repaired on an in-row
`FIXED`/"fixed in PR7" marker, closed on "Closed, not repaired"/"Closed by
this row", struck on `~~`, else carried. Elsewhere, keywords in the winning
row: a leading bold `Carried` → carried; `Repaired`/`fixed in this slice`/
`fixed by owner…`/`implementation-fixed` → repaired; `CLOSED`/`Closed by` →
closed; a terminal cell `deferred` → carried; a terminal cell `fixed` →
repaired; `accepted residual`, `blocked by packet`, or a bold `deferred`
disposition cell → carried.

Hand-ruled (complete list): `PR5-MACOS-CLIPPY-NEVER-RUN` → repaired (§3's
dated 2026-08-28 challenge outcome; `lint (macos)` live in `ci.yml`);
`PR4-PROGRAM-PATH-NOT-UNICODE` → closed (superseded by its `-CLOSED` and
`-CLOSED-NARROWED` successor rows and
`2026-08-25-commandspec-program-stays-string.md`);
`PR5D-MSVC-CLIPPY-NEVER-RUN` → repaired (`lint (windows)` runs clippy
natively on `windows-latest`; the deferred cross-target gate was superseded
by the native job, and `clippy::items_after_test_module` is now a governed
lint); `PR5-RD-001` → repaired (its row records the repair and witnesses);
`PR5D-PROOF-TESTS-COUNT` → closed (recorded, no owner needed);
`PR7-WIN-READ-RACING-BOUND-TOO-SHORT-TERMINOLOGY` → closed (a terminology
correction; the disposition lives with the base id); and the four §20
round-3 rows `PR7-R3-ATTEMPT-002-REVIEWERS-TAKE-NO-SLOT`,
`PR7-R3-ATTEMPT-004-NO-TRANSCRIPT-NO-GATE-LOG`,
`PR7-R3-SETTLE-LADDER-POSITION-RUNG-HALF`,
`PR7-R3-CONTRACT-004-UNRESOLVED-INDEX-REFUSAL-UNREACHABLE` → carried (each
names its owner in-row; no terminal keyword).

### The corrected counts, and exactly what moved

| Head | Distinct ids | repaired | carried | settled | closed | struck |
|---|---:|---:|---:|---:|---:|---:|
| `e174d086` (what §38 measured) | 197 | 94 | **77** | 17 | **7** | 2 |
| `ada79bd7` (§38's four rows added, repaired) | 201 | 98 | 77 | 17 | 7 | 2 |
| this head (this section's three rows added, repaired) | **204** | **101** | **77** | **17** | **7** | **2** |

Confirmed by this re-derivation: §38's total of 197; repaired 94; settled
17; struck 2; and its 26-missed-rows table — the non-§2 carried set is
**exactly** those 26 ids. Corrected: **carried is 77, not 75, and closed is
7, not 9.** The closed set, exhaustively: the three
`PR4-PROGRAM-PATH-NOT-UNICODE*` rows (one finding recorded three times),
`PR5D-PROOF-TESTS-COUNT`, `PR7-R3-EMIT-006-DEFER-ROUND-IS-A-BACKOFF-ROUND`,
`PR7-R3-SETTLE-CAND-OBJ-REFUSAL-UNREACHABLE`, and
`PR7-WIN-READ-RACING-BOUND-TOO-SHORT-TERMINOLOGY`. And §38's "§35's 52
minus the three §2-carried IDs that move out is 49" is corrected to **"§35's
52 minus one is 51"**: of the five ids §38 named, the four W1 ids sat in
§35's *repaired* bucket, never its 52 carried, so they subtract nothing;
only `PR4-ADAPTER-RESOLVES-ON-THE-HOST` changes origin (to the hardening
rule). §2-origin carried is **51**, and 51 + 26 = 77. The two ids §38's
split displaced into closed belong in carried; its grand total was right and
its buckets were not, which is precisely why a published count must carry
its derivation.

### The prose owner clauses: five, by detector

Detector: every non-table, non-heading line matching `Owner…:` (bold or
plain, including "Owner ruling, DATE:"), then excluding clauses whose
subject has a canonical row. Nine hits at this head; excluded: line 1413 (a
heading), line 1366 and line 2187 (they annotate the §15 six and
`PR7-R4-LOOP-004`, which have rows), and line 1182 (the 2026-08-27
restatement of `PR5-RD-002`'s trigger; that id has rows and its live
instance is §24, repaired). The row-less clauses are therefore **five**, not
§38's "four": §3's dependency clause (line 237), §12's pre-existing-flake
clause (line 1118), §18's two clauses (lines 1784 and 1815), and §20's
`effects::census_domain` clause (line 2241). Giving each a canonical row
remains work for the next ledger pass, not this repair.

### The second review's three dispositions

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR80-CANDIDATE-LABEL-RECURRENCE | P1 | ada79bd76c791a6faac18f850929fbbd8cd7b237 / reviews/2026-08-31-g2-gate-report.md:76 | the head declares no candidate exists -> sibling passages still label `50ed8c86`/`50a84acd` the candidate head -> a reader convenes the panel or reuses evidence against the baseline -> the checkpoint order is re-reversed in effect | fix_regression | docs-contract | PR80-CHECKPOINT-ORDER-REVERSED | Gate-report fifth revision's terminology pass; appended errata in both decision records and the `decisions/README.md` index fix; the acceptance grep over the seven paths whose every remaining "candidate" hit is a negation, the future sense, a quotation, the branch name, or the product sense | fixed |
| PR80-OWED-LIST-DROPS-ARTIFACT-7 | P1 | ada79bd76c791a6faac18f850929fbbd8cd7b237 / decisions/2026-08-31-g2-checkpoint-promotion.md:195 | the third addendum lists artifacts 2, 3, 4, 5 and 8 as the owed captured set -> the gate report counts six uncaptured including artifact 7's scan output -> an operator completes the shorter list and cuts a candidate without artifact 7 -> the eight-artifact precondition is violated | fix_regression | docs-contract | PR80-CHECKPOINT-ORDER-REVERSED | The fourth addendum corrects the list to 2, 3, 4, 5, 7 and 8 and names the gate report's artifact table the single enumerator, authoritative over any restatement | fixed |
| PR80-LEDGER-PROJECTION-UNPROVEN | P1 | ada79bd76c791a6faac18f850929fbbd8cd7b237 / reviews/FINDINGS.md:4056 | §38 claims 52 minus three = 49 from a named set in which only one id moves origin -> its carried/closed split contradicts its own 26-row enumeration -> it states four prose clauses and enumerates five -> a gate relying on §38 declares the ledger discharged without a reproducible accounting | fix_regression | docs-contract | PR80-LEDGER-AUDIT-NOT-FULL | This section's domain rules R1–R7, complete hand-ruled list, and corrected counts (197 = 94+77+17+7+2 at `e174d086`; 204 = 101+77+17+7+2 here), re-derivable by any implementation of the stated rules | fixed |

Companion records: `decisions/2026-08-31-g2-checkpoint-promotion.md` (fourth
addendum), `reviews/2026-08-31-g2-gate-report.md` (fifth revision),
`decisions/2026-08-31-inertness-premise-behavioural.md` (erratum).

## 40. PR80 exact-head macOS sampler recurrence (2026-08-31)

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR80-MACOS-WORKSPACE-SAMPLER-COLD-PROBE-RECURRENCE | P2 | 2ba66b6e06fa40f9d9fe06dfd21e22517e14d2d6 / hosted run 33421539013, macOS job 99584874271, `workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered` | the workspace-manager sampler measures a one-shot probe budget -> every scheduled kill lands after its child has completed -> all 32 observations are `Completed` and no killed-child residue is sampled -> the required macOS leg refuses the vacuous run | fix_regression | platform-correctness | PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE | The refusal is correct and the failed run remains durable evidence. The candidate's `src/` tree is byte-identical to the green `50ed8c86ec60164011bfd393066c4c3696d3865b` source tree (`f8d2b1c6dff093bd1b656d639fa33762e479b7f9`), so this evidence-only slice did not change the failed code. Owner: project owner; venue: post-promotion sampler-hardening work; shrinks when the workspace-manager sampler uses the established warm-up, median, actual-duration recalibration, and bounded-retry discipline and a controlled macOS repetition demonstrates that at least one kill lands without masking the vacuity oracle | deferred |

## 41. PR80 artifact-enumerator reading rule (2026-08-31)

The explicit artifact membership in the historical
`PR80-OWED-LIST-DROPS-ARTIFACT-7` row in §39 records the defect and its then
current repair; it is not an operative enumerator. The artifact table in
`reviews/2026-08-31-g2-gate-report.md` is the sole operative enumerator of
artifact membership and capture state. This section introduces no new
artifact-membership list.

## 42. PR #18 G2 artifact-capture sequencing breach (2026-08-31)

Append-only. The owner adjudicated panel round one at
`47dc9a35f6e6af59160ece49570d9934a4450dec` and required this sequencing breach
to remain visible even after its missing evidence was captured.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR18-G2-ARTIFACT-CAPTURE-AFTER-CANDIDATE-CUT | P1 | 47dc9a35f6e6af59160ece49570d9934a4450dec / `reviews/2026-08-31-g2-gate-report.md`, authoritative artifact table | the dated checkpoint decision requires all eight captured artifacts before a candidate is cut -> candidate assembly and PR #80 landing advance PR #18 to `47dc9a35` while artifacts 2, 3, 4, 5, 7 and 8 remain only oracle-passed or owed -> panel round one finds the missing captured forms as a High blocker -> the promotion cannot attest the checkpoint in the required order | fix_regression | evidence-integrity | PR80-CHECKPOINT-ORDER-REVERSED | The breach is not waived: this capture commit records the instrumented serialized run, produces and hash-pins all six missing forms, keeps the original round-one verdicts, and requires a fresh blind three-seat panel over the advanced head. Any missing capture, hash mismatch, later head movement, or non-conforming seat reopens the row. | fixed in this capture commit |

## 43. 2026-09-03 W1/W2 decomposition — nineteen findings out of review: eighteen carried, one fixed

Append-only. This section adds **eighteen rows to §2 and one row to §5 (Fixed)**, and changes no
disposition above it. It repairs nothing.

The rows exist because of the project owner's direction of 2026-09-02: **a review is scoped to the
change under review.** An observation the reviewer judges pre-existing — neither introduced nor
activated by the diff in front of it — is recorded and not fixed in that pull request. Nothing is
discarded; each becomes its own change, reviewed on its own terms. Fifteen of the eighteen reached
that disposition through a review pass or a steward's verification; one
(`W1-FIXTURES-NOT-RETIRED-W0-AUTH-PART-E-UNFULFILLED`) is an owner ruling recorded so an unfulfilled
packet clause is not mistaken for a fulfilled one, and two
(`CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`,
`W2-RETIRED-DECISIONS-PATHS-CITED-AND-MISSING`) are cross-cutting observations no single review was
positioned to make.

They land in **one append** rather than per-packet edits because this file's IDs are 1:1 with
findings and must stay mechanically re-derivable. Nineteen separate edits to §2, each arriving with
its own head, is how that property is lost.

Seven of the nineteen are the same two families, and the families are the finding:

- **Derive a domain from declarations, not from names on disk.** A census whose domain is a
  directory walk, an exact path, a file name, or a substring cannot see a `#[path]` relocation, a
  child directory, a `cfg_attr` that applies a `cfg`, or a longer variant that swallows a shorter
  one. Five rows: `PR101-CFG-ATTR-APPLIED-CFG-INVISIBLE-TO-THE-SCAN`,
  `W1-CLASSIFIED-MODULES-IS-A-HAND-MAINTAINED-ROLL-CALL`,
  `PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY`,
  `PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK`,
  `PR110-SITE-CENSUS-MATCHES-EFFECT-SITE-NAMES-BY-SUBSTRING`. The repository already holds the
  pattern that resolves it correctly, and each row names it. This is the same shape as
  `CLASS-GATE-STATED-DOMAIN-EXCEEDS-COUNTED-DOMAIN`, already in §2, seen from the domain's side
  rather than the claim's.
- **A test scratch directory whose name is reproducible across runs is not hermetic.** Two rows:
  `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED` and
  `PR104-PRELOCK-SCRATCH-NAME-REPRODUCIBLE-ACROSS-RUNS`. The first is measured, not argued.

### The evidence standard this section holds itself to

This is a findings ledger, so the bar is its own subject.

- **Every count below was derived twice, by two engines that must agree**, and stated per file
  rather than as a total. `/usr/bin/grep` on the build box is ugrep 7.8.4, whose `-E` engine
  silently under-reports — rc=0, no warning, and a plausible **low** number — so a count from it
  alone is not evidence. Where the claim is structural rather than textual, a parser was used and
  a second method corroborated it; a parser cannot under-report the way an engine can.
- **A count in prose goes stale on the next commit.** Every count here is therefore bound to the
  commit it was taken at and accompanied by the command that reproduces it, so a reader re-derives
  rather than trusts. Where a property survives every future commit and a count does not, the
  property is what the row states.
- **Where a cause is unknown, the row says so and names the measurement that would settle it.**
  The four unexplained CI observations follow `PR43-MACOS-PROC-SIGNAL-FINGERPRINT`'s wording —
  *"open as an unexplained observation, not classified as a flake or regression"* — because a
  mechanism nobody measured is a guess wearing a finding's clothes. Two rows in this append exist
  precisely because a mechanism *was* guessed: one was refuted by measurement and moved to §5, and
  one was withdrawn outright.

### The ID derivation, stated so a reader can re-run it

IDs in this file are 1:1 with findings and must stay mechanically re-derivable. The rule used here,
stated rather than remembered:

> `ID = <ORIGIN>-<SLUG>`
>
> * **`ORIGIN`** is `PR<n>` when the finding is bound to **exactly one** pull request — a review
>   pass on #n, or an observation seen only on #n.
> * **`ORIGIN`** is `W1` or `W2` when it is bound to **none** — a coordinator or steward finding, or
>   a cross-branch observation seen on several pull requests. The wave is the one the finding was
>   made in.
> * **`ORIGIN`** is `CLASS` when the row is a class **over other rows** in this file rather than a
>   finding at a site. Derived from the file: `CLASS-GATE-STATED-DOMAIN-EXCEEDS-COUNTED-DOMAIN` is
>   the only prior instance and it is the only `CLASS-` ID.
> * **`SLUG`** is the finding's title line, uppercased, every run of non-alphanumeric characters
>   collapsed to a single `-`, leading and trailing `-` dropped.
> * **An unexplained CI observation** additionally takes the file's existing fingerprint shape:
>   `<ORIGIN>-<PLATFORM>-<SUBSYSTEM>-<WHAT>-FINGERPRINT`.

**The fingerprint clause is read off the file, not invented.** Three rows carried it before this
append — `PR43-MACOS-PROC-SIGNAL-FINGERPRINT`, `PR43-WINDOWS-TOPOLOGY-KILL-FINGERPRINT` and
`PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT` — and two consequences follow, both applied:

* **The platform token is the platform, not the job.** `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT`
  fails in the job `test (winguest)` and its ID says `WINDOWS`; the job name lives in the row's
  `What` cell. This append's Windows row is therefore `W2-WINDOWS-RACING-REMOVAL-DELETE-PENDING`
  and not `…-WINGUEST-…`.
* **A third platform needs a third token, and the file had none.** `MACOS` and `WINDOWS` were the
  only two in use. `PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT` fails in
  `test (ubuntu-latest)`; the token is **`LINUX`**, the platform, on the same rule that makes
  `winguest` render as `WINDOWS`. It is stated here so the next Linux fingerprint is named the same
  way rather than after whatever the runner image is called that quarter.

**The derivation was executed against the file being appended to**, not against the state it was
drafted from. `reviews/FINDINGS.md` moved four times on 2026-09-03 — `1f30851` (C-018's row),
`079a346` (#104's merge), `2de71dd` (#110's merge) and `6724fb9` (#106's merge, which added
`CLASS-GATE-STATED-DOMAIN-EXCEEDS-COUNTED-DOMAIN`). At `ae2a58f` the file holds **183 distinct
IDs**, extracted by taking the first cell of every table row that is wholly upper-case
alphanumerics-and-hyphens and at least five characters:

    python3 - <<'PY'
    import re
    ids={c for l in open('reviews/FINDINGS.md',encoding='utf-8')
           if l.startswith('|') and len(l.split('|'))>2
         for c in [l.split('|')[1].strip().strip('`').strip()]
         if re.fullmatch(r'[A-Z0-9][A-Z0-9-]{4,}',c)}
    print(len(ids))
    PY

Against those 183, checked mechanically: **no proposed ID collides, none duplicates another
proposed ID, and no proposed ID is a prefix of any ID in the union or vice versa.** The prefix check
was run deliberately, because `PR110-SITE-CENSUS-MATCHES-EFFECT-SITE-NAMES-BY-SUBSTRING` is itself a
prefix-collision defect and a ledger that reproduced it in its own identifiers would be a poor place
to record it. The only prefix pairs in the union are four that already existed and are deliberate
lineage chains (`PR4-PROGRAM-PATH-NOT-UNICODE` → `-CLOSED` → `-CLOSED-NARROWED`, and
`PR7-WIN-READ-RACING-BOUND-TOO-SHORT` → `-TERMINOLOGY`).

**Run the same command against this file and it prints 202** — 183 plus the nineteen below — which
is the check that the rule as written reproduces the rule as applied. It was worth running: an
earlier form of this section put the working-record `C-nnn` keys in a table's **first** cell, where
the command counted all twenty-two of them as IDs and printed 224. The mapping table below therefore
leads with the ledger ID and carries the working-record key second. **A derivation rule stated
beside a table it silently mis-reads is worse than no rule**, because it reads as reproducible.

**Re-derive at land time; these IDs are proposed against the head this section was written at.** F
lands last of the W1/W2 deliverables, so `reviews/FINDINGS.md` may gain rows between this section
being written and it landing. Re-running the command above and the collision and prefix checks is
the whole of the re-derivation, and **withdrawal or addition elsewhere cannot move an ID here**,
because each derives from its own origin and slug and never from a position.

**Withdrawal is safe under this rule**, and that property was needed twice: each ID derives from its
own origin and slug and never from a position, so removing a row leaves every other ID unchanged.
Two rows were removed between the draft and this append and no ID moved.

### The mapping from the working record

The audit trail from the coordinator's carried-findings record to this ledger. The `C-nnn` keys are
that record's, not this file's; they appear here so a reader holding it can follow each finding
across, and nowhere else.

| Ledger ID | Working-record key | Origin as recorded |
|---|---|---|
| `PR101-CFG-ATTR-APPLIED-CFG-INVISIBLE-TO-THE-SCAN` | C-001 | PR #101 pass 2 (`dbedc5f`) |
| `W1-CLASSIFIED-MODULES-IS-A-HAND-MAINTAINED-ROLL-CALL` | C-002 | the coordinator, verifying #100, 2026-09-02 |
| `PR103-CONTAINER-SUBSTRATE-LIST-CHECKS-NAME-ONLY` | C-003 | PR #103 pass 3 (`dd22147`) |
| `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` | C-004 | the coordinator; five instances across four pull requests — lands in **§5 (Fixed)**, not §2 |
| `PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY` | C-005 | PR #103, four passes (`1a5e7f2`, `ffb25bd`, `dd22147`, `9ee6013`) |
| `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED` | C-006 | PR #104 pass 1 (`ca630af`), harm measured at pass 7 (`77174ce`) |
| `PR104-PRELOCK-SCRATCH-NAME-REPRODUCIBLE-ACROSS-RUNS` | C-007 | PR #104 pass 3 (`dbdce08`) |
| — | C-008 | **withdrawn at source, no row** — see "What this append deliberately does not carry" |
| `W1-FIXTURES-NOT-RETIRED-W0-AUTH-PART-E-UNFULFILLED` | C-009 | the project owner, ruling 7, 2026-09-03 |
| `W2-EXPECTED-REFS-COUNT-STALE-AFTER-EXTRACTION` | C-010 | M4's steward, verifying #110 |
| `PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK` | C-011 | PR #107 pass 2 (`b5631dd`), sharpened by M4's steward, re-derived independently by #110's reviewer |
| `W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY` | C-012 | M4's steward, in a gate run at `d8f4d13` |
| `W2-WINDOWS-RACING-REMOVAL-DELETE-PENDING` | C-013 | the winguest investigation, 2026-09-03 |
| `PR110-SITE-CENSUS-MATCHES-EFFECT-SITE-NAMES-BY-SUBSTRING` | C-014 | PR #110 pass 1 (`bab9c0b`), confirmed independently by M4's steward |
| `PR110-CONTAINMENT-COMMENT-STATES-A-FALSE-GUARANTEE` | C-015 | PR #110 pass 2, ruled out of scope |
| — | C-016 | **withdrawn at source, no row.** It is `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM`, a pre-fix instance |
| `W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT` | C-017 | #111, #108 and #110; cross-branch |
| `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT` | C-018 | PR #104, second sighting on #106 — **already in §2: landed in `1f30851`, on `master` via `079a346`; not this append's to add** |
| `PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT` | C-019 | PR #107 (`9963fb0`) |
| `PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT` | C-020 | PR #107 (`9963fb0`) |
| `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES` | C-021 | the coordinator, over the four rows above |
| `W2-RETIRED-DECISIONS-PATHS-CITED-AND-MISSING` | C-022 | M6's steward, quantified by the coordinator |

### The commits this section cites, and whether they still resolve

Every SHA below is a claim that a commit exists. A ledger entry is permanent; a SHA in it that no ref
reaches is a dead citation that looks live. Audited at `ae2a58f` with `git rev-parse --verify
--quiet <sha>^{commit}` — never a bare `$?`, because `git rev-parse` echoes an unknown ref to stdout
and errors only on stderr — and then `git merge-base --is-ancestor <sha> origin/master`:

| Commits | Reachable from |
|---|---|
| `dbedc5f`, `ca630af`, `dbdce08`, `77174ce`, `b5631dd`, `bab9c0b`, `d8f4d13`, `9807f48`, `ae59f2d`, `4ba4149`, `94f8c27`, `5b67179`, `a5d1e14`, `6d8cdda`, `9963fb0`, `6a969a3`, `eac412b`, `c30aca0`, `9a7fc22`, `741364b`, `f1918e0`, `1cbdccd`, `1f30851`, `079a346`, `046f17d`, `2de71dd`, `ac16fff`, `17d41c9`, `ae2a58f` | **ancestors of `origin/master`** — durable |
| `dd22147`, `9ee6013` | PR #103's branch, **closed unmerged**; preserved as `refs/backup/pr103-census-whole-file-test-domain` **on `origin`** |
| `4517caa` | **was orphaned** — not an ancestor of #103's head, so the branch backup does not reach it; preserved as `refs/backup/pr103-4517caa-orphan` **on `origin`** |
| `8d84057` | M3's **pre-rewrite** SHA, cited by the source record as C-011's origin; preserved as `refs/backup/w2-m3-preattrib` **on `origin`**. The post-rewrite equivalent is `b5631dd`, verified by identity rather than by a map: both name tree `7b133c60659bfd626c5b8ad08904c145b76eb1a0` and `git diff 8d84057 b5631dd` is empty |

Verified against the remote rather than from a push's output:

    $ git ls-remote origin 'refs/backup/*'
    4517caa12e7f2b6903f796e8a1650ea9c58f0a18  refs/backup/pr103-4517caa-orphan
    9ee60131aa75af25e8ea8d5cfc1f701b64fb9466  refs/backup/pr103-census-whole-file-test-domain
    4aa3e428c70361d19b6b13ebee57109b9b046b77  refs/backup/w2-m3-preattrib

Those refs sit under `refs/backup/*` rather than `refs/heads/*`, so they create no branch, appear in
no pull-request UI and are fetched by no default refspec; they keep the objects alive and do nothing
else.

**And the durable evidence for a CI observation is the run id, not the SHA.** Every fingerprint row
carries one, and run ids do not rot when a branch is deleted or rewritten.

### Venue, trigger and required evidence

Recorded in §35's carried-row form, so these eighteen are auditable the same way the fifty-two there
are. The bar §35 states applies unchanged: **a deferral by the named owner, to nobody, is not a
disposition.**

| ID | Class | `shrinks_when` | Re-opening trigger | Required evidence for the repair |
|---|:---:|---|---|---|
| `PR101-CFG-ATTR-APPLIED-CFG-INVISIBLE-TO-THE-SCAN` | V3 | `scan_module_declarations` resolves a `cfg_attr` that applies a `cfg` instead of discarding it | a slice changes `scan_module_declarations` or its `cfg_attr` arm | a red-first witness over a synthetic `#[cfg_attr(all(), cfg(test))] mod …;` **and** a killed mutation; **plus** the stated-limit paragraph in `declared_whole_file_test_modules`' doc comment updated in the same change, since it currently records the hole and would then describe code that no longer has it |
| `W1-CLASSIFIED-MODULES-IS-A-HAND-MAINTAINED-ROLL-CALL` | V3 | the module-level domain is derived rather than listed, **or** the list's semantics are stated and executed | a slice changes `CLASSIFIED_MODULES` | a witness that a production child of a listed parent is graded without being hand-enrolled, red first; **or**, if the roll-call is kept deliberately, the decision recorded beside the list with a test that fails when a new child is unenrolled. The `TOPOLOGY_MODULES` half already shrank — `f1918e0` added the `src/workspace_manager/` prefix — and that half is not re-openable |
| `PR103-CONTAINER-SUBSTRATE-LIST-CHECKS-NAME-ONLY` | V3 | `SUBSTRATE` requires **membership** — not a crate root, and in `cfg::WHOLE_FILE_TEST_MODULES` — rather than mere existence | a slice changes the `every_view_discard_removes_through_the_one_racing_removal` census | a red-first witness in which a listed file is production-reachable and the census refuses, **and** a killed mutation. **The repair is cheap and that is checkable**: the two guards are one `assert!` each over data the test already has |
| `PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY` | V3 | `CrateRoots` retains target kind **and** the resolver can answer "is this path declared unconditionally anywhere in the walk" | a slice changes `CrateRoots` or `declared_whole_file_test_modules`, **or** W3 takes up T3 | the reviewer's failure sequence driven red first, a killed mutation, **and** the two path-by-path oracles in `src/effects/tests/source_oracles.rs` still green — the repair must not change what `whole_file_test_modules` returns, which is what killed PR #103's round 2 |
| `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED` | V3 | every scratch root in `src/validate.rs` is uniquely named and RAII-reclaimed | a slice opens `src/validate.rs`'s test region | the reviewer's sentinel reproduction re-run against the repaired binary and **failing to delete the sentinel**; a killed mutation; **plus** authorization to change a frozen-legacy file, which is a separate question from the defect |
| `PR104-PRELOCK-SCRATCH-NAME-REPRODUCIBLE-ACROSS-RUNS` | V3 | `Scratch::new` allocates a name no later run can recompute | a slice changes `src/engine/topology/prelock/tests.rs` | a red-first witness that pre-creates the computed path and shows the allocation **refusing** — it currently **adopts**, silently, because `create_private_dir` → `create_dir` → `fs::create_dir_all` succeeds on an existing directory |
| `W1-FIXTURES-NOT-RETIRED-W0-AUTH-PART-E-UNFULFILLED` | V3 | `src/validate.rs`'s tests stop reading `fixtures/` from disk and the directory is retired | a slice takes up retirement — **blocked behind the `src/validate.rs` scratch row**, which is the same problem for all ten of that file's call sites at once | the four fixture files' content preserved byte-for-byte at their new home, a green suite, **and** `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED` repaired first — eight review passes established that solving retirement without it produces a new temporary-directory finding per round |
| `W2-EXPECTED-REFS-COUNT-STALE-AFTER-EXTRACTION` | V3 | the comment states the property instead of the count, or states a count that is true of the region it names | a packet holds the pin-maintenance grid lock for its own reasons | the corrected text **and** the count re-derived by a command recorded beside it over both files; a count in prose with no command beside it is how this row was created |
| `PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK` | V3 | the census derives its domain from module **declarations** rather than from a directory walk | a slice changes the child-lint census | a red-first witness that relocates children with `#[path]`, leaves one file per arm, and shows the census refusing; **and** a killed mutation. **By-name pinning is not the repair** — it closes the escape only for the files that happen to be pinned, and three packets each adding a name is the shape this programme keeps having to undo |
| `W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY` | V3 | the test can no longer exec a file a sibling `fork` may hold open for writing | a slice changes `src/runner/host/tests.rs`, **or** the failure recurs | **Neither of the two prescriptions this finding has carried is admissible without evidence** — see the row. Acceptable: a retry on `ETXTBSY`, or serialising the write against the harness's forking phase, **with** a demonstration that the chosen form addresses **fd inheritance across a `fork` in another thread** rather than the writing thread's own handle, which `std::fs::write` already closes |
| `W2-WINDOWS-RACING-REMOVAL-DELETE-PENDING` | V5 | the project owner rules on the budget, or the removal path stops depending on a bounded retry | an owner ruling, or a slice opens `racing_removal` | **a Windows reproduction**, never a Linux green; a red-first witness at the named path; **and** an accounting of what the bound protects against, because raising 64 is an infrastructure decision rather than a repair |
| `PR110-SITE-CENSUS-MATCHES-EFFECT-SITE-NAMES-BY-SUBSTRING` | V3 | the census resolves variant names instead of substring-matching them | a slice changes the site census in `src/effects/tests.rs` | a red-first witness that removes an exact shorter literal while keeping its longer partner and shows the census refusing; **and** a killed mutation. **Fix the class, not the pair**: the row enumerates every prefix collision in the tree, and repairing only the pair the reviewer named leaves the other nine |
| `PR110-CONTAINMENT-COMMENT-STATES-A-FALSE-GUARANTEE` | V3 | the sentence is true of the code, by either route | a slice changes `src/workspace_manager/containment.rs`, `remove_intent`, `remove_execution_root` or `Slot::validate` | **either** the deletion paths routed through `contained()` with a red-first witness, **or** the sentence narrowed to what is true and naming the guard that actually applies. Whichever is chosen, the **three-state trace** in the row must be re-run afterwards: a repair that fixes a claim's referent can restore its falsity, which is how this one survived a split and a repair |
| `W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT` | V5 | the pre-exec `setpgid` path is shown to be race-free, or the race is closed | **already fired** — twelve sightings across six branches, `master` included, 2026-09-01 to 09-03 | **a macOS reproduction** at a fixed tree, never a Linux green; a red-first witness on the pre-exec containment path; **and** an accounting of why the failing role varies across three roles, since two attempts of one run at one SHA named two different ones |
| `PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT` | V5 | the cause is established | an owner ruling, a second sighting, or new evidence admissible under §"The authority rule" | **a Windows reproduction**, never a Linux green; and the wider measurement the row names — whether these two tests build their event log deterministically on Windows at all — rather than the path-hint derivation, which `AlreadyStarted` does not touch |
| `PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT` | V5 | the cause is established | an owner ruling, a second sighting, or new evidence | a reproduction of a worktree registration reaching `forced removal converges` with an empty gitdir, on any platform; the row names why the Linux leg makes this the cheapest of the four to chase |
| `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES` | V5 | the class has a stated cause, or its members are shown to be unrelated | an owner ruling, a fifth fingerprint, or a member being explained | **the lead in the row tested rather than asserted**: whether one launch-gate or reaper mechanism underlies members on three different platforms. A repair of any single member does not close this row, and closing it by explaining one member is exactly the reasoning that produced the withdrawal recorded below |
| `W2-RETIRED-DECISIONS-PATHS-CITED-AND-MISSING` | V5 | the citations resolve, or the paths are re-pointed at where each record's substance now lives | an owner ruling, or a slice takes up the repair | the citation count driven to zero by a command recorded beside it, **and** a gate that resolves cited repository paths — without one the class recurs on the next directory retirement, which is how it arrived |

### Six CI signatures — the discriminator, and the one that was misread

These rows are separate **because their signatures differ**, and the discriminator is recorded so
the next observation is classified rather than absorbed. Two entries below are not among this
append's new rows: `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT` is already in §2, and
`W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` is fixed and sits in §5. Both are here because they are the
nearest attractors for the next red, and leaving an attractor out of a discriminator is how
mis-filing starts.

| ID | Platform | What fails | The binary | The tell |
|---|---|---|---|---|
| `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` — **§5, fixed** | macOS | nothing named, **or** a burst of named failures with no summary | **dies on a signal it armed itself** | `(signal: 15, SIGTERM: termination signal)` in cargo's error line, **zero** `test result:` lines, and dozens of orphaned copies of the test binary reaped at "Complete job". Post-repair it is **self-identifying**: each arm site writes one line to fd 2 naming itself |
| `W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT` | macOS | `runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` — **one** test | completes; prints `test result: FAILED. <n> passed; 1 failed` | twelve sightings on six branches **including `master`**; **the failing role varies across three roles**; identical tree green then red, and one run red on two attempts naming two different roles |
| `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT` — already in §2 | Windows (`test (winguest)`) | two `engine::topology::settle` kill tests | completes | `MalformedEntry { kind: "task_dispatched", key: 0 }` and a **trailing slash** — predicted region `src/aleph/` against hints deriving `src/aleph` |
| `PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT` | Windows (`test (winguest)`) | **the same two** `engine::topology::settle` kill tests | completes | `the log replays: AlreadyStarted`, and the string `src/aleph` appears **zero** times in the job log. Same two tests, same file, same leg, **different replay error** |
| `PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT` | Linux (`test (ubuntu-latest)`) | `workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered` — one test | completes | `forced removal converges: Git { message: "worktree registration …/.git/worktrees/kalpha-g1 has an empty gitdir" }` |
| `W2-WINDOWS-RACING-REMOVAL-DELETE-PENDING` | Windows (`test (winguest)`) | `racing_removal` exhausting its 64 attempts against an R19 view directory | completes | **established, not unexplained** — a known mechanism in production code, and the only one of the six carrying a rerun licence |

**The discriminator that failed, and what it cost.** On 2026-09-03 a macOS red on PR #104 at
`94f8c27` (run `33773356014`) was ruled out of the signal-death row and given a new category of its
own, on two grounds: *"libtest named its failures"* and *"exit 101, not a signal"*. Both are
artefacts of the mechanism rather than counter-evidence.

- A stalled launch gate refuses every waiting launch on its next tick, so each waiting test panics
  fast and libtest prints a `FAILED` line for it; the monitor raises `SIGTERM` tens of milliseconds
  later, before libtest reaches its summary. **A named burst with no summary is the signature**, not
  a counter-signature.
- `101` is **cargo's** exit code when its child dies on a signal. The binary died by `SIGTERM` and
  the job exited 101; both are true in every instance.

The ruling-out was done by searching the log for the marks of an *ordinary* failure — panic text,
assertion text, a `test result:` line — finding none, and concluding "not this row". **A signature
was ruled out by the absence of other things instead of by searching for the one line that defines
it.** Re-read from the job log, with the commands beside the results:

    $ gh run view 33773356014 --attempt 1 --job 100708958981 --log > mac.log
    $ /usr/bin/grep -c 'signal: 15' mac.log                     # 1
    $ /usr/bin/grep -c 'failures:$' mac.log                     # 0
    $ /usr/bin/grep -c 'test result:' mac.log                   # 0
    $ /usr/bin/grep -oP '\S+::\S+ \.\.\. FAILED' mac.log | wc -l   # 28
    $ /usr/bin/grep -c 'Terminate orphan process' mac.log       # 46

Twenty-seven of the twenty-eight are `runner::container::exec::tests` and one is `review::tests`,
and every one falls inside a single second. **The `signal: 15` line was in the log the whole time.**
The category invented for the residue is withdrawn; the instance belongs to the §5 row. It is
recorded here rather than tidied away because the reasoning is the reusable part.

**Three traps that point at the wrong subsystem**, every one paid for on this programme:

1. **Counting `signal` in a macOS log finds it eight times, and every one is a test name.** Counting
   occurrences rather than reading them produces a false identification in either direction. It is
   the same instrument failure as every other on this list: an answer about a smaller or different
   domain than the one asked about, returned in the shape of a correct answer.
2. **A `failed to read <path>` message on the Windows removal path means a REMOVAL failed.**
   `UpstrokeError` has one `Io` variant and one format string —
   `#[error("failed to read {}: {source}", .path.display())]` at `src/error.rs:23` — so read, write,
   create, sync **and remove** all render as `failed to read`. The message names the `Display` impl,
   not the operation.
3. **`0123456789abcdef` in those paths is the fixture constant `REPO_KEY_A`**
   (`src/runner/container/census/tests.rs:89`), not an unset `CARGO_TARGET_DIR` slot key. It is
   dangerous here specifically because the slot-pool contamination trap has been drilled on this
   programme and that hex is its visual signature. The fixed-key collision also **cannot** explain a
   CI failure: it needs two concurrent runs on one machine, and CI runs one job per machine.

**The reading rule the whole family depends on: read every run at the head, and every attempt of
every run — never the latest.** A rollup reports GREEN at a SHA with three greens and one red, and
`gh run rerun` does not create a new run — it increments `run_attempt` on the same run id, and the
API returns the latest attempt's conclusion by default. Both mechanisms hid instances of the §5 row.
Enumerated per head, and per attempt within each run:

    gh api "repos/eventloops/upstroke/actions/runs?head_sha=$FULL_SHA&per_page=30" \
      --jq '.workflow_runs[] | "\(.id) \(.name) attempt=\(.run_attempt) \(.conclusion // .status)"'
    gh api "repos/eventloops/upstroke/actions/runs/$ID/attempts/1" --jq '.conclusion'

`gh run list --limit N` counts **entries**, not CI runs, and truncates without saying so; every push
produces two workflow runs, so a window built that way looks complete and is not.

### The findings in full — the eighteen §2 rows

Each is written for a reader who was not here. Every line anchor is at **`ae2a58f`** and every one
was checked to match the construct it names; **re-derive by name, not by line**, because these
anchors have already moved twice in a day while the item names stayed put.

#### `PR101-CFG-ATTR-APPLIED-CFG-INVISIBLE-TO-THE-SCAN`

**What.** `scan_module_declarations` (`src/effects.rs:2207`) decides whether an attribute matters to
a module declaration. Its `cfg_attr` arm is

    "cfg_attr" if raw.contains("path") => pending_path = true,      // src/effects.rs:2282

so a `cfg_attr` is significant **only when its text contains `path`**. A declaration written
`#[cfg_attr(all(), cfg(test))] mod hidden_tests;` — which rustc applies as `#[cfg(test)]`, making
the named file compile only under test — is therefore read as an **unconditional** declaration, and
the file it names stays in every census's domain as production.

**Failure sequence.** Add such a declaration and its file. Leave the whole-file-test-module list
unchanged. Every set assertion still sees the old population and passes. A fixture call in that file
then sits inside the production censuses, where it can mask the deletion of a real production call —
which is the exact failure the skip sets exist to prevent.

**Why it is open rather than repaired.** The gap predates W1 by months and no W1 or W2 diff touches
it. Widening the scanner to *decide* `cfg_attr` predicates changes what **every** census in the
crate scans, and a measurement change gets its own review.

**Already recorded in the tree, which is why nothing here is news to a reader of the code.** The
limit is stated in `declared_whole_file_test_modules`' doc comment (`src/effects.rs:2022-2039`),
including the measurement that established it: *"Measured by writing one and reverting it: the
module's own `#[test]` ran, so rustc had applied the `cfg(test)`, while
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names` stayed green
with the file outside the population it resolves."* A repair must update that paragraph in the same
change, or the tree will carry a comment describing a hole it no longer has.

**Related.** `PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY` is the same resolver,
two gaps further in.

#### `W1-CLASSIFIED-MODULES-IS-A-HAND-MAINTAINED-ROLL-CALL`

**What.** `mechanism` (3)'s classification census reads its domain from
`CLASSIFIED_MODULES` (`src/effects.rs:968`). The consumer is
`reachable_fns_are_classified` (`src/effects/tests/classification.rs:99`), which asserts set
equality between the record's module keys and that constant, then reads each entry from disk. Its
doc comment says *"The domain is **derived from the modules**, not listed: a `pub fn` added to one
of them fails this test until somebody decides what it is."* That is true of the **function-level**
domain and not of the **module-level** one: the list of modules is a roll-call, so a new production
child file is graded only if somebody enrols it by hand.

**Measured at `ae2a58f`, with the derivation rather than the number.** The two lists are parsed from
the source and compared against a walk of `src/`:

    python3 - <<'PY'
    import re,os
    src=open('src/effects.rs',encoding='utf-8').read()
    def const(n):
        m=re.search(r'pub const '+n+r': &\[&str\] = &\[(.*?)\n\];',src,re.S)
        return re.findall(r'"([^"]+)"',m.group(1))
    CLS=const('CLASSIFIED_MODULES')
    allrs=sorted(os.path.relpath(os.path.join(d,f))
                 for d,_,fs in os.walk('src') for f in fs if f.endswith('.rs'))
    print([p for p in allrs if p not in CLS and os.path.dirname(p)+'.rs' in CLS])
    PY

`CLASSIFIED_MODULES` holds **56 entries and not one directory prefix**; `TOPOLOGY_MODULES`
(`src/effects.rs:912`) holds 6, of which 4 are prefixes. **Twenty-one** `.rs` files sit under a
directory whose `.rs` parent is listed and are not themselves listed. **Whether each ought to be
graded is a judgement and this row does not make it** — several are test substrate the domain may
exclude deliberately — but the mechanism is the finding: nothing fails when one is absent.

**A live instance arrived while this append was being written, and it is the best evidence in the
row.** M7 (`#123`, merged at `3af9696`) split `src/config.rs` into `parse.rs` and `read.rs`. The two
children carry **nine `pub(super)` functions** — `read_runner`, `refuse_legacy_container_selection`,
`parse_role_effort`, `parse_budgets`, `parse_gates`, `parse_engine`, `parse_interaction`,
`read_repo_config`, `read_pools`. Derived rather than assumed: **none of the nine was a `pub*` item
of `src/config.rs` before the split**, and the parent's own reachable surface is unchanged at twenty
either side, so nothing *left* the graded domain — **new reachable surface entered the crate outside
it**.

They count. `externally_reachable_fns` decides visibility with `declares_visibility`, whose whole
test is

    rest.ends_with("pub") || rest.ends_with("pub(crate)") || rest.ends_with("pub(super)")

so a `pub(super) fn` is exactly what this census exists to force somebody to classify. Because
`src/config/parse.rs` and `src/config/read.rs` are not in the roll-call, **nothing requires it and
nothing fails**.

**This is not an accusation against M7, and reading it as one would miss the point.** Three splits
before it — `m3-rundir`, `m5-host`, `m6-proc` — enrolled their children and each cited this finding
by its working-record key while doing so. M7 did not. **Neither choice can be called wrong, because
the criterion is nowhere stated**: the tree says only that the list is hand-maintained. And the
incentive runs one way — enrolling a child obliges a classification row for every reachable item in
it, while not enrolling one costs nothing and is checked by nothing. **A roll-call whose membership
rule lives in whoever last remembered it will diverge, and here it took four splits.**

**Half of this finding is fixed, and the fix is why the rest is worth stating precisely.**
`TOPOLOGY_MODULES` is matched with `str::starts_with`, and as recorded it named
`src/workspace_manager.rs` — a file — so the split's children fell outside the ban. `f1918e0` (#110)
added the `src/workspace_manager/` prefix and that half misses nothing today. The two lists are
matched differently on purpose, and the tree now says so at `src/effects.rs:903-911`: prefix
matching is right for a ban, and wrong for a roll-call whose entries are **read as source files**,
because a directory would name nothing. So the repair for the surviving half is not "add a prefix";
it is to derive the module domain or to state and execute the roll-call's semantics.

**The tree already names this finding.** The `m3-rundir`, `m5-host` and `m6-proc` blocks inside
`CLASSIFIED_MODULES` each say, in nearly the same words, that *"`C-002` is the standing finding that this
roll-call is hand-maintained rather than derived, and it is not this split's to repair."* Three
splits enrolled their children correctly and each said why. That is the roll-call working — by
somebody remembering.

**Related.** The stated-domain-versus-counted-domain shape is
`CLASS-GATE-STATED-DOMAIN-EXCEEDS-COUNTED-DOMAIN`, already in §2; this is the same shape in the
domain a gate *reads* rather than the one it counts.

#### `PR103-CONTAINER-SUBSTRATE-LIST-CHECKS-NAME-ONLY`

**What.** `every_view_discard_removes_through_the_one_racing_removal`
(`src/runner/container/tests.rs:4883`) is a source census over `src/runner`, and it excludes
out-of-line test substrate by name through a `SUBSTRATE` const (`:4888`, six entries). The only
assertion over that list is

    assert_eq!(excluded, SUBSTRATE.len(),
      "a file named in SUBSTRATE is not in the tree, so the exclusion is stale");   // :4931

— a check that each name **is met**, not that each name **is still test substrate**. A file named in
it that later becomes production-reachable — compiled as a Cargo target, or declared unconditionally
by a production parent — stays excluded, and nothing notices.

**Failure sequence.** Add an `[[example]]` target whose `src_path` is a listed file, give it a
`#[cfg(not(test))] main` that reaches a governed primitive, and the census skips it. No assertion in
that test can see it.

**Why it is open.** Byte-identical before and after PR #103 and not activated by it.

**The comparison this row used to make is withdrawn, and the withdrawal is the point.** An earlier
statement said PR #103 *"closes the same gap in its own new list"* — entries must not be crate roots
and must be members of `cfg::WHOLE_FILE_TEST_MODULES` — making the newer list strictly better
guarded than the precedent it copied. **PR #103 was closed unmerged.** That list never landed, so
the comparison has no second term. The finding itself is intact and re-verified at `ae2a58f`; only
the sentence about a sibling was wrong, and it was wrong in the specific way this whole append
exists to prevent: **text describing code that does not exist.** The two guards remain the shape of
the repair; they are simply not implemented anywhere.

#### `PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY`

**What.** Two independent gaps in `census_domain`, each established by a separate frontier pass, and
both live on `master`:

1. **Target kind is discarded.** `CrateRoots` (`src/effects.rs:1742`) keeps a `package_dir` and a
   `BTreeSet<PathBuf>` of roots and nothing else; its doc comment states the choice outright —
   *"Kinds are **not** filtered."* Cargo compiles a `[[test]]` target with `cfg(test)` on, so such a
   root can be exclusively test code, but nothing downstream can tell it from a `[[bin]]` or
   `[[example]]` root. A guard that treats "is a root" as "is production" is wrong for test targets;
   one that ignores roots is wrong for production ones. PR #103 was rejected once for each
   direction.
2. **Non-test declarations are ignored.** `declared_whole_file_test_modules`
   (`src/effects.rs:2050`) skips every declaration that is not test-only —
   `if !declaration.test_only { continue; }` (`:2076`) — so membership proves *"some test declaration resolves
   here"* and never *"only test declarations reach here"*.

**Failure sequence** (the reviewer's, at `9ee6013`): `src/topology/probe.rs` declares
`#[cfg(test)] mod fixture;`; `probe/fixture.rs` calls `crate::rundir::public_dir`; a binary root
`probe/bin.rs` declares `mod fixture;` unconditionally and calls it. The fixture is
production-reachable through the bin, but the resolver records only the test-only declaration, and
the bin — not the fixture — is the Cargo root. Any census skipping on this basis misses the caller,
silently.

**Why it matters now, and why it is not confined to a closed pull request.** Two shipped censuses
derive their skip sets from this resolver at `ae2a58f`, both adopted under `PR7-R5-ATT-001` — **an attestation key carried in the source, not a row in this file**; it resolves at `src/effects/tests/source_oracles.rs:1569`, `src/runner/mod.rs:1456`, `src/events/log/tests.rs:3412` and twice in `src/engine/topology/recover/tests.rs`, and a reader should not look for a ledger row of that name — **an attestation key carried in the source, not a row in this file**; it resolves at `src/effects/tests/source_oracles.rs:1569`, `src/runner/mod.rs:1456`, `src/events/log/tests.rs:3412` and twice in `src/engine/topology/recover/tests.rs`, and a reader should not look for a ledger row of that name:
`runner::tests::production_sources_by_path` (`src/runner/mod.rs:1458`) and the fold census in
`src/events/log/tests.rs:3414`. Both carry the blind spot today.

**The shape of the repair, recorded so whoever takes it need not re-derive it.** Retain target kind
in `CrateRoots`, and add a query for "is this path declared unconditionally anywhere in the walk".
Neither changes what `whole_file_test_modules` *returns*, so the two path-by-path oracles in
`src/effects/tests/source_oracles.rs` that killed PR #103's round 2 stay satisfied.

**Downstream.** W1's T3 — extracting `src/topology/registry.rs`'s test module — is deferred behind
this repair; that extraction is what needed the skip in the first place.

#### `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED`

**What.** Every temporary directory in `src/validate.rs`'s test region is derived from
`env::temp_dir().join(format!("upstroke-validate-<tag>-{}", process::id()))` — a **predictable**
path, created with `create_dir_all` (which accepts an existing directory), stored as a bare
`PathBuf`, and **never reclaimed**. `scratch_root` (`:403`) additionally runs

    let _ = fs::remove_dir_all(&dir);       // src/validate.rs:405

against that predictable path *before* creating it, so it deletes whatever a previous run or another
process left there, and discards the error while doing so.

**Measured at `ae2a58f`, two engines agreeing per file:** `src/validate.rs` has **12**
`env::temp_dir()` sites, **12** `create_dir_all` lines and **0** `impl Drop`.
`standards/12_standards_tests.md:16` requires *"unique temporary directories with RAII cleanup"* —
the clause moved out of `CODING_STANDARDS.md` when #116 split the standards, and it is the same
clause.

**The harm is measured, not argued.** PR #104's pass-7 reviewer pre-created
`$TMPDIR/upstroke-validate-sample-<pid>/foreign-sentinel` and ran `sample_plan_renders_expected_table`
against the exact-head binary. The test **passed**, and:

    sentinel=deleted
    replacement_plan=present

So the unowned `remove_dir_all` silently deleted foreign content and leaked its replacement. Every
earlier statement of this finding described a possible sequence; that is a demonstrated one.

**Failure sequence, for the half that is not yet demonstrated.** A previous run leaves the directory
behind; after PID reuse `create_dir_all` accepts it. If a plan name is now a directory the suite
panics; if it is a symlink, `fs::write` follows it and truncates the target. Two PID namespaces
sharing a temp mount can hold the same PID concurrently, so one process can read a file while
another rewrites it.

**Why it is open.** Byte-identical before and after PR #104 and not activated by it; the reviewer
said so explicitly and kept it out of the verdict, which turned on the newly introduced instance of
the same pattern. That instance no longer exists: owner ruling 7 reverted `src/validate.rs` to
`origin/master` entirely, so the file is back to the ten pre-existing instances with none of the
repair. **The repair and its scaffolding are gone; the pattern they were built beside is what
remains.**

**Related.** `PR104-PRELOCK-SCRATCH-NAME-REPRODUCIBLE-ACROSS-RUNS` is the same class in the
precedent this file was told to copy, and `W1-FIXTURES-NOT-RETIRED-W0-AUTH-PART-E-UNFULFILLED` is
blocked behind this row.

#### `PR104-PRELOCK-SCRATCH-NAME-REPRODUCIBLE-ACROSS-RUNS`

**What.** `Scratch::new` (`src/engine/topology/prelock/tests.rs:200`) names its root

    "upstroke-prelock-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()

Every component resets when the process does, so the name is **reproducible across runs**. A killed
run leaves a root behind; a later run that reuses the pid and gets the same thread id computes the
same path — and the allocator **adopts** it silently rather than refusing:
`create_private_dir` (`src/rundir.rs:634`) → `create_dir` (`:575`) → `fs::create_dir_all`, which
succeeds on an existing directory.

**Why it is open.** Byte-identical across PR #104 and not called by it. The reviewer said so
explicitly and kept it out of the verdict.

**Why it is worth recording anyway, and this is the substance.** This is the precedent PR #104 was
told to copy, on the strength of its measured success against leaking — 5050 `upstroke-prelock-*`
roots had accumulated by 2026-08-30 and none since, a fact its own doc comment records at
`src/engine/topology/prelock/tests.rs:181`. **It is a good precedent for reclamation and it carries
a defect in allocation**, and the packet that copied it inherited the defect along with the virtue.
Copying a precedent copies its weaknesses; the fix belongs with whoever owns that file.

**Related.** `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED` — the same allocation
weakness, ten times over, in the file that copied this one.

#### `W1-FIXTURES-NOT-RETIRED-W0-AUTH-PART-E-UNFULFILLED`

**What.** W0-AUTH **Part E** said: retire `fixtures/` and inline the corpus. **`fixtures/`
survives** — `bare-plan.md`, `cyclic-plan.md`, `sample-plan.md`, `steps-plan.md`. This row exists so
an unfulfilled packet clause is not later read as a fulfilled one.

**What PR #104 as landed did achieve, re-derived at `ae2a58f`.** Every **runtime** fixture read
outside `src/validate.rs` is gone. `src/plan/mod.rs` takes the corpus at **compile time** —
`BARE_PLAN` (`:82`), `SAMPLE_PLAN` (`:87`), `STEPS_PLAN` (`:91`), each an `include_str!` — and
`src/plan/markdown.rs` and `src/topology/registry.rs` (`:3123-3125`) consume those constants;
neither reads a fixture path any more, and `src/plan/markdown.rs`'s `fn fixture` helper, which built
the path as `Path::new("fixtures").join(name)` and so was invisible to a `fixtures/` search, is
gone. **`src/validate.rs` is the one remaining runtime reader, with 10 call sites**, all of the form
`opts("fixtures/<name>.md")`. `cyclic-plan.md` is the one file with no compile-time constant; its
only consumer is `src/validate.rs:739`.

**Why it stopped there.** `src/validate.rs` is frozen-legacy, and every attempt to give its tests a
corpus on disk produced a new finding about temporary-directory ownership — five across four repair
rounds, then three more at pass 8. Owner ruling 7 reverted the file entirely rather than ship the
ninth.

**What is owed.** A future change may retire `fixtures/` properly, from a base that is not eight
passes deep. It needs `src/validate.rs`'s tests to stop reading from disk, which means solving the
scratch-directory problem for that file — which is
`PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED`'s problem for all ten of its call
sites at once. **Doing that row first makes retirement straightforward**, and doing it second is
what produced eight passes.

#### `W2-EXPECTED-REFS-COUNT-STALE-AFTER-EXTRACTION`

**What.** `production_calls`' doc comment (`src/effects.rs:1370`) asserts *"Measured on this tree:
`workspace_manager.rs` carries four occurrences of the substring `expected_refs(`"*, and then
reasons from that number — *"one of the four survives into `production_code`'s region, and it is the
definition line of `refuse_unexpected_refs`"*. **The root file carries one.** The other three moved
to `src/workspace_manager/tests.rs` when W1 extracted the test region.

**Derived at `ae2a58f`, two engines per file:**

| file | occurrences of `expected_refs(` |
|---|---:|
| `src/workspace_manager.rs` | **1** |
| `src/workspace_manager/tests.rs` | **3** |
| every other file under `src/workspace_manager/` | 0 |

**The number is right about the subsystem and wrong about the file it names**, which is precisely
why nobody caught it: a reader who recounts across the directory reproduces "four" and moves on.

**Why it is open.** Already stale before W2 began — W1's extraction caused it, and no W2 packet
causes or worsens it. The steward who proposed it checked both directions before doing so.

**What the repair must not be.** Another count. `src/effects.rs` is in the pin-maintenance set, so
this edit takes the grid lock and belongs to whichever packet next holds it for its own reasons;
whoever makes it should state the **property** the comment needs — that a substring needle is
satisfied by a longer identifier, which is the point the sentence exists to make — rather than
re-measure a number that the next extraction will falsify again.

#### `PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK`

**What.** The child-lint census in `src/runner/container/tests.rs` derives its domain by walking each
funnel's directory: `const FUNNELS` (`:3146`), then `let arm = walk(&directory);` (`:3170`), `walk`
being the recursive reader at `:2971`. **A `#[path]` relocation is invisible to a directory walk by
construction**, so a child moved out of the directory is never graded, and every control still
passes.

**The controls, and why each one survives the escape.** M4 closed the reviewer's aggregate-floor
escape at `660e9e1` by replacing `assert!(with_children >= 2)` with
`assert_eq!(with_children, FUNNELS.len())` (`:3183`) and adding a per-arm
`assert!(!arm.is_empty())` (`:3171`) inside the loop — stated over the class, so every future funnel
root inherits it. Those repairs are on `master` and they are correct. They do not reach this
variant: relocate all but one file of an arm and the directory still exists, the arm is still
non-empty, `with_children` is unchanged, the union floor `children.len() >= 9` (`:3196`) is still
met by the files named individually, and the sixteen by-name assertions still find their sixteen.

**The bound, derived at `ae2a58f` rather than asserted:**

    python3 - <<'PY'
    import os,re
    F=["src/runner/container.rs","src/agent/proc.rs","src/runner/host.rs",
       "src/rundir.rs","src/workspace_manager.rs"]
    src=open('src/runner/container/tests.rs',encoding='utf-8').read()
    region=src[src.index('const FUNNELS: [&str; 5]'):src.index('let mut missing = Vec::new();')]
    named={m for m in re.findall(r'"(src/[A-Za-z0-9_/]+\.rs)"',region)} - set(F)
    walked=sorted(os.path.join(d,x).replace('\\','/')
                  for f in F for d,_,fs in os.walk(f[:-3]) for x in fs if x.endswith('.rs'))
    print(len(walked), len(named), len([w for w in walked if w not in named]))
    PY

**38 walked children, 16 named individually, 22 named by nothing but the walk.** Relocating the 22
with `#[path]`, minus one file kept in each arm that would otherwise empty, leaves **20 files
ungraded with every assertion in the test still green** — the union is 18, over the floor of 9;
`with_children` is 5; no arm is empty; all 16 named files are present.

**Why by-name pinning is not the answer.** Pinning another child by name catches this only if the
pinned file happens to be one of the relocated ones. It is a partial mitigation, not a closure — and
the count of pinned names has already gone 1 → 6 → 16 across three packets, each adding its own,
which is the shape this programme keeps having to undo.

**The prescription, so a repair need not re-derive it: derive the domain from the module
declarations rather than from a directory walk.** The repository already has the pattern and it is
the precedent to cite —
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`, whose body is in
`src/effects/tests/source_oracles.rs`, resolves exactly this way for exactly this reason.

**Why it is open rather than repaired.** Pre-existing at `1cbdccd`: relocating eleven Container
children and leaving one already passed the floor with Process and Host present. Neither M3's nor
M4's split activates it and neither makes it worse. A mechanism change to a census gets its own
review.

**Two independent derivations.** #110's reviewer reached the walk-based-domain blind spot from the
same `#[path]`-plus-decoy reasoning M4's steward had sent earlier, without seeing it — which raises
confidence in the finding and in the prescription alike.

**Related.** `W1-CLASSIFIED-MODULES-IS-A-HAND-MAINTAINED-ROLL-CALL`,
`PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY`, and
`CLASS-GATE-STATED-DOMAIN-EXCEEDS-COUNTED-DOMAIN` already in §2.

#### `W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY`

**What.** `an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name`
(`src/runner/host/tests.rs:7179`) writes an executable into a workspace through `marker_shim`
(`:5329`) and immediately spawns it. In a gate run at `d8f4d13` the spawn failed with

    "an empty entry before a real installation: a raw spawn: Text file busy (os error 26)"

— **ETXTBSY**: a concurrently-forking thread in the same process still held a write descriptor to
that file when `execve` ran. That is the textbook **write-then-exec race under a parallel harness**,
a concurrency failure rather than a logic one. The quoted text is the test's own panic format,
`"{what}: a raw spawn: {error}"`, with `what` the first row of its table.

**Not caused by any W2 packet, and the two functions travelled through three splits unchanged.**
Both are byte-identical from the W2 base to `ae2a58f` — hashed by extracting each function's body
and digesting it, rather than by reading a diff:

    fn marker_shim                    1cbdccd = 2de71dd = 17d41c9 = ae2a58f
                                      sha256 f666ed741cb5b533b942c7f41635f0bc3c16c36050f17a92feb12175b4ee5381 (701 bytes)
    fn an_empty_path_entry_…          1cbdccd = ae2a58f
                                      sha256 098f21e8ec2e784665a39bdcb4bc317a4a43f1c4232f8dba9db55886480058ec (4489 bytes)

M5 split `src/runner/host.rs` and M6 split `src/agent/proc.rs` in that window; **the race travelled
with the file unchanged.**

**Why it is open.** Pre-existing, not reproducible on demand, and fixing it inside a split packet
would put a concurrency change in a refactor's diff.

**Both prescriptions this finding has carried are refuted, and that is the most useful thing in this
row.** It was first written as *"an explicit `drop(file)` plus `sync_all`"*. The writer is
`std::fs::write`, which **already drops its handle before returning**, so that closes nothing still
open. The window the finding correctly diagnoses is **fd inheritance across a `fork` running in
another harness thread**: a concurrently-forking thread holds a descriptor at `execve` time even
though the writing thread has closed its own. The replacement prescription — write to a temporary
name and rename into place — was then also carried, and it does not survive either: a `fork` that
inherits the descriptor inherits it regardless of what the path is called, and the rename changes
the name rather than the open-file table. **What a repair must demonstrate is that it addresses fd
inheritance across a `fork` in another thread**, not that it closes the writer's own handle. A retry
on `ETXTBSY`, or serialising the write against the harness's forking phase, are candidates; neither
has been measured.

**Why it matters more than "a flake".** *"Passes most of the time"* is exactly how this class
survives, and the failure lands on whichever test happens to be spawning — so it is **misattributed
by construction**, the same property that made `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` sit
unrecognised across four instances.

#### `W2-WINDOWS-RACING-REMOVAL-DELETE-PENDING`

**What.** `racing_removal` (`src/runner/container.rs:1437`) retries a removal
`RACING_ACCESS_ATTEMPTS` times — `pub const RACING_ACCESS_ATTEMPTS: usize = 64` (`:404`) — and then
returns `UpstrokeError::Io`. On the Windows guest it exhausts that budget against an R19 view
directory under **delete-pending** semantics, at roughly **2%** of runs on a 16-vCPU guest. **It is a
defect in production code**, not in the harness or the build box.

**What it is not.** Not concurrency and not Docker. The guest has no Docker, and its jobs never
overlap: 123 executions, **zero** overlaps. The contention hypothesis this programme carried through
W1 — including by the coordinator — is wrong, and this row supersedes every earlier
characterisation.

**Two traps, both of which point at the wrong subsystem.**

1. **`failed to read <path>` means a REMOVAL failed.** `UpstrokeError::Io` has one `Display` —
   `#[error("failed to read {}: {source}", .path.display())]`, `src/error.rs:23` — so read, write,
   create, sync and remove all render the same way. The message names the `Display` impl, not the
   operation.
2. **`0123456789abcdef` in those paths is the fixture constant `REPO_KEY_A`**
   (`src/runner/container/census/tests.rs:89`), **not** an unset `CARGO_TARGET_DIR` slot key. It is
   dangerous here specifically because the slot-pool contamination trap has been drilled on this
   programme and that hex is its visual signature.

**How to tell it from a compile break.** Three Windows legs failing together — `lint (windows)`,
`msrv (windows-latest)`, `test (winguest)` — is a **compile error**; that is what a `cfg`-gated
unused import produced on #110. **`test (winguest)` alone, on a `racing_removal` signature, is this
race.**

**Disposition detail.** A rerun on this signature is legitimate, disclosed as such — it is the only
one of the six CI signatures in this section carrying that licence, and it has it because the
mechanism is established rather than because the failure is inconvenient. **Raising the 64 is not
the fix**, and it is an infrastructure decision for the project owner rather than a packet's to
make.

**Supersedes** the "winguest container-census races" line carried in earlier state records and its
pairing with the macOS signal death. Those are a different platform and a different mechanism.

#### `PR110-SITE-CENSUS-MATCHES-EFFECT-SITE-NAMES-BY-SUBSTRING`

**What.** `every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
(`src/effects/tests.rs:2625`) decides that a funnel names a site by plain substring containment:

    let variant = format!("{group}Site::{}", site.variant());
    if source.contains(&variant) {                                  // src/effects/tests.rs:2677

so **a longer variant satisfies a search for a shorter one**. `WorktreeSite::RemoveExecutionRoot`
(`src/workspace_manager.rs:747`) satisfies a search for `WorktreeSite::Remove`.

**Failure sequence.** Remove the exact `WorktreeSite::Remove` literal while keeping
`RemoveExecutionRoot`. The census stays green and the removed site goes unnoticed.

**The exposure is a class, not a pair, and it is enumerable.** Every group's funnel module is the
same for all its sites (`FunnelGroup::module()`, `src/topology/effects/vocab.rs:79`), so a
within-group prefix collision is a same-file collision. Parsing the variant lists out of
`src/topology/effects/sites.rs` and testing each pair at `ae2a58f` gives **ten collision pairs over
six shorter variants in four groups**:

| group | the shorter variant | is satisfied by |
|---|---|---|
| `WorktreeSite` | `Add` | `AddStaging` |
| `WorktreeSite` | `Remove` | `RemoveExecutionRoot`, `RemoveIntent`, `RemoveStaging`, `RemoveStagingIntent` |
| `WorktreeSite` | `RemoveStaging` | `RemoveStagingIntent` |
| `SnapshotSite` | `Remove` | `RemoveIntent` |
| `ContainerSite` | `Remove` | `RemoveIntent` |
| `EventSite` | `Append` | `AppendFirst`, `AppendInformational` |

**None of the six is masked today**, which is why this is carried rather than repaired: each shorter
variant is still present as an exact literal — not merely as a substring — in its own funnel module,
measured by counting matches not followed by an identifier byte. So the collisions have nothing to
hide, yet.

**Why it is open.** Pre-existing, and not activated by #110: the exact `WorktreeSite::Remove`
literal is still present at `src/workspace_manager.rs:396`. Verified by the steward before proposing
it.

**A note on a number this finding retired.** #110's body evidenced "eleven effect sites" and
under-counted, because Slot's mapping methods name twelve distinct variants. The count was stripped
under ruling 10; **the finding survives that, because the census weakness is independent of whether
any body quotes a number.**

**Related.** The same "match names on disk rather than resolve them" family as
`PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY` and
`PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK`.

#### `PR110-CONTAINMENT-COMMENT-STATES-A-FALSE-GUARANTEE`

**The claim.** `src/workspace_manager/containment.rs:83`:

> every deletion **in this subsystem** goes through
> [`WorkspaceManager::contained`](super::WorkspaceManager::contained), which compares **canonical**
> paths, so a resolved link cannot carry a removal outside the root.

**It is FALSE — not stale.** Recorded in those words deliberately: *"pre-existing, referent
updated"* reads as a bookkeeping nit, and this is a false containment assertion sitting in a
security comment.

**Every deletion in the subsystem's production region, at `ae2a58f`:**

| site | enclosing fn | through `contained()`? | what actually provides containment |
|---|---|:---:|---|
| `fs::remove_dir_all` `:478` (windows), `:506` (unix) | `remove_tree_once_handles_close` | **no** | nothing of its own — a private helper that deletes whatever path its caller hands it |
| `fs::remove_dir` `:760`, `:766` | `remove_execution_root` | **no** | fixed literal components joined onto `self.execution_root`, after `revalidate()`. Its own `# Errors` line says *"The containment refusals"* |
| `fs::remove_file` `:842` | `remove_intent` | **no** | `slot.validate()` → `safe_component` |
| helper call `:1216` | `remove_worktree` | **yes** (`:1215`) | `contained()`, as documented |
| `fs::remove_file` `:1232` | `remove_worktree`, the `locked` file | **no** | `revalidate_removal` binding the admin directory |
| helper call `:1256` | `remove_worktree`, the admin tree | **no** | `registration_still_names`, checked immediately before |

**One of six goes through `contained()`.** `contained()` has exactly one production call site in the
whole subsystem: `src/workspace_manager.rs:1215`.

**What actually provides the containment, which the comment does not name.** `Slot::validate`
(`src/workspace_manager/naming.rs:189`) calls `safe_component` (`:136`), which rejects any name that
is not ASCII alphanumerics, `-` and `_`. So the subsystem is safe on the `remove_intent` path — **by
a different mechanism than the one documented.** That is the hazard, not a harmless imprecision: a
future refactor that removes or weakens `safe_component`, or adds a deletion path reaching
`fs::remove_file` without `validate`, will be reading a comment promising a canonical-path guard
that does not run there.

**The `Slot::Staging` arm is not a gap — looked at, safe by construction.** `Staging { sequence: u64 }`
holds no string, so `validate` returns early (`naming.rs:192`) and there is nothing for
`safe_component` to reject; the reconstruction path parses `merge.s<rest>` with
`rest.parse::<u64>()`, so even a hostile intent filename cannot produce a Staging component that is
not `s<digits>`. Recorded rather than left open, because an unchased flag inside a carried finding
invites either an afternoon re-deriving it or a "fix" that validates a `u64` — a guard that cannot
fire.

**Which sharpens the finding.** The real guard is load-bearing on two of three arms: `Task` holds a
caller-controlled `String` and needs it; `Snapshot` needs it **at use**, because
`SnapshotName(rest.to_owned())` takes the string unvalidated at construction and containment survives
only because `validate()` re-checks later; `Staging` has nothing to validate. **So the documented
guard is the real one on none of the arms**, and removing `safe_component` in the belief that
`contained()` covers deletion costs `Task` and `Snapshot` their containment.

**The three-state trace, which is why nobody caught it.**

| state | the sentence says | true? |
|---|---|---|
| base `1cbdccd` | every deletion **in this module** — the parent | **FALSE** |
| after the split | every deletion **in this module** — the child | **vacuous**; the child has zero deletions |
| after repair r1 | every deletion **in this subsystem** | **FALSE again** |

**A split can make a false claim vacuous, and repairing the referent makes it false again.** Neither
the split nor the repair was wrong — the reviewer's finding correctly asked for the referent to be
fixed — and the packet still shipped a false sentence it did not write. At any single state it looks
like either a pre-existing defect or a clean repair; only the trace shows it. The parenthetical the
repair added (*"This module performs no deletion of its own"*) is true and is not the problem: the
sentence it qualifies is.

**Disposition.** Carried. Repairing it means either routing the deletion paths through `contained()`
or narrowing the sentence to what is true and naming `safe_component` as the guard that applies. Not
#110's to do — the reviewer ruled it out of scope.

#### `W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT`

**Open as an unexplained observation, not classified as a flake or regression.**

`runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` iterates every
execution role and asserts each child led its own process group. On macOS it fails intermittently,
on branches that do not touch `runner::host`:

    assertion `left == right` failed: <role>: the child did not lead its own process group,
      so the pre-exec containment step did not run for this role
      left: [false]
     right: [true]
    test result: FAILED. 1798 passed; 1 failed; 33 ignored      <- at eac412b; the passed count tracks the head

**Twelve sightings across six branches between 2026-09-01 and 2026-09-03.** The population is every
failing `test (macos-latest)` job on every CI run of `master` and the eight W1/W2 branches, read per
attempt rather than per run, and each sighting is confirmed by its own
`… every_role_reaches_the_containment_points_of_this_platform ... FAILED` line — **a mention of the
test name is not a sighting**, and counting mentions returns a different, larger set:

| branch | head | run / attempt | site | failing role |
|---|---|---|---|---|
| `master` | `fff6abd` | `33503020178` att. 1 | `host.rs:5574:13` | `probe(claude-code)` |
| `master` | `810f264` | `33535107935` att. 1 | `host.rs:5574:13` | `implement` |
| `w2-m3-rundir` | `27e905e` | `33757851135` att. 1 | `host/tests.rs:4220:9` | `probe(claude-code)` |
| `w2-m5-host` | `6a969a3` | `33774631020` att. 1 | `host/tests.rs:4227:9` | `review` |
| `w2-m1-fold` | `eac412b` | `33775798417` att. 1 | `host/tests.rs:4220:9` | `review` |
| `w2-m4-workspace` | `c30aca0` | `33777752620` att. 1 | `host/tests.rs:4220:9` | `probe(claude-code)` |
| `w2-m4-workspace` | `c30aca0` | `33777752620` att. **2** | `host/tests.rs:4220:9` | `review` |
| `w2-m5-host` | `a39b4df` | `33794294653` att. 1 | `host/tests.rs:4227:9` | `probe(claude-code)` |
| `w2-m6-proc` | `7a404df` | `33797128022` att. 1 | `host/tests.rs:4220:9` | `implement` |
| `w2-m5-host` | `4a2ab29` | `33797192635` att. 1 | `host/tests.rs:4227:9` | `probe(claude-code)` |
| `master` | `17d41c9` | `33803719525` att. 1 | `host/tests.rs:4229:9` | `review` |
| `w2-m6-proc` | `b30eba3` | `33804224405` att. 1 | `host/tests.rs:4229:9` | `probe(claude-code)` |

**The four differing sites are one assertion moved by successive splits, not four assertions.** The
two 2026-09-01 sightings are at `src/runner/host.rs:5574:13` — the location **before** W1 extracted
the test region, verified by reading that file at `fff6abd` — and carry the identical panic message.
`:4220`, `:4227` and `:4229` are the same line after M1, M5 and M6 in turn.

**Three facts this population establishes that a smaller one did not.**

1. **It fires on `master`, three times.** Not "a pull request that touches neither subsystem" — the
   integration branch itself, most recently at `17d41c9`. No packet-level explanation survives that.
2. **It predates W2's base commit, and the span is anchored rather than described.** The earliest
   sighting is **2026-09-01T11:32:42Z** (run `33503020178`, the API's `created_at`, which is UTC);
   W2's base `1cbdccd` is committed **2026-09-02T20:55:44Z**. The failure is therefore **33.4 hours
   older than the programme's own starting point**, and older still than any packet branch.

   **Both stamps are UTC, and that has to be said, because the obvious command does not print UTC
   and two of the plausible fixes do not either.** `git log -1 --format=%ci 1cbdccd` gives
   `2026-09-02 21:55:44 +0100` — the committer's local time with its offset — so a reader comparing
   that bare figure against a `Z`-stamped run time computes **34.4** hours and concludes this row is
   an hour wrong when it is not. `%cI` and `--date=iso-strict` **also** render the commit's own
   offset and do not help; `TZ=UTC` does not override them. The two forms that do:

       git log -1 --format=%ct 1cbdccd                              # 1788382544 — epoch, no timezone
       TZ=UTC git log -1 --format=%cd --date=iso-strict-local 1cbdccd   # 2026-09-02T20:55:44+00:00

   **Prefer the epoch form.** It carries no timezone to get wrong, so it cannot be misread the way
   every rendered form above can.

   This is the timestamp instance of the rule the rest of this section is built on: **a number is
   only evidence together with the method that produced it**, because two correct methods give two
   different-looking answers and the disagreement then looks like an error in the claim rather than
   in the comparison. It is recorded because it happened twice in one exchange over this very row: a
   bare local-time figure was compared against a `Z`-stamped run time and produced an apparent
   contradiction between two correct records, and the first command written here to prevent that was
   itself checked by running it and **did not reproduce** — it printed the `+01:00` form. A
   reproduction command that has not been run is a claim, not a method. **The `W2-` prefix in this ID records the wave
   the finding was made in, not when the failure began**, and a reader should not infer the latter
   from it. *An earlier revision of this row said "by two days", which is not what those two
   timestamps give — a span stated loosely in a section about measurements that do not reproduce.*
3. **The failing role varies across three roles**, not two: `probe(claude-code)` six times, `review`
   four, `implement` twice.

**An earlier statement of this finding said four sightings on three branches in about two hours.**
That was an undercount of three kinds at once, and each kind is worth naming because each is a
separate instrument failure: sightings were collected **as packets reported them** rather than by
enumerating the population, so `master`'s three were never in scope; the **run** rather than the
**attempt** was the unit, so one run's second red was invisible; and the window was taken as the
window anybody had looked at rather than as the window the runs span.

**It is not diff-caused, and there are now two independent proofs.** The narrow one:
`c30aca0`'s delta from `9a7fc22` is `reviews/`-only; `9a7fc22` was **green** (run `33776069960`,
attempt 1) and `c30aca0` is **red** — the same tree with a markdown file added. Independently, #108
does not touch `runner::host` at all: `git diff --stat origin/master...eac412b -- src/runner/host.rs
src/runner/host/` is empty. **The broad one, which supersedes both: it fires on `master`.**

**One run settles what the varying role means.** Run `33777752620` is red on **both attempts at the
identical commit**, naming `probe(claude-code)` on attempt 1 and `review` on attempt 2. A
rerun-in-place that fails again with a different role is direct evidence that **any** role can lose —
consistent with a race in the pre-exec `setpgid` path rather than with anything specific to a role.
It is also the reason the unit of enumeration here is the **attempt**: the API reports that run as
one failure, and it is two. Enumerating runs rather than attempts is how this row undercounted, and
it is the same mechanism §8b of the packet rules describes for a red hidden behind a green — here
hiding a red behind a red, which no rule had written down.

**What is not established.** Whether this is a face of `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` is
**open**. The signatures differ — that row kills the binary with a signal and reaches no summary;
this one names one test, panics cleanly, and the binary prints its `test result:` line — and they
are deliberately not merged on family resemblance. **The repair in PR #115 makes the question answer
itself**: if this shape stops recurring on heads containing it, it was the same defect; if it recurs
there, it is a different one. The test runs in every macOS job, so the evidence accrues whether or
not anyone works on it. **Merging them now would destroy exactly the evidence that settles them.**

**Not attributable to any packet.** M5, M1, M4, M3 and M6 have each shown a sighting, on different
branches, and `master` has shown three. Each packet disclosed its own with its own evidence and none
was asked to fix it.

**Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.**

#### `PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT`

**Open as an unexplained observation, not classified as a flake or regression.**

Two `engine::topology::settle` kill tests fail together on the Windows guest with a replay error:

    engine::topology::settle::tests::kill_after_failed_settlement_rematerializes_question
      panicked at src\engine\topology\settle\tests.rs:1764:56 — the log replays: AlreadyStarted
    engine::topology::settle::tests::retained_generation_not_continued_after_kill
      panicked at src\engine\topology\settle\tests.rs:1807:60 — the log replays: AlreadyStarted
    test result: FAILED. 1760 passed; 2 failed; 35 ignored

Run `33785587535`, attempt 1, job `100749444333`, `test (winguest)`, at `9963fb0` on PR #107.
`upstroke-ci` concluded failure on the back of it.

**It has its own ID, deliberately, and is NOT folded into
`PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT`.** That row is a predicted-region trailing-slash
mismatch — `src/aleph/` against `src/aleph`, `MalformedEntry { kind: "task_dispatched", key: 0 }`.
This is `AlreadyStarted`: a replay seeing a start event when already started, an **event-ordering**
failure rather than a path-derivation one. **The string `src/aleph` appears zero times in this job
log**, checked in a local copy of it. Same module, same leg, same two tests, **different assertion**.
Folding two distinct fingerprints into one record is how a class stops being countable.

**What is established, and what is not.** Established: the same two tests, in the same file, on the
same platform, fail nondeterministically with **two different replay errors** across three sightings
on three branches. Not established: whether one mechanism produces both errors. Recording the shared
surface without inventing the shared cause is the discipline here, and the temptation runs the
opposite way from the usual — two observations that share a test look far more alike than two that
share a platform.

**Nondeterministic, established by the same head passing twice and failing once**, every run
`attempt=1` so nothing is hidden inside a row:

    33784774150  9963fb0  17:28Z  success
    33785587535  9963fb0  17:36Z  FAILURE   <- this
    33786611538  9963fb0  17:47Z  success

The red run was started by a **body edit**, not a code change: all three are the same commit.

**Not a regression from the PR #115 repair.** These are kill-path tests and #107's base was the
first to carry #115, so it had to be checked rather than assumed. **A regression would be
deterministic; this is not** — the identical tree passed, failed, and passed again.

**A lead, recorded as a lead and asserted nowhere.** Both failing tests spawn subprocesses through
the settle kill path, and #115 established that one stalled launch gate can refuse every waiter at
once. **Whether that mechanism reaches this failure is untested**, and the evidence above does not go
that far.

**What would settle it.** The measurement named by `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT`
targets path-hint derivation, which `AlreadyStarted` does not touch. The wider question is whether
these two tests build their event log deterministically on Windows at all.

**Not rerun** — no licence covers this signature. Disclosed in PR #107's body with its own evidence
and merged under the project owner's explicit direction that it is non-blocking, never as a flake.

**Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.**

#### `PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT`

**Open as an unexplained observation, not classified as a flake or regression.**

    thread 'workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered'
      panicked at src/workspace_manager/tests.rs:5691:10:
      forced removal converges: Git { message: "worktree registration
        /tmp/upstroke-wm-sample-add-2519-123/repo/.git/worktrees/kalpha-g1 has an empty gitdir" }
    test result: FAILED. 1806 passed; 1 failed; 35 ignored

Run `33787330192`, attempt 1, job `100755588011`, `test (ubuntu-latest)`, at `9963fb0` on PR #107.

**A third platform.** Its own ID rather than folded into either Windows row: different platform,
different subsystem, different assertion. Nondeterministic — the same commit produced two green runs
of this leg in the same hour.

**Why this one is the cheapest of the four to chase**, recorded so the choice is not re-derived: it
is the only member on the Linux leg, which is the platform this programme's build box can reproduce
on directly. A repair for it needs no guest and no hosted macOS runner.

**Cause unknown.** The registration path reaching `forced removal converges` with an empty gitdir is
the same shape `remove_worktree` handles deliberately elsewhere — a killed `git worktree add` can
leave an empty `commondir`, and `src/workspace_manager.rs:1249-1258` has an arm for exactly that —
so whether the sampler is racing that arm or hitting a different empty-gitdir path is the question,
and it is not answered here.

**Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.**

#### `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`

**This row exists because the sightings were being disclosed as if each were isolated, and that is
exactly the shape that let `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` reach four instances before
anyone counted it.** Name the class, count it, and stop re-deriving it per packet.

**Members, all observed 2026-09-03:**

| member | platform | surface |
|---|---|---|
| `W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT` | macOS | `runner::host::tests::every_role_reaches_the_containment_points_of_this_platform`, the pre-exec process-group step |
| `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT` — already in §2 | Windows / winguest | two `engine::topology::settle` kill tests, predicted-region trailing slash |
| `PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT` | Windows / winguest | **the same two** settle tests, `the log replays: AlreadyStarted` |
| `PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT` | Linux / ubuntu | `workspace_manager` residue-and-kill test, empty gitdir |

**Four members, four distinct fingerprints, three platforms, three subsystems.**

**On the count, because it has already been stated wrongly.** An earlier statement of this class said
*five* fingerprints and listed a fifth macOS member. That member was withdrawn: it is
`W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM`, a pre-fix instance, and the discriminator section above
records how it was misfiled. **The count here is derived from the members named in the table and
nowhere else**, which is the property that matters — a class row whose count is arithmetic on a
withdrawn member is the same defect it exists to prevent.

**What is established.**

- Every member is **intermittent**: in each case the identical commit produced both green and red
  runs, and for three of the four the green and the red are named above.
- Every member sits in a **subprocess kill, settle, or residue** path.
- It spans **all three CI platforms**, so it is not one bad runner.
- It is **not caused by any one packet**: E, M1, M2, M3, M4, M5 and M6 have each shown a member on
  different branches; one member's red-then-red pair differs from its green by a markdown file; and
  **the macOS member fires on `master` three times and is 33.4 hours older (UTC) than W2's base
  commit `1cbdccd`**.

**Every per-member count in the source records was an undercount, and re-deriving the population is
what showed it.** The population is every failing `test` job of every CI run on `master` and the
eight W1/W2 branches, **enumerated per attempt** and confirmed per job by a `... FAILED` line:

| member | its own record said | the population says |
|---|---|---|
| `W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT` | two, then three, then four sightings | **twelve**, six branches, 2026-09-01 → 09-03, three on `master` |
| `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT` | one run, then a second sighting | **three** — a third at `27e905e` on `w2-m3-rundir`, run `33757851135` |
| `PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT` | one | **one**, confirmed |
| `PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT` | one | **one**, confirmed |

**Three separate instrument failures produced the undercounts, and they compound.** Sightings were
collected as packets reported them rather than by enumerating a population, so `master`'s were never
in scope. The **run** was the unit rather than the **attempt**, so a rerun-in-place that failed again
was counted once. And an entry that grew by addenda — "two instances", then "a third sighting" — was
never re-totalled, so its prose and its own table disagreed. **A count maintained by addendum is not
a measurement**, and that is the argument for this row rather than for four independent ones.

**It is not the PR #115 repair.** M4's macOS failure at `c30aca0` **predates** #115 entirely — that
branch had not base-merged it when the failure happened. A fix cannot cause a failure that occurred
before it landed.

**Two corrections that produced this row, recorded because the reasoning outlives the conclusion.** A
branch-specific hypothesis was built on (a) *"the only failures in the last thirty runs are on one
head"*, which was false — a run at `c30aca0` failed inside that window and the query missed it — and
(b) a `readiness.rs` change as a candidate mechanism, when that change is a **doc comment only** and
cannot alter runtime behaviour. **A sample was claimed to cover more than it did, and a diff was
cited without being read.** Catching both is what turned a wrong branch-level accusation into a
programme-level observation.

**The lead, recorded as a hypothesis and asserted nowhere.** PR #115 established that a stalled
launch gate can refuse every waiter at once, and every member sits in a path that spawns or reaps
subprocesses. **Whether one launch-gate or reaper mechanism underlies the class is untested**, and
the evidence does not reach it: #115's observed effect is macOS-specific, while three of the four
members are Windows and Linux. This is the hypothesis to test next, not a finding to disclose in any
pull-request body.

**Why the class is worth a row of its own.** A merge queue cannot drain against failures at this
rate — every packet that goes green re-rolls the dice on its next push, and every push is followed by
one. That is an operational fact about the queue, not only a property of four tests, and it is
invisible from inside any single packet's disclosure.

**Disposition.** Open, cause unknown. Packets disclose their own sighting with its own evidence
**and name this row**, so a reader sees the class rather than four unrelated traps. Repairing one
member does not close this row.

#### `W2-RETIRED-DECISIONS-PATHS-CITED-AND-MISSING`

**What.** PR #116 retired the `decisions/` directory. Every citation of a file in it now names a
path that does not exist — in documentation, in CI scripts, in configuration, and **in production
source**.

**Measured at `3af9696` over the tracked tree, by two engines that agree — and measured with this
section excluded, which has to be said or the figure does not reproduce:**

    $ git ls-files -z | xargs -0 /usr/bin/grep -ohP 'decisions/[A-Za-z0-9._-]+\.md' \
        | sort | uniq -c | sort -rn
    # 24 distinct paths, 168 occurrences; `decisions/` is not a directory in the tree

**Run that command against the file you are reading and it returns 25 and 173, not 24 and 168.** The
difference is this section: it names four dead paths as examples, five times between them, and one of
those — `decisions/2026-09-01-clean-base-merge-keeps-review.md` — is cited nowhere else in the tree,
so **recording the finding created a twenty-fifth dangling path.** The figures below are the
repository's, taken with `reviews/FINDINGS.md` at `3af9696`; including this section the same command
returns 25 / 25 / 173 across the same 53 files.

| | |
|---|---:|
| distinct `decisions/*.md` paths cited | **24** |
| of those, missing from the tree | **24** — all of them |
| total citation occurrences | **168** |
| files carrying at least one | **53** |

By file type: 131 in `.md`, **26 in `.rs`**, 4 in `.toml`, 4 in `.yml`, 3 in `.sh`. The heaviest
single path is `decisions/README.md` at 32 occurrences, then
`decisions/2026-08-26-durable-retry-feedback.md` at 23 and
`decisions/2026-08-12-merge-queue-execution-topology.md` at 22. The `.rs` citations are spread over
twenty files including `src/engine/classify.rs`, `src/engine/topology/run.rs`,
`src/topology/effects/sites.rs` and `src/topology/fold/check_attempt.rs`; `effects/allowlist.toml`
and `upstroke.toml` carry two each.

**No gate catches it.** `test-docs-consistency.sh` passes at `ac16fff`, at `ae2a58f` and at
`3af9696`. Nothing in
the repository resolves a cited repository path.

**The rules themselves survive; it is the citations that died — and that distinction was verified
rather than assumed.** The clean-base merge rule this programme relies on to retain reviews across
base merge-ins lived in `decisions/2026-09-01-clean-base-merge-keeps-review.md`, which is gone; the
rule is restated in `DESIGN.md` and `.github/pull_request_template.md`, so it is live. A rule cited
to a deleted file is exactly the kind of authority that evaporates on inspection, so it was checked
before being relied on.

**This is a repository-scale instance of a class this programme met three times in one day at small
scale**, and it is the **deletion** form of it: a change invalidates prose in files it does not
touch, and **a deletion invalidates every reference to what it deleted, including references in code
comments nobody thinks of as documentation.**

**Why it is open.** Not any packet's to repair — a packet fixes the citations in its own body and no
more. The repository-scale repair is its own change with its own review, and the durable fix is a
gate that resolves cited repository paths; without one, the class recurs on the next directory
retirement, which is how it arrived.

**And that gate has a requirement this row can state precisely, because this row would be its first
finding.** A naive path-resolving gate flags all five citations above — every one a *deliberate
naming of a dead path*, which is what a finding about dead paths is made of. So the gate needs to
distinguish a live reference from a mention, and the cheapest form that does not invite abuse is to
exempt a path inside a fenced block or introduced as an example, rather than to exempt a file.
**An unimplementable repair is not a disposition**, and "resolve every cited path" is unimplementable
until that distinction exists.

### The row that goes to §5 (Fixed), not §2

#### `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM`

**What was observed, for a year of reading it wrongly compressed into a day.** On macOS CI the test
binary died with `(signal: 15, SIGTERM: termination signal)` and no diagnostic, and the death landed
on whichever tests were mid-flight — so the failure was attributed to innocent tests and read as a
flake in a different subsystem each time.

**Five instances, on four pull requests that touch none of the named subsystems.** Two of the five
live only in attempt 1 of a rerun-in-place, which the conclusion query hides:

| # | PR / head | run | what the log says |
|---|---|---|---|
| 1 | #97 `9807f48` | `33674393240` att. 1 | `kill_tree_settles_the_whole_unix_group_before_it_returns` asserted, `failures:` present, `1798 passed; 1 failed`, no `signal:` line — **an assertion flake, harness alive** |
| 2 | #103 `4517caa` | `33691549623` att. 1 | `signal: 15`, a burst of `FAILED` inside milliseconds, 0 `failures:`, 0 `test result:`, 46 orphans — **harness self-kill** |
| 3 | #104 `ae59f2d` | `33741105025` att. 1 | identical signature to (1) — **an assertion flake**, and it was misfiled as the same shape as (2) |
| 4 | #108 `5b67179` | `33763282946` att. 1 | `signal: 15`, 0 `test result:`, 44 orphans, `engine::tests` in flight — **harness self-kill** |
| 5 | #104 `94f8c27` | `33773356014` att. 1 | `signal: 15`, 0 `failures:`, 0 `test result:`, 28 named `FAILED` inside one second, 46 orphans — **harness self-kill** |

**Instance 5 spent a day filed under a category of its own**, on the reasoning the discriminator
section above records; it is folded back here, and the invented category is withdrawn.

**The hypothesis this row carried for a day, and why it was wrong.** The observations were read as a
**group kill** reaching the harness's own process group. It is refuted: no kill path in the tree
signals a group that can include the harness — every group id is the pid of a child the tree itself
made a leader — and cargo, in the same process group, survived both deaths and printed its error,
which excludes group delivery outright. **The ID this row carried named that mechanism, and an ID
that asserts a mechanism is a claim like any other.**

**What actually happens.** The harness's own signal supervisor arms a **process-wide `SIGTERM`**
when a freshly forked cleanup reaper has not said READY within 2 s and then does not acknowledge
CANCEL within a further 2 s; the monitor thread re-raises it. Every `runner::container::exec::tests`
fixture runs `git` through the host runner, and every host-runner spawn enters one process-wide
launch gate, so a stalled launch froze the whole module for about five seconds and the arm refused
every waiter in the same tick. **That is why a named burst with no summary is the signature**: each
refused waiter panics fast and libtest prints its `FAILED` line, and the signal arrives before
libtest reaches its summary. The tests that passed through the burst are exactly the ones that build
no fixture.

**Confidence, stated rather than implied**: self-kill through the supervisor, high; the
READY-timeout site specifically, about 7 in 10; the Darwin cost behind the slow reaper — a per-fd
`close` loop, FIFO-backed pipes, ten or more test threads on a documented 3-vCPU runner — about even,
and unmeasured.

**The counterfactual nobody can argue with.** #108's `1041e3d` — the identical fold module, the
identical census repoints — was green on macOS with all eleven check-runs successful; the only delta
to the SIGTERMed `5b67179` is **four comment lines**, and filtering that diff to non-comment lines
yields zero. "Caused by the diff" is not sustainable in any form.

**One repeated number that is not the lead it looks like.** 44 and 46 orphaned self-copies recurred
across unrelated pull requests, which reads as a fixed number the kill tests spawn. It is not, and the
control is a **green** run rather than an argument: run `33780942121` at `741364b` — the repair's own
green run, `1800 passed; 0 failed` — reaps **54**, more than either red run, and so does instance 1,
which is an assertion flake with the harness alive. They are the same adjacent-pid pairs throughout:
fork-only guard-and-probe helpers that outlive their parent on Darwin, which ubuntu does not produce.
**The red runs have fewer only because they died early.** The orphans are the population waiting on a
frozen launch gate, not a signature — and a count that is *higher* on the green run is the reading
that settles it.

*Counts in this row were read from local copies of the job logs rather than taken from a report*, with
`gh run view <run> --attempt 1 --job <id> --log` and a `grep -oP '\S+::\S+ \.\.\. FAILED'` for the
named failures, because a bare `grep -c FAILED` also counts a test whose own name contains `T-FAILED`.

**The fix.** PR #115, merged at `046f17d`, one file, three changes in the termination module:

1. the READY-timeout path kills and reaps the late reaper and fails that launch with an ordinary
   `Err` — no agent exists and no group is registered at that point, so there was nothing to fail
   closed about;
2. `Reaper::cancel` reads until `OK` or EOF instead of judging the first byte;
3. **every arm site writes one async-signal-safe line to fd 2 naming itself**, so the next occurrence
   is evidence rather than an absence.

At `ae2a58f` — after M6 split `src/agent/proc.rs` — `arm_fail_closed_termination` is defined at
`src/agent/proc.rs:2076` with five arm sites at `:1671`, `:2010`, `:2131`, `:2173` and `:2295`.

**Guard.** `agent::proc::tests::a_late_reaper_fails_its_launch_without_arming_termination`
(`src/agent/proc.rs:4635`), driven through the subprocess helper at `:4562`. **Mutation witnesses:
reverting either behavioural change fails it.** The eight-command baseline was ALL 8 PASS at
`741364b`, and CI run `33780942121` was green on every leg with macOS reporting `1800 passed; 0
failed` in 169 s.

**What one green run does and does not prove.** One green macOS run is consistent with the fix and
with luck alike. **The evidence that counts is the shape staying absent, and the fd-2 line if it ever
returns** — which is the third change's whole purpose, and the reason this row can be closed without
closing the question.

**Recurrence, stated so a later reviewer can use this row for what §5 is for.** A macOS death
carrying `upstroke: fail-closed SIGTERM armed:` on fd 2 is a **recurrence of this row**. A macOS
death without it is something new, and it should get its own ID rather than this one.

**A false-green check, because a repair to a signal path invites the question.** The defect only
turns green→red at the job level; it cannot produce a passing job from a failing tree.

**Census.** The C-004 investigation put the macOS red rate over 2026-08-30 → 09-03 at **18 red
`test` jobs of about 269**, of which two are this shape; the other sixteen are four timing signatures
with `failures:` sections, and **they are not fixed by #115**. That census's artifacts are
box-local and are cited here as the investigation's measurement rather than re-derived; the durable
evidence for every instance above is its run id.

### What this append deliberately does not carry

**Two findings were withdrawn at source and neither has a row here. Both withdrawals are recorded
rather than tidied away, because in each case the reasoning is the reusable part.**

- **The `Corpus` scaffolding findings (three of them).** They were correct when written, against
  test scaffolding PR #104's repair rounds built. **That scaffolding no longer exists**: owner ruling
  7 reverted `src/validate.rs` to `origin/master` entirely and deleted every helper those rounds
  added. The defects went with the code. They must not be re-derived from the review records, which
  still discuss them — **text describing code that does not exist is the failure mode this whole
  append is written against**, and it has bitten twice already: once in
  `PR103-CONTAINER-SUBSTRATE-LIST-CHECKS-NAME-ONLY`'s withdrawn comparison to a list that never
  landed, and once here.
- **The macOS `runner::container::exec` module fingerprint.** It was never a distinct signature. It
  is `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM`, instance 5, and the discriminator section above
  records both the misfiling and the two artefact-of-the-mechanism discriminators that produced it.
  **A category invented for a residue propagates**: this one reached six working sessions, a merged
  pull-request body, and a merge-gate count before it was caught, and a merged body cannot be
  rewritten — so one disclosure in the repository's history names this instance under an ID that
  does not exist. That is the cost of the error and it is recorded here because it is the only place
  it now can be.

**One finding is a member of this append's class row and is not added by it.**
`PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT` landed in `1f30851` and reached `master` via `079a346`;
it sits in §2 already, once. It is named as a member of
`CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES` and nothing about it is edited here.
**It has two further sightings, and they are new information recorded in the class row rather than in
its own row**, since a landed row is not this append's to edit. Both carry the identical signature —
the same two `engine::topology::settle` tests, the same `MalformedEntry { kind: "task_dispatched" }`,
the same `src/aleph/` against `src/aleph` mismatch:

- run `33781252888`, `test (winguest)` at `6d8cdda` on PR #106, `1761 passed; 2 failed; 35 ignored`;
- run `33757851135`, `test (winguest)` at `27e905e` on PR #107 — **found by enumerating the
  population rather than by anybody meeting it**, and recorded nowhere before this append.

So that row describes **three** runs, not the one it was written for. **The row's own wording
anticipated exactly this**: it says the rate is unmeasured and names the measurement that would
settle it, rather than calling one run a flake. Further sightings do not change its disposition; they
raise the priority of the measurement it names, and three sightings on three branches make it a
member of the class row above rather than a property of any packet.

**And one row already in §2 is not this append's, though four of these rows are instances of its
shape.** `CLASS-GATE-STATED-DOMAIN-EXCEEDS-COUNTED-DOMAIN` landed with #106. The domain rows here
cross-reference it; none of them restates it.
## 44. PR #119 hooks.rs sweep (2026-09-03)

Append-only. The §6/§7 sweep of `src/workspace_manager/hooks.rs` (PR #119) took three frontier
passes: `a1b319c` (three findings), `6a13e1d` after the finding-2 repair (six findings), and
`ac466e9` after the second repair and a base merge-in (six findings). Every verdict was
`CHANGES_REQUIRED`, none P1. A fourth pass, on `43a9acd`, was the pull request's one allowed
extra pass and returned `CHANGES_REQUIRED` with three unlabelled findings the coordinator classed
P2; the head that repairs them is not re-reviewed and merges as a repair-only delta disclosed in
the body and verified by the coordinator under the owner's delegation of 2026-09-04. The pass-1
and pass-2 rows are `fixed` under the owner's direction
of the time that a file's refinement pass fixes every finding, except the sweep queue, which
PR #122 repairs. The pass-3 rows follow the owner's amendment 1 of 2026-09-04: on the sweep pull
requests P1 and P2 findings are fixed and P3 and lower are recorded. The coordinator classed
findings 1 and 3 P2, fixed in code; finding 2 is fixed by PR #122 through the base merge-in; and
findings 4 to 6, body and ledger text, are corrected as text and recorded `fixed` with the docs
commit. The macOS exit-budget observation this pull request also produced stays where it was
filed, in §2 as `PR119-MACOS-PROC-SUSPEND-CONTINUE-EXIT-BUDGET`.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR119-SWEEP-QUEUE-STALE-AT-HEAD | P3 | a1b319c9090c2b26df96c3671798d4b154a9ee9a / standards/SWEEP.md:39 | the queue says split files are queued when their split merges -> the head still calls #106, #108 and #111 open while their merges are ancestors of it -> their child files and parents are absent from the queue -> a maintainer following it leaves their untouched §6/§7 sites under the activation-rule exemption | introduced_by_feature | docs-contract | ea25bc8 | repaired by PR #122, which updates the queue after the three sibling sweeps land so one branch edits the table at a time | deferred |
| PR119-AFTER-REFUSAL-RATIONALE-OVERCLAIMS-DURABILITY | P3 | a1b319c9090c2b26df96c3671798d4b154a9ee9a / src/workspace_manager/hooks.rs:206 | the funnel doc justified `apply(After)?` by "the effect is durable" -> `verify_worktree` performs no effect -> Proceed at Before and Error at After on it -> the funnel refuses and nothing became durable, and `InjectionMode::ErrorReturn` promises only performed or partially performed | introduced_by_feature | docs-contract | f58747a | fixed at 6a13e1db68e4e1946455b92de10983b7c57596b7: the bullet states the mode's contract and what the caller acts on; `a_refusal_at_after_is_returned_after_the_primitive_ran` drives Proceed-then-Error on an effect-free primitive | fixed |
| PR119-SCOPE-STATEMENT-OMITS-STACKED-FILES | P3 | a1b319c9090c2b26df96c3671798d4b154a9ee9a / AGENTS.md:1 | the body's scope and rollback named two paths -> the diff against master adds `AGENTS.md` and appends to `reviews/FINDINGS.md` -> reverting the merge reverts four paths | introduced_by_feature | docs-contract | ea25bc8 and 2a9e330 | documented guard: the body carries one list of the paths the diff touches and every scope and rollback statement uses it; recurred as `PR119-SCOPE-ACCOUNTING-CONTRADICTORY` below because the first correction left the rollback sentence unchanged | fixed |
| PR119-LOCK-POISON-JUSTIFICATION-FALSE | P2 | 6a13e1db68e4e1946455b92de10983b7c57596b7 / src/workspace_manager/hooks.rs:144 | the `phase` doc said each hook call is one append and holders only append or read, and `phase` recovered a poisoned guard -> `HookHarness::hook` writes the open fast sequence, the reached points and the observed phases, and holders arm, disarm and open or close sequences -> a worker panics while holding the harness with a sequence open -> `phase` records the next hook into the abandoned sequence and the suite reads false coverage evidence | introduced_by_feature | correctness | f58747a | fixed at 5b8979b6cd54687b8f05f14ad1f022ed2a635f24: a poisoned harness answers `Injection::Error` and the funnel refuses; the §6/§10 paragraph names the state the lock protects; `a_poisoned_harness_refuses_rather_than_recording_into_it` poisons the harness from a thread that panics while holding it and was witnessed failing under silent recovery | fixed |
| PR119-SWEEP-QUEUE-STALE-RECURRED | P3 | 6a13e1db68e4e1946455b92de10983b7c57596b7 / standards/SWEEP.md:39 | the queue is unchanged from the first pass -> the same four merged splits are still called open -> the pull request body said every finding was fixed while this row said deferred | introduced_by_feature | docs-contract | PR119-SWEEP-QUEUE-STALE-AT-HEAD | repaired by PR #122; the body now says exactly which finding is deferred and where it is being fixed | deferred |
| PR119-CLONE-TEST-DOES-NOT-TEST-SHARING | P3 | 6a13e1db68e4e1946455b92de10983b7c57596b7 / src/workspace_manager/hooks.rs:449 | the test cloned the observer and dropped the clone at once, then read the harness while the original lived -> a `Clone` producing an independent harness and ledger passes it -> the §6 claim "cloning shares both" had no guard | introduced_by_feature | correctness | f58747a | fixed at 5b8979b6cd54687b8f05f14ad1f022ed2a635f24: `a_clone_shares_the_harness_and_the_ledger_and_the_harness_outlives_every_observer` records through the clone after the original is dropped, reads the original's ledger handle through the clone, and reads the harness after every observer is gone; witnessed failing under an independent-clone impl | fixed |
| PR119-POINT-TEST-FIRST-WINS-UNPROVEN | P3 | 6a13e1db68e4e1946455b92de10983b7c57596b7 / src/workspace_manager/hooks.rs:261 | the test drove Proceed then one Error -> a `point` that lets the last non-Proceed answer win passes it -> an observer refusing at both modes would be reported at `/error-return` instead of the first declared `/kill` | introduced_by_feature | correctness | f58747a | fixed at 5b8979b6cd54687b8f05f14ad1f022ed2a635f24: `a_point_consults_every_mode_and_applies_the_refusal_at_the_mode_that_answered` refuses at both modes and asserts `/kill`; witnessed failing under the last-answer-wins mutation | fixed |
| PR119-SCOPE-ACCOUNTING-CONTRADICTORY | P3 | 6a13e1db68e4e1946455b92de10983b7c57596b7 / AGENTS.md:1 | the body named four paths in one place and two in its rollback -> its ledger said Scope and Risk were corrected -> its evidence said the findings were in `reviews/FINDINGS.md` §43 to §45, which the head does not have | introduced_by_feature | docs-contract | PR119-SCOPE-STATEMENT-OMITS-STACKED-FILES | documented guard: one path list used everywhere the body describes scope or rollback, the §43 to §45 claim removed, and this section is where the findings are recorded | fixed |
| PR119-MACOS-REACHABILITY-OVERCLAIM | P3 | 6a13e1db68e4e1946455b92de10983b7c57596b7 / reviews/FINDINGS.md:179 | the body and the §2 row said nothing in the diff can reach the macOS timing failure -> the diff adds seven tests to the same executable and can alter scheduling -> a pass and a failure at one SHA prove nondeterminism, not absence of a defect at that head | introduced_by_feature | docs-contract | 2a9e330 | documented guard: both now say the failing test is not one the diff touches, that one pass and one failure at one SHA show nondeterminism, and that the runner-load cause is fixed in PR #125 | fixed |
| PR119-POISON-ANSWER-INVENTS-POINT-CONTRACT | P2 | ac466e9eb0259908252eb036a674853fcb211f8e / src/workspace_manager/hooks.rs:166 | a poisoned harness answered `Injection::Error` at every coordinate -> `candidate_commit_tree` passes `Before`, another holder panics while `git commit-tree` runs, the object is written -> `point(IdUnread)` is consulted at its `Kill` coordinate, the only mode it declares -> the funnel returns `Refused` naming `/kill` on a process still alive, inventing an error-return contract the design tables no recovery for, and the message reads as an injected fault rather than harness corruption | introduced_by_feature | correctness | 5b8979b6cd54687b8f05f14ad1f022ed2a635f24 | fixed at c26553658a7c9d6a8831b242de2d242bfa237698: the observer refuses wherever a refusal is legal (`Before`, `After`, a point consulted in `ErrorReturn` mode), proceeds without recording at a point whose only legal mode is `Kill`, remembers the poison through `poisoned`, and the refusal is worded "harness poisoned" through `refusal_cause` and `consult`; `a_harness_poisoned_mid_funnel_proceeds_at_a_kill_only_point_and_refuses_where_it_may` drives all three coordinates and was witnessed failing under `Error` at the `Kill` point and under the injected-fault wording | fixed |
| PR119-AGENTS-MD-CONTRADICTS-BUILD-BOX-RULE | P3 | ac466e9eb0259908252eb036a674853fcb211f8e / AGENTS.md:48 | the stacked commit's `AGENTS.md` told a Codex session to run bare `cargo` commands and to set per-run target directories -> the box rule is `upstroke-build` and never a hand-set `CARGO_TARGET_DIR` -> two bare suites share the fixed container pre-clean key and one deletes the other's live containers | introduced_by_feature | docs-contract | ea25bc8 | fixed by PR #122 at 637b8ae7b56ae6fc0b9e515bbb222899ad3f7f24 (the mirror's four false sentences), merged as 20a23d08dfcedefabc1eb710b5bc6668f2f24a72; the merge-in 5296269749fe1140f5f79c69e38b177fc6382f4a makes this branch's `AGENTS.md` identical to master's, so it leaves the diff | fixed |
| PR119-POISON-TEST-DOES-NOT-OBSERVE-HARNESS | P3 | ac466e9eb0259908252eb036a674853fcb211f8e / src/workspace_manager/hooks.rs:455 | the poison test asserted the refusal and the message and never read the harness -> a `phase` that recovers the guard, records into the abandoned sequence and then answers `Error` passes it -> the ledger's guard and witness claims were stronger than the test | introduced_by_feature | correctness | 5b8979b6cd54687b8f05f14ad1f022ed2a635f24 | fixed at c26553658a7c9d6a8831b242de2d242bfa237698: `a_poisoned_harness_refuses_rather_than_recording_into_it` asserts, on the harness itself, that the refused coordinate's count is unchanged, nothing executed, nothing was reached, and the fast sequence the poisoner left open has no site; witnessed failing under record-then-refuse | fixed |
| PR119-BODY-BEHAVIOUR-CHANGE-UNDERSTATED | P3 | ac466e9eb0259908252eb036a674853fcb211f8e / src/workspace_manager/hooks.rs:166 | the body's Summary said poison recovery was kept, its Scope said poison refuses, and its Risk called the point message the one behaviour change -> poison at `Before` now prevents an operation that previously ran and at `After` turns a completed one into `Err` -> the stated risk analysis omitted a second substantive change | introduced_by_feature | docs-contract | 5b8979b6cd54687b8f05f14ad1f022ed2a635f24 | documented guard: one statement of both behaviour changes, used in the body's Summary, Scope and Risk alike, with what each means for a caller | fixed |
| PR119-MACOS-ROW-CLAIMS-PR125-REPAIR | P3 | ac466e9eb0259908252eb036a674853fcb211f8e / reviews/FINDINGS.md:197 | the §2 row said PR #125 fixes the runner-load cause -> #125 changes only the startup `READY` waits, not the post-`SIGCONT` exit wait the failure is in -> the row also called one red and one green "not a defect in the head", which §12 says a pass and a failure at one SHA cannot show | introduced_by_feature | docs-contract | a68d16c7b24e0fd1a34bc51d33fd95e51033e8d8 | the §2 row is rewritten: the failure is the test's own exit budget on a loaded runner, nothing in this pull request or #125 addresses it, it is deferred with the row as its guard and the budget owed a load-tolerant fix, and it is nondeterministic on that runner with the cause not established | fixed |
| PR119-LOCAL-DOCKER-FAILURE-MISATTRIBUTED | P3 | ac466e9eb0259908252eb036a674853fcb211f8e / src/runner/container/exec/tests.rs:5381 | the body blamed a local red on concurrent eight-command runs sharing container names -> the box's builds are slot-scoped and their container names carry the slot, so they cannot collide that way -> the failing test finds residue by fixed `/proc` markers, so one suite can mistake another suite's live process for residue, and the body neither named that mechanism nor stopped at "plausible" | introduced_by_feature | docs-contract | ac466e9eb0259908252eb036a674853fcb211f8e | documented guard: the body paragraph states the fixed-marker mechanism as the plausible one and not as established, and the later green run as showing only that it did not recur | fixed |
| PR119-POISON-INVARIANT-CLAIMED-FOR-SHARED-HARNESS | P2 | 43a9acdcdc0227e2daa2d69f32d73bc68147fa84 / src/workspace_manager/hooks.rs:194 | the doc and the body say "a poisoned harness is never recorded into" for the shared `HookHarness` -> only `HarnessEffects` honours it, while `HarnessEventHooks` (`src/events/log.rs:298` at that head) and the run-directory, container and spawn adapters recover a poisoned guard and call `hook` -> open a fast sequence while holding the shared mutex, panic, then drive an Event-family phase -> that adapter records into the abandoned sequence and manufactures the false coverage evidence the claim says is prevented | pre_existing | correctness | 7a83e69 (the adapters' `unwrap_or_else(PoisonError::into_inner)` shape predates this pull request) | the claim is fixed at 0d09ca4df07728d55fcb637e2508cd4145aaf1d2 to what holds, `HarnessEffects` alone, and the type's doc names the four adapters that still recover and record; the adapters themselves are deferred to the sweeps of their files, `src/rundir.rs`, `src/events/log.rs`, `src/runner/container.rs` and `src/runner/mod.rs`, which are not edited here | deferred |
| PR119-POISON-CAUSE-LOST-THROUGH-FORWARDING-WRAPPERS | P2 | 43a9acdcdc0227e2daa2d69f32d73bc68147fa84 / src/workspace_manager/hooks.rs:287 | `consult` asks `refusal_cause` on the outer trait object and the method was defaulted to `None` -> a wrapper that forwards `phase` and the ledger to an inner `HarnessEffects` but not the new method (`ArmedEffects` in `src/engine/topology/scaffold.rs:418` and `src/engine/topology/candidate/tests.rs`, `TracedEffects` in `src/engine/topology/recover/tests.rs`, `LedgerAtAdd` in `src/workspace_manager/tests.rs`) returns the inner `Error` with no cause -> the message says the funnel "was made to fail", the armed-fault wording the pull request promised never appears, and nothing forced the wrappers to be updated | introduced_by_feature | correctness | c26553658a7c9d6a8831b242de2d242bfa237698 | fixed at 0d09ca4df07728d55fcb637e2508cd4145aaf1d2: the default is removed so the compiler reaches every implementation, the four wrappers forward to their inner observer and the stateless doubles answer `None`; `a_forwarding_observer_reports_the_inner_poison_as_poison` poisons the inner harness and drives a funnel through a forwarding wrapper with no fault armed, and was witnessed failing under the wrapper answering `None` as the default did | fixed |
| PR119-POISON-LATCH-IGNORED-AFTER-CLEAR-POISON | P2 | 43a9acdcdc0227e2daa2d69f32d73bc68147fa84 / src/workspace_manager/hooks.rs:221 | `phase` checks only the current `lock()` result and ignores the latched `poisoned` flag, while `harness()` exposes the raw mutex -> poison the harness with an open fast sequence, let the observer see it, call `Mutex::clear_poison` through the handle, call `phase` again -> the lock succeeds and `hook` records into the abandoned sequence while `poisoned()` still says true and the doc says nothing more is recorded | introduced_by_feature | correctness | c26553658a7c9d6a8831b242de2d242bfa237698 | fixed at 0d09ca4df07728d55fcb637e2508cd4145aaf1d2: `phase` consults the latch before the lock and answers as for poison whenever it is set, and `harness()`'s doc says the accessor does not reset the latch; `clearing_the_poison_does_not_reopen_recording` poisons with an open sequence, observes, clears, and asserts nothing recorded at `Before`, at a `Kill`-only point or at `After`, and was witnessed failing under the lock-only shape | fixed |


## 45. PR #118 naming.rs sweep (2026-09-03)

Append-only. The §6/§7 sweep of `src/workspace_manager/naming.rs` (row 3 of the
`standards/SWEEP.md` review queue) went through four frontier passes (gpt-5.6-sol at
max): six findings at `9f83b09`, five at `8d25472`, four at `cff812d`, five at `3482ba1`, none P1. The owner's direction for
the file's refinement pass was that every finding is fixed; the one exception is the
stale queue, which is another pull request's. Each row names its introducing commit
and, when fixed, the fixing commit and the test that guards it. The third pass
(four findings at `cff812d`) renamed two identifiers the second pass's rows cite; those rows
say so in place, so every backticked guard resolves at the head.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR118-TEST-CLONES-A-SLOT-TO-KEEP-IT | P2 | 9f83b094d06c9b46c094c5ff847783d2dac1a52b / src/workspace_manager/naming.rs:339 | the round-trip test calls `slot.clone()` only so `slot` survives for the next assertion -> `Slot` owns a `String`, a non-trivial clone taken to satisfy ownership -> the body and the swept-table row say the file has no clone call, false at the head | introduced_by_feature | docs-contract | 51feba7 | a7b7c98: `every_slot_shape_survives_the_intent_name_round_trip` compares borrowed values | fixed |
| PR118-RECORD-SCHEMA-PIN-TESTS-ONE-FIELD | P2 | 9f83b094d06c9b46c094c5ff847783d2dac1a52b / src/workspace_manager/naming.rs:447 | `the_intent_record_schema_is_pinned` drops `incarnation` only, then asserts no field has a default -> `#[serde(default)]` on `kind`, `slot` or `run_id` -> every assertion stays green while the missing field is silently accepted | introduced_by_feature | correctness | 51feba7 | a7b7c98: `the_intent_record_schema_is_pinned` derives the field list from the serialized record and drops each field on its own; the mutation was run per field and fails on each | fixed |
| PR118-RECLAIM-REGRESSION-PINNED-AT-PARSER-ONLY | P2 | 9f83b094d06c9b46c094c5ff847783d2dac1a52b / src/workspace_manager.rs:855 | the destructive case composes through `intents()` and `reclaim_intents()` -> the witness asserts only that `from_intent_name` returns `None` -> create `tasks.kalpha-g3.intent` with its worktree, add `tasks.kalpha-g03.intent`, reclaim -> before the fix the legitimate slot is removed and `g03` survives; the head refuses first, and no test pins that | introduced_by_feature | crash-consistency | 51feba7 (the witness); the composition is 7a83e69's | a7b7c98: `reclaim_refuses_a_non_canonical_intent_name_before_removing_anything` in `src/workspace_manager/tests.rs`; with the round-trip guard removed it fails | fixed |
| PR118-QUESTION-MARK-COUNT-WRONG-IN-DISPOSITION | P3 | 9f83b094d06c9b46c094c5ff847783d2dac1a52b / src/workspace_manager/naming.rs:242 | the disposition counts two `?` sites in `from_intent_name` -> the head has five, two `parse().ok()?` and a new `strip_prefix(..)?` among them -> the swept-table row records a §7 accounting that does not match the file | introduced_by_feature | docs-contract | 51feba7 | a7b7c98: the doc comment on `from_intent_name` dispositions all five and the `then_some` exit; 8d25472: the swept-table row | fixed |
| PR118-SCOPE-STATEMENT-OMITS-STACKED-FILES | P3 | 9f83b094d06c9b46c094c5ff847783d2dac1a52b / AGENTS.md:1 | the body says the caller edit is the only change outside `naming.rs` and `SWEEP.md` -> the exact diff carries the stacked commit's `AGENTS.md` and queue section -> the Included scope does not match the diff | introduced_by_feature | docs-contract | ea25bc8 | the body's Scope section, corrected 2026-09-03, carries the exact diff stat at each head; the merge of PR #122 removes the stacked files from the diff | fixed |
| PR118-TEST-USES-LOSSY-PATH-AS-IDENTITY | P3 | 9f83b094d06c9b46c094c5ff847783d2dac1a52b / src/workspace_manager/naming.rs:489 | the `component()` helper feeds `to_string_lossy()` into the expected intent filename -> a lossy string is an identity oracle, against §8 -> a non-UTF-8 fixture would compare replacement characters as equal | introduced_by_feature | portability | 51feba7 | a7b7c98: `component()` uses a checked `to_str()` and fails naming the premise | fixed |
| PR118-R2-TEST-CLONES-KEYS | P2 | 8d2547234f27ce4d8a177e7c4b96f7e565ff3e6e / src/workspace_manager/naming.rs:495 | `the_intent_record_schema_is_pinned` takes `object().keys().cloned()` -> every key `String` is cloned to satisfy a borrow -> the swept-table row and the body say the file has no clone call, false at the head | introduced_by_feature | docs-contract | a7b7c98 | bc07f05: the map is consumed for its keys, `Slot::parts` borrows the snapshot name through a `Cow`, and the file has no `.clone()` or `.cloned()`; the swept-table row says so | fixed |
| PR118-R2-SCHEMA-PIN-OVERCLAIMS | P2 | 8d2547234f27ce4d8a177e7c4b96f7e565ff3e6e / src/workspace_manager/naming.rs:469 | `#[serde(alias = "legacy_kind")]` on `kind` -> every assertion passes while `legacy_kind` is accepted; `kind: "bogus"` and `slot: "../../outside"` deserialize because every field is an unconstrained `String` -> "the schema is pinned" is wider than the evidence | introduced_by_feature | correctness | a7b7c98 | bc07f05: `the_intent_record_schema_is_pinned` reads the four names back and refuses each field under three other names; `kind` is `IntentKind` and `the_record_kind_is_one_of_three_words` refuses `bogus`; `slot` is `SlotId` (named `SlotPath` until the third pass) and `the_record_slot_is_refused_on_read_outside_its_grammar` refuses `..`, a leading `/`, a backslash and an empty component; both mutations were run and fail | fixed |
| PR118-R2-RECORD-SLOT-VIA-LOSSY-PATH | P2 | 8d2547234f27ce4d8a177e7c4b96f7e565ff3e6e / src/workspace_manager.rs:810 | `IntentRecord::slot` is a `String` built by `to_string_lossy().replace('\\', "/")` -> an OS path is rendered lossily into a persisted record, against §8's path rule, and the exact-schema test freezes that -> no documented exception | pre_existing | portability | 7a83e69 (the write site); 51feba7 froze it | bc07f05: `Slot::id` (named `git_path` until the third pass) builds the record's `SlotId` from the validated parts and `write_intent` takes it; the third pass withdrew the exception claim, see PR118-R3-PATH-RULE-EXCEPTION-CLAIMED; `the_record_slot_id_mirrors_the_relative_path` | fixed |
| PR118-R2-SWEEP-QUEUE-STALE | P2 | 8d2547234f27ce4d8a177e7c4b96f7e565ff3e6e / standards/SWEEP.md:39 | the queue says PR #107 is still open -> `ac16fff`, which merged it and added `src/rundir/*`, is an ancestor of the head -> following the table skips files whose split has merged | pre_existing | docs-contract | ea25bc8 | repaired by PR #122, whose `docs/sweep-queue` queues the `rundir` family; reaches this branch when #122 merges | deferred |
| PR118-R2-BODY-CLAIMS-FALSE | P3 | 8d2547234f27ce4d8a177e7c4b96f7e565ff3e6e / .github/pull_request_template.md:1 | the body says no code moved after `9f83b09`, that the findings are in `reviews/FINDINGS.md`, and to revert two commits -> `a7b7c98` moved two Rust files, the file at `8d25472` holds none of the rows, and the diff has more than two commits after the base merge-in -> the body does not describe the exact head | introduced_by_feature | docs-contract | 8d25472 | the body rewritten 2026-09-03 with the full commit list, the exact rollback set and this section, which is in the diff | fixed |
| PR118-R3-SERDE-INTO-CLONES | P2 | cff812de0b82c32667f482cc36f0ce51d142337c / src/workspace_manager/naming.rs:405 | `SlotPath` carries `#[serde(into = "String")]` -> the derived `Serialize` calls `Clone::clone` on the newtype before consuming it -> every `serde_json::to_vec(&record)` deep-copies the slot text, an ownership clone with no `.clone()` in the source, and the body and swept-table row say the file has no clone | introduced_by_feature | docs-contract | bc07f05 | ecddff8: `Serialize` for `SlotId` is written by hand from the borrow; no attribute or macro in the file expands to a clone; `the_intent_record_schema_is_pinned` still pins the bytes | fixed |
| PR118-R3-PATH-RULE-EXCEPTION-CLAIMED | P2 | cff812de0b82c32667f482cc36f0ce51d142337c / src/workspace_manager/naming.rs:190 | `SlotPath(String)` and `format!("{namespace}/{component}")` represent and build a relative path as text -> §8 requires `Path`/`PathBuf` and forbids string concatenation, and §1 says a deviation needs a reviewed change to the standard -> a type doc comment declaring itself the exception establishes nothing | introduced_by_feature | docs-contract | bc07f05 | ecddff8: the field is `SlotId`, an identifier whose grammar mirrors the relative path; no code derives a filesystem path from its text, paths come from `Slot::relative`, and the doc claims no exception; `the_record_slot_id_mirrors_the_relative_path` | fixed |
| PR118-R3-RECORD-ACCEPTS-INVALID-STATE | P2 | cff812de0b82c32667f482cc36f0ce51d142337c / src/workspace_manager/naming.rs:416 | `SlotPath::objection` checks a namespace word and one safe component -> `{"kind":"task","slot":"merge/s1",..}` and `{"kind":"task","slot":"tasks/k-g0",..}` deserialize -> a public type exists in a state no validated slot produces, against §5 and §8 | introduced_by_feature | correctness | bc07f05 | ecddff8: `SlotId::parse` goes through `Slot::from_parts`, `Slot::validate` and the canonical re-rendering, and `TryFrom<IntentRecordWire>` requires `kind` to agree with the slot; `the_record_refuses_a_kind_that_disagrees_with_its_slot` and `the_record_slot_is_refused_on_read_outside_its_grammar` carry both examples; both mutations were run and fail | fixed |
| PR118-R3-NO-ALIAS-UNPINNED | P2 | cff812de0b82c32667f482cc36f0ce51d142337c / src/workspace_manager/naming.rs:469 | the test renames each field to three guessed spellings -> `#[serde(alias = "old_kind")]` on `kind`, or an alias on an `IntentKind` variant, leaves the bytes and every test unchanged while the alias is accepted -> "no aliases" is not pinned | introduced_by_feature | correctness | bc07f05 | ecddff8: the wire struct's and `IntentKind`'s `Deserialize` are hand-written against the literal `IntentRecord::FIELDS` and `IntentKind::WORDS`, so an attribute has nowhere to go; `the_reader_accepts_exactly_the_fields_a_record_writes` compares the list to a serialized record's keys and refuses a fourth word, a duplicate and an unknown key; both mutations were run and fail | fixed |
| PR118-R4-RECORD-FIELDS-PUBLIC | P2 | 3482ba1cde7097856ca94731ee09270f1abc60e0 / src/workspace_manager/naming.rs:376 | deserialize a valid `staging` record -> assign `record.kind = IntentKind::Task` through the public field -> `Serialize` (derived) writes `kind: task` with `slot: merge/s1` -> reading those bytes back fails: the public type does not round-trip and can emit what its reader refuses, against §5's private-fields rule | introduced_by_feature | correctness | ecddff8 | 5a0ae59: the fields are private, `IntentRecord::new` takes the kind from a validated slot, accessors read; `a_record_round_trips_and_cannot_be_built_disagreeing` reads back and re-serializes every shape and refuses an invalid slot; the reviewer's sequence no longer compiles | fixed |
| PR118-R4-READERS-REPEAT-LITERALS | P2 | 3482ba1cde7097856ca94731ee09270f1abc60e0 / src/workspace_manager/naming.rs:425 | `IntentRecord::FIELDS` and `IntentKind::WORDS` are advertised but the visitors match a second, hand-repeated list -> add `"old_kind" => WireField::Kind` or `"job" => IntentKind::Task` -> the lists are unchanged and every sampled test stays green while the reader accepts the alias | introduced_by_feature | correctness | ecddff8 | 5a0ae59: the wire reader accepts a key only by finding it in `FIELDS` zipped with `WireField::ALL`, and `IntentKind` is read only by matching `as_str` of `IntentKind::ALL`, from which `WORDS` is derived; witnesses, each run at the fixing commit: adding `old_kind` to the table fails `the_reader_accepts_exactly_the_fields_a_record_writes` (the serialized keys no longer equal `FIELDS`), and accepting `job` in the kind lookup fails the same test (its refused-word sample) | fixed |
| PR118-R4-CLEAN-MERGE-CLAIM | P3 | 3482ba1cde7097856ca94731ee09270f1abc60e0 / .github/pull_request_template.md:1 | the body says master merges cleanly -> `git merge-tree 1dfb541 3482ba1 20a23d0` shows conflicts in `AGENTS.md` and `standards/SWEEP.md` -> the claim is false in the form the reviewer can check, and a resolved merge changes the head | introduced_by_feature | docs-contract | 3482ba1 (the body) | fixed as text: the body states that master `20a23d0` conflicts in those two files, that the base merge-in is sequenced after #119 by the coordinator and resolved by hand (`AGENTS.md` master's, `standards/SWEEP.md` master's tables with this row moved, `reviews/FINDINGS.md` master first then §45), and carries no clean-merge sentence; the merge-in is not done here | fixed |
| PR118-R4-SCHEMA-WITHOUT-DESIGN-AUTHORITY | P2 | 3482ba1cde7097856ca94731ee09270f1abc60e0 / design/15_design_event_log_resume_run_layout.md:19 | the diff fixes the persisted intent record's exact fields, no-alias and tagging rules -> no design sentence states that contract, §15 only says synced intents exist -> a persisted-data behaviour change without its design change, a MUST-level docs-contract deviation | introduced_by_feature | docs-contract | 51feba7 | 5a0ae59: `design/15` carries a "Synced intents" paragraph stating the four fields, the words, the identifier grammar, and the refusals, as added sentences only (the sentences `src/export.rs` pins are untouched, and its tests pass); the type's doc cites it | fixed |
| PR118-R4-STALE-CLAIMS | P3 | 3482ba1cde7097856ca94731ee09270f1abc60e0 / src/workspace_manager/naming.rs:665 | the body says "all eleven" for fifteen findings and "Serialize and Deserialize written by hand" for a type that derives both, names `SlotIdError` while the type is `SlotPathError`, and counts ten commits where the table has eleven; §45's preamble says two passes before describing the third; `SlotId::as_str` says "the path" -> each claim is false at the head | introduced_by_feature | docs-contract | bc07f05 | 5a0ae59: `SlotIdError` is the type and `SlotId::as_str` says identifier; this section's preamble counts four passes; the body counts twenty findings, says exactly what is derived and what is hand-written, and its rollback count matches its table | fixed |
## 47. PR #126 object.rs sweep (2026-09-04)

Append-only. The §6/§7 sweep of `src/workspace_manager/object.rs` (PR #126), row 4 of the review
queue. These are the rows of the sweeping session's own line-by-line review at the base
`809130d`, recorded before any frontier pass; the passes the coordinator launches append their
rows below these. Under the owner's amendment 1 of 2026-09-04, P1 and P2 findings are fixed and
P3 and lower are recorded: the one P2, the null id accepted on the new side of a create or
compare-and-swap, is fixed at `af382fa` with the parent's variant, call sites and import moved
with it; four P3 rows are fixed in the same commit because the sweep itself is their repair;
two P3 rows are deferred to the parent's sweep, the queue's last row of this family, because the
variant field and the parent's suite are where their repair lives.

The first frontier pass, on `6a54b65` (gpt-5.6-sol at max, posted 2026-09-04T04:03Z), returned
`CHANGES_REQUIRED` with four unlabelled findings, classed here under amendment 1: two P2, the
refusal's missing design authority and a guard that never observed the public primitives, fixed
at `6e7604e`; and two text findings, the stale `# Errors` contracts and a diagnostic that claimed
more than was measured, corrected as text in the same commit and recorded `fixed`. The pass ran
while master `f458cfc` was being merged in as `1df6828`; a base merge-in is not a code change, so
its findings apply to the merged head as-is. On the coordinator's repair brief,
`PR126-OBJECT-CAS-NULL-UNWITNESSED-IN-PARENT-SUITE` below is rewritten to `fixed`: the witness it
deferred is what the pass required, and the third pass row names it as its prior. The `INV-17`
name in the code this pull request introduced is replaced by a citation of `design/26` step 5;
the parent's pre-existing citations of it, one of them in the `NullExpectedOld` message the
parent's suite asserts on, are the parent's sweep's.

The second frontier pass, on `def9320` (gpt-5.6-sol at max, posted 2026-09-04T04:45Z), was the
pull request's one further pass and returned `CHANGES_REQUIRED` with six unlabelled findings
and one evidence inconsistency, no P1. The coordinator classed findings 1, 2 and 4 P2 and 3, 5
and 6 docs-contract text at P3 whose fixes are sentences; all six are fixed at `df494c8`
and the inconsistency (the body said three P2 and one P3 for the first pass; the ledger says two
and two) is corrected in the body. The repaired head is not re-reviewed: it merges as a
repair-only delta disclosed in the body and verified by the coordinator under the owner's
delegation of 2026-09-04.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR126-OBJECT-NEW-SIDE-ACCEPTS-NULL-ID | P2 | 809130d540a20ad01faa1c9e94d7acc2ab3f0359 / src/workspace_manager/object.rs:47 | the new side of create and compare-and-swap was gated only by `is_object_id`, which the null id satisfies -> a caller reaches `compare_and_swap_ref` or `create_ref_zero_old` with a value it never filled in -> git 2.43 reads a null new value as "must not exist afterwards": `update-ref --no-deref <ref> 0{40} <old>` exits 0 and deletes the ref, `update-ref --no-deref <ref> 0{40} ""` exits 0 and creates nothing -> the integration ref is deleted under a name that promises a swap, or a create reports success with no ref behind it; no production caller passes a null new value today, so latent | pre_existing | correctness | 7a83e69 | fixed at af382fa271d2dc1b856c57e756995b3010bfdc7a: `refuse_new` refuses malformed then null on the new side as `refuse_expected_old` does on its side, with `Refusal::NullNew` carrying the measurement; `a_new_value_is_a_well_formed_non_null_id_at_either_hash_length` drives both lengths and was witnessed failing under the null check removed and under the malformed check removed | fixed |
| PR126-OBJECT-REFUSALS-FLATTEN-THE-VARIANT | P3 | 809130d540a20ad01faa1c9e94d7acc2ab3f0359 / src/workspace_manager/object.rs:55 | both refusals returned `UpstrokeError` and converted the `Refusal` into a message at the return site -> no caller and no test could match the variant -> the parent's suite matches the refusals by message substring, which its own `Refusal` doc names as the failure mode, and the file's one `?` propagated a flattened string | pre_existing | correctness | 7a83e69 | fixed at af382fa271d2dc1b856c57e756995b3010bfdc7a: both refusals return `Result<(), Refusal>` and the parent's `?` sites convert through its existing `From<Refusal>`; `a_malformed_id_is_refused_naming_the_ref_the_role_and_the_value_as_offered` and `an_expected_old_is_a_well_formed_non_null_id_at_either_hash_length` assert the variant and its fields, witnessed failing under `refname` and `value` swapped | fixed |
| PR126-OBJECT-CONTRACT-UNTESTED-AT-THE-BOUNDARIES | P3 | 809130d540a20ad01faa1c9e94d7acc2ab3f0359 / src/workspace_manager/object.rs:32 | the file had no tests and the parent's suite drives the null expected-old through delete only and malformed values through create only -> nothing exercised the 64-length boundaries, a non-hex byte inside a well-formed length, the `expected-old` role or uppercase acceptance -> a predicate accepting lengths of 40 or more, or an alphabet of alphanumerics, passed the suite | pre_existing | correctness | 660e9e1 | fixed at af382fa271d2dc1b856c57e756995b3010bfdc7a: six tests in the file drive both predicates and the three refusals at both hash lengths, at lengths 0, 1, 39, 41, 63, 65 and 128, with a non-hex byte at the first and last position and a multibyte character inside forty bytes; `any_other_length_or_any_non_hex_byte_is_not_an_object_id` was witnessed failing under a length check of at least 40 and under an alphanumeric alphabet, `the_null_id_is_all_zeros_at_either_hash_length_and_nothing_else_is` under any digit counting as a zero, `a_full_hexadecimal_id_of_either_hash_length_is_an_object_id` under the SHA-256 length dropped | fixed |
| PR126-OBJECT-HASH-LENGTHS-UNNAMED | P3 | 809130d540a20ad01faa1c9e94d7acc2ab3f0359 / src/workspace_manager/object.rs:33 | the match on the lengths 40 and 64 named neither hash -> a reader has to know that 40 is SHA-1 and 64 is SHA-256 and that the predicate is format-agnostic by design -> the doc said "either hash length" without saying which | pre_existing | docs-contract | 7a83e69 | fixed at af382fa271d2dc1b856c57e756995b3010bfdc7a: `SHA1_HEX_CHARS` and `SHA256_HEX_CHARS`, each documented with the hash it is and the statement that git, not this file, decides which format the repository uses | fixed |
| PR126-OBJECT-MALFORMED-REFUSAL-UNDOCUMENTED | P3 | 809130d540a20ad01faa1c9e94d7acc2ab3f0359 / src/workspace_manager/object.rs:42 | `refuse_malformed_object_id` had no doc comment and no `# Errors` section while its sibling had both -> the `role` parameter's meaning and the fact that nothing has run when it refuses were unstated -> a caller had to read the body to learn what it can act on | pre_existing | docs-contract | 7a83e69 | fixed at af382fa271d2dc1b856c57e756995b3010bfdc7a: documented with `# Errors`, and private now that `refuse_new` and `refuse_expected_old` are its only callers; the effects wrappers row drops the name, as the census that derives the domain from the file requires | fixed |
| PR126-OBJECT-ROLE-IS-A-STRING-TAG | P3 | 809130d540a20ad01faa1c9e94d7acc2ab3f0359 / src/workspace_manager/object.rs:44 | `role: &'static str` on the refusal and on the parent's `MalformedObjectId` field is a string tag with two values where §5 asks for an enum -> a third call site can pass any text -> the refusal message names a side that does not exist; today nothing branches on the tag and the two literals are passed by the file's own two wrappers | pre_existing | docs-contract | 7a83e69 | deferred to the parent's sweep of `src/workspace_manager.rs`, the queue's last row of this family, where the variant's field lives; until then the two literals are passed only by `refuse_expected_old` and `refuse_new`, and `an_expected_old_is_a_well_formed_non_null_id_at_either_hash_length` was witnessed failing under the expected-old wrapper passing the role new | deferred |
| PR126-OBJECT-CAS-NULL-UNWITNESSED-IN-PARENT-SUITE | P3 | 809130d540a20ad01faa1c9e94d7acc2ab3f0359 / src/workspace_manager/tests.rs:1678 | the parent's suite executes its null-old measurement and refusal through `delete_ref_expected_old` only -> the merge-queue CAS and the create shape are never driven against a null or malformed value on either side against a real repository -> the null-new measurement recorded in `Refusal::NullNew` is made on a scratch repository and not executed by a test | pre_existing | correctness | 660e9e1 | fixed at 6e7604e00391e1d09fa1d5a7f356a2d3078ca2b9 on the coordinator's repair brief after the first pass: `the_null_object_id_is_never_a_new_value_through_create_or_compare_and_swap` in the parent's suite drives `compare_and_swap_ref` and `create_ref_zero_old` with the null id of both lengths as the new value against a real repository, asserts the refusal and that the ref is unchanged, and executes the raw-git measurement as `the_null_object_id_is_never_an_expected_old_value` does for the old side; witnessed failing with `refuse_new` removed from the create and, separately, from the compare-and-swap | fixed |
| PR126-REVIEW-NULL-NEW-HAS-NO-DESIGN-AUTHORITY | P2 | 6a54b658408ea4adab40963f4fc850a1b7597bd4 / src/workspace_manager.rs:227 | the new refusal cites INV-17, a packet invariant the file's pre-existing null-old refusal already cites, and no design file states the rule -> `design/26` step 5 specifies the compare-and-swap and says nothing about the null id -> a behaviour change with no living authority, against the sole-authority rule and §13 | introduced_by_feature | docs-contract | af382fa | fixed at 6e7604e00391e1d09fa1d5a7f356a2d3078ca2b9: `design/26_design_merge_queue_protocol.md` step 5 states that the engine's ref primitives take a full hexadecimal object id on both sides and refuse the null id on either before the mutating `update-ref`, and why; the `Refusal::NullNew` doc points at it | fixed |
| PR126-REVIEW-ERRORS-CONTRACTS-STALE-AT-CHANGED-SITES | P3 | 6a54b658408ea4adab40963f4fc850a1b7597bd4 / src/workspace_manager.rs:1358 | `create_ref_zero_old` and `compare_and_swap_ref` now return `NullNew` as `UpstrokeError::Refused` and their `# Errors` omit it, as do `IntegrationRefs::create_zero_old` and `ensure_integration_ref` -> the body deferred the docs to the parent's sweep while claiming §13 was applied -> §13 is not transitional, so the deferral was invalid | introduced_by_feature | docs-contract | af382fa | fixed as text at 6e7604e00391e1d09fa1d5a7f356a2d3078ca2b9: the four `# Errors` sections name the object-id refusals for `new` and, on the compare-and-swap, for `old` | fixed |
| PR126-REVIEW-NULL-NEW-GUARD-DOES-NOT-OBSERVE-THE-PRIMITIVES | P2 | 6a54b658408ea4adab40963f4fc850a1b7597bd4 / src/workspace_manager/object.rs:124 | the six new tests call the private helpers only -> `refuse_new` removed from either public primitive leaves the whole suite green -> ref at A, compare-and-swap with old A and new 0{40}, Git exits 0, the ref is deleted, the method reports success; the body's claim that the tests guard the fix was stronger than the tests | introduced_by_feature | correctness | PR126-OBJECT-CAS-NULL-UNWITNESSED-IN-PARENT-SUITE | fixed at 6e7604e00391e1d09fa1d5a7f356a2d3078ca2b9: `the_null_object_id_is_never_a_new_value_through_create_or_compare_and_swap` drives `compare_and_swap_ref` and `create_ref_zero_old` with the null id of both lengths against a real repository, asserts the refusal names its reason and that the ref is unchanged, and executes the raw-git measurement as `the_null_object_id_is_never_an_expected_old_value` does; witnessed failing with `refuse_new` removed from the create and, separately, from the compare-and-swap | fixed |
| PR126-REVIEW-NULL-NEW-MESSAGE-OVERSTATES-THE-MEASUREMENT | P3 | 6a54b658408ea4adab40963f4fc850a1b7597bd4 / src/workspace_manager.rs:236 | the message said Git would delete the ref or create nothing while reporting success -> with a mismatched old value, or an existing ref on the create path, git 2.43 exits 128 and preserves the ref -> the diagnostic claimed more than the measurement | introduced_by_feature | docs-contract | af382fa | fixed as text at 6e7604e00391e1d09fa1d5a7f356a2d3078ca2b9: the message, the variant doc and the module doc of `src/workspace_manager/object.rs` qualify the delete with "when the expected old matches" and the empty create with "when the ref is absent", and say what a mismatch does | fixed |
| PR126-REVIEW2-NULL-TESTS-INHERIT-THE-HASH-FORMAT | P2 | def9320639b28cf61965457c1bee768243ba3dbf / src/workspace_manager/tests.rs:1797 | the fixture's unqualified `git init` inherits `GIT_DEFAULT_HASH` -> the new null-new test and the pre-existing null-old test hard-code forty-zero raw `update-ref` operations -> under `GIT_DEFAULT_HASH=sha256` both fail at their raw call ("not a valid SHA1"), so the two-hash evidence held only in a SHA-1 environment, against §12's controlled-environment rule | introduced_by_feature | correctness | 6e7604e (the new test; the null-old test carried the same defect since 7a83e69) | fixed at df494c8d0acd8a643d4de17c95666af3c0c6e550: `Fixture::new` pins the object format to SHA-1 through `with_object_format`, `Fixture::created_sha256` is the other format, and `the_null_object_id_is_never_a_new_value_through_create_or_compare_and_swap` runs against both with the raw null id spelt at `fixture.head.len()`; the reviewer's command failed both tests before and passes both after; witnessed: with the pin dropped the null-old test fails again under that environment, and with the raw length fixed at forty the new test fails in the SHA-256 fixture | fixed |
| PR126-REVIEW2-DESIGN-SENTENCE-CONFLATES-CAS-AND-DELETE | P2 | def9320639b28cf61965457c1bee768243ba3dbf / design/26_design_merge_queue_protocol.md:33 | the added design sentence said a null expected-old "deletes unconditionally" -> that is the `-d` form; on the compare-and-swap form against an existing ref git 2.43 exits 128 and preserves it, and against an absent ref it creates -> the sole design authority for the refusal was factually wrong | introduced_by_feature | docs-contract | 6e7604e | fixed at df494c8d0acd8a643d4de17c95666af3c0c6e550: the added sentences distinguish the compare-and-swap (fails on an existing ref, creates on an absent one), `update-ref -d` (deletes unconditionally) and the new side (deletes on a matching old, creates nothing on an absent ref), each measured on git 2.43 on this box; no pre-existing sentence touched | fixed |
| PR126-REVIEW2-BEFORE-GIT-IS-ASKED-OVERSTATES | P3 | def9320639b28cf61965457c1bee768243ba3dbf / src/workspace_manager.rs:1372 | `refuse_symbolic` and `assert_publishable` invoke Git before `refuse_new` runs -> a symbolic ref with a null new value returns `SymbolicRef`, not `NullNew` -> "before Git is asked" in the design sentence, the test's doc and the body's Risk claimed more than the implementation | introduced_by_feature | docs-contract | 6e7604e | fixed as text at df494c8d0acd8a643d4de17c95666af3c0c6e550: `design/26`, the test's doc and the body say "before the mutating `update-ref`", and the test's doc says which reads precede it | fixed |
| PR126-REVIEW2-DOUBLES-ACCEPT-NULL-NEW | P2 | def9320639b28cf61965457c1bee768243ba3dbf / src/engine/topology/create.rs:469 | `IntegrationRefs::create_zero_old`'s doc promised the refusal while both in-tree doubles stored any value, and `ensure_integration_ref` validated nothing -> `ensure_integration_ref` over an empty `FakeRefs` with a null base returned `Ok` and stored the forbidden value, and over `FakeRefs::at` the null id took the same-target `Ok` arm -> the contract widened into implementations that did not honour it | introduced_by_feature | correctness | 6e7604e | fixed at df494c8d0acd8a643d4de17c95666af3c0c6e550: `refuse_new` is crate-visible, `ensure_integration_ref` applies it before reading the ref so a ref already at the null id is not adopted, `FakeRefs` and `RecordingRefs` apply it in their `create_zero_old`; `ensure_integration_ref_refuses_a_null_base_whether_the_ref_is_absent_or_at_it` drives the reviewer's two sequences, `the_fake_refs_refuse_a_null_new_value_as_the_real_primitive_does` and `the_recording_refs_refuse_a_null_new_value_as_the_real_primitive_does` drive each double; witnessed failing with the guard removed from `ensure_integration_ref`, from `FakeRefs` and from `RecordingRefs`, one at a time | fixed |
| PR126-REVIEW2-RECOVERY-WRAPPER-ERRORS-DOC-STALE | P3 | def9320639b28cf61965457c1bee768243ba3dbf / src/engine/topology/recover.rs:2122 | `ensure_recorded_integration_ref` forwards to `ensure_integration_ref` and its `# Errors` named neither the malformed nor the null-base refusal -> `CommitSha` does not validate the invariant, so an absent ref and a null recorded base reach the refusal through the real manager undocumented | introduced_by_feature | docs-contract | 6e7604e | fixed as text at df494c8d0acd8a643d4de17c95666af3c0c6e550: the `# Errors` section names both refusals and says `CommitSha` does not validate them | fixed |
| PR126-REVIEW2-NULLNEW-SEMVER-UNASSESSED | P3 | def9320639b28cf61965457c1bee768243ba3dbf / src/workspace_manager.rs:101 | `Refusal` is public and not non-exhaustive, and the body called `NullNew` "a public item added" with no §5 SemVer assessment -> a downstream exhaustive match over `Refusal` fails to compile with a missing arm -> the risk statement was incomplete | introduced_by_feature | compatibility | af382fa | documented guard: the body's Risk carries the §5 assessment: `workspace_manager` and `Refusal` do not exist at `v0.1.0` (they arrived at 7a83e69 on 2026-08-22, after the tag), so no released API changes; inside the unreleased 0.2 line the variant breaks an exhaustive match compiled against a master snapshot, adapted by adding an arm or a wildcard, an intentional pre-0.2.0 break; marking the enum non-exhaustive is a larger contract change left to the parent's sweep | fixed |

## 46. PR #120 containment.rs sweep (2026-09-03)

The first §6/§7 sweep pull request (`standards/SWEEP.md` queue row 1). Two frontier passes by
`gpt-5.6-sol` at `max`: the exact head `ee26cb42bbda4b2ca8bdef62e6143a46dfe74884` (verdict
CHANGES_REQUIRED, five findings, no P1,
https://github.com/eventloops/upstroke/pull/120#issuecomment-5532449708) and the repaired head
`e4bf5dc18255392664603fa3873bc099a7c6d931` (verdict CHANGES_REQUIRED, six findings, one P1,
https://github.com/eventloops/upstroke/pull/120#issuecomment-5533097443), plus two findings from
the coordinating session's own read of the second head; and the head that repaired those,
`41facd4d402270bd6e94976ae2f4257c2874f02e` (verdict CHANGES_REQUIRED, four findings, one P1,
https://github.com/eventloops/upstroke/pull/120#issuecomment-5533554245). Owner direction for this file's
refinement pass: every finding of the first two passes is fixed; from the third pass, by owner
direction of 2026-09-03 after PR #122 merged, only the P1 was repaired at first; amendment 1 of
2026-09-04 (P1 and P2 findings fixed, P3 and lower recorded) reinstated the two P2 repairs, and
the P3 is deferred to the parent's sweep (`standards/SWEEP.md` queue row 11).
A fourth pass on `d5cfbc34412f785534d7260ddf2d0147cd2b5d0c` (verdict CHANGES_REQUIRED, two P1, two P2, one P3,
https://github.com/eventloops/upstroke/pull/120#issuecomment-5533982492) is dispositioned the same
way: the P1s and P2s fixed, the P3 deferred.
A fifth pass on `dd8befecb65d1f373669a49a4fe421591390d119` (verdict CHANGES_REQUIRED, one P1, three P2,
https://github.com/eventloops/upstroke/pull/120#issuecomment-5534683780) is repaired as a class:
every funnel primitive names the paths it acts through, as data, and one walk checks them all.
A sixth pass on `62816dd4f3263e8dd252b009d3b0a6c999e5e9cc` (verdict CHANGES_REQUIRED, two P1, four P2, one P3,
https://github.com/eventloops/upstroke/pull/120#issuecomment-5535146761) narrows that claim: the
table is nine roles and no more, and Git's own repository-discovery paths are the parent's funnel
design, deferred to its sweep; the coordinator declared the seventh pass final.
The seventh pass on `206d34845185d075a93b87324fd67b6cef01d062` (verdict CHANGES_REQUIRED, four P2, one P3, no P1,
https://github.com/eventloops/upstroke/pull/120#issuecomment-5535735474) is repaired forward at the
head that merges: staging leftovers are reported and never deleted, the object lookups propagate a
refusal, a run id is the canonical ULID, and the three text claims are corrected; that head merges
as a disclosed repair-only delta verified by the coordinator under the owner's 2026-09-04
delegation, not re-reviewed. The "First bad / prior ID" column names the introducing
commit; the guard column names the fixing commit and the test that holds the repair.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR120-ABSOLUTE-RUN-ID-ALIASES-A-PEER-ROOT | P2 | ee26cb42bbda4b2ca8bdef62e6143a46dfe74884 / src/workspace_manager.rs:346 | `execution_root_of` joins an absolute `run_id` so it replaces the intended prefix -> `plain_chain_below` accepts the result whenever it lies at or below the private root, every component being `Normal` -> `derive(base, P, "/abs/P/workspaces/K/victim")` aliases a peer manager's root and `revalidate` treats the victim's worktree as this manager's slot -> `remove_worktree` deletes the victim's checkout; `run_id = "."` passes too because `components()` folds a non-leading `.` away | pre_existing | security-trust | 7a83e69 (`execution_root_of` and the walk); 2dd1350 narrowed but did not close it | fixed in e4bf5dc: `refuse_unplain_run_id` at `derive` refuses any run id that is not one plain component (`Refusal::RunId`) before a path is built; `a_run_id_that_is_not_one_plain_component_is_refused_before_any_path_is_built` | fixed |
| PR120-REVALIDATE-SKIPS-THE-PRIVATE-ROOT-ANCHOR | P2 | ee26cb42bbda4b2ca8bdef62e6143a46dfe74884 / src/workspace_manager/containment.rs:142 | the walk sets `walked = anchor` and pushes the first child before the first `symlink_metadata` -> the private root `P` itself is never examined -> rename `P` and replace it with a symlink or junction to `O` -> `create_execution_root` revalidates `P/workspaces` through the link and `canonical_prefix` resolves under `O` with nothing to compare against -> `create_dir_all` builds the hierarchy under `O` | pre_existing | security-trust | 7a83e69 (the anchored walk) | fixed in e4bf5dc: `refuse_reparse_points` examines the anchor itself and pins its canonical form; `a_private_root_replaced_by_a_link_after_derive_refuses_every_revalidation`, `a_link_planted_above_the_private_root_after_derive_refuses_every_revalidation` | fixed |
| PR120-NEW-TESTS-DISCARD-CLEANUP-ERRORS | P3 | ee26cb42bbda4b2ca8bdef62e6143a46dfe74884 / src/workspace_manager/tests.rs:1020 | three regression tests end with `let _ = fs::remove_dir_all(..)` -> a cleanup failure is unobserved, against §7 and §12's RAII temporary directories -> a leaked fixture on a shared runner is invisible to the suite | introduced_by_feature | docs-contract | 2dd1350 | fixed in e4bf5dc: the three tests take `rundir::scratch_tree::acquire`, whose `ScratchTree` drop reclaims the tree and reports a reclaim that failed; `canonical_prefix_propagates_a_resolution_failure_that_is_not_absence` | fixed |
| PR120-DERIVE-ERRORS-DOC-OMITS-IO | P3 | ee26cb42bbda4b2ca8bdef62e6143a46dfe74884 / src/workspace_manager.rs:542 | the sweep introduces `canonicalize` I/O failures at `derive` -> the rewritten `# Errors` list names refusals and Git failure but not `UpstrokeError::Io` -> a caller reading the contract handles the wrong set; `canonical_prefix`'s relative terminal arm reports the original path while carrying an error produced on the shortened head | introduced_by_feature | docs-contract | 2dd1350 | fixed in e4bf5dc (the `# Errors` list) and c66fb77d3836b24dc41f2da99f58f18309ef30b8 (the relative arm is gone: a relative path anchors at the current directory); `canonical_prefix_anchors_a_relative_path_at_the_current_directory` | fixed |
| PR120-REFUSAL-TEXT-CITES-A-RETIRED-AUTHORITY | P3 | ee26cb42bbda4b2ca8bdef62e6143a46dfe74884 / src/workspace_manager.rs:127 | the new `RootOutsidePrivateRoot` refusal text names `decisions.workspace_candidates.execution_root` -> the decisions directory was retired on 2026-09-03 and DESIGN §15 holds the exact-root contract -> an operator following the citation finds nothing | introduced_by_feature | docs-contract | 2dd1350 | fixed in e4bf5dc: the `RootOutsidePrivateRoot` and `RunId` texts cite DESIGN.md §15; `an_execution_root_with_no_plain_chain_below_the_private_root_refuses_before_any_effect` reads the text | fixed |
| PR120-ANCHOR-CHECK-BYPASSED-BETWEEN-GATE-AND-EFFECT | P1 | e4bf5dc18255392664603fa3873bc099a7c6d931 / src/workspace_manager.rs:785 | `create_execution_root` calls `revalidate()` and then enters `funnel`, whose `Before` hook runs before the primitive -> a hook that renames the private root and plants a link in its place after the check and before `create_dir_all` -> revalidation, anchor check included, has already passed -> the hierarchy is created under the link's target; `a_registration_rebound_after_validation_keeps_its_admin_state` already drives that seam | pre_existing | security-trust | 7a83e69 (the gate-before-funnel shape); e4bf5dc's anchor check inherited it | fixed in c66fb77d3836b24dc41f2da99f58f18309ef30b8: `revalidate_chain` — base real, anchor still itself, chain plain and reparse-free — runs as the first statement of every funnel primitive, adjacent to the effect, while `revalidate` stays as the gate before the funnel; `a_private_root_exchanged_between_the_before_hook_and_the_effect_is_still_refused` | fixed |
| PR120-REGULAR-FILE-ON-CHAIN-COLLAPSES-TO-NOTHING-TO-REMOVE | P2 | e4bf5dc18255392664603fa3873bc099a7c6d931 / src/workspace_manager.rs:829 | e4bf5dc lets `ENOTDIR` pass revalidation as absence -> plant a regular file at `<private>/workspaces` and call `remove_execution_root` -> `revalidate()` succeeds and `execution_root.exists()` folds the file into `false` -> `Ok(false)` with no error naming the path, where Unix used to stop with the walk's I/O error | introduced_by_feature | correctness | e4bf5dc | fixed in c66fb77d3836b24dc41f2da99f58f18309ef30b8: the walk reads each existing component's type and reports a regular file where it stands, as `NotADirectory` at its own path, on every platform; `a_regular_file_on_the_chain_is_reported_where_it_stands_and_never_as_nothing_to_remove` | fixed |
| PR120-RELATIVE-PATH-SPELLINGS-DISAGREE | P2 | e4bf5dc18255392664603fa3873bc099a7c6d931 / src/workspace_manager/containment.rs:340 | `canonical_prefix("missing")` errors `NotFound` because its parent is empty while `"./missing"` peels to `.`, canonicalizes the current directory and succeeds -> public `quiescence("missing", ..)` fails as I/O while `quiescence("./missing", ..)` reaches `VerifyFailure::NotRegistered` -> two answers for one path | pre_existing | correctness | 7a83e69 (the raw return); e4bf5dc turned it into the error | fixed in c66fb77d3836b24dc41f2da99f58f18309ef30b8: an empty parent peels to `.`, the current-directory anchor, and rejoins; `canonical_prefix_anchors_a_relative_path_at_the_current_directory`, `canonical_prefix_resolves_an_existing_relative_prefix_and_rejoins_the_rest` | fixed |
| PR120-ANCHOR-PIN-COLLAPSES-ABSENCE-INTO-IO | P3 | e4bf5dc18255392664603fa3873bc099a7c6d931 / src/workspace_manager/containment.rs:201 | `symlink_metadata` on the anchor succeeds -> the anchor vanishes -> the following `canonicalize` returns `NotFound` or `NotADirectory` and the catch-all `map_err` makes it `UpstrokeError::Io` -> against the adjacent contract that only failures other than absence do so | introduced_by_feature | correctness | e4bf5dc | fixed in c66fb77d3836b24dc41f2da99f58f18309ef30b8: the pin routes absence through `is_absent` to `Refusal::BaseIsNotADirectory`; the race itself cannot be staged from a test, and `an_absent_anchor_refuses_as_not_a_real_directory` drives the rule at the function's edge | fixed |
| PR120-BODY-CONTRADICTS-THE-DIFF | P2 | e4bf5dc18255392664603fa3873bc099a7c6d931 / reviews/FINDINGS.md:1 | the body says the only parent edit is `RootOutsidePrivateRoot` and declares run-id validation out of scope -> the head adds `Refusal::RunId`, `refuse_unplain_run_id` and the early `derive` check -> the body claims six tests where the diff adds ten, lists four behaviour changes and omits both anchor repairs, and its rollback omits e4bf5dc -> the scope statement does not match the diff | introduced_by_feature | docs-contract | the body edit of 2026-09-03 that published e4bf5dc | fixed: the Scope, Summary, Risk and rollback sections were rewritten to the exact head on 2026-09-03 after c66fb77d3836b24dc41f2da99f58f18309ef30b8, naming every parent edit, the true test count, every behaviour change and every commit and path after the base merge-in; `validate-pr-body.sh` and `validate-pr-ledger-evidence.sh` pass against it | fixed |
| PR120-RETIRED-AUTHORITY-STILL-CITED-AS-LIVING | P3 | e4bf5dc18255392664603fa3873bc099a7c6d931 / src/workspace_manager.rs:105 | the repaired path returns `Refusal::ReparsePointOnChain`, whose text says `decisions.workspace_candidates.execution_root` establishes the rule -> the swept module's doc at containment.rs:4 and around line 110 says that record requires and says the behaviour -> the record is absent at the head and is treated as living authority | pre_existing | docs-contract | 7a83e69 | fixed in c66fb77d3836b24dc41f2da99f58f18309ef30b8: the `ReparsePointOnChain` text and every sentence in `containment.rs` cite `DESIGN.md` §15, which gains the chain rule's one living sentence in `design/15_design_event_log_resume_run_layout.md`; the retired record is mentioned once, in the past tense | fixed |
| PR120-UNREACHABLE-IN-THE-PEEL | P3 | e4bf5dc18255392664603fa3873bc099a7c6d931 / src/workspace_manager/containment.rs:379 | `canonical_prefix` carries `unreachable!("a head with a parent pops to it")` in production code -> §7 wants shapes that make the impossible branch impossible, and the file is swept to leave no panic -> the `pop()` false arm is the same terminal case as the empty parent | introduced_by_feature | correctness | e4bf5dc | fixed in c66fb77d3836b24dc41f2da99f58f18309ef30b8: the arm returns the terminal case's error naming the head, the in-place `pop` stays and no per-step parent clone returns; `canonical_prefix_anchors_a_relative_path_at_the_current_directory` drives the peel's tail | fixed |
| PR120-RUN-ID-RULE-RESTATED | P3 | e4bf5dc18255392664603fa3873bc099a7c6d931 / src/workspace_manager.rs:387 | `refuse_unplain_run_id` restates `naming::safe_component`'s grammar with its own three messages -> PR #118 is reshaping `safe_component` into a `Result` with the same messages -> two statements of one rule can drift apart unnoticed | introduced_by_feature | docs-contract | e4bf5dc | fixed in c66fb77d3836b24dc41f2da99f58f18309ef30b8: the doc says it is `safe_component`'s rule for a run id and folds into one helper in the parent's sweep; `a_run_id_and_a_slot_component_are_refused_by_the_same_rule` holds the two to the same verdicts on the same inputs | fixed |
| PR120-IN-FUNNEL-CHECK-STOPS-AT-THE-ROOT | P1 | 41facd4d402270bd6e94976ae2f4257c2874f02e / src/workspace_manager.rs:754 | `revalidate_chain` walks the chain only to the execution root -> a `Before` hook renames `<root>/intents` and plants a link to a victim directory holding `tasks.kalpha-g1.intent` -> the in-funnel check passes, the link being below the root -> `remove_file` follows the link in its parent and deletes the victim's file; `write_intent` and `add_worktree` write through a substituted `intents/` or `tasks/` the same way | introduced_by_feature | security-trust | c66fb77 (the check as first written); the parent-following removals are 7a83e69's | fixed in a01eecb and retained by f90935542720d76b2ffea43b08cdab1f104d99e8, which withdrew that commit's other repairs by owner direction: `revalidate_chain` takes the effect's own target and walks down to it, every primitive passing the deepest path it acts through; `an_intents_directory_exchanged_at_the_before_hook_refuses_the_intent_removal`, `an_intents_directory_exchanged_at_the_before_hook_refuses_the_intent_write`, `a_tasks_directory_exchanged_at_the_before_hook_refuses_the_worktree_add` | fixed |
| PR120-CANONICAL-PREFIX-REJOINS-BELOW-A-FILE | P2 | 41facd4d402270bd6e94976ae2f4257c2874f02e / src/workspace_manager/containment.rs:40 | `is_absent` classes `NotADirectory` as absence for the peel -> `canonical_prefix(D/file/child)` fails `NotADirectory`, peels `child`, canonicalizes `D/file` and rejoins -> `Ok(D/file/child)`, a path the filesystem rejected, is compared for containment | introduced_by_feature | correctness | 2dd1350 | fixed in 61398c6706d093b3397c3d0e2f54f9fd01802e8e under amendment 1, reinstating a01eecb's repair that f909355 had withdrawn: a resolved prefix must be a directory while components remain below it, and is `NotADirectory` at that prefix otherwise, on every platform; `canonical_prefix_refuses_a_prefix_that_is_a_regular_file_with_components_below_it` | fixed |
| PR120-ACTIVATED-PARENT-BODIES-NOT-SWEPT | P2 | 41facd4d402270bd6e94976ae2f4257c2874f02e / src/workspace_manager.rs:1103 | the change modifies eleven funnels, `derive`, `revalidate` and `revalidate_removal`, which activates §6 and §7 over their whole bodies -> `add_worktree` keeps `intent.is_file()`, folding every metadata failure into `AddWithoutIntent`, and `remove_execution_root` keeps `let _ = fs::remove_dir(..)` -> deferring them contradicts the activation rule as written, and the owner's direction now supersedes it for the sweep pull requests | pre_existing | correctness | 7a83e69 (the sites); c66fb77 activated them | fixed in 61398c6706d093b3397c3d0e2f54f9fd01802e8e under amendment 1, reinstating a01eecb's repair that f909355 had withdrawn: every `exists()`, `is_file()`, `is_ok_and` and `let _ =` in a modified body reads the metadata and decides absence from failure, the lossy slot text in `write_intent` is a checked conversion, and two clones leave `add_worktree` and `create_execution_root`; `an_intent_that_cannot_be_read_is_an_error_and_not_an_absent_intent`, `a_scaffolding_directory_that_cannot_be_removed_is_reported_not_swallowed` | fixed |
| PR120-REPO-KEY-DOCS-CITE-RETIRED-RECORD | P3 | 41facd4d402270bd6e94976ae2f4257c2874f02e / src/workspace_manager.rs:337 | the body says every citation of the retired execution-root record in both files now cites DESIGN §15 -> `REPO_KEY_V1_DOMAIN`'s and `repo_key_v1`'s docs still cite `decisions.workspace_candidates.execution_root` as the normative source of the repo-key formula -> the record is absent at the head and DESIGN does not state the formula | pre_existing | docs-contract | 7a83e69 | deferred by owner direction: the parent's sweep, `standards/SWEEP.md` queue row 11, rewrites the docs; a01eecb carried the shape to reinstate — a `repo_key` v1 sentence in `design/15_design_event_log_resume_run_layout.md` and both docs citing it — and f90935542720d76b2ffea43b08cdab1f104d99e8 withdrew it; `the_repo_key_is_the_packets_digest_and_not_a_neighbouring_one` pins the formula meanwhile | deferred |
| PR120-HOOKS-PATH-NEVER-REVALIDATED | P1 | d5cfbc34412f785534d7260ddf2d0147cd2b5d0c / src/workspace_manager.rs:1181 | every Git command runs with `core.hooksPath` at `<root>/hooks-none` and the in-funnel check walks only the effect's target -> a `Worktree.Add` `Before` hook replaces `hooks-none` with a link to an outside directory holding an executable `post-checkout` -> the inner check walks `tasks/<slot>` and passes -> `git worktree add` follows the link and executes the outside hook | pre_existing | security-trust | 7a83e69 (the hooks path was never walked); c66fb77's in-funnel check inherited it | fixed in be57a3341d666113455a085ef1a7aac7c2667d06: `revalidate_hooks_path` walks the chain from the private root down to `hooks-none` inside the Git runner, immediately before every command; `a_hooks_path_exchanged_at_the_before_hook_refuses_the_worktree_add_and_runs_no_hook` | fixed |
| PR120-INTENT-STAGING-LEAF-FOLLOWS-A-PLANTED-LINK | P1 | d5cfbc34412f785534d7260ddf2d0147cd2b5d0c / src/workspace_manager.rs:2670 | `write_synced` stages through the fixed name `<intent>.tmp` opened with `File::create`, which follows a link -> plant `intents/<intent>.tmp -> /outside/victim` and call `write_intent` -> every check passes, `File::create` truncates and writes the victim, the link is renamed to the intent's name and the call returns success; §8 forbids a fixed staging name | pre_existing | security-trust | 7a83e69 | fixed in be57a3341d666113455a085ef1a7aac7c2667d06: a per-call unique staging name opened `create_new`, so a planted name refuses rather than being followed, and a refused attempt removes its staged file or names it in the error; `a_link_planted_at_the_old_staging_name_is_never_followed_by_the_intent_write`, `a_link_planted_at_the_intent_name_refuses_the_intent_write` (since ceec50f a link at the intent's name refuses like a link anywhere on an acted-through path) | fixed |
| PR120-DURABLE-INTENT-CHECK-OUTSIDE-THE-FUNNEL | P2 | d5cfbc34412f785534d7260ddf2d0147cd2b5d0c / src/workspace_manager.rs:1163 | `add_worktree` checks the intent before the funnel and the `Before` hook, and rechecks only the worktree path inside -> a `Before` hook removes the intent -> the inner check passes and Git creates the worktree -> `intents()` returns no slot, so `reclaim_intents` never removes it | pre_existing | crash-consistency | 7a83e69 | fixed in be57a3341d666113455a085ef1a7aac7c2667d06: the metadata read moves inside the funnel after the hook, a read failure staying `UpstrokeError::Io`; `an_intent_removed_at_the_before_hook_refuses_the_worktree_add` asserts the refusal, no worktree, and that `intents()` and `reclaim_intents` agree | fixed |
| PR120-MODE-BIT-TESTS-ASSUME-AN-UNPRIVILEGED-USER | P2 | d5cfbc34412f785534d7260ddf2d0147cd2b5d0c / src/workspace_manager/tests.rs:1527 | two Unix tests inject failure through mode bits -> root or a process with CAP_DAC_OVERRIDE reads through 000 and writes through 555 -> the scaffolding removal succeeds and deletes the root, `expect_err` panics, and `RestoreMode`'s drop panics again on the deleted path, aborting the process; `#[cfg(unix)]` does not establish the prerequisite | introduced_by_feature | correctness | 61398c6 | fixed in be57a3341d666113455a085ef1a7aac7c2667d06: after each chmod the test probes the operation the mode should refuse and fails with a diagnostic naming the prerequisite if it succeeded, and `RestoreMode` tolerates an absent path; `an_intent_that_cannot_be_read_is_an_error_and_not_an_absent_intent`, `a_scaffolding_directory_that_cannot_be_removed_is_reported_not_swallowed` | fixed |
| PR120-PARENT-ERROR-DOCS-OMIT-IO | P3 | d5cfbc34412f785534d7260ddf2d0147cd2b5d0c / src/workspace_manager.rs:699 | `revalidate` documents only containment refusals and Git errors while the regular-file test requires `UpstrokeError::Io` -> `add_worktree` documents no I/O error while the intent-metadata read returns one -> the public contracts do not name a demonstrated outcome | introduced_by_feature | docs-contract | e4bf5dc (`revalidate`'s Io), 61398c6 (`add_worktree`'s) | deferred by owner direction (P3 and lower recorded): the two `# Errors` lists are owed and are rewritten in the parent's sweep, `standards/SWEEP.md` queue row 11; `a_regular_file_on_the_chain_is_reported_where_it_stands_and_never_as_nothing_to_remove` and `an_intent_that_cannot_be_read_is_an_error_and_not_an_absent_intent` pin the outcomes the docs omit | deferred |
| PR120-REBOUND-ADMIN-DIRECTORY-FOLLOWED-INTO-A-VICTIM | P1 | dd8befecb65d1f373669a49a4fe421591390d119 / src/workspace_manager.rs:1415 | `remove_worktree` captures the registration's admin path before the `Before` hook and the inner check walks only the checkout -> the hook renames the admin directory and plants a link to an outside victim holding copied `gitdir` bytes and a `locked` file -> `registration_still_names` follows the link and accepts the copied identity -> `remove_file(admin/locked)` deletes the victim's file; the hostile link exists during the inner check, which never examines this acted-through path | pre_existing | security-trust | 7a83e69 (the admin path was never walked); c66fb77's in-funnel check and be57a33's hooks-path walk each left it open | fixed in ceec50f25bf9f0939e2307604ef1720ffab366ca as a class: `Primitive::acted_through` names every path each funnel primitive acts through and `revalidate_acted_through` walks the whole set with `symlink_metadata` on every component before the syscalls; `every_path_a_primitive_acts_through_refuses_a_link_planted_at_the_before_hook` (generated from the table, forty cases, count pinned; dropping `Registration` from `RemoveWorktree` fails it and the named test), `a_registration_admin_directory_exchanged_at_the_before_hook_refuses_the_worktree_removal` | fixed |
| PR120-ADD-AUTHORISED-THROUGH-A-LINKED-INTENTS-DIRECTORY | P2 | dd8befecb65d1f373669a49a4fe421591390d119 / src/workspace_manager.rs:1161 | after `Before`, `add_worktree` revalidates only `tasks/<slot>` and reads the intent with `symlink_metadata` -> a hook replaces `intents/` with a link to an outside directory holding a same-named regular file -> the read follows the intermediate link and reports a regular file -> Git creates a worktree whose durable intent is outside the authorized root and in someone else's control, so `reclaim_intents` cannot discover it once it disappears | pre_existing | crash-consistency | be57a33 (the read moved inside the funnel without its chain); the parent-following read is 7a83e69's | fixed in ceec50f25bf9f0939e2307604ef1720ffab366ca as a class: `AddWorktree`'s acted-through set names `IntentsDirectory` and `IntentFile` and the walk covers both before the read; `an_intents_directory_exchanged_at_the_before_hook_refuses_the_worktree_add`, and the generated test's `AddWorktree` cases (dropping `IntentsDirectory` from its set fails the generated test by count) | fixed |
| PR120-STAGING-NAME-NARROWS-VALID-SLOT-NAMES | P2 | dd8befecb65d1f373669a49a4fe421591390d119 / src/workspace_manager.rs:2708 | `safe_component` has no length bound and a 207-byte task key is valid -> its intent name is 224 bytes and the old staging name 221 -> the new `.<intent>.<ULID>.tmp` staging name is 256 bytes -> `create_new` fails `ENAMETOOLONG` on a `NAME_MAX=255` filesystem, an undisclosed regression | introduced_by_feature | compatibility | be57a33 | fixed in ceec50f25bf9f0939e2307604ef1720ffab366ca: the staging name is `.stage-<ULID>.tmp`, 37 bytes whatever the slot is called, stated on `write_intent`; `a_slot_name_at_the_old_maximum_still_lands_its_intent` | fixed |
| PR120-STAGING-ORPHAN-POISONS-INTENT-RECOVERY | P2 | dd8befecb65d1f373669a49a4fe421591390d119 / src/workspace_manager.rs:1051 | a kill after the staging file is written and before the rename leaves `.<intent>.<ULID>.tmp` -> `intents()` treats every unrecognised name as an error -> `reclaim_intents()` cannot proceed, and a retry stages under another unique name that cannot consume the orphan -> the §8 staging protocol has no recovery rule | introduced_by_feature | crash-consistency | be57a33 | fixed in ceec50f25bf9f0939e2307604ef1720ffab366ca: a staging file is never an intent — `intents` ignores the `is_staging_name` shape and `reclaim_intents` removes it under the intent-removal site, a write interrupted before its rename having not been durable; the rule is on `write_intent` and in `containment.rs`; `a_staging_orphan_is_ignored_by_intents_and_removed_by_reclaim` | fixed |
| PR120-HOOKS-PATH-NOT-PROVEN-EMPTY | P1 | 62816dd4f3263e8dd252b009d3b0a6c999e5e9cc / src/workspace_manager.rs:2454 | both checks prove `hooks-none` a real link-free directory and nothing more -> a `Worktree.Add` `Before` hook writes an executable `post-checkout` into the existing directory -> both checks pass -> `git worktree add` executes it; the hostile state exists before both checks | introduced_by_feature | security-trust | be57a33 (the runner's walk), ceec50f (the table's) | fixed in dcb0bddee749d12f1fa7d029cff93513ffbb0f78: `refuse_hooks_entries` reads the directory and refuses any entry as `Refusal::HooksPathNotEmpty`, at the Git runner and in the table's walk; `a_hook_written_into_hooks_none_at_the_before_hook_refuses_the_worktree_add_and_never_runs` | fixed |
| PR120-TABLE-OMITS-GIT-DISCOVERY-PATHS | P1 | 62816dd4f3263e8dd252b009d3b0a6c999e5e9cc / src/workspace_manager.rs:487 | the ref primitives' set is `HooksPath` alone and `base` being a directory does not inspect `base/.git` -> repositories A and B hold the same commit, `create_ref_zero_old` prechecks against A, its `Before` hook renames `A/.git` and plants `A/.git -> B/.git` -> the walk passes, A still a real directory and `hooks-none` unchanged -> `git update-ref` follows the link, creates the ref in B and returns success with A unchanged; the checkout's `.git` pointer, `admin/commondir` and the two commit-tree funnels are omitted the same way, and a test generated from the table cannot see any omission | pre_existing | security-trust | 7a83e69 (the parent's funnel ran Git through those paths before this pull request); ceec50f's table did not name them | deferred to the parent's sweep of `src/workspace_manager.rs` (`standards/SWEEP.md` queue row 11): the table is nine roles and says so in dcb0bddee749d12f1fa7d029cff93513ffbb0f78, its docs naming what it does not cover — the `.git` file or link of the checkout and of the base, and `commondir`, `objects`, `refs`, `packed-refs`, `index` and `config` behind them, and the two commit-tree funnels without a variant — and the durable fix is directory-handle-relative operations or a stated trust boundary for what may write inside the execution root, a design question for the owner (`DESIGN.md` §4, `CODING_STANDARDS.md` §14); `every_path_a_primitive_acts_through_refuses_a_link_planted_at_the_before_hook` pins the table's own size, a regression pin and not a proof | deferred |
| PR120-CANONICAL-PREFIX-PEELS-PAST-A-DANGLING-LINK | P2 | 62816dd4f3263e8dd252b009d3b0a6c999e5e9cc / src/workspace_manager/containment.rs:435 | `canonicalize` answers `NotFound` for `/d/link/child` with `link -> /missing` -> the peel treats it as absence, peels `child` and then the existing `link`, canonicalizes `/d` and rejoins -> `/d/link/child` comes back unchanged, a path through a link that is there; a foreign registration at `/alias/tasks/foreign` with `/alias` pointing at a root not yet created compares as outside before creation and resolves inside after | introduced_by_feature | correctness | 2dd1350 (the peel); e4bf5dc narrowed absence to the two kinds without reading the component | fixed in dcb0bddee749d12f1fa7d029cff93513ffbb0f78: before peeling past a `NotFound`, the head is read with `symlink_metadata` and an existing link refuses as `ReparsePointOnChain`; `canonical_prefix_refuses_a_dangling_link_rather_than_peeling_past_it` | fixed |
| PR120-STAGING-SHAPE-OWNS-WHAT-IT-CANNOT-PROVE | P2 | 62816dd4f3263e8dd252b009d3b0a6c999e5e9cc / src/workspace_manager.rs:3037 | `is_staging_name` accepts any name beginning `.stage-` and ending `.tmp` -> `intents/.stage-report.tmp` is hidden by `intents()` and durably deleted by reclaim -> cleanup infers ownership from a shared filename, against §8; the supplied test used a 24-character interior with an illegal `O` | introduced_by_feature | security-trust | ceec50f | fixed in dcb0bddee749d12f1fa7d029cff93513ffbb0f78: `staging_kind` accepts the exact shape only, `.stage-<kind>-<ULID>.tmp` with one of three kinds and twenty-six Crockford characters, and the orphan tests plant real ULIDs; `a_name_that_merely_resembles_a_staging_file_is_neither_hidden_nor_removed` | fixed |
| PR120-ORPHAN-REMOVED-UNDER-THE-WRONG-SITE | P2 | 62816dd4f3263e8dd252b009d3b0a6c999e5e9cc / src/workspace_manager.rs:1325 | every orphan is removed as `WorktreeSite::RemoveIntent` -> a staging or snapshot intent write that crashed before its rename is reclaimed under R9 instead of `RemoveStagingIntent` (R10) or `SnapshotSite::RemoveIntent` (R24) -> the wrong hooks, fault row, adjacency and accounting fire; the generic name had discarded the kind | introduced_by_feature | correctness | ceec50f | fixed in dcb0bddee749d12f1fa7d029cff93513ffbb0f78: the staging name carries the kind and `reclaim_staging_orphans` removes each under the site of that kind; `a_staging_orphan_of_each_kind_is_removed_under_its_own_site` asserts the harness's recorded sites | fixed |
| PR120-ROOT-REMOVAL-COLLAPSES-SUCCESS-INTO-FAILURE | P2 | 62816dd4f3263e8dd252b009d3b0a6c999e5e9cc / src/workspace_manager.rs:1137 | another remover deletes the empty root between `directory_is_empty` and the final `remove_dir` -> the `NotFound` is mapped to `UpstrokeError::Io` while the helper treats `NotFound` as empty -> a successful removal reports as a failure, and every new removal and write failure reads "failed to read" | introduced_by_feature | correctness | 61398c6 (the modified body); the Io text is 7a83e69's | fixed in dcb0bddee749d12f1fa7d029cff93513ffbb0f78: a `NotFound` from the final `remove_dir` is success, and the removals, creates, writes and renames this pull request added carry their operation in the new `UpstrokeError::Filesystem`, added to `src/error.rs` because no variant named an operation; `a_scaffolding_directory_that_cannot_be_removed_is_reported_not_swallowed` asserts the removal is named | fixed |
| PR120-PARENT-CONTRACTS-OMIT-NEW-OUTCOMES | P3 | 62816dd4f3263e8dd252b009d3b0a6c999e5e9cc / src/workspace_manager.rs:1836 | the ref funnels return containment refusals and I/O errors while their `# Errors` promise ref and Git errors only -> `revalidate_hooks_path` in the runner changes the error surface of `worktree_records`, both commit-tree APIs and every other Git caller -> the body named two contracts where many omit a demonstrated outcome | introduced_by_feature | docs-contract | be57a33 (the runner's walk); e4bf5dc and 61398c6 (the two named before) | deferred by owner direction (P3 and lower recorded): every parent `# Errors` list is rewritten in the parent's sweep, `standards/SWEEP.md` queue row 11; the body names each contract; `a_hook_written_into_hooks_none_at_the_before_hook_refuses_the_worktree_add_and_never_runs` pins one outcome the ref docs omit | deferred |
| PR120-RECLAIM-DELETES-A-LEFTOVER-IT-CANNOT-PROVE-IT-OWNS | P2 | 206d34845185d075a93b87324fd67b6cef01d062 / src/workspace_manager.rs:3132 | `staging_kind` accepts any twenty-six Crockford characters while the generator (`src/ulid.rs:39`) emits only `0` to `7` first -> create `intents/.stage-task-<26 Zs>.tmp` -> `intents()` hides it and `reclaim_staging_orphans` durably deletes a file this writer provably never created, and even a canonical-looking name is no provenance -> `PR120-STAGING-SHAPE-OWNS-WHAT-IT-CANNOT-PROVE` stayed unfixed: cleanup inferring ownership from a shared filename, against §8 | introduced_by_feature | security-trust | ceec50f (the orphan removal); dcb0bdd narrowed the shape and kept the deletion; prior ID PR120-STAGING-SHAPE-OWNS-WHAT-IT-CANNOT-PROVE | fixed in 7f6af205c8e05c298efb71869b09af0af00152a2: this crate deletes no staging leftover; `reclaim_intents` returns `Reclaimed { slots, staging_leftovers }`, reporting every file of the exact shape (the ULID now canonical) and removing none, while `intents()` keeps ignoring the shape and a retried write stages beside it; the rule is stated on `write_intent`; `a_staging_orphan_is_ignored_by_intents_reported_by_reclaim_and_never_removed`, `a_staging_orphan_of_each_kind_is_reported_and_no_removal_site_fires` | fixed |
| PR120-TABLE-OMITS-THE-ORPHAN-REMOVAL-TARGET | P2 | 206d34845185d075a93b87324fd67b6cef01d062 / src/workspace_manager.rs:517 | `ReclaimStagingOrphan` lists `IntentsDirectory` only while the primitive removes the captured orphan path -> enumeration captures `O`, the funnel's `Before` hook replaces `O` with another file or a link, revalidation walks the parent only -> `remove_file(O)` deletes the replacement instead of refusing; the generated test derives its cases from the table and cannot see the omission | introduced_by_feature | security-trust | ceec50f (the primitive); dcb0bdd (its table row) | fixed in 7f6af205c8e05c298efb71869b09af0af00152a2: the primitive and its row are removed together with the deletion itself, so there is no removal target to name; `every_primitive` stays exhaustive over the fourteen variants and the generated test pins 39 cases; `a_staging_orphan_of_each_kind_is_reported_and_no_removal_site_fires` asserts that no removal site fires for a leftover | fixed |
| PR120-OBJECT-LOOKUPS-COLLAPSE-A-REFUSAL-INTO-ABSENCE | P2 | 206d34845185d075a93b87324fd67b6cef01d062 / src/workspace_manager.rs:2461 | `commit_parent` and `commit_tree_sha` call `git_ok(..).ok()` -> the gate's `revalidate()` passes, an entry appears in `hooks-none`, the per-command check refuses `HooksPathNotEmpty` -> `.ok()` discards it and both return `Ok(None)` -> candidate verification reports `ObjectMissing` for a refusal (`src/engine/topology/candidate.rs:1072`); error collapsed into absence, against §7 | pre_existing | correctness | 7a83e69 (the `.ok()` fold); be57a33 added the refusal it swallows | fixed in 7f6af205c8e05c298efb71869b09af0af00152a2 and e793de5c858f64879738ac47063112a88a1b54e6: `quiet_object_lookup` answers `None` only for Git's quiet "no such object" (exit status 1, nothing on stderr) and propagates a refusal, a spawn failure or a Git failure that speaks; the engine caller already used `?` and refused on `None`, so it is unchanged; `a_hook_planted_in_hooks_none_makes_the_object_lookups_refuse_rather_than_answer_none` drives the lookup as the second command of the sequence | fixed |
| PR120-RUN-ID-CASE-VARIANT-ALIASES-A-PEER-ROOT | P2 | 206d34845185d075a93b87324fd67b6cef01d062 / src/workspace_manager.rs:409 | `refuse_unplain_run_id` accepts `RUN1` and `run1` alike while `DESIGN.md` §15 says "run-id = ULID" -> on a case-insensitive filesystem derive a victim as `RUN1` and a second manager as `run1`, both resolving to one root -> the victim's worktrees classify as the second manager's own slots by path shape alone (line 1046) -> the second manager removes the victim's slot | introduced_by_feature | security-trust | e4bf5dc (the plain-component rule) | fixed in 7f6af205c8e05c298efb71869b09af0af00152a2: only the canonical ULID as `crate::ulid::ulid` spells it is accepted (`is_canonical_ulid`: twenty-six uppercase Crockford base32 characters, the first `0` to `7`); every other spelling refuses as `Refusal::RunId` before a path is built, so the classification is unreachable through a non-canonical id; `a_run_id_is_the_canonical_ulid_and_a_case_variant_is_refused` | fixed |
| PR120-STAGING-AND-HOOKS-DOCS-FALSE-AT-THE-HEAD | P3 | 206d34845185d075a93b87324fd67b6cef01d062 / src/workspace_manager.rs:1235 | `write_intent`'s doc says `.stage-<ULID>.tmp`, 37 bytes, while the implementation writes `.stage-<kind>-<ULID>.tmp`, up to 46 -> the body says the recovery rule is stated in `containment.rs`, where it does not appear -> "every Git command" is too broad: `read_only_git` invokes Git without the hooks-path check | introduced_by_feature | docs-contract | dcb0bdd (the kind in the name); be57a33 (the "every Git command" sentence) | fixed as text in 7f6af205c8e05c298efb71869b09af0af00152a2: `write_intent` states the exact shape, the 46-byte bound and the recovery rule, and the body says the rule lives there; `revalidate_hooks_path` names the exact set it guards (every command through `git` and `git_with_identity`) and why `read_only_git` and `read_only_git_ok` are outside it: no manager, no `core.hooksPath`, plumbing that invokes no hook | fixed |

## 48. PR sweep of parsers.rs (2026-09-04)

Append-only. The §6/§7 sweep of `src/workspace_manager/parsers.rs`, row 5 of the
`standards/SWEEP.md` review queue, read line by line against `origin/master` `f458cfc`
by one Claude Fable 5.1 session whose only subject was that file; the cleanup is `196f641`.
The rows below are the sweep's own findings, every location at the base. Under the owner's
rule for the sweep pull requests (2026-09-04: P1 and P2 findings fixed, P3 and lower recorded
as deferred rows) a sweep's own findings are fixed where the fix is the file's to make and
deferred where it is another queue row's; the two deferred here belong to the parent
(`src/workspace_manager.rs`, row 11) and to `worktree.rs` (row 8). Frontier-pass rows are
appended below these as the passes happen, with the pull request's number as their prefix.

The first frontier pass, on `3ef06f2` (gpt-5.6-sol at max, posted 2026-09-04T05:16Z), returned
five findings, every one labelled P2 by the reviewer; under the owner's rule all five are fixed,
in `8cf2d90`. Between pass 1 and pass 2 the coordinator read `8cf2d90` and found two more, the
`PR127-COORD-` rows: the relative join handed out a path with `..` in it, fixed in `59fc2c6`, and
the body's refname claim had to be kept as narrow as the check, fixed as text. Neither is a
frontier pass's finding. Each `fixed` row names the test that holds the repair and the mutation it
was witnessed against on the box: for the pass-1 rows the fix reverted to its `3ef06f2` shape in
the repaired tree, for the coordinator's row the `8cf2d90` tree itself and then the join restored.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| SWEEP-PARSERS-001 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:41 | `trim_ascii` trims both ends and the form feed -> a `gitdir` file holding ` /root/slot/.git` (leading space) or `/root/slot/.git` followed by a form feed, which Git 2.43.0 reads as a relative path and as a path whose name is not `.git` (measured with `git worktree list --porcelain`: `strbuf_rtrim` takes off trailing space, tab, CR and LF only), decodes here to the checkout `/root/slot` -> `registration_admin_for` matches that admin directory for the target and recovery acts on a registration Git does not associate with the checkout | pre_existing | correctness | — | `a_gitdir_is_trimmed_exactly_as_git_trims_it`; `trim_gitdir` takes off trailing space, tab, CR and LF and nothing else. Mutations: the leading trim restored, and the form feed added to the class, each fail that test on its named case | fixed |
| SWEEP-PARSERS-002 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:124 | `.ok()` on `from_utf8` at lines 124 and 130 -> the `Utf8Error` and its `valid_up_to` are discarded -> the refusal says "cannot represent exactly" and not where -> an operator repairing a registration by hand is not told which byte to look at | pre_existing | docs-contract | — | `a_registration_that_is_not_utf8_is_refused_with_its_offset`, on the non-Unix legs: the Windows CI leg is its witness and no mutation of that arm can run on the box. `decode_path` returns the `Utf8Error` and the message carries the offset | fixed |
| SWEEP-PARSERS-003 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:268 | `decode_git_path` on Windows is `from_utf8_lossy` -> a worktree path with bytes Git for Windows does not write but a hostile or corrupt output can carry decodes to a *different* path, `U+FFFD` where the bytes were -> the parent compares, canonicalises and hands that path to removal as identity (`worktree_record`, `record_for`, `revalidate`, `assert_publishable`), while `registration_checkout` refuses the same bytes at line 123 ("lossy path aliases are not registration identity", the suite's own words) | pre_existing | correctness | — | `a_worktree_path_that_is_not_utf8_is_refused_with_its_offset` on the non-Unix legs and `a_worktree_path_keeps_every_byte_on_unix`; one strict `decode_path` per platform family, and `parse_worktree_records` returns `Result` with its two callers propagating | fixed |
| SWEEP-PARSERS-004 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:163 | the `-z` terminator is not required -> `A\0src/x`, a tail cut off before its NUL, is read as the complete record `src/x` -> the region is what the cut-off answer names, which the comment at line 171 says is refused as a truncated record | pre_existing | correctness | — | `changed_path_records_names_the_shape_it_refuses`, cases "a tail without its terminator" and "a second record without its terminator". Mutation: the terminator made optional fails the first case | fixed |
| SWEEP-PARSERS-005 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:165 | the empty-field filter re-aligns the records -> `A\0\0src/x\0` becomes the path `src/x`, a lone NUL becomes an empty diff and `A\0src/a\0\0` a well-formed one -> an answer outside the grammar is admitted as a plausible shorter list, the direction the doc comment says this decoder refuses | pre_existing | correctness | — | the same test, cases "a path that is nothing but a delimiter", "an empty path field" and "a doubled terminator". Mutation: the filter restored fails the first case | fixed |
| SWEEP-PARSERS-006 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:178 | `Err(_)` and the two `return PathSet::RepoWide` at lines 168 and 174 drop each refusal's reason at its source -> the suite's hostile-shape test asserts only `is_repo_wide()` -> a shape refused for the wrong reason (a rename score that is not a number refused as a truncated record, say) passes every test | pre_existing | docs-contract | — | `NameStatusError` names the field at the source and `decode_changed_paths` is the one fold, spelling every variant; `changed_path_records_names_the_shape_it_refuses` asserts the variant per shape, sixteen shapes. Mutations: the fold to an empty `Prefixes`, `Truncated` with a constant record index, and a rename accepted without a score each fail it on its named case | fixed |
| SWEEP-PARSERS-007 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:161 | `decode_changed_paths` returns `RepoWide` for every refusal and nothing records why -> `changed_paths` in the parent hands the merge queue a region that serialises every task -> an operator sees tasks serialise and has no diagnostic naming the undecodable path or the cut-off record | pre_existing | docs-contract | — | deferred to the parent's sweep, queue row 11, which owns `changed_paths`, the layer that knows the slot and the base; `changed_path_records` is the typed reader it can adopt, and the doc comment on `decode_changed_paths` says the reason ends there until the parent decides | deferred |
| SWEEP-PARSERS-008 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:238 | the attributes are read with `from_utf8_lossy` into the `String` fields of `WorktreeRecord` -> `assert_publishable` (`src/workspace_manager.rs:1407` at the base) compares `branch` with a refname as identity, which §8 says a lossy string never is -> a branch named with non-UTF-8 bytes on Unix cannot equal any `&str`, and its lossy spelling equals only a refname that itself contains `U+FFFD`, so the comparison can over-refuse on that collision and never under-refuse | pre_existing | portability | — | deferred: the field types are `worktree.rs`'s (queue row 8) and the comparison is the parent's (row 11); the doc comment on `parse_worktree_records` states what each attribute is read for | deferred |
| SWEEP-PARSERS-009 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:254 | `parse_worktree_records` flushes an unclosed final record, accepts bytes without a terminator and skips an attribute before any record -> a list cut short after `HEAD` is read as a complete record without the `locked` line -> the parent and `residue.rs`, which tell a registered-but-unpopulated worktree by the word `initializing` in that line, read it as populated | pre_existing | correctness | — | `a_worktree_list_cut_short_is_refused_not_read_as_complete`, four shapes. Mutations: the flush restored, the terminator made optional, and the orphan attribute skipped each fail it on its named case | fixed |
| SWEEP-PARSERS-010 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:239 | `trim_end` on a NUL-terminated attribute -> a lock reason's trailing whitespace is removed, where under `-z` Git emits the reason verbatim (measured: Git trims its own `locked` file, so the field carries what was written minus Git's trim) -> a line-oriented trim in a grammar that has no lines | pre_existing | docs-contract | — | `worktree_records_are_read_from_the_porcelain_grammar` pins a lock reason with an embedded newline and a trailing space verbatim. Mutation: the trim restored fails it | fixed |
| SWEEP-PARSERS-011 | P3 | f458cfc6d4470970744c3950f20a5a108ac0d1fe / src/workspace_manager/parsers.rs:196 | the review question of record: `?` on `Option` in `status_endpoints` -> `None` for an empty field -> the only caller reads `None` as "not a status field" and, since this sweep, refuses an empty field by name before asking; no absence becomes failure silently, and the function stays total over every slice | pre_existing | docs-contract | — | the comment at the site; `changed_path_records_names_the_shape_it_refuses` pins the empty-field refusal | rejected |
| PR127-WORKTREE-FRAMING-CLOSES-A-RECORD-AT-THE-NEXT-HEADER | P2 | 3ef06f2272d023d991a8414f8370d8aa5b4f96c2 / src/workspace_manager/parsers.rs:361 | `parse_worktree_records` closes the open record when the next `worktree` field arrives, and at line 345 accepts an empty successful answer -> `worktree /slot\0HEAD abc\0worktree /next\0HEAD def\0\0` reads as two records with no separator between them -> a list whose `locked initializing\0\0` is cut off before the next header gives the first record `locked: None`, and `residue.rs:203` classifies an interrupted add as populated; an empty answer lets `assert_publishable` see no checked-out branch -> the body's "read exactly" and cut-short claims were stronger than the head | pre_existing | correctness | — (the header close and the empty answer are the base's, f458cfc:212) | fixed in 8cf2d907db43402906f970b90476315e2cd1f3aa: a record ends only at the empty attribute; a `worktree` header while a record is open, an empty answer (Git lists at least the repository's own worktree), a separator with nothing open and an empty path each refuse by record number. `a_worktree_list_cut_short_is_refused_not_read_as_complete` ("two records without the separator between them") and `worktree_records_are_read_from_the_porcelain_grammar` ("Git never lists nothing"). Mutations: the header closing the record fails the first on that case; the empty answer accepted fails the second | fixed |
| PR127-STRUCTURAL-ATTRIBUTES-KEEP-FORBIDDEN-WHITESPACE | P2 | 3ef06f2272d023d991a8414f8370d8aa5b4f96c2 / src/workspace_manager/parsers.rs:392 | the sweep removed `trim_end` for every attribute, so whitespace is kept in `HEAD` and `branch` too -> `branch refs/heads/main ` is accepted with its space, which no refname holds -> `assert_publishable` (workspace_manager.rs:1404 at the reviewed head) does not recognise `refs/heads/main` as checked out and permits its compare-and-swap -> an under-refusal outside the lock-reason fix, against the body's "all in the refusing direction" | introduced_by_feature | correctness | 196f641 (the trim removed for every attribute) | fixed in 8cf2d907db43402906f970b90476315e2cd1f3aa: the structural attributes are held to their own grammars, `HEAD` forty or sixty-four hexadecimal digits (`is_object_id`), `branch` a refname's byte set (`can_be_refname`: no control byte, space, DEL or `~^:?*[\`; the `..`, `@{` and `.lock` clauses of `git check-ref-format` are not applied, and the doc says so), `detached` and `bare` carrying no value, none appearing twice; `locked` and `prunable` reasons stay verbatim, and so does the path, of which a space is a legal byte under `-z`. `structural_attributes_refuse_what_their_grammars_forbid`. Mutations: `can_be_refname` reduced to non-empty fails it on "a branch with a trailing space"; `is_object_id` reduced to non-empty on "a HEAD with a trailing space" | fixed |
| PR127-RELATIVE-REGISTRATIONS-STRAND-REMOVAL | P2 | 3ef06f2272d023d991a8414f8370d8aa5b4f96c2 / src/workspace_manager/parsers.rs:112 | Git 2.48's `worktree.useRelativePaths=true` makes `worktree add` write a relative `gitdir`; the manager's add passes no override (workspace_manager.rs:951) and `registration_checkout` refuses every relative path -> the add succeeds and the removal revalidation (workspace_manager.rs:1211), which scans every registration, refuses -> one relative registration, this manager's or a foreign one, strands every managed removal and its intent -> the body's "every file Git writes is unaffected" was too strong | pre_existing | correctness | — (the base refused `..` in every registration, f458cfc:87; the README's Git floor is 2.40) | fixed in 8cf2d907db43402906f970b90476315e2cd1f3aa and 59fc2c6538f8074b8f0d0e639630a70d226275fe: a relative registration is joined to the directory holding the `gitdir` file, as Git's `get_linked_worktree` does before realpath, resolved lexically, and then held to the same containment check as an absolute one; `--no-relative-paths` is not passed, since Git 2.40 has no such flag. `a_relative_registration_is_resolved_against_its_registration_directory`, and in `tests.rs` `a_relative_registration_still_binds_its_checkout` against a real repository (a relative registration binds its slot; one resolving to a foreign directory inside the root refuses). Mutation: every registration treated as absolute fails both, and `a_gitdir_is_trimmed_exactly_as_git_trims_it` on its leading-space case | fixed |
| PR127-RECORD-FOR-COLLAPSES-A-GIT-FAILURE-INTO-ABSENCE | P2 | 3ef06f2272d023d991a8414f8370d8aa5b4f96c2 / src/workspace_manager.rs:2382 | `record_for`, its whole body activated by this pull request's edit under `standards/SWEEP.md`, returns `Ok(None)` for every non-zero `git worktree list` -> the zero-length `commondir` an interrupted add leaves (documented at workspace_manager.rs:2048) makes the enumeration fail -> the residue classifier (`residue.rs:203` and `:327`) reads "not registered" and misses the registered-unpopulated worktree, a subprocess failure read as absence against §7 | pre_existing | correctness | — (the `Ok(None)` is the base's; 196f641 activated the body) | fixed in 8cf2d907db43402906f970b90476315e2cd1f3aa: `read_only_git_ok` propagates the failure carrying Git's stderr, and absence is only the parsed list not naming the worktree; the classifier's two call sites already used `?` and now propagate. `a_failed_worktree_list_is_an_error_not_an_absent_registration`: a zero-length `commondir` makes `record_for` and `classify_object_residue` return `Err`. Mutation: the `Ok(None)` restored fails it ("Git could not enumerate: None") | fixed |
| PR127-CHANGED-PATH-ALIASES-BECOME-NARROW-LEASES | P2 | 3ef06f2272d023d991a8414f8370d8aa5b4f96c2 / src/workspace_manager/parsers.rs:247 | `changed_path_records` checks a path for UTF-8 and non-emptiness only -> `M\0src/./shared.rs\0` becomes the narrow `GitPath("src/./shared.rs")` -> the lease comparator (`topology/leases.rs:99`) compares components literally, `.` is not `shared.rs`, and two owners of one normalised repository path run at once, against the parser's own claim that unsafe input is repo-wide | pre_existing | correctness | — (the base's `decode_changed_paths` admitted the same bytes) | fixed in 8cf2d907db43402906f970b90476315e2cd1f3aa: `is_normalised_repository_path` requires no backslash and no empty, `.` or `..` component, which also refuses an absolute path, a trailing separator and a doubled one; anything else is `NameStatusError::UnsafePath`, and `decode_changed_paths` folds it repo-wide, the parser's stated answer for unsafe input. `a_changed_path_that_is_not_one_normalised_path_is_repo_wide`, seven aliases and a plain path that stays narrow. Mutation: the check reduced to non-empty fails it on "a `.` component" | fixed |
| PR127-COORD-RELATIVE-JOIN-HANDS-OUT-PARENTDIR | P2 | 8cf2d907db43402906f970b90476315e2cd1f3aa / src/workspace_manager/parsers.rs:144 | the relative branch returns `admin.join(recorded)` and the normalisation guard allows `..` there, while `components().collect()` resolves nothing -> a registration of `../../../victim/.git` is handed out as `/repository/.git/worktrees/example/../../../victim` -> both callers pass it through `canonical_prefix`, which peels past an absent prefix and rejoins the remainder textually, so when the checkout does not exist the `..` survives into `is_at_or_inside` and the identity comparison, which compare components literally: the class of finding 5 above and of PR #120's dangling-prefix row | introduced_by_feature | correctness | 8cf2d90 (the join) | fixed in 59fc2c6538f8074b8f0d0e639630a70d226275fe: `resolve_relative` pops one component per `..` and refuses by name a path that would climb above the filesystem root, so the value handed out is always one normalised absolute path and no caller has to canonicalise it; a relative path not normalised in itself refuses too, and the `..` guard now applies to both branches. The coordinator's brief said to refuse a climb above the registration directory; a relative `gitdir` Git writes always climbs above it (`../../../../<checkout>/.git` from `.git/worktrees/<id>/`), so that bound would refuse the form finding 3 admits, and escape from the execution root stays the parent's containment check, now over a normalised value. `a_relative_registration_is_resolved_against_its_registration_directory` (the value has no `..`; four `..` from a four-deep registration reach the root exactly, five refuse by name) and, in `tests.rs`, `a_relative_registration_still_binds_its_checkout`, whose new case writes a registration that climbs above the root to an absent checkout and requires `revalidate_removal` to refuse by name. Witnessed against 8cf2d90: that case fails there with `None`, the absent checkout read as "not this registration"; the join restored in the repaired tree fails the parsers test and the `tests.rs` test | fixed |
| PR127-COORD-REFNAME-CLAIM-WIDER-THAN-THE-CHECK | P3 | 8cf2d907db43402906f970b90476315e2cd1f3aa / src/workspace_manager/parsers.rs:396 | `can_be_refname` applies the byte-set clauses of `git check-ref-format` and its doc says the `..`, `@{` and `.lock` clauses are not applied -> a body sentence calling `branch` "a valid refname" would claim more than the head checks -> the reviewer measures the body against the head, and the claim would be the next finding | introduced_by_feature | docs-contract | 8cf2d90 (the check and its doc) | fixed as text in the pull request body: the claim is exactly the doc's, and the body says why the unapplied clauses cannot produce finding 2's harm, which is a branch Git itself checked out failing to match its own name: Git never writes a `branch` field spelled with `..`, `@{` or `.lock`, and what a stray or hostile byte can add to Git's spelling is a byte, which the byte set refuses; the check does not tell a well-formed refname from a corrupted well-formed one, and does not claim to | fixed |

## 49. PR #125 closed after eight frontier passes (2026-09-04)

Append-only. PR #125 (`fix/darwin-helper-ready-budget`, head `33604e6` at closure) set out to fix
master's macOS test-leg failure "Unix cleanup reaper did not initialize" by raising the forked
helpers' READY budget from two seconds to ten. Eight frontier passes (`gpt-5.6-sol` at `max`)
each found a new P1 in code the pull request added:
pass 1 at `b058661` https://github.com/eventloops/upstroke/pull/125#issuecomment-5533118669,
pass 2 at `102c27e` https://github.com/eventloops/upstroke/pull/125#issuecomment-5533321264,
pass 3 at `aa7699d` https://github.com/eventloops/upstroke/pull/125#issuecomment-5533611903,
pass 4 at `ecc9aa1` https://github.com/eventloops/upstroke/pull/125#issuecomment-5534857355,
pass 5 at `8841045` https://github.com/eventloops/upstroke/pull/125#issuecomment-5535670541,
pass 6 at `de69832` https://github.com/eventloops/upstroke/pull/125#issuecomment-5536270335,
pass 7 at `1428112` https://github.com/eventloops/upstroke/pull/125#issuecomment-5539395887,
pass 8 at `33604e6` https://github.com/eventloops/upstroke/pull/125#issuecomment-5539824458.
The budget increase was withdrawn as a fix at pass 7 (the exact head `ecc9aa1` had failed with
the budget elapsed, run 33821116191, and the cause was never established), and the pull request
was narrowed to a READY failure diagnostic and a bounded, ownership-checked end of a helper; pass
8 found two P1s in that kept code, and the coordinator, under the owner's written delegation for
the pull request, applied the stopping rule set after pass 6 and closed it rather than open a
ninth round. No code from the pull request is merged. These rows preserve what the eight passes
learned so that none of it is rediscovered; every one is `deferred`, and its guard column is the
proposal it carries for the change that takes it up. Rows bound to `0bff83d` name the defect on
master, which the pull request did not introduce and did not merge a fix for; rows bound to
`33604e6` name a defect in the closed pull request's own attempt, reachable at the pull request,
recorded so the next attempt does not repeat it.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR125-CLOSE-UNBOUNDED-KILL-AND-WAIT-AT-FIVE-SITES | P1 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/agent/proc.rs:2325 | five sites kill a forked helper and then block in `waitpid(pid, 0)` without a bound: `Reaper::abandon` through `close_and_wait`, the `setpgid` failure in `spawn_reaper`, `Guard::abort_setup`, and the descriptor-configuration and READY failures in `spawn_guard` -> the helper is in uninterruptible I/O (a stalled `open` of a cleanup lease or a `close` of an inherited descriptor) with SIGKILL pending -> the reaper's callers hold the launch barrier, under which the signal monitor refuses to kill or stop any registered group, so every running agent outlives a SIGTERM for as long as the kernel takes; the guard's callers hold supervisor initialisation | pre_existing | liveness | 6798089 (the barrier and the sites) | deferred: a bounded end of a helper, asking `waitpid(pid, WNOHANG)` first and then polling to a short named budget, after which a child still alive is left for the process's exit to collect; it must report only what `waitpid` and `kill` actually returned (row PR125-CLOSE-DISCARDED-KILL-RESULT) and must not claim the pid's identity from a `WNOHANG` zero (row PR125-CLOSE-PID-IDENTITY-UNDER-A-HOST-WILDCARD-WAITER); the acknowledged-exit wait in `close_and_wait` after CLEANUP or CANCEL stays unbounded, because the reaper's exit is what releases the cleanup lease the caller depends on; the closed pull request's `kill_and_reap_helper` at its `33604e6` is a starting shape with those two defects | deferred |
| PR125-CLOSE-PID-IDENTITY-UNDER-A-HOST-WILDCARD-WAITER | P1 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/agent/proc.rs:2320 | a helper is signalled by pid at every site above, and any diagnostic read of it (state, open descriptors) would be by pid too -> the library anticipates an embedding host whose SIGCHLD handler reaps with wildcard waits (the comment at src/agent/proc.rs:2487 on master; `install_reaper_dispositions` scrubs that handler from the reaper for exactly this reason) -> the helper dies before READY, the host's handler reaps it, the kernel reuses the number for another host fork, and the signal or the snapshot lands on that process; a `waitpid(pid, WNOHANG)` answering zero proves only that the number names some unreaped child of this process, not the helper, and no source census can see host code | pre_existing | correctness | 6798089 | deferred: this is a design decision for the owner, not a repair round; the durable fix on Linux is `pidfd_open` at fork time, signalling through `pidfd_send_signal` and waiting through `poll` on the descriptor, which cannot name a reused pid and gives the bounded wait above for free; on Darwin and the other hosts there is no equivalent, so the design must state a trust boundary, either that an embedding host may not reap this process's children with wildcard waits while a helper is being started or ended, or that a helper's end is best-effort there and says so; the closed pull request's ownership proof inventoried only this crate, which the pass-8 verdict shows is not enough | deferred |
| PR125-CLOSE-DISCARDED-KILL-RESULT | P1 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/agent/proc.rs:2320 | every one of the five sites writes `let _ = libc::kill(pid, libc::SIGKILL)` -> a sandbox or LSM answers EPERM, or an ESRCH race lands -> the helper is not signalled and may stay in its pre-READY `open` or `close` holding its cleanup lease, while the caller proceeds as if it were dead; a bounded end that then reports "sent SIGKILL" and "left running with the signal pending" invents a history that was not observed, which is what pass 8's second P1 found in the closed pull request | pre_existing | correctness | 6798089 | deferred: the end of a helper reports what `kill` returned (0, ESRCH, EPERM) and what each `waitpid` returned, and nothing else; a `kill` that failed is a distinct outcome the READY failure message carries, and §7 forbids discarding the result of a signal the caller depends on | deferred |
| PR125-CLOSE-MACOS-READY-RED-CAUSE-UNKNOWN | P1 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/agent/proc.rs:2461 | master's macOS test leg fails "Unix cleanup reaper did not initialize" (2026-09-03: `2de71dd`, `17d41c9`, `ae2a58f`; PR #125's `ecc9aa1`, run 33821116191) -> the parent forks the reaper, which scrubs dispositions, `setpgid`s, closes every descriptor number up to the ceiling one `close` at a time on Darwin, takes each cleanup lease with an `open` and a non-blocking `flock`, and writes READY -> nothing arrives within the budget, and the launch fails with no other information; at a ten-second budget the exact head failed with "waited 10.000190708s of 10s; descriptor ceiling 10240", so the parent polled on time and the child was silent for ten seconds, which the ordinary cost of its work (milliseconds) does not explain | undetermined | liveness | W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT is the other macOS fingerprint of the same period, not this one; first sighting of this one 2026-09-03 on master | deferred: the cause is not established and a bigger budget did not help at the exact head, so the next change is a diagnostic, not a budget: on a READY failure, read what the parent can see of the child before anything is closed or killed (state from `/proc` on Linux and `proc_pidinfo` on Darwin, open-descriptor count where the host answers, a failed or short query reported as unknown and an `Err` directory entry as unreadable, never as a count), carry `open_max` and the elapsed wait, and do it for the guard as well as the reaper; the closed pull request's `helper_snapshot` at `33604e6` is the shape, minus the two defects in rows PR125-CLOSE-GUARD-TEARDOWN-BEFORE-THE-SNAPSHOT and PR125-CLOSE-READDIR-ERRORS-COUNTED-AS-DESCRIPTORS; what is established: the child's `open` and `close` can block, and a parent that polls on time measures nothing about the child's scheduling | deferred |
| PR125-CLOSE-GUARD-TEARDOWN-BEFORE-THE-SNAPSHOT | P2 | 33604e648aa06fdd0551526b3b8f95d3676df7ae / src/agent/proc.rs:3234 | on a guard READY failure the closed pull request closed the command and acknowledgement descriptors before reading `helper_snapshot` -> a slow guard reaching its READY write just then finds no reader, takes EPIPE and exits -> the snapshot reports "exited and not yet reaped" for a guard that was alive at the deadline and was killed by the diagnostic's own teardown; the guard's message also omitted `open_max` while the body claimed both messages carried it | introduced_by_feature | correctness | 77be7c3 | deferred: the snapshot is read before any descriptor is closed, at both helpers, and both messages carry `open_max`; a test drives the ordering with a helper that writes READY at the deadline | deferred |
| PR125-CLOSE-READDIR-ERRORS-COUNTED-AS-DESCRIPTORS | P2 | 33604e648aa06fdd0551526b3b8f95d3676df7ae / src/agent/proc.rs:3940 | the Linux descriptor reader counted `read_dir` entries with `entries.count()` -> `ReadDir` yields `Result<DirEntry>`, so an entry that errors is counted as a descriptor -> the stated contract, a failed query is "not readable" and never a count, did not hold for a partially readable table | introduced_by_feature | correctness | d530899 | deferred: an `Err` entry makes the whole count "not readable"; the test that proves the reader has an arm for it | deferred |
| PR125-CLOSE-SOURCE-CENSUS-BELOW-THE-STANDARD | P2 | 33604e648aa06fdd0551526b3b8f95d3676df7ae / src/agent/proc.rs:5809 | the closed pull request pinned "no production wildcard wait in this crate" with a test that matched two literal substrings in raw source text with `#[cfg(test)]` blocks stripped -> it cannot see `waitid(P_ALL, ..)`, an imported `waitpid`, or a `-1` held in a variable, it did not blank comments or literals, and it carried no positive control that injects a violation into the whole domain -> §12's census requirements were not met, and no census of this crate can see an embedding host's code, so it could not carry the identity proof it was written for | introduced_by_feature | docs-contract | 77be7c3 | deferred: if a source census is ever written for this, it blanks comments and every literal kind before matching, proves the blanker on a fixture, asserts the size and boundaries of its domain, recognises every spelling of a wildcard wait, and carries a positive control that injects one violation and sees the expected failure (standards/12_standards_tests.md); and it is evidence about this crate only, never about a host | deferred |
| PR125-CLOSE-SCHEDULER-BOUND-TIMING-TESTS | P2 | 33604e648aa06fdd0551526b3b8f95d3676df7ae / src/agent/proc.rs:5669 | the closed pull request's tests of the bounded end required wall-clock time under the budget (a killed child reaped inside 500 ms) or under the budget plus a second (a child left running), measured across thread creation, and discarded a worker's `JoinHandle` -> a test thread descheduled for that long fails correct code, and an earlier test of the same class had already failed on a loaded macOS runner at `ee0e914` (run 33861839254: 134.7 ms past a 100 ms bound) | introduced_by_feature | docs-contract | 6da790e | deferred: tests of a bounded wait observe the mechanism of the bound (the number of looks taken, or a clock the test controls), not wall time, and join every worker they spawn; §12's rule that concurrency tests do not depend on scheduling luck | deferred |
| PR125-CLOSE-GROUP-SCANNER-RETRIES-UNDER-THE-BARRIER | P2 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/agent/proc.rs:2341 | `spawn_reaper` calls `verify_group_scanner` under the launch barrier, before the reaper's READY wait -> the scanner retries for up to two seconds without looking at a pending termination -> a running group outlives a SIGTERM by that interval while the launch holds the barrier; found by pass 7 while the READY wait was interruptible and left as master has it when that was withdrawn | pre_existing | liveness | 6798089 (the barrier); the scanner's retry is older | deferred: named for the file's owner; any change that makes the READY wait give way to a termination must cover this retry too, or the coverage claim is false | deferred |
| PR125-CLOSE-GROUP-ORACLE-CANNOT-SEE-A-ZOMBIE-ON-DARWIN | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/agent/proc.rs:506 | `child_leads_its_own_group` asks `getpgid(pid) == pid` after the spawn returns and its doc says a zombie still answers -> on XNU `proc_find` excludes exited processes, so a shim child that exits before the parent's look answers ESRCH -> `every_role_reaches_the_containment_points_of_this_platform` fails "the child did not lead its own process group" on macOS, most often on the role whose child does the least work; a hypothesis for the standing row, untested without a Mac | pre_existing | portability | W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT; PR7-MACOS-PROCESS-GROUP-FLAKE | deferred: one experiment on a Mac decides it (fork a child that exits at once, do not reap it, call `getpgid` on it); if it holds, the oracle needs a zombie-aware query, as `group_has_non_zombie_members` already passes the non-zero argument to `proc_pidinfo` for | deferred |
| PR125-CLOSE-BODY-ACCOUNTING-AND-TITLE | P3 | 33604e648aa06fdd0551526b3b8f95d3676df7ae / src/agent/proc.rs:1451 | the closed pull request's title still claimed a load-tolerant READY budget after the budget was withdrawn, and its body counted the child's pre-READY work as "eight `sigaction`s" -> `scrub_private_helper_dispositions` attempts `sigaction` for every signal from 1 through 128 except SIGKILL and SIGSTOP, 126 calls, before the explicit setup -> a body's accounting of a child's work is a claim a reviewer checks against the code, and a title is retitled when the change it names is withdrawn | introduced_by_feature | docs-contract | 77be7c3 | deferred: recorded so the next attempt counts the calls from the code and retitles on a narrowing | deferred |

## 51. PR sweep of worktree.rs (2026-09-04)

Append-only. The §6/§7 sweep of `src/workspace_manager/worktree.rs`, row 8 of the
`standards/SWEEP.md` review queue, read against `origin/master` `0bff83d` (the merge of PR #127,
whose parser is the record's one producer) by one Claude Fable 5.1 session whose only subject was
that file; the cleanup is `5e60c5e`, and the producer's forced edit in parsers.rs is `219e9e0`.
The file has no `?`, no lock, no shared ownership and no `clone()` call, so the rows below are the
four readings the owner's amendment 7 asks for: correctness, the shape, the standards and the
tests. Under the owner's rule for the sweep pull requests (2026-09-04: P1 and P2 findings fixed,
P3 and lower recorded) every row is P3; the ones the file can repair are fixed in the same
commits, and the ones another file owns are deferred with the file named. sweep_coordinator's
rule of 2026-09-04 for this branch: parsers.rs (PR #127, merged) may move with the record, and
residue.rs (PR #128, open) may not, so the `locked` field stays readable by the parent module
until the base merge-in that follows #128. Frontier-pass rows are appended below these as the
passes happen, with the pull request's number as their prefix.

The first frontier pass, on `ea4fd74` (gpt-5.6-sol at max, posted 2026-09-04T11:15Z), returned
CHANGES_REQUIRED with five unlabelled findings; sweep_coordinator classed 1, 2 and 3 P2 and 4 and
5 text at P3, all fixed. Finding 1, a user's `git worktree lock --reason initializing` read as
Git's own marker, is `SWEEP-WORKTREE-013` below: the engine's add funnel passes no lock of its
own, so the marker is Git's and a marker only the engine writes is the parent's to add (row 11,
the proposal in the row); the doc at `5e19848` no longer calls it an invariant. Findings 2 and 3
are repaired in the same commit by `OpenRecord`, which applies every rule of the record as the
parser feeds it each attribute, and the sweep-own rows above that name `from_porcelain` describe
`5e60c5e`; at `5e19848` the constructor they describe is `OpenRecord::at` and its readers, and
`close`, and the tests they name are the same tests, two of them renamed with `_read_once`.
`SWEEP-WORKTREE-012`, a sighting when first written, is rewritten with the mechanism the repair
round measured: a second occurrence with the same `Worktree.Add` histogram, and a hand-built
registration whose zero-length `commondir` makes `git worktree list` exit 128, which #127's
`record_for` repair turned from absence into the `Err` the sampler counts as unclassified.

The second frontier pass, on `c0eb8c5` (gpt-5.6-sol at max, posted 2026-09-04T14:49Z), returned
CHANGES_REQUIRED with four unlabelled findings; sweep_coordinator classed 1, 2 and 3 P2 and 4
text at P3, and all four are fixed in `1720d91`. Finding 3 is the substantial one: the record
kept `detached` and `bare` for duplicate detection only, so a record with a HEAD and no branch
was accepted and `assert_publishable` read its `branch: None` as "no branch is checked out
here". The shape rule that replaces it is what Git 2.43.0 was measured printing, not what its
documentation says. The rows below name the pass-2 findings; `SWEEP-WORKTREE-014` is the
tree-wide citation question finding 2 uncovered, and the rows above that name
`MalformedAttribute` describe `5e19848`, whose type is `MalformedRecord` from `1720d91` on,
since it refuses records and not only attributes.

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| SWEEP-WORKTREE-001 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/worktree.rs:35 | `WorktreeRecord` has public `String` fields for `head` and `branch` -> the grammar (`HEAD` an object id, `branch` a refname's byte set) lives in `parse_worktree_records` alone and the value accepts any string -> a second producer, or a test building one by hand, holds no invariant the type states: the shape PR #118 found in naming.rs, private fields behind a validated constructor being the accepted repair | pre_existing | correctness | — | fixed at 5e60c5ef25da9b2ae10df12cfc07fdf448a24dc8: the fields are private and the record is built one way (at 5e60c5e `from_porcelain`; at 5e19848 `OpenRecord`, whose readers apply object.rs's `is_object_id` to `HEAD` and `can_be_refname`, moved here from parsers.rs, to `branch` as each attribute arrives), with accessors for every reader; the producer moved with it at 219e9e0d7523abba13cb37863fe7fcbec18b7bef. `a_head_is_a_full_hexadecimal_object_id_of_either_length_read_once` (three accepted, eight refused shapes, a second HEAD) and `a_branch_is_inside_the_refname_byte_set_read_once_and_kept_as_bytes` (each of the 41 forbidden bytes, the empty name, a second branch). Mutations: the length check dropped from the head grammar fails the first and the parsers' `structural_attributes_refuse_what_their_grammars_forbid` on `HEAD abc`; the byte set reduced to non-empty fails the second and the same parsers test on the trailing space | fixed |
| SWEEP-WORKTREE-002 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/worktree.rs:41 | `branch` is `from_utf8_lossy` into a `String` -> `assert_publishable` compares it with a `&str` refname as identity -> on Unix a branch spelled with bytes that are not UTF-8 equals no refname but does equal its own `U+FFFD` spelling, so the check over-refuses on that one collision and can never under-refuse (`SWEEP-PARSERS-008`, whose guard named this file) | pre_existing | portability | SWEEP-PARSERS-008 | fixed at 5e60c5ef25da9b2ae10df12cfc07fdf448a24dc8: `branch` is the bytes Git printed and `has_checked_out` compares them with the refname's UTF-8, the parent's one comparison moved to it; `checked_out_is_byte_equality_with_the_full_refname`. Mutations: the comparison made lossy fails it on the `caf\u{FFFD}` case; the constructor requiring UTF-8 of a branch fails it and `a_branch_is_inside_the_refname_byte_set_read_once_and_kept_as_bytes` | fixed |
| SWEEP-WORKTREE-003 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/worktree.rs:46 | `locked: Option<String>` is documented as "Git's own lock reason" with no statement of what `None` and `Some("")` are -> a lock taken without a reason lists as `locked` with no value (measured, Git 2.43.0: `git worktree lock` lists `locked`; `--reason "why  "` lists `locked why`) and reads as `Some("")` -> the word `initializing` is spelled by three consumers (the parent's `quiescence`, residue.rs twice) and nothing in the type says a bare lock is not it | pre_existing | docs-contract | — | fixed at 5e60c5ef25da9b2ae10df12cfc07fdf448a24dc8: `lock_reason` and `prunable_reason` state the three shapes and `is_initializing` is the one spelling, adopted by the parent's `quiescence` and by tests.rs; `a_bare_lock_is_a_lock_without_a_reason_and_only_initializing_is_initializing`. Mutations: the bare lock mapped to `None` fails it and the parsers' `worktree_records_are_read_from_the_porcelain_grammar`; `starts_with` in place of equality fails it on the trailing-space case; the prunable reason read in place of the lock fails it and `reclaim_removes_a_registered_but_unpopulated_worktree` in tests.rs | fixed |
| SWEEP-WORKTREE-004 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/residue.rs:206 | residue.rs compares `record.locked` with `initializing` at two sites (206 and 329) -> that file is PR #128's, queue row 6, in flight, and not this sweep's to edit -> the field stays `pub(super)`, readable by the parent module, until those lines move, so the record is not yet private in full | pre_existing | docs-contract | SWEEP-WORKTREE-003 | deferred on sweep_coordinator's rule of 2026-09-04: the doc comment on the `locked` field says why it is readable and what moves; the two lines become `is_initializing` and the field goes private in the base merge-in that follows #128, which the coordinator calls | deferred |
| SWEEP-WORKTREE-005 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/worktree.rs:34 | `Clone` is derived on a record owning a `PathBuf` and four heap strings, and nothing clones one -> §6 asks who copies a value and why; the parent iterates the list by value and moves the path into its refusal | pre_existing | docs-contract | — | fixed at 5e60c5ef25da9b2ae10df12cfc07fdf448a24dc8: `Clone` dropped from `WorktreeRecord`, `into_path` being the move; kept on `VerifyFailure` (the engine's `Reuse` derives it and the settle double answers one recorded failure to every caller) and on `Quiescence` (the double records every question asked), the reason on each type. Guard: a derive with no caller to satisfy, so the tree compiling without it is the check, and a caller that needs one says so at the type | fixed |
| SWEEP-WORKTREE-006 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/worktree.rs:109 | `HeadMismatch` displays as `HEAD is …` -> §7's Display text starts lowercase so a report chain reads `…: the worktree's HEAD is …` -> the one arm of seven starting uppercase | pre_existing | docs-contract | — | fixed at 5e60c5ef25da9b2ae10df12cfc07fdf448a24dc8: reworded; `every_verify_failure_displays_as_a_lowercase_fragment_carrying_its_fields` pins every arm (an exhaustive match, so a new variant does not compile until it is listed) lowercase and without a trailing period, `expected` and `actual` in their places, and the residue element named. Mutations: the two swapped, and a period added to `Missing`, each fail it | fixed |
| SWEEP-WORKTREE-007 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/engine/topology/run.rs:1338 | `VerifyFailure`'s `Display` is rendered by no production caller -> `RetryOutcome::Close { closed, .. }` drops the failure at this site and `Reuse::Recreated` carries one that only the dispatch tests read -> an operator sees a retained generation close as `WorktreeMissing`, or a worktree rebuilt, and is never told which of the seven observations it was: the message this file writes for a caller to act on reaches no one | pre_existing | correctness | — | deferred to the engine, which is not on this queue and outside this sweep's bound: `run.rs` should carry the failure's text into the `generation_closed` record's detail or the run's log at the site that drops it, and the dispatch path should do the same for `Recreated`. The `VerifyFailure` doc states the contract the engine owes: the variant and its `Display` are what an operator is told afterwards | deferred |
| SWEEP-WORKTREE-008 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/worktree.rs:118 | `Residue(element)` displays `{element:?}`, the Debug spelling of a closed enum (`IndexLock`), in an operator message -> `ResidueElement` has serde `snake_case` names and no `Display` -> the message's vocabulary is the derive's, not a chosen one | pre_existing | docs-contract | — | deferred to `src/topology/effects/residue_authority.rs` (queue row 24), which owns the vocabulary: a `Display` matching its serde spelling, adopted here. `every_verify_failure_displays_as_a_lowercase_fragment_carrying_its_fields` pins that the text ends with the element's Debug name, so the adoption is one assertion's change | deferred |
| SWEEP-WORKTREE-009 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/worktree.rs:130 | `Quiescence::AtBase`, `HoldsTree`, `HeadMismatch` and `TreeMismatch` carry object ids as bare `String`, and `TreeMismatch::difference` carries the differing paths as one lossy `String` the parent's `index_differs_from` builds -> §5 asks for a dedicated identifier type where a mix-up is possible (a base and a tree are both hex) and §8 for paths as `Path` values -> a public API change reaching the engine's `dispatch.rs`, `settle.rs` and their tests, outside this sweep's bound | pre_existing | docs-contract | — | deferred: a validated object-id type belongs to `object.rs`, which owns `is_object_id` (the engine's `CommitSha` is the unvalidated spelling of the same thing), and `difference` as a typed list of paths or a reason belongs to the parent's `index_differs_from` (row 11); the field's doc comment on `difference` states that the string is a diagnostic | deferred |
| SWEEP-WORKTREE-010 | P3 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/worktree.rs:1 | the module has no tests of its own -> the record's grammar and the Display contract are exercised only through the parser's and the parent's suites -> a mutation of the value's own rules is caught, if at all, by a file whose subject is something else | pre_existing | docs-contract | — | fixed at 5e60c5ef25da9b2ae10df12cfc07fdf448a24dc8: seven tests in the file; nine mutations run on the box against the committed tree, each killed by the test rows 001, 002, 003 and 006 name | fixed |
| SWEEP-WORKTREE-011 | P3 | 219e9e0d7523abba13cb37863fe7fcbec18b7bef / src/workspace_manager/parsers.rs:1029 | the parsers test's expected records are built through `from_porcelain` -> the oracle for the reason grammar moved with the constructor: with a bare lock mapped to `None`, `worktree_records_are_read_from_the_porcelain_grammar` stayed green because its expected record was built by the same mutated code -> a test whose oracle is the implementation under test, for that one rule | introduced_by_feature | docs-contract | — | fixed at 219e9e0d7523abba13cb37863fe7fcbec18b7bef: the test asserts `lock_reason` and `prunable_reason` of the bare record directly, at this location (the fixtures go through `OpenRecord` since 5e19848, and the assertion stands). Witnessed: the bare-lock mutation run again fails it | fixed |
| SWEEP-WORKTREE-012 | P2 | 0bff83dfa632b80a0373202613f37cce222410f9 / src/workspace_manager/tests.rs:6777 | two runs of `sampled_git_child_kills_every_residue_classified_and_recovered` on the box out of twenty-six, both under a mutation of this module the classifier cannot reach on Git's own output, failed `every sample is accounted for by exactly one class` with 7 of 8, both with the `Worktree.Add` site reading `n=8 none=1 internal=6 after=0 unclassified=1` -> an unclassified sample is `classify_object_residue` returning `Err` (`tests.rs:7216`, `.ok()`), and since PR #127's `record_for` repair (`8cf2d90`) a non-zero `git worktree list` is an `Err` where it was `Ok(None)` -> measured on the box (Git 2.43.0) over hand-built registrations: an admin directory with only `gitdir`, or with `gitdir` and `commondir` and no `HEAD`, lists fine (`HEAD 0000…`, `detached`, `prunable gitdir file points to non-existent location`, inside the grammar), and a zero-length `commondir` alone exits 128 with `fatal: failed to read .git/worktrees/x/commondir: Success` -> a `git worktree add` killed in the window between creating `commondir` and writing it leaves the one registration the classifier now answers with `Err`, the sampler counts it unclassified, and the test's own reading of that count is "durable state no tabled action recovers" | fix_regression | correctness | PR127-RECORD-FOR-COLLAPSES-A-GIT-FAILURE-INTO-ABSENCE | deferred, not this file's: the residue classifier (residue.rs, row 6, PR #128 open) or the parent's `record_for` (row 11) decides that a `worktree list` failing to read a registration's `commondir` is Git's own interrupted-add residue and classifies it (`RegisteredUnpopulatedWorktree`, `Internal`) instead of erroring, and the sampler then accounts for it; until then the fingerprint is the `Worktree.Add` histogram with `unclassified=1` at the histogram-total assertion, 2 in 26 on the box, not yet seen in CI (three green runs of this pull request and #127's runs) | deferred |
| SWEEP-WORKTREE-013 | P2 | ea4fd748236917e10eab916c3266d2ce976672f6 / src/workspace_manager/worktree.rs:226 | `is_initializing` compares the lock reason with Git's word and the doc at the reviewed head called it an invariant -> `git worktree lock --reason initializing <path>` on a populated worktree lists `locked initializing`, the helper answers `true`, `quiescence` answers `Unpopulated`, and an open generation is removed with force and re-added while a retained one is closed -> a user-controlled reason read as provenance; the behaviour is the base's (`workspace_manager.rs:1601` at `0bff83d` compared the same string), and the reviewed head certified it (pass-1 finding 1) | pre_existing | correctness | — | deferred to the parent's sweep, queue row 11, with the proposal: the add funnel (`WORKTREE_ADD_ARGV`, `worktree add --detach --quiet`, no lock of its own) passes `--lock --reason` with a token only the engine writes (its prefix, the run id and the slot), so a populated engine worktree is always locked with the engine's token, `git worktree lock` refuses a second lock, and a reason that is neither the token nor Git's `initializing` is a distinct refusal; that reaches forced removal (`--force --force` for a locked worktree), `worktree prune`'s skip of locked entries and the residue classifier's after-reference, which is why it is the parent's. At 5e19848d3eae6b16d96c2786eaf708282aba7fa1 the doc says what is true: the record cannot tell Git's word from a writer's, both read `true`, and a writer to the execution root who forges it is inside the trust boundary the root assumes (§14, the same writer can delete the checkout). `a_bare_lock_is_a_lock_without_a_reason_and_only_initializing_is_initializing` pins the documented reading; the token's two-case test (bare word not read as initializing, the token read as such) belongs to row 11 with the funnel | deferred |
| PR131-DETACHED-AND-BARE-ACCEPTED-TWICE | P2 | ea4fd748236917e10eab916c3266d2ce976672f6 / src/workspace_manager/parsers.rs:295 | the doc says none of the four structural attributes appears twice, and the `detached`/`bare` arm records no seen state while the constructor never sees them -> `worktree /repo\0HEAD <sha>\0detached\0detached\0\0` is accepted -> the parser's stated grammar is not the one it applies, in code this pull request rewrote (pass-1 finding 2; the arm is the base's, `parsers.rs:557` at `0bff83d`) | pre_existing | correctness | — | fixed at 5e19848d3eae6b16d96c2786eaf708282aba7fa1: `OpenRecord::detached` and `OpenRecord::bare` refuse a second reading as `MalformedAttribute::BooleanTwice`, "has a boolean attribute twice", by record number like the other attributes. `a_boolean_attribute_is_read_once` in worktree.rs; the reviewer's record and `bare` twice in the parsers' `structural_attributes_refuse_what_their_grammars_forbid`. Mutation: the `detached` check removed fails both | fixed |
| PR131-CLOSURE-TIME-VALIDATION-CHANGES-THE-DIAGNOSTIC | P2 | ea4fd748236917e10eab916c3266d2ce976672f6 / src/workspace_manager/parsers.rs:288 | `from_porcelain` validated at record closure, `HEAD` before `branch` -> a record with an invalid `branch` before an invalid `HEAD` was refused for the HEAD where the base refused it for the branch -> a behaviour change the body disclosed only for malformed-plus-framing, and a claim of same messages that did not hold (pass-1 finding 3) | introduced_by_feature | docs-contract | 5e60c5e (the constructor at closure) | fixed at 5e19848d3eae6b16d96c2786eaf708282aba7fa1: `OpenRecord` applies every rule as the attribute is fed to it, so the refusal names the first attribute outside the grammar in attribute order, as the base did; `close` cannot fail. `a_record_with_two_malformed_attributes_is_refused_for_the_first_read` in the parsers module, both orders. Witnessed: the test planted in the `ea4fd74` tree fails on "the branch came first"; at 5e19848d3eae6b16d96c2786eaf708282aba7fa1 the parser's refusal ignored fails it and the grammar tests | fixed |
| PR131-DISPLAY-DOC-PROMISES-AN-OPERATOR-CONTRACT | P3 | ea4fd748236917e10eab916c3266d2ce976672f6 / src/workspace_manager/worktree.rs:243 | the `VerifyFailure` doc said the variant and its `Display` are what an operator is told afterwards -> no production caller renders the `Display` (`SWEEP-WORKTREE-007`) -> a living behavioural contract stated in rustdoc, outside `DESIGN.md`, for behaviour the tree does not have (pass-1 finding 4) | introduced_by_feature | docs-contract | 5e60c5e (the sentence) | fixed as text at 5e19848d3eae6b16d96c2786eaf708282aba7fa1: the doc says the variant is what a caller reads today (`Reuse::Recreated`, `RetryOutcome::Close`), that nothing renders the `Display` to an operator yet, and that each arm carries what it compared so that a renderer has it; the obligation stays the engine's deferred row | fixed |
| PR131-BODY-COUNTS-AND-DERIVES-INEXACT | P3 | ea4fd748236917e10eab916c3266d2ce976672f6 / src/workspace_manager/worktree.rs:103 | the body said eight fixed and four deferred (seven and five), "every reader moved to an accessor" while residue.rs reads the field twice by the coordinator's ruling, five accessor sites in workspace_manager/tests.rs and four in attempt/tests.rs (six and three), and `MalformedAttribute` carried `Clone + Copy` with no caller while the same pull request removed `Clone` elsewhere for that reason -> the reviewer measures the body against the head and each of the four is a claim the head does not support (pass-1 finding 5) | introduced_by_feature | docs-contract | 5e60c5e (the derive); ea4fd74 (the body) | fixed: the derives dropped at 5e19848d3eae6b16d96c2786eaf708282aba7fa1 (`MalformedAttribute` derives `Debug`, `PartialEq`, `Eq` and `Error`, the three the parser's join and the tests use); the counts, the reader sentence and the site counts corrected in the body, every count of which is now derived from the diff at the head | fixed |
| PR131-P2-UNPOPULATED-DISPLAY-CLAIMS-A-HISTORY | P2 | c0eb8c5d35517b9402f7ddd5dfc46eb0c8c9ee42 / src/workspace_manager/worktree.rs:426 | `is_initializing`'s doc admits the record cannot tell Git's `initializing` from a writer's, while `VerifyFailure::Unpopulated`'s `Display` still said the worktree "was never populated" and that `git worktree add` still holds the lock -> `git worktree lock --reason initializing <path>` on a populated worktree makes verification answer `Unpopulated` -> whatever renders the failure reports a cause the predicate cannot know, and the body's claim that every arm names an observation was false | pre_existing | docs-contract | — | fixed at 1720d912e5d363f32a87b577cd185ad75ab0e4d6: the arm reads "the worktree is registered and holds an `initializing` lock, the lock `git worktree add` writes while it populates a checkout", and the variant's doc says the reuse path treats it as the residue element without claiming it is proof of one. `every_verify_failure_displays_as_a_lowercase_fragment_carrying_its_fields` asserts the text names the lock and does not claim a history. Mutation: the old wording restored fails it (it survived the same test before this assertion was added, which is why the assertion is there) | fixed |
| PR131-P2-RECORD-SHAPE-NOT-ENFORCED | P2 | c0eb8c5d35517b9402f7ddd5dfc46eb0c8c9ee42 / src/workspace_manager/worktree.rs:250 | `OpenRecord` kept `detached` and `bare` only for duplicate detection and `close` discarded them -> `worktree /repo\0HEAD <sha>\0\0` was accepted with `branch: None`, and `bare` with a checkout and `branch` with `detached` were accepted too -> `assert_publishable` (`src/workspace_manager.rs:2002`) reads `branch: None` as "no branch is checked out here" and grants publication of a refname on evidence that is malformed rather than absent, against §14's fail-closed rule for external CLI output | pre_existing | security-trust | — | fixed at 1720d912e5d363f32a87b577cd185ad75ab0e4d6: `close` refuses a set of attributes that is not a worktree -- bare, or a HEAD with exactly one of `branch` and `detached` -- as `MalformedRecord::BareWithCheckout`, `NoHead`, `BranchAndDetached` or `NeitherBranchNorDetached`, and the parser names the record. The rule is what Git 2.43.0 was measured printing over twelve shapes (the body has the table). `a_record_is_bare_or_a_head_with_exactly_one_of_branch_and_detached` in worktree.rs and `a_record_whose_attributes_are_not_a_worktree_is_refused` in parsers.rs, six refused shapes and six measured accepted ones. Mutations: the whole rule removed, the neither-branch-nor-detached arm removed, and the bare arm made unreachable each fail both tests | fixed |
| PR131-P2-INVENTED-DESIGN-AND-TRUST-AUTHORITY | P2 | c0eb8c5d35517b9402f7ddd5dfc46eb0c8c9ee42 / src/workspace_manager/worktree.rs:5 | the module doc said the retired `decisions.workspace_candidates.generation`'s substance is `DESIGN.md` §26 and the `is_initializing` doc ruled that a writer to the execution root is inside the trust boundary (§14) -> `DESIGN.md`'s retired-records table maps no `workspace_candidates` record to any section and no design sentence settles that trust boundary -> a doc comment became the authority for a design claim and for a security-policy question, against §1 and the owner's amendment 11 | introduced_by_feature | docs-contract | 5e60c5e (the §26 parenthetical); 5e19848 (the §14 sentence) | fixed at 1720d912e5d363f32a87b577cd185ad75ab0e4d6: the quotation is marked as the retired record's own words with no claim about where its substance lives now, and the trust-boundary question is `SWEEP-WORKTREE-013`'s, for the owner. The body no longer says the `Worktree.Verify` contract is "as `sites.rs` states it": `sites.rs` is implementation. Guard: the doc states only what the code does, and `SWEEP-WORKTREE-014` carries the tree-wide citation question | fixed |
| SWEEP-WORKTREE-014 | P3 | c0eb8c5d35517b9402f7ddd5dfc46eb0c8c9ee42 / src/workspace_manager/worktree.rs:5 | `src/` cites `decisions.workspace_candidates.*` at 51 sites, and `DESIGN.md`'s retired-records table names no such record -> a reader following a citation from the code reaches a record retired on 2026-09-03 and no design section -> the tree's design authority for the workspace-candidates rules is unlocatable from the code, which pass 2 found by reading this file's copy of it | pre_existing | docs-contract | PR131-P2-INVENTED-DESIGN-AND-TRUST-AUTHORITY | deferred: the mapping is `DESIGN.md`'s to state (a retired-records row for `workspace_candidates`, or the section that now carries its substance), and 50 of the 51 citations are other files'; this file quotes the record as the record's own words meanwhile, which is the only claim it can support | deferred |
| PR131-P3-EXACT-HEAD-CLAIMS-STILL-INACCURATE | P3 | c0eb8c5d35517b9402f7ddd5dfc46eb0c8c9ee42 / src/workspace_manager/worktree.rs:344 | the body said seven module tests (eight), nine mutations (eleven in its own table), twelve P3 sweep findings (`SWEEP-WORKTREE-012` is P2 since the repair round rewrote it), all five pass-1 findings repaired (`SWEEP-WORKTREE-013` is deferred), construction through `from_porcelain` (renamed), every reader on an accessor (residue.rs reads the field), and reasons "verbatim" (`reason` decodes lossily, as its own test shows) -> the reviewer measures the body against the head and each is a claim the head does not support | introduced_by_feature | docs-contract | ea4fd74 (the body); 5e19848 (the `verbatim` comments) | fixed at 1720d912e5d363f32a87b577cd185ad75ab0e4d6 and in this body: the code comments say the reasons are decoded with replacement characters, and every count in the body is derived from this head -- the test count from `cargo test --lib workspace_manager::worktree`, the mutation count from the table itself, the dispositions from the ledger rows below | fixed |
