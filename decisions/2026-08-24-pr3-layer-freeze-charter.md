# 2026-08-24 — the PR3-layer freeze: charter, adjudication, and the G2 pass

**Verdict.** Four rulings, together replacing the unwritten scope of the 2026-08-20
ruling with a written one.

1. **The freeze is chartered.** `src/topology/**` and `DESIGN.md:222` are frozen by
   default against edits from feature slices — the scope every actor has honoured in
   practice since PR4, now stated rather than inferred. What "frozen" admits is defined
   by three classes in §"The charter" below: disclosed behaviour-neutral readers
   (Class A) are legitimate per-instance deviations; behaviour changes (Class B) belong
   to a dedicated pass or per-instance owner approval; design changes (Class C) require
   a decision record. The 2026-08-20 principle — **a slice may not quietly redesign
   what it implements** — is retained and is this charter's spine.
2. **The challenge of 2026-08-24**
   (`reviews/2026-08-24-unfreeze-challenge-request.md`) **is adjudicated.** PR7's
   `src/topology/fold.rs` footprint — ten delegating readers, one packet-conforming
   conjunct, and the eleventh reader at `3362f65`, the branch tip when the challenge
   was filed — is **accepted as a disclosed deviation**, measured and blessed below.
   The **standing self-serve rule its Claim 5 proposed is rejected.** Its Claim 1
   finding — that the freeze had no authoritative charter — is **accepted**, and this
   record is the repair.
3. **The G2 pass over PR3's layer is brought forward** and runs as the next slice,
   **before PR8**. Scope, workstreams and exit criteria are
   `proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md`. The bar is the owner's, stated
   2026-08-24: the best code and test coverage this project can produce, with effort
   explicitly not a constraint. Until the pass lands, **reader accretion stops**: from
   this record forward, no reader lands outside the pass. The moratorium binds from this
   record, not from the 2026-08-24 adjudication it transcribes, and one reader landed in
   between — `open_no_attempt` at `ffcc74a` (2026-08-25), the twelfth, disclosed and
   approved per-instance under Class A as `PR7-FOLD-OPEN-NO-ATTEMPT`
   (`reviews/FINDINGS.md` §3). It, not the eleventh, is the last outside the pass.
4. **A Rust coding-standards document lands on master first**, and the cleanliness
   review of the layer runs as the **final** workstream of the pass, enforcing that
   document — never interleaved with behaviour commits. Cleanliness churn must be
   measured behaviour-neutral (the §12 purity technique: identical sorted line
   multisets, or better) and reviewed under the standards' own text, so that a
   cleanliness finding has a quotable passage the way a packet finding does.

This **supersedes in part** the owner ruling of 2026-08-20 (transcribed in
`reviews/FINDINGS.md` §2 on `PR4-SPAWN-SITE-PROBE-CONTEXT` and
`PR4-PROGRAM-PATH-NOT-UNICODE`, recorded by `20d44a4`): its "revisit at G2" matures
now, and its two accepted deviations transfer into the pass's scope as work items. Its
refusals were correct when made and are not disturbed retroactively.

## Context — what was actually frozen, by whom, in writing

The 2026-08-20 ruling froze **two named things**. The transcription commit `20d44a4`
is owner-authored and says so in the owner's own words: *"The owner has ruled that
**both** frozen files stay frozen"* — `src/topology/effects.rs`, *"which PR3 froze"*,
and `DESIGN.md:222`. The directory-wide reading appears only in later implementer
glosses: FINDINGS §13's process note (*"the freeze covers `src/topology/**` and
`DESIGN.md:222` only"*) and §11's opening. No decision record chartered either
reading; both records dated 2026-08-20 in this folder are about other subjects.

