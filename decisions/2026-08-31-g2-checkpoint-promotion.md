# 2026-08-31 — the G2 checkpoint promotion candidate, and what it discharges

**Verdict.** This promotion is **the G2 checkpoint** that
`2026-08-25-checkpoint-merges.md` ordered. That record stays **controlling in
full**: nothing in it is superseded, narrowed, or set aside here. This record is
the reconciliation — it names each obligation that record imposes, points at the
evidence that discharges it, and states plainly which obligations are discharged
**now** and which are discharged **only when the panel and the serialized suite
report**. A candidate is assembled; a candidate is not a pass.

The candidate is `promotion/g2-candidate-assembly`, cut at
`50ed8c86ec60164011bfd393066c4c3696d3865b`. `master` before the promotion is
`76b6a784ae5562ac044d6ff9a15b68397bd9b0e0`.

## What the checkpoint record obliges, and where each obligation stands

| # | Obligation (`2026-08-25-checkpoint-merges.md`) | Where it is discharged | State at this candidate |
|---|---|---|---|
| 1 | #18 becomes the G2 merge; the candidate is cut after the G2 gate passes on the cleaned tree | `reviews/2026-08-31-g2-gate-report.md` | **Partly.** The candidate is cut and the gate report exists as artifact 1. The gate **has not passed**: six of eight artifacts are owed to execution this host may not perform. See §"What is not discharged" |
| 2 | Every open `reviews/FINDINGS.md` row triaged to repaired / carried-with-owner-and-re-opening-condition / closed-not-owed, before the candidate is cut | `reviews/FINDINGS.md` §35 | **Discharged.** 65 logical §2 rows normalized latest-disposition-wins; every live row lands in exactly one of the three buckets |
| 3 | §4's recurrence classes reviewed for structural guards at the same sitting | `reviews/FINDINGS.md` §35, "Recurrence classes" | **Discharged.** All 18 classes reviewed; each carries a guard verdict of mechanical, partial, or convention-only |
| 4 | The three-model panel attests the code **and** the audited ledger together | not in this repository | **Owed.** This commit is assembly, not attestation. No review of any kind was run to produce it |
| 5 | The G2 gate's eight artifacts produced on the extended input range before the candidate is cut; the gate report is artifact 1 | `reviews/2026-08-31-g2-gate-report.md` | **Partly.** Artifact 1 exists. Artifact 7's inputs are present and hash-pinned. Artifacts 2–6 and 8 are named, scoped, and **owed** |
| 6 | Inert by default, **verified not assumed**; the panel confirms it on the candidate | `reviews/2026-08-31-g2-gate-report.md` §"Inert by default" | **Discharged structurally, owed behaviourally.** Five proofs are verified by construction at this base; the panel's confirmation is still the panel's to give |
| 7 | No `0.2.0` tag until v0.2 completes | this record, and the gate report | **Discharged.** This promotion authorizes no tag. `Cargo.toml` stays `0.1.0`; `v0.1.0` is still the only tag |

## The input range, and the authority for it

The G2 gate's own `frozen_input_range` field names the unextended range —
PR4, PR5, PR6 and PR7 merged on top of G1. `2026-08-25-checkpoint-merges.md`
requires the artifacts on the **extended** range instead, per packet erratum
**E5** (`upstroke-lab:packet/2026-08-25-g2-pass-errata.md`), which adds the G2
pass slice and the whole-tree sweep slice.

The extended range is what this candidate is assembled against. The authority is
the owner's direct promotion amendment of 2026-08-31, which requires the eight
artifacts over the E5-extended range; that amendment is the adoption. The
erratum itself lives in the private companion repository and is not reproduced
here.

**One thing a later reader should not have to re-derive:** the packet record's
own range field and the range this candidate is measured against are different,
deliberately, and the difference is E5 plus the owner's amendment — not drift.

## What is not discharged, stated so it cannot be mistaken for a pass

- **The gate has not passed.** Six of the eight artifacts require executing the
  full suite, the Docker-gated suite, and the hosted macOS and Windows legs.
  Assembly does not produce them and this record does not claim them.
- **No attestation.** Obligation 4 is untouched. The three-model panel has not
  run, and an agent session has no standing to merge to `master` in any case
  (`CLAUDE.md`; `2026-08-23-retire-app-attestation.md`).
- **The full suite has not run for this commit.** Root serializes it globally
  after the commit lands. Nothing here asserts a suite result.

