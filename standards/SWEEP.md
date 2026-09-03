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
every Rust file under `src/` is listed in the swept table, this paragraph and the activation
rule are deleted, and §6 and §7 bind the whole tree with no scoping. Errors are handled where
they arise; a `?` that survives a sweep is one the reviewer agreed was deliberate.

## Review queue

The large modules are being split into per-concern child modules, one pull request per parent.
Each file a split produces is queued here as the split merges, and is swept in queue order: one
session, one file, one review by Claude Fable 5.1 whose only subject is that file, then the pull
request that lands the cleanup and moves the row to the swept table. The parent module is queued
after its children, since it keeps whatever the split did not move.

| # | File | Lines | From | Merged |
|---|---|---|---|---|
| 1 | `src/workspace_manager/containment.rs` | 235 | #110 | 2026-09-03 |
| 2 | `src/workspace_manager/hooks.rs` | 200 | #110 | 2026-09-03 |
| 4 | `src/workspace_manager/object.rs` | 68 | #110 | 2026-09-03 |
| 5 | `src/workspace_manager/parsers.rs` | 274 | #110 | 2026-09-03 |
| 6 | `src/workspace_manager/residue.rs` | 400 | #110 | 2026-09-03 |
| 7 | `src/workspace_manager/snapshot_ref.rs` | 56 | #110 | 2026-09-03 |
| 8 | `src/workspace_manager/worktree.rs` | 135 | #110 | 2026-09-03 |
| 9 | `src/workspace_manager.rs` | 2,535 | #110 | 2026-09-03 |

Splits still open, to be queued when they merge: #106 (`topology::effects`), #107 (`rundir`),
#108 (`topology::fold`), #111 (`runner::host`), #117 (`agent::proc`).

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
| `src/workspace_manager/naming.rs` | `ecddff8` | 2026-09-03 | First pass `51feba7`, reviewed at `9f83b09` (PR #118, six findings, fixed in `a7b7c98`); second pass at `8d25472` (five findings: three fixed in `bc07f05`, the stale queue is PR #122's, the body claims the PR's); third pass at `cff812d` (four findings, fixed in `ecddff8`). `reviews/FINDINGS.md` §45 has every row. §6: no shared ownership or lock, and no clone: none written, and none in expanded serde code, since the record's `Serialize` and `Deserialize` are written by hand (each earlier pass had found one: `slot.clone()`, `keys().cloned()`, then the derive's `into = "String"`). §7: `from_intent_name` has three `?` sites and `Slot::from_parts`, which it shares with `SlotId`'s parser, five more; each returns the parser's `None` and is dispositioned on `from_intent_name` in terms of what "not an intent name" and "malformed intent name" mean to the directory walk, which has one action for both. Fixed: `from_intent_name` accepted non-canonical integers (`g03`, `g+3`) and so was not `intent_name`'s inverse; it now round-trips, pinned at the parser and, in `tests.rs`, through `reclaim_intents`. `safe_component` returns `Result` (§5); `validate` reuses `kind()`; `Slot::parts` is the one rendering that `relative` (a `PathBuf`), `intent_name` and the record's `SlotId` spell. The intent record is validated whole on read through a hand-written wire reader against the literal `IntentRecord::FIELDS`: `kind` is `IntentKind` (hand-written reader over `IntentKind::WORDS`), `slot` is `SlotId`, an identifier mirroring the relative path and never a path, parsed back into a validated `Slot` and required to agree with `kind`. Unit tests in the file for all of it. Deferred to row 9 (the parent): `write_synced` stages an intent as `<stem>.tmp` in the intents directory, and `intents()` refuses that residue after an interrupted write. |
