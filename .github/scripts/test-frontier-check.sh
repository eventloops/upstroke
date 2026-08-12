#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
builder="$script_dir/frontier-check-payload.sh"
repository="keybindings/tactus"
reviewed_sha="0123456789abcdef0123456789abcdef01234567"
stale_sha="89abcdef0123456789abcdef0123456789abcdef"
pr_number="8"
review_url="https://github.com/$repository/pull/$pr_number#issuecomment-123456"
evidence_digest="abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"

payload="$(GITHUB_REPOSITORY="$repository" bash "$builder" \
  "$reviewed_sha" "$pr_number" "$review_url" "$evidence_digest")"

jq -e \
  --arg sha "$reviewed_sha" \
  --arg url "$review_url" \
  --arg digest "$evidence_digest" \
  '. == {
    name: "tactus-frontier-review",
    head_sha: $sha,
    status: "completed",
    conclusion: "success",
    details_url: $url,
    external_id: ("tactus-frontier-review:pr-8:" + $sha + ":" + $digest),
    output: {
      title: "Independent frontier review passed",
      summary: ("PR #8 passed independent frontier review for exact head `" + $sha + "`.\n\nEvidence SHA-256: `" + $digest + "`."),
      text: ("Review evidence: " + $url)
    }
  }' <<< "$payload" >/dev/null

expect_fail() {
  local name="$1"
  shift
  if GITHUB_REPOSITORY="$repository" bash "$builder" "$@" >/dev/null 2>&1; then
    echo "expected check payload fixture to fail: $name" >&2
    exit 1
  fi
}

expect_fail "uppercase SHA" "${reviewed_sha^^}" "$pr_number" "$review_url" "$evidence_digest"
expect_fail "stale evidence URL" "$reviewed_sha" "$pr_number" \
  "https://github.com/$repository/pull/7#issuecomment-123456" "$evidence_digest"
expect_fail "non-comment evidence URL" "$reviewed_sha" "$pr_number" \
  "https://github.com/$repository/pull/$pr_number" "$evidence_digest"
expect_fail "invalid digest" "$reviewed_sha" "$pr_number" "$review_url" "$stale_sha"

if GITHUB_REPOSITORY="not-a-repository" bash "$builder" \
  "$reviewed_sha" "$pr_number" "$review_url" "$evidence_digest" >/dev/null 2>&1; then
  echo "expected invalid repository fixture to fail" >&2
  exit 1
fi

echo "frontier check payload fixtures: PASS"
