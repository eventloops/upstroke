# Standards sweep

§6 (shared ownership, locks, clones) and §7 (the `?` operator) were tightened on 2026-09-03. They
bind new and materially changed code immediately. The existing tree predates them and is being
brought up to them one file at a time: each file gets a deep review by a frontier model, then a
pull request that lands the cleanup and adds the file to the table below.

**Activation rule.** In a file not yet listed here, §6 and §7 apply to the code a change adds or
rewrites: every line inside a hunk the change introduces or modifies, and the whole body of any
function the change modifies. A pure formatting, renaming or comment change activates nothing.
An existing `Rc`, `Arc`, `Mutex`, `RwLock`, `clone()` or `?` outside that scope is not a review
finding. Once a file is listed, §6 and §7 apply to it in full, and a reviewer may cite them
against any line.

**The activation rule is temporary.** It exists only because the tree predates the rules. When
every Rust file the standards govern (§1: all Rust in the repository, which today is `src/` and
`examples/probe.rs`) is listed in the swept table, this paragraph and the activation rule are
deleted, and §6 and §7 bind the whole tree with no scoping. Errors are handled where they arise;
a `?` that survives a sweep is one the reviewer agreed was deliberate.

## Review queue

The large modules are being split into per-concern child modules, one pull request per parent.
When a split merges, every Rust file under the family's directory joins the queue (the children
the split produced, and the test, fixture and support files that were extracted there earlier),
followed by the parent, which keeps whatever the split did not move. Families are queued in the
order their splits merged. Each file is swept in queue order: one session, one file, one review
by Claude Fable 5.1 whose only subject is that file, then the pull request that lands the
cleanup and moves the row to the swept table. Files outside `src/` that the standards govern
(today `examples/probe.rs`) are queued last.

| # | File | Lines | From | Merged |
|---|---|---|---|---|
| 1 | `src/workspace_manager/containment.rs` | 235 | #110 | 2026-09-03 |
| 2 | `src/workspace_manager/hooks.rs` | 200 | #110 | 2026-09-03 |
| 3 | `src/workspace_manager/naming.rs` | 254 | #110 | 2026-09-03 |
| 4 | `src/workspace_manager/object.rs` | 68 | #110 | 2026-09-03 |
| 5 | `src/workspace_manager/parsers.rs` | 274 | #110 | 2026-09-03 |
| 6 | `src/workspace_manager/residue.rs` | 400 | #110 | 2026-09-03 |
| 7 | `src/workspace_manager/snapshot_ref.rs` | 56 | #110 | 2026-09-03 |
| 8 | `src/workspace_manager/worktree.rs` | 135 | #110 | 2026-09-03 |
| 9 | `src/workspace_manager/fixture.rs` | 386 | #100 | 2026-09-02 |
| 10 | `src/workspace_manager/tests.rs` | 5,973 | #100 | 2026-09-02 |
| 11 | `src/workspace_manager.rs` | 2,535 | #110 | 2026-09-03 |
| 12 | `src/topology/effects/bijection.rs` | 475 | #106 | 2026-09-03 |
| 13 | `src/topology/effects/export.rs` | 123 | #106 | 2026-09-03 |
| 14 | `src/topology/effects/harness.rs` | 360 | #106 | 2026-09-03 |
| 15 | `src/topology/effects/registry.rs` | 727 | #106 | 2026-09-03 |
| 16 | `src/topology/effects/residue_authority.rs` | 1,086 | #106 | 2026-09-03 |
| 17 | `src/topology/effects/sites.rs` | 1,617 | #106 | 2026-09-03 |
| 18 | `src/topology/effects/vocab.rs` | 796 | #106 | 2026-09-03 |
| 19 | `src/topology/effects/tests.rs` | 6,073 | #98 | 2026-09-02 |
| 20 | `src/topology/effects.rs` | 723 | #106 | 2026-09-03 |
| 21 | `src/rundir/classify.rs` | 280 | #107 | 2026-09-03 |
| 22 | `src/rundir/discovery.rs` | 331 | #107 | 2026-09-03 |
| 23 | `src/rundir/names.rs` | 45 | #107 | 2026-09-03 |
| 24 | `src/rundir/ownership.rs` | 344 | #107 | 2026-09-03 |
| 25 | `src/rundir/retention.rs` | 232 | #107 | 2026-09-03 |
| 26 | `src/rundir/scratch_tree.rs` | 1,281 | #77 | 2026-08-31 |
| 27 | `src/rundir/tests.rs` | 4,079 | #100 | 2026-09-02 |
| 28 | `src/rundir.rs` | 1,792 | #107 | 2026-09-03 |
| 29 | `src/topology/fold/apply.rs` | 603 | #108 | 2026-09-03 |
| 30 | `src/topology/fold/check_attempt.rs` | 792 | #108 | 2026-09-03 |
| 31 | `src/topology/fold/check_candidate.rs` | 252 | #108 | 2026-09-03 |
| 32 | `src/topology/fold/check_end.rs` | 187 | #108 | 2026-09-03 |
| 33 | `src/topology/fold/check_integration.rs` | 659 | #108 | 2026-09-03 |
| 34 | `src/topology/fold/outcome.rs` | 219 | #108 | 2026-09-03 |
| 35 | `src/topology/fold/parse.rs` | 57 | #108 | 2026-09-03 |
| 36 | `src/topology/fold/predicates.rs` | 320 | #108 | 2026-09-03 |
| 37 | `src/topology/fold/region.rs` | 107 | #108 | 2026-09-03 |
| 38 | `src/topology/fold/start.rs` | 293 | #108 | 2026-09-03 |
| 39 | `src/topology/fold/tests.rs` | 9,805 | #98 | 2026-09-02 |
| 40 | `src/topology/fold.rs` | 862 | #108 | 2026-09-03 |
| 41 | `src/runner/host/environment.rs` | 288 | #111 | 2026-09-03 |
| 42 | `src/runner/host/naming.rs` | 320 | #111 | 2026-09-03 |
| 43 | `src/runner/host/probe.rs` | 133 | #111 | 2026-09-03 |
| 44 | `src/runner/host/tests.rs` | 7,391 | #102 | 2026-09-02 |
| 45 | `src/runner/host.rs` | 777 | #111 | 2026-09-03 |
| 46 | `src/agent/proc/ambient.rs` | 228 | #117 | 2026-09-03 |
| 47 | `src/agent/proc/drain.rs` | 95 | #117 | 2026-09-03 |
| 48 | `src/agent/proc/hooks.rs` | 113 | #117 | 2026-09-03 |
| 49 | `src/agent/proc/test_support/readiness.rs` | 583 | #115 | 2026-09-03 |
| 50 | `src/agent/proc/tests.rs` | 3,893 | #117 | 2026-09-03 |
| 51 | `src/agent/proc.rs` | 5,239 | #117 | 2026-09-03 |
| 52 | `examples/probe.rs` | 70 | — | — |

Line counts are as of the split's merge and are a guide to session sizing, not a contract. The
"From" column is the pull request that created the file at its current path; "Merged" is when
that landed. The rest of `src/` (the modules no split has touched) joins the queue when the
families above are swept, or earlier if the owner names a file.

Baseline at the tightening (master `cfec136`, 114 Rust files under `src/`):

| Construct | Sites | Files |
|---|---|---|
| `Arc<` | 81 | 20 |
| `Mutex<` | 145 | 32 |
| `Rc<` | 4 | 2 |
| `.clone()` | 1,941 | 84 |
| `?` (propagation) | ≈1,200 | 71 |

## Swept files

| File | Swept at (commit) | Date | Notes |
|---|---|---|---|
| _none yet_ | | | |
