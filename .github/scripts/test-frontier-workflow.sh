#!/usr/bin/env bash
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$script_dir/../.." && pwd)"
workflow="$root/.github/workflows/frontier-review.yml"
canonical="printf '%s\\n' \"\$pr_body\" | bash .github/scripts/validate-pr-body.sh \"\$pr_title\""

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

echo "frontier workflow trust fixtures: PASS"
