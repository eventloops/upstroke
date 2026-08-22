#!/usr/bin/env bash
# Documentation and workflow-trigger claims that go stale silently, checked
# against the tree rather than against a hard-coded copy of it.
#
# THE CLAIMS THIS GATE ENFORCES -- exactly these, nothing else is in its scope:
#
#   C1  Every repository path CLAUDE.md or CONTRIBUTING.md names in backticks
#       exists at this head, or EACH occurrence is qualified within its own
#       window (three lines before to four after) by one of the marker phrases
#       below. Qualification is syntactic: the gate checks that a phrase is
#       present near that occurrence, not what the phrase refers to.
#   C2  The set of gate commands each of CLAUDE.md, CONTRIBUTING.md and the
#       pull-request template documents EQUALS the set ci.yml runs -- both
#       directions, pinned to the msrv job's toolchain where the command is the
#       msrv job's. A documented gate command is syntactic too: a standalone line
#       that is a fenced-code line or a checklist item and starts with `cargo`.
#       CI itself carries the floor the design packet names (`cargo test
#       --all-targets`), and the MSRV triangle agrees: Cargo.toml's rust-version,
#       the msrv job's toolchain, and the documents' `cargo +X` pin. And
#       CLAUDE.md does not claim CONTRIBUTING.md omits a flag it carries.
#   C3  CLAUDE.md's gate-count claim equals the tree, and the set of test-*.sh
#       files in .github/scripts EQUALS the set the lint job invokes, both
#       directions. An invocation from any other job does not count.
#   C4  The workflow trigger contract is EXACTLY what the slice-PR record
#       decided (decisions/2026-08-21-stacked-slice-prs.md): ci.yml triggers on
#       push and pull_request, pr-policy.yml on pull_request, each with the branch
#       list [master, codex/parallelism-design] and nothing else;
#       frontier-review.yml triggers on repository_dispatch and nothing else;
#       frontier-review-invalidate.yml triggers on pull_request_target with
#       branches [master] and nothing else; and neither attestation workflow
#       names the integration branch anywhere.
#
# EVERY CHECK IS AN EQUALITY OR AN EXACT PIN. A presence test -- a substring, a
# one-way subset, a forbidden value standing in for a required one, a flag per
# path instead of per occurrence -- is how every earlier version of this file
# was killed, and each fix below names the mutation it exists to kill:
#   round 1: MUT-CI-PR-BRANCH-MASKED (whole-file grep), MUT-TEMPLATE-MSRV-REMOVED
#            and MUT-CI-CLIPPY-ALL-FEATURES-REMOVED (hard-coded command list),
#            MUT-ROOT-PATH-MISSPELLED (path regex blind to root files),
#            MUT-GATE-COUNT-STALE (a count nobody checked);
#   round 2: MUT-INVALIDATOR-MASTER-REMOVED (forbidding a value is not pinning
#            one), MUT-CI-MSRV-TOOLCHAIN-DRIFT (toolchain normalised away),
#            MUT-CI-BASH-GATE-OMITTED (files counted, invocations not);
#   round 3: MUT-CI-CARGO-TEST-STEP-DELETED (docs ⊇ CI is one-way),
#            MUT-MASTER-TRIGGERS-REMOVED (integration branch present, master
#            not required), MUT-CLAUDE-TEST-SCOPE-NARROWED (substring matched an
#            unrelated example), MUT-FORWARD-PATH-REUSED-AS-CURRENT (one
#            qualified occurrence marked the path for all of them).
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$script_dir/../.." && pwd)"
cd "$root"

failed=0
error() { echo "$*" >&2; failed=1; }

# block <file> <key>: the lines nested under a two-space-indented YAML key --
# an `on:` event such as `pull_request_target`, or a job such as `lint`. The
# block ends at the next key at that indentation or at the next top-level key.
# Keys may carry hyphens (`merge-gate`), or the lint block would run on into
# the next job and a gate invoked from the wrong job would count as invoked.
block() {
  awk -v key="  $2:" '
    $0 == key { inblock = 1; next }
    inblock && /^  [A-Za-z0-9_-]+:/ { inblock = 0 }
    inblock && /^[A-Za-z]/ { inblock = 0 }
    inblock { print }
  ' "$1"
}

