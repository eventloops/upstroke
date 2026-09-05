# `src/topology/queue.rs`

Extended notes for [`src/topology/queue.rs`](../../../src/topology/queue.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The candidate queue: which prepared candidate the run integrates next.

FIFO by `task_candidate_created` append order over every candidate,
including lineage members — the order is the log's, so a replay and a live
run reach the same next candidate without either of them sorting anything.

Position is not eligibility. A candidate keeps its place while it is
ineligible, and the run integrates the first entry that *is* eligible rather
than blocking behind the head. Four things make an entry ineligible:

* its task is awaiting input (a verification park, or a repair admission);
* its verification is deferred, until the next `defer_wait_elapsed` or
  resume;
* it is an ordinary candidate overlapping any active lineage lease;
* it is a lineage member overlapping an *older* active lineage lease.

The last two are one rule read from both sides. A lineage holds the region a
rejection made contentious, so ordinary work stays out of it entirely, and
two lineages contending for one region resolve by age instead of taking
turns blocking each other.

## `pub struct QueueEntry {`

One candidate holding a place in the queue.

## `pub struct QueueEntry` › `pub paths: PathSet,`

The region the candidate's diff actually touched.

## `pub struct QueueEntry` › `pub lineage_root: Option<TaskKey>,`

The lineage this candidate belongs to, if it is a repair.

## `pub struct QueueEntry` › `pub verification_deferred: bool,`

Set by a deferred verification outage, cleared by `defer_wait_elapsed`
or a resume.

## `pub struct QueueEntry` › `pub defers: u32,`

How many times this candidate's verification has been deferred, counted
against the run's frozen ceiling.

## `pub struct QueueEntry` › `pub sequence: Option<SequenceId>,`

The verification sequence currently open for this candidate, if one is.

## `pub enum Ineligible {`

Why a queued candidate cannot be integrated right now.

## `pub enum Ineligible` › `AwaitingInput,`

The task is parked on a question.

## `pub enum Ineligible` › `VerificationDeferred,`

An outage deferred the verification and the backoff has not elapsed.

## `pub enum Ineligible` › `InsideLineage { root: TaskKey },`

An ordinary candidate inside a region a lineage holds.

## `pub enum Ineligible` › `BehindOlderLineage { root: TaskKey },`

A lineage member inside a region an older lineage holds.

## `pub struct CandidateQueue {`

Every prepared candidate, in the order their refs were created.

## `impl CandidateQueue` › `pub fn push(&mut self, entry: QueueEntry) {`

Append a candidate to the back of the queue.

## `impl CandidateQueue` › `pub fn remove(&mut self, key: TaskKey, generation: GenerationId) {`

Remove the entry for one candidate, keeping the order of the rest.

## `impl CandidateQueue` › `pub fn holds_task(&self, key: TaskKey) -> bool {`

Whether any queued candidate belongs to `key`.

## `impl CandidateQueue` › `pub fn wake_deferred(&mut self) {`

Clear every deferred flag: the backoff elapsed, or the run resumed.

## `impl CandidateQueue` › `pub fn ineligible<F>(`

Why this entry is not integrable, or `None` when it is.

## `impl CandidateQueue` › `let own_age = leases.lineage(mine).map_or(u32::MAX, |lease| lease.age);`

Its own lineage overlaps by construction — that is what the
lease is for. Only a lineage created earlier holds it back.

## `impl CandidateQueue` › `pub fn first_eligible<F>(`

The candidate the run is entitled to integrate next.
