---
id: SWEEP-FOLD-APPLY-QUEUE-SEQUENCE-UNREAD
severity: P3
disposition: deferred     # the field belongs to src/topology/queue.rs and six of its eight construction sites are in row 39's file, both outside this one-file sweep
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/queue.rs:15
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/fold/tests.rs` (queue row 39), which holds six of the eight `QueueEntry` literals, together with whoever owns `src/topology/queue.rs`
---

## Failure sequence

Location as first recorded: src/topology/queue.rs:15 (the field); src/topology/fold/apply.rs:249,259,283,290,306 at `ee5dc81f` (every write)

`QueueEntry::sequence: Option<SequenceId>` is written by five statements, all of them in
`src/topology/fold/apply.rs`, and read by nothing in the crate:

```
$ grep -rn 'sequence' src/topology/queue.rs
15:    pub sequence: Option<SequenceId>,
```

That declaration is the only line of `src/topology/queue.rs` that names the field. Its five
writes, all in `src/topology/fold/apply.rs` at `ee5dc81f`, are the initialiser at line 249
(`sequence: None`), line 259 (`entry.sequence = Some(started.sequence)`) and the three clearings at
lines 283, 290 and 306 (`entry.sequence = None`). No line anywhere in `src/` reads it back.

`src/topology/queue.rs` never mentions the field after declaring it: `CandidateQueue::ineligible`
decides eligibility from `awaiting_input`, `verification_deferred`, `paths` and `lineage_root`, and
`first_eligible` from `ineligible` alone. No checker reads it either — every event that cites a
sequence resolves it against `RunState::transaction`, not against the queue.

The measured consequence is that three of this file's decisions cannot be witnessed. At
`ee5dc81f`, with the whole `topology::fold` suite (131 tests) as the oracle, each of these
mutations was applied alone, run and reverted, and **survived**:

| Mutation | Site |
|---|---|
| a verification start stops recording its sequence on the queue entry | `apply_verification_started` |
| a parked outage keeps the candidate's open sequence | `apply_verification_unavailable`'s `Parked` arm |
| an interrupted verification keeps the candidate's open sequence | `release_transaction` |

The three are not defects in themselves — the field they maintain is consistent, and
`RunState`'s `PartialEq` makes it part of the live-versus-replay comparison, so a divergence
*between* those two paths would still be caught. What no test can catch is the field going wrong in
both paths at once, because nothing downstream asks it anything.

## What the change that takes this up should do

Decide which the field is, and make the code say so:

- **If it is dead**, delete it. That is one field in `src/topology/queue.rs`, the five writes above
  (all in `apply.rs`, all deletions), and the six `QueueEntry` literals in
  `src/topology/fold/tests.rs` (lines 6991, 8362, 8392, 8401, 8718, 8811 at `ee5dc81f`). The three
  mutations above stop existing rather than surviving.
- **If it is the queue's record of which transaction owns an entry**, give it the reader that makes
  it load-bearing — `CandidateQueue::ineligible` refusing an entry whose `sequence` is `Some`, say,
  which is today implied by `check_transaction_start`'s "one transaction at a time" rather than
  stated by the queue — and then the three mutations above are killed by whatever test covers that
  reader.

Either way the decision belongs with the queue's own sweep, not with the application that keeps the
field in step.
