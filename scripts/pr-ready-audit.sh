#!/usr/bin/env bash
# pr-ready-audit.sh: decide, per open pull request, whether it is ready to enqueue for merge
# under the three-lane finding policy, and optionally maintain the lane and ready labels.
#
#   scripts/pr-ready-audit.sh [--apply] [--enqueue] [--ready-label NAME] [--reviewer LOGIN] [PR ...]
#
# --apply maintains the lane:* and ready label on each pull request. --enqueue adds every READY,
# un-drafted pull request to the merge queue (`gh pr merge --merge --auto`) in the order listed,
# which is the priority order; it implies --apply.
#
# Lanes, decided by the branch prefix and nothing else (a lane:* label is output, never input;
# a wrong one is corrected by --apply and reported as lane-label-mismatch):
#   lane:findings-p3     codex/findings-p3-*   must fix everything; ready only on a PASS verdict
#   lane:findings-p1p2   codex/findings-*      must fix P0-P2; P3 may be filed and deferred
#   lane:feature         everything else       must fix P0-P1; P2-P3 may be filed and deferred
#
# A pull request is READY when all of these hold on its current head:
#   - not a draft, no merge conflicts
#   - the latest `upstroke-ci` and `upstroke-pr-policy` check runs on the head succeeded
#   - the latest review comment (either the `Reviewed head:` workflow form or the
#     `<!-- upstroke-frontier-review -->` form) reviewed the head itself, or a commit the head
#     differs from only by merge commits and pushes confined to reviews/findings/ or
#     reviews/FINDINGS.md (MAINTAINING step 5 keeps the review across both)
#   - no finding in that review has a severity the lane must fix
#   - every allowed finding has a ledger row in the body whose disposition is deferred (with a
#     file under reviews/findings/ on the branch naming the finding id), rejected or accepted-risk
#
# Other states: NEEDS-ATTEST (the head moved past the reviewed commit by more than merges and
# ledger pushes: a repair-only push the owner reads and attests under step 5, or a new change that
# needs another pass), MANUAL (the review carries findings without ids, so rows cannot be matched
# mechanically), and NOT-READY with the blockers listed.
#
# Only review comments posted by the trusted reviewer account count: the repository owner by
# default, or --reviewer LOGIN. Anyone else's comment carrying the markers is ignored, so a
# contributor cannot mint a PASS.
#
# Limits, stated so nobody reads more into READY than it says: the audit sees severities and the
# fields the review JSON carries. A finding whose object carries a non-empty witness,
# reproduction, repro, failing_test or mutation field blocks in every lane; a witness that exists
# only in prose does not reach the audit, and the deferring implementor's row asserts there is
# none (MAINTAINING step 5 binds them). A master merge-in is checked for leaving no branch commit
# outside the ledger, not for a byte-identical branch diff.
#
# Needs: bash, git (a checkout with `origin` pointing at the repository), gh (its built-in --jq
# does all JSON work; a standalone jq is not required).

set -euo pipefail

apply=0
enqueue=0
ready_label="ready-to-merge"
reviewer=""
prs=()
while (($#)); do
  case "$1" in
    --apply) apply=1 ;;
    --enqueue) apply=1; enqueue=1 ;;
    --ready-label) ready_label="$2"; shift ;;
    --reviewer) reviewer="$2"; shift ;;
    -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
    *) prs+=("$1") ;;
  esac
  shift
done

repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
[[ -n "$reviewer" ]] || reviewer="$(gh repo view "$repo" --json owner --jq .owner.login)"

if ((${#prs[@]} == 0)); then
  mapfile -t prs < <(gh pr list --repo "$repo" --state open --limit 100 --json number --jq '.[].number')
fi

ensure_labels() {
  gh label create lane:feature --repo "$repo" --color 0e8a16 --force \
    --description "feature or sweep work: fix P0-P1, file P2-P3" >/dev/null
  gh label create lane:findings-p1p2 --repo "$repo" --color fbca04 --force \
    --description "P1/P2 findings workflow: fix P0-P2, file P3" >/dev/null
  gh label create lane:findings-p3 --repo "$repo" --color d93f0b --force \
    --description "P3 findings workflow: ready only on PASS" >/dev/null
  gh label create "$ready_label" --repo "$repo" --color 5319e7 --force \
    --description "audit passed: enqueue for merge" >/dev/null
}

lane_for() {
  local branch="$1"
  case "$branch" in
    codex/findings-p3-*) echo findings-p3 ;;
    codex/findings-*) echo findings-p1p2 ;;
    *) echo feature ;;
  esac
}

must_fix_for() {
  case "$1" in
    findings-p3) echo "P0 P1 P2 P3" ;;
    findings-p1p2) echo "P0 P1 P2" ;;
    feature) echo "P0 P1" ;;
  esac
}

