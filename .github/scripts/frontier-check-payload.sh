#!/usr/bin/env bash
set -euo pipefail

# Renders the App check payload. The attested SHA is the head the check is
# published on; the reviewed SHA is the head the frontier review actually read.
# They differ only under exempt-path drift, whose ancestor/exempt-only property
# is enforced by the trusted workflow BEFORE this script runs -- this script
# renders the claim, it does not judge it.
# decisions/2026-08-20-review-invalidation-scope.md

if [[ $# -ne 5 ]]; then
  echo "usage: frontier-check-payload.sh <attested-sha> <reviewed-sha> <pr-number> <review-url> <evidence-digest>" >&2
  exit 2
fi

attested_sha="$1"
reviewed_sha="$2"
pr_number="$3"
review_url="$4"
evidence_digest="$5"

if [[ ! "$attested_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "attested SHA must be 40 lowercase hexadecimal characters" >&2
  exit 2
fi
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

external_id="tactus-frontier-review:pr-$pr_number:$attested_sha:$evidence_digest"
if [[ "$attested_sha" == "$reviewed_sha" ]]; then
  summary="PR #$pr_number passed independent frontier review for exact head \`$attested_sha\`."
else
  external_id+=":reviewed:$reviewed_sha"
  summary="PR #$pr_number passed independent frontier review at \`$reviewed_sha\`. Attested for head \`$attested_sha\`: the trusted workflow verified the intervening diff is confined to the exempt set (\`reviews/FINDINGS.md\`)."
fi

jq -n \
  --arg attested_sha "$attested_sha" \
  --arg review_url "$review_url" \
  --arg evidence_digest "$evidence_digest" \
  --arg external_id "$external_id" \
  --arg summary "$summary" \
  '{
    name: "tactus-frontier-review",
    head_sha: $attested_sha,
    status: "completed",
    conclusion: "success",
    details_url: $review_url,
    external_id: $external_id,
    output: {
      title: "Independent frontier review passed",
      summary: ($summary + "\n\nEvidence SHA-256: `" + $evidence_digest + "`."),
      text: ("Review evidence: " + $review_url)
    }
  }'
