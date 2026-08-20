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

echo "frontier workflow trust fixtures: PASS"
