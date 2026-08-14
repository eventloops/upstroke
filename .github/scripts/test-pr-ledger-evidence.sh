#!/usr/bin/env bash
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
validator="$script_dir/validate-pr-ledger-evidence.sh"
scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT

git -C "$scratch" init -q
git -C "$scratch" config user.name 'ledger fixture'
git -C "$scratch" config user.email 'ledger-fixture@tactus.local'
mkdir -p "$scratch/src"
printf '%s\n' 'fn implementation() {}' 'fn regression_guard_exists() {}' > "$scratch/src/example.rs"
git -C "$scratch" add src/example.rs
git -C "$scratch" commit -qm 'fixture root'
root_sha="$(git -C "$scratch" rev-parse HEAD)"

git -C "$scratch" switch -q -c unrelated
printf '%s\n' 'unrelated branch' > "$scratch/unrelated.txt"
git -C "$scratch" add unrelated.txt
git -C "$scratch" commit -qm 'unrelated review lane'
unrelated_sha="$(git -C "$scratch" rev-parse HEAD)"

git -C "$scratch" switch -q --detach "$root_sha"
printf '%s\n' 'integrated head' > "$scratch/integrated.txt"
git -C "$scratch" add integrated.txt
git -C "$scratch" commit -qm 'integrated head'
head_sha="$(git -C "$scratch" rev-parse HEAD)"

header='| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |'
separator='|---|---|---|---|---|---|---|---|---|'
row="| PRX-001 | P1 | $root_sha / src/example.rs:2 | bad input -> weak check -> incorrect success | fix_regression | security-trust | pre-commit lane | \`regression_guard_exists\` | fixed |"
body="$header"$'\n'"$separator"$'\n'"$row"

expect_pass() {
  local name="$1"
  local candidate="$2"
  if ! printf '%s\n' "$candidate" | (cd "$scratch" && bash "$validator" "$head_sha"); then
    echo "expected ledger evidence pass: $name" >&2
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local candidate="$2"
  if printf '%s\n' "$candidate" | (cd "$scratch" && bash "$validator" "$head_sha") \
    >/dev/null 2>&1; then
    echo "expected ledger evidence failure: $name" >&2
    exit 1
  fi
}

expect_pass 'integrated reviewed commit and live guard' "$body"
expect_fail 'unresolvable reviewed SHA' "${body/$root_sha/0000000000000000000000000000000000000000}"
expect_fail 'review-lane commit not integrated' "${body/$root_sha/$unrelated_sha}"
expect_fail 'missing location path' "${body/src\/example.rs/src\/missing.rs}"
expect_fail 'location beyond end of file' "${body/src\/example.rs:2/src\/example.rs:99}"
expect_fail 'stale renamed regression' "${body/regression_guard_exists/old_regression_name}"

echo 'PR ledger evidence fixtures passed'
