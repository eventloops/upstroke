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
manifest, the fourteen prompts, the raw lens logs and `findings.json` are **not** in the
tree, and everything that rests on them is **seat-attested rather than
repository-verifiable**. An earlier revision said "three claims" and enumerated three;
that count was wrong and is replaced by the criterion, because a count of unverifiable
claims is itself the kind of thing that goes stale:

**Every claim about how the review was conducted is seat-attested.** That includes the
exclusive file partition; the fifteen source files that drew no finding; the eight-gate
run that preceded the lenses; that no lens produced a diff; that each row is faithful to
the lens that raised it and carries the lens's own wording; the fourteen per-lens totals;
and the local gate results and embedded-binary check reported in the pull request body.
None of those can be checked from this repository, because the artifacts that would
settle them are outside it.

**What the repository *does* let you check** is every filed row: its file, its region
hash, and that the hash relocates to exactly one contiguous region at `3e5212d`. That is
a claim about the tree, not about the review, and it is the only class of claim here that
a reader can verify without trusting this seat.

**Citation wording varies** — 26 distinct section strings for 12 actual sections
(`§12 tests`, `§12 Tests`, `§12 testing strategy`, …). Rows are filed verbatim rather
than silently normalised. Normalised: §12 102, §7 59, §8 42, §14 36, §5 20, §9 16,
§4 13, §3 12, §10 8, §13 7, §6 5, §11 1.

## 0b. What this file is, and what it is not

**This document is a report, not an authority.** It records rulings the owner made; it
does not make them and it does not constitute them.

**The authority model, corrected.** An earlier revision said living authority is reserved
to *"`DESIGN.md` and the records"*. `decisions/README.md` says something narrower in its
first contract bullet: **"DESIGN.md remains the only living authority for product design.
Records here are history, not spec."** A decision record is where a ruling's reasoning is
kept; where the ruling changes the spec, `DESIGN.md` takes the compressed edit at decision
time, citing the record. A *review* document is neither, and this one claims to be
neither.

**What that means for the rulings reported here.** Three are recorded in decision records
— `decisions/2026-08-25-commandspec-program-osstring.md`,
`decisions/2026-08-25-checkpoint-merges.md`,
`decisions/2026-08-24-pr3-layer-freeze-charter.md` — and **none of those three exists in
this tree.** They land in pull request #40, so every citation of them here **dangles until
#40 lands**. That is the accurate statement of the hazard, and it is narrower than the one
an earlier revision made.

**No `DESIGN.md` edit is owed by anything in this file**, and the reason is worth stating
because a reader could reasonably expect one. §2's clause says durable wire-facing log
identity **stays** `String` — it records that the existing spec is unchanged, so there is
no compressed edit to make. The one ruling here that *would* change the spec, the
`OsString` widening, is scheduled to `DESIGN.md:222` at W4 by its own record and is
deliberately not made now.

Two consequences follow and both are now stated wherever they bite:

1. **Merge order: #40, then #42, then #41.** This is **the owner's ruling of
   2026-08-28**, correcting an earlier ruling of their own that called the dependency
   soft and ordered only #42 before #41. #40 first, because the three records cited above
   are its content and the citations here dangle until it lands. #42 before this one,
   because §1(b) says the trust-boundary cluster *"routes to `reviews/FINDINGS.md` §2"* in
   the present tense, and that is true only once #42 lands. **What this tree can show is
   that the three records are absent from it**; that they land in #40, and what #40 and
   #42 currently contain, are the owner's statements about mutable branches this document
   does not pin to a head.
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

### (a) The lossy-path class — **REOPENED IN FULL and escalated**, 14 findings

**This section has now been wrong three times, in three different ways, and the third
correction is the one that generalises.** The first revision rejected twelve and struck
one. The owner amended that on 2026-08-28: the strike was on a ground the standard
refuses, and `create.rs`'s `canonical_string` is a live defect rather than a compliant
deviation. The second review of this pull request then checked the call sites and found
that **the amendment landed on the wrong site**, and that the test it states does not
stop where either revision stopped.

