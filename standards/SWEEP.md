# Standards sweep

§6 (shared ownership, locks, clones) and §7 (the `?` operator) were tightened on 2026-09-03, and
§7's panic surface (indexing, slicing, `unreachable!`) on 2026-09-04. They bind new and materially
changed code immediately. The existing tree predates them and is being brought up to them one file
at a time: each file gets a deep review by a frontier model, then a pull request that lands the
cleanup and adds the file to the table below.

**Activation rule.** In a file not yet listed here, §6 and §7 apply to the code a change adds or
rewrites: every line inside a hunk the change introduces or modifies, and the whole body of any
function the change modifies. A pure formatting, renaming or comment change activates nothing.
An existing `Rc`, `Arc`, `Mutex`, `RwLock`, `clone()`, `?`, `v[i]`, `&v[a..b]` or `unreachable!`
outside that scope is not a review finding. Once a file is listed, §6 and §7 apply to it in full,
and a reviewer may cite them against any line.

**The activation rule is temporary.** It exists only because the tree predates the rules. When
every Rust file the standards govern (§1: all Rust in the repository, which today is `src/` and
`examples/probe.rs`) is listed in the swept table, the transitional wording goes in one pull
request, every sentence of it: this file's opening paragraph ("bind new and materially changed
code immediately ... one file at a time"), this paragraph and the activation rule above; §6's
paragraph "These three rules bind the code a change adds or rewrites, now" and its "Enforced by"
line's reference to this file; §7's sentence "This rule is transitional in the same way as
§6's" and, in its panic-surface paragraph, the sentence beginning "This rule is transitional in
the same way as §6's and the `?` rule above"; §1's sentence "Some standards are newer than the
tree; `standards/SWEEP.md` says which"; §16's sentence "A §6 or §7 finding in an unswept file is
in scope only under the activation rule"; `MAINTAINING.md`'s triage clause "or against an unswept
file under a transitional standard"; the `CODING_STANDARDS.md` index paragraph that points here;
and the hard-conventions bullet in `CLAUDE.md` and `AGENTS.md` that says these rules "bind the
code a change adds or rewrites" and points at the activation rule. That same pull request adds
`clippy::indexing_slicing` and `clippy::unreachable` to `[lints]`, taking
`allow-indexing-slicing-in-tests` and no allowance for `unreachable!`, which has none to take (see
§7). That is what makes the prose removable: the panic surface §7 governs stops being a review duty
**for these two constructs** at the commit the build starts catching them. It does not end the
review duty for the panic surface as a whole — `assert!`, `split_at`, `expect`-shaped helpers and
arithmetic overflow all still terminate, and a local macro that expands to `unreachable!` resolves
elsewhere and is not caught — so §7's paragraph on what may panic survives these lints. After that §6 and §7 bind the whole tree with no
scoping, and this file is the record of how the tree got there. Errors are handled where they
arise; a `?` that survives a sweep is one the reviewer agreed was deliberate; an index or an
`unreachable!` that survives one is under an `#[expect]` that says why.

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
| 9 | `src/workspace_manager/fixture.rs` | 386 | #110 | 2026-09-03 |
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

**Row 12 has had two passes and is not swept.** PR #137 landed correctness repairs in
`src/rundir/classify.rs` — a first-line re-read that carried the scan's proof onto bytes the scan
never saw, an unreachable `?`, an unclamped short read, and a module comment that stated a safety
property the code does not have — with three tests and their mutations. It also **found a P1 it
could not repair**: the probe does not terminate on a source answering `Interrupted`, the census
holds the physical worktree lock across it, and both repairs attempted were wrong — the second
because an exhausted retry bound has to answer `Husk`, which is the reclaiming classification, so
it traded a hang for a deletion. The interrupted-read behaviour is master's and the defect is
`SWEEP-CLASSIFY-001`, an open P1 whose row carries both unbounded doors, the reproduction and why a
cap cannot be the fix. A repair needs a classification that is neither `Committed` nor `Husk`, so
it needs the same signature change as the folds below. It did **not** discharge §7 for the file.
**Seven** inspections still fold an I/O failure the filesystem declined to explain into the same
`Husk` as an honest absence: the `symlink_metadata` guard, the `open`, the `fstat` that takes the
bound, the window read, the scan read's own `Err(_)` arm, the seek and the re-read. Seven is
derived rather than counted -- PR #137's body carries the command that enumerates the sites, after
two frontier passes corrected the number twice. Making those errors changes `classify_run_dir`'s
public signature, the `pub` `RunDirClass` re-export, `RunDirEntry`'s `pub` class field in the
engine's census, and both `list_runs` and `list_husks` in `src/rundir/discovery.rs` (queue row 13)
— three production call sites in three files and thirty-six call sites in all, which is past any
reading of a sweep's own-file bound. Listing the file here would activate §6 and §7 over it in
full, and recording a violation does not satisfy a standard, so the row stays open and a successor
takes the folds with the call sites they force. The findings are `reviews/FINDINGS.md` §56.

