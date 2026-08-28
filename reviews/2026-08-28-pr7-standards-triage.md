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

**What it confirmed about its own conduct**, measured over all 321:

- **0 findings carry "no citable section".** Every row names a section.
- **0 findings are phrased as an imperative instruction to change a file** — and that
  is the claim, narrower than the one an earlier revision made. It said "0 findings
  propose an edit", which is **false and is withdrawn**: the measurement matched
  imperative phrasing and does not catch a remedy named in the indicative. Two rows do
  name one. `src/config.rs`'s container-path row says the value *"needs a native or
  dedicated target-path type with explicit platform semantics"*; the task-path-hint row
  says *"a validated `PathHint` or path-pattern newtype could preserve their parsing
  and matching contract"*. Both are quoted here rather than left for a reader to find.
  What **is** still true is the thing the frozen-layer instruction was about: no row
  instructs the pass to edit a frozen file, and no lens produced a diff.

**What the review's own artifacts cannot show you from this repository.** The lens
manifest, the fourteen prompts, the raw lens logs and `findings.json` are **not** in
the tree. Three claims rest on them and are therefore **seat-attested rather than
repository-verifiable**: the exclusive file partition, the fifteen source files that
drew no finding, and the eight-gate run that preceded the lenses. They are stated as
attestations, not as things this pull request lets you check. What the repository *does*
let you check is every filed row: its file, its region hash, and that the hash relocates
to exactly one contiguous region at `3e5212d`.

**Citation wording varies** — 26 distinct section strings for 12 actual sections
(`§12 tests`, `§12 Tests`, `§12 testing strategy`, …). Rows are filed verbatim rather
than silently normalised. Normalised: §12 102, §7 59, §8 42, §14 36, §5 20, §9 16,
§4 13, §3 12, §10 8, §13 7, §6 5, §11 1.

## 0b. What this file is, and what it is not

**This document is a report, not an authority.** It records rulings the owner made; it
does not make them and it does not constitute them. Every ruling it reports is
constituted by a decision record — `decisions/2026-08-25-commandspec-program-osstring.md`,
`decisions/2026-08-25-checkpoint-merges.md`, `decisions/2026-08-24-pr3-layer-freeze-charter.md`
— and **none of those three exists in this tree.** They land in pull request #40.

An earlier revision of this file wrote its rulings in a voice that made a review
document read as living product authority, which `decisions/README.md` reserves to
`DESIGN.md` and the records. Two consequences follow and both are now stated wherever
they bite:

1. **Merge order: #40, then #42, then #41 — a hard dependency, not a recommendation.**
   An earlier ruling called it soft and ordered only #42 before #41; the owner
   corrected both on 2026-08-28. #40 must land first because the three records above
   are its content; merging this pull request first would leave a review document as
   the only statement of rulings whose records do not exist. #42 must land before this
   one because §1(b) below says the trust-boundary cluster *"routes to
   `reviews/FINDINGS.md` §2"*, and that is true only once #42 lands.
2. **Where this file states a ruling, it cites the record that will carry it.** Where
   no record exists yet, it says the ruling is reported and pending its record rather
   than writing it as settled law.

## 1. The defect-shaped findings, after the owner's ruling of 2026-08-28

The findings escalated here have been ruled on, and the ruling **split them the
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

### (a) The lossy-path class — **SPLIT**: 12 rejected, 1 reopened as a live defect

Thirteen findings said path identity reaches a durable record through
`to_string_lossy`. An earlier revision of this file rejected twelve and **struck** the
thirteenth. The owner corrected both halves on 2026-08-28. The class now splits on a
single question, applied per site: **does the code carry a rationale that addresses the
lossy conversion or the `String` choice itself?**

**The arithmetic, stated because an earlier revision's did not close.** The class holds
**thirteen** findings. Twelve are rejected — the eleven the earlier revision rejected
other than `canonical_string`, plus the scaffold row it wrongly struck — and one,
`canonical_string`, is reopened. An earlier revision opened this section with "the 28
findings escalated here have been ruled on" while accounting for twenty, and that
sentence is withdrawn: **28 counted findings, and the class lists below count sites**,
which are not the same unit and were never going to add up. The three classes are
thirteen lossy-path findings, seven routed trust-boundary findings, and nine
unbounded-input **sites**; no total is claimed across units it cannot be taken over.

