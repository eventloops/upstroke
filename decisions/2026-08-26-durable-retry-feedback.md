# 2026-08-26 — the retry brief survives a crash

**Verdict.** `FailureRecord` gains **one** additive field —
`#[serde(default)] pub detail: Option<String>` — carrying what §11.4 sends back to
the next attempt. `SCHEMA_VERSION` does not move. The schema-4 driver's §11.4 brief
stops being a process-local accumulator and becomes a fold over the durable record,
derived by one function that the live loop and a replay both call. This is a **Class C**
per-instance exception to the 2026-08-20 owner ruling that the frozen files stay
frozen (recorded in `ff0490a`, cited by
[2026-08-20 — what invalidates a frontier review](2026-08-20-review-invalidation-scope.md)),
authorised by the owner on 2026-08-26 against the measurement below and scoped to
exactly this field, its derivation, its witness, and this record.

## The measurement that forced it

The 2026-08-26 frontier review of `75da796`
([record](../reviews/2026-08-26-pr7-frontier-review-75da796.md), finding 2) held that
schema-4 retries violate DESIGN.md by losing failure feedback across a crash. The
repair round's first move was to ask whether the brief could be **rebuilt** from what
the log already holds, because a derivation needs no wire change at all. It cannot.
§11.4 names two feedback sources, and neither reaches the wire. Measured at
`bd3b9cd`:

```
$ grep -c 'required_changes' src/events/mod.rs src/topology/events.rs
src/events/mod.rs:0
src/topology/events.rs:0

$ grep -cE 'log_tail|gate_log|FEEDBACK_TAIL_BYTES' src/events/mod.rs src/topology/events.rs
src/events/mod.rs:0
src/topology/events.rs:0

$ sed -n '731,736p' src/events/mod.rs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureRecord {
    pub kind: FailureKind,
    pub origin: FailureOrigin,
    pub reason: String,
}
```

`reason` is the human-facing summary — `"gate `fmt` failed: exit 1"` — not the gate's
output. `ReviewRecord::outcome` is `Passed | Failed | Unavailable`, three states with no
text. So a resumed run can reconstruct *that* an attempt failed and *how it was
classified*, and nothing about **what the worker must do differently**. Rebuild-on-resume
was the preferred fork and it is not reachable; that is what makes this a wire question
rather than an implementation one.

## The exact shape

One field, on the type that already holds the failure:

```rust
pub struct FailureRecord {
    pub kind: FailureKind,
    pub origin: FailureOrigin,
    pub reason: String,
    /// What the next attempt is told, verbatim (§11.4).
    #[serde(default)]
    pub detail: Option<String>,
}
```

