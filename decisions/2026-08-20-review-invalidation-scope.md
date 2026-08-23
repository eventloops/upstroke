# 2026-08-20 — what invalidates a frontier review

**Verdict.** A frontier review binds to the exact tree it read, with one explicit exception:
a later head whose entire difference from the reviewed head lies inside an **exempt path set**
keeps the review. The set starts as exactly one path — `reviews/FINDINGS.md` — and the default
is deny: every path not named invalidates, and widening the set is a successor decision record,
never a quiet edit. Re-attestation onto the new head additionally requires the reviewed head to
be an **ancestor** of the new one (a force-push never inherits), and the exempt-only property to
be **computed by the trusted workflow**, never asserted by the requester.

The GitHub-side enforcement — `frontier-review.yml`, its payload and validator scripts, and the
matching refinement to MAINTAINING.md steps 5–7 — is specified here and **not yet built**; until
that PR lands, attestation remains exact-head. What the box tooling alone controls was changed
the same day: it no longer pays for redundant reviews, and no longer mislabels valid ones (§
"Implemented" below).

## The measured cost that forced this

All from pull request #18, all on 2026-08-20 or the day before. None of it is assumed.

- `ff0490a` ("docs(reviews): record the owner ruling that the frozen files stay frozen")
  changed `reviews/FINDINGS.md` by 2 insertions and 2 deletions. `src/` is **byte-identical**
  to the prior head `cab3d042` (`git diff --quiet cab3d042 ff0490a -- src/`). That push landed
  mid-review: a completed 1381-second max-effort review of `cab3d042` (verdict
  `CHANGES_REQUIRED`, 2 findings) was discarded as stale, and a fresh ~28-minute run was
  started at 15:01 to re-derive findings against code that had not changed.
- The circularity is structural, not bad luck: `FINDINGS.md` lives in the repository *because*
  it is a review input — so **recording a ruling invalidates the review it was meant to
  settle.** Every future ruling repeats this cost until the binding changes.
- `b1864dd` ("docs(reviews): record PR4's CI attestation…") is the same class one day earlier,
  and the iteration cost is what teaches people to skip re-reviews — the habit that let a
  deleted contract-named proof test reach a reviewer at all.
- Review wall-clocks measured this week: 1719 s, 1594 s, 1381 s. This is the unit price of
  every unnecessary invalidation.

## The same day's second incident, and the rules it set

The 14:22 review was destroyed twice over. `~/.upstroke-env` rotated at 14:28:26, mid-review;
the post-review head check ran on the stale in-memory token, got `401 Bad credentials`, and
the script compared that JSON error body to the SHA — concluding "HEAD MOVED" and discarding
the verdict. **An API failure is not a moved head.** Rules adopted in both drivers, same day:

1. Re-source the credential environment before any post-review call.
2. Believe "the head moved" only from a value that parses as a 40-hex SHA; retry, and on
   persistent failure say *unverified*, never *stale*.
3. Record per-head "already reviewed" state only after a confirmed successful post — a state
   file written on a failed post loses the review twice.

## The rule, mechanically

For a dispatch whose `reviewed_sha` X differs from the current head Y, the trusted workflow:

- requires `git merge-base --is-ancestor X Y` — rewritten history never inherits;
- requires the drift `git diff --name-only --no-renames --ignore-submodules=none X..Y` to name no path outside the
  exempt set, computed on the trusted side from refs it fetched itself. **Renames are never
  exempt**: `--no-renames` splits a rename into its endpoints, so renaming a non-exempt file
  onto the exempt path surfaces the source as a deletion and refuses — under rename
  detection, `--name-only` reports only the exempt destination (review finding
  `DRIFT-RENAME-ENDPOINT`, reproduced with an R100 before fixing). And the producer's exit
  status is checked explicitly: a diff that fails to run is a refusal, never an empty
  "no drift" (`DRIFT-DIFF-STATUS` — process substitution swallows the status). Submodule
  pointers are never hidden: `--ignore-submodules=none` overrides any `ignore = all` in
  `.gitmodules`, which otherwise drops a gitlink retarget even from `--name-status`
  (`DRIFT-IGNORED-GITLINK`, round 2, reproduced);
- validates the evidence comment against X, byte-exactly, unchanged
  (`validate-frontier-evidence.sh` semantics stay as they are);
- re-runs its lint/test legs on Y as it does today, and publishes the App check on **Y** with
  an `external_id` that carries X and the evidence digest — the claim recorded is "reviewed at
  X; attested at Y; X..Y exempt-only", never a pretence that Y was reviewed;
- keeps every existing refusal: behind-base still refuses (exempt drift is not base drift),
  metadata digests still bind, the invalidator still applies.

The trigger stays exactly what it is today — an owner dispatch. This record narrows what a
claim must re-pay for; it automates nothing new about making claims.

## Why the exempt set is one file

