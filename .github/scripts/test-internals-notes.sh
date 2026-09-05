#!/usr/bin/env bash
# Check the source/notes contract, then prove N1-N4 on isolated fixtures.
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"
script_dir="$(cd "${BASH_SOURCE[0]%/*}" && pwd)"
root="$(cd "$script_dir/../.." && pwd)"
bash "$script_dir/validate-internals-notes.sh" "$root"
bash "$script_dir/internals-notes-fixtures.sh" "$script_dir/validate-internals-notes.sh"
