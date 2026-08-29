# 2026-08-28 — standards review of the merged head: triage for the owner

The 321 rows are filed in `reviews/2026-08-25-pr7-standards-worklist.md`, routed
101 to W10.2 and 220 to W10.3. Each region sha256 relocates its citation; it does not
authorize W10.4 reuse unless the entire reviewed file is byte-identical. This file is
only for the three things the rider does not decide.

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
   catch it: no second reader covers that file, and the region-hash gate only checks
   that a *filed* finding's quote relocates. Whole-file identity is the separate W10.4
   reuse condition. Every one of those controls validates what was found; none of them
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

**What that means for the rulings reported here.** The prerequisite records now exist:
`decisions/2026-08-25-commandspec-program-stays-string.md`,
`decisions/2026-08-25-checkpoint-merges.md`, and
`decisions/2026-08-24-pr3-layer-freeze-charter.md`. The first records the withdrawal of
the proposed `OsString` widening; `DESIGN.md` remains the living authority and retains
`CommandSpec.program: String`.

**No `DESIGN.md` edit is owed by anything in this file.** Durable wire-facing log
identity stays `String`, and the proposed `CommandSpec.program` widening was withdrawn.
The pass records W4 as empty, consistent with `DESIGN.md:222`.

Two consequences follow and both are now stated wherever they bite:

1. **Merge order was #40, then #42, then #41.** At this reconciled head the prerequisite
   records and the #42 ledger rows are present, so the dependencies this report names are
   satisfied rather than forward promises.
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
or `OsString`"* — is an imperative requirement even though it carries no explicit RFC
2119 keyword. The standard defines the strength of explicit keywords; it does not assign
untagged imperatives a default SHOULD strength. This report therefore does not use §1's
SHOULD-deviation exception to dismiss an untagged rule.

### (a) The lossy-path class — **REOPENED IN FULL and escalated**

**This section has now been wrong three times, in three different ways, and the third
correction is the one that generalises.** The first revision rejected twelve and struck
one. The owner amended that on 2026-08-28: the strike was on a ground the standard
refuses, and `create.rs`'s `canonical_string` is a live defect rather than a compliant
deviation. The second review of this pull request then checked the call sites and found
that **the amendment landed on the wrong site**, and that the test it states does not
stop where either revision stopped.

**The membership, because an earlier revision asserted a count without one — and the first
list was still short.** The rows below cite §8 and belong to this family. The
`src/agent/claude.rs` settings-path row was missing from the previous revision's list, and
its absence is the reason a list is given at all rather than a number:

| work-list line | file | what the lossy value becomes |
|---|---|---|
| 47 | `src/engine/topology/create.rs` | `run_started`'s `private_dir` |
| 50 | `src/engine/topology/create.rs` | `canonical_string` → owner and commit `public_dir` |
| 51 | `src/engine/topology/create.rs` | `CreatingMarker.private_dir` |
| 59 | `src/engine/topology/dispatch.rs` | `task_dispatched` worktree identity |
| 93 | `src/engine/topology/scaffold.rs` | `RunStarted4`'s `execution_root` and `private_dir` |
| 112 | `src/topology/events.rs` | `RunStarted4`'s path fields as `String` |
| 149 | `src/agent/claude.rs` | a settings path in a **subprocess argument** |
| 156 | `src/agent/codex.rs` | a schema path in a **subprocess argument** |
| 198 | `src/engine/coordinator.rs` | the operational private-directory path |
| 201 | `src/engine/preflight.rs` | plan and configuration paths in a record |
| 224 | `src/events/mod.rs` | persisted plan and configuration paths |
| 237 | `src/gates.rs` | executable suffix probing through `display()` |
| 258 | `src/rundir.rs` | the ownership proof's run identity |
| 264 | `src/rundir.rs` | `CreatingMarker`'s canonical private-directory identity |
| 267 | `src/runner/container.rs` | a Docker **bind-mount source** |

**The failure modes, and they are not interchangeable.** An earlier revision applied
one sequence to the whole class, which is how it ended up on the wrong site.

- **A — the string is read back and a path is reconstructed** (the run-started and marker
  private directories, the `RunStarted4` path fields, the legacy record fields and the
  repo-relative path). A replaced byte produces a **different path** after restart. This is the sequence
  the owner described, and its clearest instance is `RunStarted4.private_dir`: written with
  `to_string_lossy` at `create.rs:1647`, and turned back into a path by
  `PathBuf::from(&started.private_dir)` at `recover.rs:335`. It bites `DESIGN.md` §4's
  replay invariant.

  **Two rows an earlier revision put here do not belong.** Row 59's
  `TaskDispatched.worktree_path` is written at `dispatch.rs:398` and **production replay
  never reads it**: recovery derives the slot from the key and generation at
  `recover.rs:2642`. Row 93's `scaffold.rs` writes fixture events and nothing there reads
  the strings back. Both are recorded below as **D**.
