# Maintaining upstroke

The operating contract for changes to the protected default branch, `master`. Repository rules
enforce the mechanical parts; the owner is responsible for the review evidence. It applies to
source, documentation, workflows, release machinery and this file.

## How a change lands

1. **Branch from current `master`** and keep the change to one coherent, independently revertible
   outcome. Conventional Commit title: `type(optional-scope): summary`.
2. **Open a draft pull request early.** The body has six sections — Summary, Scope, Validation,
   Review evidence, Risk and rollback, Review finding ledger — and
   `.github/scripts/validate-pr-body.sh` rejects anything else. Run it against your body before
   pushing.
3. **Run the eight-command baseline** (`CODING_STANDARDS.md` §2) before every push, then wait for
   the two required contexts: `upstroke-ci` (formatting, Clippy on three platforms, the test and
   MSRV matrices, the four Bash gates) and `upstroke-pr-policy` (title, body sections, ledger). A
   branch behind `master` is updated first and waits again.
4. **One frontier review pass on the green head.** Give the exact diff and head SHA to an
   independent frontier-class reviewer at `max` effort — today `gpt-5.6-sol` through `codex exec`,
   run by the owner's review driver against the pull request's own base. Allow at least 90 minutes per
   pass and stream the output; a timeout, transport failure or missing verdict is not a pass.
   Record in the body: implementation model and effort, reviewed head SHA, reviewer model and
   effort, transport and wall-clock limit, and a durable link to the verdict as written (the driver
   posts it to the pull request as one SHA-bound comment).
5. **Triage every finding.** The author babysits the pull request until it lands:
   - A finding that is **relevant to the change** and a **serious P1** (below) is fixed, and the
     repaired head gets a fresh pass.
   - A relevant finding that is not serious is fixed at the author's discretion or logged as tech
     debt: a ledger row with a stable id, an honest failure sequence, and disposition `deferred`
     or `accepted-risk`. Rows that outlive the pull request move to `reviews/FINDINGS.md`.
   - A finding that is **not relevant to the change** — pre-existing, out of scope, or against an
     unswept file under a transitional standard (`standards/SWEEP.md`) — is logged the same way or
     `rejected` with the reason, and blocks nothing.
   - Two rules outrank the label. A `MUST` deviation in materially touched code is fixed, or the
     standard is amended by reviewed change. A finding carrying a failing test, reproduction or
     mutation witness is fixed whatever its severity. Either may be `rejected` only by a row
     showing the evidence invalid: a `MUST` the code does not breach, a witness that does not
     reproduce on the head.

   A repair-only push after a pass that found no serious P1 needs no second pass: the owner reads
   `git diff <reviewed> <head>`, confirms it contains those repairs and nothing else — never a
   workflow, gate script or validator edit — and says so in the body. A push confined to
   `reviews/FINDINGS.md`, or a conflict-free merge of `master` that leaves `git diff master...HEAD`
   byte-identical with CI green on the merged head and no gate edited by the pull request, keeps
   the review as well; record both SHAs, and for the merge-in both base SHAs and the diff hash
   before and after. Anything wider is a new change and is reviewed again. A panel-reviewed
   checkpoint candidate is the exception: any head movement re-runs every seat.
6. **Record the pass as written.** A `CHANGES_REQUIRED` whose findings all landed as repairs or
   ledger rows is recorded as that verdict with each disposition, never as a pass. When the merged
   head differs from the reviewed head, list the delta commits and what verified each. Re-run
   `validate-pr-body.sh` from the default branch against the live title and body.
7. **Merge with a merge commit** once every conversation is resolved and both contexts are green
   on the head being merged. The merge is the owner's attestation that the evidence is real and the
   merged head is accounted for: reviewed directly, or separated from the reviewed SHA only by the
   deltas step 5 allows. The owner may delegate the merge, in writing, to the agent doing the work
   on a pull request that has reached this state; the delegation is disclosed in the body. Never
   push to `master` directly. Delete the branch.

### Serious P1

A finding is a serious P1 when its failure sequence is concrete on the current head and reaches at
least one of:

- a `DESIGN.md` §4 invariant;
- the trust boundary, the merge or release machinery, or a gate change that misstates what the
  gate enforces (`security-trust`);
- durable state: the event log, replay, or anything that makes a recorded run unreproducible or
  corrupt;
- loss or corruption of data in a user repository — the engine owns git;
- a legal or licensing defect.

The reviewer's label does not decide this; the owner classifies, and a P1 whose failure needs
speculative preconditions is reclassified down with a ledger row saying why.

### Tech debt sweeps

Logged rows are swept, not forgotten, at three points: before any release tag or crates.io publish,
where every open `accepted-risk` and `deferred` row is re-triaged and the release notes name what
ships open; at each integration checkpoint merge; and on owner call. A sweep fixes a row, re-accepts
it dated, or converts it to a tracked follow-up; a row re-accepted twice carries the owner's dated
note saying why it stays.

### Review finding ledger

Every actionable finding gets a stable id and one row in the pull request's ledger:

- severity `P0`–`P3`, the full reviewed SHA and `path:line`, and a concrete `A -> B -> failure`
  sequence;
