#!/usr/bin/env bash
# Rename the project from `tactus` to `upstroke`, deterministically.
#
# One-shot. Run it once from anywhere inside a checkout whose tree still says
# `tactus`, then build, test and commit the result. It is committed so the
# identical transformation can be applied to any other branch that predates the
# rename, so that the two sides merge without conflicts:
#
#     git show master:scripts/rename-tactus-to-upstroke.sh | bash
#
# What it does, in order:
#
#   1. Substitutes every case form of the name (`tactus`, `Tactus`, `TACTUS`)
#      in every tracked, non-binary file except `Cargo.lock` and this script.
#      Plain substitution, not `\b`-bounded, on purpose: `TACTUS_HOME`,
#      `tactus_version`, `TactusError` and `libtactus-*.rlib` must all change.
#      The build-box hostname `tactusbox` is the one deliberate exception; it
#      is parked behind a placeholder and restored afterwards.
#      GitHub URLs are moved to the repository's current home at the same time:
#      the tree still carried `keybindings/tactus` from before the organisation
#      transfer, and a bare name swap would have produced `keybindings/upstroke`,
#      which has never existed. The target is `eventloops/upstroke`.
#   2. `git mv`s every tracked path that contains the name, longest first.
#   3. Lets cargo rewrite the root package entry in `Cargo.lock`; the lock is
#      never edited by hand.
#   4. Runs `cargo fmt`: identifier lengths changed, so rustfmt re-wraps some
#      lines. That reflow is part of the deterministic output.
#
# It is not idempotent: a second run would rename the "formerly `tactus`" note
# that the README and CHANGELOG add after the mechanical pass.
set -euo pipefail

self="scripts/rename-tactus-to-upstroke.sh"
cd "$(git rev-parse --show-toplevel)"

# 1. Text substitution. `git grep -I` skips binary files; the pathspec keeps
#    the lock and this script out of the edit set. grep exits 1 when nothing
#    matches, which is "nothing to do" rather than an error.
{ git grep -z -l -I -i -e tactus -- . ":(exclude)Cargo.lock" ":(exclude)${self}" || [ "$?" -eq 1 ]; } \
  | xargs -0 -r sed -i \
      -e 's/tactusbox/__KEEP_HOST__/g' \
      -e 's#keybindings/tactus#eventloops/tactus#g' \
      -e 's#keybindings\.github\.io/tactus#eventloops.github.io/tactus#g' \
      -e 's/Tactus/Upstroke/g; s/tactus/upstroke/g; s/TACTUS/UPSTROKE/g' \
      -e 's/__KEEP_HOST__/tactusbox/g'

# 2. Paths. Longest first so that nothing is moved out from under a later move;
#    `mkdir -p` because `git mv` will not create a missing destination directory.
{ git ls-files | grep -i tactus || [ "$?" -eq 1 ]; } \
  | awk '{ print length, $0 }' | sort -rn | cut -d' ' -f2- \
  | while IFS= read -r old; do
      new="$(printf '%s\n' "$old" | sed \
        -e 's/tactusbox/__KEEP_HOST__/g' \
        -e 's/Tactus/Upstroke/g; s/tactus/upstroke/g; s/TACTUS/UPSTROKE/g' \
        -e 's/__KEEP_HOST__/tactusbox/g')"
      if [ "$new" != "$old" ]; then
        mkdir -p "$(dirname "$new")"
        git mv "$old" "$new"
        printf 'renamed %s -> %s\n' "$old" "$new"
      fi
    done

# 3. Cargo.lock: only the workspace package entry is rewritten.
cargo update --workspace

# 4. Reflow.
cargo fmt

printf '\nremaining matches (expect only tactusbox and this script):\n'
git grep -n -i tactus || true
