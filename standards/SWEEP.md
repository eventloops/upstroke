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
`examples/probe.rs`) is listed in the swept table, the transitional wording goes in one pull
request, every sentence of it: this file's opening paragraph ("bind new and materially changed
code immediately ... one file at a time"), this paragraph and the activation rule above; §6's
paragraph "These three rules bind the code a change adds or rewrites, now" and its "Enforced by"
line's reference to this file; §7's sentence "This rule is transitional in the same way as
§6's"; §1's sentence "Some standards are newer than the tree; `standards/SWEEP.md` says which";
§16's sentence "A §6 or §7 finding in an unswept file is in scope only under the activation
rule"; `MAINTAINING.md`'s triage clause "or against an unswept file under a transitional
standard"; the `CODING_STANDARDS.md` index paragraph that points here; and the hard-conventions
bullet in `CLAUDE.md` and `AGENTS.md` that says these rules "bind the code a change adds or
rewrites" and points at the activation rule. After that §6 and §7 bind the whole tree with no
scoping, and this file is the record of how the tree got there. Errors are handled where they
arise; a `?` that survives a sweep is one the reviewer agreed was deliberate.

## Review queue

The queue is the W2 decomposition wave: the seven large modules split into per-concern child
modules by the pull requests that merged on 2026-09-03 (#110, #107, #106, #108, #111, #117,
#123), one pull request per parent. For each family, every Rust file under its
directory is queued (the children the split produced, and the test, fixture and support files
that were extracted there earlier), followed by the parent, which keeps whatever the split did
not move. Families are listed in the order their splits merged, and a family of this wave that
merged after the last edit of this table joins it in the next pull request that touches the
table, whichever that is; a stale queue is a queue with work owed, not a broken one. Each file is
swept in queue order: one session, one file, one review by Claude Fable 5.1 whose only subject
is that file, then the pull request that lands the cleanup and moves the row to the swept table.
Files outside `src/` that the standards govern (today `examples/probe.rs`) close the wave.

The tree has older per-concern families that no pull request of this wave produced, among them
`src/plan/markdown/` (`cd612c3`, 2026-08-30), `src/engine/topology/`, `src/runner/container/`,
`src/effects/`, `src/events/log/`, `src/validate/`, `src/status/` and `src/connect/`, and flat
modules no split has touched. They are not on this queue. They join it, family by family, after
the wave above or earlier when the owner names one, and the merge-order rule applies within a
wave, not across the history of the tree.

| # | File | Lines | Family | Merged |
|---|---|---|---|---|
| 5 | `src/workspace_manager/parsers.rs` | 274 | #110 | 2026-09-03 |
| 6 | `src/workspace_manager/residue.rs` | 400 | #110 | 2026-09-03 |
| 8 | `src/workspace_manager/worktree.rs` | 135 | #110 | 2026-09-03 |
| 9 | `src/workspace_manager/fixture.rs` | 386 | #110 | 2026-09-03 |
| 10 | `src/workspace_manager/tests.rs` | 5,973 | #110 | 2026-09-03 |
| 11 | `src/workspace_manager.rs` | 2,535 | #110 | 2026-09-03 |
| 12 | `src/rundir/classify.rs` | 280 | #107 | 2026-09-03 |
| 13 | `src/rundir/discovery.rs` | 331 | #107 | 2026-09-03 |
| 14 | `src/rundir/names.rs` | 45 | #107 | 2026-09-03 |
| 15 | `src/rundir/ownership.rs` | 344 | #107 | 2026-09-03 |
| 16 | `src/rundir/retention.rs` | 232 | #107 | 2026-09-03 |
| 17 | `src/rundir/scratch_tree.rs` | 1,281 | #107 | 2026-09-03 |
| 18 | `src/rundir/tests.rs` | 4,079 | #107 | 2026-09-03 |
| 19 | `src/rundir.rs` | 1,792 | #107 | 2026-09-03 |
| 20 | `src/topology/effects/bijection.rs` | 475 | #106 | 2026-09-03 |
| 21 | `src/topology/effects/export.rs` | 123 | #106 | 2026-09-03 |
| 22 | `src/topology/effects/harness.rs` | 360 | #106 | 2026-09-03 |
| 23 | `src/topology/effects/registry.rs` | 727 | #106 | 2026-09-03 |
| 24 | `src/topology/effects/residue_authority.rs` | 1,086 | #106 | 2026-09-03 |
| 25 | `src/topology/effects/sites.rs` | 1,617 | #106 | 2026-09-03 |
| 26 | `src/topology/effects/vocab.rs` | 796 | #106 | 2026-09-03 |
| 27 | `src/topology/effects/tests.rs` | 6,073 | #106 | 2026-09-03 |
| 28 | `src/topology/effects.rs` | 723 | #106 | 2026-09-03 |
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
| 39 | `src/topology/fold/tests.rs` | 9,805 | #108 | 2026-09-03 |
| 40 | `src/topology/fold.rs` | 862 | #108 | 2026-09-03 |
| 41 | `src/runner/host/environment.rs` | 288 | #111 | 2026-09-03 |
| 42 | `src/runner/host/naming.rs` | 320 | #111 | 2026-09-03 |
| 43 | `src/runner/host/probe.rs` | 133 | #111 | 2026-09-03 |
| 44 | `src/runner/host/tests.rs` | 7,391 | #111 | 2026-09-03 |
| 45 | `src/runner/host.rs` | 777 | #111 | 2026-09-03 |
| 46 | `src/agent/proc/ambient.rs` | 228 | #117 | 2026-09-03 |
| 47 | `src/agent/proc/drain.rs` | 95 | #117 | 2026-09-03 |
| 48 | `src/agent/proc/hooks.rs` | 113 | #117 | 2026-09-03 |
| 49 | `src/agent/proc/test_support/readiness.rs` | 583 | #117 | 2026-09-03 |
| 50 | `src/agent/proc/tests.rs` | 3,893 | #117 | 2026-09-03 |
| 51 | `src/agent/proc.rs` | 5,239 | #117 | 2026-09-03 |
| 52 | `src/config/parse.rs` | 571 | #123 | 2026-09-03 |
| 53 | `src/config/read.rs` | 274 | #123 | 2026-09-03 |
| 54 | `src/config.rs` | 2,875 | #123 | 2026-09-03 |
| 55 | `examples/probe.rs` | 70 | — | — |

Line counts are as of the family's split merge and are a guide to session sizing, not a
contract. "Family" is the pull request whose split defines the family the file belongs to, and
"Merged" is when that split landed; neither says which pull request first created the file at
its path (several of the test and support files were extracted earlier: by #98, #100 and #102,
and `readiness.rs` by `1cd4b1e` on 2026-08-30), and `git log --follow` is the record for that.
`examples/probe.rs` belongs to no family.

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
| `src/workspace_manager/containment.rs` | `2dd1350` | 2026-09-03 | the `?` sites in `refuse_reparse_points` are kept: each propagates a typed refusal or `Io` error, path attached; the run id is refused at `derive` unless it is the canonical ULID of `DESIGN.md` §15 (`Refusal::RunId`), the anchor itself is re-examined and canonically pinned on every revalidation, the walk reports a regular file on the chain where it stands, and every funnel primitive re-checks the chain down to its own target after the `Before` hook; `refuse_unreal_directory` and `canonical_prefix` decide absence (`NotFound`, `NotADirectory`) from failure instead of `?` or a discarded `Err`; the walk refuses a chain that is not plain components below the anchor (new `Refusal::RootOutsidePrivateRoot`); no `Rc`, `Arc` or lock, the peel's per-step parent clone replaced by `PathBuf::pop`; the `strip_verbatim` and "every deletion" doc claims corrected |
| `src/workspace_manager/hooks.rs` | `f58747a` | 2026-09-03 | `Arc<Mutex<HookHarness>>`, the lock and the two ledger clones kept with their lifecycle, protected invariant and handle semantics stated; the three `?` in `funnel` kept as deliberate and documented at the site; `point` now applies a refusal at the mode that answered (it named `kill` whatever fired); a test module for the protocol itself. After review (PR #119): a poisoned harness refuses wherever a refusal is legal and proceeds unrecorded at a `Kill`-only point (`c265536`). |
| `src/workspace_manager/naming.rs` | `5a0ae59` | 2026-09-04 | First pass `51feba7`, reviewed at `9f83b09` (PR #118, six findings, fixed in `a7b7c98`); second pass at `8d25472` (five findings: three fixed in `bc07f05`, the stale queue is PR #122's, the body claims the PR's); third pass at `cff812d` (four findings, fixed in `ecddff8`); fourth pass at `3482ba1` (five unlabelled findings, classified P2/P2/text/docs-contract/text and all fixed in `5a0ae59`; the persisted intent record's wire contract now has a design sentence in `design/15`). `reviews/FINDINGS.md` §45 has every row. §6: no shared ownership or lock, and no clone: none written, and none in expanded serde code, since the record's `Serialize` and `Deserialize` are written by hand (each earlier pass had found one: `slot.clone()`, `keys().cloned()`, then the derive's `into = "String"`). §7: `from_intent_name` has three `?` sites and `Slot::from_parts`, which it shares with `SlotId`'s parser, five more; each returns the parser's `None` and is dispositioned on `from_intent_name` in terms of what "not an intent name" and "malformed intent name" mean to the directory walk, which has one action for both. Fixed: `from_intent_name` accepted non-canonical integers (`g03`, `g+3`) and so was not `intent_name`'s inverse; it now round-trips, pinned at the parser and, in `tests.rs`, through `reclaim_intents`. `safe_component` returns `Result` (§5); `validate` reuses `kind()`; `Slot::parts` is the one rendering that `relative` (a `PathBuf`), `intent_name` and the record's `SlotId` spell. The intent record derives `Serialize` and `Deserialize`; underneath, a hand-written wire reader accepts a key only by finding it in `IntentRecord::FIELDS`, `IntentKind` is read only by matching `IntentKind::WORDS` (derived from `as_str`), and `slot` is `SlotId`, an identifier mirroring the relative path and never a path, parsed back into a validated `Slot` and required to agree with `kind`; the record's fields are private behind `IntentRecord::new`, so what it writes it reads back. Unit tests in the file for all of it. Deferred to row 9 (the parent): `write_synced` stages an intent as `<stem>.tmp` in the intents directory, and `intents()` refuses that residue after an interrupted write. |
| `src/workspace_manager/object.rs` | `af382fa` | 2026-09-04 | No shared ownership, lock or clone: a refusal copies the ref name and the offered value into the error value it returns. The two refusals return `Refusal` rather than the parent's flattened `UpstrokeError`, so a caller or a test matches the variant, and the two `?` are same-type propagation stated at the site. The hash lengths are named. The null id is now refused on the new side of a create or compare-and-swap too (`Refusal::NullNew`; the parent's two call sites and import moved with it): measured on git 2.43 it deletes the ref through the CAS and creates nothing with exit 0. A test module in the file drives both predicates and the three refusals at both hash lengths and the length and alphabet boundaries; each test was witnessed failing under a mutation. After review (PR #126, the pass on `6a54b65`): the null-new refusal is witnessed through both public primitives against a real repository with the raw measurement executed, `design/26` step 5 states the rule, and the changed `# Errors` contracts and the diagnostic say exactly what was measured (`6e7604e`). After the second pass (on `def9320`, the last): the fixture pins its object format and the witness runs in a repository of each format, the design sentences distinguish the compare-and-swap from the delete, `refuse_new` is crate-visible and applied by `ensure_integration_ref` and both `IntegrationRefs` doubles, and the SemVer assessment of the added variant is in the body (`df494c8`). |
| `src/workspace_manager/snapshot_ref.rs` | `dda65ab` | 2026-09-04 | Two types and no functions at the base, so §6/§7 literally found nothing and the sweep was §5 and §12, with the owner's amendment 7 passes (correctness, better implementation) applied. `ObjectId`: a full hexadecimal object id of either hash length that is not the null id, validated once at `ObjectId::new` through the parent's `is_object_id` and `is_null_object_id`, spelt lowercase so `Eq` and `Hash` are equality of the object; `SnapshotInput`'s three ids and `Snapshot`'s HEAD are this type, so neither can be built from a ref name, a short id or an option-shaped string (the funnel handed every value to `git commit-tree` and `git worktree add` unchecked). `Snapshot`: private fields behind one parent-visible constructor that builds the slot from a `SnapshotName` and holds the HEAD as one `SnapshotHead`, so the ephemeral commit is the HEAD rather than a second field that has to agree with it; accessors `slot`, `path`, `head`, `ephemeral`; `Clone` dropped (a copy is two values for one checkout). Parent moved with it: `Refusal::NotAnObjectId`, `add_snapshot` builds through the constructor and checks the `git commit-tree` line is an id before checking anything out at it, `remove_snapshot` reads the slot through the accessor; the engine's one construction site and two path reads, and the read and construction sites in the two suites. §6: no shared ownership or lock; the clones in `add_snapshot` are two of the name and one of the input's commit, each a small owned value stated at the site. §7: the funnel's three `?` are same-type propagation; the commit-tree line's refusal becomes a Git error naming the command. Seven tests in the file, each witnessed under at least one of ten mutations. Deferred to row 11 (the parent): consolidating `ObjectId` beside its predicates in `object.rs` and giving the ref primitives and the engine's `CommitSha` the same type. After review (PR #130, the pass on `be201d8`, one P2): the newtype is a spelling and cannot know the repository's object format, so a ref spelt as hexadecimal of the other format's length passed it and `git worktree add` followed the ref (measured, git 2.43); `add_snapshot` now resolves each input once against the repository, peeled to its role's object type, and accepts only an answer equal to the input (`Refusal::SnapshotInputResolvesElsewhere`, `SnapshotObject`), witnessed in a repository of each format (`a1c8d2f`). |
