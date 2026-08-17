#!/usr/bin/env bash
# =============================================================================
# tactusbox — full rebuild script
# =============================================================================
#
# Reproduces phases 1-4, 6 and 10 of the tactusbox build on a FRESH OVHcloud
# (or equivalent) dedicated server running Ubuntu Server 24.04 LTS.
#
# WHY THIS EXISTS
#   Neither OVH nor Hetzner resizes a dedicated box in place. Scaling means
#   rebuilding on new hardware. With this script that is 1-3 hours; without it,
#   most of a day.
#
# THIS SCRIPT DOES NOT INSTALL THE OS.
#   The OS install happens before any shell exists, so it cannot live here.
#   Replay `ovh-install-settings.json` (kept alongside this file) through the
#   OVHcloud API console FIRST, then run this script. That JSON pins the
#   critical detail: `"mountPoint": "/", "size": 0` means "all remaining space",
#   which is what keeps / at ~890 GiB instead of a ~20 GB root with the bulk on
#   /home. sccache lives in /var/cache and Docker in /var/lib, both under /.
#
# ENVIRONMENT ASSUMPTIONS (verified on the original build, 2026-08-17)
#   - Ubuntu 24.04.x LTS, kernel 6.8.x
#   - Login is the `ubuntu` user with passwordless sudo. NOT root: OVH's Ubuntu
#     template follows the cloud-image convention and refuses root SSH with
#     "Please login as the user ubuntu rather than the user root."
#   - 2x NVMe in soft RAID1 (md2 -> /boot, md3 -> /), both [UU]
#   - 32 threads, ~125 GB RAM
#
# INTERACTIVE STEPS ARE NOT AUTOMATED AND MUST NOT BE.
#   Three things need a human. The script stops and tells you:
#     1. `tailscale up --ssh`      (browser login URL)
#     2. `claude setup-token`      (paste-back OAuth flow)
#     3. `codex login --device-auth` (device code at chatgpt.com)
#   No credential is ever written into this script or echoed by it.
#
# USAGE
#   ./setup.sh              # run every phase in order
#   ./setup.sh 2 3 4        # run only the named phases
#   ./setup.sh --list       # show phases
#
# =============================================================================

set -euo pipefail

readonly TACTUS_REPO="https://github.com/keybindings/tactus.git"
readonly TACTUS_DIR="/srv/tactus"
readonly TACTUS_ENV="$HOME/.tactus-env"
readonly RAMTARGET="/mnt/ramtarget"
readonly RAMTARGET_SIZE="48G"
readonly SCCACHE_DIR="/var/cache/sccache"
readonly SCCACHE_SIZE="100G"
readonly MSRV="1.85.0"
readonly NODE_MAJOR="22"
readonly CODEX_VERSION="0.147.0"
readonly SWAPFILE="/swapfile"
readonly SWAPFILE_SIZE="32G"

# ---------------------------------------------------------------- output ------
readonly C_OK=$'\033[32m'; readonly C_ERR=$'\033[31m'
readonly C_WARN=$'\033[33m'; readonly C_HEAD=$'\033[1;36m'; readonly C_OFF=$'\033[0m'

phase()  { printf '\n%s=== PHASE %s: %s ===%s\n' "$C_HEAD" "$1" "$2" "$C_OFF"; }
ok()     { printf '  %s[ ok ]%s %s\n' "$C_OK" "$C_OFF" "$*"; }
warn()   { printf '  %s[warn]%s %s\n' "$C_WARN" "$C_OFF" "$*"; }
fail()   { printf '  %s[FAIL]%s %s\n' "$C_ERR" "$C_OFF" "$*" >&2; return 1; }
note()   { printf '         %s\n' "$*"; }

# Halt for a human. Never try to automate past this.
handback() {
  printf '\n%s>>> MANUAL STEP REQUIRED <<<%s\n' "$C_WARN" "$C_OFF"
  printf '%s\n' "$@"
  printf '\nRe-run this script with the remaining phases once done.\n\n'
}

have() { command -v "$1" >/dev/null 2>&1; }

