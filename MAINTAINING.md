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
6. Fix every finding. Any code push creates a new head SHA, so return to step 3 and review the new
   head. Feature ideas discovered during review belong in the design or a follow-up unless they are
   required for the current change to be correct. One exception, decided in
   `decisions/2026-08-20-review-invalidation-scope.md`: a push whose entire diff from the reviewed
   head lies inside the exempt path set (exactly `reviews/FINDINGS.md`) does not invalidate the
   review — re-send the step 7 dispatch with the original reviewed SHA, and the trusted workflow
   itself verifies ancestry and the exempt-only diff before attesting the current head, recording
   both SHAs on the published check. Everything else invalidates, deliberately.
7. Once a review passes, post a dedicated evidence comment containing exactly
   `UPSTROKE_FRONTIER_REVIEW: 1`, `VERDICT: PASS`, and `REVIEWED_SHA: <full SHA>` on separate lines,
   with no other text. Send the `frontier-review` repository dispatch with the PR number, full
   reviewed head SHA, and evidence URL. The default-branch workflow refuses ambiguous or behind
   evidence — and stale evidence, unless the reviewed SHA is an ancestor of the current head with
   an exempt-only diff (step 6) — runs the default branch's canonical PR-body validator over the
   live title/body,
   validates the evidence comment, reruns formatting, Clippy, and
   all three platform test jobs from its trusted default-branch definition, then uses the dedicated
   `Upstroke Frontier Review Gate` GitHub App to publish a successful `upstroke-frontier-review` check
   on the exact head. Editing the PR title or body after that success causes the trusted
   invalidator to turn the same App-owned check into a failure; repeat the frontier review and
   attestation against the current metadata rather than treating an unchanged commit SHA as
   unchanged review scope.
8. Resolve every conversation, mark the PR ready, and merge with a merge commit. Do not push or
   force-push directly to `master`. Delete the source branch after merge.

Slices of a long-running design land as pull requests **into** their integration branch
(today `codex/parallelism-design`): they receive `upstroke-ci`, `upstroke-pr-policy`, and a
single-reviewer frontier review of each head, but **no attestation**. The App check is minted
only for pull requests into `master`; the integration branch's own pull request is attested
exactly once, on the head that merges, after its last update from `master`. Merge commits only
on and into the integration branch — a rewrite orphans every ledger row bound to a replaced
SHA. Decided in `decisions/2026-08-21-stacked-slice-prs.md`.

The attestation workflow records a review; it does not perform one. The App-owned check is the hard
merge gate. The ruleset binds the check name to App id `4574301`, while the App credential is kept
out of every pull-request workflow. A same-named GitHub Actions check therefore cannot satisfy the
rule. The repository owner remains responsible for the truth of the linked semantic review;
dispatching without a real passing review is a policy violation.

`upstroke-pr-policy` deliberately remains a `pull_request` workflow so contributors get immediate,
unprivileged feedback from the candidate they are editing. Its result is not trusted: the
default-branch `repository_dispatch` workflow fetches the live title/body and runs its own canonical
`validate-pr-body.sh` immediately after dispatch validation and again just before App-token minting.
The separate `pull_request_target` invalidator is deliberately metadata-only: GitHub loads it from
the default branch, it checks out only `github.sha` from that trusted base, and it never loads or
executes the pull request. On a title/body edit it uses the dedicated App to PATCH every successful
`upstroke-frontier-review` check from that App on the unchanged head to `failure`. The signing workflow
also re-reads metadata after publication and applies the same invalidation if an edit races the
final API call. Do not add candidate checkout, PR-authored scripts, dependency installation, or
any command derived from PR content to this privileged invalidator.

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
pre-existing defects exposed by the changed path. A genuinely architectural or unrelated defect
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

### App-gate migration record

