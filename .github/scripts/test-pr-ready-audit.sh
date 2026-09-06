#!/usr/bin/env bash
# scripts/pr-ready-audit.sh, exercised against fixtures: the pure parts (the lane and its
# severity set, the two review parsers -- the workflow's fenced JSON verdict and the frontier
# prose form --, the frontmatter id match, the newest-check-run choice, the ledger-row parse and
# the identity-drift comparison), and the write sequence, which touches gh and no git and so runs
# whole against a fake `gh` on PATH that logs every call. What talks to git is still out of scope
# here; the audit's behaviour on a live pull request is observed on the pull request.
#
# Each case names the defect it exists to catch, so a green run says what it proved:
#   MUT-LANE-LABEL-INPUT         the lane came from a label, not the branch prefix
#   MUT-P3-LANE-DEFERS           the P3 lane let a P3 be deferred
#   MUT-JSON-SPLIT-BY-REGEX      findings read by splitting text on "},{" instead of a parser
#   MUT-NULL-WITNESS             a null witness field counted as a witness
#   MUT-MUST-UNSEEN              a MUST deviation was not flagged
#   MUT-BAD-SEVERITY-PASSES      a P9 finding passed as deferrable
#   MUT-STRAY-TOKEN-UNSEEN       a severity token outside the verdict object was ignored
#   MUT-VERDICT-FROM-PROSE       the verdict was read from prose, not the object with the findings
#   MUT-QUOTED-JSON-IS-JSON      a prose review quoting JSON was parsed as the JSON form
#   MUT-PROSE-HEADING-FINDING    a prose P1 written as a heading vanished
#   MUT-PROSE-LAST-VERDICT       the first VERDICT line won over the last
#   MUT-FRONTMATTER-BY-SUBSTRING an id in prose satisfied the frontmatter match
#   MUT-FRONTMATTER-PATTERN      an id with a dot matched as a regex
#   MUT-CHECK-RUN-FIRST-SUCCESS  an older success outranked a newer failure
#   MUT-CHECK-RUN-UNSTARTED      an unstarted run was ordered by a stand-in date, not its id
#   MUT-LEDGER-HEADER-AS-ROW     the header or separator line parsed as a row
#   MUT-DRIFT-BASE-IGNORED       a moved base compared equal to the base the audit judged
#   MUT-DRIFT-UNREAD-IS-AGREE    a failed re-read of the identity read as agreement
#   MUT-DRIFT-STATE-DEFAULT-OK   an unrecognised drift token left the pull request READY
#   MUT-LABEL-BASE-UNCHECKED     the base was not re-read before the ready label was written
#   MUT-LABEL-NOT-RECONCILED     a ready label this run wrote survived the run ending not-READY
#   MUT-ENQUEUE-BASE-UNBOUND     nothing was read after the enqueue, so a retarget stood
#   MUT-ENQUEUE-NOT-WITHDRAWN    a drifted enqueue was reported and left in the queue
#   MUT-RETARGET-BEFORE-ENQUEUE  a base_ref_changed after the review did not stop the enqueue
#   MUT-ENQUEUE-CAN-MERGE        the enqueue used a call that merges when the base has no queue
#   MUT-DEQUEUE-UNVERIFIED       a removal that removed nothing was reported as a withdrawal
#   MUT-TIMELINE-FAIL-OPEN       an unreadable timeline read as a timeline without a retarget
#   MUT-QUEUE-ENTRY-UNCHECKED    the queued commit was never compared with the audited head
#   MUT-QUEUE-READ-FAIL-OPEN     an unreadable queue state was taken as a confirmed entry
#   MUT-ENQUEUE-REFUSED-DEQUEUES a refused enqueue withdrew a pull request someone else queued
#   MUT-NODE-ID-UNREAD           a pull request was queued with no id to withdraw it by
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$script_dir/../.." && pwd)"
cd "$root"
failed=0
error() { echo "$*" >&2; failed=1; }
expect() {  # expect <case> <got> <want>
  [[ "$2" == "$3" ]] || error "$1: got [$2], want [$3]"
}

