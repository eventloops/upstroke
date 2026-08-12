# Decision record — v0.2 worktrees, merge queue, and execution topology

**Date:** 2026-08-12
**Status:** Decided — implementation may begin in the staged order below.
**Inputs:** DESIGN.md §§3–5, 8, 11–15, 19, 21, and 23.2; the current
`Workspace`, scheduler, event fold, and resume implementation; the
[design-council decision](2026-08-11-design-council.md), which named merge
arbitration as the first hard v0.2 decision. Decision: project owner, with the
protocol synthesized in this record.

---

## Verdict

Build v0.2 around **immutable, verified task candidates and one serialized,
event-driven integration queue**. Parallel workers never mutate the run branch.
The coordinator alone writes the event log and Git refs.

1. Each dispatched task gets a detached linked worktree at the run's integration
   HEAD at dispatch. The user worktree is neither switched nor dirtied.
2. A successful attempt is committed to an engine-owned internal candidate ref.
   It becomes `AwaitingMerge`, not `Done`; there is no intermediate state that
   satisfies dependencies.
3. The public `tactus/run-<ulid>` ref is the integration head. It must not be
   checked out in any worktree while the run is live; operators inspect it
   through a detached checkout, and tactus refuses to publish while Git reports
   it checked out. A single merge queue applies candidates in
   `task_candidate_created` event order, skipping only candidates held behind an
   active repair-path lease.
4. If the integration head still equals the candidate's base, the candidate's
   existing gates and reviews verified the exact commit and no work is repeated.
   Otherwise the queue cherry-picks the immutable candidate onto the current
   head in a detached staging worktree and reruns **all recorded gates and review
   passes on that exact proposed tree**. A clean Git apply is not evidence that
   the code still integrates semantically.
5. Only a verified proposed commit may be published. The queue records
   `merge_prepared { expected_head, proposed_sha, ... }`, then advances the run
   ref with Git's compare-and-swap `update-ref <ref> <new> <expected-old>`, then
   records `task_merged`. Dependencies become ready only after `task_merged`.
6. A textual conflict, code-attributed integration-gate failure, or review
   rejection atomically records a fully frozen synthetic `Fix` task inside the
   rejection event. Provider/rate-limit, process-spawn, and runner failures keep
   their existing defer/rebind/halt semantics and never become product edits. A
   repair records the rejecting head as evidence, then starts at the integration
   head current at its actual dispatch with the candidate materialized. It
   inherits the run's frozen routing/effort/review/gate policy and emits a
   replacement candidate through the same queue. Its `mid` minimum
   tier is a floor inside the already-frozen pin/maximum constraints, never a
   reason to override them. A recorded run-scoped repair limit bounds automatic
   generations; the same atomic event registers an over-limit or policy-blocked
   repair as `AwaitingInput` rather than leaving a rejection/question gap. No
   special unreviewed conflict resolver exists.
7. Worktrees and containers meet at a new `Runner` boundary. Adapters produce a
   serializable `CommandSpec`; the runner decides cwd, host/container placement,
   mounts, environment, supervision, and timeout. Workers, gates, and reviewers
   all execute through it. Git stays in the host coordinator and is never an
   agent tool.
8. Establish this topology with `max_parallel = 1` before introducing Tokio.
   Concurrency changes admission and scheduling only; it does not get a second
   merge or recovery path.

This deliberately replaces DESIGN.md's earlier shorthand
`Done -> AwaitingMerge -> Merged`: calling a task done before its code is on the
integration head is both misleading and unsafe for dependency readiness.

## The durable protocol

### 1. Refs and workspaces

The run owns these Git identities:

```text
refs/heads/tactus/run-<run-id>                         integration head
refs/tactus/runs/<run-id>/candidate-prepared/<task>/<gen>  protected, non-authoritative commit
refs/tactus/runs/<run-id>/candidates/<task>/<gen>     immutable candidate
refs/tactus/runs/<run-id>/prepared/<sequence>          proposed integration commit
```

