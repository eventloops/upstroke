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

## 1. The defect-shaped findings, after the owner's ruling of 2026-08-28

The 28 findings escalated here have been ruled on, and the ruling **split them the
opposite way round from how they were filed**. What decides is not how alarming a
finding reads but which requirement keyword its clause carries, because `§1` grades
the evidence a deviation needs to the strength of the rule it deviates from:

> A `SHOULD` deviation needs a concrete reason in the code or pull request. A `MUST`
> deviation needs an explicit, reviewed change to this standard or to the controlling
> design—not an ad hoc exception.

`§8`'s path-representation bullet — *"Represent paths with `Path`, `PathBuf`, `OsStr`,
or `OsString`"* — carries **no requirement keyword**, while two sibling bullets in the
same section are explicitly MUST-tagged: *"Path containment checks MUST account for
`..`, absolute paths, symlinks/reparse points…"* and *"Event schema changes MUST
preserve or deliberately migrate supported historical runs"*. The author distinguished
MUST from untagged inside one section, so the untagged bullet is at most SHOULD.

### (a) The lossy-path class — **REJECTED**, 12 findings

Thirteen findings said path identity reaches a durable record through
`to_string_lossy`. One is struck outright (below) and the other twelve are compliant
documented deviations from a SHOULD, not defects. The reason is on the field:
`src/topology/events.rs`'s `TaskDispatched::worktree_path` says it is *"recorded as the
string a later process compares and re-derives… a platform path type here would make a
log written on one operating system a question on another"*, and
`src/engine/topology/dispatch.rs` cites that doc **by name at the call site**.
`create.rs`'s `canonical_string` likewise documents its fallback as deliberate and
paired with the matching fallback on the comparison side, so a filesystem that will not
canonicalise yields two equal non-canonical strings rather than one of each.

**The lens failure mode is worth naming**, because it is the reason 12 rows were filed
against a rule they satisfy: each lens cited `§8` and none engaged the rationale
sitting on the field it was citing. A reader that quotes a clause without reading the
justification the code offers against that clause will file a compliant deviation as a
violation every time — and `§1` makes that justification the whole test for a SHOULD.

**One row is struck, not rejected**: `src/engine/topology/scaffold.rs` is not
production. Its own header reads *"The schema-4 run a dispatch or attempt test
drives"*, and `src/engine/topology.rs` declares it `#[cfg(test)] mod scaffold;`. It was
tabled here without that being checked — this seat's error, of the same class it has
been catching in others.

### (b) The trust-boundary class — **ROUTING CHANGED**, 7 findings

These cite the bullet that **is** MUST-tagged, and they sit on a security boundary.
`§1` refuses an ad hoc exception for a MUST, so the documented in-code rationale that
discharges class (a) does **not** discharge these — and several of them are documented.
`recover.rs`'s `normalize` carries a substantial argument for its lexical fallback; it
is still an ad hoc exception, and the standard says so in as many words. The same kind
of evidence settles a SHOULD and cannot settle a MUST.

They are therefore **not** the sweep's work. They route to `reviews/FINDINGS.md` §2 as
contract/correctness rows with a named owner:

| file | § | observation | region sha256 |
|---|---|---|---|
| `src/engine/topology/recover.rs` | §14 | The explicit private-root comparison falls back to lexical equality on every canonicalization error. | `31b6eba9705daa9e…` |
| `src/engine/topology/recover.rs` | §14 | Owner-record public-directory authentication falls back to lexical spelling when canonical evidence is unavailable. | `388c5fbd6fab41a9…` |
| `src/engine/topology/recover.rs` | §8 | The recorded private-root locator is accepted without absolute-path or symlink/reparse-point containment validation. | `f2aa6763a72ff901…` |
| `src/rundir.rs` | §14 | The question-payload write boundary interpolates an unvalidated component into an authoritative path. | `1e5f413d6388eaee…` |
| `src/rundir.rs` | §14 | The answer staging boundary uses an unvalidated component as part of its write path. | `975a3033271821f9…` |
| `src/rundir.rs` | §14 | The private-half ownership proof falls back to an uncanonicalized public path when canonicalization evidence is unavailable. | `eb0463a6ae63dc8b…` |
| `src/runner/container/exec.rs` | §8 | Confinement uses a lexical prefix comparison as its entire filesystem-containment decision. | `3e0c3df5db999002…` |

`§8`'s own words on why a documented fallback is not enough here: *"Lexical
normalization alone does not prove filesystem containment."*

Not repaired by this seat, and not carried by #40 or #41.

## 2. W4's scope does not grow

The escalation asked whether W4 should absorb the lossy-path class, since one of its
sites — `src/topology/events.rs` — is serde-visible and a type change there would be
Class C and a schema bump rather than a widening.

**Ruled: it does not**, for three independent reasons, any one sufficient. There is
nothing to fix, per (a). Even if there were, `§8`'s *own* MUST makes an event-schema
change a migration of supported historical runs, categorically not the one-field
widening W4 is chartered for. And the `OsString` record scopes itself to *"the one
measured field"* with *"no speculative widening"*, so it cannot authorise what it
expressly declined to cover.

What the ruling added instead is a clause, not a record: the two live documents reason
oppositely about one trade-off — `events.rs` argues cross-platform log legibility for
`String`, the `OsString` record argues cross-platform replay of a non-UTF-8 program was
never meaningful — and both are right about their own subject. Ephemeral spawn-time
identity widens; durable wire-facing log identity stays `String`. That clause is in the
record's Consequences list, which W4 already opens.

## 3. Frozen-layer edits

None proposed, by construction and by measurement. The 101 W10.2 rows all sit inside
`src/topology/**` or `src/engine/topology/**` and are routed to the pass, which is
where a repair to them belongs.