## Rollback, and why its bar is deliberately high

The eventual merge of this candidate into protected `master` is rolled back with

```
git revert -m 1 <MERGE_OID>
```

`-m 1` keeps `master`'s own first-parent line and undoes the second parent — the
whole integration side — in one commit. That is the only rollback this promotion
has, and it is a one-way door in practice: once the revert is on `master`,
**re-promoting the same work requires reverting the revert**, because the
original merge base is already an ancestor and a plain re-merge brings nothing
back. A second promotion therefore starts from a revert-of-a-revert, which is
harder to review than the original merge and reads worse in history.

That bar is intended. The checkpoint exists to move the tree at its
maximum-confidence moment; a rollback that were cheap would invite merging
before that moment arrived.

## Relationship to other records

- `2026-08-25-checkpoint-merges.md` — **controlling, unchanged.** This record
  reconciles against it and supersedes no part of it.
- `2026-08-21-stacked-slice-prs.md` — unchanged; already superseded in part by
  the checkpoint record, not further by this one.
- `2026-08-20-review-invalidation-scope.md` — unchanged, and it governs this
  candidate: a push whose whole diff from a reviewed head is confined to
  `reviews/FINDINGS.md` keeps the review, and everything else invalidates it.
  This commit touches `decisions/` and `reviews/` beyond that file, so it is
  **not** exempt-only and does not inherit any earlier head's review.
- `2026-08-23-retire-app-attestation.md` — unchanged; the owner's merge remains
  the attestation.

## Addendum, 2026-08-31 — the serialized suite ran green, and what that moved

Appended rather than applied in place, per this folder's immutability rule. The
table above stands as written; this section states what its rows 1 and 5 now
read, and what did not move.

Root ran the globally serialized suite at the exact committed candidate head
`50a84acd3ebf5f0ecffc35a7a5b4ea68960310f9`:
`upstroke-build cargo test --all-targets --all-features`, `rc=0`, fresh compile,
library **1801 passed / 0 failed / 34 ignored**, binary **8 passed / 0 failed**,
example 0 tests, with the `real_docker_*` tests exercising a live **Docker
server 29.7.2**.

**Obligation 5 moves, partly.** Artifact 6 — the Docker-gated suite result with
its environment noted — is now **produced** on Linux. Artifact 7's allow-placement
scan **passed**, so its result joins its pinned inputs. The oracles beneath
artifacts 2, 3, 4, 5 and 8 all executed and passed.

**Obligation 1 does not move, and this is the part worth being exact about.**
The gate still does not pass. Six artifacts name a captured form — a table, a
log, a transcript, a diff, an observed-class histogram — and none of those was
collected. A passing oracle is evidence for an artifact; it is not the artifact.
The run is also Linux-only, and macOS and Windows are hosted evidence this host
does not produce.

**Obligation 4 is entirely untouched.** The three-model panel has not been
convened and **its membership is not settled**. *[Superseded by the third
addendum below: membership was settled by ruling 2 at this same head. The panel
still has not convened.]* The gate's own pass rule needs
questions answered and no open critical/high finding, which is a review outcome;
no suite result can supply it. This remains the blocking obligation.

**Obligation 6 is strengthened, not re-derived.** The inertness proofs were
structural — read from the tree, not run. A fresh compile necessarily evaluated
the four compile-time schema assertions, and
`max_parallel_above_one_is_refused_rather_than_read_past` passed with the rest.
The schema-4 **visibility** conflict recorded in the gate report stands
unchanged: `src/lib.rs:49` is `pub mod topology;`, the surface is **not**
`pub(crate)`, and the condition is met behaviourally rather than by visibility.

Evidence: `reviews/2026-08-31-g2-gate-report.md`, "The serialized gate run".

## Addendum, 2026-08-31 (second) — owner rulings 1 and 2

Appended, not applied in place. Two owner rulings of 2026-08-31 land against
this record and are recorded in their own files:

- **`2026-08-31-inertness-premise-behavioural.md`** (ruling 1, ratified as
  amended) formalizes obligation 6. The inertness premise is **behavioural** and
  holds at `50ed8c86`; the visibility form of the claim is **retired as false**
  and must not reappear in any rewritten evidence. Binding amendment 1b corrected
  the gate report: a library consumer can **write** schema-4 durable state
  through the checked funnel, not merely name the vocabulary. Binding amendment
  1a carries that as `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED` in `reviews/FINDINGS.md`
  §37, owned by the project owner at the **PR12 activation slice**. **No
  visibility change to the code is authorized in this promotion**; narrowing is
  post-G2 managed debt. The addendum above stands, with its "three compile-time
  schema assertions" corrected to **four** (`src/topology/schema.rs:98-101`).
