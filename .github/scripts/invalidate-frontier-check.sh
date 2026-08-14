#!/usr/bin/env bash
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

if [[ $# -ne 3 ]]; then
  echo "usage: invalidate-frontier-check.sh <head-sha> <pr-number> <expected-app-id>" >&2
  exit 2
fi

head_sha="$1"
pr_number="$2"
expected_app_id="$3"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

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
if [[ ! "${GITHUB_REPOSITORY:-}" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  echo "GITHUB_REPOSITORY must be an owner/repository name" >&2
  exit 2
fi

pages="$(gh api --paginate --slurp \
  "repos/$GITHUB_REPOSITORY/commits/$head_sha/check-runs?check_name=tactus-frontier-review&app_id=$expected_app_id&filter=all&per_page=100" \
  -H 'Accept: application/vnd.github+json' \
  -H 'X-GitHub-Api-Version: 2026-03-10')"
check_runs="$(jq '{check_runs: [.[].check_runs[]]}' <<< "$pages")"
plan="$(printf '%s' "$check_runs" | bash "$script_dir/frontier-invalidation-plan.sh" \
  "$head_sha" "$pr_number" "$expected_app_id")"

invalidated=0
while IFS= read -r item; do
  [[ -n "$item" ]] || continue
  check_id="$(jq -r '.id' <<< "$item")"
  payload="$(jq -c '.payload' <<< "$item")"
  updated="$(gh api --method PATCH \
    "repos/$GITHUB_REPOSITORY/check-runs/$check_id" \
    -H 'Accept: application/vnd.github+json' \
    -H 'X-GitHub-Api-Version: 2026-03-10' \
    --input - <<< "$payload")"

  jq -e \
    --argjson check_id "$check_id" \
    --arg head_sha "$head_sha" \
    --argjson expected_app_id "$expected_app_id" \
    '.id == $check_id
      and .name == "tactus-frontier-review"
      and .head_sha == $head_sha
      and .app.id == $expected_app_id
      and .status == "completed"
      and .conclusion == "failure"' <<< "$updated" >/dev/null || {
        echo "GitHub did not return the expected failed App-owned check run" >&2
        exit 1
      }
  invalidated=$((invalidated + 1))
done < <(jq -c '.[]' <<< "$plan")

echo "invalidated $invalidated App-owned frontier check(s) on $head_sha"