- provenance: `pre_existing`, `introduced_by_feature`, `fix_regression`, or `undetermined`;
- category: `correctness`, `crash-consistency`, `security-trust`, `portability`, `liveness`,
  `performance`, `compatibility`, or `docs-contract`;
- first-bad commit where history can establish it, and any earlier finding id when it recurs;
- the named regression test or documented deterministic guard, and a disposition: `fixed`,
  `rejected`, `deferred`, or `accepted-risk`.

Provenance explains where a defect came from; it does not make it less real. Every code defect
fixed in the pull request gets a regression test that fails on the first-bad shape. Keep fixed and
rejected rows: the ledger preserves why, not only what remains open.

`validate-pr-body.sh` enforces the header and tokens. `validate-pr-ledger-evidence.sh` resolves
each row against the exact head: the reviewed SHA must be an ancestor, the path and line must exist
at that commit, and every backticked regression or guard identifier must occur in tracked content.
Bind rows to the first integrated commit, never to a lane commit that was later cherry-picked.
Change a validator and its fixtures in the same pull request as any schema change.

Slices of a long-running design land as pull requests into their integration branch (today
`codex/parallelism-design`) under the same steps; the integration branch's own pull request into
`master` is reviewed once more, on the head that merges. Merge commits only, everywhere: a rewrite
orphans every ledger row bound to a replaced SHA.

## Repository rules

The default-branch ruleset requires a pull request and an up-to-date branch, `upstroke-ci` and
`upstroke-pr-policy` on the current head, resolved conversations, and merge commits only; it blocks
deletion and non-fast-forward updates and has no bypass actor. A tag ruleset on `refs/tags/v*`
blocks updates and deletions with no bypass. Required-check names are API: to rename one, land the
replacement, observe it on a pull request, update the ruleset, then remove the old requirement.

**Trust boundary.** There is one trusted same-repository writer: the owner. A pull request can
edit `ci.yml`, `pr-policy.yml` and the validators they run and still turn both contexts green, so
the checks catch honest mistakes and are not the security boundary. The boundary is that only the
owner merges — or an agent the owner has delegated to for that pull request — after an independent
review recorded per step 4, and that the diff the owner reads includes any change to the gates. No
automated process merges or mints a merge-gating check: there is no machine review check, no App,
and no token that can attest. A return of automated attestation needs a signer distinct from the
owner's token, structurally validated verdicts, and a reviewer that holds no attesting credential.

Keep workflow approval for **all** external contributors, and keep release immutability enabled;
bootstrap and audit both through the API:

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

Fork pull requests are provisional: their checks are candidate-controlled, so the whole diff
including workflow edits is reviewed before merge. Do not add another same-repository writer
without revisiting this section. Required approving reviews stay at zero because an author cannot
approve their own pull request; if a machine account ever opens pull requests,
`require_code_owner_review` with the owner as code owner restores an exact-head sign-off. Any future
emergency bypass is pull-request-only, limited to an Actions outage or a rule that blocks its own
repair, and never a direct push.

## Release contract

Release tags use `v*` and the tag ruleset makes them immutable. Repository release immutability is
a separate mandatory setting: tag protection fixes the commit, release immutability keeps published
binaries from being replaced under it. GitHub applies it only to future releases, so enable it and
read it back before creating another tag. The release job's token cannot read that setting, so
`UPSTROKE_IMMUTABLE_RELEASES_REQUIRED=true` records the owner's readback and makes an incomplete
bootstrap fail closed. Re-read the live setting before every release.

The release workflow verifies that the tagged commit is reachable from `origin/master`, that the
tag matches `Cargo.toml`, and that the release gates and platform builds pass before publishing. It
refuses an existing mutable or incomplete release, discards only an unpublished same-tag draft
created by `github-actions[bot]` after a failed attempt, and skips rather than overwrites a complete
immutable release. Uploads must contain exactly the three expected assets; the workflow verifies
GitHub's signed release attestation and each local archive against its attested digest. The GitHub
release is created before the irreversible crates.io publish. Create releases from an already
merged mainline commit.

The next release is also gated by the 2026-09-01 relicensing: each archive must carry `LICENSE`,
`NOTICE` and generated third-party attributions before any new `v*` tag is created. The workflow
does not yet inspect archive contents, so until it does, verify this at tag time by reading the
archives back.

Release `v0.1.0` predates release immutability and is the sole legacy exception. Do not rerun,
replace or delete its assets. Its preserved GitHub asset digests are:

- `upstroke-aarch64-apple-darwin.tar.gz`: `sha256:552302e348273143665d2604130e6c1487647a90b496a8d8f789d30839175289`
- `upstroke-x86_64-pc-windows-msvc.zip`: `sha256:e88206643c07ac5cee418ed27ddbbb7e6bcffc1835e727a68bbd716f876c8871`
- `upstroke-x86_64-unknown-linux-gnu.tar.gz`: `sha256:94447cfd56d0d8ba5eae1ec391c2564a7ddba2fceb15cee35ce537a0ba00d798`

## High-blast-radius changes

Changes to event or replay schemas, Git/ref handling, agent permissions, `upstroke.toml`, the
design, CI, release, or rules deserve an especially narrow pull request and a focused fresh-context
review. A pull request can edit ordinary Actions workflows, so the independent review is the trust
boundary for changes to the gates themselves, and the owner reads those diffs line by line before
merging.