PR_READY_AUDIT_LIBRARY=1 source scripts/pr-ready-audit.sh
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# --- lanes and severity sets ------------------------------------------------------------------
expect MUT-LANE-LABEL-INPUT "$(lane_for codex/findings-p3-7d2d8e9dc74a)" findings-p3
expect MUT-LANE-LABEL-INPUT "$(lane_for codex/findings-ba8fdc8dec2b)" findings-p1p2
expect MUT-LANE-LABEL-INPUT "$(lane_for codex/sweep-f3eb59a749fe)" feature
expect MUT-LANE-LABEL-INPUT "$(lane_for fix/sampler-kill-and-inspection)" feature
expect MUT-P3-LANE-DEFERS "$(must_fix_for findings-p3)" "P0 P1 P2 P3"
expect MUT-P3-LANE-DEFERS "$(must_fix_for findings-p1p2)" "P0 P1 P2"
expect MUT-P3-LANE-DEFERS "$(must_fix_for feature)" "P0 P1"

# --- the workflow form: a fenced JSON verdict ---------------------------------------------------
cat > "$tmp/json.md" <<'EOF'
Findings workflow review 2/2.

Reviewed head: 4ad962f000000000000000000000000000000001
Base: 5157509000000000000000000000000000000002
Reviewer: gpt-5.6-sol, medium effort. An earlier draft said {"verdict":"PASS"} but see below;
the prose here also mentions a P1 that is not in the object.

Unedited verdict:

```json
{"reviewed_sha":"4ad962f000000000000000000000000000000001","base_sha":"5157509000000000000000000000000000000002","verdict":"CHANGES_REQUIRED","findings":[{"id":"A-DEFERRABLE","severity":"P3","reproduction":null,"witness":false},{"id":"B-WITNESSED","severity":"P2","failing_test":"a_test_that_fails"},{"id":"C-MUST","severity":"P2","correction":"This is a MUST deviation of standards section 7"},{"id":"D-BAD","severity":"P9"},{"id":"E.DOTTED","severity":"P3","location":"src/x.rs:1"}]}
```
EOF
expect MUT-QUOTED-JSON-IS-JSON "$(review_kind "$tmp/json.md")" json
got="$(parse_verdict_json "$tmp/json.md" | tr '\t' '|')"
want='META|4ad962f000000000000000000000000000000001|CHANGES_REQUIRED|5157509000000000000000000000000000000002
STRAY|P1|0
P3|A-DEFERRABLE|0
P2|B-WITNESSED|1
P2|C-MUST|2
ERR|bad-severity:D-BAD|0
P3|E.DOTTED|0'
expect "MUT-JSON-SPLIT-BY-REGEX/MUT-NULL-WITNESS/MUT-MUST-UNSEEN/MUT-BAD-SEVERITY-PASSES/MUT-STRAY-TOKEN-UNSEEN/MUT-VERDICT-FROM-PROSE" "$got" "$want"

# A pretty-printed object with "}, {" between findings is the same object to a parser.
cat > "$tmp/pretty.md" <<'EOF'
Reviewed head: 4ad962f000000000000000000000000000000001

```json
{
  "reviewed_sha": "4ad962f000000000000000000000000000000001",
  "verdict": "CHANGES_REQUIRED",
  "findings": [
    {"id": "FIRST", "severity": "P3"}, {"id": "SECOND", "severity": "P1"}
  ]
}
```
EOF
got="$(parse_verdict_json "$tmp/pretty.md" | tr '\t' '|')"
want='META|4ad962f000000000000000000000000000000001|CHANGES_REQUIRED|-
P3|FIRST|0
P1|SECOND|0'
expect MUT-JSON-SPLIT-BY-REGEX "$got" "$want"

# --- the frontier form: prose ------------------------------------------------------------------
cat > "$tmp/prose.md" <<'EOF'
<!-- upstroke-frontier-review pr=145 head=c3a6665000000000000000000000000000000003 -->
## Frontier review of `c3a6665` (gpt-5.6-sol, max effort)

**VERDICT: PASS**

<details>
1. **P2 — The claimed closure fails.** Detail.

2. **P3 — A scope claim does not match the diff.** Detail.

### P1 — data loss written as a heading, which a numbered-only parser would miss.

I found no MUST deviation.