# Assert a binary exists; collect failures rather than dying on the first.
require_bins() {
  local missing=() c
  for c in "$@"; do
    if have "$c"; then ok "$c -> $(command -v "$c")"; else missing+=("$c"); fi
  done
  if (( ${#missing[@]} )); then fail "missing: ${missing[*]}"; fi
}

# Append a line to a file only if it is not already present (idempotency).
ensure_line() {
  local line="$1" file="$2"
  touch "$file"
  grep -qxF "$line" "$file" || printf '%s\n' "$line" >> "$file"
}

# Same, for root-owned system files.
#
# This exists because the first version of this script used plain ensure_line on
# /etc/fstab, which fails with "Permission denied" -- and because the tmpfs and
# swapfile had ALREADY been mounted imperatively by that point, everything looked
# fine. The failure only shows up at the next reboot, when /mnt/ramtarget is not
# mounted and every build silently writes to disk instead of RAM, and swap drops
# back to 1 GiB. Nothing errors; it just gets slow. Caught 2026-08-17.
ensure_line_sudo() {
  local line="$1" file="$2"
  sudo grep -qxF "$line" "$file" 2>/dev/null \
    || printf '%s\n' "$line" | sudo tee -a "$file" >/dev/null
}

# Make the build env visible to NON-INTERACTIVE shells.
#
# THIS IS NOT COSMETIC. Ubuntu's stock ~/.bashrc opens with
#     case $- in *i*) ;; *) return;; esac
# so anything APPENDED to it is invisible to non-interactive shells. Agent
# subprocesses (claude -p, codex exec) and `ssh host 'cmd'` are non-interactive.
# Append instead of prepend and those workers silently build with no
# RUSTC_WRAPPER and no CARGO_INCREMENTAL=0 -- i.e. no sccache and a near-zero
# hit rate, with nothing in the output to tell you. So: PREPEND to .bashrc
# (above the early return) and also append to .profile for login shells.
readonly ENV_BLOCK_START='# --- tactus build env (must precede the non-interactive early return) ---'
readonly ENV_BLOCK_END='# --- end tactus build env ---'

ensure_shell_env() {
  local f block
  block=$(cat <<EOF
${ENV_BLOCK_START}
[ -f "\$HOME/.cargo/env" ]  && . "\$HOME/.cargo/env"
[ -f "\$HOME/.tactus-env" ] && . "\$HOME/.tactus-env"
${ENV_BLOCK_END}
EOF
)
  for f in "$HOME/.bashrc" "$HOME/.profile"; do
    touch "$f"
    # Idempotent: drop any previous block and stray sourcing lines first.
    sed -i "/^${ENV_BLOCK_START}$/,/^${ENV_BLOCK_END}$/d" "$f"
    sed -i '/\.cargo\/env/d; /tactus-env/d' "$f"
  done
  # .bashrc must be PREPENDED (early return lives near the top).
  { printf '%s\n' "$block"; cat "$HOME/.bashrc"; } > "$HOME/.bashrc.tmp"
  mv "$HOME/.bashrc.tmp" "$HOME/.bashrc"
  # .profile has no such guard; appending is fine.
  printf '%s\n' "$block" >> "$HOME/.profile"

  # Verify in a real non-interactive shell rather than trusting the edit.
  local got
  got=$(bash -lc 'printf "%s|%s" "${RUSTC_WRAPPER:-UNSET}" "${CARGO_INCREMENTAL:-UNSET}"')
  ok "non-interactive shell sees RUSTC_WRAPPER|CARGO_INCREMENTAL = ${got}"
  case "$got" in
    UNSET*|*UNSET) warn "env NOT visible to non-interactive shells -- sccache will be bypassed" ;;
  esac
}

# =============================================================================
# PHASE 1 — Access: hostname, tmux, Tailscale, firewall, sshd hardening
# =============================================================================
#
# ORDERING IS SAFETY-CRITICAL. Read before editing.
#
# `ufw` here allows ONLY tailscale0. Enabling it before the tailnet is up and
# independently verified locks you out of the box completely -- the public IP
# stops answering and there is no other route in except OVH Serial-over-LAN.
#
# So phase 1 is deliberately split:
#   1a  hostname + tmux + install tailscale   (safe, no lockout risk)
#   1b  tailscale up --ssh                    (MANUAL: browser login)
#   1c  ufw + PasswordAuthentication no       (ONLY after 1b is verified)
#
# Before running 1c you must have proven, from a SECOND machine already on the
# tailnet, that `ssh <box>` works over the tailnet -- while the original
# public-IP session is still open. Verify, do not assume.
#
phase_1a() {
  phase 1a "hostname, tmux, Tailscale install"

  sudo hostnamectl set-hostname tactusbox
  # Keep /etc/hosts in step or sudo emits "unable to resolve host" on every call.
  if ! grep -q tactusbox /etc/hosts; then
    printf '127.0.1.1 tactusbox\n' | sudo tee -a /etc/hosts >/dev/null
  fi
  ok "hostname: $(hostname)"

  sudo apt-get update -qq
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq tmux
  # Long agent stages run 10-40 min detached; without tmux a dropped SSH
  # connection kills a 40-minute worker.
  ensure_line 'set -g history-limit 200000' "$HOME/.tmux.conf"
  ensure_line 'set -g mouse on'             "$HOME/.tmux.conf"
  ok "tmux $(tmux -V | awk '{print $2}') configured"

  if ! have tailscale; then
    curl -fsSL https://tailscale.com/install.sh | sh
  fi
  ok "tailscale $(tailscale version | head -1)"
  systemctl is-active --quiet tailscaled && ok "tailscaled active" || fail "tailscaled not active"
}

phase_1b() {
  phase 1b "Tailscale authentication (MANUAL)"
  if tailscale status >/dev/null 2>&1; then
    ok "already authenticated to tailnet"
    tailscale status | head -5
    return 0
  fi
  handback \
"Run this INSIDE tmux so the login survives a dropped connection:

    tmux new-session -s tsup 'sudo tailscale up --ssh'

Visit the printed https://login.tailscale.com/... URL to authorise the node.

You must ALSO install Tailscale on whichever machine you drive this box from
and join it to the same tailnet, or phase 1c will lock you out."
  return 1
}

phase_1c() {
  phase 1c "Firewall + sshd hardening (DESTRUCTIVE IF UNVERIFIED)"

  # Refuse to proceed unless the tailnet is genuinely up. This guard is the
  # difference between a hardened box and a bricked one.
  if ! tailscale status >/dev/null 2>&1; then
    fail "tailnet is NOT up. Enabling ufw now would lock you out. Run phase 1b."
  fi
  if ! ip link show tailscale0 >/dev/null 2>&1; then
    fail "tailscale0 interface does not exist. Refusing to enable ufw."
  fi
  ok "tailscale0 present, tailnet up"

  warn "About to restrict inbound traffic to tailscale0 only."
  note "Confirm NOW, from a second machine on the tailnet, that ssh works,"
  note "with your current public-IP session still open. Ctrl-C if unsure."
  read -r -p "  Type YES to continue: " confirm
  [ "$confirm" = "YES" ] || { note "aborted"; return 1; }

  # DEADMAN SWITCH. If the rules below are wrong, the box becomes unreachable
  # and the only way back in is OVH Serial-over-LAN. This timer disables ufw
  # after 5 minutes unless we cancel it, turning a lockout into a wait.
  sudo systemd-run --on-active=300 --unit=ufw-deadman /usr/sbin/ufw --force disable
  warn "deadman armed: ufw auto-disables in 5 min unless cancelled"

  sudo ufw default deny incoming
  sudo ufw default allow outgoing
  sudo ufw allow in on tailscale0
  # Not in the original brief, added deliberately: without inbound 41641/udp,
  # Tailscale cannot accept direct WireGuard connections and silently falls back
  # to relaying via DERP. Still works, but slower -- and we have just made the
  # tailnet the ONLY route in, so its performance now matters. Authenticated
  # WireGuard, so opening it costs nothing security-wise.
  sudo ufw allow 41641/udp comment 'tailscale direct (else it relays via DERP)'
  sudo ufw --force enable
  sudo ufw status verbose

  echo
  warn "VERIFY NOW from the other machine: ssh over the tailnet must work."
  note "If it does not, do nothing -- the deadman will restore access in <5 min."
  read -r -p "  Tailnet ssh confirmed working? Type YES to cancel the deadman: " confirm2
  if [ "$confirm2" = "YES" ]; then
    sudo systemctl stop ufw-deadman.timer 2>/dev/null || true
    sudo systemctl reset-failed ufw-deadman.service 2>/dev/null || true
    ok "deadman cancelled; firewall stands"
  else
    warn "deadman left armed -- ufw will disable itself shortly"
    return 1
  fi

  # Password auth. NOTE: on the OVH Ubuntu 24.04 template this is ALREADY `no`,
  # set by /etc/ssh/sshd_config.d/60-cloudimg-settings.conf, which takes
  # precedence over sshd_config. We write it explicitly anyway so that removing
  # that drop-in cannot silently re-enable password login.
  if ! grep -qE '^PasswordAuthentication no' /etc/ssh/sshd_config; then
    printf '\n# tactus: explicit, redundant with sshd_config.d/60-cloudimg-settings.conf.\nPasswordAuthentication no\nKbdInteractiveAuthentication no\n' \
      | sudo tee -a /etc/ssh/sshd_config >/dev/null
  fi
  # Validate BEFORE reloading. A bad config plus a reload is how people lose a box.
  #
  # sshd -t needs the privilege separation directory to exist. Ubuntu 24.04 uses
  # SOCKET ACTIVATION (ssh.socket active, ssh.service inactive until a connection
  # arrives), and /run/sshd is created by ssh.service's RuntimeDirectory=sshd.
  # So on a freshly booted box /run/sshd does not exist and `sshd -t` fails with
  # "Missing privilege separation directory" -- a FALSE negative that looks like
  # a broken config. Create it first so the check tests what we think it tests.
  sudo mkdir -p /run/sshd
  sudo sshd -t || fail "sshd config invalid -- NOT reloading, you would lose access"
  ok "sshd config validates"
  sudo systemctl reload ssh
  ok "password auth disabled: $(sudo sshd -T | grep -i '^passwordauthentication')"
  note "Verify a fresh session works before closing this one."
}

# =============================================================================
# PREFLIGHT — install the token health check and its cron
# =============================================================================
#
# Expects ~/bin/tactus-preflight and 99-tactus-preflight to sit alongside this
# script (they travel together). Run AFTER phase 5, since the preflight proves
# tokens with live calls and cannot pass before they exist.
#
phase_preflight() {
  phase preflight "token health check + cron"
  [ -x "$HOME/bin/tactus-preflight" ] || fail "~/bin/tactus-preflight missing"

  "$HOME/bin/tactus-preflight" || fail "preflight does not pass -- fix auth before installing cron"

  # 6-hourly, offset off the hour to avoid the cron stampede.
  ( crontab -l 2>/dev/null | grep -v 'tactus-preflight'
    echo "7 */6 * * * $HOME/bin/tactus-preflight --quiet >> $HOME/.tactus-preflight.cron.log 2>&1" ) | crontab -
  ok "cron installed: $(crontab -l | grep tactus-preflight)"

  # No MTA and no push channel on this box, so failures surface at login.
  if [ -f "$(dirname "$0")/99-tactus-preflight" ]; then
    sudo install -m 755 "$(dirname "$0")/99-tactus-preflight" /etc/update-motd.d/99-tactus-preflight
    ok "MOTD banner installed"
  else
    warn "99-tactus-preflight not found alongside setup.sh; MOTD banner skipped"
  fi
}

# =============================================================================
# PHASE 2 — Base packages
# =============================================================================
#
# Two are load-bearing and easy to skip:
#   jq         - 5 of the 7 CI gate scripts in .github/scripts/ invoke it.
#   bubblewrap - enforces Codex's read-only sandbox. Without the system copy,
#                `codex exec` warns "could not find bubblewrap on PATH" and
#                silently falls back to a bundled one. It works either way, but
#                this is the ONLY containment mechanism for the reviewer, so we
#                want the real /usr/bin/bwrap.
#
phase_2() {
  phase 2 "Base packages"
  sudo apt-get update -qq
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
    build-essential pkg-config libssl-dev \
    git curl jq tmux mosh ripgrep unzip rsync bubblewrap \
    ca-certificates gnupg
  require_bins git curl jq tmux mosh rg rsync bwrap cc
  ok "jq $(jq --version), bubblewrap $(bwrap --version | awk '{print $2}')"
}

# =============================================================================
# PHASE 3 — Rust toolchain
# =============================================================================
#
# 1.85.0 is REQUIRED: the MSRV gate is
#   cargo +1.85.0 check --locked --all-targets --all-features
# There is no rust-toolchain.toml in the repo -- toolchain selection is explicit
# at call sites, so nothing auto-corrects a wrong default.
#
phase_3() {
  phase 3 "Rust toolchain (stable + ${MSRV} MSRV)"
  if ! have rustup; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --component rustfmt,clippy
  fi
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"
  rustup toolchain install "$MSRV"

  ok "$(rustc --version)"
  ok "$(cargo --version)"
  ok "$(rustfmt --version)"
  ok "$(cargo clippy --version)"
  cargo "+${MSRV}" --version >/dev/null 2>&1 \
    && ok "MSRV resolves: $(cargo "+${MSRV}" --version)" \
    || fail "cargo +${MSRV} does not resolve"

  ensure_shell_env
}

# =============================================================================
# PHASE 4 — Node and the two agent CLIs
# =============================================================================
#
# codex is pinned to EXACTLY 0.147.0 -- the workflow depends on that version's
# semantics. Invocation notes worth keeping with the install:
#   - `codex exec` in 0.147.0 REJECTS `-a never`; that flag no longer exists.
#   - working review invocation:
#       codex exec -m gpt-5.6-sol -c 'model_reasoning_effort="max"' \
#         --strict-config -s read-only --ephemeral -C <dir> -o <file> -
#   - `--skip-git-repo-check` is REQUIRED whenever -C points outside a git repo,
#     or codex exits "Not inside a trusted directory" before making any call.
#
phase_4() {
  phase 4 "Node ${NODE_MAJOR}, claude-code, codex ${CODEX_VERSION}"
  if ! have node; then
    curl -fsSL "https://deb.nodesource.com/setup_${NODE_MAJOR}.x" | sudo -E bash -
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq nodejs
  fi
  ok "node $(node --version), npm $(npm --version)"

  sudo npm install -g @anthropic-ai/claude-code
  sudo npm install -g "@openai/codex@${CODEX_VERSION}"

  ok "claude $(claude --version)"
  local cv; cv="$(codex --version)"
  [ "$cv" = "codex-cli ${CODEX_VERSION}" ] \
    && ok "codex $cv" \
    || fail "codex version is '$cv', expected 'codex-cli ${CODEX_VERSION}'"
}

# =============================================================================
# PHASE 5 — Authentication (MANUAL) and the preflight check
# =============================================================================
phase_5() {
  phase 5 "Authentication (MANUAL)"
  handback \
"Both flows are proven to work headless -- no tunnelling or workarounds needed.

  1. claude setup-token
     Prints a URL using redirect_uri=platform.claude.com/oauth/code/callback
     &code=true -- a paste-back flow, so no localhost callback and no SSH
     tunnel. It emits a long-lived token that it saves NOWHERE. Put it in
     ${TACTUS_ENV} (mode 600) as:
         export CLAUDE_CODE_OAUTH_TOKEN=<token>

  2. codex login --device-auth
     Prints a chatgpt.com URL and a short code.

Then run: ./setup.sh preflight"
  return 1
}

# =============================================================================
# PHASE 6 — Build caching
# =============================================================================
#
# CARGO_INCREMENTAL=0 is REQUIRED, not stylistic: sccache cannot cache
# incremental artifacts and silently drops to a near-zero hit rate otherwise.
# It costs a little on rebuilds of the same tree and wins substantially across
# different worktrees, which is the actual pattern here.
#
# Target dirs go on tmpfs -- the single biggest wall-clock win available with
# 128 GB. They must be PER-WORKTREE: a shared target dir serialises builds on
# cargo's directory lock, which silently destroys the parallelism this box
# exists to provide.
#
phase_6() {
  phase 6 "Build caching: sccache + tmpfs + swap"
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"

  have sccache || cargo install sccache --locked
  ok "$(sccache --version)"

  sudo mkdir -p "$SCCACHE_DIR"
  sudo chown "$(id -u):$(id -g)" "$SCCACHE_DIR"

  sudo mkdir -p "$RAMTARGET"
  if ! mountpoint -q "$RAMTARGET"; then
    sudo mount -t tmpfs -o "size=${RAMTARGET_SIZE},mode=1777" tmpfs "$RAMTARGET"
  fi
  ensure_line_sudo "tmpfs	${RAMTARGET}	tmpfs	size=${RAMTARGET_SIZE},mode=1777	0	0" /etc/fstab
  ok "tmpfs at ${RAMTARGET} ($(df -h "$RAMTARGET" | awk 'NR==2{print $2}'))"

  # The OVH install leaves only ~1 GiB of swap across two unmirrored 512M
  # partitions. With a 48G tmpfs competing for RAM, give the kernel somewhere to
  # evict cold pages instead of invoking the OOM killer on a 40-minute worker.
  if ! swapon --show=NAME --noheadings | grep -q "$SWAPFILE"; then
    sudo fallocate -l "$SWAPFILE_SIZE" "$SWAPFILE"
    sudo chmod 600 "$SWAPFILE"
    sudo mkswap "$SWAPFILE" >/dev/null
    sudo swapon "$SWAPFILE"
    ensure_line_sudo "${SWAPFILE}	none	swap	sw	0	0" /etc/fstab
  fi
  ok "swap total: $(free -h | awk '/Swap/{print $2}')"

  # Persistence is not optional here and is invisible until a reboot, so assert
  # it rather than assuming the fstab edits landed.
  sudo grep -q "$RAMTARGET" /etc/fstab \
    && ok "tmpfs persisted in /etc/fstab" \
    || fail "tmpfs NOT in /etc/fstab -- it will vanish on reboot"
  sudo grep -q "$SWAPFILE" /etc/fstab \
    && ok "swapfile persisted in /etc/fstab" \
    || fail "swapfile NOT in /etc/fstab -- swap drops to ~1 GiB on reboot"
  sudo findmnt --verify >/dev/null 2>&1 \
    && ok "fstab validates" \
    || warn "findmnt --verify reported issues -- inspect before rebooting"

  cat > "$TACTUS_ENV.phase6" <<EOF
# Build caching -- sourced from ${TACTUS_ENV}
export RUSTC_WRAPPER=sccache
export SCCACHE_DIR=${SCCACHE_DIR}
export SCCACHE_CACHE_SIZE=${SCCACHE_SIZE}
# REQUIRED: sccache cannot cache incremental artifacts.
export CARGO_INCREMENTAL=0
# Size of the target-dir slot pool used by tactus-build. Set at or above your
# maximum concurrent build count: too few and builds queue on a slot lock, too
# many and you dilute cache reuse across more distinct paths.
export TACTUS_SLOTS=8
export TACTUS_RAMTARGET=${RAMTARGET}

# DO NOT set CARGO_TARGET_DIR per worktree. Use \`tactus-build <cmd>\` instead.
#
# Measured on this box 2026-08-17 with two worktrees at an identical commit:
#     source differs, target same   -> 54/55 sccache hits (98.18%)
#     source same,    target differs->  0/55 sccache hits ( 0.00%)
# The cache key is poisoned by CARGO_TARGET_DIR, not by the source path: every
# rustc call carries -L dependency=<target>/... and --extern <target>/...
#
# A target dir per worktree is an UNBOUNDED set of paths, so no two worktrees
# ever share cache entries. A bounded slot pool keeps isolation (cargo's lock
# only conflicts between CONCURRENT builds) while making paths repeat.
#
# Wall clock, second worktree, this project: 8.80s -> 4.62s.
tactus_target() {
  echo "tactus_target is deprecated -- use: tactus-build cargo <args>" >&2
  return 1
}
EOF
  touch "$TACTUS_ENV"; chmod 600 "$TACTUS_ENV"
  ensure_line "source ${TACTUS_ENV}.phase6" "$TACTUS_ENV"
  ensure_shell_env
  ok "build env written to ${TACTUS_ENV}.phase6"

  # SOLVED (2026-08-17). Earlier note here said cross-worktree hits were 0% and
  # probably unfixable. A controlled experiment showed otherwise -- see the
  # comment block in ~/bin/tactus-build. Short version: the cache key is poisoned
  # by CARGO_TARGET_DIR, NOT by the source path, so a bounded slot pool restores
  # reuse while keeping concurrent builds isolated. 8.80s -> 4.62s on the second
  # worktree. Use `tactus-build cargo ...`, never a per-worktree CARGO_TARGET_DIR.
  if [ -x "$HOME/bin/tactus-build" ]; then
    ok "tactus-build present -- use it instead of setting CARGO_TARGET_DIR"
  else
    warn "~/bin/tactus-build missing; it should travel alongside setup.sh"
  fi
}

# =============================================================================
# PHASE 10 — Docker
# =============================================================================
phase_10() {
  phase 10 "Docker CE"
  if ! have docker; then
    sudo install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/ubuntu/gpg \
      | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    sudo chmod a+r /etc/apt/keyrings/docker.gpg
    printf 'deb [arch=%s signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu %s stable\n' \
      "$(dpkg --print-architecture)" "$(. /etc/os-release && echo "$VERSION_CODENAME")" \
      | sudo tee /etc/apt/sources.list.d/docker.list >/dev/null
    sudo apt-get update -qq
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
      docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  fi
  sudo systemctl enable --now docker
  sudo usermod -aG docker "$USER" || true
  ok "$(sudo docker --version)"
  sudo docker run --rm hello-world >/dev/null 2>&1 \
    && ok "docker run --rm hello-world succeeded" \
    || fail "hello-world failed"
  note "Group change needs a new login before rootless 'docker' works for $USER."
}

# =============================================================================
# main
# =============================================================================
readonly PHASES=(1a 1b 1c 2 3 4 5 preflight 6 10)

usage() {
  printf 'usage: %s [phase ...]\n\nphases: %s\n' "$0" "${PHASES[*]}"
  printf '  1a  hostname, tmux, tailscale install\n'
  printf '  1b  tailscale auth              (MANUAL)\n'
  printf '  1c  ufw + sshd hardening        (verify tailnet first!)\n'
  printf '  2   base packages\n'
  printf '  3   rust toolchain + MSRV\n'
  printf '  4   node, claude-code, codex\n'
  printf '  5   claude/codex auth           (MANUAL)\n'
  printf '  preflight  token health check + 6-hourly cron + MOTD banner\n'
  printf '  6   sccache, tmpfs, swap\n'
  printf '  10  docker\n'
}

main() {
  if [ "${1:-}" = "--list" ] || [ "${1:-}" = "-h" ]; then usage; exit 0; fi
  local want=("$@")
  (( ${#want[@]} )) || want=("${PHASES[@]}")
  local p
  for p in "${want[@]}"; do
    if declare -F "phase_${p}" >/dev/null; then
      "phase_${p}" || warn "phase ${p} stopped (see above)"
    else
      warn "unknown phase: ${p}"
    fi
  done
  printf '\n%sdone%s\n' "$C_HEAD" "$C_OFF"
}

main "$@"