**Where `required_changes` lands, and what `ReviewRecord` needs: nothing.** Both of
§11.4's sources are already unified *before* the record is built.
`ladder::AttemptFailure::feedback` is documented as "a gate log tail (§11.1) or the
reviewer's `required_changes` (§11.2), verbatim", and both are set at classify time:
`classify::gate_failure` calls `.with_feedback(failure.log_tail.clone())`, and the review
path at `engine/attempt.rs:452` calls `.with_feedback(feedback)` where `feedback` is
`required_changes` rendered one-per-line ("required_changes is what the retry gets back
verbatim (§11.4)", the comment above it). The one production construction of an
`AttemptRecord` for a settled attempt — `classify::attempt_record` — copies that field
across.

The census that says so is the compiler's. Adding the field broke **17** initializers;
exactly **one** of them is production — `events::InterruptedAttempt::event`, the record for an
attempt that started and never reported back, which sets `detail: None` because nothing
judged it — and the other 16 are in test modules. The 18th site is
`classify::attempt_record` itself, which is where the value is written and so never
failed to compile. Those two are the whole of it, measured over production regions:

```
$ for f in $(grep -rl 'AttemptRecord {' --include='*.rs' src/ | grep -v '/tests\.rs$'); do
    cut=$(grep -n '#\[cfg(test)\]' "$f" | head -1 | cut -d: -f1); cut=${cut:-999999}
    grep -n 'AttemptRecord {' "$f" \
      | grep -vE 'pub struct AttemptRecord|impl AttemptRecord|-> AttemptRecord \{' \
      | awk -F: -v c="$cut" -v f="$f" '$1 < c {print f ":" $1}'
  done
src/engine/classify.rs:207
src/events/mod.rs:1005
```

`classify::attempt_record`'s own doc said "the one production construction" without a
qualifier — the §22b class, a property claim about the rest of the tree — and is
corrected in the same commit. So `detail` carries whichever source produced it, `ReviewRecord` is not
touched, and the wire gains no second place where feedback can live.

**Why not a new event or a new record.** A second event kind would put the feedback and
the transition it belongs to in two lines with a crash window between them. Schema 3
already settled this shape for the same reason: DESIGN.md:421 records that the *ladder
transition* is "now embedded in a failed `attempt_finished`" so a resumed binary "could
[not] spend the same known failure again after a crash". The feedback is the other half
of that same sentence — the transition says *where the next attempt goes*, the detail
says *what it is told* — and it belongs on the same line.

## `SCHEMA_VERSION` does not move

`events::SCHEMA_VERSION` stays `3`. The argument, written out:

- **Additive-optional.** A log line written before this field existed deserializes to
  `detail: None`. Older logs fold unchanged; no migration, no transition append, nothing
  to refuse. Pinned by
  `recover::tests::a_log_predating_the_detail_field_folds_and_resumes`, which strips the
  key from a real fixture's bytes and resumes the run from the result.

  **`#[serde(default)]` is not what carries that**, and this record does not claim it is.
  Serde's derive already treats a missing `Option<T>` field as `None`: removing the
  attribute leaves that witness green (measured 2026-08-26), and a two-struct probe —
  one field with the attribute, one without, decoding `{"kind":"gate_failed"}` — returns
  `detail: None` from both. The attribute is kept because the owner's authorization
  specifies it, because it states the intent where a reader looks for it, and because
  every other optional field on this wire (`ReviewRecord::adapter`, `::effort`, `::pool`,
  `::preflight_cli_version`) carries it.
- **The strict door still holds.** Schema 4 refuses unknown fields in a transaction
  payload (`refusals[24]`) through `strict::boxed`, which decodes, re-encodes, and
  reports **any key the input carried that the record did not claim back**. Adding a
  field adds an output key, never an unclaimed input key, so an older log passes the
  door exactly as before. The door's exactness argument requires that every embedded
  record "serializes every field it deserializes" and uses no `skip_serializing_if` —
  which is why this field is a plain `#[serde(default)]` and deliberately **not**
  `skip_serializing_if = "Option::is_none"`, even though that would keep existing lines
  byte-identical. Keeping the door exact is worth more than keeping the bytes narrow.

  **Measured 2026-08-26, because the argument alone was not enough.** Adding
  `skip_serializing_if` leaves the door's own precondition test —
  `events::tests::a_known_null_survives_the_strict_door_and_an_unknown_null_does_not` —
  **green**: its fixture's `AttemptRecord` has `failure: None`, so no `FailureRecord`
  appears in the payload it checks at all. One record deeper, an input carrying
  `"detail":null` decodes to `None`, re-encodes to nothing, and the door reports `detail`
  as a key the record did not claim back — refusing every failed attempt's settlement.
  `engine::tests::an_explicit_null_detail_survives_the_strict_door` is that case, in a
  file this exception may touch, and it fails under the attribute and passes without it.
- **No external schema-4 writer exists.** The same repair round narrowed the schema-4
  driver's surface: `src/engine/mod.rs:61` is `pub(crate) mod topology;`, so nothing
  outside this crate can construct or append one of these records. A wire nobody else
  writes cannot be broken for a writer that does not exist.
- **Growth is bounded where it matters and named where it is not.** A gate tail is
  capped at `gates::FEEDBACK_TAIL_BYTES` = 8 KiB before it is stored
  (`engine/topology/attempt.rs:918`), and a timeout's feedback at 2000 bytes. The review
  path's `required_changes` carries no constant cap — it is bounded by the reviewer's own
  verdict envelope. So a failed `attempt_finished` line grows by at most 8 KiB for the
  gate half and by the reviewer's required-changes text for the review half. That is
  stated rather than claimed away: the prompt-side bound (`MAX_FEEDBACK_ENTRIES` = 6) is a
  bound on what a *prompt* quotes, not on what the log stores.
- **The legacy schema-3 wire and `report.json` are unaffected — by the carrier choice
  below, not by accident.**

  > **Amended 2026-08-26, before this record landed anywhere.** This bullet first read:
  > *"`report.json` is unaffected … `detail` is `None` on every successful attempt and
  > every attempt the legacy coordinator settles, because the legacy path constructs its
  > feedback into `LadderRetry` as before and this change adds no call site to it."* That
  > was false, and it was false at the moment it was written. The frontier re-review of
  > `c2c0294` found it as finding A.
  >
  > **The mechanism, measured.** `classify::attempt_record` is *shared*, and
  > `src/engine/coordinator.rs:844` — the live schema-3 path, the one `upstroke run` uses
  > today — calls it with `failure: result.failure.as_ref()`, an `AttemptFailure` whose
  > `feedback` holds the gate tail or the reviewer's `required_changes`. Writing
  > `detail: failure.feedback.clone()` unconditionally therefore put the full text on the
  > legacy wire **and** into `report.json`, which clones these records into
  > `TaskReport.attempts` at `src/engine/report.rs:530` — once per failed attempt,
  > duplicating the `ladder_retry` copy, and reversing the reason `LadderRetry`'s own doc
  > gives for holding the text: *"a gate log tail runs to kilobytes, and `report.json`
  > should not grow one per attempt."*
  >
  > **Why the original claim survived every check.** "This change adds no call site" is
  > true and irrelevant. The change adds no *initializer*; it changes what an existing
  > shared one writes. A census over constructions — the instrument this slice reaches
  > for, and the one that found the second `AttemptRecord` construction below — sees
  > initializers, not **value flow through a shared builder into an existing caller**.
  > Only reading the callers, or a reviewer, finds that. It is the §22b class with a
  > sharper edge: not a claim that aged, but a claim about a caller that was never read.
  >
  > **The repair that makes this bullet true** is `classify::FeedbackCarrier`, an explicit
  > two-variant choice on `AttemptFacts` with **no default**, so a caller must decide and
  > a third engine does not compile until someone does. `coordinator.rs` passes
  > `LadderEvent` (its transitions carry `summary` and `detail` outright); the schema-4
  > driver passes `AttemptRecord` (its transitions have no feedback field at all). Held by
  > `engine::tests::the_legacy_wire_and_report_carry_no_feedback_on_the_attempt_record`,
  > which runs a real legacy gate failure and compares the settled bytes against the bytes
  > `610106b` wrote for the same scenario, and by
  > `recover::tests::a_retried_worker_is_told_what_the_last_attempt_failed_on`, which pins
  > the other carrier at a live schema-4 settlement. Pointing either caller at the other
  > variant fails exactly one of them, and nothing else.

  One residual difference on the legacy wire is stated rather than hidden: `detail`
  serializes as an explicit `null`, so a legacy `failure` object gains that one key.
  `skip_serializing_if` would remove it and is not available — see the strict-door bullet
  above, which is now a measurement rather than an argument. The witness asserts exactly
  that: strip `,"detail":null` and the bytes are `610106b`'s bytes.

## The passages it serves

- **DESIGN.md §11.4**, the live passage the review cited: "failure feedback (gate log or
  `required_changes`) goes back to the *same rung* via session resume … `attempts_per`
  exhausted → next rung, fresh session, **accumulated feedback summary included**." A
  ledger row cannot waive a live passage; the review was right about that, and this is
  the repair rather than a waiver.
