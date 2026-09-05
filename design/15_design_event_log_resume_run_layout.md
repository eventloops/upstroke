## 15. Event log, resume, run layout (P6)

The durable run artifacts are **split in two**, by who is allowed to read each half. v0.2 adds a third, disposable execution root that contains code but no authority:

```
<repo>/.upstroke/runs/<run-id>/     # run-id = ULID — the ops surface
    events.jsonl                  # append-only source of truth
    plan.normalized.json          # the frozen plan this run executes
    artifacts/                    # conventions-brief.md, decisions-record.md, contracts
    questions/<question-id>.json  # rendered question payloads for notifiers
    answers/<question-id>.json    # answers dropped by `upstroke answer`
    run.lock                      # advisory; OS-released, so a crash leaves nothing stale
    report.json                   # projection of the log for humans; never read back
~/.upstroke/runs/<run-id>/          # agent-authored — outside every agent's reach
    transcripts/<task>-<attempt>.json
    reviews/<task>-<attempt>-review.json
    settings/<task>-<attempt>.json    # the per-attempt permission surface
    gates/<task>-<attempt>-<gate>.log
    gate-worktrees/                    # synced intents + disposable exact snapshots
~/.upstroke/workspaces/<repo-key>/<run-id>/  # v0.2; exact path recorded on run_started
    tasks/<task>-<generation>/          # detached linked worktrees
    merge/                              # detached integration staging worktree
upstroke.toml                       # repo-root config, checked in
```

The split keeps transcripts and reviewer records out of ordinary workspace reads, but the shipped host runner does **not** make the public half authoritative against hostile candidate code. Adapter deny rules reduce direct agent-tool access; they are defence in depth, not an OS boundary. Repository-controlled gates execute candidate build/test code as the Upstroke user and can discover the source worktree and modify `.upstroke`. A host-run event log is therefore an operational recovery record for trusted repositories and plans, not a tamper-resistant attestation. Moving coordinator authority outside every role mount and enforcing that with the external/container runner is a blocking backlog item before any stronger claim; use a dedicated OS account or VM for untrusted input.

The v0.2 execution root is deliberately non-authoritative. A container receives only its role's one worktree mount; it never receives the public log, sibling worktrees, or private artifacts. On the host runner the agent permission surface remains the boundary and gate code is not OS-confined — the reason the container runner exists. Worktree disappearance is recoverable from events and internal refs, and cleanup follows a terminal event rather than creating one.

The execution root is created only when the managed base is a real directory, the chain from the authorized private root down to the root carries no symlink or reparse point and no regular file, the canonical root is inside no repository worktree, and no foreign worktree is inside it. A run id is one plain path component, so the path recorded above is the only one it can name. Every create, reclaim and delete revalidates all of that before entering its effect funnel, and re-checks the chain inside the funnel, after its before-hook and immediately before the effect.

Current host-process crash containment is deliberately platform-specific. On Unix, ordinary descendants remain in an isolated process group and a separate cleanup reaper retains the run's cleanup lease if the conductor is killed; code that deliberately daemonises out of that group remains outside the host-runner contract. On Windows, each command is created suspended, assigned to a private kill-on-close Job Object, and only then resumed. Direct-child success and timeout both terminate and boundedly observe that job empty; abrupt conductor death closes its non-inheritable handle and lets the kernel terminate ordinary descendants. PID scanning and `taskkill` are not part of the ownership protocol. Exact gate/review worktrees likewise record and sync a private intent before `git worktree add`; resume reclaims every such registration before it switches branches or dispatches another worker.

**When a Unix helper does not start.** The cleanup reaper and the job-control guard are forked before any agent exists. Each must acknowledge startup within the existing two-second read budget. A launch without that acknowledgement fails and reports the elapsed wait, budget, descriptor ceiling, and the results of the existing kill and wait cleanup. The cleanup wait itself is not bounded by the startup budget. A collected exit status does not establish when the helper exited relative to that deadline, and a PID observation does not establish identity if an embedding host can reap children independently.

The reaper also reports its first failing startup operation through a fixed-size record on its existing acknowledgement channel. The record names disposition setup, child process-group isolation, cleanup-lease open or shared lock, or initial READY publication. It preserves the syscall errno and, for a lease, its zero-based ordinal in the launch's cleanup-path list. A zero-progress READY write has no syscall errno. The child uses only fixed storage and post-fork-safe operations, then exits with status 1. The successful READY byte is unchanged. The parent reads the whole reply against one deadline and distinguishes a child record from timeout, EOF, syscall failure, and malformed or truncated data. Failure to deliver a complete record leaves the child stage unknown. This evidence identifies an observed operation failure when available; instrumentation alone does not explain a prior macOS startup failure.

