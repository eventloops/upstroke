#!/usr/bin/env bash
# pr-ready-audit.sh: decide, per open pull request, whether it is ready to enqueue for merge
# under the three-lane finding policy, and optionally maintain the lane and ready labels.
#
#   scripts/pr-ready-audit.sh [--apply] [--enqueue] [--ready-label NAME] [PR ...]
#
# NAME must not be a lane:* label. The ready label is advisory: it reports that this audit found
# the head it read READY. A GitHub label is not bound to a commit, so the head is read again
# before the label is written and again after, and a label written across a push is removed
# (HEAD-MOVED); that narrows the race, it cannot close it. The act bound to the audited head is
# the enqueue, through `gh pr merge --match-head-commit`, and the base is read again just before
# it (BASE-MOVED); nothing may treat the label alone as permission to merge.
#
# --apply maintains the lane:* and ready label on each pull request. --enqueue adds every READY
# pull request to the merge queue (`gh pr merge --merge --auto`) in the order the arguments give,
# so the caller states the priority; with no arguments it walks the open pull requests in the
# API's order, which is not a priority. It implies --apply.
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
# A pull request is READY when all of these hold on its current head and base:
#   - not a draft; GitHub reports it mergeable: DIRTY, UNKNOWN and BLOCKED fail closed, and
#     BEHIND fails closed while the default-branch ruleset requires an up-to-date branch (the
#     ruleset is read; once the merge queue replaces that requirement, BEHIND is what the queue
#     exists to handle and no longer blocks)
#   - the newest `upstroke-ci` and `upstroke-pr-policy` check runs on the head succeeded
#   - the latest review comment by the repository owner (the `Reviewed head:` workflow form with
#     its fenced JSON verdict, or the `<!-- upstroke-frontier-review -->` prose form) reviewed
#     the head itself, or a commit the head differs from only by clean merge commits (git's own
#     merge of the two parents, the branch diff byte-identical before and after, and no gate
#     edited by the pull request) and pushes confined to reviews/findings/ or reviews/FINDINGS.md
#     (MAINTAINING step 5 keeps the review across both)
#   - the review is against the pull request's own base: the workflow form must record its base
#     commit, which must lie on the current base branch (and, for a base other than master, not
#     on master); no base change may be recorded on the pull request after the review was
#     posted, which is checked again just before an enqueue; the prose form records no base, so
#     it is bound to the base only by that timeline check and counts only on a master-based
#     pull request
#   - no finding in that review has a severity the lane must fix
#   - every allowed finding has a ledger row in the body whose disposition is deferred, with
#     exactly one file under reviews/findings/ on the branch whose YAML frontmatter (the block
#     between the opening --- and the next) carries `id: <the finding id>`; a rejected or
#     accepted-risk row is the owner's call and sends the pull request to MANUAL
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
# binds them). A merge-in is checked on step 5's terms: git's own merge of its parents, the
# branch diff byte-identical before and after, no gate edited by the pull request, and no branch
# commit outside the ledger.
#
# The pure parts (lane, severity sets, the two review parsers, the frontmatter id match, the
# newest-check-run choice, the ledger-row parse) are functions, exercised by
# .github/scripts/test-pr-ready-audit.sh; sourcing this file with PR_READY_AUDIT_LIBRARY=1
# defines them without running the audit.
#
# Needs: bash, git (a checkout with `origin` pointing at the repository), gh (its built-in --jq
# does the API-side JSON work), and python3 or python for the verdict JSON; without a python
# the JSON form fails closed.

set -euo pipefail

# ---- pure helpers -------------------------------------------------------------------------------