# The latest review comment on a pull request posted by the trusted reviewer, or nothing.
latest_review() {
  gh api "repos/$repo/issues/$1/comments" --paginate \
    --jq "[.[] | select(.user.login == \"$reviewer\") | select(.body | test(\"<!-- upstroke-frontier-review|Reviewed head: [0-9a-f]{40}\"))] | last | .body // empty"
}

# Ledger rows from the body: "ID<TAB>severity<TAB>sha-or-location<TAB>disposition" per row.
ledger_rows() {
  gh pr view "$1" --repo "$repo" --json body --jq .body | awk -F'|' '
    /^## Review finding ledger/ { inledger = 1; next }
    /^## / { inledger = 0 }
    inledger && /^\|/ && $2 !~ /^ *ID *$/ && $2 !~ /^-+$/ && $2 !~ /None yet/ {
      gsub(/^ +| +$/, "", $2); gsub(/^ +| +$/, "", $3); gsub(/^ +| +$/, "", $4); gsub(/^ +| +$/, "", $10)
      print $2 "\t" $3 "\t" $4 "\t" $10
    }'
}

if ((apply)); then ensure_labels; fi

printf '%-5s %-14s %-8s %-13s %s\n' PR LANE HEAD STATE DETAIL
for pr in "${prs[@]}"; do
  meta="$(gh pr view "$pr" --repo "$repo" \
    --json headRefName,headRefOid,isDraft,mergeStateStatus,labels \
    --jq '[.headRefName, .headRefOid, (.isDraft|tostring), .mergeStateStatus, ([.labels[].name]|join(" "))] | @tsv')"
  IFS=$'\t' read -r branch head draft merge_state labels <<< "$meta"
  lane="$(lane_for "$branch")"
  must_fix="$(must_fix_for "$lane")"
  blockers=()
  state=READY

  [[ "$draft" == "true" ]] && blockers+=("draft")
  [[ "$merge_state" == "DIRTY" ]] && blockers+=("conflicts")

  # Latest check runs on the head, one per required context.
  checks="$(gh api "repos/$repo/commits/$head/check-runs?per_page=100" \
    --jq '[.check_runs[] | select(.name == "upstroke-ci" or .name == "upstroke-pr-policy") | "\(.name)=\(.conclusion // .status)"] | join(" ")')"
  for ctx in upstroke-ci upstroke-pr-policy; do
    case " $checks " in
      *" $ctx=success "*) ;;
      *" $ctx="*) blockers+=("$ctx:${checks##*"$ctx="}"); blockers[-1]="${blockers[-1]%% *}" ;;
      *) blockers+=("$ctx:missing") ;;
    esac
  done

  review="$(latest_review "$pr")"
  if [[ -z "$review" ]]; then
    blockers+=("no-review")
    reviewed=""
    verdict=""
  else
    reviewed="$(grep -oE '(head=|Reviewed head: )[0-9a-f]{40}' <<< "$review" | head -1 | grep -oE '[0-9a-f]{40}' || true)"
    verdict="$(grep -oE '("verdict": ?"|VERDICT:\**:? *)[A-Z_]+' <<< "$review" | head -1 | grep -oE '[A-Z_]+$' || true)"
    case "$verdict" in
      PASS|CHANGES_REQUIRED) ;;
      "") blockers+=("no-verdict") ;;
      *) blockers+=("verdict:$verdict") ;;
    esac
    [[ "$lane" == findings-p3 && "$verdict" != PASS ]] && blockers+=("verdict-not-pass")
  fi

  # Head movement since the reviewed commit.
  moved=""
  if [[ -n "$reviewed" ]]; then
    git fetch -q origin "$branch" master 2>/dev/null || true
    if ! git cat-file -e "$reviewed^{commit}" 2>/dev/null; then
      blockers+=("reviewed-sha-unknown:${reviewed:0:7}")
    elif ! git merge-base --is-ancestor "$reviewed" "$head"; then
      blockers+=("reviewed-not-ancestor:${reviewed:0:7}")
    elif [[ "$reviewed" != "$head" ]]; then
      # Commits master already has arrived through a merge-in; only the branch's own count.
      touched="$(git log --no-merges --name-only --format= "$reviewed..$head" --not origin/master | sort -u)"
      if [[ -z "$touched" ]]; then
        moved="merges-only"
      elif ! grep -vE '^reviews/(findings/|FINDINGS\.md$)' <<< "$touched" >/dev/null; then
        moved="ledger-only"
      else
        moved="repairs"
      fi
    fi
  fi

  # Findings in the latest review, as "severity<TAB>id<TAB>witnessed" (id empty for the
  # frontier form). One finding object per line, so a witness field is read from its own object.
  findings=()
  if [[ -n "$review" ]]; then
    if grep -q '"findings"' <<< "$review"; then
      while IFS=$'\t' read -r sev id wit; do
        [[ -n "$sev" ]] && findings+=("$sev"$'\t'"$id"$'\t'"$wit")
      done < <(grep -oE '"findings": ?\[.*' <<< "$review" | sed -E 's/\},\{/}\n{/g' | awk '
        {
          sev = ""; id = ""; wit = 0
          if (match($0, /"severity": ?"P[0-3]"/)) sev = substr($0, RSTART + RLENGTH - 3, 2)
          if (match($0, /"id": ?"[^"]+"/)) { id = substr($0, RSTART, RLENGTH); sub(/^"id": ?"/, "", id); sub(/"$/, "", id) }
          if ($0 ~ /"(witness|reproduction|repro|failing_test|mutation|mutation_witness)": ?"[^"]+"/) wit = 1
          if (id == "") id = "-"
          if (sev != "") print sev "\t" id "\t" wit
        }')
    else
      while read -r sev; do findings+=("$sev"$'\t'-$'\t'0); done < <(grep -oE '^[0-9]+\. \*\*P[0-3]' <<< "$review" | grep -oE 'P[0-3]')
    fi
  fi
  [[ "$verdict" == PASS && ${#findings[@]} -gt 0 ]] && blockers+=("pass-with-findings")

  rows="$(ledger_rows "$pr")"
  for f in "${findings[@]}"; do
    IFS=$'\t' read -r sev id wit <<< "$f"
    [[ "$id" == "-" ]] && id=""   # "-" stands in for "no id": a tab-separated read collapses empty fields
    if [[ "$wit" == 1 ]]; then
      blockers+=("witnessed:${id:-unnamed}")
      continue
    fi
    if [[ " $must_fix " == *" $sev "* ]]; then
      blockers+=("open-$sev:${id:-unnamed}")
      continue
    fi
    if [[ -z "$id" ]]; then
      blockers+=("manual:$sev-without-id")
      continue
    fi
    disposition="$(awk -F'\t' -v id="$id" '$1 == id { print $4; exit }' <<< "$rows")"
    case "$disposition" in
      deferred)
        if ! git grep -q -F "$id" "$head" -- reviews/findings 2>/dev/null; then
          blockers+=("no-file:$id")
        fi ;;
      rejected|accepted-risk) ;;
      fixed)
        [[ "$moved" != "repairs" ]] && blockers+=("fixed-but-head-unmoved:$id") ;;
      "") blockers+=("no-row:$id") ;;
      *) blockers+=("bad-disposition:$id=$disposition") ;;
    esac
  done

  # Hard blockers decide first; a repair push only matters once nothing else stands in the way.
  # Draft is not a blocker: the implementor un-drafts when the audit says READY.
  hard=()
  for b in "${blockers[@]:-}"; do
    [[ -n "$b" && "$b" != draft && "$b" != manual:* ]] && hard+=("$b")
  done
  if ((${#hard[@]})); then
    state=NOT-READY
  elif [[ "$moved" == "repairs" ]]; then
    state=NEEDS-ATTEST
  elif printf '%s\n' "${blockers[@]:-}" | grep -q '^manual:'; then
    state=MANUAL
  fi

  detail="verdict=${verdict:-none} reviewed=${reviewed:0:7}${moved:+ moved=$moved}"
  for l in lane:feature lane:findings-p1p2 lane:findings-p3; do
    [[ "$l" != "lane:$lane" && " $labels " == *" $l "* ]] && detail+=" lane-label-mismatch=$l"
  done
  ((${#blockers[@]})) && detail+=" blockers=$(IFS=,; echo "${blockers[*]}")"
  printf '%-5s %-14s %-8s %-13s %s\n' "#$pr" "$lane" "${head:0:7}" "$state" "$detail"

  if ((apply)); then
    for l in lane:feature lane:findings-p1p2 lane:findings-p3; do
      [[ "$l" != "lane:$lane" && " $labels " == *" $l "* ]] && gh pr edit "$pr" --repo "$repo" --remove-label "$l" >/dev/null
    done
    [[ " $labels " != *" lane:$lane "* ]] && gh pr edit "$pr" --repo "$repo" --add-label "lane:$lane" >/dev/null
    if [[ "$state" == READY && "$draft" != "true" ]]; then
      [[ " $labels " != *" $ready_label "* ]] && gh pr edit "$pr" --repo "$repo" --add-label "$ready_label" >/dev/null
      if ((enqueue)); then
        if gh pr merge "$pr" --repo "$repo" --merge --auto >/dev/null 2>&1; then
          echo "      enqueued #$pr"
        else
          echo "      could not enqueue #$pr (already queued, or the ruleset has no merge queue yet)"
        fi
      fi
    else
      [[ " $labels " == *" $ready_label "* ]] && gh pr edit "$pr" --repo "$repo" --remove-label "$ready_label" >/dev/null
    fi
  fi
done
