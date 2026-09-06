#!/usr/bin/env bash
# pr-ready-audit.sh: decide, per open pull request, whether it is ready to enqueue for merge
# under the three-lane finding policy, and optionally maintain the lane and ready labels.
#
#   scripts/pr-ready-audit.sh [--apply] [--enqueue] [--ready-label NAME] [PR ...]
#
# --apply maintains the lane:* and ready label on each pull request. --enqueue adds every READY
# pull request to the merge queue (`gh pr merge --merge --auto`) in the order the arguments give,
# so the caller states the priority; with no arguments it walks `gh pr list` order, newest first,
# which is not a priority. It implies --apply.
#
# --ready-label NAME uses an existing label of that name as it is (the audit adds and removes it
# on pull requests and never recolours or redescribes it) and creates it only when absent.
#
# Lanes, decided by the branch prefix and nothing else (a lane:* label is output, never input;
# a wrong one is corrected by --apply and reported as lane-label-mismatch):
#   lane:findings-p3     codex/findings-p3-*   must fix everything; ready only on a PASS verdict
#   lane:findings-p1p2   codex/findings-*      must fix P0-P2; P3 may be filed and deferred
#   lane:feature         everything else       must fix P0-P1; P2-P3 may be filed and deferred
#
# A pull request is READY when all of these hold on its current head:
#   - not a draft; GitHub reports it mergeable (DIRTY, UNKNOWN and BLOCKED all fail closed)
#   - the latest `upstroke-ci` and `upstroke-pr-policy` check runs on the head succeeded
#   - the latest review comment (either the `Reviewed head:` workflow form or the
#     `<!-- upstroke-frontier-review -->` form) reviewed the head itself, or a commit the head
#     differs from only by merge commits and pushes confined to reviews/findings/ or
#     reviews/FINDINGS.md (MAINTAINING step 5 keeps the review across both)
#   - no finding in that review has a severity the lane must fix
#   - every allowed finding has a ledger row in the body whose disposition is deferred (with a
#     file under reviews/findings/ on the branch naming the finding id), rejected or accepted-risk
#
# Other states: NEEDS-ATTEST (the head moved past the reviewed commit by more than clean merges
# and ledger pushes: a repair-only push the owner reads and attests under step 5, a merge commit
# that is not git's own merge of its parents, or a new change that needs another pass), MANUAL
# (the review is prose the audit cannot judge: findings without ids, or a severity token outside
# the numbered findings, so a person reads it), and NOT-READY with the blockers listed.
#
# Only review comments posted by the repository owner count, and there is no override: the
# owner is MAINTAINING's one trusted writer, and anyone else's comment carrying the markers is
# ignored, so a contributor cannot mint a PASS.
#
# Limits, stated so nobody reads more into READY than it says: the audit sees severities and the
# fields the review JSON carries. A finding whose object carries a witness, reproduction, repro,
# failing_test or mutation field that is not null, false or empty blocks in every lane, and so
# does one whose fields name a MUST deviation (a field named mandatory/deviation/must_*, or the
# word MUST in any string field); a witness or deviation that exists only in prose does not
# reach the audit, and the deferring implementor's row asserts there is none (MAINTAINING step 5
# binds them). A master merge-in is checked for being git's own merge of its parents and for
# leaving no branch commit outside the ledger.
#
# Needs: bash, git (a checkout with `origin` pointing at the repository), gh (its built-in --jq
# does all JSON work; a standalone jq is not required).

set -euo pipefail

apply=0
enqueue=0
ready_label="ready-to-merge"
prs=()
while (($#)); do
  case "$1" in
    --apply) apply=1 ;;
    --enqueue) apply=1; enqueue=1 ;;
    --ready-label) ready_label="$2"; shift ;;
    -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
    *) prs+=("$1") ;;
  esac
  shift
done

repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
reviewer="$(gh repo view "$repo" --json owner --jq .owner.login)"