**Synced intents.** Each intent file is one JSON object with exactly four string fields, in this
order: `kind`, `slot`, `run_id`, `incarnation`. `kind` is one of `task`, `staging` or `snapshot`.
`slot` is the slot's identifier, `<namespace>/<component>`, the canonical spelling of a slot the
engine validated; it names the slot for whoever reads the record, and the filesystem path is
derived from the intent's file name and the execution root, never from this field. A reader
accepts no other key, no alias for a key or a kind word, no default for a missing field, and no
record whose `kind` disagrees with the namespace of its `slot`; any of those is refused. No code
in the engine acts on a record's contents: reclaim trusts the intent's file name alone, and the
record is provenance for an operator and for any future reader, which this contract binds. The
implementation is `IntentRecord` in `src/workspace_manager/naming.rs`.

Every transition is an event `{ts, event, task?, attempt?, rung?, profile?, data}` — including `question_raised`, `question_answered`, `design_defect`, `capacity_snapshot`, `pool_exhausted`, and `spend_down_engaged`. `status`, the ledger, and the capacity view are pure folds over this file.

**One fold, not two.** The engine never mutates run state directly: it appends an event and folds it back in through the same function `resume` and `status` use to rebuild state from the file, and it applies the event *as it will be read back* rather than as constructed. A live run and a replay of its own log are therefore the same computation, not two that agree by inspection. Two things deliberately do not survive replay — a session id and its `resume_next` flag, because both describe a conversation that believed it had left edits in a working tree that a crash has since rolled back (§14 pairs session-resume with tree retention precisely so the two never diverge).

`upstroke resume <run-id>` replays, verifies the run branch HEAD matches the last committed event (mismatch = refuse with an explanation), re-probes agents, re-snapshots capacity, and continues — parked questions intact. Git and the log cannot be updated atomically, so schema 3 makes the successful settlement itself carry the exact prepared identity: captured full run-branch ref, parent and tree feed hook-free `commit-tree`; the resulting commit, message, and deterministic private pin are verified before `attempt_finished` is appended. Publication compare-and-swaps the **recorded full branch ref**, never mutable symbolic `HEAD`, from the recorded parent to that commit, removes the pin with a non-dereferencing compare-and-swap, and then appends `task_committed`. Resume accepts only the resulting exact crash prefixes: parent plus matching pin means publish that object; commit plus matching pin means remove the pin; commit with the pin already gone means append the missing `task_committed`. A pin without a successful settlement is orphan residue and is removed without dereferencing symbolic refs. Any substituted or symbolic pin, third branch SHA, changed branch identity, or mismatched commit object refuses while preserving evidence. Schema-1/2 success has no prepared identity, so it is **never** adopted from parent plus subject alone; even a matching message can name an arbitrary tree. It also refuses when the frozen plan's digest moved, when the recorded chain structure no longer matches (a rung is an index into that chain), when the branch is gone, and when another process owns either the run or its physical worktree.

v0.2 extends that shipped exact-identity rule into two candidate/merge transactions. After fixing the verified tree, the engine creates and temporarily pins an immutable commit object; `candidate_prepared` is the sole successful settlement for that candidate-producing attempt and records exactly one complete attempt/base/commit/tree identity before the authoritative candidate ref moves, so resume adopts only that exact shape. Recovery then appends the missing `task_candidate_created`, whose append position establishes FIFO order. `merge_rejected` similarly embeds the complete frozen repair-task payload and admission state so rejection, task registration, key assignment, `AwaitingRepair`, and either runnable or human-gated repair state are one append rather than a rejection/spawn/question crash window. A human-gated admission's embedded question is itself authoritative for status, notification, and `upstroke answer`; it is not followed by a duplicate `question_raised`. Each `merge_verification_started` has exactly one terminal record: successful evidence lives inside `merge_prepared`, code failure inside `merge_rejected`, and infrastructure/crash outcomes inside unavailable/interrupted events. There is no standalone successful or failed finish event before the state-changing append. `merge_prepared` records disposition, expected integration SHA, proposed SHA, candidate, verification evidence, and repair lineage before `git update-ref` advances the run ref by compare-and-swap. On resume, expected-old means retry that same transition and append `task_merged`, proposed means append the missing `task_merged`, and any third SHA means refuse; `already_present` uses equal expected/proposed SHAs, so the same rule becomes a checked no-op. A proposed commit with no prepared/rejected terminal event is residue and is reverified; a dangling merge-review process is settled as interrupted with unknown spend first. The event schema moves rather than teaching `task_committed` a second meaning. The complete protocol and fault table are §26.

