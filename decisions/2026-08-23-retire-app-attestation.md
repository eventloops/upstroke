# 2026-08-23 — retire the App-signed attestation gate

**Verdict.** The `upstroke-frontier-review` check minted by the `Upstroke Frontier Review
Gate` GitHub App, the attestation and invalidation workflows that produced it, the
evidence-comment protocol they consumed, and the `frontier-check-signer` environment are
retired. The default-branch ruleset requires `upstroke-ci` and `upstroke-pr-policy` on the
current head, a pull request, an up-to-date branch, resolved conversations, merge commits
only, and no bypass. **The review obligation does not change**: every head that merges is
reviewed by an independent frontier model first, the reviewed SHA and a durable link to
the verdict are recorded in the pull request, and the owner's merge is the attestation.

This supersedes the *mechanism* behind stage 1 of
`decisions/2026-08-20-automated-review-gate.md` and the "attestation stays master-only"
clause of `decisions/2026-08-21-stacked-slice-prs.md`. It does not touch the scheduling
rule (single reviewer per head, the panel once on the merge candidate), the invalidation
scope of `decisions/2026-08-20-review-invalidation-scope.md`, or the three properties §5
of the 2026-08-20 record demands before any automated attestation is built. Those remain
the bar.

Executed the same day, in this order: the ruleset edited to the two Actions contexts (the
rename pull request went from `BLOCKED` to `CLEAN` on that edit alone); both workflows
disabled; the environment deleted with its secret and variables; this record and the
repository cleanup. Once this lands nothing references App id `4574301`; deleting the
registration is the owner's final step. `MAINTAINING.md` carries the compressed edit.

## What the App actually enforced

Measured against this repository, not assumed:

- **Against agents: nothing the convention did not already do.** The attestation workflow
  required the evidence comment to be owner-authored, and the build box and every agent
  session act on the owner's token (2026-08-20 record, §5, in its own words). An agent
  could have posted the comment and sent the dispatch; what stopped it was that none
  does. Retiring the App moves the same convention one step later — no automated process
  merges — and leaves the boundary where it was.
- **Against forks and same-name checks: real, and not needed yet.** GitHub identifies every
  `GITHUB_TOKEN` workflow as one app, so a pull request can mint a green check under any
  name. The App binding was the only thing that made a required context unforgeable.
  There are no external contributors; the owner reads every diff, edits to the gates
  included, before merging.
- **Against a stale or edited review: a machine record of the reviewed SHA, and a failure
  on title/body edits.** Useful. Replaced by the `Reviewed head SHA` field in the body and
  the owner's re-check in step 7 of the mainline sequence.

## What it cost

- The rename pull request (#29, 2026-08-23) could not merge until the ruleset's three
  bound contexts were renamed by hand: the renamed workflows reported under names the rule
  had never heard of. The App's display name, two environment variables, one environment
  secret, one repository variable, the ruleset binding, the check's `external_id` format
  and the workflow names all had to move together.
- The signing key is an environment secret that cannot be read back, renamed or copied:
  every rename or rotation needs the PEM from wherever it was kept. Locating it stalled the
  rename.
- Two privileged workflows (a `repository_dispatch` signer and a `pull_request_target`
  invalidator), four scripts, four fixture tests and roughly a third of `MAINTAINING.md`
  existed to operate a check that, for the threat actually present, the owner's merge
  already provides.

## Rejected

- **Keep the App until stage 2 is authorised.** It would be maintained for a scenario
  that is not authorised, at a cost paid on every rename, rotation and ruleset edit now.
- **Replace it with required code-owner review.** Not now: GitHub does not let an author
  approve their own pull request, and the owner authors most of them. It becomes the right
  mechanism the day pull requests are opened by a machine account —
  `require_code_owner_review` plus `dismiss_stale_reviews_on_push` then gives an
  exact-head owner sign-off with no key to manage. Recorded in `MAINTAINING.md`.
- **A PAT-driven signer.** Rejected as the 2026-08-20 record already rejected it: a
  fine-grained token that can dispatch needs `Contents: write`, which also grants push.
- **The deployment-environment gate.** `frontier-reviewed` failed its stale-SHA canary in
  August — GitHub reported a pull request mergeable after its head changed — and must not
  return as an attestation mechanism. The environment object itself is an orphan and may
  be deleted.

## If automated attestation returns

It starts from a new decision record, not from this one, and from 2026-08-20 §5: the
reviewer process holds no credential that can attest; verdicts are structured and
validated on the trusted side; any low-risk class is computed from the diff on the default
branch. The signer is a GitHub App registered for that purpose, its key an environment
secret under a `master`-only branch policy, its check bound in the ruleset by integration
id. The retired workflows remain in history at `3d9cf38` if their shape is wanted again;
`frontier-review.yml` had the correct form — dispatch-triggered, checked out at
`github.sha`, never at the pull-request head.