# events <file>: the event keys under `on:`, one per line, in file order.
events() {
  awk '
    /^on:/ { inon = 1; next }
    inon && /^[A-Za-z]/ { inon = 0 }
    inon && match($0, /^  [A-Za-z_]+:/) { print substr($0, 3, RLENGTH - 3) }
  ' "$1"
}

# branches_line <file> <event>: the branch filter under one event, with the
# surrounding whitespace stripped. Every line under the event that starts with
# `branches:` is printed, so two filters -- or none -- fail the exact comparison.
branches_line() {
  block "$1" "$2" | grep -E '^\s*branches:' | sed -E 's/^\s+//; s/\s+$//' || true
}

# --- C1. every repository path a document names must resolve, per occurrence --
# MUT-ROOT-PATH-MISSPELLED: bare document names must exist at the root, not only
# directory-prefixed paths. MUT-FORWARD-PATH-REUSED-AS-CURRENT: a qualified
# forward reference used to mark the PATH, so a second, unqualified occurrence of
# the same missing path passed as a current pointer. Each occurrence is judged
# on its own window now. A qualifier may say the path is coming, or that it
# deliberately does not exist ("there is **no** rust-toolchain.toml").
marker='arrives with|arrive with|not yet|until that merges|until it merges|lands with|forward reference|\*\*no |there is \*\*?no|does not exist|must not exist'
for doc in CLAUDE.md CONTRIBUTING.md; do
  [[ -f "$doc" ]] || continue
  rooted=$(grep -oE '`(src|infra|\.github|acceptance|decisions|proposals|reviews|examples|fixtures|docs)/[A-Za-z0-9_./-]*`' "$doc" | tr -d '`' || true)
  bare=$(grep -oE '`[A-Za-z0-9][A-Za-z0-9_.-]*\.(md|toml|lock)`' "$doc" | tr -d '`' || true)
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    [[ -e "$path" ]] && continue
    while IFS= read -r line_no; do
      [[ -z "$line_no" ]] && continue
      from=$(( line_no > 3 ? line_no - 3 : 1 ))
      sed -n "${from},$(( line_no + 4 ))p" "$doc" | grep -qiE "$marker" \
        || error "$doc:$line_no names \`$path\`, which does not exist at this head; this occurrence is neither marked as a forward reference nor stated as deliberately absent"
    done < <(grep -nF -- "\`$path\`" "$doc" | cut -d: -f1)
  done < <(printf '%s\n%s\n' "$rooted" "$bare" | grep -v '^$' | sort -u)
done

# --- C2. documented gate commands EQUAL ci.yml's, both ways; floor; MSRV -------
# MUT-TEMPLATE-MSRV-REMOVED / MUT-CI-CLIPPY-ALL-FEATURES-REMOVED: the authority
# is ci.yml, never a list in this script. MUT-CI-CARGO-TEST-STEP-DELETED: a
# one-way "documents carry what CI runs" let CI drop a documented gate, so the
# comparison is an equality in both directions. MUT-CLAUDE-TEST-SCOPE-NARROWED:
# a substring search found the CI command inside an unrelated example
# (`tactus-build cargo test ...`), so documented commands are extracted as
# standalone command lines, not searched for as text. MUT-CI-MSRV-TOOLCHAIN-
# DRIFT: the toolchain pin was normalised away; the msrv job's commands must be
# documented pinned to the toolchain CI actually selects, and Cargo.toml's
# rust-version must agree with it.
python3 - "$root" <<'PY' || failed=1
import io, os, re, sys
root = sys.argv[1]
bad = False
def fail(msg):
    global bad
    print(msg, file=sys.stderr)
    bad = True

ci = io.open(os.path.join(root, ".github/workflows/ci.yml"), encoding="utf-8").read()

def job(name):
    # A job is a two-space-indented key under `jobs:`; its block ends at the
    # next key at that indentation (hyphens allowed: `merge-gate`).
    m = re.search(r"^  %s:\n(.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)" % re.escape(name), ci, re.S | re.M)
    return m.group(1) if m else ""

run_line = re.compile(r"^\s*- run:\s*(cargo .+?)\s*$", re.M)
ci_commands = run_line.findall(ci)
if not ci_commands:
    fail("could not read any cargo command from ci.yml")

