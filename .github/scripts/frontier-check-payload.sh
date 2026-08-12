#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: frontier-check-payload.sh <reviewed-sha> <pr-number> <review-url> <evidence-digest>" >&2
  exit 2
fi

reviewed_sha="$1"
pr_number="$2"
review_url="$3"
evidence_digest="$4"

if [[ ! "$reviewed_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "reviewed SHA must be 40 lowercase hexadecimal characters" >&2
  exit 2
fi
if [[ ! "$pr_number" =~ ^[1-9][0-9]*$ ]]; then
  echo "pull-request number must be a positive integer" >&2
  exit 2
fi
if [[ ! "$evidence_digest" =~ ^[0-9a-f]{64}$ ]]; then
  echo "evidence digest must be a lowercase SHA-256 value" >&2
  exit 2
fi
if [[ ! "${GITHUB_REPOSITORY:-}" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  echo "GITHUB_REPOSITORY must be an owner/repository name" >&2
  exit 2
fi

review_prefix="https://github.com/$GITHUB_REPOSITORY/pull/$pr_number#issuecomment-"
review_comment_id="${review_url#"$review_prefix"}"
if [[ "$review_url" == "$review_comment_id" || ! "$review_comment_id" =~ ^[1-9][0-9]*$ ]]; then
  echo "review URL must identify the exact PR evidence comment" >&2
  exit 2
fi

jq -n \
  --arg reviewed_sha "$reviewed_sha" \
  --arg pr_number "$pr_number" \
  --arg review_url "$review_url" \
  --arg evidence_digest "$evidence_digest" \
  '{
    name: "tactus-frontier-review",
    head_sha: $reviewed_sha,
    status: "completed",
    conclusion: "success",
    details_url: $review_url,
    external_id: ("tactus-frontier-review:pr-" + $pr_number + ":" + $reviewed_sha + ":" + $evidence_digest),
    output: {
      title: "Independent frontier review passed",
      summary: ("PR #" + $pr_number + " passed independent frontier review for exact head `" + $reviewed_sha + "`.\n\nEvidence SHA-256: `" + $evidence_digest + "`."),
      text: ("Review evidence: " + $review_url)
    }
  }'
