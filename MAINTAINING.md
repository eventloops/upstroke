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
7. Once a review passes, post a dedicated evidence comment containing exactly
   `TACTUS_FRONTIER_REVIEW: 1`, `VERDICT: PASS`, and `REVIEWED_SHA: <full SHA>` on separate lines,
   with no other text. Send the `frontier-review` repository dispatch with the PR number, full
   reviewed head SHA, and evidence URL. The default-branch workflow refuses stale, ambiguous, or
   behind evidence, validates the PR metadata and evidence comment, reruns formatting, Clippy, and
   all three platform test jobs from its trusted default-branch definition, then uses the dedicated
   `Tactus Frontier Review Gate` GitHub App to publish a successful `tactus-frontier-review` check
   on the exact head.
8. Resolve every conversation, mark the PR ready, and merge with a merge commit. Do not push or
   force-push directly to `master`. Delete the source branch after merge.

The attestation workflow records a review; it does not perform one. The App-owned check is the hard
merge gate. The ruleset binds the check name to App id `4574301`, while the App credential is kept
out of every pull-request workflow. A same-named GitHub Actions check therefore cannot satisfy the
rule. The repository owner remains responsible for the truth of the linked semantic review;
dispatching without a real passing review is a policy violation.

### App-gate migration

The original required-deployment gate failed its stale-SHA canary: GitHub reported a pull request
mergeable after its head changed even though `frontier-reviewed` existed only on the previous SHA.
That deployment must not remain the semantic gate.

Migrate without opening an unprotected interval:

1. Install the private `Tactus Frontier Review Gate` App only on `keybindings/tactus`, with metadata
   read and checks write permissions and no webhook or event subscriptions.
2. Put its client id, numeric App id, and private key in the `frontier-check-signer` environment.
   Restrict that environment to protected branches so a pull-request-authored job cannot receive
   the key.
3. Land the default-branch attestation workflow and fixtures while the existing rules remain in
   force.
4. On a new canary PR, obtain an exact-head frontier review and observe a successful
   `tactus-frontier-review` check whose `.app.id` is `4574301`.
5. Add that App-bound check to the ruleset while retaining every existing requirement. Push a new
   canary head and prove that the stale check does not unblock it; then review and attest the new
   head.
6. Only after the canary passes, remove the `frontier-reviewed` deployment requirement and retire
   the old deployment writer. Never remove the deterministic checks during this sequence.

Example dispatch after a passing review:

```bash
pr=123
reviewed_sha="$(gh pr view "$pr" --json headRefOid --jq .headRefOid)"
# Give this exact full SHA and its diff to the reviewer. After it passes, post
# one machine record as the entire evidence comment (put prose in another comment):
evidence_body="$(printf 'TACTUS_FRONTIER_REVIEW: 1\nVERDICT: PASS\nREVIEWED_SHA: %s' \
  "$reviewed_sha")"
review_url="$(gh api --method POST "repos/keybindings/tactus/issues/$pr/comments" \
  -f "body=$evidence_body" --jq .html_url)"
gh api --method POST repos/keybindings/tactus/dispatches \
  -f event_type=frontier-review \
  -F "client_payload[pull_request]=$pr" \
  -f "client_payload[reviewed_sha]=$reviewed_sha" \
  -f "client_payload[review_url]=$review_url"
```

## Enforced repository rules

The default-branch ruleset must:

- require a pull request and an up-to-date branch;
- require `tactus-ci` and `tactus-pr-policy` on the current head;
- require `tactus-frontier-review` on the current head, bound to GitHub App id `4574301`;
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
then requests a repository-scoped installation token for the dedicated App. The App publishes only
the final exact-SHA check. The ruleset's App binding, rather than the check name alone, is the
security boundary.

The App is private, installed only on `keybindings/tactus`, and has only metadata read and checks
write. Its private key is an environment secret, never a repository secret. Only the final trusted
`repository_dispatch` job declares `frontier-check-signer`; that environment accepts protected
branches only. The short-lived installation token explicitly requests only `checks: write` and is
revoked by the token action at job completion. Keep approval for workflow runs from **all** external
contributors as a second, independent fork safeguard.

Bootstrap and audit the external configuration through the API. Supply the downloaded PEM on stdin
so it is never written into a command line, log, tracked file, or pull-request workflow:

```bash
gh api --method PUT repos/keybindings/tactus/environments/frontier-check-signer \
  -F wait_timer=0 \
  -F 'deployment_branch_policy[protected_branches]=true' \
  -F 'deployment_branch_policy[custom_branch_policies]=false'

gh variable set TACTUS_FRONTIER_APP_ID --env frontier-check-signer \
  --repo keybindings/tactus --body 4574301
gh variable set TACTUS_FRONTIER_APP_CLIENT_ID --env frontier-check-signer \
  --repo keybindings/tactus --body Iv23liSwpgxIDc8SN4ED
gh secret set TACTUS_FRONTIER_APP_PRIVATE_KEY --env frontier-check-signer \
  --repo keybindings/tactus < tactus-frontier-review-gate.private-key.pem

gh api --method PUT \
  repos/keybindings/tactus/actions/permissions/fork-pr-contributor-approval \
  -H 'X-GitHub-Api-Version: 2026-03-10' \
  -f approval_policy=all_external_contributors

gh api --method PUT repos/keybindings/tactus/immutable-releases \
  -H 'X-GitHub-Api-Version: 2026-03-10'

gh variable set TACTUS_IMMUTABLE_RELEASES_REQUIRED \
  --repo keybindings/tactus --body true

gh api repos/keybindings/tactus/environments/frontier-check-signer
gh variable list --env frontier-check-signer --repo keybindings/tactus
gh secret list --env frontier-check-signer --repo keybindings/tactus
gh api repos/keybindings/tactus/actions/permissions/fork-pr-contributor-approval \
  -H 'X-GitHub-Api-Version: 2026-03-10'
gh api repos/keybindings/tactus/immutable-releases \
  -H 'X-GitHub-Api-Version: 2026-03-10'
gh variable get TACTUS_IMMUTABLE_RELEASES_REQUIRED \
  --repo keybindings/tactus
```

The rules prevent accidental direct merges and stale-SHA evidence, and prevent same-name fork-check
spoofing from satisfying the semantic gate. They do not defend against compromise or dishonesty of
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
cannot read that administration-scoped setting, so `TACTUS_IMMUTABLE_RELEASES_REQUIRED=true`
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

- `tactus-aarch64-apple-darwin.tar.gz`: `sha256:552302e348273143665d2604130e6c1487647a90b496a8d8f789d30839175289`
- `tactus-x86_64-pc-windows-msvc.zip`: `sha256:e88206643c07ac5cee418ed27ddbbb7e6bcffc1835e727a68bbd716f876c8871`
- `tactus-x86_64-unknown-linux-gnu.tar.gz`: `sha256:94447cfd56d0d8ba5eae1ec391c2564a7ddba2fceb15cee35ce537a0ba00d798`

## High-blast-radius changes

Changes to event or replay schemas, Git/ref handling, agent permissions, `tactus.toml`, `DESIGN.md`,
CI, release, rules, or frontier-review attestation deserve an especially narrow PR and a focused
fresh-context review. A PR can edit ordinary Actions workflows, so the independent review is the
human trust boundary for changes to the gates themselves; the dedicated App remains the machine
identity that enforces the resulting exact-SHA verdict.