# The MSRV triangle.
cargo_toml = io.open(os.path.join(root, "Cargo.toml"), encoding="utf-8").read()
rv = re.search(r'^rust-version\s*=\s*"([0-9]+\.[0-9]+(?:\.[0-9]+)?)"', cargo_toml, re.M)
msrv_job = job("msrv")
toolchains = re.findall(r"^\s*toolchain:\s*(\S+)\s*$", msrv_job, re.M)
msrv_commands = run_line.findall(msrv_job)
if not rv:
    fail("Cargo.toml carries no rust-version to pin the msrv job against")
if not msrv_job:
    fail("ci.yml has no msrv job")
elif len(toolchains) != 1:
    fail("ci.yml msrv job must select exactly one toolchain, found %r" % (toolchains,))
elif not msrv_commands:
    fail("ci.yml msrv job runs no cargo command")
toolchain = toolchains[0] if len(toolchains) == 1 else None
if rv and toolchain and toolchain != rv.group(1) and not toolchain.startswith(rv.group(1) + "."):
    fail("ci.yml msrv job runs toolchain %s but Cargo.toml rust-version is %s" % (toolchain, rv.group(1)))

# The floor the design packet names for release gates: cargo test --all-targets.
if not any(c.startswith("cargo test ") and "--all-targets" in c.split() for c in ci_commands):
    fail("ci.yml must run `cargo test --all-targets` (with any further flags): the packet's release gates require it")

# What each document must say: every CI command, the msrv job's pinned to the
# toolchain CI selects.
expected = set()
for c in ci_commands:
    if c in msrv_commands and toolchain:
        expected.add(c.replace("cargo ", "cargo +%s " % toolchain, 1))
    else:
        expected.add(c)

# What each document says: standalone cargo command lines -- a fenced-code line
# or a checklist item, optionally backticked, optionally followed by a comment.
doc_line = re.compile(r"^\s*(?:- \[[ xX]\] )?`?(cargo [^`#]*?)`?\s*(?:#.*)?$", re.M)
docs = ["CLAUDE.md", "CONTRIBUTING.md", ".github/pull_request_template.md"]
for doc in docs:
    p = os.path.join(root, doc)
    if not os.path.exists(p):
        continue
    text = io.open(p, encoding="utf-8").read()
    documented = set(m.group(1).strip() for m in doc_line.finditer(text))
    for c in sorted(expected - documented):
        fail("%s does not carry the gate command CI runs: %s" % (doc, c))
    for c in sorted(documented - expected):
        fail("%s documents a gate command CI does not run: %s" % (doc, c))
raise SystemExit(1 if bad else 0)
PY

# A claim that another document is stale must not outlive the fix.
if [[ -f CLAUDE.md && -f CONTRIBUTING.md ]] \
   && grep -Fq -- '--all-features' CONTRIBUTING.md \
   && grep -qiE 'CONTRIBUTING\.md.{0,40}(omits|is stale|does not (carry|include))' CLAUDE.md; then
  error "CLAUDE.md claims CONTRIBUTING.md omits --all-features, but CONTRIBUTING.md carries it at this head"
fi

# --- C3. the gate inventory: tree == lint-job invocations; CLAUDE.md's count --
# MUT-GATE-COUNT-STALE: a count is a fact about the tree; check it.
# MUT-CI-BASH-GATE-OMITTED: files in the tree prove nothing about CI running
# them; every test-*.sh must be invoked by a `- run: bash .github/scripts/<name>`
# line inside the lint job's own block, and the lint job must invoke nothing the
# tree does not carry. An invocation from another job does not count: block()
# ends at the next job.
tree_gates=$(ls .github/scripts/test-*.sh 2>/dev/null | sed 's|^\.github/scripts/||' | sort -u)
lint_gates=$(block .github/workflows/ci.yml lint \
  | grep -oE '^\s*- run: bash \.github/scripts/test-[A-Za-z0-9_.-]+\.sh\s*$' \
  | sed -E 's|^\s*- run: bash \.github/scripts/||; s|\s*$||' | sort -u || true)
[[ -n "$lint_gates" ]] || error "ci.yml's lint job invokes no .github/scripts/test-*.sh gate"
while IFS= read -r gate; do
  [[ -z "$gate" ]] && continue
  grep -qxF "$gate" <<< "$lint_gates" \
    || error ".github/scripts/$gate exists but ci.yml's lint job never runs it"
