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
  # The opening link is a Markdown link in any of the forms a reader can
  # actually follow: inline with a bare or `<>`-delimited destination and an
  # optional title, or a full, collapsed or shortcut reference resolved
  # against the document's own link reference definitions.
  #
  # A reference is only as good as the definition a renderer would pick, which
  # is the FIRST claim on the label. So a label is claimed by every definition
  # shaped line the renderer could read -- inside a blockquote or a list, or
  # with its destination wrapped onto the next line -- and a claim this parser
  # cannot read refuses the label rather than letting a later definition stand
  # in for it. Text that is not a definition at all claims nothing: a fenced or
  # indented code line, an HTML comment or raw HTML block, and a paragraph
  # continuation. Everything this refuses is a refusal, never an acceptance: a
  # construct spelled in a way the parser does not read is reported as a
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
    function inline_destination(rest,   dest, head, gap) {
      sub(/^[ \t]+/, "", rest)
      head = substr(rest, 1, 1)
      if (head == "<") {
        if (!match(rest, /^<[^<>]*>/)) return ""
        dest = substr(rest, 2, RLENGTH - 2)
      } else {
        match(rest, /^[^ \t()]*/)
        dest = substr(rest, 1, RLENGTH)
      }
      rest = substr(rest, RLENGTH + 1)
      # A "(" straight after a bare destination belongs to it: Markdown allows
      # balanced parentheses there. With no whitespace before it there is no
      # title either, so the whitespace rule below refuses the line.
      gap = (rest ~ /^[ \t]/)
      sub(/^[ \t]+/, "", rest)
      if (substr(rest, 1, 1) == ")") return dest
      # A title is separated from the destination by whitespace. Without it
      # Markdown reads no link at all.
      if (!gap) return ""
      head = substr(rest, 1, 1)
      if (head == "\"") { if (!match(rest, /^"[^"]*"/)) return "" }
      else if (head == "'"'"'") { if (!match(rest, /^'"'"'[^'"'"']*'"'"'/)) return "" }
      else if (head == "(") { if (!match(rest, /^\([^()]*\)/)) return "" }
      else return ""
      rest = substr(rest, RLENGTH + 1)
      sub(/^[ \t]+/, "", rest)
      if (substr(rest, 1, 1) != ")") return ""
      return dest
    }

    # The destination of a link reference definition line, or empty when the
    # line is not one this parser reads.
    function definition_destination(text,   rest, head, dest) {
      rest = text
      sub(/^\[[^][]+\][ \t]*:/, "", rest)
      sub(/^[ \t]+/, "", rest)
      head = substr(rest, 1, 1)
      if (head == "<") {
        if (!match(rest, /^<[^<>]*>/)) return ""
        dest = substr(rest, 2, RLENGTH - 2)
      } else {
        match(rest, /^[^ \t]*/)
        dest = substr(rest, 1, RLENGTH)
      }
      # An empty destination means the real one is on the next line, which
      # this parser does not read: it returns empty and the label is refused.
      rest = substr(rest, RLENGTH + 1)
      if (rest == "") return dest
      if (rest !~ /^[ \t]/) return ""
      rest = trim(rest)
      if (rest == "") return dest
      if (rest ~ /^"[^"]*"$/ || rest ~ /^'"'"'[^'"'"']*'"'"'$/ || rest ~ /^\([^()]*\)$/) return dest
      return ""
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
      # link is excluded by construction: this is the first bracket.
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

    # Past the opening paragraph, collect the link reference definitions a
    # reference form resolves against, and the claims it must refuse.
    fence != "" {
      if (indent < 4 && substr(bare, 1, 1) == fence && run_of(bare, fence) >= fence_len \
          && substr(bare, run_of(bare, fence) + 1) ~ /^[ \t]*$/) fence = ""
      in_paragraph = 0
      next
    }
    comment { if (index(line, "-->") > 0) comment = 0; in_paragraph = 0; next }
    # Raw HTML holds no definition, and this parser does not model where each
    # kind of block ends, so the first one closes the question for the file.
    html { next }
    line ~ /^[[:space:]]*$/ { in_paragraph = 0; next }
    {
      head = substr(bare, 1, 1)
      if (indent < 4 && (head == "`" || head == "~") && run_of(bare, head) >= 3) {
        fence = head
        fence_len = run_of(bare, head)
        in_paragraph = 0
        next
      }
      # An HTML comment is a block only where it begins the line; elsewhere it
      # is inline and leaves the paragraph it sits in open.
      if (indent < 4 && substr(bare, 1, 4) == "<!--") {
        if (index(substr(bare, 5), "-->") == 0) comment = 1
        in_paragraph = 0
        next
      }
      if (indent < 4 && head == "<") { html = 1; next }
      if (indent >= 4 && !in_paragraph) next
      # An ATX heading is one to six hashes and then a space or end of line.
      # "#not a heading" is paragraph text and ends nothing.
      hashes = run_of(bare, "#")
      if (hashes > 0 && hashes < 7 && substr(bare, hashes + 1) ~ /^([ \t].*)?$/) {
        in_paragraph = 0
        next
      }
      # Every definition-shaped line a renderer could read claims its label,
      # container prefixes included; only the first claim decides.
      body = bare
      sub(/^[>[:space:]]*/, "", body)
      if (body ~ /^[-+*][ \t]/ || body ~ /^[0-9]+[.)][ \t]/) {
        sub(/^[^ \t]+[ \t]+/, "", body)
      }
      if (!in_paragraph && body ~ /^\[[^][]+\][ \t]*:/) {
        ref = norm(substr(body, 2, index(body, "]") - 2))
        dest = ""
        if (body == bare && indent < 4) dest = definition_destination(body)
        if (!(ref in claimed)) {
          claimed[ref] = 1
          if (dest != "") defs[ref] = dest
        }
        in_paragraph = (dest == "")
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