**Twelve are compliant documented deviations from a SHOULD — this half stands.** The
reason is on the field. `src/topology/events.rs`'s `TaskDispatched::worktree_path` says
it is *"recorded as the string a later process compares and re-derives… a platform path
type here would make a log written on one operating system a question on another"*, and
`src/engine/topology/dispatch.rs` cites that doc **by name at the call site**.
`RunStarted4`'s `execution_root` carries the same argument in its own words — *"A string
rather than a `std::path::PathBuf`, exactly as `private_dir` and `worktree_path` are: a
recorded root has to mean the same thing on the Windows machine that resumes the run as
on the Linux one that wrote it"* — and `private_dir` shares that doc by reference. Those
docs address the representation decision the rows attack, so `§1`'s SHOULD test is met.

**One is reopened as a live defect: `create.rs`'s `canonical_string`.** An earlier
revision rejected it with the others on the grounds that it "documents its fallback as
deliberate". Read again what the doc actually defends:

> the proof compares the record against `canonicalize(<public>)` with the same
> fallback, so a filesystem that will not canonicalize produces two equal non-canonical
> strings

That defends the **`unwrap_or_else` fallback**, and it defends it well — the two sides
fall back together, so they still agree. It says **nothing whatever about
`to_string_lossy`**, which runs on *both* branches:

```rust
fn canonical_string(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}
```

So the lossy conversion after a **successful** canonicalize is undocumented, and `§1`
does not discharge an undocumented deviation at any requirement strength.

**The concrete failure, and it is not a style matter.** A run rooted beneath a valid
Unix filename containing byte `0x80` canonicalizes fine and is then recorded as a path
containing U+FFFD. After a restart, recovery reconstructs a **different** path from the
record than the one the run used. That contradicts `DESIGN.md` §4's replay invariant —
state is derived by replaying the log, so a log that cannot name the run's own root
cannot reconstruct it.

`canonical_string` has exactly two callers, both writing `public_dir`: the owner record
at `create.rs:1503` and the commit record at `:1689`. It does **not** feed `RunStarted4`,
so this reopening and the twelve rejections do not overlap — the boundary is real, not
a compromise. Its comparison-side twin, `recover.rs`'s `canonical_display`, is already
routed to `reviews/FINDINGS.md` §2 as `PR7-STD-OWNER-RECORD-LEXICAL-AUTH`; **the write
side belongs beside it**, and this is the recommendation carried to the owner.

**The scaffold row is restored, and disposed on merits.** An earlier revision struck
`src/engine/topology/scaffold.rs` on the ground that it is `#[cfg(test)]` and therefore
not production. **That is not a ground.** `CODING_STANDARDS.md` says in its opening
lines that *"It applies to production code, tests, examples, build support, and
code-generation inputs."* Striking it was this seat's error, of the same class it has
been catching in others, and the owner corrected it. Disposed under the same test as
the twelve: `scaffold.rs:154` and `:156` convert into `RunStarted4`'s `execution_root`
and `private_dir` with no canonicalize step and no independent identity decision of
their own, so the fields' own documented rationale covers them. **Rejected as a
compliant documented deviation — on merits, not on venue.**

**The lens failure mode is worth naming**, because it is the reason twelve rows were
filed against a rule they satisfy: each lens cited `§8` and none engaged the rationale
sitting on the field it was citing. A reader that quotes a clause without reading the
justification the code offers against that clause will file a compliant deviation as a
violation every time — and `§1` makes that justification the whole test for a SHOULD.
**The symmetric failure is this seat's**, and `canonical_string` is the instance: it
read that a rationale existed and did not check *which* clause the rationale defended.

### (b) The trust-boundary class — **ROUTING CHANGED**, 7 findings

These cite a MUST-tagged clause, and they sit on a security boundary. `§1` refuses an
ad hoc exception for a MUST, so the documented in-code rationale that discharges class
(a) does **not** discharge these — and several of them are documented. `recover.rs`'s
`normalize` carries a substantial argument for its lexical fallback; it is still an ad
hoc exception, and the standard says so in as many words. The same kind of evidence
settles a SHOULD and cannot settle a MUST.

