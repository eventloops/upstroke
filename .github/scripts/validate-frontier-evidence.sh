#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! "$1" =~ ^[0-9a-f]{40}$ ]]; then
  echo "usage: validate-frontier-evidence.sh <full-lowercase-reviewed-sha>" >&2
  exit 2
fi

reviewed_sha="$1"
comment_body="$(cat)"
expected_body="$(printf 'UPSTROKE_FRONTIER_REVIEW: 1\nVERDICT: PASS\nREVIEWED_SHA: %s' "$reviewed_sha")"

if [[ "$comment_body" != "$expected_body" ]]; then
  echo "review evidence must be exactly one UPSTROKE_FRONTIER_REVIEW v1 PASS record for $reviewed_sha" >&2
  exit 1
fi
