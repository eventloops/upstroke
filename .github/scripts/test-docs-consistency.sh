#!/usr/bin/env bash
# Documentation and workflow-trigger claims that go stale silently, checked
# against the tree rather than against a hard-coded copy of it.
#
# Every class here is a review finding, not a hypothesis. PR #20's first draft
# of CLAUDE.md described a tree that did not exist at its own head and asserted
# that CONTRIBUTING.md omitted --all-features while the same commit added it.
# PR #23 asked for the trigger lists to be pinned. PR #25's own review then
# killed the first version of this file four ways -- a masked trigger block, a
# hard-coded command set that never consulted CI, a path regex blind to
# root-level files, and a gate count nobody checked. Each fix below names the
# mutation it exists to kill.
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$script_dir/../.." && pwd)"
cd "$root"

failed=0
error() { echo "$*" >&2; failed=1; }

# --- 1. every repository path a document names must resolve -----------------
# MUT-ROOT-PATH-MISSPELLED: the first version matched only paths beginning with
# a known directory, so `MAINTAINERS.md` sailed through. A token with a slash
# must resolve exactly; a bare document name must exist somewhere tracked.
# A forward reference is legitimate -- a document may name a file that arrives
# with another pull request -- but it must say so within a line or two, because
# the prose is hard-wrapped and a qualifier routinely lands on the next line.
# A qualifier may say the path is coming, or that it deliberately does not
# exist ("there is **no** rust-toolchain.toml"). Either way the claim is
# checkable prose rather than a broken pointer.
marker='arrives with|arrive with|not yet|until that merges|until it merges|lands with|forward reference|\*\*no |there is \*\*?no|does not exist|must not exist'
for doc in CLAUDE.md CONTRIBUTING.md; do
  [[ -f "$doc" ]] || continue
  # Two domains, deliberately narrow. Repository-rooted paths under a known
  # top-level directory must resolve exactly. Bare document names must exist at
  # the repository root -- that is the MUT-ROOT-PATH-MISSPELLED case, where
  # `MAINTAINERS.md` sailed through the first version. Relative fragments
  # (`plan/`, `engine/`), git refs (`origin/branch`) and command text are not
  # pointers into the tree and are not checked here.
  rooted=$(grep -oE '`(src|infra|\.github|acceptance|decisions|proposals|reviews|examples|fixtures|docs)/[A-Za-z0-9_./-]*`' "$doc" | tr -d '`' || true)
  bare=$(grep -oE '`[A-Za-z0-9][A-Za-z0-9_.-]*\.(md|toml|lock)`' "$doc" | tr -d '`' || true)
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    [[ -e "$path" ]] && continue
    marked=0
    while IFS= read -r line_no; do
      [[ -z "$line_no" ]] && continue
      from=$(( line_no > 3 ? line_no - 3 : 1 ))
      sed -n "${from},$(( line_no + 4 ))p" "$doc" | grep -qiE "$marker" && marked=1
    done < <(grep -nF -- "\`$path\`" "$doc" | cut -d: -f1)
    (( marked )) || error "$doc names \`$path\`, which does not exist at this head and is neither marked as a forward reference nor stated as deliberately absent"
  done < <(printf '%s\n%s\n' "$rooted" "$bare" | grep -v '^$' | sort -u)
done

# --- 2. documented gate commands are DERIVED from ci.yml, never assumed -----
# MUT-TEMPLATE-MSRV-REMOVED / MUT-CI-CLIPPY-ALL-FEATURES-REMOVED: the first
# version compared the documents against a list written into this script, so a
# change to CI drifted from the documents with the fixture still green. The
# authority is ci.yml; the documents must carry what it actually runs. A
# document may pin a toolchain (`cargo +1.85.0 check ...`) where CI selects one
# through its toolchain action, so that one token is allowed to differ.
python3 - "$root" <<'PY' || failed=1
import io, os, re, sys
root = sys.argv[1]
ci = io.open(os.path.join(root, ".github/workflows/ci.yml"), encoding="utf-8").read()
commands = re.findall(r"^\s*- run:\s*(cargo .+?)\s*$", ci, re.M)
if not commands:
    print("could not read any cargo command from ci.yml", file=sys.stderr)
    raise SystemExit(1)
