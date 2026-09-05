## 26. Merge queue and execution topology protocol

Decided 2026-08-12. This section carries the verdict and the durable protocol of that decision verbatim (its section numbers below are the record's own); §7, §14 and §15 summarise it. Implementation order, the measured-versus-assumed ledger and the rejected options were record material and are not reproduced.

### Verdict

Build v0.2 around **immutable, verified task candidates and one serialized,
event-driven integration queue**. Parallel workers never mutate the run branch.
The coordinator alone writes the event log and Git refs.

1. Each dispatched task gets a detached linked worktree at the run's integration
   HEAD at dispatch. The user worktree is neither switched nor dirtied.
2. A successful attempt is committed to an engine-owned internal candidate ref.
   It becomes `AwaitingMerge`, not `Done`; there is no intermediate state that
   satisfies dependencies.
3. The public `upstroke/run-<ulid>` ref is the integration head. It must not be
   checked out in any worktree while the run is live; operators inspect it
   through a detached checkout, and upstroke refuses to publish while Git reports
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
   The engine's ref primitives take a full hexadecimal object id on both sides
   and refuse the null id on either before the mutating `update-ref`, because
   Git reads it as a condition rather than an id (measured, git 2.43). As the
   expected old of a compare-and-swap it means "must not exist": against an
   existing ref the swap exits 128 and preserves it, and against an absent
   ref it creates. As the expected old of `update-ref -d <ref> 0{40}` it
   deletes unconditionally. As the new value it means "must not exist
   afterwards": with a matching expected old it deletes the ref, and on an
   absent ref it creates nothing, both with exit 0.
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

### The durable protocol

#### 1. Refs and workspaces

The run owns these Git identities:

```text
refs/heads/upstroke/run-<run-id>                         integration head
refs/upstroke/runs/<run-id>/candidate-prepared/<task>/<gen>  protected, non-authoritative commit
refs/upstroke/runs/<run-id>/candidates/<task>/<gen>     immutable candidate
refs/upstroke/runs/<run-id>/prepared/<sequence>          proposed integration commit
```

`<task>` above is an engine-issued numeric task key, not the user-authored task
id. Original tasks receive keys in frozen plan order and spawned tasks receive
monotonic keys in the order of the event that atomically registers them
(`task_spawned` normally, embedded in `merge_rejected` for a merge repair). A
sanitized label may follow for humans but is never identity. Refs, worktree
paths, and artifact stems use the key so hostile ids and two ids that sanitize
alike cannot traverse or alias storage.

Task and merge worktrees are detached checkouts under a stable execution root
recorded by `run_started` (default `~/.upstroke/workspaces/<repo-key>/<run-id>`).
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

#### 2. State and event authority

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
  status, notifiers, and `upstroke answer`; no duplicate `question_raised` is
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
schema 4. The topology originally reserved schema 3, but the complete-review
and atomic sequential-attempt settlement contracts used that boundary first: a
schema-2 binary can ignore the recorded timeout and truncate a prompt, or ignore
an embedded ladder decision and repeat a settled failure, so schema 3 must
remain their downgrade barrier. Older binaries must reject schema 4 before folding
topology events; this is not an additive field smuggled into an earlier schema.
An existing schema-1, schema-2, or schema-3 run stays on the sequential execution
path until it finishes. It may perform the review-contract upgrade to schema 3,
but no live run ever appends a 3 -> 4 transition. `TaskCommitted` remains the
sequential event and is not overloaded with two meanings. Starting a new run is
how an operator adopts the v0.2 topology.

Only the coordinator owns `EventLog` and Git ref mutation. Tokio workers return
typed results over channels. This preserves the current invariant that live
state and replay are one fold and avoids relying on concurrent append order or
filesystem locking as scheduler semantics.

#### 3. Verification freshness

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

#### 4. Repair tasks

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

#### 5. Crash boundaries

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

#### 6. Runner and concurrency contracts

Adapters stop returning a live `std::process::Command`. They return a data-only
`CommandSpec` (program, args, environment overlay, and stdin). A `RunnerRequest`
adds workspace, role (`probe`, `implement`, `gate`, `review`), timeout, agent identity,
and credential/mount policy. `Runner::run` returns the existing `ProcessOutput`.
Adapters still own CLI flags, prompt delivery, permission settings, output
parsing, session resume, and rate-limit recognition; runners do not interpret
agent output.

`CommandSpec.env` overlays a runner-defined base; it is never a replacement for
the process environment. The host base starts from the Upstroke process
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

#### Verification contract: the effect-site bijection

Added 2026-09-05 (PR #146) under §13's same-change rule; not part of the verbatim record above.
It records what `src/topology/effects` enforces, so that the checker has a design sentence to be
held to rather than a code comment.

**The inventory is closed and typed.** Every external-effect site a schema-4 run has is a
variant of `EffectSiteId` (grouped by funnel: worktree, snapshot, ref, object, run directory,
event, answer, lock, report, process, container). A site that is not a variant does not exist, and
a registry entry naming one is refused at the wire format. Each site carries, by construction,
its resource row, its fault-matrix row, its scope (topology, shared or legacy), its parent-side
sub-effect points and their injection modes and platform, its command-internal residue classes
and the residue elements each must construct, and at most one observable order — which of the
effect and its event append is durable first, fixed by the site's adjacency.

**The fault-injection registry is a document of entries keyed (site, phase, order)**, where a
phase is the before hook, the after hook, one sub-effect point in one injection mode, one residue
class, or the no-execution record below. An entry's expected residue and resume action are the
site's own semantics, not the entry's opinion of them; the format refuses an entry that disagrees,
that carries executed-hook evidence for a residue class (no parent hook can observe a
command-internal prefix), or that carries recovery-proven evidence for a hook.

**The bijection check** (`check_bijection`) takes an inventory, a hook harness, the entries as a
bare slice, and a host, and returns every way the following fails; an empty answer is the claim
holding **over that inventory and that host** and nothing wider. For every claimed site in the
inventory: the harness observed both hook phases and, in every injection mode it supports, every
sub-effect point the host requires; an entry exists at every observable order for each of those
phases and carries passing evidence; and each residue class has a recovery-proven entry whose
synthetic records construct, recover and classify every element the class lists, and whose
sampling record is non-zero, classifies every sample, and accounts for exactly `n` samples. An
entry for a site outside the inventory, a duplicate key, and an entry the format would refuse
are each reported. Legacy-scoped sites carry no site-coverage requirement. The entry audit and
the empty-fast-sequence check below apply independently of the inventory. With an empty inventory,
every supplied entry is reported as outside it; even with no entries, a begun fast sequence with
no hook observation is reported.

**The fast-path no-execution record.** Item 4 above fixes that an integration whose base is still
the head publishes the exact candidate: no staging worktree is added, nothing is cherry-picked, and
no prepared pin is taken. The three sites those effects belong to therefore carry a fifth kind of
entry, the no-execution record, naming every fast sequence the suite exercised. A sequence is
exercised only by what the harness observed inside it: the harness records a hook of some site
while recording that sequence. This observation is the marker. The check includes a sequence
that is still open and does not establish that the sequence ended or an integration completed.
The check holds the record to the harness: it fails when the suite began no fast sequence at
all (an empty harness is not evidence), when a begun sequence had no hook observed inside it (a
trace the harness saw nothing in is not an exercised fast integration, whatever the records say of
it), when the record names a sequence the harness never began, when the record says nothing about
a begun sequence (reporting beside the gap whether the harness observed a hook of the site in it),
and when the record names a sequence in which the harness did observe a hook of the site — a
contradiction between record and observation. The record is
additional to, never instead of, the site's ordinary coverage: the same three sites execute on
the stale path and are held to every requirement above.

**What the harness can witness, and what the check therefore cannot say.** The harness records
an execution only when a funnel calls its hook, and a sub-effect point only when its armed
injection fired. So an absence of observation is not evidence of non-execution: a path that
performs an effect without its hook is invisible to the harness, and the check's report of "no
hook observed" inside a sequence is exactly that and no more. That is why the record's own names
are the claim and the observations are what the claim is held to, and why a registry's sampling
count `n` is checked for self-consistency and not against any authority: the number is the
registry's, and whether it held across runs is a property no single document can show.

**Diagnostics are typed.** A failure names its site as `EffectSiteId` and its phase as the hook
phase or entry phase it is about. The free text a failure carries in its own fields is a document's
own words: a resume action in the fault matrix's wording, and the name a suite gave a fast
sequence. An entry the format refuses is reported with the format's own error value embedded, and
that value carries what the entry wrote in the field the format refused — a residue detail, a
resume action, a site or phase name as text — so a hand-edited document's own text can reach a
reader through that one variant, quoted, never interpreted.
