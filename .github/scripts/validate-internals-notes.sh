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
#   N5  No notes file carries a Rustdoc-only link form. Every inline link
#       destination resolves -- a URL, a same-file anchor, or a relative path
#       inside this repository -- and in the effects notes every bracketed
#       code span is an inline link rather than a shortcut reference.
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

# --- N5. no Rustdoc-only link form survives in the notes ---------------------
#
# The notes were rustdoc before they were Markdown, and rustdoc's two link
# forms mean nothing to a Markdown reader. `docs/` is also the GitHub Pages
# source for upstroke.rs, so what these files render to is published.
#
# Two claims, with two domains, because the two forms cost differently to
# remove and only one of them has been measured over the whole tree:
#
#   (a) EVERY file under docs/internals/, README.md included -- N2 and N3 skip
#       that one because it mirrors no module, but its links point at real
#       files like any other's. An inline link destination is an
#       http/https/mailto URL, a same-file `#anchor`, or a relative path that
#       exists in this repository and stays inside it. A Rustdoc item path is
#       none of those, and no renderer keeps it: GitHub's GFM drops the link
#       and emits the label alone, measured through `POST /markdown`, and the
#       plain CommonMark renderer the audit ran emits
#       `<a href="crate::effects::census_domain">`, an href that resolves
#       nowhere. Eleven destinations of that shape were in the tree when this
#       was written: eight spelled `crate::`, and `Self::close_and_wait`,
#       `RunState::apply` and `std::time::Duration`, which a `crate::` search
#       does not find at all. Resolving the destination is what finds every
#       spelling of it, and a mistyped relative path with them.
#
#   (b) The effects notes -- `docs/internals/effects.md` and everything under
#       `docs/internals/effects/`. A bracketed code span is an inline link or
#       it is not bracketed: `[`normalize_lint`]` is rustdoc's shortcut
#       reference, and with no reference definition CommonMark renders it as
#       `[<code>normalize_lint</code>]`, bracket noise rather than
#       navigation. 145 of those were in these ten files; 1743 remain in 111
#       other notes files, and PR161-NOTES-SHORTCUT-REFERENCES-TREE is the
#       standing entry for them. This claim is what keeps the converted files
#       converted, so widen the domain with the conversion, never ahead of it.
#
# THIS IS A LEXICAL REFUSAL, NOT A MARKDOWN PARSER, and that is a decision
# rather than a shortcut. The first two versions of this check tried to read
# the file the way CommonMark reads it -- drop fenced blocks, pair backtick
# runs into code spans, scan a leaf block at a time -- and review found four
# defects in the reading itself: a whole-file scan let an odd backtick swallow
# a later destination; a wrapped line opening on `#[cfg(test)]` was taken for
# an ATX heading; ` ```a code span``` ` was taken for an opening fence, whose
# info string may not contain a backtick, and everything after it was skipped;
# and "```not-a-closing-fence" was taken for a closing one. Each fix was
# correct and each left the next corner: HTML blocks, setext headings, lazy
# continuation, list-item containers, indented code, tables. `test-docs-
# consistency.sh` records four review rounds learning the same thing about the
# same kind of surface, and withdrew every claim that needed it.
#
# So this scans the bytes and refuses the form wherever it appears, in a code
# span or a fence or a comment as much as in prose. A false refusal is a
# quoted example, and the only file whose job is to quote these forms is
# `docs/internals/README.md`; `quoted_destination` below lists its three by
# exact text. Anything else is a violation, and a new quotation is a visible
# row in that list rather than a silent pass. The trade is deliberate: a
# refusal too many costs one reviewed line, a refusal too few is the defect
# coming back.
#
# Two things this deliberately does not do. It does not resolve `#anchor`s: a
# heading's slug belongs to the renderer, these headings repeat verbatim
# within a file, and which one an anchor names is a review duty rather than a
# gate's. And it does not read link reference definitions -- `[label]: dest`.
# Telling a real definition from the `[`item`]: prose` shape that fills these
# notes needs block position, which is exactly what is not computed here.
# Inside (b)'s domain that shape cannot survive anyway: it is a bracketed code
# span with no `(` after it, which is what (b) refuses. Outside it, it is part
# of the 1743.

# The occurrences that are quoted rather than used, by file and exact
# destination text. All three are worked examples in the notes README: two
# predate this check and show the backlink contract, and the third is the
# retired Rustdoc form itself, shown so a reader can recognize it.
quoted_destinations=(
  'docs/internals/README.md|../../../src/runner/host.rs'
  'docs/internals/README.md|relative/path.rs'
  'docs/internals/README.md|crate::effects::census_domain'
)
declare -A quoted_used=()

