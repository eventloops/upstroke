# Maintaining upstroke

This document is the operating contract for changes to the protected default branch, `master`.
Repository rules enforce the mechanical parts; maintainers are responsible for the semantic review
evidence. The contract applies to source, documentation, workflows, release machinery, and the
contract itself.

## Mainline sequence

1. Branch from current `master` and keep the change to one coherent, independently revertible
   outcome.
2. Push the branch and open a **draft pull request early**. Use a Conventional Commit title:
   `type(optional-scope): summary`.
3. Let the inexpensive deterministic gates finish first:
   - `upstroke-pr-policy` gives fast candidate-controlled feedback on the PR title and evidence ledger;
     it is not a trusted merge boundary because a pull request can edit both workflow and validator.
   - `upstroke-ci` aggregates formatting, Clippy, and the Windows, Linux, and macOS test matrix.
4. If the branch is behind `master`, update it and wait for both gates again.
5. Only after both gates are green, give the exact current diff and head SHA to an independent
   frontier-class reviewer at `max` effort. AI-assisted implementation should use a frontier-class
   implementation model at `xhigh` effort or higher. Record the implementation and review model,
   effort, head SHA, transport, wall-clock limit, and durable review link in the PR. Until Upstroke
   owns this supervision natively, allow at least 90 minutes **per frontier review pass** and use
   the review CLI's streaming output. A timeout, transport failure, or missing verdict never passes.
6. Triage every finding against the serious-P1 bar defined under "Finding triage and cleanup
   points". A serious P1 must be fixed, and the repaired head returns to step 3 for a fresh pass.
   Every other finding is fixed at the author's discretion or accepted as logged baggage: a ledger
   row with a stable id, an honest failure sequence, and disposition `accepted-risk` or `deferred`.
   Accepted findings block nothing and oblige no further pass. A push that repairs reviewed
   findings means the recorded pass no longer binds to the head — never claim it does — but the
   merge may proceed once the owner has read the repair delta (`git diff <reviewed> <head>`)
   against the findings and disclosed that verification in the body. One path is exempt from even
   that, decided in `decisions/2026-08-20-review-invalidation-scope.md`: a push whose entire diff
   from the reviewed head lies inside `reviews/FINDINGS.md` — record both SHAs and confirm the
   exempt-only diff with `git diff --stat <reviewed> <head>`. Feature ideas discovered during
   review belong in the design or a follow-up. Re-scoped in
   `decisions/2026-09-01-review-effort-rescoped.md`.
7. Once a review passes, record it in the PR body's Review evidence section: implementation and
   reviewer models and effort, the full reviewed head SHA, transport and wall-clock limit, and a
   durable link to the verdict. Re-run `.github/scripts/validate-pr-body.sh` from the default
   branch against the live title and body — `upstroke-pr-policy` ran the candidate's copy. Editing
   the title or body afterwards changes what was reviewed: re-check the body, and if the ledger
   changed in substance, review again. When the merged head differs from the reviewed head, the
   evidence section lists the delta commits and states what verified each: a fresh pass, the
   owner's diff-read against the findings, or an explicit owner waiver citing its authorizing
   record.
8. Resolve every conversation, mark the PR ready, and merge with a merge commit. The merge is the
   owner's attestation that the review recorded in the PR is real and that the reviewed SHA is the
   head being merged. Do not push or force-push directly to `master`. Delete the source branch
   after merge.

Slices of a long-running design land as pull requests **into** their integration branch
(today `codex/parallelism-design`): they receive `upstroke-ci`, `upstroke-pr-policy`, and a
single-reviewer frontier review of each head. The integration branch's own pull request into
`master` is reviewed once more, on the head that merges, after its last update from `master`.
Merge commits only on and into the integration branch — a rewrite orphans every ledger row bound
to a replaced SHA. Decided in `decisions/2026-08-21-stacked-slice-prs.md`.

There is no machine-minted review check. The App-signed `upstroke-frontier-review` check, the
attestation and invalidation workflows that produced it, and the `frontier-check-signer`
environment were retired in `decisions/2026-08-23-retire-app-attestation.md`, which also states
what must be true before any automated attestation returns. The owner's merge is the attestation,
and the review obligation above is unchanged by the loss of the machinery that recorded it.

`upstroke-pr-policy` is a `pull_request` workflow so contributors get immediate, unprivileged
feedback from the candidate they are editing. Its result is candidate-controlled: a pull request
can edit the workflow and the validator it runs, so both are part of the diff the review and the
owner judge, and the owner re-runs the default branch's `validate-pr-body.sh` before merging.

### Review finding ledger

Do not let review findings disappear into a sequence of force-pushed fixes. Give every actionable
finding a stable id and retain one ledger row in the pull request with:

- severity, exact reviewed SHA and file/line, plus a concrete failure sequence;
- provenance: `pre_existing`, `introduced_by_feature`, `fix_regression`, or `undetermined`;
- category: `correctness`, `crash-consistency`, `security-trust`, `portability`, `liveness`,
  `performance`, `compatibility`, or `docs-contract`;