done <<< "$tree_gates"
while IFS= read -r gate; do
  [[ -z "$gate" ]] && continue
  grep -qxF "$gate" <<< "$tree_gates" \
    || error "ci.yml's lint job runs .github/scripts/$gate, which is not in the tree"
done <<< "$lint_gates"
actual_gates=$(printf '%s\n' "$tree_gates" | grep -c . || true)
if [[ -f CLAUDE.md ]]; then
  while IFS= read -r claimed; do
    [[ -z "$claimed" ]] && continue
    [[ "$claimed" == "$actual_gates" ]] \
      || error "CLAUDE.md claims $claimed \`test-*.sh\` gates; the tree has $actual_gates"
  done < <(grep -oE '[0-9]+ `test-\*\.sh` gates' CLAUDE.md | grep -oE '^[0-9]+')
  grep -qE '[0-9]+ `test-\*\.sh` gates' CLAUDE.md \
    || error "CLAUDE.md must state the gate count as 'N \`test-*.sh\` gates' so it can be checked"
fi

# --- C4. the trigger contract, pinned exactly --------------------------------
# MUT-CI-PR-BRANCH-MASKED: each event's own block, never the whole file.
# MUT-MASTER-TRIGGERS-REMOVED: requiring the integration branch to be present
# let master be removed; the branch list is compared for exact equality.
# MUT-INVALIDATOR-MASTER-REMOVED: forbidding the integration-branch name is not
# pinning master; the invalidator's filter is compared for exact equality too.
# The event set of every workflow is pinned as well, so a trigger cannot be
# added to the attestation path, or removed from the slice path, unnoticed.
# decisions/2026-08-21-stacked-slice-prs.md
slice_list='branches: [master, codex/parallelism-design]'
pin_events() {  # pin_events <file> <expected events, sorted, space separated>
  local f="$1" want="$2" got
  got="$(events "$f" | sort | tr '\n' ' ' | sed -E 's/ +$//')"
  [[ "$got" == "$want" ]] \
    || error "$f must trigger on exactly [$want], got [${got:-<none>}]"
}
pin_branches() {  # pin_branches <file> <event> <expected branches line>
  local f="$1" event="$2" want="$3" got
  got="$(branches_line "$f" "$event")"
  [[ "$got" == "$want" ]] \
    || error "$f: $event must carry exactly '$want', got: ${got:-<none>}"
}
for f in .github/workflows/ci.yml .github/workflows/pr-policy.yml \
         .github/workflows/frontier-review.yml .github/workflows/frontier-review-invalidate.yml; do
  [[ -f "$f" ]] || error "$f is missing"
done
if [[ -f .github/workflows/ci.yml ]]; then
  pin_events .github/workflows/ci.yml "pull_request push"
  pin_branches .github/workflows/ci.yml push "$slice_list"
  pin_branches .github/workflows/ci.yml pull_request "$slice_list"
fi
if [[ -f .github/workflows/pr-policy.yml ]]; then
  pin_events .github/workflows/pr-policy.yml "pull_request"
  pin_branches .github/workflows/pr-policy.yml pull_request "$slice_list"
fi
if [[ -f .github/workflows/frontier-review.yml ]]; then
  pin_events .github/workflows/frontier-review.yml "repository_dispatch"
fi
if [[ -f .github/workflows/frontier-review-invalidate.yml ]]; then
  pin_events .github/workflows/frontier-review-invalidate.yml "pull_request_target"
  pin_branches .github/workflows/frontier-review-invalidate.yml pull_request_target 'branches: [master]'
fi
# Attestation is never minted for a slice pull request, so neither attestation
# workflow may name the integration branch at all -- in a trigger or anywhere.
for wf in frontier-review frontier-review-invalidate; do
  f=".github/workflows/$wf.yml"
  [[ -f "$f" ]] || continue
  grep -Fq 'codex/parallelism-design' "$f" \
    && error "$f must stay master-only: attestation is never minted for a slice pull request"
done

if (( failed )); then
  echo "documentation consistency fixtures: FAIL" >&2
  exit 1
fi
echo "documentation consistency fixtures: PASS"
