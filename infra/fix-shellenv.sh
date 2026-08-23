#!/usr/bin/env bash
# Make the upstroke build environment visible to NON-INTERACTIVE shells.
#
# Ubuntu's stock ~/.bashrc opens with:
#     case $- in *i*) ;; *) return;; esac
# so anything appended to the end is invisible to non-interactive shells. Agent
# subprocesses (claude -p, codex exec) and `ssh host 'cmd'` are non-interactive,
# which means they would inherit NO RUSTC_WRAPPER and NO CARGO_INCREMENTAL=0 --
# building without sccache at a near-zero hit rate, silently.
#
# Fix: source the upstroke env from ABOVE that early return, and from ~/.profile.
set -euo pipefail

BLOCK_START='# --- upstroke build env (must precede the non-interactive early return) ---'
BLOCK_END='# --- end upstroke build env ---'

strip_old() {
  local f="$1"
  [ -f "$f" ] || return 0
  # Drop any previous upstroke block and any bare appended sourcing lines.
  sed -i "/^${BLOCK_START}$/,/^${BLOCK_END}$/d" "$f"
  sed -i '/\.cargo\/env/d' "$f"
  sed -i '/upstroke-env/d' "$f"
}

block() {
  cat <<'EOF'
# --- upstroke build env (must precede the non-interactive early return) ---
[ -f "$HOME/.cargo/env" ]        && . "$HOME/.cargo/env"
[ -f "$HOME/.upstroke-env" ]       && . "$HOME/.upstroke-env"
# --- end upstroke build env ---
EOF
}

echo "=== patching ~/.bashrc (prepend) ==="
strip_old "$HOME/.bashrc"
{ block; cat "$HOME/.bashrc"; } > "$HOME/.bashrc.new"
mv "$HOME/.bashrc.new" "$HOME/.bashrc"
head -8 "$HOME/.bashrc"

echo
echo "=== patching ~/.profile (append; login shells) ==="
strip_old "$HOME/.profile"
block >> "$HOME/.profile"
tail -5 "$HOME/.profile"

echo
echo "=== VERIFY: non-interactive login shell ==="
bash -lc 'printf "RUSTC_WRAPPER=%s\nSCCACHE_DIR=%s\nSCCACHE_CACHE_SIZE=%s\nCARGO_INCREMENTAL=%s\ncargo=%s\nsccache=%s\n" \
  "${RUSTC_WRAPPER:-UNSET}" "${SCCACHE_DIR:-UNSET}" "${SCCACHE_CACHE_SIZE:-UNSET}" \
  "${CARGO_INCREMENTAL:-UNSET}" "$(command -v cargo || echo MISSING)" "$(command -v sccache || echo MISSING)"'

echo
echo "=== VERIFY: upstroke_target helper ==="
bash -lc 'type upstroke_target >/dev/null 2>&1 && echo "upstroke_target: defined" || echo "upstroke_target: MISSING"'
