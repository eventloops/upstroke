#!/usr/bin/env bash
# Gate: the internal module notes and the source agree, in both directions.
#
#   N1  Every `Extended notes:` mention in a Rust comment under src/ is spelled
#       exactly
#         //! Extended notes: `docs/internals/<module>.md`
#       where <module> is the marker's own file with `src/` and `.rs` stripped,
#       and that notes file exists. Nothing else on the line: no anchor, no
#       prose, no other comment form.
#   N2  Every docs/internals/**/*.md (except README.md) mirrors a Rust module
#       that exists, and that module carries exactly one marker. A notes file
#       whose module lost its marker is caught from this side.
#   N3  Every notes file links back to its module, and the link resolves from
#       the notes file's own directory to the repository root, at any depth.
#   N4  A module carries at most one marker, and it sits above the first code.
#
# The domain is Rust comments, not raw source text (§12). The scan below blanks
# string, byte-string, raw-string and character literals, and every other code
# byte, before it looks for the phrase: an ordinary Rust string that happens to
# contain `Extended notes:` is source text, not a marker, and neither N1 nor N4
# sees it. Comment bytes keep their own columns, so the spelling check still
# anchors on the real line, and "the first code" is the first line carrying
# anything outside a comment.
#
# An absent docs/internals/ is a failure, never "nothing to check": with
# markers in src/ it is a deleted notes tree, and with none it is a gate
# measuring nothing. Both refuse.
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"
# The scan is byte-oriented: a character literal's width is read from its UTF-8
# lead byte, which needs `length` and `substr` counting bytes rather than
# characters.
export LC_ALL=C

cd "${1:?usage: validate-internals-notes.sh REPOSITORY}"
root="$(pwd)"

failed=0
error() {
  echo "$*" >&2
  failed=1
}

notes_root='docs/internals'
marker_re='^//! Extended notes: `(docs/internals/[A-Za-z0-9_./-]+\.md)`$'

# --- the comment view of src/ -------------------------------------------------
#
# One pass over every Rust source, mirroring `blank_comments_and_strings` in
# `src/effects.rs`: line and nesting block comments are kept where they stand,
# and literals and code become spaces. It writes two records:
#
#   markers  file:line:comment-view-of-the-line   one per line whose comment
#                                                 text mentions the phrase
#   summary  file<TAB>markers<TAB>first-marker-line<TAB>first-code-line
#                                                 one per file that has lines
#
comment_view='
function repeat(ch, n,   s) { s = ""; while (n-- > 0) { s = s ch } return s }

# A char literal is one escape or one UTF-8 character, then the closing quote.
# A lifetime never closes, so `&\47a str` stays code.
function char_literal(l, i,   c, w, k) {
  c = substr(l, i + 1, 1)
  if (c == "") { return 0 }
  if (c == "\\") {
    k = i + 3
    while (k <= i + 12 && substr(l, k, 1) != Q && substr(l, k, 1) != "") { k += 1 }
    return substr(l, k, 1) == Q ? k - i + 1 : 0
  }
  w = 1
  if (c >= "\302" && c <= "\337") { w = 2 }
  else if (c >= "\340" && c <= "\357") { w = 3 }
  else if (c >= "\360" && c <= "\364") { w = 4 }
  return substr(l, i + 1 + w, 1) == Q ? w + 2 : 0
}

function blanked(b) { if (b != " " && b != "\t") { code = 1 } }

function view(l,   n, i, out, two, c, j, raw, h, len) {
  n = length(l)
  out = ""
  i = 1
  while (i <= n) {
    two = substr(l, i, 2)
    if (depth > 0) {
      if (two == "/*") { depth += 1; out = out two; i += 2 }
      else if (two == "*/") { depth -= 1; out = out two; i += 2 }
      else { out = out substr(l, i, 1); i += 1 }
      continue
    }
    if (hashes >= 0) {
      if (substr(l, i, 1) == "\"" && substr(l, i + 1, hashes) == raw_close) {
        out = out repeat(" ", hashes + 1)
        code = 1
        i += hashes + 1
        hashes = -1
      } else {
        blanked(substr(l, i, 1))
        out = out " "
        i += 1
      }
      continue
    }
    if (instring) {
      c = substr(l, i, 1)
      len = (c == "\\") ? 2 : 1
      if (len > n - i + 1) { len = n - i + 1 }
      if (c == "\"") { instring = 0 }
      out = out repeat(" ", len)
      code = 1
      i += len
      continue
    }
    if (two == "//") { out = out substr(l, i); i = n + 1; continue }
    if (two == "/*") { depth = 1; out = out two; i += 2; continue }
    c = substr(l, i, 1)
    if ((c == "b" || c == "r") && (i == 1 || substr(l, i - 1, 1) !~ /[0-9A-Za-z_]/)) {
      j = i
      if (substr(l, j, 1) == "b") { j += 1 }
      raw = (substr(l, j, 1) == "r")
      if (raw) { j += 1 }
      h = 0
      while (substr(l, j, 1) == "#") { h += 1; j += 1 }
      if (substr(l, j, 1) == "\"" && (raw || h == 0)) {
        out = out repeat(" ", j - i + 1)
        code = 1
        i = j + 1
        if (raw) { hashes = h; raw_close = repeat("#", h) } else { instring = 1 }
        continue
      }
    }
    if (c == "\"") { instring = 1; out = out " "; code = 1; i += 1; continue }
    if (c == Q) {
      len = char_literal(l, i)
      if (len > 0) { out = out repeat(" ", len); code = 1; i += len; continue }
    }
    blanked(c)
    out = out " "
    i += 1
  }
  return out
}

