#!/usr/bin/env bash
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
planner="$script_dir/frontier-invalidation-plan.sh"
head_sha="0123456789abcdef0123456789abcdef01234567"
other_sha="89abcdef0123456789abcdef0123456789abcdef"
app_id=4574301

# Model the exact post-sign edit case: the dedicated App has already produced
# the successful required context on this unchanged head. Candidate-owned and
# stale-head lookalikes must remain untouched.
checks="$(jq -n \
  --arg head_sha "$head_sha" \
  --arg other_sha "$other_sha" \
  --argjson app_id "$app_id" \
  '{check_runs: [
    {id: 101, name: "upstroke-frontier-review", head_sha: $head_sha,
     status: "completed", conclusion: "success", app: {id: $app_id}},
    {id: 102, name: "upstroke-frontier-review", head_sha: $head_sha,
     status: "completed", conclusion: "success", app: {id: 15368}},
    {id: 103, name: "upstroke-frontier-review", head_sha: $other_sha,
     status: "completed", conclusion: "success", app: {id: $app_id}},
    {id: 104, name: "unrelated", head_sha: $head_sha,
     status: "completed", conclusion: "success", app: {id: $app_id}}
  ]}')"

plan="$(printf '%s' "$checks" | bash "$planner" "$head_sha" 14 "$app_id")"
jq -e '
  length == 1
  and .[0].id == 101
  and .[0].payload.status == "completed"
  and .[0].payload.conclusion == "failure"
  and .[0].payload.output.title == "Frontier review invalidated by PR metadata edit"
' <<< "$plan" >/dev/null

# Apply the planned PATCH to the fixture. The same App id, check name, check id,
# and head SHA which were successful before the edit are now conclusively
# failed; no weaker same-name context is substituted.
after="$(jq --argjson patch "$(jq '.[0]' <<< "$plan")" '
  .check_runs |= map(
    if .id == $patch.id then
      .status = $patch.payload.status
      | .conclusion = $patch.payload.conclusion
    else . end
  )
' <<< "$checks")"
jq -e \
  --arg head_sha "$head_sha" \
  --argjson app_id "$app_id" \
  '[.check_runs[] | select(
     .name == "upstroke-frontier-review"
     and .head_sha == $head_sha
     and .app.id == $app_id
   )] == [{
     id: 101,
     name: "upstroke-frontier-review",
     head_sha: $head_sha,
     status: "completed",
     conclusion: "failure",
     app: {id: $app_id}
   }]' <<< "$after" >/dev/null

if printf '%s' "$checks" | bash "$planner" "${head_sha^^}" 14 "$app_id" >/dev/null 2>&1; then
  echo "uppercase head SHA unexpectedly accepted" >&2
  exit 1
fi
if printf '%s' '{"check_runs":{}}' | bash "$planner" "$head_sha" 14 "$app_id" >/dev/null 2>&1; then
  echo "malformed check-run collection unexpectedly accepted" >&2
  exit 1
fi

echo "frontier post-sign invalidation fixtures: PASS"
