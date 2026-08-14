#!/usr/bin/env bash
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

if [[ $# -ne 3 ]]; then
  echo "usage: frontier-invalidation-plan.sh <head-sha> <pr-number> <expected-app-id>" >&2
  exit 2
fi

head_sha="$1"
pr_number="$2"
expected_app_id="$3"

if [[ ! "$head_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "head SHA must be 40 lowercase hexadecimal characters" >&2
  exit 2
fi
if [[ ! "$pr_number" =~ ^[1-9][0-9]*$ ]]; then
  echo "pull-request number must be a positive integer" >&2
  exit 2
fi
if [[ ! "$expected_app_id" =~ ^[1-9][0-9]*$ ]]; then
  echo "expected App id must be a positive integer" >&2
  exit 2
fi

# The caller supplies GitHub's check-runs response on stdin. Updating the
# existing App-owned run is stronger than publishing a same-named Actions
# result: it turns the exact check which satisfied branch protection into a
# failure while preserving its evidence URL and external id for audit.
jq -e \
  --arg head_sha "$head_sha" \
  --arg pr_number "$pr_number" \
  --argjson expected_app_id "$expected_app_id" \
  '
    if (.check_runs | type) != "array" then
      error("check_runs must be an array")
    else
      [
        .check_runs[]
        | select(
            .name == "tactus-frontier-review"
            and .head_sha == $head_sha
            and .app.id == $expected_app_id
            and (.status != "completed" or .conclusion == "success")
          )
        | {
            id,
            payload: {
              status: "completed",
              conclusion: "failure",
              output: {
                title: "Frontier review invalidated by PR metadata edit",
                summary: (
                  "PR #" + $pr_number
                  + " changed its title or body after this exact-head review was attested. "
                  + "Run a fresh trusted frontier review and attestation against the current metadata."
                )
              }
            }
          }
      ]
    end
  '
