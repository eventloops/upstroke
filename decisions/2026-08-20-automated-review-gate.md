# 2026-08-20 — the automated review gate

**Verdict.** Changes reach `master` reviewed by frontier models on a fixed schedule:
**a single reviewer on every head, and the three-model panel exactly once per pull
request, on the head intended to merge.** The panel's remit is the one the old S9 final
confirmation held. Delivery is staged in five steps, each useful alone and reversible;
**only stage 1 is authorised by this record.**

The owner's stated end state is that the panel can approve and complete a pull request
without a human in the loop. This record treats that as the destination and writes down
what must be true before each step toward it is safe. It does not authorise that step.

---

## 1. The scheduling rule, and the evidence that earned it

> **Panel exactly once per PR, on the head you intend to merge. Single reviewer for
> every round before that.**

Three measured observations, all from this project, decide this. None is an assumption.

**Every review round before the last one returns `CHANGES_REQUIRED`.** On slice PR4,
all three S9 confirmations and the frontier review returned `CHANGES_REQUIRED`. A round
whose outcome is "send it back" does not need three independent opinions to reach that
outcome; one establishes it just as well, and the two others are spent proving a
conclusion already reached.

**Diversity's value rises with prior review, not with code size.** A single reviewer on
fresh code finds most of what is there. The same reviewer on code already reviewed three
times is re-sampling its own blind spot — it is most likely to miss exactly what it
missed before. Independent seats are therefore worth most where nothing follows them to
catch the miss. That is the merge candidate, and only the merge candidate.

**The window that actually leaked was the one after the last review.** Three of the four
findings the frontier review raised on PR4 were in code **no reviewer had ever seen**,
because repairs landed after the final confirmation. This is the failure that mattered,
and neither more seats nor more rounds addresses it. Reviewing the exact head you intend
to land closes it by construction.

Cost, on PR4's actual shape: 8 rounds × 1 reviewer + 1 panel × 3 = **11 reviewer calls**,
against 24 for panel-every-round. The saving is real but it is not the argument; the
placement is. A cheaper schedule that reviewed the wrong tree would not be worth taking.

### What this is not

It is not a claim that one reviewer is as good as three. It is a claim about **where**
the second and third seats pay for themselves, which is a different question. Slice PR3
measured the other end of that curve: six concurrent lenses returned 196 findings and two
independent skeptics killed 114 of them — a **58% noise rate**. Fan-out has a cost paid
in adjudication, not only in tokens.

## 2. S9 collapses into the gate panel

The old S9 stage reviewed a **pre-commit working tree**. By construction it could never
review what actually merged — which is precisely how PR4 reached its head with three
unreviewed commits. Its remit moves to the gate panel, which reviews a pushed head.

Three properties of S9 are why it worked, and each must survive the move. They are
requirements on the panel, not history:

1. **The full packet, queried and never dumped.** The reviewer receives the slice's
   frozen contract, the 645 KB design packet
   (`upstroke-parallel-design-neutral-v16.json`, sha256
   `02bfed758c72a0ccdd91c11a6aaf9a59683a6e1f914356ab0153ec341a455df6`), `DESIGN.md`, and
   `reviews/FINDINGS.md`. The instruction to read it with `python3 -c` and never `cat` is
   load-bearing: dumping it exhausts the context the review needs.

2. **The reading trap, stated explicitly.** The packet carries fourteen generations of
   superseded history *inline*, and it reads exactly like specification.
   `*_verification_dispositions`, the `v4_`…`v15_*` keys, and
   `finding_dispositions[].rationale` are **history**. `decisions.*`, `invariants`, and
   `transaction_fault_matrix` are **live**. A reviewer that cites dead rationale as
   authority produces confident, well-argued, false findings. This is the single
   highest-value line in the existing S9 prompt and it is carried forward verbatim.

3. **Review the code, not the narrative.** The reviewer gets a snapshot with `.git`
   removed, deliberately, so it cannot anchor on branch state, commit messages, or the
   author's PR body. It reviews the artifact in front of it.