if ((${#prs[@]} == 0)); then
  mapfile -t prs < <(gh pr list --repo "$repo" --state open --limit 100 --json number --jq '.[].number')
fi

# Creates a label only when the repository has none of that name: an existing label, including
# one handed in as --ready-label, keeps its colour and description.
ensure_labels() {
  local existing
  existing="$(gh label list --repo "$repo" --limit 200 --json name --jq '.[].name')"
  create() {
    grep -qxF "$1" <<< "$existing" \
      || gh label create "$1" --repo "$repo" --color "$2" --description "$3" >/dev/null
  }
  create lane:feature 0e8a16 "feature or sweep work: fix P0-P1, file P2-P3"
  create lane:findings-p1p2 fbca04 "P1/P2 findings workflow: fix P0-P2, file P3"
  create lane:findings-p3 d93f0b "P3 findings workflow: ready only on PASS"
  create "$ready_label" 5319e7 "audit passed: enqueue for merge"
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

# The latest review comment on a pull request posted by the repository owner, or nothing.
# `--paginate` hands `--jq` each page separately, so `last` is per page: each page yields its
# newest match as "<created_at> <id>", the newest across pages wins by timestamp, and that one
# comment is fetched by id. Comments arrive oldest first, so the per-page `last` is its newest.
latest_review() {
  local id
  id="$(gh api "repos/$repo/issues/$1/comments?per_page=100" --paginate \
    --jq "[.[] | select(.user.login == \"$reviewer\") | select(.body | test(\"<!-- upstroke-frontier-review|Reviewed head: [0-9a-f]{40}\"))] | last | select(. != null) | \"\(.created_at) \(.id)\"" \
    | sort | tail -1 | awk '{print $2}')"
  [[ -n "$id" ]] && gh api "repos/$repo/issues/comments/$id" --jq '.body'
  return 0
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
  # GitHub computes mergeability lazily and answers UNKNOWN until it has; asking again after a
  # pause usually settles it, and an UNKNOWN that survives three asks fails closed below.
  for attempt in 1 2 3; do
    meta="$(gh pr view "$pr" --repo "$repo" \
      --json headRefName,headRefOid,isDraft,mergeStateStatus,labels \
      --jq '[.headRefName, .headRefOid, (.isDraft|tostring), .mergeStateStatus, ([.labels[].name]|join(" "))] | @tsv')"
    IFS=$'\t' read -r branch head draft merge_state labels <<< "$meta"
    [[ "$merge_state" == UNKNOWN && $attempt -lt 3 ]] || break
    sleep 3
  done
  lane="$(lane_for "$branch")"
  must_fix="$(must_fix_for "$lane")"
  blockers=()
  state=READY

  [[ "$draft" == "true" ]] && blockers+=("draft")
  case "$merge_state" in
    DIRTY) blockers+=("conflicts") ;;
    UNKNOWN|"") blockers+=("mergeability-unknown") ;;   # GitHub has not computed it yet
    BLOCKED) blockers+=("blocked-by-rules") ;;          # a ruleset requirement is unmet, e.g. an unresolved conversation
  esac

  # The newest check run per required context on the head: a re-run creates a new run with the
  # same name, so the runs are grouped by name and the latest start wins before its conclusion
  # is read; an older success never outranks a newer failure.
  checks="$(gh api "repos/$repo/commits/$head/check-runs?per_page=100" --paginate \
    --jq '[.check_runs[] | select(.name == "upstroke-ci" or .name == "upstroke-pr-policy")] | group_by(.name) | map(max_by(.started_at // "")) | map("\(.name)=\(.conclusion // .status)") | join(" ")' \
    | tr '\n' ' ')"
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
      # Every merge commit the branch added must be exactly what git produces from its two
      # parents on its own: a merge carrying hand edits, a conflict resolution or a third parent
      # is a new change (MAINTAINING step 5) that `--no-merges` below would otherwise hide.
      merge_edits=""
      for m in $(git rev-list --merges "$reviewed..$head" --not origin/master); do
        if (($(git rev-list --parents -n 1 "$m" | wc -w) != 3)); then merge_edits="$m"; break; fi
        if ! expected="$(git merge-tree --write-tree "$m^1" "$m^2" 2>/dev/null)"; then
          merge_edits="$m"; break   # a conflict, or a git too old for --write-tree: fail closed
        fi
        [[ "$(git rev-parse "$m^{tree}")" == "$expected" ]] || { merge_edits="$m"; break; }
      done
      # Commits master already has arrived through a merge-in; only the branch's own count.
      touched="$(git log --no-merges --name-only --format= "$reviewed..$head" --not origin/master | sort -u)"
      if [[ -n "$merge_edits" ]]; then
        moved="merge-edits:${merge_edits:0:7}"
      elif [[ -z "$touched" ]]; then
        moved="merges-only"
      elif ! grep -vE '^reviews/(findings/|FINDINGS\.md$)' <<< "$touched" >/dev/null; then
        moved="ledger-only"
      else
        moved="repairs"
      fi
    fi
  fi

  # Findings in the latest review, as "severity<TAB>id<TAB>witnessed" ("-" for no id). The JSON
  # form is read by a JSON parser, never by regex; without one the audit fails closed. The
  # frontier form is prose, read conservatively: numbered "N. **P<n>" findings are taken, and any
  # severity token anywhere else in the text sends the pull request to MANUAL, PASS included.
  findings=()
  if [[ -n "$review" ]]; then
    if grep -q '"findings"' <<< "$review"; then
      py="$(command -v python3 || command -v python || true)"
      if [[ -z "$py" ]]; then
        blockers+=("findings-unparsed:no-python")
      else
        review_file="$(mktemp)"
        printf '%s' "$review" > "$review_file"
        while IFS=$'\t' read -r sev id wit; do
          [[ -n "$sev" ]] && findings+=("$sev"$'\t'"$id"$'\t'"$wit")
        done < <("$py" - "$review_file" <<'PY'
import json, re, sys
sys.stdout.reconfigure(newline=chr(10))  # a Windows python writes CRLF to a pipe, and bash would read the CR into the last field
text = open(sys.argv[1], encoding="utf-8").read()
# The verdict is the last fenced JSON object; the older bare form has no fence.
found = re.findall(r"```json\s*(\{.*?\})\s*```", text, re.S) or re.findall(r"(\{\"role_understanding.*\})", text, re.S)
try:
    verdict = json.loads(found[-1]) if found else None
except ValueError:
    verdict = None
if not isinstance(verdict, dict) or not isinstance(verdict.get("findings"), list):
    print("ERR\tunparsed\t0")
    sys.exit(0)
for f in verdict["findings"]:
    if not isinstance(f, dict):
        print("ERR\tunparsed\t0")
        continue
    sev = str(f.get("severity", "")).strip()
    fid = str(f.get("id", "")).strip() or "-"
    def present(v):
        return v not in (None, False, "", [], {}) and str(v).strip() != ""
    wit = int(any(present(f.get(k)) for k in ("witness", "reproduction", "repro", "failing_test", "mutation", "mutation_witness")))
    # A MUST deviation is fixed whatever its label (MAINTAINING step 5): any field of the finding
    # naming MUST as a word, or a field whose name says mandatory/deviation, marks it.
    must = 0
    for k, v in f.items():
        if re.search(r"(mandatory|deviation|must_)", str(k), re.I) and present(v):
            must = 1
        if isinstance(v, str) and re.search(r"\bMUST\b", v):
            must = 1
    if not re.fullmatch(r"P[0-3]", sev):
        print("ERR\tbad-severity:" + fid + "\t0")
        continue
    print(sev + "\t" + fid + "\t" + str(wit + 2 * must))
PY
)
        rm -f "$review_file"
      fi
    else
      while read -r sev; do findings+=("$sev"$'\t'-$'\t'0); done < <(grep -oE '^[0-9]+\. \*\*P[0-3]' <<< "$review" | grep -oE 'P[0-3]')
      stray="$(grep -vE '^[0-9]+\. \*\*P[0-3]' <<< "$review" | grep -oE '\bP[0-3]\b|\bMUST\b' | sort -u | tr '\n' '/' | sed 's#/$##' || true)"
      [[ -n "$stray" ]] && blockers+=("manual:$stray-outside-numbered-findings")
    fi
  fi
  [[ "$verdict" == PASS && ${#findings[@]} -gt 0 ]] && blockers+=("pass-with-findings")
  [[ "$verdict" == CHANGES_REQUIRED && ${#findings[@]} -eq 0 ]] && blockers+=("findings-unparsed:changes-required-lists-none")

  rows="$(ledger_rows "$pr")"
  for f in "${findings[@]}"; do
    IFS=$'\t' read -r sev id wit <<< "$f"
    [[ "$id" == "-" ]] && id=""   # "-" stands in for "no id": a tab-separated read collapses empty fields
    if [[ "$sev" == ERR ]]; then
      blockers+=("findings-unparsed:$id")
      continue
    fi
    # wit is a bit field from the parser: 1 = a witness field is present, 2 = a MUST deviation.
    if (( wit & 2 )); then
      blockers+=("must-deviation:${id:-unnamed}")
      continue
    fi
    if (( wit & 1 )); then
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
        [[ "$moved" == repairs || "$moved" == merge-edits:* ]] || blockers+=("fixed-but-head-unmoved:$id") ;;
      "") blockers+=("no-row:$id") ;;
      *) blockers+=("bad-disposition:$id=$disposition") ;;
    esac
  done

  # Hard blockers decide first; a repair push only matters once nothing else stands in the way.
  # A draft is NOT-READY: READY means enqueueable as it stands.
  hard=()
  for b in "${blockers[@]:-}"; do
    [[ -n "$b" && "$b" != manual:* ]] && hard+=("$b")
  done
  if ((${#hard[@]})); then
    state=NOT-READY
  elif [[ "$moved" == repairs || "$moved" == merge-edits:* ]]; then
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
        # --match-head-commit binds the enqueue to the head this audit judged: a push that
        # lands between the audit and this call makes GitHub refuse, never enqueue the newcomer.
        if gh pr merge "$pr" --repo "$repo" --merge --auto --match-head-commit "$head" >/dev/null 2>&1; then
          echo "      enqueued #$pr at ${head:0:7}"
        else
          echo "      could not enqueue #$pr at ${head:0:7} (head moved, already queued, or the ruleset has no merge queue yet)"
        fi
      fi
    else
      [[ " $labels " == *" $ready_label "* ]] && gh pr edit "$pr" --repo "$repo" --remove-label "$ready_label" >/dev/null
    fi
  fi
done