The original `frontier-reviewed` deployment gate failed its stale-SHA canary: GitHub reported a
pull request mergeable after its head changed even though the deployment existed only on the
previous SHA. The replacement was introduced without an unprotected interval: the dedicated App
first emitted `upstroke-frontier-review` alongside the old gate, the ruleset then required that check
and bound it to App id `4574301`, and a no-tree-change canary proved that the old App check did not
follow a new commit identity. Only after that proof were the deployment requirement, writer, and
`deployments: write` permission retired. The App-owned exact-head check is now the sole semantic
merge gate; do not reintroduce the deployment as an attestation mechanism.

Example dispatch after a passing review:

```bash
pr=123
reviewed_sha="$(gh pr view "$pr" --json headRefOid --jq .headRefOid)"
# Give this exact full SHA and its diff to the reviewer. After it passes, post
# one machine record as the entire evidence comment (put prose in another comment):
evidence_body="$(printf 'UPSTROKE_FRONTIER_REVIEW: 1\nVERDICT: PASS\nREVIEWED_SHA: %s' \
  "$reviewed_sha")"
review_url="$(gh api --method POST "repos/eventloops/upstroke/issues/$pr/comments" \
  -f "body=$evidence_body" --jq .html_url)"
gh api --method POST repos/eventloops/upstroke/dispatches \
  -f event_type=frontier-review \
  -F "client_payload[pull_request]=$pr" \
  -f "client_payload[reviewed_sha]=$reviewed_sha" \
  -f "client_payload[review_url]=$review_url"
```

## Enforced repository rules

The default-branch ruleset must:

- require a pull request and an up-to-date branch;
- require `upstroke-ci` and `upstroke-pr-policy` on the current head;
- require `upstroke-frontier-review` on the current head, bound to GitHub App id `4574301`;
- require all review conversations to be resolved;
- allow merge commits only;
- block branch deletion and non-fast-forward updates; and
- have no direct-push bypass.

A separate tag ruleset targeting `refs/tags/v*` must block both updates and deletions with no
bypass. This makes the release tags described below genuinely immutable.

### Trust boundary

The current repository has one trusted same-repository writer: its owner. GitHub identifies every
workflow using `GITHUB_TOKEN` as the same GitHub Actions app. A fork can therefore mint successful
jobs named `upstroke-ci`, `upstroke-pr-policy`, or anything else without receiving a write token. Those
fast checks are required for feedback but are not the security boundary. The default-branch
attestation workflow independently reruns their substance with no PR-controlled workflow code,
then requests a repository-scoped installation token for the dedicated App. The App publishes only
the final exact-SHA check. The ruleset's App binding, rather than the check name alone, is the
security boundary.

The App is private, installed only on `eventloops/upstroke`, and has metadata read plus checks and
commit-statuses write. GitHub requires the latter at installation level to make the App eligible as
an expected required-check source; the workflow never requests it in its token. The private key is
an environment secret, never a repository secret. Only the final trusted `repository_dispatch` job
and the metadata-only default-branch `pull_request_target` invalidator declare
`frontier-check-signer`; that environment's sole custom branch policy is the exact `master` branch.
Their short-lived installation tokens explicitly request only `checks: write` and are revoked by
the token action at job completion. Neither job has deployment-write authority; the App-owned check
is their only merge-gate output. Keep approval for workflow runs from **all** external contributors
as a second, independent fork safeguard.

Bootstrap and audit the external configuration through the API. Supply the downloaded PEM on stdin
so it is never written into a command line, log, tracked file, or pull-request workflow:

