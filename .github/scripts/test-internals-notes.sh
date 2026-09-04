#!/usr/bin/env bash
# Gate: the internal module notes and the source agree.
#
#   N1  Every `Extended notes:` marker in src/ names a docs/internals/ file
#       that exists, and an anchor that some heading in that file generates.
#   N2  Every docs/internals/**/*.md (except README.md) mirrors a Rust module
#       that exists: docs/internals/X.md <-> src/X.rs.
#   N3  Every notes file links back to its module, and the link resolves.
#   N4  The marker spelling is uniform, so `grep -rn 'Extended notes:' src/` is
#       the whole inventory. A marker names its path in backticks and nothing
#       else on the line.
#   N5  A module with a notes file carries exactly one marker, in its module
#       header. The notes carry the module's prose; the source carries none, so
#       one pointer at the top is the whole of the cross-reference and a second
#       marker further down means prose crept back in beside it.
#
# Drift this catches: a module renamed or split out from under its notes, a
# notes file deleted while markers still point at it, an anchor that stopped
# existing when a heading was reworded.
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

cd "${BASH_SOURCE[0]%/*}/../.."

failed=0
error() {
  echo "$*" >&2
  failed=1
}

# Every anchor a notes file's headings generate, one line each, under GitHub's
# rules for the subset we need: strip the leading hashes, lower-case, drop
# anything that is not alphanumeric/space/underscore/hyphen, spaces to hyphens.
#
# Computed once per file and cached. Doing it per heading per marker is ~1700
# processes on this tree and minutes of wall clock on Windows.
declare -A anchor_cache=()
anchors_of() {
  local path="$1"
  if [[ -z "${anchor_cache[$path]+set}" ]]; then
    anchor_cache[$path]="$(
      grep '^#\{1,6\} ' "$path" \
        | sed 's/^#\{1,6\} *//' \
        | tr '[:upper:]' '[:lower:]' \
        | sed -e 's/[^a-z0-9 _-]//g' -e 's/ /-/g' || true
    )"
  fi
  printf '%s' "${anchor_cache[$path]}"
}

notes_root='docs/internals'

if [[ ! -d "$notes_root" ]]; then
  echo "no $notes_root; nothing to check"
  exit 0
fi

# --- N1 + N4. every marker resolves -----------------------------------------

marker_count=0
while IFS= read -r hit; do
  file="${hit%%:*}"
  rest="${hit#*:}"
  line="${rest%%:*}"
  text="${rest#*:}"

  target="$(sed -n 's/.*`\(docs\/internals\/[^`]*\)`.*/\1/p' <<< "$text")"
  if [[ -z "$target" ]]; then
    error "$file:$line: marker does not name a \`docs/internals/...\` path in backticks"
    continue
  fi
  marker_count=$((marker_count + 1))

  path="${target%%#*}"
  anchor=""
  [[ "$target" == *'#'* ]] && anchor="${target#*#}"

  if [[ ! -f "$path" ]]; then
    error "$file:$line: marker names $path, which does not exist"
    continue
  fi

  if [[ -n "$anchor" ]]; then
    if ! grep -Fqx -- "$anchor" <<< "$(anchors_of "$path")"; then
      error "$file:$line: $path has no heading generating anchor #$anchor"
    fi
  fi
done < <(grep -rn 'Extended notes:' src/ --include='*.rs' || true)

(( marker_count > 0 )) || error "no \`Extended notes:\` markers found in src/; this gate is inert"

# --- N5. one marker per module, in the module header ------------------------

while IFS= read -r module; do
  count="$(grep -c 'Extended notes:' "$module" || true)"
  if (( count != 1 )); then
    error "$module carries $count \`Extended notes:\` markers; a module with notes carries exactly one"
    continue
  fi
  # `awk` reading the file directly, never `grep ... | head`: `head` closing
  # the pipe early gives `grep` a SIGPIPE, and under `set -o pipefail` that is
  # a failed pipeline. It depends on buffering, so it passed locally and on
  # the build box and failed on CI.
  at="$(awk '/Extended notes:/ { print NR; exit }' "$module")"
  first_code="$(awk '!/^[[:space:]]*\/\// && NF { print NR; exit }' "$module")"
  if (( at > first_code )); then
    error "$module has its marker at line $at, below the first code at line $first_code; it belongs in the module header"
  fi
done < <(grep -rl 'Extended notes:' src/ --include='*.rs' | sort)

# --- N2 + N3. every notes file mirrors a live module -------------------------

notes_count=0
while IFS= read -r notes; do
  [[ "$notes" == "$notes_root/README.md" ]] && continue
  notes_count=$((notes_count + 1))

  module="src/${notes#"$notes_root/"}"
  module="${module%.md}.rs"
  if [[ ! -f "$module" ]]; then
    error "$notes describes $module, which does not exist"
    continue
  fi

  # N3: the file links back, and the relative link resolves from its own
  # directory.
  link="$(awk 'match($0, /\(\.\.[^)]*\.rs\)/) { print substr($0, RSTART + 1, RLENGTH - 2); exit }' "$notes")"
  if [[ -z "$link" ]]; then
    error "$notes does not link back to its module"
    continue
  fi
  resolved="$(cd "${notes%/*}" && realpath -m --relative-to="$(cd ../../.. && pwd)" "$link" 2>/dev/null || true)"
  [[ "$resolved" == "$module" ]] \
    || error "$notes links to $link, which resolves to '${resolved:-nothing}' rather than $module"
done < <(find "$notes_root" -name '*.md' | sort)

(( notes_count > 0 )) || error "no notes files under $notes_root; this gate is inert"

if (( failed == 0 )); then
  echo "internals notes: $marker_count marker(s), $notes_count notes file(s), all resolve"
fi

exit "$failed"
