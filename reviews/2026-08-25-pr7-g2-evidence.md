# PR7 — evidence for G2

**Head:** `f6ed9f1` · **Suite:** 1673/0 (1665 lib + 8 bin, 32 ignored) · **CI:** nine
checks green including `test (windows-latest)`

## What this document is, and is not

**G2 is not a gate on this pull request.** Its `frozen_input_range` is "merged PR4,
PR5, PR6, PR7 on top of G1" and its `blocks_later_prs` is "PR8 and later may not merge
until G2 passes". G2 runs on the **integrated tree after PR7 merges**.

So this is not the G2 gate report. It is **PR7's contribution to the eight artifacts
`cumulative_review_gates.gates[G2].required_artifacts` names**, produced now so the gate
has them, with each measurement taken at `f6ed9f1` and reproducible by the command shown.

| # | Required artifact | PR7's share |
|---|---|---|
| 1 | gate report | **not PR7's** — written at G2, on the integrated tree |
| 2 | host/container parity outputs | host half below; container half is PR6's |
| 3 | fault-injection evidence table | **§3** — the largest, and PR7's |
| 4 | ref/worktree/snapshot/object/container/run-directory inventory | **§4** |
| 5 | user-checkout inventory diff | **§5** |
| 6 | Docker-gated suite result | **not PR7's** — PR6's, environment-gated; §6 records the environment |
| 7 | effect-enforcement artifacts | **§7** |
| 8 | runner identity outputs | PR4/PR6's; §8 records PR7's per-invocation rows |

## 3. Fault-injection evidence — the eleven rows

`pr_sequence[8].gating` names eleven `transaction_fault_matrix` rows. Their `test:`
fields name **117** snake_case tests between them.

**What the number below is, and is not.** It is a *presence* check: every name the packet
lists exists in `src/` and runs. It is **not** a claim that each test asserts what its row's
`resume_action` requires, and a G2 reviewer must not read it as coverage. The gate's own
remit is to confirm the behaviour; this artifact confirms the checklist is answered, which
is the thing a reviewer would otherwise have to re-derive by hand. S5 round 1 found real
defects in code these 117 tests cover, which is the sharpest possible evidence that
presence and correctness are different measurements.

```
$ bash ~/tactus-artifacts/pr7/drivers/tfm-gate.sh .
  [ok  ] T-RUNSTART    37/37   T-DISPATCH   4/4    T-ATTEMPT    9/9
  [ok  ] T-CAND-OBJ     4/4    T-CAND-REF   3/3    T-SCRUB      3/3
  [ok  ] T-FAILED       6/6    T-RETAINED   3/3    T-RETRY      4/4
  [ok  ] T-APPEND      19/19   T-RESUME    25/25
  ---- 117/117 present in ./src  (1 named but owned by a later slice)
  [ PASS ]
```

The one exception is named rather than absorbed: a single test in the checklist belongs
to a later slice's range, and the gate reports it as such instead of counting it green.

**And a second qualification, from S5.** Three of the defects round 1 confirmed —
the driver's ladder position, the spend double-count, and the gate set not
short-circuiting — were in behaviour these rows govern, and none of the 117 caught them.
Each now has a witness of its own. The checklist is necessary and it is not sufficient.

**Residue-class evidence** is both halves the artifact asks for. Synthetic, per element:
`the_checked_in_residue_class_record_is_what_the_enums_generate`,
`each_site_lists_the_residue_elements_its_own_command_can_leave`,
`a_residue_class_entry_with_an_executed_hook_claim_is_refused`. Sampling, with every
observed residue classified and recovered:
`workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered`
and `engine::topology::attempt::tests::sampled_git_add_and_write_tree_child_kills_every_residue_classified_and_recovered`.

### The withheld catalogue, and two cuts of it

Six mutation catalogues were authored from the packet alone, before the driver existed and
withheld from the implementer: **265 entries** in `catalogues/ALL.json`, each carrying a
`target`, the `mutation`, its `packet_basis`, the `killing_assertion` it predicts, and the
`named_test_survived` it expects to slip past.

