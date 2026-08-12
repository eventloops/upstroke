# Maintaining tactus

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
   - `tactus-pr-policy` checks the PR title format and that the required evidence headings exist.
   - `tactus-ci` aggregates formatting, Clippy, and the Windows, Linux, and macOS test matrix.
4. If the branch is behind `master`, update it and wait for both gates again.
5. Only after both gates are green, give the exact current diff and head SHA to an independent
   frontier-class reviewer at `max` effort. AI-assisted implementation should use a frontier-class
   implementation model at `xhigh` effort or higher. Record the implementation and review model,
   effort, head SHA, transport, wall-clock limit, and durable review link in the PR. Until Tactus
   owns this supervision natively, allow at least 90 minutes **per frontier review pass** and use
   the review CLI's streaming output. A timeout, transport failure, or missing verdict never passes.
6. Fix every finding. Any code push creates a new head SHA, so return to step 3 and review the new
   head. Feature ideas discovered during review belong in the design or a follow-up unless they are
   required for the current change to be correct.
7. Once a review passes, post its evidence on the PR and send the `frontier-review` repository
   dispatch with the PR number, full reviewed head SHA, and evidence URL. The default-branch
   workflow refuses stale or behind evidence, validates the PR metadata and evidence comment,
   reruns formatting, Clippy, and all three platform test jobs from its trusted default-branch
   definition, then records a successful `frontier-reviewed` deployment on the exact head.
8. Resolve every conversation, mark the PR ready, and merge with a merge commit. Do not push or
   force-push directly to `master`. Delete the source branch after merge.

The attestation workflow records a review; it does not perform one. The successful deployment is
the hard merge gate: unlike an Actions check name, an untrusted fork cannot mint it with a
read-only token. The repository owner remains responsible for the truth of the linked semantic
review. Dispatching without a real passing review is a policy violation.

### Bootstrap exception

The process cannot require an attestation whose workflow is not on `master` yet. This one bootstrap
PR is therefore merged only after its deterministic gates and a manually recorded independent
frontier review pass. After merge, first configure the `frontier-reviewed` environment and
all-external-contributor workflow approval, then use the licence-correction PR as the same-repo
canary. Run its gates and review, dispatch the attestation, verify the reviewed deployment, create
the branch ruleset disabled and inspect it, then activate it before merging the canary. Create and
activate the immutable release-tag ruleset before any release. This exception ends once those
rules are active; it is not a reusable bypass.

Example dispatch after a passing review:

```bash
pr=123
reviewed_sha="$(gh pr view "$pr" --json headRefOid --jq .headRefOid)"
# Give this exact full SHA and its diff to the reviewer. After it passes:
gh api --method POST repos/keybindings/tactus/dispatches \
  -f event_type=frontier-review \
  -F "client_payload[pull_request]=$pr" \
  -f "client_payload[reviewed_sha]=$reviewed_sha" \
  -f "client_payload[review_url]=https://github.com/keybindings/tactus/pull/$pr#issuecomment-0000000000"
```

## Enforced repository rules

The default-branch ruleset must:

- require a pull request and an up-to-date branch;
- require `tactus-ci` and `tactus-pr-policy` on the current head;
- require a successful deployment to the `frontier-reviewed` environment on that head;
- require all review conversations to be resolved;
- allow merge commits only;
- block branch deletion and non-fast-forward updates; and
- have no direct-push bypass.

A separate tag ruleset targeting `refs/tags/v*` must block both updates and deletions with no
bypass. This makes the release tags described below genuinely immutable.

### Trust boundary

The current repository has one trusted same-repository writer: its owner. GitHub identifies every
workflow using `GITHUB_TOKEN` as the same GitHub Actions app. A fork can therefore mint successful
jobs named `tactus-ci`, `tactus-pr-policy`, or anything else without receiving a write token. Those
fast checks are required for feedback but are not the security boundary. The default-branch
attestation workflow independently reruns their substance with no PR-controlled workflow code,
then uses its `deployments: write` token to record the exact reviewed SHA. Fork tokens are
read-only; jobs that reference the protected environment directly require owner approval.