- **The replay-survival list** (DESIGN.md §15): "Two things deliberately do not survive
  replay — a session id and its `resume_next` flag." Feedback was a third, undeclared and
  unintended. This removes it rather than adding it to the list.
- **"One fold, not two"**, same passage: the engine "appends an event and folds it back in
  through the same function `resume` and `status` use … and it applies the event *as it
  will be read back* rather than as constructed." The brief is now derived by
  `Brief::record(key, &record)` — called by the live loop on the record it is appending,
  and by `Brief::replay(events)` on the barrier's own parse of the log. One derivation,
  two callers, no side channel that live mode uses and replay does not. The shape is
  `select::Spend`'s, which states the identical rule for the same reason.

**DESIGN.md is not edited by this record.** It does not enumerate `FailureRecord`'s
fields — the freeze is on the file, not on a passage — and both §11.4 and the
replay-survival list already say what the code now does. The compressed-edit obligation in
`decisions/README.md` applies when a record's outcome changes the spec; this one changes
the implementation to meet it.

## What was rejected

- **Rebuild from the existing record.** The preferred fork, and the measurement above
  closes it: neither §11.4 source is on the wire in any form.
- **Leave it as a recorded deviation.** The standing position before this round, carried
  as `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4`. The frontier review's objection is correct
  and is now adopted: a ledger disposition records a decision, it does not amend the sole
  living authority.
- **`skip_serializing_if = "Option::is_none"`.** Would keep every existing line
  byte-identical, at the cost of the strict door's stated exactness argument. Refused
  above.
- **A `feedback` field on the fold.** Would make the brief fold state proper, and would be
  a second frozen-file change (`src/topology/fold.rs`) beyond the one authorised. The
  driver-side derivation reaches the same value from the same bytes, and the scope
  condition is binding.

## Scope

This record authorises exactly: the one field, its population in
`classify::attempt_record` **under the carrier the caller names**, the `Brief` derivation
and its two call sites in the schema-4 driver, and the witnesses. No other frozen-file change rides along. The corresponding ledger
entries are `reviews/FINDINGS.md` §3 (the per-instance Class C approval) and §22 (the
`PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4` row flipping to fixed, sha-stamped).