VERDICT: CHANGES_REQUIRED
</details>
EOF
expect MUT-QUOTED-JSON-IS-JSON "$(review_kind "$tmp/prose.md")" prose
got="$(parse_prose_review "$tmp/prose.md" | tr '\t' '|')"
want='META|c3a6665000000000000000000000000000000003|CHANGES_REQUIRED|-
P2|-|0
P3|-|0
STRAY|MUST/P1|0'
expect "MUT-PROSE-HEADING-FINDING/MUT-PROSE-LAST-VERDICT" "$got" "$want"

# A prose review that quotes a JSON object stays prose.
printf 'Reviewed head: %s\nThe object {"verdict":"PASS","findings":[]} is an example.\nVERDICT: CHANGES_REQUIRED\n' \
  "4ad962f000000000000000000000000000000001" > "$tmp/quoted.md"
expect MUT-QUOTED-JSON-IS-JSON "$(review_kind "$tmp/quoted.md")" prose

# --- the frontmatter id match ------------------------------------------------------------------
printf -- '---\nid: OTHER-ID\nseverity: P2\n---\n\nThe prose below repeats a line.\nid: TARGET-ID\n' > "$tmp/prose-id.md"
printf -- '---\nid: TARGET-ID\nseverity: P2\n---\n\nBody.\n' > "$tmp/front-id.md"
printf -- '---\nid: TARGETXID\n---\n' > "$tmp/x-id.md"
printf 'id: TARGET-ID\n---\nno opening fence\n' > "$tmp/no-front.md"
frontmatter_has_id TARGET-ID < "$tmp/prose-id.md" && error "MUT-FRONTMATTER-BY-SUBSTRING: an id in prose matched"
frontmatter_has_id TARGET-ID < "$tmp/front-id.md" || error "MUT-FRONTMATTER-BY-SUBSTRING: the frontmatter id did not match"
frontmatter_has_id TARGET.ID < "$tmp/x-id.md" && error "MUT-FRONTMATTER-PATTERN: a dot matched as a regex"
frontmatter_has_id TARGET-ID < "$tmp/no-front.md" && error "MUT-FRONTMATTER-BY-SUBSTRING: a file without a frontmatter block matched"

# --- the newest check run per name -------------------------------------------------------------
got="$(printf 'upstroke-ci\t100\tsuccess\nupstroke-ci\t250\tfailure\nupstroke-pr-policy\t120\tsuccess\nupstroke-ci\t90\tsuccess\n' | newest_per_name | tr ' ' '\n' | grep . | sort | tr '\n' ' ')"
expect MUT-CHECK-RUN-FIRST-SUCCESS "$got" "upstroke-ci=failure upstroke-pr-policy=success "
got="$(printf 'upstroke-ci\t300\tsuccess\nupstroke-ci\t200\tcancelled\n' | newest_per_name)"
expect MUT-CHECK-RUN-UNSTARTED "$got" "upstroke-ci=success "
got="$(printf 'upstroke-ci\t1000\tqueued\nupstroke-ci\t999\tsuccess\n' | newest_per_name)"
expect MUT-CHECK-RUN-UNSTARTED "$got" "upstroke-ci=queued "

# --- the ledger rows ---------------------------------------------------------------------------
cat > "$tmp/body.md" <<'EOF'
## Summary

Text with a | pipe.

## Review finding ledger

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| A-DEFERRABLE | P3 | 4ad962f / src/x.rs:1 | a -> b | pre_existing | correctness | — | `guard` | deferred |
| B-FIXED | P2 | 4ad962f / src/y.rs:2 | a -> b | introduced_by_feature | liveness | — | `test` | fixed |

## Risk and rollback

| not | a | ledger | row |
EOF
got="$(ledger_rows_from_body < "$tmp/body.md" | tr '\t' '|')"
want='A-DEFERRABLE|P3|4ad962f / src/x.rs:1|deferred
B-FIXED|P2|4ad962f / src/y.rs:2|fixed'
expect MUT-LEDGER-HEADER-AS-ROW "$got" "$want"

