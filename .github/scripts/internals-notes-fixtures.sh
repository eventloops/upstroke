#!/usr/bin/env bash
# Exercise the real N1-N4 validator over isolated source/notes trees.
set -euo pipefail
validator="${1:?usage: internals-notes-fixtures.sh VALIDATOR}"
scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
cases=0

fixture() {
  cases=$((cases + 1))
  tree="$scratch/$cases"
  module="src/$1.rs"
  notes="docs/internals/$1.md"
  mkdir -p "$tree/${module%/*}" "$tree/${notes%/*}"
  link="$(realpath -m --relative-to="$tree/${notes%/*}" "$tree/$module")"
  printf '//! Extended notes: `%s`\npub struct Example;\n' "$notes" > "$tree/$module"
  printf '# `%s`\n\nExtended notes for [`%s`](%s).\n' "$module" "$module" "$link" > "$tree/$notes"
}

check() {
  local expected="$1" name="$2" actual=0
  bash "$validator" "$tree" > "$tree/result.log" 2>&1 || actual=$?
  if [[ "$actual" != "$expected" ]]; then
    cat "$tree/result.log" >&2
    echo "$name: expected exit $expected, got $actual" >&2
    exit 1
  fi
}

for depth in capacity agent/bin agent/proc/test_support/readiness; do
  fixture "$depth"
  check 0 "backlink_at_depth_$depth"
done

fixture runner/host
printf '# `%s` - the host runner, `host-v1`\n\nExtended notes for [`%s`](%s). The code is the authority\nfor what it does.\n' "$module" "$module" "$link" > "$tree/$notes"
check 0 curated_heading_and_continuation_prose

fixture capacity
printf '\n\nExtended notes for [`%s`](%s).\n' "$module" "$link" > "$tree/$notes"
check 0 opening_link_without_heading

fixture capacity
printf '[Source](%s) with continuation prose.\n' "$link" > "$tree/$notes"
check 0 source_label_does_not_require_generated_wording

fixture capacity
sed 's/$/\r/' "$tree/$notes" > "$tree/crlf"
mv "$tree/crlf" "$tree/$notes"
check 0 backlink_with_crlf

fixture capacity
printf 'module (%s)\n' "$link" > "$tree/$notes"
check 1 plain_path_is_not_a_backlink

fixture capacity
printf '<!-- (%s) -->\n[Source](../../src/missing.rs)\n' "$link" > "$tree/$notes"
check 1 hidden_decoy_does_not_mask_wrong_link

fixture capacity
printf '# `%s`\n\nExtended notes for [`%s`](../../src/missing.rs).\n<!-- (%s) -->\n' "$module" "$module" "$link" > "$tree/$notes"
check 1 visible_backlink_must_resolve_to_own_module

for context in html fenced tilde_fenced indented tab_indented inline escaped image reference details hidden_label empty_label entity_label; do
  fixture capacity
  paragraph="Extended notes for [\`$module\`]($link)."
  case "$context" in
    html) printf '<!--\n%s\n-->\n' "$paragraph" ;;
    fenced) printf '```markdown\n%s\n```\n' "$paragraph" ;;
    tilde_fenced) printf '~~~markdown %s\n~~~\n' "$paragraph" ;;
    indented) printf '    %s\n' "$paragraph" ;;
    tab_indented) printf '  \t%s\n' "$paragraph" ;;
    inline) printf '`%s`\n' "$paragraph" ;;
    escaped) printf 'Extended notes for \\[source](%s).\n' "$link" ;;
    image) printf '![Source](%s)\n' "$link" ;;
    reference) printf '[unused]: (%s)\n' "$link" ;;
    details) printf '<details>\n%s\n</details>\n' "$paragraph" ;;
    hidden_label) printf '[<!-- Source -->](%s)\n' "$link" ;;
    empty_label) printf '[   ](%s)\n' "$link" ;;
    entity_label) printf '[&#32;](%s)\n' "$link" ;;
  esac > "$tree/$notes"
  check 1 "${context}_does_not_supply_opening_backlink"
done

fixture capacity
printf '<!--\n' > "$tree/$notes"
printf '# `%s`\n\nExtended notes for [`%s`](%s).\n-->\n' "$module" "$module" "$link" >> "$tree/$notes"
check 1 hidden_document_does_not_supply_backlink

fixture capacity
printf '# `%s` <!--\n\nExtended notes for [`%s`](%s).\n-->\n' "$module" "$module" "$link" > "$tree/$notes"
check 1 comment_opened_in_heading_does_not_supply_backlink

fixture capacity
printf '//! Extended notes: `%s` extra\npub struct Example;\n' "$notes" > "$tree/$module"
check 1 marker_with_extra_prose_is_rejected

fixture capacity
printf '//! Extended notes: `docs/internals/wrong.md`\npub struct Example;\n' > "$tree/$module"
check 1 marker_must_name_own_notes

fixture capacity
rm "$tree/$module"
check 1 notes_without_source_are_rejected

fixture capacity
printf 'pub struct Example;\n' > "$tree/$module"
check 1 notes_without_marker_are_rejected

fixture capacity
printf '//! Extended notes: `%s`\n' "$notes" >> "$tree/$module"
check 1 duplicate_marker_is_rejected

fixture capacity
printf 'pub struct Example;\n//! Extended notes: `%s`\n' "$notes" > "$tree/$module"
check 1 marker_after_code_is_rejected

fixture capacity
rm -r "$tree/docs/internals"
check 1 absent_notes_tree_with_marker_is_rejected

fixture capacity
rm -r "$tree/docs/internals"
printf 'pub struct Example;\n' > "$tree/$module"
check 1 absent_notes_tree_without_markers_is_rejected

fixture capacity
rm "$tree/$notes"
check 1 empty_notes_tree_is_rejected

echo "internals notes fixtures: $cases cases passed"
