#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
validator="$script_dir/validate-frontier-evidence.sh"
reviewed_sha="0123456789abcdef0123456789abcdef01234567"
stale_sha="89abcdef0123456789abcdef0123456789abcdef"

valid_record="$(printf 'UPSTROKE_FRONTIER_REVIEW: 1\nVERDICT: PASS\nREVIEWED_SHA: %s' "$reviewed_sha")"

expect_pass() {
  local name="$1"
  local body="$2"
  if ! printf '%s' "$body" | bash "$validator" "$reviewed_sha"; then
    echo "expected evidence fixture to pass: $name" >&2
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local body="$2"
  if printf '%s' "$body" | bash "$validator" "$reviewed_sha" 2>/dev/null; then
    echo "expected evidence fixture to fail: $name" >&2
    exit 1
  fi
}

expect_pass "one exact record" "$valid_record"
expect_fail "stale structured SHA" "$(printf 'UPSTROKE_FRONTIER_REVIEW: 1\nVERDICT: PASS\nREVIEWED_SHA: %s' "$stale_sha")"
expect_fail "mixed PASS and FAIL" "$(printf 'UPSTROKE_FRONTIER_REVIEW: 1\nVERDICT: FAIL\nVERDICT: PASS\nREVIEWED_SHA: %s' "$reviewed_sha")"
expect_fail "duplicate verdict" "$(printf 'UPSTROKE_FRONTIER_REVIEW: 1\nVERDICT: PASS\nVERDICT: PASS\nREVIEWED_SHA: %s' "$reviewed_sha")"
expect_fail "multiple structured SHAs" "$(printf 'UPSTROKE_FRONTIER_REVIEW: 1\nVERDICT: PASS\nREVIEWED_SHA: %s\nREVIEWED_SHA: %s' "$stale_sha" "$reviewed_sha")"
expect_fail "current SHA outside stale record" "$(printf 'UPSTROKE_FRONTIER_REVIEW: 1\nVERDICT: PASS\nREVIEWED_SHA: %s\nCURRENT_SHA: %s' "$stale_sha" "$reviewed_sha")"
expect_fail "quoted markers" "$(printf '> UPSTROKE_FRONTIER_REVIEW: 1\n> VERDICT: PASS\n> REVIEWED_SHA: %s' "$reviewed_sha")"
expect_fail "valid record plus quoted failure" "$(printf '%s\n> VERDICT: FAIL' "$valid_record")"

echo "frontier evidence fixtures: PASS"