docs = ["CLAUDE.md", "CONTRIBUTING.md", ".github/pull_request_template.md"]
bad = False
for doc in docs:
    p = os.path.join(root, doc)
    if not os.path.exists(p):
        continue
    text = io.open(p, encoding="utf-8").read()
    # Allow an explicit toolchain pin in prose: `cargo +1.85.0 check ...`.
    normalised = re.sub(r"\bcargo \+\S+ ", "cargo ", text)
    for command in commands:
        if command not in normalised:
            print("%s does not carry the gate command CI runs: %s" % (doc, command),
                  file=sys.stderr)
            bad = True
raise SystemExit(1 if bad else 0)
PY

# A claim that another document is stale must not outlive the fix.
if [[ -f CLAUDE.md && -f CONTRIBUTING.md ]] \
   && grep -Fq -- '--all-features' CONTRIBUTING.md \
   && grep -qiE 'CONTRIBUTING\.md.{0,40}(omits|is stale|does not (carry|include))' CLAUDE.md; then
  error "CLAUDE.md claims CONTRIBUTING.md omits --all-features, but CONTRIBUTING.md carries it at this head"
fi

# --- 3. the gate inventory CLAUDE.md advertises must match the tree ---------
# DOC-GATE-COUNT-STALE / MUT-GATE-COUNT-STALE: this change added an eighth gate
# while CLAUDE.md still said seven, in two places. A count is a fact about the
# tree; check it.
actual_gates=$(ls .github/scripts/test-*.sh 2>/dev/null | wc -l | tr -d '[:space:]')
if [[ -f CLAUDE.md ]]; then
  while IFS= read -r claimed; do
    [[ -z "$claimed" ]] && continue
    [[ "$claimed" == "$actual_gates" ]] \
      || error "CLAUDE.md claims $claimed \`test-*.sh\` gates; the tree has $actual_gates"
  done < <(grep -oE '[0-9]+ `test-\*\.sh` gates' CLAUDE.md | grep -oE '^[0-9]+')
  grep -qE '[0-9]+ `test-\*\.sh` gates' CLAUDE.md \
    || error "CLAUDE.md must state the gate count as 'N \`test-*.sh\` gates' so it can be checked"
fi

# --- 4. trigger contract, scoped to the block that actually matters ---------
# MUT-CI-PR-BRANCH-MASKED: the first version grepped the whole file, so
# narrowing only the pull_request branch list still matched the push block and
# passed. Extract each event's own block.
# decisions/2026-08-21-stacked-slice-prs.md.
block() {  # block <file> <event-key>
  awk -v key="  $2:" '
    $0 == key { inblock = 1; next }
    inblock && /^  [a-z_]+:/ { inblock = 0 }
    inblock && /^[a-z]/ { inblock = 0 }
    inblock { print }
  ' "$1"
}
for spec in "ci:push" "ci:pull_request" "pr-policy:pull_request"; do
  wf="${spec%%:*}"; event="${spec##*:}"
  f=".github/workflows/$wf.yml"
  [[ -f "$f" ]] || { error "$f is missing"; continue; }
  got="$(block "$f" "$event" | grep -E '^\s*branches:' || true)"
  [[ -n "$got" ]] || { error "$f has no branches list under $event"; continue; }
  grep -Fq 'codex/parallelism-design' <<< "$got" \
    || error "$f: the $event branch list must include the integration branch, got:${got}"
done
for wf in frontier-review frontier-review-invalidate; do
  f=".github/workflows/$wf.yml"
  [[ -f "$f" ]] || { error "$f is missing"; continue; }
  grep -Fq 'codex/parallelism-design' "$f" \
    && error "$f must stay master-only: attestation is never minted for a slice pull request"
done
grep -Fq 'repository_dispatch:' .github/workflows/frontier-review.yml \
  || error "frontier-review.yml must be dispatch-triggered"
grep -qE '^\s*pull_request:' .github/workflows/frontier-review.yml \
  && error "frontier-review.yml must never trigger on pull_request"

if (( failed )); then
  echo "documentation consistency fixtures: FAIL" >&2
  exit 1
fi
echo "documentation consistency fixtures: PASS"