# --- the identity comparison -------------------------------------------------------------------
expect MUT-DRIFT-BASE-IGNORED "$(identity_drift head1 master "head1 master")" ""
expect MUT-DRIFT-BASE-IGNORED "$(identity_drift head1 master "head1 release")" "base-moved:release"
expect MUT-DRIFT-BASE-IGNORED "$(identity_drift head1 master "head2xxxxxxx master")" "head-moved:head2xx"
expect MUT-DRIFT-UNREAD-IS-AGREE "$(identity_drift head1 master "")" "unconfirmed:re-read-failed"
expect MUT-DRIFT-UNREAD-IS-AGREE "$(identity_drift head1 master "head1")" "unconfirmed:re-read-failed"
expect MUT-DRIFT-UNREAD-IS-AGREE "$(identity_drift head1 master " master")" "unconfirmed:re-read-failed"
expect MUT-DRIFT-STATE-DEFAULT-OK "$(drift_state head-moved:head2)" HEAD-MOVED
expect MUT-DRIFT-STATE-DEFAULT-OK "$(drift_state base-moved:release)" BASE-MOVED
expect MUT-DRIFT-STATE-DEFAULT-OK "$(drift_state unconfirmed:re-read-failed)" UNCONFIRMED
expect MUT-DRIFT-STATE-DEFAULT-OK "$(drift_state "")" UNCONFIRMED

# --- the write sequence, driven against a stateful fake gh -------------------------------------
# maintain_labels_and_enqueue talks to gh and not to git, so the order of its reads and writes is
# observable here. The fake logs every call on one line and answers:
#   pr view --json headRefOid,baseRefName  the next line of $GH_IDENTITY, the last repeating
#   pr view --json id                      a fixed node id
#   api .../timeline                       $GH_TIMELINE, or exit 1 when $GH_TIMELINE_FAIL is 1
#   api graphql enqueuePullRequest         records "queued no-auto $GH_ENQUEUE_HEAD" in $GH_QUEUE,
#                                          or exit 1 when $GH_ENQUEUE_FAIL is 1
#   api graphql dequeuePullRequest         clears $GH_QUEUE, unless $GH_DEQUEUE_NOOP is 1, when it
#                                          exits 0 having removed nothing -- which is what the real
#                                          CLI's --disable-auto does to a queued pull request
#   api graphql isInMergeQueue             $GH_QUEUE, or exit 1 when $GH_QUEUE_READ_FAIL is 1
#   pr merge --disable-auto                logged, and deliberately no effect on $GH_QUEUE
# It is stateful on purpose: the defect this catches is a withdrawal reported from an exit status
# while the entry is still in the queue, and no stateless fake can tell the two apart.
mkdir -p "$tmp/bin"
cat > "$tmp/bin/gh" <<'GH'
#!/usr/bin/env bash
set -uo pipefail
printf '%s\n' "${*//$'\n'/ }" >> "$GH_LOG"
case "$1 ${2:-}" in
  "pr view")
    if [[ "$*" == *headRefOid* ]]; then
      [[ "${GH_IDENTITY_FAIL:-0}" == 1 ]] && exit 1
      n="$(grep -c 'headRefOid' "$GH_LOG")"
      total="$(wc -l < "$GH_IDENTITY")"
      ((n > total)) && n="$total"
      sed -n "${n}p" "$GH_IDENTITY"
    elif [[ "$*" == *"--json id"* ]]; then
      [[ "${GH_NODE_FAIL:-0}" == 1 ]] && exit 1
      echo PR_NODE_ID
    fi ;;
  "api graphql")
    if [[ "$*" == *enqueuePullRequest* ]]; then
      [[ "${GH_ENQUEUE_FAIL:-0}" == 1 ]] && exit 1
      printf 'queued no-auto %s' "${GH_ENQUEUE_HEAD:-head1}" > "$GH_QUEUE"
    elif [[ "$*" == *dequeuePullRequest* ]]; then
      [[ "${GH_DEQUEUE_NOOP:-0}" == 1 ]] || printf 'not-queued no-auto -' > "$GH_QUEUE"
    elif [[ "$*" == *isInMergeQueue* ]]; then
      [[ "${GH_QUEUE_READ_FAIL:-0}" == 1 ]] && exit 1
      cat "$GH_QUEUE"
    fi ;;
  "api "*) [[ "${GH_TIMELINE_FAIL:-0}" == 1 ]] && exit 1; cat "$GH_TIMELINE" ;;
