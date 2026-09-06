#!/usr/bin/env bash
# Gate: the internal module notes and the source agree, in both directions.
#
#   N1  Every `Extended notes:` marker in src/ is spelled exactly
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
# An absent docs/internals/ is a failure, never "nothing to check": with
# markers in src/ it is a deleted notes tree, and with none it is a gate
# measuring nothing. Both refuse.
set -euo pipefail
export PATH="/usr/bin:/bin:$PATH"

cd "${1:?usage: validate-internals-notes.sh REPOSITORY}"
root="$(pwd)"

failed=0
error() {
  echo "$*" >&2
  failed=1
}

notes_root='docs/internals'
marker_re='^//! Extended notes: `(docs/internals/[A-Za-z0-9_./-]+\.md)`$'

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
done < <(grep -rn 'Extended notes:' src/ --include='*.rs' || true)

(( marker_count > 0 )) || error "no \`Extended notes:\` markers found in src/; this gate is inert"

# --- N4. at most one marker per module, in the module header -----------------

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
  back="$(grep -c 'Extended notes:' "$module" || true)"
  (( back == 1 )) \
    || error "$notes exists but $module carries $back \`Extended notes:\` marker(s); a module with notes carries exactly one"

  # N3 accepts the documented opening paragraph, not an arbitrary .rs path.
  # The opening link may follow a descriptive H1 and blank lines, and ordinary
  # prose may continue after it. Refuse other preceding blocks rather than
  # treating an HTML comment or code example as navigation. This recognizes
  # the existing notes opening, not arbitrary Markdown throughout the file.
  #
  # The opening link is a Markdown link in any of the four forms a reader can
  # actually follow: inline with a bare or `<>`-delimited destination and an
  # optional title, or a full, collapsed or shortcut reference resolved
  # against the document's own link reference definitions. A reference
  # definition is invisible, so one cannot open the file itself, and only
  # definitions at block level count: inside a fence, an HTML comment, an
  # indented code block, a paragraph or a container they are text, not
  # navigation. Everything this refuses is a refusal, never an acceptance:
  # a construct spelled in a way the parser does not read is reported as a
  # missing backlink rather than passed through unchecked.
  link="$(awk '
    function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t]+$/, "", s); return s }
    # Reference labels match case-insensitively, on collapsed whitespace.
    function norm(s) { s = trim(s); gsub(/[ \t]+/, " ", s); return tolower(s) }
    # The length of the run of ch at the start of s. Interval expressions are
    # avoided throughout: this gate runs under whichever awk the runner has.
    function run_of(s, ch,   n) { n = 0; while (substr(s, n + 1, 1) == ch) n++; return n }

    # The destination of an inline link, given the text just after its "(".
    # Empty unless the parentheses hold a destination and an optional title.
    function inline_destination(rest,   dest, head) {
      sub(/^[ \t]+/, "", rest)
      head = substr(rest, 1, 1)
      if (head == "<") {
        if (!match(rest, /^<[^<>]*>/)) return ""
        dest = substr(rest, 2, RLENGTH - 2)
      } else {
        match(rest, /^[^ \t()]*/)
        dest = substr(rest, 1, RLENGTH)
      }
      if (dest == "") return ""
      rest = substr(rest, RLENGTH + 1)
      sub(/^[ \t]+/, "", rest)
      head = substr(rest, 1, 1)
      if (head == "\"") { if (!match(rest, /^"[^"]*"/)) return ""; rest = substr(rest, RLENGTH + 1) }
      else if (head == "'"'"'") { if (!match(rest, /^'"'"'[^'"'"']*'"'"'/)) return ""; rest = substr(rest, RLENGTH + 1) }
      else if (head == "(") { if (!match(rest, /^\([^()]*\)/)) return ""; rest = substr(rest, RLENGTH + 1) }
      sub(/^[ \t]+/, "", rest)
      if (substr(rest, 1, 1) != ")") return ""
      return dest
    }

    {
      sub(/\r$/, "")
      line = $0
      bare = line
      sub(/^ +/, "", bare)
      indent = length(line) - length(bare)
    }

    !opened {
      if (line ~ /^[[:space:]]*$/) next
      if (!heading && line ~ /^# [^<>]+$/) { heading = 1; next }
      # Navigation is an opening paragraph, not a list, quote, heading, or
      # code block. Container prefixes can otherwise hide code indentation.
      if (line ~ /^(    | *\t| *[>#]| *[-+*][ \t]| *[0-9]+[.)][ \t]| *~~~)/) exit
      # A link reference definition renders nothing, so it cannot open a file.
      # An indented one has already left through the container rule above.
      if (bare ~ /^\[[^][]+\][ \t]*:/) exit

      at = index(line, "[")
      if (at == 0) exit
      # Only ordinary prose may precede the link. A code span, HTML block,
      # image, escape, or code indentation cannot supply it. An enclosing
      # link is excluded by construction: this is the line'"'"'s first bracket.
      if (substr(line, 1, at - 1) ~ /[`<\\!]/) exit
      rest = substr(line, at + 1)
      shut = index(rest, "]")
      if (shut == 0) exit
      label = substr(rest, 1, shut - 1)
      if (label ~ /[<&\\]/ || label !~ /[[:alnum:]_]/) exit
      # A single code-span label must close inside the brackets. Otherwise
      # Markdown can consume the apparent closing bracket as code.
      if (label ~ /`/ && label !~ /^`[^`]+`$/) exit
      rest = substr(rest, shut + 1)
      head = substr(rest, 1, 1)
      if (head == "(") { result = inline_destination(substr(rest, 2)); exit }
      if (head == "[") {
        shut = index(substr(rest, 2), "]")
        if (shut == 0) exit
        want = substr(rest, 2, shut - 1)
        # A collapsed [label][] reuses the label, already checked above; an
        # explicit reference label is matched literally, so hold it to the
        # same plain spelling.
        if (want == "") want = label
        else if (want ~ /[<&\\]/) exit
      } else {
        want = label
      }
      want = norm(want)
      if (want == "") exit
      opened = 1
      in_paragraph = 1
      next
    }

    # Past the opening paragraph, collect the block-level link reference
    # definitions a reference form resolves against.
    fence != "" {
      if (indent < 4 && substr(bare, 1, 1) == fence) {
        len = run_of(bare, fence)
        if (len >= fence_len && substr(bare, len + 1) ~ /^[ \t]*$/) fence = ""
      }
      in_paragraph = 0
      next
    }
    comment { if (index(line, "-->") > 0) comment = 0; in_paragraph = 0; next }
    line ~ /^[[:space:]]*$/ { in_paragraph = 0; next }
    {
      head = substr(bare, 1, 1)
      if (indent < 4 && (head == "`" || head == "~") && run_of(bare, head) >= 3) {
        fence = head
        fence_len = run_of(bare, head)
        in_paragraph = 0
        next
      }
      at = index(line, "<!--")
      if (at > 0) {
        if (index(substr(line, at + 4), "-->") == 0) comment = 1
        in_paragraph = 0
        next
      }
      if (indent >= 4 && !in_paragraph) next
      if (head == "#") { in_paragraph = 0; next }
      if (!in_paragraph && match(bare, /^\[[^][]+\][ \t]*:/)) {
        def = substr(bare, 1, RLENGTH)
        rest = substr(bare, RLENGTH + 1)
        ref = norm(substr(def, 2, index(def, "]") - 2))
        sub(/^[ \t]+/, "", rest)
        head = substr(rest, 1, 1)
        if (head == "<") {
          if (!match(rest, /^<[^<>]*>/)) { in_paragraph = 1; next }
          dest = substr(rest, 2, RLENGTH - 2)
        } else {
          match(rest, /^[^ \t]*/)
          dest = substr(rest, 1, RLENGTH)
        }
        # A destination continued on the next line is not read as one.
        if (dest == "") { in_paragraph = 1; next }
        rest = trim(substr(rest, RLENGTH + 1))
        if (rest != "" && rest !~ /^("[^"]*"|'"'"'[^'"'"']*'"'"'|\([^()]*\))$/) { in_paragraph = 1; next }
        if (!(ref in defs)) defs[ref] = dest
        next
      }
      in_paragraph = 1
    }

    END {
      if (result == "" && want != "" && (want in defs)) result = defs[want]
      # Whether the destination is this module is a question for resolution
      # below, not for the parser: it is printed exactly as it was written.
      print result
    }
  ' "$notes")"
  if [[ -z "$link" ]]; then
    error "$notes must open with a visible Markdown backlink to its module, optionally after an H1"
    continue
  fi
  # `--`: a destination is arbitrary text, and realpath would otherwise read
  # a leading dash as its own option.
  resolved="$(cd "${notes%/*}" && realpath -m --relative-to="$root" -- "$link" 2>/dev/null || true)"
  [[ "$resolved" == "$module" ]] \
    || error "$notes links to $link, which resolves to '${resolved:-nothing}' rather than $module"
done < <(find "$notes_root" -name '*.md' 2>/dev/null | sort)

(( notes_count > 0 )) || error "no notes files under $notes_root; this gate is inert"

if (( failed == 0 )); then
  echo "internals notes: $marker_count marker(s), $notes_count notes file(s), all resolve both ways"
fi

exit "$failed"