`<task>` above is an engine-issued numeric task key, not the user-authored task
id. Original tasks receive keys in frozen plan order and spawned tasks receive
monotonic keys in the order of the event that atomically registers them
(`task_spawned` normally, embedded in `merge_rejected` for a merge repair). A
sanitized label may follow for humans but is never identity. Refs, worktree
paths, and artifact stems use the key so hostile ids and two ids that sanitize
alike cannot traverse or alias storage.

Task and merge worktrees are detached checkouts under a stable execution root
recorded by `run_started` (default `~/.tactus/workspaces/<repo-key>/<run-id>`).
They are separate from private transcripts and from the user's checkout. A
container sees only the one workspace mounted for its role; sibling worktrees,
the event log, and private artifacts are not mounted.

A task branches from the integration SHA observed at **dispatch**, after all of
its declared dependencies are `Merged`. Independent tasks may therefore begin
from different integration snapshots. A generation identifies one workspace at
one dispatch base, not one worker attempt. A same-rung session-resume retry
reuses that generation and worktree so the cumulative diff survives and is
re-gated. Any retry, unpark, or defer recovery that creates a fresh workspace at
the then-current integration head receives a new generation. At most one
candidate can be prepared from a generation. Candidate refs are immutable, and
integration creates a new proposed commit rather than rebasing or amending a
candidate in place.

The queue uses cherry-pick semantics because the candidate is already one
engine-owned commit. The fast path publishes that exact commit when its parent
is still the integration head. The stale path creates one new linear commit and
records both SHAs, preserving the v0.1 one-commit-per-task history without
retaining ephemeral merge parents.

If a stale cherry-pick is empty because the candidate's complete change is
already present, the queue does not manufacture an empty commit. It re-verifies
the current integration tree, supplying the original candidate patch and
acceptance criteria to the integration reviewer while recording why the
candidate-phase empty-diff rule does not apply. It then records `merge_prepared`
with disposition `already_present` and
`expected_head == proposed_sha == current integration SHA`. The normal
compare-and-swap is therefore a validation-only no-op, after which
`task_merged` records the unchanged SHA. This is the only conflict-free path
without a task commit, and the event lineage makes the reason explicit.

### 2. State and event authority

The durable task states are:

```text
Pending | Deferred | AwaitingInput
        | AwaitingMerge(candidate)
        | AwaitingRepair(fix_task)
        | Merged(integration_sha)
        | Failed
```

`Ready`, `Running`, `Gating`, `Reviewing`, and `MergeVerifying` are views derived
from dependencies and process records. `MergePrepared` is a single run-level
transaction, not a second task terminal state. The run state changes from its
fixed plan-aligned vectors to an ordered task registry so replay can append
synthetic tasks without pretending they were in `plan.normalized.json`.

Schema 3 records at least:

- `task_spawned`: a reusable frozen-spawn payload containing the complete
  synthetic `Task`, resolved ladder and binding constraints, resolved review
  passes, origin, lineage, and immutable task key. Replay never regenerates a
  prompt or routing/verification policy from today's code. An ordinary dynamic
  task carries this payload in `task_spawned`; a merge repair embeds the same
  payload in `merge_rejected` so rejection and registration are one append.
  Auto bindings chosen later remain attempt events, as live capacity routing
  requires, and may select only runner-probed agents from this run.
- `task_dispatched`: the start of a fresh generation: integration base,
  worktree identity, and predicted path leases, written before any process
  starts. A same-rung resumed retry starts another attempt in the recorded
  generation without emitting a second dispatch. A killed attempt therefore
  resumes or cleans the workspace it actually used rather than one reconstructed
  from the integration head that exists later.
