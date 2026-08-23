#!/usr/bin/env bash
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$script_dir/../.." && pwd)"
workflow="$root/.github/workflows/frontier-review.yml"
invalidation_workflow="$root/.github/workflows/frontier-review-invalidate.yml"
canonical="printf '%s\\n' \"\$pr_body\" | bash .github/scripts/validate-pr-body.sh \"\$pr_title\""
canonical_evidence="printf '%s\\n' \"\$pr_body\" | bash .github/scripts/validate-pr-ledger-evidence.sh \"\$head_sha\""

validate_block="$(sed -n '/^  validate:/,/^  lint:/p' "$workflow")"
attest_block="$(sed -n '/^  attest:/,$p' "$workflow")"

assert_trusted_validation() {
  local name="$1"
  local block="$2"

  if ! grep -Fq 'ref: ${{ github.sha }}' <<< "$block"; then
    echo "$name does not check out the trusted default-branch workflow SHA" >&2
    exit 1
  fi
  if ! grep -Fq 'pr_title="$(jq -r '\''.title'\'' <<< "$pr")"' <<< "$block" ||
     ! grep -Fq 'pr_body="$(jq -r '\''.body // ""'\'' <<< "$pr" | tr -d '\''\r'\'')"' <<< "$block"; then
    echo "$name does not fetch both live PR title and body" >&2
    exit 1
  fi
  if ! grep -Fq "$canonical" <<< "$block"; then
    echo "$name does not pass the fetched title/body to the canonical validator" >&2
    exit 1
  fi
  if ! grep -Fq 'refs/pull/$PR_NUMBER/head:$ledger_ref' <<< "$block" ||
     ! grep -Fq "$canonical_evidence" <<< "$block"; then
    echo "$name does not resolve ledger evidence against the fetched exact PR history" >&2
    exit 1
  fi
}

assert_trusted_validation "validate job" "$validate_block"
assert_trusted_validation "attest job" "$attest_block"

if grep -Eq 'title_pattern=|required_sections=\(' "$workflow"; then
  echo "frontier workflow duplicates PR policy instead of using the canonical validator" >&2
  exit 1
fi

validator_line="$(grep -nF "$canonical" "$workflow" | tail -n 1 | cut -d: -f1)"
token_line="$(grep -nF 'name: Mint a repository-scoped review-gate token' "$workflow" | cut -d: -f1)"
if [[ -z "$validator_line" || -z "$token_line" || "$validator_line" -ge "$token_line" ]]; then
  echo "the final trusted PR-policy validation must precede App-token minting" >&2
  exit 1
fi

if ! grep -Fq 'pull_request_target:' "$invalidation_workflow" ||
   ! grep -Fq 'types: [edited]' "$invalidation_workflow"; then
  echo "frontier invalidation must be driven by trusted PR metadata-edit events" >&2
  exit 1
fi
if ! grep -Fq 'if: github.event.changes.title != null || github.event.changes.body != null' \
  "$invalidation_workflow"; then
  echo "frontier invalidation is not restricted to title/body edits" >&2
  exit 1
fi
if ! grep -Fq 'ref: ${{ github.sha }}' "$invalidation_workflow"; then
  echo "frontier invalidation does not check out the trusted default-branch SHA" >&2
  exit 1
fi
if grep -Fq 'ref: ${{ github.event.pull_request.head.sha }}' "$invalidation_workflow" ||
   grep -Eq '^[[:space:]]+repository:[[:space:]]*\$\{\{[[:space:]]*github\.event\.pull_request\.head' \
     "$invalidation_workflow"; then
  echo "frontier invalidation must never check out candidate-controlled code" >&2
  exit 1
fi

invalidate_token_line="$(grep -nF 'name: Mint the review-gate App token' \
  "$invalidation_workflow" | cut -d: -f1)"
invalidate_call_line="$(grep -nF 'bash .github/scripts/invalidate-frontier-check.sh' \
  "$invalidation_workflow" | cut -d: -f1)"
if [[ -z "$invalidate_token_line" || -z "$invalidate_call_line" ||
      "$invalidate_token_line" -ge "$invalidate_call_line" ]]; then
  echo "the metadata-edit job does not use the dedicated App to invalidate its own check" >&2
  exit 1
fi

