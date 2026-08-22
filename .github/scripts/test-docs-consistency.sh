#!/usr/bin/env bash
# Documentation claims that go stale silently, checked against the tree.
#
# Both classes here are review findings, not hypotheses. On PR #20 the first
# draft of CLAUDE.md described a tree that did not exist at its own head
# (DOC-HEAD-LAYOUT: src/engine/, src/topology/, infra/ — the engine is the flat
# engine.rs) and asserted that CONTRIBUTING.md omitted --all-features while the
# same commit added it (DOC-STALE-GATE-CLAIM). Both were caught by a human-run
# frontier review; this makes them mechanical.
#
# On PR #23 the review asked for the same treatment of the workflow trigger
# lists (TRIGGER-CONTRACT-FIXTURE): slice pull requests must reach the
# integration branch's CI, and attestation must stay master-only. A grep
# fixture is the right strength there — the trigger list is a literal.
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$script_dir/../.." && pwd)"
cd "$root"

failed=0
error() { echo "$*" >&2; failed=1; }

# --- 1. every repository path CLAUDE.md names in backticks must resolve ------
# A forward reference is legitimate — CLAUDE.md may point at a file that
# arrives with another pull request — but it must say so on the same line, so
# a reader can tell "not here yet" from "wrong".
if [[ -f CLAUDE.md ]]; then
  marker='arrives with|arrive with|not yet|until that merges|until it merges|lands with|forward reference'
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    [[ -e "$path" ]] && continue
    # The prose is hard-wrapped, so a marker qualifying a path routinely sits on
    # a neighbouring line. Every mention must be marked, not merely one of them.
    unmarked=0
    while IFS= read -r line_no; do
      [[ -z "$line_no" ]] && continue
      from=$(( line_no > 2 ? line_no - 2 : 1 ))
      to=$(( line_no + 3 ))
      sed -n "${from},${to}p" CLAUDE.md | grep -qiE "$marker" || unmarked=1
    done < <(grep -nF -- "\`$path\`" CLAUDE.md | cut -d: -f1)
    (( unmarked )) && error "CLAUDE.md names \`$path\`, which does not exist at this head and is not marked as a forward reference"
  done < <(grep -oE '`(src|infra|\.github|acceptance|decisions|proposals|reviews|examples|fixtures|docs)/[A-Za-z0-9_./-]*`' CLAUDE.md \
             | tr -d '`' | sort -u)
fi

# --- 2. duplicated gate claims must agree with CONTRIBUTING.md --------------
# CLAUDE.md and CONTRIBUTING.md both carry the gate commands. Two copies of one
# fact drift; when they do, one of them tells a session to "fix" the other.
if [[ -f CLAUDE.md && -f CONTRIBUTING.md ]]; then
  for doc in CLAUDE.md CONTRIBUTING.md; do
    for needed in 'cargo fmt --check' \
                  'cargo clippy --all-targets --all-features -- -D warnings' \
                  'cargo test --all-targets --all-features'; do
      grep -Fq -- "$needed" "$doc" \
        || error "$doc does not carry the gate command: $needed"
    done
  done
  # A claim that the other file is stale must not survive the commit that fixes it.
  if grep -Fq -- '--all-features' CONTRIBUTING.md \
     && grep -qiE 'CONTRIBUTING\.md.{0,40}(omits|is stale|does not (carry|include))' CLAUDE.md; then
    error "CLAUDE.md claims CONTRIBUTING.md omits --all-features, but CONTRIBUTING.md carries it at this head"
  fi
fi

# --- 3. the pull-request template's gate list must match what CI runs -------
if [[ -f .github/pull_request_template.md ]]; then
  for needed in 'cargo clippy --all-targets --all-features -- -D warnings' \
                'cargo test --all-targets --all-features'; do
    grep -Fq -- "$needed" .github/pull_request_template.md \
      || error "the pull-request template's Validation list does not match CI: missing $needed"
  done
fi

# --- 4. trigger contract: slices reach CI, attestation stays master-only ----
# decisions/2026-08-21-stacked-slice-prs.md.
for wf in ci pr-policy; do
  f=".github/workflows/$wf.yml"
  [[ -f "$f" ]] || { error "$f is missing"; continue; }
  grep -Fq 'branches: [master, codex/parallelism-design]' "$f" \
    || error "$f must run for pull requests into the integration branch"
done
for wf in frontier-review frontier-review-invalidate; do
  f=".github/workflows/$wf.yml"
  [[ -f "$f" ]] || { error "$f is missing"; continue; }
  if grep -Fq 'codex/parallelism-design' "$f"; then
    error "$f must stay master-only: attestation is never minted for a slice pull request"
  fi
done
grep -Fq 'repository_dispatch:' .github/workflows/frontier-review.yml \
  || error "frontier-review.yml must be dispatch-triggered"
grep -Fq 'pull_request:' .github/workflows/frontier-review.yml \
  && error "frontier-review.yml must never trigger on pull_request"

if (( failed )); then
  echo "documentation consistency fixtures: FAIL" >&2
  exit 1
fi
echo "documentation consistency fixtures: PASS"