- `candidate_prepared`: the **sole** successful settlement for an attempt that
  produces a candidate. It contains exactly one complete attempt record plus
  generation, dispatch base, candidate/tree SHAs, commit message, and changed
  paths; `attempt_finished` is not also emitted for that attempt. Ledger,
  status, export, budget, and replay folds consume this embedded attempt record
  exactly as they consume one ordinary settlement. The engine first creates the
  immutable commit object and pins it under a non-authoritative prepared ref;
  this event is then written before the authoritative candidate ref moves.
- `task_candidate_created`: candidate SHA and ref; folds the task to
  `AwaitingMerge` and fixes its FIFO position.
- `merge_verification_started`: candidate, current head, proposed SHA, and the
  recorded gates/review passes about to run. Every start has exactly one terminal
  record. A pass terminates inside `merge_prepared`; a code rejection terminates
  inside `merge_rejected`; infrastructure unavailability and crash interruption
  use `merge_verification_unavailable` and `merge_verification_interrupted`.
  Those four terminal shapes carry the complete gate/review records, usage/cost,
  and outcome, and ledger/status count exactly one of them.
- `merge_rejected`: conflict or code-attributed gate/review rejection plus the
  complete frozen-spawn payload for the repair task it caused and its admission
  (`runnable` or `human_required` with a complete frozen question). Its single
  fold appends that task to the registry, assigns its key, moves the rejected
  task to `AwaitingRepair`, and puts the repair in `Pending` or `AwaitingInput`.
  A human-required admission is also the authoritative question record for
  status, notifiers, and `tactus answer`; no duplicate `question_raised` is
  emitted. When verification ran, this event also contains its complete failed
  terminal record; no separate `merge_verification_finished` is emitted.
  Infrastructure outcomes use their unavailable/interrupted events and ordinary
  scheduling policy instead of this event.
- `merge_prepared`: disposition, expected integration head, proposed SHA,
  candidate SHA, verification source (`candidate` fast path or a recorded merge
  verification), and every task in the candidate's repair lineage that this
  commit satisfies. For `already_present`, proposed and expected are the same
  current-head SHA. When stale or already-present verification ran, this event
  is also its complete successful terminal record; there is no success event
  followed by a second prepare append.
- `task_merged`: the successful publication transaction and final integration
  SHA, including an `already_present` transaction that validated an unchanged
  ref.

These are execution-semantic events, so fresh v0.2-topology runs start at event
schema 3. Older binaries must reject schema 3 before folding it; this is not an
additive field smuggled into schema 2. An existing schema-1 or schema-2 run stays
on the v0.1 sequential execution path until it finishes: schema 1 may perform
the existing 1 -> 2 identity upgrade, but no live run ever appends a 2 -> 3
transition. `TaskCommitted` remains the schema-2/v0.1 event and is not overloaded
with two meanings. Starting a new run is how an operator adopts the v0.2
topology.

Only the coordinator owns `EventLog` and Git ref mutation. Tokio workers return
typed results over channels. This preserves the current invariant that live
state and replay are one fold and avoids relying on concurrent append order or
filesystem locking as scheduler semantics.

### 3. Verification freshness

The candidate attempt keeps the existing standard: capture the engine-owned
diff, run every recorded gate, run every configured review pass at the recorded
effort, then prepare the candidate commit. The task worktree is scrubbed after
`task_candidate_created` durably records the immutable candidate, never merely
because the verified tree or unrecorded Git object exists.

At integration:

| Relationship to current integration head | Required verification |
|---|---|
| Candidate parent equals head | Reuse the candidate's recorded gates/reviews; the commit is the exact object they judged. |
| Head advanced, cherry-pick is clean | Rerun every recorded gate and review on the proposed integrated tree; the review diff is proposed commit vs. current integration parent. |
| Candidate change is already wholly present | Rerun gates on current head and review that exact tree against the original candidate patch/acceptance; record the no-op disposition instead of invoking the candidate empty-diff rejection. |
| Cherry-pick conflicts | Do not judge conflict markers; spawn an integration Fix task. |
| Integrated gates or review reject the code | Do not publish; spawn an integration Fix task with the evidence. |
| Runner/provider/reviewer infrastructure fails | Leave the candidate queued and apply the ordinary halt, defer, or pool-rebind policy; do not ask a Fix task to edit code without code evidence. |