Two cuts of that catalogue name the driver, and **both are correct under their own
definition**:

| Cut | Definition | Entries |
|---|---|---|
| §17's, at its measurement time | "written against `TopologyRun`, its loop, or the production `EventEmitter`" | **93** |
| S5's, at application time | `target` names `TopologyRun` (86) **or** begins `predicted-*`, the driver predicates authored before the driver existed (44); union | **115** |

The second is a broader needle over the same catalogue, not a correction of the first.
`predicted-*` entries were speculative when §17 counted — there was no driver to apply them
to — and are applicable now that `TopologyRun` exists. **Application follows the
measurement, not the historical number**, so S5 applies 115; §17's 93 stands as what it
was.

Application is part of **PR7's** evidence rather than the G2 pass's, because the driver
they were authored against is this slice's. The pass's W8 therefore becomes
re-measurement against the merged head rather than first application.

**A caveat the gate should carry forward.** The sampler has no seed; its variance is one
measured git duration. A mutation caught only by a sampler is not a witness, and this
slice treated one such catch as variance and wrote a direct test instead
(`f574622`). The sampling record is evidence of coverage, not of determinism.

## 4. Inventories

`effect_sites.json` is generated from the enums and checked in; the generator and the
file are compared by `the_checked_in_effect_sites_json_is_what_the_enums_generate`.

**70 sites**, by group:

| group | sites | | group | sites |
|---|---|---|---|---|
| run_dir | 14 | | event | 7 |
| worktree | 11 | | object | 6 |
| ref | 8 | | lock | 6 |
| container | 8 | | snapshot | 4 |
| answer | 3 | | process | 2 |
| report | 1 | | | |