dest_count=0
while IFS= read -r notes; do
  in_effects=0
  case "$notes" in
    "$notes_root/effects.md" | "$notes_root"/effects/*) in_effects=1 ;;
  esac

  while IFS=$'\t' read -r kind at text; do
    if [[ "$kind" == SHORTCUT ]]; then
      error "$notes:$at: [$text] is a Rustdoc shortcut reference; CommonMark renders it as bracketed code. Make it an inline Markdown link or drop the brackets"
      continue
    fi
    # CommonMark's destination, then its optional title: `<dest>` keeps
    # spaces, a bare destination ends at the first one. A destination may be
    # written on the line after its `(`, so leading whitespace is stripped
    # rather than read as an empty path.
    dest="${text#"${text%%[![:space:]]*}"}"
    if [[ "$dest" == '<'*'>'* ]]; then
      dest="${dest#<}"
      dest="${dest%%>*}"
    else
      dest="${dest%%[[:space:]]*}"
    fi
    quoted=0
    for entry in "${quoted_destinations[@]}"; do
      if [[ "$notes|$dest" == "$entry" ]]; then
        quoted_used["$entry"]=1
        quoted=1
        break
      fi
    done
    (( quoted == 0 )) || continue
    case "$dest" in
      http://* | https://* | mailto:* | '#'*) continue ;;
    esac
    dest_count=$((dest_count + 1))
    target="${dest%%#*}"
    if [[ -z "$target" ]]; then
      error "$notes:$at: link destination '$dest' has no path before its anchor"
      continue
    fi
    resolved="$(cd "${notes%/*}" && realpath -m --relative-to="$root" "$target" 2>/dev/null || true)"
    if [[ -z "$resolved" || "$resolved" == ../* ]]; then
      error "$notes:$at: link destination '$dest' resolves outside this repository"
      continue
    fi
    [[ -e "$root/$resolved" ]] \
      || error "$notes:$at: link destination '$dest' resolves to $resolved, which does not exist"
  done < <(awk -v effects="$in_effects" '
    function lineof(p,   k) {
      for (k = 1; k <= last; k++) if (p <= nl[k]) return k
      return last
    }
    { sub(/\r$/, ""); doc = doc $0 "\n"; nl[NR] = length(doc); last = NR }
    END {
      n = length(doc)
      i = 1
      while (i <= n) {
        c = substr(doc, i, 1)
        # A bracketed code span. The backtick run is matched inside the
        # brackets and nowhere else, so nothing elsewhere in the file can
        # change what this is: no pairing across paragraphs, no fence state,
        # no block boundary to get wrong.
        if (effects && c == "[" && (i == 1 || substr(doc, i - 1, 1) != "\\") \
            && substr(doc, i + 1, 1) == "`") {
          j = i + 1
          while (substr(doc, j, 1) == "`") j++
          run = j - i - 1
          k = j
          closed = 0
          while (k <= n) {
            if (substr(doc, k, 2) == "\n\n") break
            if (substr(doc, k, 1) == "`") {
              m = k
              while (m <= n && substr(doc, m, 1) == "`") m++
              if (m - k == run) { closed = m; break }
              k = m
            } else k++
          }
          if (closed > 0 && substr(doc, closed, 1) == "]" \
              && substr(doc, closed + 1, 1) != "(") {
            label = substr(doc, i + 1, closed - i - 1)
            gsub(/[\n\t]/, " ", label)
            printf "SHORTCUT\t%d\t%s\n", lineof(i), label
            i = closed + 1
            continue
          }
        }
        # An inline destination. Parentheses balance, a line break inside is
        # legal CommonMark and common in wrapped prose, and a blank line ends
        # the search rather than running the rest of the file into it.
        if (c == "]" && substr(doc, i + 1, 1) == "(") {
          depth = 1
          k = i + 2
          torn = 0
          while (k <= n) {
            if (substr(doc, k, 2) == "\n\n") { torn = 1; break }
            ch = substr(doc, k, 1)
            if (ch == "(") depth++
            else if (ch == ")") { depth--; if (depth == 0) break }
            k++
          }
          if (!torn && k <= n && substr(doc, k, 1) == ")") {
            dst = substr(doc, i + 2, k - i - 2)
            gsub(/[\n\t]/, " ", dst)
            printf "DEST\t%d\t%s\n", lineof(i), dst
            i = k + 1
            continue
          }
        }
        i++
      }
    }
  ' "$notes")
done < <(find "$notes_root" -name '*.md' 2>/dev/null | sort)

(( dest_count > 0 )) || error "no relative link destinations found under $notes_root; N5 is inert"

# An exemption that stopped applying is a hole nobody is watching. Every row
# whose file is present must have matched something in it.
for entry in "${quoted_destinations[@]}"; do
  [[ -f "${entry%%|*}" ]] || continue
  [[ -n "${quoted_used[$entry]:-}" ]] \
    || error "${entry%%|*} no longer quotes '${entry#*|}'; drop that row from quoted_destinations"
done

# The effects notes are (b)'s whole domain. If the module is here and its
# notes are not, the claim measures nothing and says so rather than passing.
if [[ -f src/effects.rs && ! -f "$notes_root/effects.md" ]]; then
  error "src/effects.rs exists but $notes_root/effects.md does not; N5's converted domain is empty"
fi

if (( failed == 0 )); then
  echo "internals notes: $marker_count marker(s), $notes_count notes file(s), $dest_count link destination(s), all resolve both ways"
fi

exit "$failed"