This spends a second review on stale candidates. That is intentional. Review
has measured as expensive, but moving it only to the merge queue would serialize
the whole verification pipeline and substantially complicate attempt/crash
accounting; never repeating it would weaken the exact-commit guarantee precisely
when parallel work changed the context. Record the second-pass rate and cost.
That evidence can justify a later optimization; cost alone cannot justify
quietly changing what `Merged` means.

### 4. Repair tasks

A merge rejection emits one `merge_rejected` event whose embedded frozen-spawn
payload has a run-local monotonic id such as `merge-fix-0001-<task>`.
Hash-derived ids alone are not collision or replay authority. There is no
second `task_spawned` append on this path: the rejection's one fold both puts the
rejected task in `AwaitingRepair` and registers the repair. Its admission field
makes that repair either `Pending` or `AwaitingInput` with a frozen question.
Its payload includes:

- the original task and complete repair lineage;
- the immutable candidate SHA and rejecting integration SHA;
- conflict paths or failing gate/review evidence;
- the original acceptance criteria plus the requirement to preserve already
  merged behavior; and
- path hints expanded by the candidate's actual changed paths and conflict
  paths, with `min_tier = mid`. That floor is intersected with the original
  task's recorded hard pin and maximum. If the intersection is empty, the new
  repair is atomically registered `AwaitingInput`; it cannot run unless an
  answer records an explicit one-off binding. Declining fails the lineage. The
  engine never silently breaks a pin or policy ceiling.

At actual dispatch, the engine records the then-current integration head as the
repair's generation base. For a text conflict, it applies the candidate there
without committing, leaving the unmerged index for the worker to resolve. For a
semantic rejection, it materializes the clean proposal against that current
head and supplies the original failed evidence. The payload's rejecting head
remains immutable lineage evidence, not a promise to start later work from a
stale tree. The worker may edit but never commit; the engine refuses a result
with unresolved index entries and then runs the ordinary gates and reviews.

The repair holds leases on its **actual** affected paths. The queue may continue
publishing candidates whose known changed paths are disjoint, so one hard merge
does not stop the run, but it will not knowingly move another overlapping
candidate ahead of the repair. When the replacement candidate merges,
`merge_prepared.satisfies` settles both the repair task and its original plan
task (and any prior repair generations) at the same integration SHA.

`run_started` freezes `max_merge_repairs` (default 2) per original plan task.
Every automatic `merge_rejected` in a lineage consumes one of those generations;
a new synthetic task does not reset the counter. When the next repair would
exceed the limit, `merge_rejected` still atomically registers its complete task
but gives it `human_required` admission, so no process starts and no payload has
to be regenerated after an answer. A human answer explicitly activates that one
recorded generation; another rejection registers another parked generation and
requires another answer. Exhausting a repair's normal ladder uses the same
human-top-rung/failure policy. The autonomous path is therefore bounded, while
an operator can still continue deliberately with the latest evidence.

### 5. Crash boundaries

Git objects and JSONL cannot be one transaction. Recovery therefore uses
record-before-authoritative-effect plus exact, narrow adoption:

| Crash point | Resume action |
|---|---|
| Before `candidate_prepared` | The attempt is interrupted under the existing rule; any unrecorded prepared ref/object and edits are discarded. |
| After `candidate_prepared`, before `task_candidate_created` | Promote the protected object, or recreate only its final ref, when the recorded candidate object still exists and its parent, tree, task/generation, and message all match; a missing/different object refuses rather than synthesizing a new SHA. Then append `task_candidate_created`; that resume-time append fixes the candidate's FIFO position and moves the task to `AwaitingMerge`. |
| Candidate recorded, worktree missing | Recreate the worktree/ref from the recorded SHA; directory existence is never state. |
| After `merge_rejected`, before repair dispatch or question delivery | No synthesis is required: the same event already registered the complete frozen task, moved its parent to `AwaitingRepair`, and made the repair `Pending` or `AwaitingInput`. A pending repair uses the ordinary record-before-process dispatch; the embedded question is immediately visible in status and notification delivery uses the existing retry/idempotency rules. |
| Proposed commit exists, no `merge_prepared` or `merge_rejected` terminal event | Settle any dangling merge-verification process as interrupted/unknown-spend, treat the proposal as unverified residue, and rerun verification; a pass or code rejection is never stranded in a separate finished event. |
| After `merge_prepared`, run ref still equals `expected_head` | Retry the same compare-and-swap to the recorded proposed SHA, then append `task_merged`. For `already_present` the two SHAs are equal, so this is a validation-only no-op followed by the same settlement. |
| After the ref moved, before `task_merged` | If the ref equals the recorded proposed SHA, append `task_merged`; any third SHA is foreign history and resume refuses. |
| `task_merged` exists but the ref disagrees | Refuse; the log and integration branch no longer describe the same run. |

Worktree cleanup happens only after the event proving the corresponding state
is terminal. Internal candidate/prepared refs remain until `run_finished`, then
may be pruned after the report is durable. The run lock still excludes two
coordinators; ref compare-and-swap is the second line of defence against an
external Git mutation.

### 6. Runner and concurrency contracts

Adapters stop returning a live `std::process::Command`. They return a data-only
`CommandSpec` (program, args, environment overlay, and stdin). A `RunnerRequest`
adds workspace, role (`probe`, `implement`, `gate`, `review`), timeout, agent identity,
and credential/mount policy. `Runner::run` returns the existing `ProcessOutput`.
Adapters still own CLI flags, prompt delivery, permission settings, output
parsing, session resume, and rate-limit recognition; runners do not interpret
agent output.

`CommandSpec.env` overlays a runner-defined base; it is never a replacement for
the process environment. The host base starts from the Tactus process
environment, while the container base starts from the image environment. The
runner then supplies role-scoped `HOME`, `PATH`, and credential locations before
applying non-reserved adapter overrides. An adapter that conflicts with a
runner-reserved environment key is a pre-flight error rather than an
order-dependent override. Probes and real processes use the same composed
environment and mounts.

Pre-flight probes also execute through the selected runner. The capability and
version snapshot must describe the CLI that will run, not a similarly named host
binary outside the container; gate-shell/program availability is checked inside
the same boundary. A container image that lacks a recorded shell or CLI refuses
before spend.

Every process capable of executing repository-controlled code goes through the
runner. In particular, gates do not remain a host-side escape hatch while agents
move into containers. Reviewer workspaces mount read-only; implementation/gate
workspaces mount read-write; private run artifacts and the event log are absent.
Persistent per-agent credential volumes survive token rotation. Codex retains
the already-decided `external-sandbox` exception only inside a standard-hardened
container. Host runner behavior remains available and honestly provides no OS
boundary around gate code.

A linked worktree's `.git` file points back into the authoritative repository,
which must not simply be mounted into an agent container: that would expose all
candidate refs and let a process contend on the real index. The container runner
overlays a disposable, role-scoped Git view for commands such as `git diff` or
`git describe`: detached HEAD and index for the exact workspace, no engine refs,
and read-only access to the object store. Writes affect only disposable metadata.
The host coordinator alone can move real refs. Container pre-flight includes a
Git-dependent gate so this projection is proven rather than assumed.

Tokio adds three independent controls:

- a global active-pipeline limit (`max_parallel`), held only while a task or
  integration pipeline is actively executing. It is released while a candidate
  waits in the merge queue and whenever a task is parked, deferred, or halted,
  then acquired again for a fresh dispatch or integration verification;