function emit() { if (seen != "") { print seen "\t" count "\t" at "\t" first_code >> summary } }

BEGIN { Q = "\47"; phrase = "Extended notes:"; hashes = -1 }

FNR == 1 {
  emit()
  seen = FILENAME
  depth = 0
  hashes = -1
  instring = 0
  count = 0
  at = 0
  first_code = 0
}

{
  code = 0
  text = view($0)
  if (code && first_code == 0) { first_code = FNR }
  if (index(text, phrase) > 0) {
    count += 1
    if (at == 0) { at = FNR }
    print FILENAME ":" FNR ":" text >> markers
  }
}

END { emit() }
'

work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
: > "$work/markers"
: > "$work/summary"

sources=()
while IFS= read -r source; do
  sources+=("$source")
done < <(find src -name '*.rs' -type f 2>/dev/null | sort)

if (( ${#sources[@]} > 0 )); then
  awk -v markers="$work/markers" -v summary="$work/summary" "$comment_view" "${sources[@]}"
fi

# --- N1. every marker is exact, names its own module's notes, and they exist --

marker_count=0
while IFS= read -r hit; do
  file="${hit%%:*}"
  rest="${hit#*:}"
  line="${rest%%:*}"
  text="${rest#*:}"

  if [[ ! "$text" =~ $marker_re ]]; then
    error "$file:$line: marker is not spelled exactly \`//! Extended notes: \`docs/internals/<module>.md\`\`: $text"
    continue
  fi
  target="${BASH_REMATCH[1]}"
  marker_count=$((marker_count + 1))

  expected="$notes_root/${file#src/}"
  expected="${expected%.rs}.md"
  [[ "$target" == "$expected" ]] \
    || error "$file:$line: marker names $target; this module's notes are $expected"
  [[ -f "$target" ]] \
    || error "$file:$line: marker names $target, which does not exist"
done < "$work/markers"

(( marker_count > 0 )) || error "no \`Extended notes:\` markers found in src/; this gate is inert"

# --- N4. at most one marker per module, in the module header -----------------

while IFS=$'\t' read -r module count at first_code; do
  (( count > 0 )) || continue
  if (( count != 1 )); then
    error "$module carries $count \`Extended notes:\` markers; a module with notes carries exactly one"
    continue
  fi
  # A module that is all comments has no first code for the marker to sit above.
  if (( first_code > 0 && at > first_code )); then
    error "$module has its marker at line $at, below the first code at line $first_code; it belongs in the module header"
  fi
done < "$work/summary"

# --- N2 + N3. every notes file mirrors a live, marked module -----------------

[[ -d "$notes_root" ]] || error "$notes_root is absent; with markers in src/ that is a deleted notes tree"

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

  # N2: the module points back at this file, exactly once.
  back="$(awk -F'\t' -v module="$module" '$1 == module { print $2; found = 1 } END { if (!found) { print 0 } }' "$work/summary")"
  (( back == 1 )) \
    || error "$notes exists but $module carries $back \`Extended notes:\` marker(s); a module with notes carries exactly one"

  # N3 accepts the documented opening paragraph, not an arbitrary .rs path.
  # The opening link may follow a descriptive H1 and blank lines, and ordinary
  # prose may continue after it. Refuse other preceding blocks rather than
  # treating an HTML comment or code example as navigation. This recognizes
  # the existing notes opening, not arbitrary Markdown throughout the file.
  link="$(awk '
    { sub(/\r$/, "") }
    /^[[:space:]]*$/ { next }
    {
      if (!heading && $0 ~ /^# [^<>]+$/) { heading = 1; next }
      # Navigation is an opening paragraph, not a list, quote, heading, or
      # code block. Container prefixes can otherwise hide code indentation.
      if ($0 ~ /^(    | *\t| *[>#]| *[-+*][ \t]| *[0-9]+[.)][ \t]| *~~~)/) exit
      if (match($0, /\[[^][]+\]\(\.\.\/[A-Za-z0-9_.\/-]+\.rs\)/)) {
        before = substr($0, 1, RSTART - 1)
        # Only ordinary prose may precede the link. A code span, HTML block,
        # image, escape, enclosing link, or code indentation cannot supply it.
        if (before ~ /[`<\\!\[]/) exit
        link = substr($0, RSTART, RLENGTH)
        label = substr(link, 2, index(link, "](") - 2)
        if (label ~ /[<&\\]/ || label !~ /[[:alnum:]_]/) exit
        # A single code-span label must close inside the brackets. Otherwise
        # Markdown can consume the apparent closing bracket as code.
        if (label ~ /`/ && label !~ /^`[^`]+`$/) exit
        sub(/^\[[^][]+\]\(/, "", link)
        sub(/\)$/, "", link)
        print link
        exit
      }
      exit
    }
  ' "$notes")"
  if [[ -z "$link" ]]; then
    error "$notes must open with a visible Markdown backlink to its module, optionally after an H1"
    continue
  fi
  resolved="$(cd "${notes%/*}" && realpath -m --relative-to="$root" "$link" 2>/dev/null || true)"
  [[ "$resolved" == "$module" ]] \
    || error "$notes links to $link, which resolves to '${resolved:-nothing}' rather than $module"
done < <(find "$notes_root" -name '*.md' 2>/dev/null | sort)

(( notes_count > 0 )) || error "no notes files under $notes_root; this gate is inert"

if (( failed == 0 )); then
  echo "internals notes: $marker_count marker(s), $notes_count notes file(s), all resolve both ways"
fi

exit "$failed"