**Row 29 (`src/topology/fold/apply.rs`) has had a frontier pass, four repair rounds, and is not
swept.** The pass and repairs landed the file's §5, §6 and §7 work. Five catch-alls over closed
enums were made exhaustive, so a new variant is a compile error rather than a silent default:
`apply_verification_unavailable` over `UnavailableOutcome`, `apply_answer`'s dispatch over `Derived`,
and the guards in `apply_merge_prepared` (`PreparedDisposition`), `apply_merge_rejected`
(`RejectionDisposition`) and `close_generation` (`GenerationLease`) that decided a `next_sequence`
increment or a lease release from one variant. The
module doc's replay-purity and ownership claims were corrected against the source. And one
correctness fix landed: an answered question now returns its task to the state it was parked from — a
derived `OpenQuestion.parked_from`, restored in `apply_answer` — rather than to a fixed `Pending`,
which was right only for a spawn admission or a parked settlement and lost an `AwaitingMerge` or
`Deferred` bare-question park. `apply` was otherwise found sound: a pure function of
`(state, event, derived)` with no clock, environment, randomness or I/O; transitions total over the
closed 24-variant vocabulary; §7 clean.

The row stays in the queue, and the items found during the pass are labelled by stage:

- **The in-flight wedge — FIXED in #153.** A fold-legal `question_raised` then `Declined` on a task
  whose attempt was in flight wedged the run so `derived_outcome` could never end it. Ruled a
  check-layer behaviour change and fixed in `src/topology/fold/check_end.rs` (row 32) on its own
  stream, `fix/declined-halt-wedge` at `7a6b23b`. What bears on row 29 is that its two doors —
  raise-time and answer-time — let `apply_answer` be written against a task that is still parked.
  This record does not restate #153's guard, which has moved across its heads; read it at that sha.
- **A declined repair failing only the repair — FILED, not fixed.** `apply_answer`'s `Declined` arm
  fails only the answered task (`master`'s behaviour), so declining a repair's question leaves the
  lineage root `AwaitingRepair` and the run wedges. Pass 3 fixed it in this file; pass 4 reverted the
  fix, because it cannot be completed here: failing the lineage must also clear `self.transaction`,
  which `release_holdings_of` does not, and the question is admitted on a task with a live transaction
  by `check_end.rs`'s `check_question_raised` — so the surviving transaction lets `apply_task_merged`
  turn the declined lineage back to `Merged` and publish declined work. `design/26` (*Declining fails
  the lineage.*) and `release_holdings_of`'s own doc are the authority; the finding is
  `SWEEP-FOLD-APPLY-DECLINE-LINEAGE`, naming both homes — row 29 for the failing and the transaction,
  `check_end.rs` row 32 for the admission — for a successor once #152 and #153 land.
- **`apply_answer` returning a parked task to a fixed `Pending` — FIXED here.** The correctness fix
  above. `apply_answer` restores the parked-from state under a conjunction guard: **only when the
  answer closes the last open question for the task and the task is still `AwaitingInput`.** Each
  conjunct was a separate pass's finding when it stood alone — still-parked, because a task can reach
  `Merged` while a question is open and restoring then un-merges it; last-question, because a task can
  hold two open questions and restoring while one is open un-parks it with input outstanding. And
  because `open_question` records `parked_from` at raise time, a second question raised on an
  already-parked task inherits the first's `parked_from` (the parking *episode*'s state), so the
  answer-return is a function of the log and not of answer order. Closes
  `PR153-FOLD-ANSWER-RETURNS-TO-PENDING`; #153 deletes that finding file once this fix is on master,
  citing the commit that introduces it.