- per-agent/pool semaphores acquired by **every** CLI process, including reviews
  and integration re-reviews; and
- one merge permit, held only while preparing/verifying/publishing a candidate.

Dispatch path leases use normalized non-glob prefixes from `path_hints`; prefix
ancestor/descendant pairs overlap, and an absent or repo-wide hint takes a global
lease. This is deliberately conservative and only an admission optimization:
actual changed paths replace predictions once a candidate exists, and merge
verification remains the correctness boundary. Edits outside hints are recorded
as plan-quality warnings rather than trusted away.

A predicted dispatch lease is held only while its generation can still receive
worker edits. A same-session immediate retry retains that generation, worktree,
and lease. Parking or deferring discards the non-resumable worktree and releases
both the active-pipeline permit and predicted lease; an answer or reset
re-dispatches the task at the then-current integration head under a new
generation. Candidate creation converts the prediction to an actual-path queue
lease: the global permit is released while queued, but the narrower actual lease
remains until merge, rejection, or conversion into the repair lineage so known
overlapping work does not start from a head that lacks the candidate. A repair's
actual-path lease follows the same terminal rules.

Budget admission remains centralized. Because CLIs report cost only after a
process returns, parallel in-flight work can overshoot a reported-dollar ceiling
by the combined unknown cost of admitted processes. The ledger must state that
bound honestly; operators requiring the narrowest stop use `max_parallel = 1`
until providers expose pre-authorizable envelopes. Concurrency must not turn the
existing reported-spend ceiling into a falsely exact guarantee.

## Implementation order and acceptance

1. Add the schema/state/task-registry changes and fault-injection seams.
2. Add the workspace manager, immutable refs, `CommandSpec`, and host/container
   runner boundary while execution is still sequential.
3. Run every task through candidate creation and the merge queue at
   `max_parallel = 1`; prove fast-path history and every crash window.
4. Add stale-candidate verification and conflict/semantic repair tasks; prove a
   rejected proposed tree never advances the run ref.
5. Replace the sequential drain with the Tokio coordinator, task/pool permits,
   and predicted/actual path leases. No Git or event behavior changes in this
   step.

The implementation is not complete until tests demonstrate:

- the user's checked-out branch, index, and files are byte-for-byte unchanged;
- two independent tasks visibly overlap, merge through one queue, and a
  dependent starts from a head containing both;
- absent/overlapping hints serialize dispatch, while disjoint hints permit it;
- a clean but stale candidate is re-gated and re-reviewed;
- conflict, gate failure, and review failure each produce a replayable Fix task;
- kills at every tabled crash boundary neither duplicate nor lose a commit;
- deleting a task worktree does not lose recorded work;
- an already-present stale candidate settles explicitly without manufacturing
  an empty commit;
- a same-rung resumed retry retains and re-gates the cumulative generation tree,
  while a park or defer releases admission resources and later uses a new base;
- every candidate-producing attempt contributes exactly one settlement to
  replay, status, budget, ledger, and `export-decisions`;
- every started integration verification contributes exactly one terminal
  settlement through prepared, rejected, unavailable, or interrupted, with no
  pass-to-prepare or failure-to-repair crash gap;
- a kill after `candidate_prepared` appends the missing candidate event on
  resume, and a kill after `merge_rejected` neither loses nor duplicates its
  repair task;
- repeated integration rejections stop at the frozen automatic repair limit,
  atomically register the next repair and question in `AwaitingInput`, and spend
  nothing further until an answer; a delayed repair dispatch uses the current
  integration head while retaining the rejecting head as lineage evidence;
- schema-2 runs continue on the sequential topology without a 2 -> 3 upgrade,
  while a schema-2 binary refuses a fresh schema-3 run;
