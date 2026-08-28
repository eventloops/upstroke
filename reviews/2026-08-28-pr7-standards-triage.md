# 2026-08-28 — standards review of the merged head: triage for the owner

The 321 rows are filed in `reviews/2026-08-25-pr7-standards-worklist.md`, routed
101 to W10.2 and 220 to W10.3, each salvaged by the sha256 of its region. This file
is only for the three things the rider does not decide.

Counts here are computed, not restated; the commands are in the commit message of
`38ffdb4` and in `analyze`-style one-liners over `findings.json`.

## 0. What the review can and cannot tell you

**The design provides no redundancy, and that is a limitation of the review, not
merely an unanswered question.** The partition is exclusive by construction — every
one of the 96 `.rs` files is read by exactly one lens, verified: zero files and zero
regions are cited by more than one lens.

Two consequences, and the second is the one that matters:

1. "No material disagreement between lenses" is **not a finding of agreement**. No two
   lenses read the same text, so there was never an opportunity to disagree.
2. **A defect missed by its lens is missed unrecoverably.** Nothing downstream can
   catch it: no second reader covers that file, the hash gate only checks that a
   *filed* finding's quote is real, and the salvage control only proves filed rows
   relocate. Every one of those controls validates what was found; none of them
   bounds what was not.

So the 321 findings are a **lower bound from fourteen single passes**, not a survey
with a measured miss rate. Assurance is exactly one reader deep everywhere, and the
per-lens budgets — 9,558 to 20,722 lines — are large enough that a single pass over
one of them plainly cannot be exhaustive; the coverage blocks say so themselves, and
`L1-fold` for instance declares "read in full: none" and names four test-body line
ranges it swept structurally rather than read.

What would change this, if the owner wants assurance rather than breadth: run a
second lens over a sample of areas and measure the overlap. Two independent passes
over the same file yield a capture–recapture estimate of what one pass misses, which
is the only way this review can report a miss rate instead of a count. The tooling
takes it directly — the partition is one file, `lens-manifest.sh`.

**Two things it did confirm about its own conduct**, both measured over all 321:

- **0 findings carry "no citable section".** Every row names a section.
- **0 findings propose an edit.** The frozen-layer instruction held: layer lenses
  observed and cited, and none phrased a finding as an instruction to change a file.

**Citation wording varies** — 26 distinct section strings for 12 actual sections
(`§12 tests`, `§12 Tests`, `§12 testing strategy`, …). Rows are filed verbatim rather
than silently normalised. Normalised: §12 102, §7 59, §8 42, §14 36, §5 20, §9 16,
§4 13, §3 12, §10 8, §13 7, §6 5, §11 1.

## 1. Findings that cite a section but read as defects

**28 of 321** describe a security or correctness consequence rather than a style
miss. They are correctly filed — each cites a real §8/§9/§14 clause — but "the
cleanliness sweep will decide" may be the wrong disposition and the wrong venue.
They fall in three classes.

### (a) Lexical path comparison standing in for a security decision — 5 sites

Canonicalisation fails, and the code silently compares spellings instead:

| file | what |
|---|---|
| `src/runner/container/exec.rs` | **confinement uses a lexical prefix comparison as its entire filesystem-containment decision** |
| `src/engine/topology/recover.rs` | private-root comparison falls back to lexical equality on every canonicalisation error |
| `src/engine/topology/recover.rs` | owner-record public-directory authentication falls back to lexical spelling |
| `src/engine/topology/create.rs` | `canonical_string` substitutes an unverified lexical path when evidence is unavailable |
| `src/rundir.rs` | the private-half ownership proof falls back to an uncanonicalised public path |

The `exec.rs` one is the container's confinement boundary, which `DESIGN.md` §4
treats as a trust boundary rather than a convenience.

### (b) Unbounded reads on input that crosses a trust boundary — 9 sites

`src/events/log.rs` (whole-log read bounded only by an attacker-controlled file size;
unbounded `read_to_end` on the growing log), `src/engine/topology/recover.rs`
(persisted owner/commit records), `src/config.rs`, `src/engine/attempt.rs` and
`src/review.rs` (agent-authored artifacts into prompts, no per-artifact or aggregate
bound), `src/topology/schema.rs` (arbitrarily long header line),
`src/runner/container.rs` (both streams captured with no pre-allocation bound and no
timeout), and `src/validate.rs` (cycle detection recurses the untrusted task graph
with no depth bound).

### (c) Lossy UTF-8 conversion of path **identity** into durable records — 10 sites

§8 says a lossy display string is for diagnostics only, **never identity**:
`engine/topology/create.rs` (×3: creation marker, `run_started`, owner/commit
identity), `engine/topology/dispatch.rs` (`task_dispatched` worktree identity),
`engine/topology/scaffold.rs`, `engine/coordinator.rs`, `engine/preflight.rs`,
`rundir.rs`, `gates.rs`, `agent/codex.rs`, `runner/container.rs` (bind-mount source
inside Docker's comma-delimited mount syntax).

## 2. The lossy-path class versus W4's scope

Class (c) is the same class as `CommandSpec.program`, and it is wider than the record
that just settled that field. The `OsString` record scopes itself to "the one
measured field", with `args` and `env` audited in W4 and "no speculative widening".
These ten sites are not mentioned.

The one that changes the character of the question: `src/topology/events.rs` records
path identity as `String`/`Option<String>` in `RunStarted4`. That module is
**serde-visible**, and recorded runs replay from it — so under the freeze charter a
type change there is **Class C and a schema bump**, not the mechanical widening W4
contemplates.

Whether W4's scope grows to cover the durable event schema, or that becomes its own
decision record, is the owner's call and is not made here.

## 3. Frozen-layer edits

None proposed, by construction and by measurement. The 101 W10.2 rows all sit inside
`src/topology/**` or `src/engine/topology/**` and are routed to the pass, which is
where a repair to them belongs.