One transitional fact, surfaced by this record's own implementing review
(`REVIEW-LEDGER-MISSING` on PR #22): `reviews/FINDINGS.md` lands with the parallelism slice
(PR #18) and is absent on master-based heads until that merges. A reviewer that finds it
absent notes the absence and proceeds — the union of settled dispositions does not exist yet
at that head, and pretending otherwise would anchor the review on a file from a different
branch.

## 3. Adjudication routes on evidence class, not severity

The rules already exist and are not re-derived here. Severity is a reviewer's opinion
about impact; evidence class is a property of what it attached. Only the second is
mechanical, so only the second decides routing.

| Evidence attached | Route |
|---|---|
| failing test, reproduction, or mutation witness | **blocks** — not debatable |
| cites a **live** packet passage plus `file:line` | adjudicate mechanically: is the key live, and does the code say that |
| neither | ledger as hygiene |

Anything routed to adjudication then takes the **hardening rule** already recorded in
`reviews/FINDINGS.md` (owner-authorised 2026-08-18): a live passage says the behaviour is
**wrong** → defect, repair in-slice. The behaviour satisfies the packet and the finding
proposes something **stronger** → hardening, recorded with an owner and a slice, and not
repaired in the slice that surfaced it.

**Round cap 2, then escalate to the owner.** Non-convergence is the signal, not a state
to grind through. "Every repair round rewrites assertions" is a recorded recurrence class
with two occurrences, and each rewrite is a fresh chance to encode a defect as an
expectation. A third round is more likely to add one than remove one.

## 4. The staged rollout

| Stage | Scope | Human in the loop | Status |
|---|---|---|---|
| 1 | auto-review every PR head change, single reviewer, **comment only** | — | **authorised here** |
| 2 | panel on the merge candidate, structured verdicts, adjudication in code | dispatch | not authorised |
| 3 | panel runs inside the trusted workflow; the workflow attests | — | not authorised |
| 4 | auto-merge for a low-risk class (docs, infra, no `src/` semantics) | — | not authorised |
| 5 | auto-merge for `src/`, once 1–4 have a track record | — | not authorised |

**Stage 1 changes no trust boundary, and it changes no policy either.** MAINTAINING.md
step 6 *already* requires that any code push returns to step 3 and re-reviews the new
head. In practice that never happened — PR4 is the proof. Stage 1 does not add an
obligation; it makes an existing one actually occur. It posts a comment and attests
nothing, so the worst case of a wrong review is a wrong comment.

This is why stage 1 needs no edit to MAINTAINING.md. **Stages 2 onward do**, because they
move who performs and who attests the review, and MAINTAINING.md is the living authority
for that. Per `decisions/README.md`, that edit lands at the time of *that* decision,
citing *that* record — not this one.

## 5. Credential separation, and the limit of what it currently buys

A reviewer is **not sandboxed**. Frontier finding #5 on PR #19 established this against
the earlier assumption: `--mode=plan` is not an enforcement boundary, and the allow-list
grants `python3` and `git`, so reviewed content can induce file writes, credential reads
within the running user, and network calls. **Reviewed content is hostile input.** That
is the premise, and it is measured, not defensive drafting.

The response is a separate identity. `upstroke-review` (uid 1001, home `0700`, reachable
as `ssh upstroke-review@tactusbox`) holds its **own** credentials for all three reviewer
CLIs, verified live. It cannot read `/home/ubuntu/.upstroke-env` or `ubuntu`'s Claude
credentials — verified denied in both directions. It has **no `GH_TOKEN`**, which is the
property that matters most: the account that reads hostile input cannot reach the GitHub
API at all. There is deliberately no `ubuntu` → `upstroke-review` SSH key, so that path
does not exist.

**The isolation is advisory, not enforced, and a future reader must not infer otherwise.**
`/etc/sudoers.d/90-cloud-init-users` grants `ubuntu ALL=(ALL) NOPASSWD:ALL` (verified
2026-08-20). `ubuntu` can therefore `sudo -u upstroke-review` at will, and nothing in the
current configuration prevents it. What the separation buys today is **blast-radius
containment against the reviewer**, not containment of `ubuntu`: a prompt injection that
captures the reviewer process lands in an account with no GitHub token and no sudo. It
does not buy defence against a compromised `ubuntu`, and it never claimed to.

Making it enforced means removing that sudoers grant, which requires a **verified
independent admin path first** or the box locks itself out. The owner has not approved
this, and it is not proposed here.

## 6. The risk auto-merge creates, stated before it is built

Stages 4 and 5 are the reason this record exists rather than a commit message.

Today the chain from "a model approves" to "code is on `master`" is broken in one
specific place: `frontier-review.yml` requires the `UPSTROKE_FRONTIER_REVIEW` evidence
comment to be authored by the **repository owner**. The box's GitHub token *is* the
owner's. So the only thing preventing an agent from landing its own work is that no
automated process posts that comment — a convention, enforced by the deliberate
omission of one `gh api` call in `upstroke-frontier-review`, which prints the text for a
human instead of posting it.

**Auto-merge closes that loop.** Once a process can post the evidence comment, the
attack becomes: get hostile text into a reviewed diff, induce the reviewer to emit
`APPROVE`, and the same machinery that reports the verdict now lands it. The reviewer's
input is attacker-controlled by definition — that is what reviewing a pull request *is*.

Three properties, therefore, before any stage-4 auto-merge is built. Recorded now so
that stage's record is judged against them:

- **The reviewer process must never hold a credential that can attest.** Today it holds
  none at all; that must remain true. The driver posts, the reviewer does not, and the
  reviewer's output is data the driver validates — never a command the driver runs.
- **The verdict must be structurally validated, not pattern-matched.** An `APPROVE`
  extracted by substring search from model prose is forgeable by the prose. Findings
  must arrive as structured output whose shape the driver checks before acting.
- **The low-risk class must be defined by path, mechanically, on the trusted side.**
  "Docs and infra, no `src/` semantics" is only meaningful if the workflow computes it
  from the diff on the default branch. A class the pull request can talk its way into is
  not a class.

A further consequence for stage 3: a self-hosted runner must trigger **only** on
`repository_dispatch` / `workflow_dispatch`, never on `pull_request`. A
`pull_request`-triggered self-hosted runner executes a pull request's own modified
workflow file as `ubuntu`, on the box. The existing `frontier-review.yml` already has
the correct shape — dispatch-triggered, checked out at `github.sha`, never at the PR
head — and stage 3 must not weaken it. The attestation credential stays the App
(id `4574301`), not a PAT: `repository_dispatch` via a fine-grained PAT needs
`Contents: write`, which also grants push, and the App can publish the check without it.

**What the owner-authored evidence comment is, plainly (settled 2026-08-20, before this
record landed).** Asked directly by the other active session: what does "owner-authored"
enforce, given the box's token *is* the owner? As a factor it is ceremony — the dispatch
carries the same identity, so the comment proves possession of the same credential twice.
What is real is the human act of choosing to run the two commands, plus one verified bound:
the PAT cannot rewrite the ruleset it satisfies (the administration endpoints return 403,
probed 2026-08-20). This record therefore treats "owner-authored" as an audit anchor, not a
second factor, and the durable resolution is the review moving inside the trusted path — not
a stronger comment.

## 7. Options rejected

**Panel on every round.** 24 reviewer calls against 11 on PR4's shape, and it does not
address the failure that actually occurred — unreviewed commits after the last review.
It spends the extra seats where prior review has already made them least productive.

**Keep S9 as a separate pre-commit stage alongside a gate panel.** S9 reviews a working
tree, so it cannot review what merges; keeping it means paying for a review of a tree
that is not the merge candidate and then paying again for one that is. Its remit moves
rather than duplicates.

**Enforce the isolation now by removing `ubuntu`'s sudo.** Correct in direction, and it
is what would make §5 an enforcement rather than a convention. Rejected for sequencing
only: without a verified independent admin path, the failure mode is losing the box. It
needs its own record.

**Have the reviewer post its own evidence comment.** This is the shortest path to the
owner's end state and it is the one thing that must not be done first. It hands an
account that reads hostile input the ability to attest. Stages 2–3 exist to move
attestation into the trusted workflow *before* anything automated can attest at all.

## 8. What this record does not decide

- Which three models fill the panel seats, and at what effort.
- The structured verdict schema, and how adjudication is expressed in code.
- Whether the panel's `CHANGES_REQUIRED` may dispatch a repair worker automatically.
- Anything in stages 2–5. Each needs its own record, and stages 4–5 need §6 satisfied.

## 9. Scaffolding and the self-hosting horizon (added before landing, 2026-08-20)

MAINTAINING.md step 5 already says it: "**Until Upstroke owns this supervision natively**…".
The gate this record builds is upstroke's own core loop, performed by hand, and
[2026-08-11 — self-hosting v0.2](2026-08-11-self-hosting-v02.md) commits the project to
closing that loop through the engine. This record's machinery therefore divides in two, and
every stage-2+ proposal takes one test first: **would this code be deleted the day upstroke
self-hosts the review? Then it is scaffolding — build the dumbest interim that works, or
build it in the engine instead.**

- **Boundary — permanent, invest properly.** The GitHub trust plane: the ruleset, the
  App-owned check, dispatch-only privileged workflows, the validators, credential scoping,
  and the invalidation semantics in
  [2026-08-20 — what invalidates a frontier review](2026-08-20-review-invalidation-scope.md).
  Whoever performs a review — a human, the box, or the engine — these decide what GitHub
  will believe about it.
- **Scaffolding — thin by intent, deleted without ceremony.** The box-side orchestration:
  the stage-1 driver, its prompts, its verdict plumbing. Kept deliberately dumb; nothing in
  it may grow adjudication logic.
- **Consequently:** stage 2's panel fan-out, structured multi-verdict adjudication,
  evidence-class routing in code, and round caps are **engine work** (§15 council ledgers;
  the review plan read from the record). The interim panel is three manual CLI invocations
  on the merge candidate, read by a human. Stage 3's relocation of the reviewer is likewise
  deferred — the engine *is* the relocation — and stages 4–5 are engine-era questions, not
  to be built as box machinery at all.
- One convention adopted now because the other session demonstrated its necessity: **a
  slice's frozen contract imports the current FINDINGS.md rulings at freeze time.** Pointing
  reviewers at the ledger was proven insufficient (a reviewer read it and re-derived anyway,
  defensibly); only the contract changed behaviour. A generator script can follow if doing
  it by hand ever hurts.

## Cross-references

- `reviews/FINDINGS.md` — the hardening rule (owner-authorised 2026-08-18), the
  authority rule, and the boundary rule this record's §3 routes into.
- `MAINTAINING.md` steps 5–8 — the review and attestation obligation stage 1 automates
  and stages 2–3 will amend.
- `.github/workflows/frontier-review.yml` — the trusted attestation path whose shape
  stage 3 must preserve.
- [2026-08-12 — v0.2 merge queue and execution topology](2026-08-12-merge-queue-execution-topology.md)
  — the shared worktree and runner boundary the review driver runs inside.
- [2026-08-11 — self-hosting v0.2](2026-08-11-self-hosting-v02.md) — the horizon §9
  divides this record against.
- [2026-08-20 — what invalidates a frontier review](2026-08-20-review-invalidation-scope.md)
  — the invalidation semantics §9 classes as boundary; written the same day.

## 2026-09-01 — the every-head cadence is narrowed

The scheduling rule's first half — a single reviewer on every head — was
narrowed on 2026-09-01: one full pass per pull request remains the norm, a
serious P1 repair returns for a fresh pass, and a non-serious repair-only
delta may instead be owner-verified and disclosed in the pull-request body.
The post-review repair commits §1 measured are exactly the deltas the
narrowing governs. §3's adjudication routing is untouched: a finding
carrying a failing test, reproduction, or mutation witness still blocks,
whatever its severity — the narrowing governs how many passes run, never
what evidence blocks. §5's conditions on any automated return, and the
panel cadence — exactly once, on the merge candidate, every seat re-run on
any head movement (`2026-08-31-panel-seats.md`) — are untouched. See
[2026-09-01 — review effort is re-scoped](2026-09-01-review-effort-rescoped.md).