- **`2026-08-31-panel-seats.md`** (ruling 2, ratified as amended) fills
  **obligation 4**, which was the blocking one. The panel is three seats — Sol
  `gpt-5.6-sol` at `max`, `claude-fable-5` at `max` explicitly pinned, and
  `gemini-3.1-pro-high` via `agy` by absolute path — each with a recorded
  invocation guard. Binding amendment 2a withdraws every pre-authorized
  fallback: **one repair attempt, then wait for the owner, and the panel does not
  convene partially.**

**Obligation 4 is now *defined*, not discharged.** The seats are ratified; no
seat has run. The panel convenes only after this branch lands and the PR #18
body is rewritten, on a stable exact head, and any head movement afterwards
invalidates the seats that already ran. Nothing in this addendum makes the gate
pass.

## Addendum, 2026-08-31 (third) — the PR #80 review, and two corrections to this record

Appended, not applied in place. The sole fresh review of `e174d086` returned
CHANGES_REQUIRED with four validated blockers. Two of them land on this record
and both are correct.

### Correction 1 — the checkpoint ordering was reversed, and is restored here

The obligation table above says of row 1 *"the candidate is cut"* at
`50ed8c86`, while the same record admits the gate has not passed and artifacts
are missing. That reverses the order
`decisions/2026-08-25-checkpoint-merges.md` fixes, and the review was right to
call it a narrowing of an immutable decision without a successor ruling. It was
not intended as one, and it is not one.

**`50ed8c86` is the pre-assembly baseline, not the cut candidate.** No candidate
has been cut. The controlling order stands exactly as written: the G2 gate
passes on the cleaned tree, the eight artifacts exist, the full-ledger audit is
complete — **and then** the candidate is cut.

The review's sharpest point is the one to keep: **the ledger audit was written
after `50ed8c86`, so that commit could not have carried it.** A commit cannot
carry evidence that postdates it.

**The candidate will be the integration landing head** — the head
`codex/parallelism-design` carries after this evidence branch lands under the
standing per-head ceremony and the outstanding artifacts and gates are complete.
Rows 1, 2 and 5 of the table above are to be read with this correction: they
describe evidence produced **on the baseline, toward** the cut, not a cut that
has happened.

**Still owed before any candidate can be cut**, stated plainly:

- **Artifacts 2, 3, 4, 5 and 8** in their captured form — the parity outputs, the
  fault-injection evidence table and its observed-class histogram, the
  before/after inventory and husk census table, the user-checkout diff, and the
  per-invocation boundary/image-id log with the inspection- and probe-refusal
  transcripts. Their oracles pass; the artifacts do not exist.
- **macOS and Windows evidence.** The serialized suite was Linux-only.
- **The panel.** Three ratified seats, none of them run.
- **This evidence branch landing**, and a fresh exact-head review of the landing
  head.

### Correction 2 — the panel text was stale at its own head

This record's second addendum says the panel "has not been convened and **its
membership is not settled**". The first half is still true; the second was
already false when written, because the same commit adopted
`2026-08-31-panel-seats.md`. **Membership is settled and ratified; no seat has
run.** The gate report carries the same correction.

### The other two blockers

Recorded here for traceability; both are repaired outside this record.

- **The full-ledger audit was not full.** §35 projected only §2's rows, leaving
  live deferred rows elsewhere — including all four `PR73-*` rows and
  `PR64-CLEANUP-003-SCRATCH-PRECLEAN` — uncounted. `reviews/FINDINGS.md` §38 now
  projects **every canonical row in the ledger**. Obligation 2 above was
  **overstated as discharged** and is discharged by §38, not by §35 alone.
- **The coverage map invented an exemption.** It treated `decisions/`,
  `proposals/`, `docs/`, root Markdown and ignore files as review-exempt, where
  `2026-08-20-review-invalidation-scope.md` authorises **exactly**
  `reviews/FINDINGS.md`. `reviews/2026-08-31-g2-first-parent-coverage.md` is
  reclassified; residue rises from 7 units to **18**.