- **B — both sides render lossily and only compare** (`canonical_string`'s `public_dir`). `canonical_string` writes
  `public_dir`, but recovery does **not** reconstruct a path from that record: it derives
  the public directory from `repo_root` and `run_id` at `recover.rs:280`, renders *that*
  lossily through `canonical_display`, and compares strings at `:632`. Both sides produce
  U+FFFD, so they agree, and no different path is reconstructed. The failure here is
  narrower and still real: **two distinct non-UTF-8 directories whose lossy renderings
  collide authenticate against each other's owner record**, and the disagreement refusal
  never fires.

  **Row 258 was put here and is not class B.** `rundir.rs:1472` renders **one** basename
  lossily and the values it compares against are not independently rendered; worse, that
  same basename builds the expected private path at `:1532`. Neither half of the class
  description holds. It is class **C**.
- **C — the lossy string selects a target** (the settings path, the schema path, the
  executable probe, the ownership proof's basename and the mount source). It is not read back;
  it is handed to a subprocess, an executable probe, a constructed path, or Docker's mount
  syntax. A replaced byte **names a different or nonexistent object** at the moment of use.
  The `claude.rs` row is the sharpest: a settings file under a directory containing raw
  byte `0x80`, and a sibling whose name contains a literal U+FFFD, render to the same
  spelling — so the agent loads the wrong settings file, possibly a more permissive one.
- **D — written and never read** (the dispatched worktree path and the scaffold fixture). The value is durable or fixture-only and no
  production path reconstructs it. **Recorded, not dismissed**: §8's rule is that a lossy
  display string is never identity, and these are identity fields whose current consumers
  happen not to depend on them. A future consumer is one commit away, which is a weaker
  finding than A but not none.

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
- The class-C sites have no field-level rationale available to them at all: a settings
  path, a schema path, an executable probe, an ownership proof's basename and a Docker
  mount source are not `RunStarted4` or `TaskDispatched` fields, so the rationale that
  discharged the durable records cannot reach them even in principle.

**Disposition: OPEN, escalated to the owner, and no rejection is claimed.** This seat has
no standing to reverse an owner ruling and is not doing so — it reports that the ruling's
own test, applied to the call sites, does not sustain the rejections, and asks for the
narrower question to be decided: **is a documented `String`-over-`PathBuf` choice also a
documented defence of `to_string_lossy`?**

**The question is fairly premised.** A `String` in this codebase demonstrably can carry a
faithful encoding of non-UTF-8 bytes: `runner/container/intent.rs` percent-encodes a path
injectively at `:160` and decodes it at `:230`. So "we chose `String` for cross-platform
legibility" does not by itself decide "and therefore losing bytes is acceptable".

**A "yes" would not return the class to rejected, and an earlier revision said it would.**
Only `TaskDispatched.worktree_path` and `RunStarted4`'s fields carry the cited rationale at
all. The marker's private directory, the legacy record fields and the repo-relative path
have **no such rationale to extend**, so they are undocumented on either answer. A "yes"
returns the rows whose fields carry it and leaves the rest exactly where they are.

**And the repair is not a one-line conversion change** — but it is also not the *same*
repair everywhere, which a later revision wrongly implied by prescribing one remedy for the
whole class. **The remedy depends on which boundary the value crosses.**

- **A value that is persisted and read back** — class A, and the comparison in class B —
  needs a **two-sided, backward-compatible representation**. An encoding has to be decoded
  at every consumer, `PathBuf::from` at `recover.rs:335` among them, or a byte written as
  `%80` becomes the literal path `%80`; and adding decoding without a tagged or versioned
  form reinterprets a historical path genuinely named `%80`, which is the compatibility
  migration §8 makes a MUST. That is a design decision rather than a sweep item.
- **A value used at the moment of construction or hand-off** — class C — needs **no
  encoding at all**, and prescribing one would make it worse. `gates.rs`'s executable probe
  builds a candidate locally through `base.display()`; it has no writer, no persisted form
  and no second consumer, so carrying the OS-native type to the append fixes it outright.
  The settings path handed to an agent CLI is the sharper case: an external process cannot
  decode a representation private to this engine, so percent-encoding it would name a
  literal `%80` to the tool. `Command::arg` takes an `OsStr`, which is the whole repair.

Treating class C as a versioned-format decision would defer a local fix indefinitely while
a valid executable under a non-UTF-8 path stays undetected. That is why the classes are
split by failure mode and not merged into one remedy.

Until that ruling, the rows above stay filed in the work-list with this triage beside
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
contract rows with a named owner, and those rows are present at this reconciled head.
The digests below are full sha256 values for the regions in the original reviewed tree
`3e5212d`; they preserve review provenance but do not claim current-tree W10.4 reuse.
`reviews/FINDINGS.md` is canonical for each row's current target and digest after #42's
re-derivation, so a differing digest there is expected evidence of a changed file rather
than a second current locator:

| row id | file | § | observation | review-source region sha256 at `3e5212d` |
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

### (c) The unbounded-input class — explicit dispositions

An earlier revision of this file described this class and the final commit **removed it
without disposing of it**. The observations are restored here with a disposition each,
which is what was missing.

`src/runner/container.rs`'s `exec_streams` is not in this table because it cites **§9**, whose
subprocess requirements are MUST-tagged, so §1 sends it to an owner rather than to the
sweep. It is routed as `PR7-STD-CONTAINER-EXEC-UNBOUNDED` — see §1(b). Leaving it here
also made the sentence below false about its own table.

The rows that remain here cite `§14`, and the clause they land on — *"Bound input size,
recursion, collection growth, output capture, concurrency, and retry work before
allocating or spawning from untrusted values"* — is an imperative requirement. Its lack
of an explicit keyword does not make it a SHOULD or authorize a documented deviation.

| site | reason in the code? | disposition |
|---|---|---|
| `src/util.rs` `read_file_bounded`, and `src/events/log.rs`'s whole-log read through it | **yes, explicitly** | **OPEN.** The doc accurately says *"It is not a cap: a regular file is read in full however large it is… the read is bounded, not the answer."* That rationale documents the behavior but does not satisfy §14's imperative to bound input before allocating. |
| `src/events/log.rs` incremental poll, `file.read_to_end(&mut buffer)` after `seek` | **no — and it contradicts its own module** | **OPEN.** `read_bytes`'s doc states the module's rule — `read_file_bounded` *"here and at every other read of a log in this module"* — and this site is the exception to it. The surrounding code already reads `file.metadata().len()` for its truncation check and does not use it as a bound. Failure: a large or continuously appended log forces unbounded allocation and the poll need never reach EOF. |
| `src/engine/topology/recover.rs` `read_record` (`owner.json`, `committed.json`) | no — the doc is *"Read one JSON record, or refuse naming the file."* | **OPEN**, unbounded persisted run data, which `§14` names as a trust-boundary input. |
| `src/engine/attempt.rs` and `src/review.rs`, agent-authored artifacts read into prompts | no | **OPEN**, undocumented, and the input is model output — the least trusted class this engine handles. |
| `src/config.rs` | no — the nearby doc is about modification time, not bounds | **OPEN**, undocumented. |
| `src/topology/schema.rs` header probe | not checked to the same depth | **OPEN, and the check is owed.** Recorded as unverified rather than asserted either way. |
| `src/validate.rs` cycle detection over the untrusted task graph | not checked to the same depth | **OPEN, and the check is owed.** |
| `src/export.rs` `export::load`, whole normalized plan read before digest or schema validation | no | **OPEN.** Persisted run data is untrusted under §14; `std::fs::read` allocates the complete file before either integrity check. |

**What the seat verified and what it did not.** Every row above was read at its cited site
in this tree **except** `src/topology/schema.rs`'s header probe and `src/validate.rs`'s
cycle detection, which say so in their own cells. They are named rather than numbered here
because two earlier revisions referred to them by position and both went stale — first when
a row was routed out, then when the ordinals shifted behind it.
An earlier revision's error was to let a class disappear rather than say "unresolved",
and a disposition that overstates its own evidence would repeat that in the other
direction.

**Recommendation carried to the owner, not a ruling made here.** The `log.rs`
incremental poll deserves a `reviews/FINDINGS.md` §2 row with an owner, for the same
reason the trust-boundary cluster got one: it has a concrete availability failure
sequence against the artifact `DESIGN.md` §4 makes the engine's ground truth. The
remaining open sites are unbounded-input observations and belong to the sweep, where
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
- The `commandspec-program-stays-string` record withdraws the proposed widening and the
  pass records W4 as empty, so it cannot authorize a different field change.

**And the reopened class does not reach W4 either**, which is why withdrawing the first
reason changes no outcome. W4 is empty; `CommandSpec.program` remains `String`. No row
in §1(a) is about that field: they are event and
marker records, subprocess arguments, an executable probe and a mount source. Their repair —
where the owner rules one is owed — is **whatever §1(a)'s boundary split prescribes for each
of them**, and not one remedy for all: a two-sided, backward-compatible representation where
a value is persisted and read back, and the OS-native type carried to the call where it is
used at hand-off. An earlier revision of this paragraph named the durable remedy for the
whole class, which is the over-generalisation §1(a) records. Either way it is a different act
from
widening a type in the frozen design and needs no `DESIGN.md:222` edit.

What the ruling added instead is a clause, not a record: the two live documents reason
oppositely about one trade-off — `events.rs` argues cross-platform log legibility for
`String`, while the command-spec record preserves the existing `String` boundary after
considering non-UTF-8 program identity. Durable wire-facing identity and
`CommandSpec.program` both stay `String`; W4 opens neither.

## 3. Frozen-layer edits

None proposed, by construction and by measurement. The 101 W10.2 rows all sit inside
`src/topology/**` or `src/engine/topology/**` and are routed to the pass, which is
where a repair to them belongs.