esac
exit 0
GH
chmod +x "$tmp/bin/gh"

repo="o/r"
ready_label="ready-to-merge"
review_at="2026-09-06T10:00:00Z"
export GH_LOG="$tmp/gh.log" GH_IDENTITY="$tmp/identity" GH_TIMELINE="$tmp/timeline" GH_QUEUE="$tmp/queue"
export GH_IDENTITY_FAIL=0 GH_NODE_FAIL=0 GH_TIMELINE_FAIL=0 GH_QUEUE_READ_FAIL=0
export GH_ENQUEUE_FAIL=0 GH_DEQUEUE_NOOP=0 GH_ENQUEUE_HEAD=head1

# writes IDENTITY TIMELINE STATE LABELS ENQUEUE: runs the sequence and prints its gh calls, one
# per line, with the constant parts of each call dropped. What it said lands in $tmp/said.
writes() {
  : > "$GH_LOG"
  printf '%s\n' "$1" > "$GH_IDENTITY"
  printf '%s' "$2" > "$GH_TIMELINE"
  printf 'not-queued no-auto -' > "$GH_QUEUE"
  enqueue="$5"
  PATH="$tmp/bin:$PATH" maintain_labels_and_enqueue 9 findings-p1p2 head1 master "$3" "$4" "$review_at" > "$tmp/said" 2>&1
  sed -e 's/^pr view .*headRefOid.*/view/' -e 's/^pr view .*--json id.*/node/' \
      -e 's/^api graphql .*enqueuePullRequest.*/enqueue/' \
      -e 's/^api graphql .*dequeuePullRequest.*/dequeue/' \
      -e 's/^api graphql .*isInMergeQueue.*/queue-state/' \
      -e 's/^api repos.*timeline.*/timeline/' \
      -e 's/^pr edit 9 --repo o\/r //' -e 's/^pr merge 9 --repo o\/r //' "$GH_LOG" | tr '\n' ' '
}
said() { tr '\n' ' ' < "$tmp/said"; }
reset_fakes() {
  GH_IDENTITY_FAIL=0; GH_NODE_FAIL=0; GH_TIMELINE_FAIL=0; GH_QUEUE_READ_FAIL=0
  GH_ENQUEUE_FAIL=0; GH_DEQUEUE_NOOP=0; GH_ENQUEUE_HEAD=head1
}

steady='head1 master'
four="$steady
$steady
$steady
$steady"

# The whole sequence on a pull request nothing touches: lane label, identity, ready label,
# identity, node id, identity and timeline before the enqueue, the enqueue, identity and timeline
# after it, and the queue entry read back.
got="$(writes "$four" "" READY "" 1)"
expect MUT-ENQUEUE-BASE-UNBOUND "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view timeline enqueue view timeline queue-state "
# The enqueue is enqueue-only. `gh pr merge --merge --auto` merges a mergeable pull request whose
# base carries no queue, so its absence from the log is the property, not an incidental spelling.
expect MUT-ENQUEUE-CAN-MERGE "$(grep -c -e '--merge' -e '--auto' "$GH_LOG" || true)" 0
expect MUT-ENQUEUE-CAN-MERGE "$(grep -c 'expectedHeadOid' "$GH_LOG" || true)" 1

# The base moved between the audit's read and the label write: neither labelled nor enqueued.
got="$(writes "head1 release" "" READY "" 1)"
expect MUT-LABEL-BASE-UNCHECKED "$got" "--add-label lane:findings-p1p2 view "

# The base moved after the label was written: the label this run wrote is taken back, and the
# reconciliation is against the state the run ends on, not the labels it started from.
got="$(writes "$steady
$steady
head1 release" "" READY "" 1)"
expect MUT-LABEL-NOT-RECONCILED "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view --remove-label ready-to-merge "

# A retarget recorded after the review, with the ref name unchanged: read off the timeline before
# the enqueue, so nothing is queued.
got="$(writes "$four" "2026-09-06T11:00:00Z" READY "" 1)"
expect MUT-RETARGET-BEFORE-ENQUEUE "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view timeline --remove-label ready-to-merge "