lane_for() {
  case "$1" in
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

# review_kind FILE: "json" when the comment carries a fenced ```json verdict (or the older bare
# role_understanding object), "prose" otherwise. A prose review that merely quotes JSON is prose.
review_kind() {
  if grep -qE '^```json|"role_understanding"' "$1"; then echo json; else echo prose; fi
}

# parse_verdict_json FILE: the workflow form. Prints tab-separated lines:
#   META <reviewed_sha> <verdict> <base_sha>     identity of the one object the findings come from
#   STRAY <tokens> 0                            severity or MUST tokens found outside that object
#   <severity> <id or -> <bits>                 one per finding; bits: 1 = witness field, 2 = MUST
#   ERR <reason> 0                              an object or finding the parser cannot judge
parse_verdict_json() {
  local py
  py="$(command -v python3 || command -v python || true)"
  if [[ -z "$py" ]]; then
    printf 'ERR\tno-python\t0\n'
    return 0
  fi
  "$py" - "$1" <<'PY'
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
# Identity, verdict, base and findings from this one object. Anything in the comment outside the
# object that looks like a finding is for a person, not for the parser.
print("META\t" + str(verdict.get("reviewed_sha", "")).strip() + "\t" + str(verdict.get("verdict", "")).strip() + "\t" + (str(verdict.get("base_sha", "")).strip() or "-"))
outside = text.replace(found[-1], "")
stray = sorted(set(re.findall(r"\b(?:P[0-3]|MUST)\b", outside)))
if stray:
    print("STRAY\t" + "/".join(stray) + "\t0")
def present(v):
    return v not in (None, False, "", [], {}) and str(v).strip() != ""
for f in verdict["findings"]:
    if not isinstance(f, dict):
        print("ERR\tunparsed\t0")
        continue
    sev = str(f.get("severity", "")).strip()
    fid = str(f.get("id", "")).strip() or "-"
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
}

# parse_prose_review FILE: the frontier form, read conservatively. Prints the same shape:
#   META <head from the marker or Reviewed-head line> <last VERDICT> -
#   <severity> - 0     one per numbered "N. **P<n>" finding
#   STRAY <tokens> 0   any P0-P3 or MUST token outside the numbered findings, PASS included
parse_prose_review() {
  local f="$1" head verdict stray
  head="$(grep -oE '(head=|Reviewed head: )[0-9a-f]{40}' "$f" | head -1 | grep -oE '[0-9a-f]{40}' || true)"
  verdict="$(grep -oE 'VERDICT:\**:? *[A-Z_]+' "$f" | tail -1 | grep -oE '[A-Z_]+$' || true)"
  printf 'META\t%s\t%s\t-\n' "$head" "$verdict"
  grep -oE '^[0-9]+\. \*\*P[0-3]' "$f" | grep -oE 'P[0-3]' | sed 's/$/\t-\t0/' || true
  stray="$(grep -vE '^[0-9]+\. \*\*P[0-3]' "$f" | grep -oE '\bP[0-3]\b|\bMUST\b' | sort -u | tr '\n' '/' | sed 's#/$##' || true)"
  [[ -n "$stray" ]] && printf 'STRAY\t%s\t0\n' "$stray"
  return 0
}

# frontmatter_has_id ID: reads a finding file on stdin and succeeds only when its YAML
# frontmatter, the block between the opening `---` on line 1 and the next `---`, carries the
# line `id: ID` (README: the id lives in the frontmatter, not the name). The same line in prose
# or a code block further down is not a frontmatter id. The id is a fixed string, whole line.
frontmatter_has_id() {
  awk 'NR == 1 { if ($0 != "---") exit; next } $0 == "---" { exit } { print }' | grep -qxF "id: $1"
}

# newest_per_name: reads "name<TAB>id<TAB>conclusion-or-status" lines, one per check run across
# every page, and prints "name=value " for the highest id per name. GitHub assigns check-run
# ids in creation order, so the highest id is the newest run whether or not it ever started.
newest_per_name() {
  sort -t $'\t' -k1,1 -k2,2n | awk -F'\t' '{ last[$1] = $3 } END { for (n in last) printf "%s=%s ", n, last[n] }'
}

# ledger_rows_from_body: reads a pull-request body on stdin and prints one line per ledger row,
# "ID<TAB>severity<TAB>sha-or-location<TAB>disposition".
ledger_rows_from_body() {
  awk -F'|' '
    /^## Review finding ledger/ { inledger = 1; next }
    /^## / { inledger = 0 }
    inledger && /^\|/ && $2 !~ /^ *ID *$/ && $2 !~ /^-+$/ && $2 !~ /None yet/ {
      gsub(/^ +| +$/, "", $2); gsub(/^ +| +$/, "", $3); gsub(/^ +| +$/, "", $4); gsub(/^ +| +$/, "", $10)
      print $2 "\t" $3 "\t" $4 "\t" $10
    }'
}

# ---- GitHub-facing helpers ----------------------------------------------------------------------

# Creates a label only when the repository has none of that name: an existing label, including
# one handed in as --ready-label, keeps its colour and description.
ensure_labels() {
  local existing
  existing="$(gh api "repos/$repo/labels?per_page=100" --paginate --jq '.[].name')"
  create() {
    grep -qxF "$1" <<< "$existing" \
      || gh label create "$1" --repo "$repo" --color "$2" --description "$3" >/dev/null
  }
  create lane:feature 0e8a16 "feature or sweep work: fix P0-P1, file P2-P3"
  create lane:findings-p1p2 fbca04 "P1/P2 findings workflow: fix P0-P2, file P3"
  create lane:findings-p3 d93f0b "P3 findings workflow: ready only on PASS"
  create "$ready_label" 5319e7 "audit passed: enqueue for merge"
}

# ruleset_state: prints "<strict> <queue>", 1 or 0 each: whether an active branch ruleset still
# requires an up-to-date branch, and whether one carries the merge-queue rule. Every active
# branch ruleset is read, which over-approximates on the safe side.
ruleset_state() {
  local strict=0 queue=0 id rules
  for id in $(gh api "repos/$repo/rulesets?targets=branch" --jq '.[] | select(.enforcement == "active") | .id'); do
    rules="$(gh api "repos/$repo/rulesets/$id" --jq '.rules[] | "\(.type)=\(.parameters.strict_required_status_checks_policy // "")"')"
    grep -q '^merge_queue=' <<< "$rules" && queue=1
    grep -q '^required_status_checks=true$' <<< "$rules" && strict=1
  done
  echo "$strict $queue"
}

# latest_review_id PR: the id of the newest review comment posted by the repository owner, or
# nothing. `--paginate` hands `--jq` each page separately, so `last` is per page: each page
# yields its newest match as "<created_at> <id>" and the newest across pages wins by timestamp.
latest_review_id() {
  gh api "repos/$repo/issues/$1/comments?per_page=100" --paginate \
    --jq "[.[] | select(.user.login == \"$reviewer\") | select(.body | test(\"<!-- upstroke-frontier-review|Reviewed head: [0-9a-f]{40}\"))] | last | select(. != null) | \"\(.created_at) \(.id)\"" \
    | sort | tail -1 | awk '{print $2}'
  return 0
}

# retargeted_after PR ISO-TIME: succeeds when the pull request's base was changed after that
# moment, which a review posted before it cannot have seen.
retargeted_after() {
  local when
  for when in $(gh api "repos/$repo/issues/$1/timeline?per_page=100" --paginate \
      --jq '.[] | select(.event == "base_ref_changed") | .created_at'); do
    [[ "$when" > "$2" ]] && return 0
  done
  return 1
}

# ---- the audit ----------------------------------------------------------------------------------

main() {
  apply=0
  enqueue=0
  ready_label="ready-to-merge"
  prs=()
  while (($#)); do
    case "$1" in
      --apply) apply=1 ;;
      --enqueue) apply=1; enqueue=1 ;;
      --ready-label)
        ready_label="$2"; shift
        [[ "$ready_label" == lane:* ]] && { echo "refusing: --ready-label must not be a lane:* label" >&2; exit 2; } ;;
      -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
      *) prs+=("$1") ;;
    esac
    shift
  done

  repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
  reviewer="$(gh repo view "$repo" --json owner --jq .owner.login)"
  read -r strict_up_to_date has_queue <<< "$(ruleset_state)"

  if ((${#prs[@]} == 0)); then
    mapfile -t prs < <(gh api "repos/$repo/pulls?state=open&per_page=100" --paginate --jq '.[].number')
  fi
  if ((apply)); then ensure_labels; fi

  printf '%-5s %-14s %-8s %-13s %s\n' PR LANE HEAD STATE DETAIL
  local pr
  for pr in "${prs[@]}"; do
    audit_one "$pr"
  done
}

audit_one() {
  local pr="$1"
  local meta branch head draft merge_state labels base base_oid attempt
  # GitHub computes mergeability lazily and answers UNKNOWN until it has; asking again after a
  # pause usually settles it, and an UNKNOWN that survives three asks fails closed below.
  for attempt in 1 2 3; do
    # One field per line, read with mapfile: a tab-separated read would collapse an empty field
    # (no labels) and shift every field after it.
    mapfile -t meta < <(gh pr view "$pr" --repo "$repo" \
      --json headRefName,headRefOid,isDraft,mergeStateStatus,labels,baseRefName,baseRefOid \
      --jq '.headRefName, .headRefOid, (.isDraft|tostring), .mergeStateStatus, ([.labels[].name]|join(" ")), .baseRefName, .baseRefOid')
    branch="${meta[0]:-}"; head="${meta[1]:-}"; draft="${meta[2]:-}"; merge_state="${meta[3]:-}"
    labels="${meta[4]:-}"; base="${meta[5]:-}"; base_oid="${meta[6]:-}"
    [[ "$merge_state" == UNKNOWN && $attempt -lt 3 ]] || break
    sleep 3
  done
  local lane must_fix state
  lane="$(lane_for "$branch")"
  must_fix="$(must_fix_for "$lane")"
  blockers=()
  state=READY

  [[ "$draft" == "true" ]] && blockers+=("draft")
  case "$merge_state" in
    DIRTY) blockers+=("conflicts") ;;
    UNKNOWN|"") blockers+=("mergeability-unknown") ;;   # GitHub has not computed it yet
    BLOCKED) blockers+=("blocked-by-rules") ;;          # a ruleset requirement is unmet, e.g. an unresolved conversation
    BEHIND)                                             # out of date: a blocker only while the ruleset demands an update
      ((strict_up_to_date)) && blockers+=("behind-base:ruleset-requires-up-to-date") ;;
  esac

  # The newest check run per required context on the head, chosen across every page in the
  # shell (`--paginate` runs the jq filter per page).
  local checks ctx
  checks="$(gh api "repos/$repo/commits/$head/check-runs?per_page=100" --paginate \
    --jq '.check_runs[] | select(.name == "upstroke-ci" or .name == "upstroke-pr-policy") | "\(.name)\t\(.id)\t\(.conclusion // .status)"' \
    | newest_per_name)"
  for ctx in upstroke-ci upstroke-pr-policy; do
    case " $checks " in
      *" $ctx=success "*) ;;
      *" $ctx="*) blockers+=("$ctx:${checks##*"$ctx="}"); blockers[-1]="${blockers[-1]%% *}" ;;
      *) blockers+=("$ctx:missing") ;;
    esac
  done

  # The latest owner review: its posting time, its kind, and its parse.
  local review_id review_at review_file kind reviewed="" verdict="" review_base="-"
  findings=()
  review_id="$(latest_review_id "$pr")"
  if [[ -z "$review_id" ]]; then
    blockers+=("no-review")
  else
    review_at="$(gh api "repos/$repo/issues/comments/$review_id" --jq '.created_at')"
    review_file="$(mktemp)"
    gh api "repos/$repo/issues/comments/$review_id" --jq '.body' > "$review_file"
    kind="$(review_kind "$review_file")"
    local sev id wit
    while IFS=$'\t' read -r sev id wit extra; do
      case "$sev" in
        META) reviewed="$id"; verdict="$wit"; review_base="${extra:-"-"}" ;;
        STRAY) blockers+=("manual:$id-outside-the-$([[ "$kind" == json ]] && echo verdict-object || echo numbered-findings)") ;;
        "") ;;
        *) findings+=("$sev"$'\t'"$id"$'\t'"$wit") ;;
      esac
    done < <(if [[ "$kind" == json ]]; then parse_verdict_json "$review_file"; else parse_prose_review "$review_file"; fi)
    rm -f "$review_file"

    case "$verdict" in
      PASS|CHANGES_REQUIRED) ;;
      "") blockers+=("no-verdict") ;;
      *) blockers+=("verdict:$verdict") ;;
    esac
    [[ "$lane" == findings-p3 && "$verdict" != PASS ]] && blockers+=("verdict-not-pass")
    [[ "$verdict" == PASS && ${#findings[@]} -gt 0 ]] && blockers+=("pass-with-findings")
    [[ "$verdict" == CHANGES_REQUIRED && ${#findings[@]} -eq 0 ]] && blockers+=("findings-unparsed:changes-required-lists-none")

    # The review must be against this pull request's own base (MAINTAINING step 4): a base
    # changed after the review was posted is a diff the review never saw, and the workflow
    # form's base commit must lie on the current base branch (and, off master, not on master,
    # since the integration branch carries master's history too).
    if retargeted_after "$pr" "$review_at"; then
      blockers+=("retargeted-after-review")
    fi
  fi

  # Head movement since the reviewed commit, and the base the review was made against.
  moved=""
  if [[ -n "$reviewed" ]]; then
    # refs/pull/N/head is the head whatever repository it lives in; a fork's branch is not on
    # origin, so fetching by branch name would leave the head and the reviewed commit unknown.
    git fetch -q origin "refs/pull/$pr/head" "$base" master 2>/dev/null || true
    if [[ "$kind" == json && "$review_base" == "-" ]]; then
      blockers+=("review-records-no-base")          # the workflow form always records base_sha; one without it is not judged
    elif [[ "$review_base" != "-" ]]; then
      if ! git cat-file -e "$review_base^{commit}" 2>/dev/null \
        || ! git merge-base --is-ancestor "$review_base" "origin/$base"; then
        blockers+=("review-base-not-on-$base:${review_base:0:7}")
      elif [[ "$base" != master ]] && git merge-base --is-ancestor "$review_base" origin/master; then
        blockers+=("review-base-on-master-not-$base:${review_base:0:7}")
      fi
    elif [[ "$base" != master ]]; then
      blockers+=("manual:prose-review-records-no-base-and-base-is-$base")
    fi
    if ! git cat-file -e "$reviewed^{commit}" 2>/dev/null; then
      blockers+=("reviewed-sha-unknown:${reviewed:0:7}")
    elif ! git merge-base --is-ancestor "$reviewed" "$head"; then
      blockers+=("reviewed-not-ancestor:${reviewed:0:7}")
    elif [[ "$reviewed" != "$head" ]]; then
      # A merge-in keeps the review only on MAINTAINING step 5's terms: the merge commit is
      # exactly what git produces from its two parents on its own (a hand edit, a conflict
      # resolution or a third parent is a new change that `--no-merges` below would hide), the
      # branch's diff against its base is byte-identical before and after the merge, and the pull
      # request edits no gate. Anything wider is reviewed again.
      local merge_edits="" merges m expected before after touched
      merges="$(git rev-list --merges "$reviewed..$head" --not "origin/$base")"
      for m in $merges; do
        if (($(git rev-list --parents -n 1 "$m" | wc -w) != 3)); then merge_edits="$m"; break; fi
        if ! expected="$(git merge-tree --write-tree "$m^1" "$m^2" 2>/dev/null)"; then
          merge_edits="$m"; break   # a conflict, or a git too old for --write-tree: fail closed
        fi
        [[ "$(git rev-parse "$m^{tree}")" == "$expected" ]] || { merge_edits="$m"; break; }
        # Byte-identical branch diff: the branch side against its base before the merge, and the
        # merge against the side it merged in, must be the same patch.
        before="$(git diff "$(git merge-base "$m^1" "$m^2")" "$m^1" | git hash-object --stdin)"
        after="$(git diff "$m^2" "$m" | git hash-object --stdin)"
        [[ "$before" == "$after" ]] || { merge_edits="$m"; break; }
      done
      if [[ -n "$merges" && -z "$merge_edits" ]] \
        && git diff --name-only "origin/$base...$head" -- .github/workflows .github/scripts | grep -q .; then
        blockers+=("review-stale:gate-edit-with-merge-in")   # step 5: no exemption for a gate-editing pull request
      fi
      # Commits the base already has arrived through a merge-in; only the branch's own count.
      touched="$(git log --no-merges --name-only --format= "$reviewed..$head" --not "origin/$base" | sort -u)"
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

  local rows f disposition nfiles cand
  rows="$(gh pr view "$pr" --repo "$repo" --json body --jq .body | ledger_rows_from_body)"
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
      deferred)   # the lane rule: an allowed finding is filed and deferred, one file per finding
        nfiles=0
        # NUL-separated names, so a path with whitespace stays one candidate.
        while IFS= read -r -d '' cand; do
          cand="${cand#*:}"   # git grep prefixes each name with "<sha>:"
          if git show "$head:$cand" 2>/dev/null | frontmatter_has_id "$id"; then nfiles=$((nfiles + 1)); fi
        done < <(git grep -l -z -F -e "id: $id" "$head" -- 'reviews/findings/*.md' 2>/dev/null || true)
        case "$nfiles" in
          1) ;;
          0) blockers+=("no-file:$id") ;;
          *) blockers+=("duplicate-file:$id") ;;
        esac ;;
      rejected|accepted-risk) blockers+=("manual:disposition-$disposition:$id") ;;   # the owner's call, not the audit's
      fixed)
        [[ "$moved" == repairs || "$moved" == merge-edits:* ]] || blockers+=("fixed-but-head-unmoved:$id") ;;
      "") blockers+=("no-row:$id") ;;
      *) blockers+=("bad-disposition:$id=$disposition") ;;
    esac
  done

  # Hard blockers decide first; a repair push only matters once nothing else stands in the way.
  # A draft is NOT-READY: READY means enqueueable as it stands.
  local hard=() b
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

  local detail l
  detail="verdict=${verdict:-none} reviewed=${reviewed:0:7}${moved:+ moved=$moved}"
  [[ "$base" != master ]] && detail+=" base=$base"
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
    # The ready label is a report of this audit at $head, not an authorisation: GitHub labels are
    # not bound to a commit, so a push can always land between the audit and the label write.
    # The head is read again before the write and again after it, and a label written across a
    # move is removed; that narrows the window, it cannot close it. The act that is bound to the
    # audited head is the enqueue, through --match-head-commit, and nothing may treat the label
    # alone as permission to merge.
    local now after_write
    if [[ "$state" == READY ]]; then
      now="$(gh pr view "$pr" --repo "$repo" --json headRefOid --jq .headRefOid)"
      if [[ "$now" != "$head" ]]; then
        echo "      head moved to ${now:0:7} since the audit read ${head:0:7}: not labelled, not enqueued"
        state=HEAD-MOVED
      fi
    fi
    if [[ "$state" == READY ]]; then
      [[ " $labels " != *" $ready_label "* ]] && gh pr edit "$pr" --repo "$repo" --add-label "$ready_label" >/dev/null
      after_write="$(gh pr view "$pr" --repo "$repo" --json headRefOid --jq .headRefOid)"
      if [[ "$after_write" != "$head" ]]; then
        gh pr edit "$pr" --repo "$repo" --remove-label "$ready_label" >/dev/null
        echo "      head moved to ${after_write:0:7} while labelling ${head:0:7}: label removed, not enqueued"
        state=HEAD-MOVED
      fi
    fi
    if [[ "$state" == READY ]]; then
      if ((enqueue)); then
        # The base is read again and the timeline re-checked just before the call: a base changed
        # since the audit is a different diff, and --match-head-commit binds only the head. That
        # narrows the base window to the call itself; a retarget after the enqueue lands in the
        # queue on the new base with both contexts re-run there but without a review of that
        # diff, and it is visible on the pull request's timeline as a base change after the
        # review, which the next audit reports.
        local base_now
        base_now="$(gh pr view "$pr" --repo "$repo" --json baseRefName --jq .baseRefName)"
        if [[ "$base_now" != "$base" ]] || retargeted_after "$pr" "$review_at"; then
          echo "      base changed since the audit read $base: not enqueued"
          state=BASE-MOVED
        fi
      fi
    fi
    if [[ "$state" == READY ]]; then
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
  return 0
}

if [[ "${PR_READY_AUDIT_LIBRARY:-0}" != 1 ]]; then
  main "$@"
fi