- first-bad commit where history can establish it, and any earlier finding id when it recurs; and
- disposition plus prevention: the named regression test, or an explicit explanation of why a
  deterministic test is infeasible and the documented guard/pitfall that prevents false claims.

Provenance explains where a defect came from; it does not make a defect less real. Fix bounded
pre-existing defects exposed by the changed path, or accept them with a logged row — the
serious-P1 bar decides which, and a serious P1 is never accepted. A genuinely architectural or unrelated defect
may move to a critical follow-up only when the current PR documents the limitation honestly and
does not claim the missing guarantee. Every code defect fixed in the PR gets a regression test
that fails on the first-bad shape. If the same failure is found again, link the old id and treat the
missing or inadequate regression as a process defect too. Keep fixed and rejected rows in the
ledger: the point is to preserve why a change was accepted, rejected, or deferred, not merely to
list what remains open.

The exact table header and canonical tokens in the pull-request template are machine-enforced by
`.github/scripts/validate-pr-body.sh`. Every non-empty row binds the finding to a full reviewed SHA
and `file:line`, includes an explicit `A -> B -> failure` sequence, and uses one disposition:
`fixed`, `rejected`, `deferred`, or `accepted-risk`. Update the validator and its rejection fixtures
in the same pull request before changing this schema; aliases and free-form categories fail closed.

`.github/scripts/validate-pr-ledger-evidence.sh` then resolves those claims against the exact PR
head. A reviewed SHA must be an ancestor of that head, the named path and line must exist at the
reviewed commit, and every backticked regression or guard identifier must occur in tracked exact-head
content. Never cite an isolated worktree/lane commit that was later cherry-picked under another
identity: bind the row to the first integrated PR commit and record the pre-commit lane in
`First bad / prior ID` if it matters. When a regression is renamed or consolidated, update the live
ledger to its current tracked name; do not preserve a stale claim by adding a no-op alias test. An
explicit deterministic invariant may remain prose rather than a backticked identifier, but its
wording must say what enforces it.

### Finding triage and cleanup points

A finding is a **serious P1** when its failure sequence is concrete on the current head and
reaches at least one of:

- a `DESIGN.md` §4 invariant;
- the trust boundary, the merge or release machinery, or any gate change that misstates what the
  gate enforces (`security-trust`);
- durable state: the event log, replay, or anything that makes a recorded run unreproducible or
  corrupt;
- loss or corruption of data in a user repository — the engine owns git;
- a legal or licensing defect.

The severity label a reviewer assigns does not decide this; the owner classifies, and a P1 whose
failure needs speculative preconditions is reclassified down with a ledger row saying why.

Everything below the bar is baggage the project deliberately carries: rows stay in their PR
ledgers, and findings that outlive their PR belong in `reviews/FINDINGS.md`. Baggage is swept,
not forgotten, at three designated points: before any release tag or crates.io publish, where
every open `accepted-risk` and `deferred` row is re-triaged and the release notes name what
ships open; at each integration checkpoint merge (`decisions/2026-08-25-checkpoint-merges.md`);
and at owner-called sweeps. A sweep fixes a row, re-accepts it dated, or converts it to a
tracked follow-up; a row re-accepted twice carries the owner's dated note saying why it stays.
Decided in `decisions/2026-09-01-review-effort-rescoped.md`.

## Enforced repository rules

The default-branch ruleset must:

- require a pull request and an up-to-date branch;
- require `upstroke-ci` and `upstroke-pr-policy` on the current head;
- require all review conversations to be resolved;
- allow merge commits only;
- block branch deletion and non-fast-forward updates; and
- have no direct-push bypass.

A separate tag ruleset targeting `refs/tags/v*` must block both updates and deletions with no
bypass. This makes the release tags described below genuinely immutable.

### Trust boundary

The current repository has one trusted same-repository writer: its owner. GitHub identifies every
workflow using `GITHUB_TOKEN` as the same GitHub Actions app, so a pull request — from a fork or a
branch — can edit `ci.yml`, `pr-policy.yml`, or the validators they run and still produce green
`upstroke-ci` and `upstroke-pr-policy` contexts. Those checks are required for feedback and for
catching honest mistakes; they are not the security boundary. The boundary is that only the owner
merges, after an independent review of the exact head, and that the diff the owner reviews includes
any change to the gates themselves.

That is the boundary the retired App check enforced in practice. Its attestation workflow required
the evidence comment to be owner-authored, and the build box and every agent session act on the
owner's token — so what kept an agent from landing its own work was never the App but the
convention that no automated process posts the comment or sends the dispatch. The convention is
now that no automated process merges. `decisions/2026-08-20-automated-review-gate.md` §5 records
why that must hold and what must change before it may be relaxed: a reviewer must never hold a
credential that can attest, verdicts must be structurally validated, and any low-risk class must be
computed on the trusted side. A return of automated attestation starts from that record and a new
one of its own, with a signer distinct from the owner's token — a GitHub App, not a PAT.

