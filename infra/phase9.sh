#!/usr/bin/env bash
# Phase 9 -- prove the environment reproduces known-green, then record a baseline.
#
# Runs the four cargo gates, the seven bash CI gates, and a timed baseline.
#
# Uses `tactus-build` rather than setting CARGO_TARGET_DIR directly. A
# per-worktree target dir gives 0% sccache reuse and forces a full rebuild in
# every worktree; the slot pool gives cargo-level artifact reuse instead.
# See the comment block in ~/bin/tactus-build.
set -uo pipefail   # NOT -e: every gate must run and report, not stop at the first failure

[ -f "$HOME/.cargo/env" ]  && . "$HOME/.cargo/env"
[ -f "$HOME/.tactus-env" ] && . "$HOME/.tactus-env"

REPO="${1:-/srv/tactus}"
cd "$REPO" || exit 1

echo "=========================================================="
echo " PHASE 9  --  $REPO"
echo "=========================================================="
echo "HEAD              $(git rev-parse --short HEAD)  $(git log -1 --format=%s)"
echo "working tree      $(git status --porcelain | tr '\n' ' ')"
echo "rustc / cargo     $(rustc --version | awk '{print $2}') / $(cargo --version | awk '{print $2}')"
echo "RUSTC_WRAPPER     ${RUSTC_WRAPPER:-UNSET}"
echo "CARGO_INCREMENTAL ${CARGO_INCREMENTAL:-UNSET}"
echo "nproc             $(nproc)"
echo "uptime            $(uptime -p)"
echo

declare -A RC ELAPSED
gate() {
  local name="$1"; shift
  local start end
  start=$(date +%s)
  "$@" >"/tmp/phase9_$name.log" 2>&1
  RC["$name"]=$?
  end=$(date +%s)
  ELAPSED["$name"]=$((end - start))
  if [ "${RC[$name]}" -eq 0 ]; then
    printf '  [ PASS ] %-8s %3ss\n' "$name" "${ELAPSED[$name]}"
  else
    printf '  [ FAIL ] %-8s %3ss  (rc=%s)\n' "$name" "${ELAPSED[$name]}" "${RC[$name]}"
    sed 's/^/           /' "/tmp/phase9_$name.log" | tail -25
  fi
}

echo "--- cargo gates (all four must pass) ---"
gate fmt    tactus-build cargo fmt --check
gate clippy tactus-build cargo clippy --all-targets --all-features -- -D warnings
gate test   tactus-build cargo test --all-targets --all-features
gate msrv   tactus-build cargo +1.85.0 check --locked --all-targets --all-features
echo

echo "--- test counts ---"
grep -h "test result:" "/tmp/phase9_test.log" 2>/dev/null | sed 's/^/  /'
echo

echo "--- seven bash CI gates ---"
# Invoked from the repo ROOT, the way ci.yml does it. test-pr-policy.sh derives
# its own path with ${BASH_SOURCE[0]%/*}, which strips nothing when the script is
# named without a slash, so `cd .github/scripts && bash test-pr-policy.sh` fails
# on that one script while the CI-style invocation works.
bash_pass=0; bash_fail=0
for s in .github/scripts/test-*.sh; do
  if bash "$s" >"/tmp/phase9_$(basename "$s").log" 2>&1; then
    printf '  [ PASS ] %s\n' "$(basename "$s")"; bash_pass=$((bash_pass+1))
  else
    printf '  [ FAIL ] %s\n' "$(basename "$s")"; bash_fail=$((bash_fail+1))
    tail -12 "/tmp/phase9_$(basename "$s").log" | sed 's/^/           /'
  fi
done
echo "  -> $bash_pass passed, $bash_fail failed"
echo

echo "--- syntax check on ALL scripts ---"
syn=0
for s in .github/scripts/*.sh; do bash -n "$s" || { echo "  SYNTAX FAIL: $s"; syn=1; }; done
[ $syn -eq 0 ] && echo "  all $(ls -1 .github/scripts/*.sh | wc -l) scripts pass bash -n"
echo

echo "--- baseline: time cargo test --all-targets --all-features (warm) ---"
{ time tactus-build cargo test --all-targets --all-features >/dev/null 2>&1; } 2>&1 | grep -E 'real|user|sys' | sed 's/^/  /'
echo

echo "=========================================================="
fail=0
for k in fmt clippy test msrv; do [ "${RC[$k]}" -ne 0 ] && fail=1; done
[ $bash_fail -ne 0 ] && fail=1
if [ $fail -eq 0 ]; then
  echo " PHASE 9: ALL GREEN"
else
  echo " PHASE 9: FAILURES ABOVE"
fi
echo "=========================================================="
exit $fail