**Which MUST, corrected.** An earlier revision said all seven cite `§8`'s containment
bullet. **Five cite `§14`.** The owner corrected the grounds on 2026-08-28 and the
routing conclusion is unchanged, because `§14` is MUST-tagged in both places these rows
land — its opening *"Code MUST validate them before granting filesystem, process, git,
capacity, or state-transition authority"* and its final bullet *"Security-sensitive
comparisons and decisions MUST fail closed on malformed, contradictory, or unavailable
evidence; availability fallbacks must not silently grant more authority."* What was
false was the evidence sentence, not the verdict.

They are therefore **not** the sweep's work. They route to `reviews/FINDINGS.md` §2 as
contract rows with a named owner. **That routing is a claim about pull request #42, and
it is true only once #42 lands** — which is why the merge order in §0b puts #42 first.
Digests are full sha256, because an elided digest cannot be checked. The row ids are
**forward references**: #42 creates them and they do not resolve in this tree, which is
the second reason the merge order in §0b puts #42 first. They are named anyway, because
a routing claim that cannot be followed to a row id is not checkable at all:

| row id (created by #42) | file | § | observation | region sha256 |
|---|---|---|---|---|
| `PR7-STD-PRIVATE-ROOT-LEXICAL-COMPARE` | `src/engine/topology/recover.rs` | §14 fail-closed | The explicit private-root comparison falls back to lexical equality on every canonicalization error. | decision site `5289194ca998e04b98b33aba06400b2abab199a6fdce2c9737693e326f6990c5`; rationale `74cb133ae3a953d0c6a7e7dcf8c25c445203f0cbe52f457c309645e4963b555f` |
| `PR7-STD-OWNER-RECORD-LEXICAL-AUTH` | `src/engine/topology/recover.rs` | §14 fail-closed | Owner-record public-directory authentication falls back to lexical spelling when canonical evidence is unavailable. | decision site `a332d47443baaa6c12f1f74ee47e06a6b654e16a6bbbd731261d1de46971fb75`; rationale `1366553bf35fea1422476857aa79e3b8ac7c76e7e77011c01e84efcea7d0abb1` |
| `PR7-STD-PRIVATE-ROOT-NO-CONTAINMENT` | `src/engine/topology/recover.rs` | §8 containment | The recorded private-root locator is accepted without absolute-path or symlink/reparse-point containment validation. | `f2aa6763a72ff901dda7a55bfb71c583343276162706c926334a19363dd73284` |
| `PR7-STD-QUESTION-PAYLOAD-COMPONENT` | `src/rundir.rs` | §14 validate-before-authority | The question-payload write boundary interpolates an unvalidated component into an authoritative path. | `1e5f413d6388eaee5594a0021c64d13c297671a22cd7b74a34c46b135af94557` |
| `PR7-STD-ANSWER-STAGING-COMPONENT` | `src/rundir.rs` | §14 validate-before-authority | The answer staging boundary uses an unvalidated component as part of its write path. | `975a3033271821f91216179bae82995fe4a1380a19697e62d18d213aff009859` |
| `PR7-STD-OWNERSHIP-PROOF-UNCANONICAL` | `src/rundir.rs` | §14 fail-closed | The private-half ownership proof falls back to an uncanonicalized public path when canonicalization evidence is unavailable. | `eb0463a6ae63dc8b69740dc416acec8f611e6915a313a72dd9c4216b9947b8c4` |
| `PR7-STD-CONTAINER-LEXICAL-CONFINEMENT` | `src/runner/container/exec.rs` | §8 containment | Confinement uses a lexical prefix comparison as its entire filesystem-containment decision. | `3e0c3df5db999002cfbd6b5ebc0340735f07fa286df13d465f944a1be183e1cb` |

**Two rows carry two digests each, and the reason is a defect this seat introduced.**
An earlier revision hashed only `normalize`'s three-line body and `canonical_display`'s
six — the helpers, without the doc comments that are the documented rationale and
without the comparisons that are the decision. Editing either rationale would leave the
digest verifying while the row's own "this site is documented" claim became false. Each
now records both regions, labelled.

`§8`'s own words on why a documented fallback is not enough for the two containment
rows: *"Lexical normalization alone does not prove filesystem containment."*

Not repaired by this seat, and not carried by #40 or #41.

### (c) The unbounded-input class — 9 sites, and an explicit disposition

An earlier revision of this file described this class and the final commit **removed it
without disposing of it**, while §1's opening said all 28 escalated findings had been
ruled on. They had not: 12 + 1 + 7 is 20. The nine are restored here with a disposition
each, which is what was missing.

All nine cite `§14`, and the clause they land on — *"Bound input size, recursion,
collection growth, output capture, concurrency, and retry work before allocating or
spawning from untrusted values"* — carries **no requirement keyword**, while a sibling
bullet in the same section is explicitly MUST-tagged. By exactly the reasoning applied
to `§8`'s untagged path bullet, it is **at most a SHOULD**, so `§1`'s test is whether
the site carries a concrete reason in the code.

| site | reason in the code? | disposition |
|---|---|---|
| `src/util.rs` `read_file_bounded`, and `src/events/log.rs`'s whole-log read through it | **yes, explicitly** | **Rejected — compliant documented deviation.** The doc says the quiet part: *"It is not a cap: a regular file is read in full however large it is… the read is bounded, not the answer."* `log.rs`'s `read_bytes` cites it and says why, naming `PR5-RD-001`. |
| `src/events/log.rs` incremental poll, `file.read_to_end(&mut buffer)` after `seek` | **no — and it contradicts its own module** | **OPEN. The strongest of the nine.** `read_bytes`'s doc states the module's rule — `read_file_bounded` *"here and at every other read of a log in this module"* — and this site is the exception to it. The surrounding code already reads `file.metadata().len()` for its truncation check and does not use it as a bound. Failure: a large or continuously appended log forces unbounded allocation and the poll need never reach EOF. |
| `src/engine/topology/recover.rs` `read_record` (`owner.json`, `committed.json`) | no — the doc is *"Read one JSON record, or refuse naming the file."* | **OPEN**, undocumented SHOULD deviation on persisted run data, which `§14` names as a trust-boundary input. |
| `src/engine/attempt.rs` and `src/review.rs`, agent-authored artifacts read into prompts | no | **OPEN**, undocumented, and the input is model output — the least trusted class this engine handles. |
| `src/config.rs` | no — the nearby doc is about modification time, not bounds | **OPEN**, undocumented. |
| `src/topology/schema.rs` header probe | not checked to the same depth | **OPEN, and the check is owed.** Recorded as unverified rather than asserted either way. |
| `src/runner/container.rs` `exec_streams` | no — its doc is about stream separation | **OPEN**, undocumented; `Command::output` captures both streams with no pre-allocation bound and no timeout. |
| `src/validate.rs` cycle detection over the untrusted task graph | not checked to the same depth | **OPEN, and the check is owed.** |

**What the seat verified and what it did not.** The first five rows above were read at
the cited sites in this tree; the last two were not audited to that depth and say so.
An earlier revision's error was to let a class disappear rather than say "unresolved",
and a disposition that overstates its own evidence would repeat that in the other
direction.

**Recommendation carried to the owner, not a ruling made here.** The `log.rs`
incremental poll deserves a `reviews/FINDINGS.md` §2 row with an owner, for the same
reason the trust-boundary cluster got one: it has a concrete availability failure
sequence against the artifact `DESIGN.md` §4 makes the engine's ground truth. The
remaining open sites are undocumented SHOULD deviations and belong to the sweep, where
their rows already are. **No row is added to `reviews/FINDINGS.md` by this pull
request** — #42 owns that file in this sequence, and two branches editing it would
manufacture a conflict between two of this seat's own changes.

## 2. W4's scope does not grow

The escalation asked whether W4 should absorb the lossy-path class, since one of its
sites — `src/topology/events.rs` — is serde-visible and a type change there would be
Class C and a schema bump rather than a widening.

**Ruled: it does not.** An earlier revision gave three reasons and called any one
sufficient; the first of them was *"there is nothing to fix, per (a)"*, and (a) no
longer says that — `canonical_string` is reopened. **That reason is withdrawn.** The
other two are independent of it and each remains sufficient on its own:

- `§8`'s *own* MUST makes an event-schema change a migration of supported historical
  runs, categorically not the one-field widening W4 is chartered for.
- The `OsString` record scopes itself to *"the one measured field"* with *"no
  speculative widening"*, so it cannot authorise what it expressly declined to cover.

**And the reopened finding does not reach W4 either**, which is why withdrawing the
first reason changes no outcome: `canonical_string` writes `public_dir` in the owner
and commit records, not `CommandSpec.program`, and its repair is a conversion change at
one call site rather than a type widening in the frozen design.

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