Keep approval for workflow runs from **all** external contributors as the fork safeguard, and keep
the release immutability setting. Bootstrap and audit both through the API:

```bash
gh api --method PUT \
  repos/eventloops/upstroke/actions/permissions/fork-pr-contributor-approval \
  -H 'X-GitHub-Api-Version: 2026-03-10' \
  -f approval_policy=all_external_contributors

gh api --method PUT repos/eventloops/upstroke/immutable-releases \
  -H 'X-GitHub-Api-Version: 2026-03-10'

gh variable set UPSTROKE_IMMUTABLE_RELEASES_REQUIRED \
  --repo eventloops/upstroke --body true

gh api repos/eventloops/upstroke/actions/permissions/fork-pr-contributor-approval \
  -H 'X-GitHub-Api-Version: 2026-03-10'
gh api repos/eventloops/upstroke/immutable-releases \
  -H 'X-GitHub-Api-Version: 2026-03-10'
gh variable get UPSTROKE_IMMUTABLE_RELEASES_REQUIRED \
  --repo eventloops/upstroke
```

The rules prevent accidental direct merges, merging a branch that is behind `master`, and merging
with unresolved conversations. They do not defend against compromise or dishonesty of the owner
account, and they do not make a green `upstroke-ci` on a fork pull request mean anything on its
own. Keep an explicit inventory: the owner is the only same-repository writer, and no secret,
token, or App can mint a merge-gating check.

External-fork support is provisional. A fork pull request's checks are candidate-controlled, so
its entire diff — workflow and validator edits included — is reviewed before merge, and the
external-contributor approval setting above keeps its workflows from running unreviewed.

Do not grant another account same-repository write access without a focused review of this trust
model: a second writer could merge, and nothing mechanical distinguishes their merge from the
owner's.

Because this is currently a solo-maintainer repository, required approving reviews remain zero:
GitHub does not allow a PR author to approve their own PR. The independent frontier model performs
the semantic review, and the owner's merge is its attestation. If pull requests come to be opened
by a machine account, `require_code_owner_review` with the owner as code owner and
`dismiss_stale_reviews_on_push` restore a machine-enforced, exact-head owner sign-off with no
credential to manage.

The current rulesets have no bypass actor. If a future reviewed change adds an emergency bypass, it
must be **pull-request-only** and limited to an Actions outage or a broken rule that prevents its own
repair PR. It must never permit a direct push. Explain the bypass and recovery in the PR before
merging; ordinary urgency is not a reason to use it.

Required-check and required-environment names are API contracts. To rename one, first land the
replacement, observe it on a PR, update the ruleset, and only then remove the old requirement in a
later PR.

## Release contract

Release tags use `v*` and are made immutable by the tag ruleset. Repository release immutability is
a separate mandatory setting: tag protection fixes the commit identity, while release immutability
prevents published binaries from being replaced under that tag. GitHub applies the setting only to
future releases, so enable and read it back before creating another tag. The release job's token
cannot read that administration-scoped setting, so `UPSTROKE_IMMUTABLE_RELEASES_REQUIRED=true`
records the completed owner readback and makes an incomplete bootstrap fail closed. It is not proof
against a dishonest owner changing both values; the post-publication signed immutable-release and
asset checks remain authoritative. Re-read the live setting before every release and in the
periodic governance drift audit.

The release workflow independently verifies that the tagged commit is reachable from
`origin/master`, that the tag matches `Cargo.toml`, and that the release gates and platform builds
pass before publishing. It refuses an existing mutable or incomplete release, discards only an
unpublished same-tag draft created by `github-actions[bot]` after a failed attempt, refuses to
delete any other same-tag draft, and skips rather than overwrites a complete immutable release. New
uploads must contain the exact three expected assets; the workflow verifies GitHub's signed release
attestation and each local archive against its attested digest. The GitHub release is created before
the irreversible crates.io publish. Create releases from an already merged mainline commit; branch
protection does not make arbitrary tags safe by itself.

Release `v0.1.0` predates repository release immutability and remains the sole legacy exception. Do
not rerun, replace, or delete its assets. Its preserved GitHub asset digests are:

- `upstroke-aarch64-apple-darwin.tar.gz`: `sha256:552302e348273143665d2604130e6c1487647a90b496a8d8f789d30839175289`
- `upstroke-x86_64-pc-windows-msvc.zip`: `sha256:e88206643c07ac5cee418ed27ddbbb7e6bcffc1835e727a68bbd716f876c8871`
- `upstroke-x86_64-unknown-linux-gnu.tar.gz`: `sha256:94447cfd56d0d8ba5eae1ec391c2564a7ddba2fceb15cee35ce537a0ba00d798`

## High-blast-radius changes

Changes to event or replay schemas, Git/ref handling, agent permissions, `upstroke.toml`, `DESIGN.md`,
CI, release, or rules deserve an especially narrow PR and a focused fresh-context review. A PR can
edit ordinary Actions workflows, so the independent review is the trust boundary for changes to the
gates themselves, and the owner reads those diffs line by line before merging.
