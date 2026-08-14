#!/usr/bin/env bash
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

if [[ $# -ne 1 || ! "$1" =~ ^[0-9a-f]{40}$ ]]; then
  echo "usage: validate-pr-ledger-evidence.sh <exact-head-sha>" >&2
  exit 2
fi

head_sha="$1"
body="$(tr -d '\r')"
failed=0

error() {
  echo "$*" >&2
  failed=1
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

if ! git cat-file -e "$head_sha^{commit}" 2>/dev/null; then
  echo "exact PR head is not available in the local object database: $head_sha" >&2
  exit 1
fi

# Load tracked textual content once. A large finding ledger otherwise starts a
# fresh `git grep` for every named regression, which is needlessly slow on the
# same Windows hosts whose evidence this policy is meant to preserve.
tracked_text_file="$(mktemp)"
trap 'rm -f -- "$tracked_text_file"' EXIT
git grep -I -h -e '.' "$head_sha" -- > "$tracked_text_file" 2>/dev/null || true
declare -A reviewed_commits=()
declare -A location_lines=()

header='| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |'
mapfile -t lines <<< "$body"
header_index=-1
for index in "${!lines[@]}"; do
  if [[ "${lines[$index]}" == "$header" ]]; then
    header_index=$index
    break
  fi
done
if (( header_index < 0 )); then
  echo "cannot validate ledger evidence without the canonical ledger header" >&2
  exit 1
fi

for ((index = header_index + 2; index < ${#lines[@]}; index++)); do
  line="${lines[$index]}"
  [[ -z "$line" || "$line" == '## '* ]] && break
  [[ "$line" != '|'* ]] && break
  inner="${line#|}"
  inner="${inner%|}"
  IFS='|' read -r -a columns <<< "$inner"
  if (( ${#columns[@]} != 9 )); then
    error "ledger evidence row has ${#columns[@]} columns; run validate-pr-body.sh first"
    continue
  fi
  for column in "${!columns[@]}"; do
    columns[$column]="$(trim "${columns[$column]}")"
  done
  id="${columns[0]}"
  [[ "$id" == "None yet" ]] && continue

  location="${columns[2]}"
  if [[ ! "$location" =~ ^([0-9a-f]{40})[[:space:]]+/[[:space:]]+([A-Za-z0-9._/-]+):([1-9][0-9]*)$ ]]; then
    error "$id has a location that cannot be resolved safely: $location"
    continue
  fi
  reviewed_sha="${BASH_REMATCH[1]}"
  path="${BASH_REMATCH[2]}"
  line_number="${BASH_REMATCH[3]}"
  if [[ "$path" == /* || "$path" == ./* || "$path" == '..' || "$path" == ../* ||
        "$path" == */../* || "$path" == */.. || "$path" == *'//'* ]]; then
    error "$id must use a normalized repository-relative path: $path"
    continue
  fi
  if [[ -z "${reviewed_commits[$reviewed_sha]:-}" ]]; then
    if ! git cat-file -e "$reviewed_sha^{commit}" 2>/dev/null; then
      reviewed_commits[$reviewed_sha]=missing
    elif ! git merge-base --is-ancestor "$reviewed_sha" "$head_sha" 2>/dev/null; then
      reviewed_commits[$reviewed_sha]=unintegrated
    else
      reviewed_commits[$reviewed_sha]=integrated
    fi
  fi
  case "${reviewed_commits[$reviewed_sha]}" in
    missing)
      error "$id reviewed SHA is not available: $reviewed_sha"
      continue
      ;;
    unintegrated)
      error "$id reviewed SHA is not an ancestor of exact head $head_sha: $reviewed_sha"
      continue
      ;;
    integrated) ;;
  esac
  location_key="$reviewed_sha:$path"
  if [[ -z "${location_lines[$location_key]:-}" ]]; then
    if ! git cat-file -e "$location_key" 2>/dev/null; then
      location_lines[$location_key]=missing
    else
      location_lines[$location_key]="$(git show "$location_key" | awk 'END { print NR + 0 }')"
    fi
  fi
  if [[ "${location_lines[$location_key]}" == missing ]]; then
    error "$id location does not exist at its reviewed SHA: $reviewed_sha / $path"
    continue
  fi
  line_count="${location_lines[$location_key]}"
  if (( line_number > line_count )); then
    error "$id location line $line_number exceeds $path at $reviewed_sha ($line_count lines)"
  fi

  # Backticks identify concrete tracked tests/scripts/guards. Prose is allowed
  # for an explicitly documented invariant, but a named identifier must resolve
  # at the exact head so a renamed or never-integrated regression cannot be
  # presented as durable prevention evidence.
  prevention="${columns[7]}"
  rest="$prevention"
  while [[ "$rest" == *'`'* ]]; do
    rest="${rest#*\`}"
    if [[ "$rest" != *'`'* ]]; then
      error "$id has an unclosed backtick in its prevention evidence"
      break
    fi
    identifier="${rest%%\`*}"
    rest="${rest#*\`}"
    if [[ -z "$identifier" ]]; then
      error "$id has an empty prevention identifier"
    elif ! grep -F -q -- "$identifier" "$tracked_text_file"; then
      error "$id prevention identifier is not tracked at exact head: $identifier"
    fi
  done
done

exit "$failed"