Configure the `frontier-reviewed` environment with `keybindings` (user id `38257252`) as its
required reviewer and `prevent_self_review = false`. Also require approval for workflow runs from
**all** external contributors. The environment review gates PR-authored jobs that declare this
environment; it does not mediate Deployments API calls or substitute for inspecting fork workflow
changes. The legitimate attestation calls the Deployments API directly from the trusted
default-branch workflow, so it does **not** create an environment approval request. Normally deny
any `frontier-reviewed` approval request shown by Actions: it came from a job that declared the
environment, not from the attestation path. These two settings are part of the gate and must not be
weakened independently of a reviewed repository change.

Bootstrap those settings through the versioned API, then read them back before opening the canary:

```bash
gh api --method PUT repos/keybindings/tactus/environments/frontier-reviewed \
  -H 'X-GitHub-Api-Version: 2026-03-10' --input - <<'JSON'
{"prevent_self_review":false,"reviewers":[{"type":"User","id":38257252}],"deployment_branch_policy":null}
JSON

gh api --method PUT \
  repos/keybindings/tactus/actions/permissions/fork-pr-contributor-approval \
  -H 'X-GitHub-Api-Version: 2026-03-10' \
  -f approval_policy=all_external_contributors

gh api repos/keybindings/tactus/environments/frontier-reviewed \
  -H 'X-GitHub-Api-Version: 2026-03-10'
gh api repos/keybindings/tactus/actions/permissions/fork-pr-contributor-approval \
  -H 'X-GitHub-Api-Version: 2026-03-10'
```

The rules prevent accidental direct merges and stale-SHA evidence, and prevent same-name fork-check
spoofing from satisfying the overall merge gate. They do not defend against compromise or
dishonesty of the owner account. Any same-repository workflow, fine-grained token, or GitHub App
with deployment-write authority can mint the gate, so keep an explicit inventory: the trusted
`frontier-review.yml` job is the only workflow granted `deployments: write`; the owner is the only
same-repository writer; and no token or installed App may have deployment-write authority for this
repository without a reviewed trust-model change. The evidence link also remains an owner
attestation rather than something GitHub can semantically judge.

External-fork support is provisional until a canary from a separately owned fork proves GitHub's
required-deployment behavior. The canary must verify that the trusted workflow can check out the
fork commit, that the created deployment records the fork's exact head SHA, that the deployment
unblocks only that SHA, and that pushing a new head invalidates it. Do not merge or advertise
external-fork contributions before that canary passes. If GitHub does not associate the deployment
with a fork-only SHA as required, keep external forks unsupported and move the semantic gate to the
dedicated GitHub App design below.

Do not grant another account same-repository write access under this model. Before doing so, move
the semantic gate to a dedicated GitHub App whose credential is unavailable to PR workflows and
bind its required check to that App's integration ID. Canary same-repository and fork behavior
before replacing the required deployment.

Because this is currently a solo-maintainer repository, required approving reviews remain zero:
GitHub does not allow a PR author to approve their own PR. The independent frontier model performs
the semantic review, and the `frontier-reviewed` deployment is its enforced owner attestation.

The maintainer has a **pull-request-only** emergency bypass. It is for an Actions outage or a broken
repository rule that prevents the repair PR from satisfying its own gate. It never permits a direct
push. Explain the bypass and recovery in the PR before merging; ordinary urgency is not a reason to
use it.

Required-check and required-environment names are API contracts. To rename one, first land the
replacement, observe it on a PR, update the ruleset, and only then remove the old requirement in a
later PR.

## Release contract

Release tags use `v*` and are made immutable by the tag ruleset. The release workflow independently
verifies that the tagged commit is reachable from `origin/master`, that the tag matches
`Cargo.toml`, and that the release gates and platform builds pass before publishing. The GitHub
release is created before the irreversible crates.io publish. Create releases from an already
merged mainline commit; branch protection does not make arbitrary tags safe by itself.

## High-blast-radius changes

Changes to event or replay schemas, Git/ref handling, agent permissions, `tactus.toml`, `DESIGN.md`,
CI, release, rules, or frontier-review attestation deserve an especially narrow PR and a focused
fresh-context review. A PR can edit ordinary Actions workflows, so the independent review is the
trust boundary for changes to the gates themselves.