**The membership, because an earlier revision asserted a count without one.** Fourteen
filed rows cite §8 and belong to this family. They are listed here so the arithmetic can
be checked rather than believed:

| work-list line | file | what the lossy value becomes |
|---|---|---|
| 47 | `src/engine/topology/create.rs` | `run_started`'s `private_dir` |
| 50 | `src/engine/topology/create.rs` | `canonical_string` → owner and commit `public_dir` |
| 51 | `src/engine/topology/create.rs` | `CreatingMarker.private_dir` |
| 59 | `src/engine/topology/dispatch.rs` | `task_dispatched` worktree identity |
| 93 | `src/engine/topology/scaffold.rs` | `RunStarted4`'s `execution_root` and `private_dir` |
| 112 | `src/topology/events.rs` | `RunStarted4`'s path fields as `String` |
| 156 | `src/agent/codex.rs` | a schema path in a **subprocess argument** |
| 198 | `src/engine/coordinator.rs` | the operational private-directory path |
| 201 | `src/engine/preflight.rs` | plan and configuration paths in a record |
| 224 | `src/events/mod.rs` | persisted plan and configuration paths |
| 237 | `src/gates.rs` | executable suffix probing through `display()` |
| 258 | `src/rundir.rs` | the ownership proof's run identity |
| 264 | `src/rundir.rs` | `CreatingMarker`'s canonical private-directory identity |
| 267 | `src/runner/container.rs` | a Docker **bind-mount source** |

**Three failure modes, and they are not interchangeable.** An earlier revision applied
one sequence to the whole class, which is how it ended up on the wrong site.

- **A — the string is read back and a path is reconstructed** (47, 51, 59, 93, 112, 198,
  201, 224, 264). A replaced byte produces a **different path** after restart. This is
  the sequence the owner described, and its clearest instance is `RunStarted4.private_dir`:
  written with `to_string_lossy` at `create.rs:1647`, and turned back into a path by
  `PathBuf::from(&started.private_dir)` at `recover.rs:335`. It bites `DESIGN.md` §4's
  replay invariant.
- **B — both sides render lossily and only compare** (50, 258). `canonical_string` writes
  `public_dir`, but recovery does **not** reconstruct a path from that record: it derives
  the public directory from `repo_root` and `run_id` at `recover.rs:280`, renders *that*
  lossily through `canonical_display`, and compares strings at `:632`. Both sides produce
  U+FFFD, so they agree, and no different path is reconstructed. The failure here is
  narrower and still real: **two distinct non-UTF-8 directories whose lossy renderings
  collide authenticate against each other's owner record**, and the disagreement refusal
  never fires.
- **C — the lossy string selects a target** (156, 237, 267). It is not read back at all;
  it is handed to a subprocess, an executable probe, or Docker's mount syntax. A replaced
  byte **names a different or nonexistent object** at the moment of use.

**Why `canonical_string` was the wrong home for the replay sequence, stated plainly.**
The owner's amendment is right that its doc defends only the `unwrap_or_else` fallback
and says nothing about `to_string_lossy`. It is wrong that this produces a different
reconstructed path, because nothing reconstructs a path from `public_dir`. The finding
survives with the class-B sequence above; the class-A sequence belongs to
`RunStarted4.private_dir`, which the same amendment left rejected as compliant.

**And that is why the whole class is reopened rather than re-split.** The test the owner
states is *"does the code carry a rationale that addresses the lossy conversion?"* Applied
consistently, it does not stop at `canonical_string`:

- `TaskDispatched::worktree_path` and `RunStarted4::execution_root` argue for **`String`
  rather than `std::path::PathBuf`** — *"a recorded root has to mean the same thing on the
  Windows machine that resumes the run as on the Linux one that wrote it, and a platform
  path type would make that a question about separators"*. That is an argument about the
  **type**, and a `String` field can hold a faithful encoding of non-UTF-8 bytes. Neither
  doc mentions `to_string_lossy` or defends discarding identity bytes.
