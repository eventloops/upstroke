# infra/

Provisioning and operations tooling for the dedicated build box that runs the
multi-agent workflow.

Neither OVH nor Hetzner resizes a dedicated server in place, so scaling means
rebuilding on new hardware. With this directory that is 1–3 hours; without it,
most of a day.

## Rebuilding from nothing

1. **`ovh-install-settings.json`** — replay through the OVHcloud API console.
   This step cannot live in `setup.sh` because it happens before any shell
   exists. Substitute your own `sshKey` first.

   The load-bearing value is `"mountPoint": "/", "size": 0`. `0` means *use all
   remaining space*, which yields ~890 GiB on `/`. OVH's default is a ~20 GB
   root with the bulk on `/home`, and sccache (`/var/cache`) plus Docker
   (`/var/lib`) both live under `/`, so a small root fills during the first
   real build.

2. **`setup.sh`** — everything else. `./setup.sh --list` for phases; individual
   phases are runnable (`./setup.sh 2 3 4`).

   ```
   1a  hostname, tmux, tailscale install
   1b  tailscale auth              (MANUAL — browser login)
   1c  ufw + sshd hardening        (verify tailnet first, deadman-switched)
   2   base packages
   3   rust toolchain + MSRV
   4   node, claude-code, codex
   5   claude/codex auth           (MANUAL — paste-back + device code)
   preflight  token health check + 6-hourly cron + MOTD banner
   6   sccache, tmpfs, swap
   10  docker
   ```

   The three manual steps are deliberately not automated: they are interactive
   OAuth flows, and a script that pretends otherwise would only fail later and
   less clearly.

## Operations

| File | Purpose |
|---|---|
| `tactus-preflight` | Proves both agent CLIs can make a **live call**. Cron'd 6-hourly. |
| `tactus-watch` | Polls the watched branch; runs the full gates on each new commit. |
| `tactus-build` | Wraps cargo with a slot-pooled `CARGO_TARGET_DIR`. **Use instead of setting it yourself.** |
| `phase9.sh` | The gate runner: 4 cargo gates, 7 bash CI gates, `bash -n` on all scripts, timed baseline. Exits non-zero on failure. |
| `tactus-session` + `.service` | Long-lived tmux orchestrator session, started at boot via a lingering systemd user service. |
| `99-tactus-preflight` | MOTD banner surfacing failing tokens or failing gates at login. |
| `fix-shellenv.sh` | Standalone version of the non-interactive-shell fix (also in `setup.sh`). |

## Findings worth keeping

`REPORT.md` records the full build with 24 numbered differences from the
original plan. Four are worth knowing before touching any of this:

**Never set `CARGO_TARGET_DIR` per worktree.** Measured with two worktrees at an
identical commit: source path differs / target path same → **98% sccache hits**;
source path same / target path differs → **0%**. The cache key is poisoned by
the target directory, not the source. A directory per worktree is an unbounded
set of paths, so nothing is ever reused. `tactus-build` uses a bounded slot pool
instead — full isolation between concurrent builds, but repeating paths.
Second-worktree build: **8.82 s → 4.94 s**, 1 crate rebuilt instead of 55.

**Environment must be sourced above the non-interactive guard.** Ubuntu's
`.bashrc` opens with `case $- in *i*) ;; *) return;; esac`. Anything appended is
invisible to non-interactive shells — which is what agent subprocesses and
`ssh host 'cmd'` are. Get this wrong and workers build with no sccache and
`CARGO_INCREMENTAL` unset, silently, at a near-zero hit rate. The tmux
auto-attach is the mirror image and must sit *below* that guard.

**Check exit codes, not output.** `codex login status` prints "Not logged in"
and exits **0**. `git rev-parse <unknown-ref>` prints its argument to stdout and
errors only on stderr. Both produce confident false results in a naive check.

**Imperative success does not imply persistent success.** A tmpfs and swapfile
were mounted correctly but never written to `/etc/fstab`; everything looked
right until a reboot would have silently moved every build from RAM to disk.
Assert persistence, then reboot and verify.
