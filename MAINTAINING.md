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
   effort, head SHA, and durable review link in the PR.
6. Fix every finding. Any code push creates a new head SHA, so return to step 3 and review the new
   head. Feature ideas discovered during review belong in the design or a follow-up unless they are
   required for the current change to be correct.
7. Once a review passes, post its evidence on the PR and send the `frontier-review` repository
   dispatch with the PR number, full reviewed head SHA, and evidence URL. The default-branch
   workflow refuses to attest a stale, behind, or mechanically failing head and creates the
   required `frontier-review` check run through the GitHub Actions app.
8. Resolve every conversation, mark the PR ready, and merge with a merge commit. Do not push or
   force-push directly to `master`. Delete the source branch after merge.

The attestation workflow records a review; it does not perform one. The ruleset can enforce the
exact SHA, current mechanical checks, and GitHub Actions identity, but the repository owner remains
responsible for the truth of the linked semantic review. Dispatching without a real passing review
is a policy violation.

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
- require `tactus-ci`, `tactus-pr-policy`, and `frontier-review` on the current head;
- require all review conversations to be resolved;
- allow merge commits only;
- block branch deletion and non-fast-forward updates; and
- have no direct-push bypass.

A separate tag ruleset targeting `refs/tags/v*` must block both updates and deletions with no
bypass. This makes the release tags described below genuinely immutable.

Because this is currently a solo-maintainer repository, required approving reviews remain zero:
GitHub does not allow a PR author to approve their own PR. The frontier-review check run is the
independent semantic-review gate.

The maintainer has a **pull-request-only** emergency bypass. It is for an Actions outage or a broken
repository rule that prevents the repair PR from satisfying its own gate. It never permits a direct
push. Explain the bypass and recovery in the PR before merging; ordinary urgency is not a reason to
use it.

Required-check names are API contracts. To rename one, first land the replacement, observe it pass
on a PR, update the ruleset, and only then remove the old check in a later PR.

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