Operative practice was nevertheless directory-shaped with a disclosure valve, used
twice and never objected to: PR5's §11 ceremony for `registry.rs` (test-fixture-only,
measured, *"recorded as a forced consequence"*), and PR7's own
`PR7-FOLD-ACCESSORS-IN-PR3-LAYER` row (*"The owner should still rule, which is why
the row exists"*). The charter below codifies that valve rather than inventing one.

## Measured, not assumed

All measured on 2026-08-24 against `slice/pr7` at
`3c09f6ee7260eb8ac6f7a48a63d4779ded20de11`, fresh clone, build box:

- **Suite: 1660 passed, 0 failed.** PR7's `src/topology/` footprint is one file,
  `fold.rs`, **+652 / −0**: 51 production code, 127 comments and blanks, 474 test —
  the challenge's Claim 2, reproduced to the digit. Nine of ten files untouched.
- **Run A — the enforcement world.** `git revert --no-commit 84a3978` (the contested
  repair): **1661 passed, 0 failed.** Reverting the repair leaves a green suite over a
  real, latent width>1 correctness defect the suite cannot see. Full enforcement
  (fold.rs untouched) does not even compile: the driver consumes the readers at 36
  call sites (`select.rs` 21, `settle.rs` 11).
- **Run B — the guard works.** Restoring only the engine's second derivation of
  `predicted_region` fails exactly one test — the assertion `84a3978` added, which
  compares the recorded region against the fold's answer rather than a literal.
- **The asymmetry that decides the class fix.** At the base commit the fold already
  *refuses* a recorded attempt binding that diverges from its own derivation
  (`FoldError::BindingMismatch`, six occurrences), while `check_dispatched` validates
  a `task_dispatched` lease's **shape only** and `apply_dispatched` grants **whatever
  region the event carries**. The defect walked through that gap; the reader alone is
  a convention, not a guarantee. Closing it is workstream W1 of the pass.
- **The category drifted in fifteen minutes.** The challenge was filed at 13:08
  (`3c09f6e`); the eleventh reader landed at 13:23 (`3362f65`), and by its own doc it
  is *"deliberately only half of the fold's rule"* — a composition, not a delegation.

Assumed, packet-side, not verifiable from this repository: that PR11 is the first
width above one, and PR8/PR9's exact event needs — though every merge-path event the
public `2026-08-12-merge-queue-execution-topology.md` names already exists in
`src/topology/events.rs`, including the absence of `merge_verification_finished`.

## The charter

Default: a feature slice does not edit the frozen set. Deviations come in three
classes. **When a change's class is arguable, it is Class B until ruled otherwise** —
the failure direction is asking, not landing.

- **Class A — disclosed readers.** An *additive, behaviour-neutral* `pub fn` that
  **delegates to existing logic** — no composed half-rules, no new derivation, no
  deletion, no visibility widening of anything else. Permitted without pre-approval
  **only** with the ceremony PR5 and PR7 already used, in the same commit: a ledger
  row carrying the measured split (code / comments / tests), the delegation target
  named, and rustdoc attachment checked (two of PR7's **first eleven** insertions —
  the count at `3362f65`, before `ffcc74a` added the twelfth — detached a
  neighbour's doc comment; the class has already broken a build once via a duplicated
  `#[test]` attribute).

  **Suspended while the moratorium runs.** Item 3 stops reader accretion from this
  record until the pass lands. This class states the *ceremony* a disclosed reader
  carries, not an exemption from that stop: the two answer different questions —
  Class A asks what ceremony a reader carries, item 3 asks whether a reader may land
  at all right now — and a permission-with-ceremony does not survive suspension of
  the underlying permission. During the window there is no self-serve reader route
  and the twelfth reader stands as the last. Class A is dormant, not deleted, and
  revives by item 3's own terms when the pass lands.
- **Class B — behaviour changes toward the packet.** A repair that changes what the
  layer *does*, even where the packet demands it (PR7's
  `&& self.pipeline_reservable()` conjunct is the worked example). Only inside a
  dedicated PR3-layer pass, or with per-instance owner approval **before** landing.
  This is the class the layer's record punishes when it lands piecemeal: PR5's round 7
  was reverted for buying convergence by silently weakening a packet-required refusal,
  and PR7 tripled the fix-introduces-a-defect class in one slice.
- **Class C — design changes.** A new variant of a packet-enumerated inventory, a
  type widening, a changed value, any edit to `DESIGN.md:222` or to packet-transcribed
  content — including anything serde-visible in `src/topology/events.rs`, because
  recorded runs replay from `events.jsonl` and a wire-facing rename is a schema
  change however cosmetic it looks. Owner decision record, always. These are the two
  shapes the 2026-08-20 ruling refused, and that refusal stands as the rule.

**Inside a chartered pass, the frozen set is fully open.** Refactoring,
decomposition, renames and deletions included — under the pass's ceremony:
behaviour-neutrality proofs for pure restructuring, mutation witnesses for behaviour
changes, owner errata for anything in Class C, regenerated artifacts checked in. The
classes above govern slices *outside* a pass; the freeze protects the layer from
piecemeal edits, not from its own scheduled redesign.

**What is blessed today, retroactively and by name:** the ten readers and the Class-B
conjunct at `3c09f6e`, the eleventh reader at `3362f65` — with the note that its
half-rule shape is exactly what workstreams W2 and W5 of the pass must collapse back
into a full delegation — and the twelfth reader `open_no_attempt` at `ffcc74a`, which
landed after the 2026-08-24 adjudication and carries its own per-instance Class A row
(`PR7-FOLD-OPEN-NO-ATTEMPT`). Nothing else.

## Rejected options

- **Enforce the freeze by reverting the repair.** Rejected on Run A: the enforcement
  world is green over an invisible correctness defect, and the fuller enforcement does
  not compile without re-deriving the fold's predicates in the driver — manufacturing
  the second authorities this slice's reviews spent rounds killing.
- **The challenge's standing rule** (*"a slice may add a public reader that delegates
  and changes no behaviour"*). Rejected three ways: it fails its own retroactive test —
  the conjunct, an edit PR7 needed, is neither permitted by it nor caught by its
  decision-record triggers; its supporting claim that it would have *"forbidden both
  edits the owner reverted during this slice"* has no record anywhere in the
  repository; and its category boundary lasted fifteen minutes before its own author
  widened it. It also codifies the weak fix for the defect class it cites — the strong
  fix is W1's fold-side refusal, which that rule would forbid.
- **Total unfreeze — any slice edits the layer freely.** Rejected on the layer's own
  record. The duplication defect class this project measures — four instances in PR7
  alone — lived in *unfrozen* code and is a between-lanes context failure; lane
  workers with partial context are precisely who should not be redesigning the
  vocabulary mid-feature. Effort was never the constraint; unreviewed blast radius is.
- **Keeping the original post-v0.2 timing for the pass.** Rejected on the owner's
  2026-08-24 bar, and on dependency: PR8's merge queue builds directly on lease
  semantics this pass repairs, and stacking it on known-defective vocabulary compounds
  the cost of every later fix.

## Consequences

- `proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md` is the pass's plan; it binds when
  this record lands and cites it.
- `reviews/FINDINGS.md`: §3 gains the challenge's adjudication row citing this record;
  §2 gains a row for the region-validation gap (owner: the pass, W1); the stale
  `PR7-FOLD-ACCESSORS-IN-PR3-LAYER` numbers (+628, nine) are refreshed to the tree's
  (+652 at `3c09f6e`, ten; +766 at `3362f65`, eleven) — an append-style correction,
  not a silent edit. The doc-attachment defects may be repaired on `slice/pr7` before
  it merges.
- `DESIGN.md` gets its compressed edits when the pass lands, each citing this record
  and the pass's own findings; the one edit due now is the §21 note that the PR3-layer
  pass precedes PR8 in the v0.2 sequence.
- The coding-standards document and its mechanical enforcement land on master by their
  own pull request (W10 defines them); `ci.yml` wiring notes
  `PR5-MACOS-CLIPPY-NEVER-RUN`, which belongs to the slice that next opens that file.
- Packet errata are owner-side and enumerated in the proposal's "Owner inputs": the
  effect-site variant, the adjacency row, the `CommandSpec` shape, the region-validation
  contract, and `matches_override`'s field list.