- `scaffold.rs:151` **independently chooses** `.to_string_lossy()`. An earlier revision
  of this file said it "makes no identity decision of its own"; it does, and that claim is
  **withdrawn**.
- The class-C sites (156, 237, 267) have no field-level rationale available to them at
  all: a subprocess argument, an executable probe and a Docker mount source are not
  `RunStarted4` or `TaskDispatched` fields, so the rationale that discharged the durable
  records cannot reach them even in principle.

**Disposition: OPEN, escalated to the owner, and no rejection is claimed.** This seat has
no standing to reverse an owner ruling, and it is not doing so — it is reporting that the
ruling's own test, applied to the call sites, does not sustain the twelve rejections, and
asking for the narrower question to be decided: **is a documented `String`-over-`PathBuf`
choice also a documented defence of `to_string_lossy`?** If yes, class A and the
`scaffold.rs` row return to rejected and only classes B and C stand. If no, the class is
open and the sweep is the wrong venue for at least class A.

Until that ruling, the fourteen rows stay filed in the work-list with this triage beside
them, and **no `reviews/FINDINGS.md` row is added by this pull request** — #42 owns that
file in this sequence.

**The lens failure mode is still worth naming.** Each lens cited `§8` and none engaged
the rationale sitting on the field it was citing, which is how a compliant deviation gets
filed as a violation. **The symmetric failure is this seat's, twice**: the first revision
read that a rationale existed and did not check which clause it defended; the second read
the amendment and did not check which call site the sequence fits.

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

**An eighth row joins them, and the miss it exposes is the same shape.** The routing test
— which requirement keyword does the cited clause carry — was applied to §8 and §14 and
**not to §9**. `src/runner/container.rs`'s `exec_streams` row (work-list line 272) cites
*"§9 Processes and external tools"*, and §9 says *"Every subprocess integration MUST
define and test: … timeout, cancellation, and descendant-process cleanup"*, plus
stdout/stderr size behaviour. That is a MUST; §1 sends it to an owner exactly as it sent
the other seven; and `exec_streams`'s doc comment argues only about **stream separation**,
so there is no rationale to weigh even at SHOULD strength. It is routed as
`PR7-STD-CONTAINER-EXEC-UNBOUNDED`, and it is also **removed from §1(c)'s list below**,
where an earlier revision left it as sweep work.

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
| `src/runner/container.rs` `exec_streams` | no — its doc is about stream separation | **ROUTED, not sweep work.** Its filed row cites §9, whose subprocess requirements are MUST-tagged, so §1 sends it to an owner. It is `PR7-STD-CONTAINER-EXEC-UNBOUNDED` in `reviews/FINDINGS.md` §2 — see §1(b). Listing it here as a SHOULD was the miss that §1(b) now records. |
| `src/validate.rs` cycle detection over the untrusted task graph | not checked to the same depth | **OPEN, and the check is owed.** |

**What the seat verified and what it did not.** The table has **eight** rows. Rows 1-5
and row 7 were read at the cited sites in this tree. **Rows 6 and 8** —
`src/topology/schema.rs`'s header probe and `src/validate.rs`'s cycle detection — were
not audited to that depth and say so in their own cells. An earlier revision said "the
last two", which named the wrong two once row 7 was routed out.
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

**And the reopened class does not reach W4 either**, which is why withdrawing the first
reason changes no outcome. W4 widens **one** field, `CommandSpec.program`, from `String`
to `OsString`. Not one of the fourteen rows in §1(a) is about that field: they are event
and marker records, a subprocess argument, an executable probe and a mount source. Their
repair — where the owner rules one is owed — is a **conversion** change at the call site,
choosing a faithful encoding instead of `to_string_lossy`, which is a different act from
widening a type in the frozen design and needs no `DESIGN.md:222` edit.

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