Of these, **2** carry an `IdUnread` point, **9** declare residue classes, and **4** are
read-only. `every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
is what makes the declaration binding rather than descriptive.

**Husk census evidence** — the run-directory half, by test:
`resume_reclaims_a_provable_husk_beside_the_run_and_retains_a_possibly_committed_one`,
`the_resume_census_reports_the_husk_it_could_not_reclaim`,
`resume_completes_past_a_husk_whose_private_half_cannot_be_removed`,
`a_husk_whose_private_half_cannot_be_removed_is_reported_and_the_census_continues`,
`torn_first_line_is_husk_or_possibly_committed_per_commit_record`.

## 5. User-checkout inventory

`checkout inventory unchanged` is in `pr_sequence[8].proof_tests`. The engine never
touches the user's checkout: every worktree it creates is under the execution root.

**Citations corrected at `0cd2001` (`PR7-R3-CONTRACT-002`).** Two of the five tests
previously listed here do not hold this line, and saying they did was the failure mode
this document exists to avoid — it is frontier-review input, where a false claim costs
more than a false comment.

Holding the line:

- `snapshots_create_no_object_for_a_commit_and_never_share_a_checkout` — a snapshot
  never shares the user's checkout.
- `gate_snapshot_does_not_execute_post_checkout_hook` and
  `branch_creation_and_switch_do_not_execute_post_checkout_hook` — the engine's
  worktree operations do not run the user's hooks.

**Not** holding it, and previously cited as if they did:

- `sparse_checkout_is_refused_before_worker_spend` asserts that an incomplete index
  fails with "sparse checkout is active", that a complete checkout is accepted, and two
  Git-version requirements. It says nothing about inventory or worktree location: it is
  a **precondition refusal** test.
- `sparse_checkout_preflight_refusal_leaves_worktree_clean` asserts the worktree is
  clean **after a refusal**, which is a claim about a path that did no work — not about
  a run's effect on the checkout.

So the line rests on three tests, not five, and the two removed are a *precondition*
and a *refusal-cleanup*. Whether three tests are sufficient evidence for
`proof_tests`'s "checkout inventory unchanged" is a question for the reviewer; the
point of this correction is that the question is now asked against what the tests
actually assert.

## 6. Docker-gated suite — environment

**Corrected at `0cd2001` — the previous text said "this box has no Docker daemon",
and that is false** (`PR7-R3-CONTRACT-005`).

The box **has** a Docker daemon and the Docker-gated tests execute against it. They run
in every full-suite invocation on this machine, and their real-daemon responses are
visible in this slice's own history: the `Conflict. The container name ... is already in
use` failures recorded during round 2 came from a live daemon, not from a fake.

What that misstatement cost is worth recording, because it is the reason to correct
rather than delete it. Believing the gated suite did not run, round 2 attributed those
failures to passive "contention" between concurrent runs. Round 3's `contract` lens
found the real cause: `fake::preclean_names` kills and removes containers by a **fixed**
name with no liveness check, so a second suite run killed the first's live container
(`PR7-R3-CONTRACT-001`, repaired by scoping the name to the build slot). A false
environment claim in this document delayed that diagnosis by a round.

## 7. Effect enforcement

**Four** artifacts, all present and all enforced by tests that run in CI.
Re-measured at `0cd2001`; the previous table said "five" while listing four, and its
`effects/wrappers.toml` row was stale — that file moved when `commit_parent` was
classified during round 2 (`PR7-R3-CONTRACT-008`).

| artifact | bytes | sha256 (16) |
|---|---|---|
| `clippy.toml` | 18898 | `80d59dae80054fd7` |
| `effects/allowlist.toml` | 32262 | `2121b60d10f904b9` |
| `effects/wrappers.toml` | 27063 | `0a261b72874f5ad3` |
| `effect_sites.json` | 33547 | `ab9edaad67abcfc7` |

`cargo test --all-features effects::` — **90 passed, 0 failed**. Among them the
allow-placement scan (`every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist`)
and the classification census
(`every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified`), which
caught three unclassified functions during this slice and refused each until it was
classified.

`clippy --all-targets --all-features -- -D warnings` passes on ubuntu, macos and windows.

## 8. Runner identity — PR7's rows

PR4 and PR6 own this artifact. PR7 adds the per-invocation boundary rows for its own
sites: every worker, gate, review, re-ask and probe process carries a typed
`InvocationId` minted by `AttemptIdentities`, and `InvocationLedger` asserts each is
registered exactly once and settled exactly once (R4). The slot pair is asserted rather
than brokered at `max_parallel = 1` (R3).

> **Correction, 2026-08-27, at `8f0e605`.** "Every … probe process" was false for
> **fresh creation** when this was written, and stayed false until the date of this note.
> `create.rs`'s P4 registered one `probe(agent, 0)` identity around the *whole* adapter
> call and handed the adapter the raw `Runner`, so an adapter that runs several processes
> put one of them in the ledger. A current Codex probe runs **ten** Runner requests —
> version, two help probes, six strict-config probes and the model catalog — of which
> ordinal 0 was accounted and 1 through 9 were absent.
>
> The consequence was a *wrong* row rather than a missing one: with the version probe
> succeeding and a later probe failing, the ledger recorded **ordinal 0 cancelled** — the
> identity of the process that succeeded — and held no record of the one that failed.
>
> Resume was never affected: `preflight::Registering` wraps the Runner and registers each
> request, and its own doc says why — *"one place, so that 'each a registered invocation'
> is true of a process an adapter built as much as of one this module built"*. Fresh
> creation was the other place. It now uses the same wrapper, and
> `create::tests::the_creation_ledger_accounts_every_probe_process` holds it: two
> processes, the second refusing, and the ledger reads one completed and one cancelled
> where it read nought and one before.
>
> Found by the frontier review of `bf927f3` (its third P1), not by this artifact's own
> evidence — which is the point of recording it here rather than only in the ledger.

Two identity facts this slice established are worth carrying to the gate:

- The **append-error protocol's obligation (3)** — cancelling in-flight invocations —
  moved from `emit` to the caller, because the ledger belongs to the driver.
  `AppendError` is now unreachable without discharging it: the report carries a private
  witness and `UncancelledAppend::cancelling` is its only constructor. See §3 of
  `reviews/FINDINGS.md`.
- `TaskFold::defers` is a **frozen-file change**, disclosed as
  `PR7-FOLD-DEFERS-ACCUMULATOR` with per-instance Class B approval, its witness, and the
  contract passages that oblige the branch.