- **The answer-return rule — now in the design.** That a bare question on a non-`Pending` task is
  valid input was already settled by `design/12` and by `select/tests.rs`; what the design did not
  state was the answer-return, and `design/12` now does — an answered question returns the task to
  the state it was parked from. No longer an open question.
- **The design-authority claim — WITHDRAWN on evidence.** An earlier draft held the row open because
  the contracts `apply.rs` states the effect of (INV-02, ST-06, `transaction_fault_matrix[T-ATTEMPT]`,
  the retired `decisions/2026-08-12` record) name no `DESIGN.md` section. Wrong: `DESIGN.md`'s row for
  `decisions/2026-08-12` maps it to §26, which carries that decision verbatim. The finding is deleted.
- **Owed before the row is swept.** `QuestionOrigin`'s only role, deciding the answer-return, is now
  subsumed by `parked_from`; removing the type reaches `check_end.rs` (row 32, open in #153) and
  `start.rs` (row 38, the `Derived::Answer` construction), so it is recorded as
  `SWEEP-FOLD-APPLY-ORIGIN-SUPERSEDED` for those rows rather than done here.

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

Baseline at the panic-surface tightening, measured at master `93d6337` over 166 Rust files under
`src/` and `examples/` by running the two lints themselves —
`cargo clippy --all-targets --all-features -- -W clippy::unreachable -W clippy::indexing_slicing`,
counting each warning's primary span once:

| Construct | Sites | Files | In a `tests.rs` | Elsewhere |
|---|---|---|---|---|
| `unreachable!` | 65 | 20 | 43 | 22 |
| indexing and slicing | 1,034 | 72 | 442 | 592 |

**This is a measurement at a named head, not a property of the tree, and the sweep moves it.**
Merging master in twice while this section was being written took `unreachable!` from a claimed 77
to a claimed 79 and indexing from 1,030 to 1,034 — the second of those from one merge, `93d6337`,
which added four indexing sites in a suite. The command above is recorded so the number can be
taken again rather than argued about; a stale figure here is a re-run, not a finding.

**Neither construct can be counted by text, and the earlier figures in this section were a text
census that was wrong.** `grep` finds 79 mentions of `unreachable!` across 23 files at this head;
14 of them are comments, doc comments and string literals discussing the construct, several of them
in the very tests whose helper ends in one. The count above is what the compiler resolves, and it
supersedes both the 77 recorded at `44dc06f` and the 79 that replaced it. The suite
`src/workspace_manager/tests.rs` holds **one** invocation, at line 7390, and it was there at
`44dc06f`: PR #136 added two mentions and no invocations.

Two limits of these numbers, both stated rather than hidden. They are a **Linux** run, so code
behind a platform gate is invisible to them: `src/runner/host/tests.rs:5161` is a real
`unreachable!` inside `#[cfg(windows)] fn windows_ambient_coordinator_helper`, which makes the
across-platform count 66 across 21 files, and the msvc leg is what would report the Windows half of
the indexing count. And the `tests.rs` split is by file name, so an inline `#[cfg(test)] mod tests`
inside a production file counts under "elsewhere": 592 and 22 are upper bounds on the production
figure, not the figure.

Of the nine files in the swept table, eight contain no `unreachable!` at all and the ninth,
`src/workspace_manager/tests.rs`, contains the single site above — which it inherited rather than
introduced, and which its sweep already reasoned about.

## Swept files

| File | Swept at (commit) | Date | Notes |
|---|---|---|---|
| `src/workspace_manager/containment.rs` | `2dd1350` | 2026-09-03 | the `?` sites in `refuse_reparse_points` are kept: each propagates a typed refusal or `Io` error, path attached; the run id is refused at `derive` unless it is the canonical ULID of `DESIGN.md` §15 (`Refusal::RunId`), the anchor itself is re-examined and canonically pinned on every revalidation, the walk reports a regular file on the chain where it stands, and every funnel primitive re-checks the chain down to its own target after the `Before` hook; `refuse_unreal_directory` and `canonical_prefix` decide absence (`NotFound`, `NotADirectory`) from failure instead of `?` or a discarded `Err`; the walk refuses a chain that is not plain components below the anchor (new `Refusal::RootOutsidePrivateRoot`); no `Rc`, `Arc` or lock, the peel's per-step parent clone replaced by `PathBuf::pop`; the `strip_verbatim` and "every deletion" doc claims corrected |
| `src/workspace_manager/hooks.rs` | `f58747a` | 2026-09-03 | `Arc<Mutex<HookHarness>>`, the lock and the two ledger clones kept with their lifecycle, protected invariant and handle semantics stated; the three `?` in `funnel` kept as deliberate and documented at the site; `point` now applies a refusal at the mode that answered (it named `kill` whatever fired); a test module for the protocol itself. After review (PR #119): a poisoned harness refuses wherever a refusal is legal and proceeds unrecorded at a `Kill`-only point (`c265536`). |
| `src/workspace_manager/naming.rs` | `5a0ae59` | 2026-09-04 | First pass `51feba7`, reviewed at `9f83b09` (PR #118, six findings, fixed in `a7b7c98`); second pass at `8d25472` (five findings: three fixed in `bc07f05`, the stale queue is PR #122's, the body claims the PR's); third pass at `cff812d` (four findings, fixed in `ecddff8`); fourth pass at `3482ba1` (five unlabelled findings, classified P2/P2/text/docs-contract/text and all fixed in `5a0ae59`; the persisted intent record's wire contract now has a design sentence in `design/15`). `reviews/FINDINGS.md` §45 has every row. §6: no shared ownership or lock, and no clone: none written, and none in expanded serde code, since the record's `Serialize` and `Deserialize` are written by hand (each earlier pass had found one: `slot.clone()`, `keys().cloned()`, then the derive's `into = "String"`). §7: `from_intent_name` has three `?` sites and `Slot::from_parts`, which it shares with `SlotId`'s parser, five more; each returns the parser's `None` and is dispositioned on `from_intent_name` in terms of what "not an intent name" and "malformed intent name" mean to the directory walk, which has one action for both. Fixed: `from_intent_name` accepted non-canonical integers (`g03`, `g+3`) and so was not `intent_name`'s inverse; it now round-trips, pinned at the parser and, in `tests.rs`, through `reclaim_intents`. `safe_component` returns `Result` (§5); `validate` reuses `kind()`; `Slot::parts` is the one rendering that `relative` (a `PathBuf`), `intent_name` and the record's `SlotId` spell. The intent record derives `Serialize` and `Deserialize`; underneath, a hand-written wire reader accepts a key only by finding it in `IntentRecord::FIELDS`, `IntentKind` is read only by matching `IntentKind::WORDS` (derived from `as_str`), and `slot` is `SlotId`, an identifier mirroring the relative path and never a path, parsed back into a validated `Slot` and required to agree with `kind`; the record's fields are private behind `IntentRecord::new`, so what it writes it reads back. Unit tests in the file for all of it. Deferred to row 9 (the parent): `write_synced` stages an intent as `<stem>.tmp` in the intents directory, and `intents()` refuses that residue after an interrupted write. |
| `src/workspace_manager/object.rs` | `af382fa` | 2026-09-04 | No shared ownership, lock or clone: a refusal copies the ref name and the offered value into the error value it returns. The two refusals return `Refusal` rather than the parent's flattened `UpstrokeError`, so a caller or a test matches the variant, and the two `?` are same-type propagation stated at the site. The hash lengths are named. The null id is now refused on the new side of a create or compare-and-swap too (`Refusal::NullNew`; the parent's two call sites and import moved with it): measured on git 2.43 it deletes the ref through the CAS and creates nothing with exit 0. A test module in the file drives both predicates and the three refusals at both hash lengths and the length and alphabet boundaries; each test was witnessed failing under a mutation. After review (PR #126, the pass on `6a54b65`): the null-new refusal is witnessed through both public primitives against a real repository with the raw measurement executed, `design/26` step 5 states the rule, and the changed `# Errors` contracts and the diagnostic say exactly what was measured (`6e7604e`). After the second pass (on `def9320`, the last): the fixture pins its object format and the witness runs in a repository of each format, the design sentences distinguish the compare-and-swap from the delete, `refuse_new` is crate-visible and applied by `ensure_integration_ref` and both `IntegrationRefs` doubles, and the SemVer assessment of the added variant is in the body (`df494c8`). |
| `src/workspace_manager/parsers.rs` | `196f641` | 2026-09-04 | §6: no shared ownership, lock or clone; the one copy is the bytes of a Git path becoming the owned `PathBuf`, stated at the site. §7: the two `.ok()` are gone (one strict `decode_path` per platform family returns the `Utf8Error`, and every refusal says from which byte a path stops being UTF-8); `status_endpoints`'s `?` on `Option` is kept as a membership test's own verdict, stated at the site; the `--name-status -z` refusals are a `NameStatusError` at their source, naming the field, and `decode_changed_paths` is the one fold to `RepoWide`, spelling every variant. Fixed: `registration_checkout` trimmed leading whitespace and a trailing form feed that Git keeps as part of the path (measured, Git 2.43.0), so it bound a registration to a checkout Git does not read from it; `decode_git_path` was lossy on Windows while the path is identity in the parent, so `parse_worktree_records` returns `Result` and refuses, and its two callers propagate; both NUL-delimited grammars require their terminator and refuse an empty field, a doubled terminator, an unclosed final record and an attribute before any record, where before a cut-short answer was read as complete; the line-oriented `trim_end` on a NUL-terminated attribute is gone. Unit tests in the file, each hostile shape asserting which refusal it saw; eleven mutations run on the box, each killed by the named case. `reviews/FINDINGS.md` §48 has every row. Deferred to the parent (row 11) and `worktree.rs` (row 8): recording why a region became repo-wide, and the lossy `branch` attribute that `assert_publishable` compares with a refname. After review (PR #127, pass 1 on `3ef06f2`, five findings all P2, fixed in `8cf2d90`): a worktree record ends only at the empty attribute and an empty answer refuses, where a header had closed the open record; `HEAD`, `branch`, `detached` and `bare` are held to their own grammars while reasons stay verbatim; a relative `gitdir` is Git's own form (2.48's `worktree.useRelativePaths`) and resolves against its registration directory instead of refusing; `record_for` propagates a failed enumeration instead of answering absence; a changed path that is not one normalised repository path is repo-wide. Coordinator read of `8cf2d90` (two rows): the relative join is resolved lexically and never hands out `..` (`59fc2c6`), and the refname claim is kept to the byte set `can_be_refname` applies. |
| `src/workspace_manager/snapshot_ref.rs` | `dda65ab` | 2026-09-04 | Two types and no functions at the base, so §6/§7 literally found nothing and the sweep was §5 and §12, with the owner's amendment 7 passes (correctness, better implementation) applied. `ObjectId`: a full hexadecimal object id of either hash length that is not the null id, validated once at `ObjectId::new` through the parent's `is_object_id` and `is_null_object_id`, spelt lowercase so `Eq` and `Hash` are equality of the object; `SnapshotInput`'s three ids and `Snapshot`'s HEAD are this type, so neither can be built from a ref name, a short id or an option-shaped string (the funnel handed every value to `git commit-tree` and `git worktree add` unchecked). `Snapshot`: private fields behind one parent-visible constructor that builds the slot from a `SnapshotName` and holds the HEAD as one `SnapshotHead`, so the ephemeral commit is the HEAD rather than a second field that has to agree with it; accessors `slot`, `path`, `head`, `ephemeral`; `Clone` dropped (a copy is two values for one checkout). Parent moved with it: `Refusal::NotAnObjectId`, `add_snapshot` builds through the constructor and checks the `git commit-tree` line is an id before checking anything out at it, `remove_snapshot` reads the slot through the accessor; the engine's one construction site and two path reads, and the read and construction sites in the two suites. §6: no shared ownership or lock; the clones in `add_snapshot` are two of the name and one of the input's commit, each a small owned value stated at the site. §7: the funnel's three `?` are same-type propagation; the commit-tree line's refusal becomes a Git error naming the command. Seven tests in the file, each witnessed under at least one of ten mutations. Deferred to row 11 (the parent): consolidating `ObjectId` beside its predicates in `object.rs` and giving the ref primitives and the engine's `CommitSha` the same type. After review (PR #130, the pass on `be201d8`, one P2): the newtype is a spelling and cannot know the repository's object format, so a ref spelt as hexadecimal of the other format's length passed it and `git worktree add` followed the ref (measured, git 2.43); `add_snapshot` now resolves each input once against the repository, peeled to its role's object type, and accepts only an answer equal to the input (`Refusal::SnapshotInputResolvesElsewhere`, `SnapshotObject`), witnessed in a repository of each format (`a1c8d2f`). After the second pass (on `891cd7a`, one P1): a replacement object defeated that check, since `rev-parse` prints the replaced id while Git reads the replacement everywhere and the checkout materialised it; every command the manager runs now carries `GIT_NO_REPLACE_OBJECTS=1`, set where those commands are built, and the refusal names the object type each role requires rather than calling a full id malformed (`8a2b0bd`). After the third and last pass (on `8c5dbc3`): that isolation reaches only the children this manager spawns, not a gate or reviewer running in the snapshot (the host runner composes its own environment) nor `read_only_git`, so every claim is narrowed to manager-run commands and the open half — the runner's environment, the read-only path, and a `design/` sentence defining what an exact snapshot is measured against — is a deferred row in §50 awaiting the owner's design ruling; both directions of the wrong-type mistake are now the typed refusal and the engine's own malformed ids are Git errors (`3e00568`). |
| `src/workspace_manager/residue.rs` | `a17b8c5` | 2026-09-04 | §6: no shared ownership or lock, and no clone; `ResidueTarget` is four borrows and is now `Copy`, the reason on the line. §7: every `?` propagates the parent's `UpstrokeError` unchanged and each is kept as deliberate, with the module doc saying why and the three `# Errors` contracts saying what the error names. Fixed in the file: every residue name was read with `Path::exists`, which answers `false` for a permission failure, a symlink loop and a transient I/O error alike, so a git dir the process could not search classified `None` and `administrative_residue_at` called it quiescent; every name now goes through `name_present` (`symlink_metadata`, only `NotFound` is absence, and a link is not followed because a name Git takes with `O_EXCL` is the fact whatever it points at), and `index_lock_present` moved into the file from the parent, its only caller — the one line this sweep changes outside the file. The adds' after phase and their residue element are the two states of one `add_state` reading. Three tests, each witnessed under a stated mutation. **Three frontier passes (`b5c12e0`, `f161c9e`, `dfc238c`) went to a programme this sweep attempted and then reverted at `59f2bd99c95c80f1a2a011a78ebe34b43fbf4555`**: making the parent's inspections report failure instead of folding it into absence. Each round's repair introduced the next defect — an object-store readability scan that missed alternates and could block on a FIFO, then a registration scan with the same unbounded-read class, and a fold-into-absence defect one level deeper each pass. The parent is master's again, and `reviews/FINDINGS.md` §51 carries a row per case for row 11's sweep: the dangling `gitdir` symlink Git omits from its listing, `GIT_TRACE` making absence an error, `direct_ref_target` and `quiescence` folding, the gitfile grammar Git parses and this does not, and bounds on every read of a repository-controlled file. |
| `src/workspace_manager/worktree.rs` | `5e60c5e` | 2026-09-04 | §6: no shared ownership, lock or clone; `Clone` dropped from the record (nothing copies one; the parent moves the path into its refusal through `into_path`), kept on `VerifyFailure` and `Quiescence` with the reason stated on each; the copies in `from_porcelain` are borrowed porcelain bytes becoming the owned record. §7: no `?`; the constructor returns a typed `MalformedAttribute` and the parser joins it after the record number. Fixed: `WorktreeRecord` holds its attribute grammar by construction (private fields; `from_porcelain` the one constructor, applying object.rs's `is_object_id` to `HEAD` and `can_be_refname`, moved here from parsers.rs, to `branch`; the producer moved with it in `219e9e0`); `branch` is the bytes Git printed and `has_checked_out` compares them with the refname exactly, closing `SWEEP-PARSERS-008`; `lock_reason` and `prunable_reason` say that a label printed with no value is `Some("")` (measured, Git 2.43.0) and `is_initializing` is the one spelling of the word, adopted by the parent's `quiescence`; `HeadMismatch` displays as a lowercase fragment. Seven unit tests in the file, nine mutations run on the box and each killed by a named test. `reviews/FINDINGS.md` §52 has every row. Deferred: `locked` stays `pub(super)` until residue.rs (row 6, PR #128) can move its two sites to `is_initializing`, in the base merge-in that follows; the engine renders no `VerifyFailure` (`run.rs` drops it from `RetryOutcome::Close`); a `Display` for `ResidueElement` (row 24); typed object ids and a typed tree difference for `Quiescence` and `VerifyFailure` (object.rs and the parent's `index_differs_from`, row 11). After review (PR #131, pass 1 on `ea4fd74`, five findings, three P2 and two text; four fixed in `5e19848` and one deferred as `SWEEP-WORKTREE-013`): every rule of the record is applied by `OpenRecord` as the parser feeds it each attribute, so a refusal names the first attribute outside the grammar in attribute order and `detached` or `bare` twice refuses like the rest; `is_initializing` is documented as Git's word compared exactly and not as provenance, a writer's `git worktree lock --reason initializing` being indistinguishable from Git's, with the engine-written lock token proposed to row 11 (`SWEEP-WORKTREE-013`). After the second pass (on `c0eb8c5`, four findings, three P2 and one text, all fixed in `1720d91`): `OpenRecord::close` refuses a set of attributes that is not a worktree -- bare, or a HEAD with exactly one of `branch` and `detached`, the rule Git 2.43.0 was measured printing over twelve shapes -- so `branch()` answering `None` is a fact about the worktree and `assert_publishable` no longer reads malformed evidence as absence; `VerifyFailure::Unpopulated` reports the lock it saw rather than a history; and the module quotes the retired decision record as its own words, the trust-boundary question going to the owner. After the third pass (on `9dd3a79`, six findings, no P1, which converged it): `can_be_refname` applies every rule `git check-ref-format` documents for a full refname, so a `branch` the record accepts is one Git would accept; `has_checked_out` is exact bytes and no longer certified as ref identity, since a case-insensitive files backend can hold two spellings of one ref (`SWEEP-WORKTREE-015`, deferred with the parent's check/use race, `SWEEP-WORKTREE-016`); and the retired record's normative wording is out of the module entirely, the missing design mapping being `SWEEP-WORKTREE-014`. |
| `src/workspace_manager/tests.rs` | `31e2e44` | 2026-09-04 | The suite of the Worktree/Snapshot/Ref/Object funnel, 8,529 lines and 126 `#[test]` items at the base `95c5bd3` (122 of them on Linux), swept under the owner's amendment 7 with the tests as the whole job rather than a coda. §6, re-derived by reading after pass 1 found the first version of this sentence false: no `Rc`, and three kinds of sharing each with a reason at its site -- the `Arc<Mutex<HookHarness>>` the `harness()` helper builds, two holders with independent lifetimes and the observer `src/workspace_manager/hooks.rs` states; `SAMPLED_LAUNCHES`, **a lock this file does own**, process-global because the record must be written by the statement that performs the kill; and six `Arc<Atomic...>` cloned into a thread or an observer closure, which arrived with PR #109's merge-in and are not this sweep's lines. §7: no `.unwrap()`, and the three `?` are inside `parse_budget_spec`, each mapping into a `BudgetSpecError` variant that names what was wrong with the operator's spec; what the rule found was the discard clause, and two `let _ =` are gone with the things they were hiding. **Two groups did not pin what they claimed.** The `IdUnread` abort accepted `!helper.status.success()`, which the helper's own `unreachable!` panic also satisfies: measured, with `Injection::Kill` replaced by `Injection::Proceed` the test passed, and the oracle is now `died_by_abort`. The intent-ordering assertion compared two positions in a first-observation log the test itself appended to in that order, so no edit to `src/` could falsify it; it is deleted and the claim named where it is enforced. **One public method had no test in this suite at all:** `candidate_diff`, whose one appearance was the hostile-slot grid, where every call is expected to fail -- a body diffing from current HEAD, and one dropping `--binary`, each passed master's whole crate suite of 1,903 tests. **Three tests are deleted as weaker duplicates** of the swept `src/workspace_manager/parsers.rs`, one of them vacuous on Windows because its `/repository` fixture is not absolute there. The hostile-slot grid gains the legal-name control it never had, both source censuses now blank through `crate::effects` and carry the positive control §12 requires, and `launch_end` reads the shared `died_by_kill` instead of a second copy of the platform fingerprint. 19 rows fixed and 10 deferred, 13 P2 and 16 P3 with no P1 at any point; `reviews/FINDINGS.md` §54 has every row, each fix witnessed against a stated mutation. Deferred, each naming its file: 61 refusals asserted by message substring rather than by value (needs the parent's flattening decided, row 11), `SampledChild` and the kill-helper launch duplicating `src/workspace_manager/fixture.rs` (each also edits `effects/allowlist.toml`, whose funnel row counts this file's process sites), `recover_sample` comparing two discarded results, the acted-through generator reading metadata with `.ok()`, `recovered` true by construction for the two commit-tree sites (row 28 owns the type), and `--no-ext-diff` and `--no-textconv` unwitnessed here. After review (PR #136, pass 1 on `966775e`, five findings, four P2 and one P3, no P1, every one of them a test or census that appears to guard something it does not, and three of the four P2s inside this sweep's own repairs): the `candidate_diff` test staged one more file after the capture, because `reset --soft` leaves the index equal to the tree just captured and a body ignoring `tree` and diffing the index produced identical output; the sampler asserts a classifier refusal before anything derived from the tally, since a refusal became `None` and the generic total assertion fired first with the refusal's words unprinted; the argv census counts **every** string literal in each funnel body and declares what each is, the reviewer's `.into()` reproduction having moved neither `OsString::from` count, and the claim that it stops a funnel growing an argument is withdrawn as more than a text census can hold; and `kill_git_child` records the kill's error beside whether the child had already exited, where discarding it let a shape's kills all fail with its firing count intact and the global floor met by another shape. Each is witnessed by running the reviewer's own reproduction at `966775e` and at the repaired head. The P3 corrected this record: the file does own a lock, `SAMPLED_LAUNCHES`, and the §6 paragraph is re-derived by reading. After pass 2 (on `142c321`, two findings, both P2, no P1, both again inside this sweep's own repairs): the census's function extent is found in the fully blanked text, since a raw string carrying four spaces and a brace forged a boundary and truncated the body while the declared counts still matched; and the abort oracle compares against a **measured** abort — a child whose whole body is `std::process::abort()` — because the shared predicate's Windows arm is a negation that accepts `process::exit(1)`. The second is witnessed on every platform by a test of the oracle rather than of the funnel, the Unix legs being unable to see a Windows-only mutation. |

## Second pass owed

The first five files were swept under a narrower brief than the one now in force. That brief
asked for §6 and §7 conformance; from 2026-09-04 a sweep also asks for correctness defects, for
a better implementation where the current shape is clumsier than it needs to be, and for tests
that would not survive the obvious mutation.

These five are not unswept, and their frontier passes did find real defects — the time-of-check
race in `containment.rs`, the null id accepted on the new side of a compare-and-swap in
`object.rs`, a registration bound to a checkout Git does not read from it in `parsers.rs`. What
those passes did not ask for is the second limb: the shape improvement nobody raises because the
code is already correct.

§7's panic surface post-dates every row in the swept table, not just the first five: the rule was
tightened on 2026-09-04 and the last of these landed the same day under the brief as it then read.
What is owed on the eight production files is the indexing half — each `v[i]` and `&v[a..b]`
dispositioned, or replaced by `get`, `first`, `last`, `split_at_checked` or a pattern. That is a
cheap re-read rather than a session, and the pull request that lands the lints can settle those
eight from its own output, since the run above already names every site.

The ninth row, `src/workspace_manager/tests.rs`, owes something different and smaller than an
earlier draft of this section claimed. Its indexing sites fall under the test allowance the section
above takes, so nothing is owed there. Its one `unreachable!`, at line 7390, is denied in tests like
any other and needs an `#[expect(clippy::unreachable, reason = "…")]` or the proof that removes it —
one site, inherited from before the sweep rather than introduced by it.

They are listed here so the gap is recorded rather than forgotten. The queue comes first: a file
with no pass at all earns attention before a file that has had several. Decide whether to spend
sessions on these once the queue is empty.

| File | Swept at | Under | What a second pass is looking for |
|---|---|---|---|
| `src/workspace_manager/containment.rs` | `2dd1350` | seven passes | Shape of the walk and the per-primitive re-check; whether the refusal vocabulary is larger than it needs to be |
| `src/workspace_manager/hooks.rs` | `f58747a` | three passes | The `Arc<Mutex<HookHarness>>` was kept with its lifecycle stated; whether the protocol needs shared ownership at all |
| `src/workspace_manager/naming.rs` | `5a0ae59` | four passes | The hand-written wire reader against the derived one; whether `Slot` and `SlotId` earn being two types |
| `src/workspace_manager/object.rs` | `af382fa` | two passes | Whether the two refusal types and the parent's flattening are one concept too many |
| `src/workspace_manager/parsers.rs` | `196f641` | one pass | The two NUL-delimited grammars are parsed separately; whether one reader serves both |

`src/workspace_manager.rs` (queue row 11) reads every one of these files' call sites when it is
swept. That session should record improvements it notices in the children as rows here, which is
the cheapest form this second pass can take.