## Addendum, 2026-08-31 (fourth) — the collision ruling, the owed set corrected, and this record's own labels

Appended, not applied in place. By owner ruling of 2026-08-31 this folder is
append-only without exception from here: no in-place edits of any kind,
supersession markers included; correction is by appended erratum only. The
in-place marker the previous commit added to the second addendum predates
that ruling and is disclosed here as the last of its kind.

### The step-1 collision, ruled and recorded

The owner ratification's authorized step 1 was executed twice: this branch's
`e174d086` (committed 14:08:27 UTC) won, and the adjudication seat's
delegated assembly worker — pre-flighted against the ratified base
`8dff3e91` under an explicit lease — **aborted at pre-flight with zero
changes** when it found HEAD moved. No double-commit occurred; the
one-writer-per-region discipline held by construction. The lease was
released **unused**. Durable evidence, outside this repository by design:
`DRIFT-REPORT-2026-08-31-step1-collision.md` and
`LEASE-2026-08-31-step1-assembly-docs.md` in the promotion artifacts
directory.

The owner ruled: **`e174d086` is canonical step 1.** Its four disclosed
extras beyond the three ratified amendments — the gate report's §3 heading
rescope, the three→four const-assertion citation correction
(`src/topology/schema.rs:98-101`, verified), this record's second addendum,
and the in-place word edit to the first addendum — are **retroactively
ratified as corrective**. The ratification's return-before-committing clause
was binding and was not honored for them; from here **any material deviation
beyond what a ruling authorizes returns to the owner before committing,
without exception**. `upstroke_root` is the sole executor for the remaining
sequence; `promotion_decisions_fable` stood down correctly and is a
read-only verifier. This addendum exists so the panel finds the deviation
triaged rather than discovering it.

### The owed captured set includes artifact 7

The third addendum's "Still owed" list named artifacts 2, 3, 4, 5 and 8 and
dropped artifact 7, while the gate report counts **six** uncaptured. The
gate report's artifact table is the **single enumerator** of the owed set;
where any list here and that table disagree, the table is authoritative.
Corrected, in captured form: **artifacts 2, 3, 4, 5, 7 and 8** — the parity
outputs; the fault-injection evidence table and its observed-class
histogram; the before/after inventory and husk census table; the
user-checkout diff; **the allow-placement scan's printed output as a
standalone artifact** (artifact 7 — its five inputs are hash-pinned and the
scan passed in the serialized run; the output was not captured); and the
per-invocation boundary/image-id log with the inspection- and probe-refusal
transcripts. macOS and Windows evidence, the panel (three ratified seats,
none run), and this evidence branch landing under the per-head ceremony
remain owed exactly as the third addendum states.

### This record's own earlier labels

Read the verdict's "The candidate is `promotion/g2-candidate-assembly`, cut
at `50ed8c86…`" and every "candidate head" in the first and second addenda —
including the first addendum's "exact committed candidate head `50a84acd…`"
— under the third addendum's correction: `50ed8c86` is the **pre-assembly
baseline**, `50a84acd…` is the **committed evidence head**, and no candidate
has been cut. The earlier sections stand as written, per the append-only
rule; this paragraph is the reading rule.

### Obligation 2's discharge chain

§35 normalized §2 (65 logical rows; correct for what it measured). §38
extended the projection ledger-wide and repaired §35's completeness defect,
but published a carried/closed split and a "52 minus three = 49" derivation
its own enumeration does not support. `reviews/FINDINGS.md` **§39** is the
corrected, mechanically re-derivable projection — at `e174d086`: **197 ids =
94 repaired + 77 carried + 17 settled + 7 closed + 2 struck**, with §38's
197 total and its 26-missed-rows table confirmed and its 75/9 split and 49
corrected to 77/7 and 51. Obligation 2 is discharged by §35 and §38 **as
corrected by §39**.

## Fifth addendum — artifact membership has one operative enumerator

The fourth addendum's captured-form list, including its line-268 restatement,
was a corrective historical record, not an independent operator checklist.
It is non-operative from this addendum forward. The artifact table in
`reviews/2026-08-31-g2-gate-report.md` is the sole operative enumerator of
artifact membership and capture state. Every decision, revision, PR body, and
operator checklist refers to that table without restating its members. If any
historical list in this record disagrees with the table, the historical list
must not be used to cut or approve a candidate.