# A timeline that could not be read is not a timeline without a retarget: nothing is enqueued.
GH_TIMELINE_FAIL=1
got="$(writes "$four" "" READY "" 1)"
expect MUT-TIMELINE-FAIL-OPEN "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view timeline --remove-label ready-to-merge "
reset_fakes

# The base moved after the enqueue: the entry is withdrawn with dequeuePullRequest, the withdrawal
# is read back, and the label goes with it.
got="$(writes "$steady
$steady
$steady
head1 release" "" READY "" 1)"
expect MUT-ENQUEUE-NOT-WITHDRAWN "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view timeline enqueue view dequeue --disable-auto queue-state --remove-label ready-to-merge "
case "$(said)" in *"withdrawn, and the withdrawal read back"*) ;;
  *) error "MUT-ENQUEUE-NOT-WITHDRAWN: a confirmed withdrawal was not reported: [$(said)]" ;; esac

# The same drift, but the removal does not remove: the CLI exits 0 and the entry stays queued.
# The audit must not report a withdrawal it did not get, and must still take the label back.
GH_DEQUEUE_NOOP=1
got="$(writes "$steady
$steady
$steady
head1 release" "" READY "" 1)"
expect MUT-DEQUEUE-UNVERIFIED "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view timeline enqueue view dequeue --disable-auto queue-state --remove-label ready-to-merge "
case "$(said)" in *"withdrawal could NOT be confirmed"*) ;;
  *) error "MUT-DEQUEUE-UNVERIFIED: an unverified removal was reported as a withdrawal: [$(said)]" ;; esac
case "$(said)" in *"withdrawn, and the withdrawal read back"*)
  error "MUT-DEQUEUE-UNVERIFIED: a no-op removal claimed a confirmed withdrawal: [$(said)]" ;; esac
reset_fakes

# Nothing moved, but the queue entry holds a commit this audit never judged: still a withdrawal.
GH_ENQUEUE_HEAD=head9
got="$(writes "$four" "" READY "" 1)"
expect MUT-QUEUE-ENTRY-UNCHECKED "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view timeline enqueue view timeline queue-state dequeue --disable-auto queue-state --remove-label ready-to-merge "
reset_fakes

# The queue state could not be read: an unread entry is not a confirmed one.
GH_QUEUE_READ_FAIL=1
got="$(writes "$four" "" READY "" 1)"
expect MUT-QUEUE-READ-FAIL-OPEN "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view timeline enqueue view timeline queue-state dequeue --disable-auto queue-state --remove-label ready-to-merge "
case "$(said)" in *"withdrawal could NOT be confirmed"*) ;;
  *) error "MUT-QUEUE-READ-FAIL-OPEN: an unreadable queue state did not fail closed: [$(said)]" ;; esac
reset_fakes

# A refused enqueue withdraws nothing and reports the state it read.
GH_ENQUEUE_FAIL=1
got="$(writes "$four" "" READY "" 1)"
expect MUT-ENQUEUE-REFUSED-DEQUEUES "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node view timeline enqueue queue-state "
reset_fakes

# A re-read of the identity that fails is drift, not agreement.
GH_IDENTITY_FAIL=1
got="$(writes "$steady" "" READY "" 1)"
expect MUT-DRIFT-UNREAD-IS-AGREE "$got" "--add-label lane:findings-p1p2 view "
reset_fakes

# Without a node id there is no withdrawal, so there is no enqueue either.
GH_NODE_FAIL=1
got="$(writes "$four" "" READY "" 1)"
expect MUT-NODE-ID-UNREAD "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view node --remove-label ready-to-merge "
reset_fakes

# Not READY on entry: the lane label is still maintained and a standing ready label is removed.
got="$(writes "$steady" "" NOT-READY "lane:findings-p1p2 ready-to-merge" 1)"
expect MUT-LABEL-NOT-RECONCILED "$got" "--remove-label ready-to-merge "

# Without --enqueue the sequence still labels, and still brackets the write with a re-read.
got="$(writes "$steady" "" READY "" 0)"
expect MUT-LABEL-BASE-UNCHECKED "$got" \
  "--add-label lane:findings-p1p2 view --add-label ready-to-merge view "

if ((failed)); then
  echo "test-pr-ready-audit: FAILED" >&2
  exit 1
fi
echo "test-pr-ready-audit: ok"