- status/replay and the live coordinator derive identical task and queue state;
- host and container runners preserve adapter parsing, while the container
  prevents reviewer writes and gate writes outside its mounted workspace and a
  Git-dependent gate sees only the disposable role-scoped Git view; both runners
  prove the same base-plus-overlay environment contract during probe and run;
  and
- `max_parallel = 1` retains one linear engine commit per plan task in the
  conflict-free case.

Repository gates remain exactly the release gates: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets`.

## Measured vs. assumed

**Measured on this repository:**

- The current engine has one mutable workspace, commits successful tasks
  directly to the run branch, treats `Done` as dependency-ready, and has one
  event writer/fold. Resume narrowly adopts one engine-shaped commit past the
  recorded head.
- On Windows Git on 2026-08-12, a detached linked worktree created and retained
  a commit; `git update-ref <ref> <new> <expected-old>` advanced the run ref when
  the expected SHA matched and rejected a stale expected SHA. The throwaway
  repository was removed after the check.
- Review cost has been 44–77% of spend in the measured cheap-worker runs, and a
  reviewer has caught a semantic failure after build/tests passed (DESIGN.md
  §23.2). A second review is therefore both expensive and non-trivial.
- Codex/container sandbox behavior and the need to confine gate-executed code
  were measured on 2026-08-11 and remain governed by DESIGN.md §21.

**Assumed until the implementation supplies evidence:**

- Worktree creation and duplicate build artifacts are cheap enough for the
  expected 2–3-way personal workload.
- `path_hints` are accurate enough to unlock useful parallelism; the design does
  not rely on that accuracy for correctness.
- Most stale candidates either pass the second verification or fail with enough
  evidence for one repair task to resolve them.
- The throughput gained from parallel implementation exceeds the serialized
  queue and extra-review cost.
- Persistent credential volumes behave correctly for every supported CLI, and
  Windows/WSL filesystem performance is acceptable only with the repository on
  the Linux side. Both require per-adapter acceptance runs.

## Rejected options

1. **Merge into the run branch, then run gates/review.** Rejected: a crash or
   observer can see an unverified integration head, and rollback becomes another
   authoritative ref mutation to recover.
2. **Treat a clean cherry-pick as still verified.** Rejected: textual
   non-overlap says nothing about cross-file behavior, generated code, feature
   interactions, or repository-wide review findings.
3. **Always rerun verification, even when candidate parent equals head.**
   Rejected: it buys no freshness—the commit object is identical—and doubles
   the most expensive measured phase for sequential and uncontended work.
4. **Move the only review into the merge queue.** Deferred, not adopted: it can
   avoid double review but serializes model judging and splits one attempt's
   worker/review/crash accounting across a durable queue. Measure the adopted
   protocol before paying that complexity.
5. **Static plan-order merging.** Rejected: waiting for an earlier unrelated
   long task destroys the parallel speedup. Candidate-created event order is
   deterministic for replay of a run; identical reruns are not promised the
   same race outcome.
6. **Rebase candidate branches in place.** Rejected: it rewrites already judged
   evidence, invalidates session/worktree assumptions, and makes crash adoption
   ambiguous. Proposed commits are new immutable objects instead.
7. **Merge commits for task worktrees.** Rejected: ephemeral branch topology
   leaks into the durable run history and complicates dependency bases and
   recovery without preserving information the event lineage does not already
   retain.
8. **Let each worker append events or merge its own branch.** Rejected: ordering,
   budget admission, and replay would depend on concurrent filesystem races; a
   compromised worker could forge the authority that judges it.
9. **Resolve conflicts with an unreviewed Git heuristic or a special cheap
   prompt.** Rejected: conflict resolution writes product code. It is an
   ordinary Fix task with the normal ladder, frozen effort policy, gates, and
   reviews.
10. **Use a hosted PR per task.** Rejected: the execution engine never speaks
    HTTP, task latency becomes a remote-service property, and local immutable
    refs already provide the audit identity needed here.