```bash
gh api --method PUT repos/eventloops/upstroke/environments/frontier-check-signer \
  -F wait_timer=0 \
  -F 'deployment_branch_policy[protected_branches]=false' \
  -F 'deployment_branch_policy[custom_branch_policies]=true'
gh api --method POST \
  repos/eventloops/upstroke/environments/frontier-check-signer/deployment-branch-policies \
  -f name=master -f type=branch

gh variable set UPSTROKE_FRONTIER_APP_ID --env frontier-check-signer \
  --repo eventloops/upstroke --body 4574301
gh variable set UPSTROKE_FRONTIER_APP_CLIENT_ID --env frontier-check-signer \
  --repo eventloops/upstroke --body Iv23liSwpgxIDc8SN4ED
gh secret set UPSTROKE_FRONTIER_APP_PRIVATE_KEY --env frontier-check-signer \
  --repo eventloops/upstroke < upstroke-frontier-review-gate.private-key.pem

gh api --method PUT \
  repos/eventloops/upstroke/actions/permissions/fork-pr-contributor-approval \
  -H 'X-GitHub-Api-Version: 2026-03-10' \
  -f approval_policy=all_external_contributors

gh api --method PUT repos/eventloops/upstroke/immutable-releases \
  -H 'X-GitHub-Api-Version: 2026-03-10'

gh variable set UPSTROKE_IMMUTABLE_RELEASES_REQUIRED \
  --repo eventloops/upstroke --body true

gh api repos/eventloops/upstroke/environments/frontier-check-signer
gh api \
  repos/eventloops/upstroke/environments/frontier-check-signer/deployment-branch-policies
gh variable list --env frontier-check-signer --repo eventloops/upstroke
gh secret list --env frontier-check-signer --repo eventloops/upstroke
gh api repos/eventloops/upstroke/actions/permissions/fork-pr-contributor-approval \
  -H 'X-GitHub-Api-Version: 2026-03-10'
gh api repos/eventloops/upstroke/immutable-releases \
  -H 'X-GitHub-Api-Version: 2026-03-10'
gh variable get UPSTROKE_IMMUTABLE_RELEASES_REQUIRED \
  --repo eventloops/upstroke
```

After `gh secret list` confirms the signing-secret name, revoke every active App key whose PEM is
not in the maintained inventory. Only then add Commit statuses read/write to the App registration
and approve the updated installation. Changing App permissions affects every active private key,
so granting it first would broaden an unaccounted credential. Keep the workflow's generated token
down-scoped to `permission-checks: write`; it must not request `permission-statuses`.

Never commit, paste, or print the PEM. Delete the downloaded copy only after the environment secret
has been stored and independently exercised successfully.

The rules prevent accidental direct merges, stale-SHA evidence, and a post-attestation title/body
edit retaining success, and prevent same-name fork-check spoofing from satisfying the semantic
gate. They do not defend against compromise or dishonesty of
the owner account or theft of the App private key. Keep an explicit inventory: the owner is the only
same-repository writer; App id `4574301` is the sole integration trusted for the semantic check; and
no other secret, token, or App may mint that check without a reviewed trust-model change. The
evidence link also remains an owner attestation rather than something GitHub can semantically judge.

External-fork support is provisional until a canary from a separately owned fork proves GitHub's
check-run behavior. The canary must verify that the trusted workflow can check out the fork commit,
that the App can publish its check on the fork-only head SHA, that the App-bound rule accepts only
that SHA, and that pushing a new head invalidates it. Do not merge or advertise external-fork
contributions before that canary passes. If GitHub cannot attach the check to a fork-only SHA, keep
external forks unsupported rather than weakening the App binding.

Do not grant another account same-repository write access without a focused review of this trust
model. The dedicated App prevents another writer's ordinary Actions workflow from impersonating
the semantic gate, but that writer could still propose changes to the trusted default-branch
workflow and external configuration.

Because this is currently a solo-maintainer repository, required approving reviews remain zero:
GitHub does not allow a PR author to approve their own PR. The independent frontier model performs
the semantic review, and the App-owned check is its enforced owner attestation.

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
CI, release, rules, or frontier-review attestation deserve an especially narrow PR and a focused
fresh-context review. A PR can edit ordinary Actions workflows, so the independent review is the
human trust boundary for changes to the gates themselves; the dedicated App remains the machine
identity that enforces the resulting exact-SHA verdict.
