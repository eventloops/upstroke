#!/usr/bin/env bash
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

title="${1:-}"
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

title_pattern='^(feat|fix|docs|refactor|test|chore|ci|build|perf|security|release|revert)(\([a-z0-9][a-z0-9._/-]*\))?!?: .+'
if [[ ! "$title" =~ $title_pattern ]]; then
  error "Pull-request title must use: type(optional-scope): summary"
  error "Allowed types: feat, fix, docs, refactor, test, chore, ci, build, perf, security, release, revert"
fi

required_sections=(
  "## Summary"
  "## Scope"
  "## Validation"
  "## Review evidence"
  "## Risk and rollback"
  "## Review finding ledger"
)
for section in "${required_sections[@]}"; do
  if ! grep -Fqx "$section" <<< "$body"; then
    error "Pull-request body is missing: $section"
  fi
done

expected_header='| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |'
expected_separator='|---|---|---|---|---|---|---|---|---|'
mapfile -t lines <<< "$body"
ledger_index=-1
header_index=-1
for index in "${!lines[@]}"; do
  if [[ "${lines[$index]}" == '## Review finding ledger' ]]; then
    if (( ledger_index >= 0 )); then
      error "Pull-request body contains more than one Review finding ledger section"
    fi
    ledger_index=$index
  fi
done

if (( ledger_index >= 0 )); then
  for ((index = ledger_index + 1; index < ${#lines[@]}; index++)); do
    [[ "${lines[$index]}" == '## '* ]] && break
    if [[ "${lines[$index]}" == "$expected_header" ]]; then
      if (( header_index >= 0 )); then
        error "Review finding ledger contains more than one canonical header"
      fi
      header_index=$index
    fi
  done
fi

if (( header_index < 0 )); then
  error "Review finding ledger must use the exact canonical header: $expected_header"
else
  separator_index=$((header_index + 1))
  if (( separator_index >= ${#lines[@]} )) || [[ "${lines[$separator_index]}" != "$expected_separator" ]]; then
    error "Review finding ledger must use the exact canonical separator: $expected_separator"
  fi

  declare -A ids=()
  row_count=0
  none_count=0
  for ((index = header_index + 2; index < ${#lines[@]}; index++)); do
    line="${lines[$index]}"
    [[ -z "$line" || "$line" == '## '* ]] && break
    [[ "$line" != '|'* ]] && break
    inner="${line#|}"
    inner="${inner%|}"
    IFS='|' read -r -a columns <<< "$inner"
    if (( ${#columns[@]} != 9 )); then
      error "Ledger row $((row_count + 1)) must contain exactly 9 columns"
      row_count=$((row_count + 1))
      continue
    fi
    for column in "${!columns[@]}"; do
      columns[$column]="$(trim "${columns[$column]}")"
    done
    row_count=$((row_count + 1))

    if [[ "${columns[0]}" == "None yet" ]]; then
      none_count=$((none_count + 1))
      for column in {1..8}; do
        [[ "${columns[$column]}" == "—" ]] || error "The None yet row must contain only em-dash placeholders"
      done
      continue
    fi

    id="${columns[0]}"
    severity="${columns[1]}"
    location="${columns[2]}"
    sequence="${columns[3]}"
    provenance="${columns[4]}"
    category="${columns[5]}"
    prevention="${columns[7]}"
    disposition="${columns[8]}"

    if [[ "$id" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
      if [[ -n "${ids[$id]:-}" ]]; then
        error "Ledger finding ID is duplicated: $id"
      fi
      ids[$id]=1
    else
      error "Ledger row $row_count has an invalid finding ID: $id"
    fi
    [[ "$severity" =~ ^P[0-3]$ ]] || error "$id must use severity P0, P1, P2, or P3"
    [[ "$location" =~ ^[0-9a-f]{40}[[:space:]]+/[[:space:]]+[^[:space:]|]+:[1-9][0-9]*$ ]] || \
      error "$id must bind a full reviewed SHA and file:line with 'SHA / path:line'"
    if [[ ${#sequence} -lt 12 || "$sequence" != *'->'* ]]; then
      error "$id must record a concrete failure sequence containing '->'"
    fi
    case "$provenance" in
      pre_existing|introduced_by_feature|fix_regression|undetermined) ;;
      *) error "$id has unsupported provenance: $provenance" ;;
    esac
    case "$category" in
      correctness|crash-consistency|security-trust|portability|liveness|performance|compatibility|docs-contract) ;;
      *) error "$id has unsupported category: $category" ;;
    esac
    if [[ -z "${columns[6]}" ]]; then
      error "$id must record a first-bad/prior ID or an em dash when history is indeterminate"
    fi
    if [[ -z "$prevention" || "$prevention" == "—" ]]; then
      error "$id must name a regression test or a documented deterministic guard"
    fi
    case "$disposition" in
      fixed|rejected|deferred|accepted-risk) ;;
      *) error "$id has unsupported disposition: $disposition" ;;
    esac
  done

  (( row_count > 0 )) || error "Review finding ledger must contain a finding row or the canonical None yet row"
  if (( none_count > 0 && row_count != 1 )); then
    error "The None yet row cannot be mixed with finding rows"
  fi
fi

exit "$failed"