publish_line="$(grep -nF 'check_run="$(GH_TOKEN="$APP_TOKEN" gh api --method POST' \
  "$workflow" | cut -d: -f1)"
race_invalidation_line="$(grep -nF 'GH_TOKEN="$APP_TOKEN" bash .github/scripts/invalidate-frontier-check.sh' \
  "$workflow" | cut -d: -f1)"
if [[ -z "$publish_line" || -z "$race_invalidation_line" ||
      "$publish_line" -ge "$race_invalidation_line" ]]; then
  echo "the signer does not fail a just-published check when metadata races publication" >&2
  exit 1
fi

# Exempt-path drift must be enforced in BOTH the validate and attest jobs, on
# the trusted side, and the drifted attestation must name the reviewed SHA.
if [[ "$(grep -cF 'git merge-base --is-ancestor "$REVIEWED_SHA" "$head_sha"' "$workflow")" -ne 2 ]]; then
  echo "exempt-path drift must be ancestor-checked in both validate and attest" >&2
  exit 1
fi
if [[ "$(grep -cF '!= "reviews/FINDINGS.md"' "$workflow")" -ne 2 ]]; then
  echo "the exempt path set must be enforced in both validate and attest" >&2
  exit 1
fi
if ! grep -Fq 'expected_external_id+=":reviewed:$REVIEWED_SHA"' "$workflow"; then
  echo "a drifted attestation must record the reviewed SHA in the external id" >&2
  exit 1
fi
if ! grep -Fq '"$ATTESTED_SHA" "$REVIEWED_SHA" "$PR_NUMBER" "$REVIEW_URL" "$EVIDENCE_DIGEST")"' "$workflow"; then
  echo "the check payload must be built from both the attested and reviewed SHAs" >&2
  exit 1
fi

# Round-1 review findings, kept dead: a rename onto the exempt path must
# surface its source (--no-renames), and a failed drift producer must fail
# closed rather than read as an empty diff.
if [[ "$(grep -cF 'git diff --name-only --no-renames --ignore-submodules=none "$REVIEWED_SHA..$head_sha"' "$workflow")" -ne 2 ]]; then
  echo "drift must be computed with --no-renames and --ignore-submodules=none in both validate and attest" >&2
  exit 1
fi
if grep -Fq 'done < <(git diff' "$workflow"; then
  echo "the drift producer status must be checked; process substitution hides it" >&2
  exit 1
fi
if [[ "$(grep -cF 'could not compute the drift between' "$workflow")" -ne 2 ]]; then
  echo "a failed drift computation must fail closed in both jobs" >&2
  exit 1
fi

# --- behavioral: execute the extracted drift block against fixture repos -----
# Grep fixtures cannot see logic polarity: inverting both stale-head guards to
# == leaves every asserted fragment in place while skipping validation exactly
# when drift exists (round-3 finding DRIFT-GUARD-COVERAGE, named mutation
# MUT-DRIFT-GUARD-INVERTED, confirmed live). So pin the polarity, then execute
# the shipped text against real repositories and assert its verdicts.
if [[ "$(grep -cF 'if [[ "$head_sha" != "$REVIEWED_SHA" ]]; then' "$workflow")" -ne 2 ]]; then
  echo "both drift guards must compare head to reviewed SHA with != (polarity pinned)" >&2
  exit 1
fi

drift_tmp="$(mktemp -d)"
cleanup_drift() { rm -rf "$drift_tmp"; }
trap cleanup_drift EXIT

block="$drift_tmp/drift-block.sh"
awk '
  $0 ~ /if \[\[ "\$head_sha" != "\$REVIEWED_SHA" \]\]; then/ && !got { f = 1 }
  f { print }
  f && $0 == "          fi" { got = 1; f = 0 }
' "$workflow" > "$block"
if [[ "$(wc -l < "$block")" -lt 30 ]]; then
  echo "could not extract the drift block from the workflow" >&2
  exit 1
fi
bash -n "$block"

mkrepo() {
  git init -q "$1"
  git -C "$1" config user.email fixtures@upstroke.invalid
  git -C "$1" config user.name "drift fixtures"
}
run_drift() {
  ( cd "$3" && REVIEWED_SHA="$1" head_sha="$2" \
      bash -c "set -euo pipefail; . '$block'; echo DRIFT-OK" 2>&1 )
}
expect_accept() {
  local out
  out="$(run_drift "$1" "$2" "$3")" || { echo "drift block refused: $4 ($out)" >&2; exit 1; }
  grep -q DRIFT-OK <<< "$out" || { echo "drift block did not accept: $4" >&2; exit 1; }
}
expect_refuse() {
  local out
  if out="$(run_drift "$1" "$2" "$3")"; then
    echo "drift block accepted what it must refuse: $4" >&2
    exit 1
  fi
  grep -q "$5" <<< "$out" || { echo "wrong refusal for $4: $out" >&2; exit 1; }
}

base="$drift_tmp/base"; mkrepo "$base"
mkdir -p "$base/reviews"
echo code > "$base/src.rs"; echo ledger > "$base/reviews/FINDINGS.md"
git -C "$base" add -A; git -C "$base" commit -qm A
sha_a="$(git -C "$base" rev-parse HEAD)"
echo ruling >> "$base/reviews/FINDINGS.md"
git -C "$base" commit -aqm B
sha_b="$(git -C "$base" rev-parse HEAD)"
echo more >> "$base/src.rs"
git -C "$base" commit -aqm C
sha_c="$(git -C "$base" rev-parse HEAD)"

expect_accept "$sha_a" "$sha_a" "$base" "equal SHAs"
expect_accept "$sha_a" "$sha_b" "$base" "exempt-only drift"
expect_refuse "$sha_a" "$sha_c" "$base" "non-exempt drift" "outside the exempt set"
expect_refuse "$sha_b" "$sha_a" "$base" "reversed ancestry" "not an ancestor"

ren="$drift_tmp/rename"; mkrepo "$ren"
mkdir -p "$ren/reviews"
echo old > "$ren/reviews/archive.md"
git -C "$ren" add -A; git -C "$ren" commit -qm X
ren_x="$(git -C "$ren" rev-parse HEAD)"
git -C "$ren" mv reviews/archive.md reviews/FINDINGS.md
git -C "$ren" commit -qm Y
ren_y="$(git -C "$ren" rev-parse HEAD)"
expect_refuse "$ren_x" "$ren_y" "$ren" "rename onto the exempt path" "archive.md"

subr="$drift_tmp/sub"; mkrepo "$subr"
echo one > "$subr/f"; git -C "$subr" add f; git -C "$subr" commit -qm s1
sub_1="$(git -C "$subr" rev-parse HEAD)"
echo two >> "$subr/f"; git -C "$subr" commit -aqm s2
sub_2="$(git -C "$subr" rev-parse HEAD)"
glm="$drift_tmp/gitlink"; mkrepo "$glm"
( cd "$glm" && git -c protocol.file.allow=always submodule add -q "$subr" vendor/dep )
git -C "$glm" config -f .gitmodules submodule.vendor/dep.ignore all
mkdir -p "$glm/reviews"; echo ledger > "$glm/reviews/FINDINGS.md"
# Stage the gitlink through update-index: with ignore = all in .gitmodules,
# some git versions silently skip a submodule path handed to git add (the box
# staged it; ubuntu-latest did not, so the fixture built no drift and the
# block rightly accepted -- flipping this test's verdict). cacheinfo staging
# bypasses ignore semantics, and the fixture then asserts the drift it built.
git -C "$glm" update-index --add --cacheinfo "160000,$sub_1,vendor/dep"
git -C "$glm" add .gitmodules reviews/FINDINGS.md
git -C "$glm" commit -qm X
glm_x="$(git -C "$glm" rev-parse HEAD)"
echo ruling >> "$glm/reviews/FINDINGS.md"
git -C "$glm" update-index --add --cacheinfo "160000,$sub_2,vendor/dep"
git -C "$glm" add reviews/FINDINGS.md
git -C "$glm" commit -qm Y
glm_y="$(git -C "$glm" rev-parse HEAD)"
if [[ "$(git -C "$glm" rev-parse "$glm_x:vendor/dep")" != "$sub_1" ||
      "$(git -C "$glm" rev-parse "$glm_y:vendor/dep")" != "$sub_2" ]]; then
  echo "gitlink fixture failed to build the drift it claims to test" >&2
  exit 1
fi
expect_refuse "$glm_x" "$glm_y" "$glm" "gitlink retarget under ignore=all" "vendor/dep"

shim="$drift_tmp/shim"; mkdir -p "$shim"
printf '#!/bin/bash\nif [ "$1" = diff ]; then echo "shim: diff refused" >&2; exit 128; fi\nexec /usr/bin/git "$@"\n' > "$shim/git"
chmod 755 "$shim/git"
shim_out=""
if shim_out="$( cd "$base" && PATH="$shim:$PATH" REVIEWED_SHA="$sha_a" head_sha="$sha_b" \
    bash -c "set -euo pipefail; . '$block'; echo DRIFT-OK" 2>&1 )"; then
  echo "drift block accepted despite a failed diff producer" >&2
  exit 1
fi
grep -q "could not compute the drift" <<< "$shim_out" || {
  echo "failed producer did not fail closed: $shim_out" >&2
  exit 1
}

trap - EXIT
cleanup_drift

echo "frontier workflow trust fixtures: PASS"