`reviews/FINDINGS.md` is the only path with a measured circular cost. `decisions/` records and
prose docs plausibly deserve exemption too — and each can have it the day someone writes the
successor record arguing it. Keeping the first set minimal is the point of default-deny: every
widening is a visible, reviewable act with a name on it.

## Options rejected

- **Scope-hash `src/` (or "the slice's owned paths") instead of the head SHA** — the other
  session's proposed remedy. Rejected because it inverts the default at the trust boundary:
  `.github/workflows/**`, the validator scripts, `Cargo.toml`/`Cargo.lock`, and MAINTAINING.md
  would *stop* invalidating — precisely the paths whose silent drift is most dangerous,
  including the definition of this gate itself. Default-allow anywhere on the attestation
  plane is the wrong direction, and it turns "which paths are covered" into an implicit
  property of where a hash was scoped rather than a recorded decision.
- **Require the exempt diff to be append-only.** Would have blocked the motivating edit:
  `ff0490a` modified two existing lines to record an owner ruling. FINDINGS.md's append-only
  convention governs entries, not bytes.
- **Do nothing.** The cost recurs on every recorded ruling at ~23–29 minutes each, and the
  habit it teaches — batch the rulings, skip the re-review — is the worse failure and is
  already observed.

## The risk accepted, stated plainly

Under this rule, an edit to `reviews/FINDINGS.md` can merge without a frontier review of that
edit (within its PR; a review of the earlier head still covered everything else). Bounds:
`upstroke-ci` and `upstroke-pr-policy` still gate it; the re-attesting dispatch is an explicit
owner act naming exactly that drift; the file governs the review *process*, not build outputs;
and under the ledger's own authority rule the file records rulings — it does not create them,
so a hostile edit there claims an authority the process does not grant it. Residual risk:
social engineering of the human at dispatch time, which is the same residual the evidence path
carries today. This paragraph is the acceptance, so a future reader knows it was priced.

## Implemented 2026-08-20 (box-side only; no trust boundary moved)

- `upstroke-review-watch` (stage 1): an eligible head whose only difference from an
  already-reviewed head is exempt-only **inherits** that review — a state file records the
  inheritance, no duplicate comment is posted, no reviewer is paid. `--check-drift PR OLD NEW`
  exposes the predicate as a diagnostic; it was verified against the incident pair
  (`cab3d042..ff0490a` → exempt-only; a code-bearing pair and the reversed direction →
  refused). §"second incident" rules 1–3 were already in this driver.
- `upstroke-frontier-review`: §"second incident" rules applied (backup kept alongside); a moved
  head is now classified `STALE` vs `STALE-EXEMPT-ONLY`, so a human stops discarding verdicts
  that remain valid for the code. It still offers no evidence text for either — exact-head
  attestation holds until the workflow PR lands.

- Review round 1 on the implementing PR (#22) surfaced `DRIFT-RENAME-ENDPOINT` and
  `DRIFT-DIFF-STATUS` in the workflow's first cut, and the same rename hole existed in both
  box drivers (`upstroke-review-watch`'s mirror diff and `upstroke-frontier-review`'s compare-API
  classifier, which folds a rename into one destination-named entry). All three now treat
  renames as non-exempt and fail closed on a failed producer; both holes were reproduced
  before being fixed, and the PR's ledger carries the full rows.

- Review round 2 surfaced `DRIFT-IGNORED-GITLINK`: with `ignore = all` in `.gitmodules`, a
  gitlink retarget to an unreviewed commit vanished even from `--name-status` — reproduced
  before fixing. Every drift computation (both workflow jobs and the stage-1 mirror driver)
  now passes `--ignore-submodules=none`.

- Review round 3 surfaced `DRIFT-GUARD-COVERAGE`: inverting both stale-head guards to
  equality survived every grep fixture (named mutation `MUT-DRIFT-GUARD-INVERTED`, confirmed
  live before fixing). `test-frontier-workflow.sh` now pins the guard polarity and
  **executes** the extracted drift block against fixture repositories — seven behavioral
  verdicts including the rename, gitlink, and failed-producer attacks. The workflow itself
  needed no change in this round.

## Measured vs assumed

Measured: the ff0490a/cab3d042 byte-identity; the 14:22/14:28/14:45 timeline and the 401 body
in the run log; wall-clocks 1719/1594/1381 s; the PAT's inability to modify rulesets or
protection (403 on the administration endpoints, probed today); ancestry and exempt-onlyness
of the incident pair (the `--check-drift` verification). Assumed, and named as judgment: that
exempt FINDINGS.md edits remain low-risk under the bounds above — revisit if a challenge or
ruling ever changes what a *pending* review would conclude.

## Cross-references

- [2026-08-20 — the automated review gate](2026-08-20-automated-review-gate.md) — §9 classes
  this rule as boundary work that survives self-hosting; stage 1 is where inheritance runs.
- `reviews/FINDINGS.md` — the authority rule this record leans on, and the file whose binding
  it changes.
- MAINTAINING.md steps 5–7 — refined by the workflow PR that implements this record.