**Gates are taken from the record, not re-derived — and not refused over.** `run_started` records each effective gate in full (name, command, shell, timeout) and a resume rebuilds and runs *those*, exactly as it reads the review plan from the record rather than re-resolving who judges. This is the property a live run already has for free: config is parsed once at pre-flight and gates execute from memory, so a mid-run edit to `upstroke.toml` cannot change what a running task is verified against. Honouring the same snapshot across an interruption is what makes every `task_committed` in one log mean the same thing — and it matters concretely once runs self-host, because the workspace an implementer edits *contains the `upstroke.toml` its own gates come from*. Refusing on a mismatch was the first design and was worse in both directions: it left the weakened-gate case detected but the run dead, and it made a gate edit that the run's own reviewed task legitimately committed permanently unresumable. A config that differs today is a warning naming the difference, not an error; the edit simply applies to the next run. Logs predating the record re-derive and warn, saying whether the recorded gate *names* still match — which is proof rather than suspicion when they do not — and that resume writes what it settled on into its own `run_resumed`, so the next one is an ordinary record-bearing resume rather than a second re-derivation that could land somewhere else. `shell` is recorded because it is half of what a command means (`cmd = "true"` always passes under `sh` and is not a program at all under `cmd.exe`); the portability that argued against pinning it does not exist anyway, since `private_dir` already records an absolute host path. The finding and the withdrawn refusal remedy were recorded on 2026-08-11. An attempt the log ends mid-flight is settled as `attempt_interrupted`: recorded in the ledger with unknown spend, but not counted against the rung's allowance, because nothing judged the code — the same rule §19 applies to an outage.

**Effort and worker bindings are taken from the same run snapshot.** `run_started.effort_policy` records the resolved implementation value at small, mid, and frontier plus the review value, while every chain records each rung's exact agent and model plus whether it was pinned. Every worker and every review pass reads those snapshots, so editing `[routing.effort]`, adding a pin, or installing another CLI between processes cannot change one run's execution identity or standard. A mismatch warns and continues with the record; a changed chain shape refuses because recorded rung indices would no longer mean the same thing. Start a new run to adopt current routing.

Those identity fields require event schema 2. A schema-1 log remains readable by a current binary: its first resume re-derives the missing policy and bindings once with explicit warnings, then records them on `run_resumed`. Before it appends any event whose meaning depends on the new identity, it appends `run_schema_upgraded { from: 1, to: 2 }`. Current replay validates that transition; an old binary does not know the marker and therefore refuses instead of silently continuing a run whose new fields it would ignore. Later resumes are record-bound and do not append a second marker.

The complete-review and atomic sequential-settlement contracts begin at event
schema 3. A schema-2 binary ignores the recorded per-pass timeout and retains
its 60 KiB prompt truncation; it would also ignore the ladder transition now
embedded in a failed `attempt_finished` and could spend the same known failure
again after a crash. Fresh runs therefore write schema 3, and a current binary
resuming a schema-1 or schema-2 run appends a transition to 3 before another
attempt. Older binaries refuse that opening schema or transition instead of
silently applying weaker verification or replay semantics. Every failed
sequential attempt embeds its retry, escalation, deferral, terminal failure, or
parking decision in the same durable settlement. A parking settlement carries
the authoritative question too; it is not followed by separate ladder,
`question_raised`, or `task_parked` events that a crash could strand between.
A declined `question_answered` likewise freezes the contemporaneous
`on_task_failure` decision, so resume can append a missing task settlement
without reinterpreting the human's already-durable answer through edited config.

The v0.2 execution topology consequently begins at event schema 4 because its
task states and transactions change execution meaning. Fresh topology runs write
schema 4 in `run_started`; older binaries reject them before folding. Existing
schema-1 through schema-3 runs finish through the sequential path, including the
review-contract upgrade when needed. No in-flight run appends a 3 → 4 upgrade:
starting a new run is the compatibility boundary for adopting worktrees,
candidates, and the merge queue.
