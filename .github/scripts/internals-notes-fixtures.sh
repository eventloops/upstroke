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
printf '[`Source`](%s) with continuation prose.\n' "$link" > "$tree/$notes"
check 0 balanced_code_span_label_is_visible

for context in quoted_tilde_fence listed_tilde_fence quoted_indented_code listed_indented_code ordered_indented_code code_span_crosses_link_bracket; do
  fixture capacity
  case "$context" in
    quoted_tilde_fence) printf '> ~~~markdown [Source](%s)\n> example\n> ~~~\n' "$link" ;;
    listed_tilde_fence) printf -- '- ~~~markdown [Source](%s)\n  example\n  ~~~\n' "$link" ;;
    quoted_indented_code) printf '>     [Source](%s)\n' "$link" ;;
    listed_indented_code) printf -- '-     [Source](%s)\n' "$link" ;;
    ordered_indented_code) printf '1.     [Source](%s)\n' "$link" ;;
    code_span_crosses_link_bracket) printf '[`Source](%s)`\n' "$link" ;;
  esac > "$tree/$notes"
  check 1 "${context}_does_not_supply_opening_backlink"
done

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

# The opening backlink may be spelled in any Markdown link form a reader can
# follow: an inline destination, bare or angle-delimited, with an optional
# title; or a full, collapsed or shortcut reference resolved against a
# block-level link reference definition.

fixture capacity
printf '[Source](<%s>)\n' "$link" > "$tree/$notes"
check 0 angle_delimited_destination_is_a_backlink

for title in double single paren; do
  fixture capacity
  case "$title" in
    double) printf '[Source](%s "the module")\n' "$link" ;;
    single) printf "[Source](%s 'the module')\n" "$link" ;;
    paren)  printf '[Source](%s (the module))\n' "$link" ;;
  esac > "$tree/$notes"
  check 0 "inline_${title}_title_is_a_backlink"
done

fixture capacity
printf '[Source][mod]\n\n[mod]: %s\n' "$link" > "$tree/$notes"
check 0 full_reference_is_a_backlink

fixture capacity
printf '[`%s`][]\n\n[`%s`]: %s\n' "$module" "$module" "$link" > "$tree/$notes"
check 0 collapsed_reference_is_a_backlink

fixture capacity
printf 'Extended notes for [source].\n\n[source]: %s\n' "$link" > "$tree/$notes"
check 0 shortcut_reference_is_a_backlink

fixture capacity
printf '# `%s`\n\n[Source][ MOD ]\n\n[mod]: <%s> "the module"\n' "$module" "$link" > "$tree/$notes"
check 0 reference_label_folds_case_and_whitespace

fixture capacity
printf '[Source][mod]\n\n```markdown\n[mod]: ../../src/missing.rs\n```\n\n[mod]: %s\n' "$link" > "$tree/$notes"
check 0 definition_after_a_fenced_example_still_resolves

fixture capacity
printf '[Source][mod]\n\n[mod]: %s\n\n[mod]: ../../src/missing.rs\n' "$link" > "$tree/$notes"
check 0 first_definition_of_a_label_wins

fixture capacity
printf '[Source][mod]\n\n    code line\n[mod]: %s\n' "$link" > "$tree/$notes"
check 0 definition_after_an_indented_code_block_resolves

fixture capacity
printf '[Source][mod]\n\n[mod]:\n\n[mod]: %s\n' "$link" > "$tree/$notes"
check 0 a_definition_whose_destination_wraps_is_not_read

# A reference resolves only against a definition a reader's renderer would
# also see. Code, comments, containers and paragraph text supply none, and a
# definition is invisible, so it cannot be the opening either.

for context in no_definition fenced_definition tilde_fenced_definition commented_definition inline_commented_definition indented_definition tab_indented_definition paragraph_definition quoted_definition listed_definition leading_definition image_reference code_span_reference; do
  fixture capacity
  case "$context" in
    no_definition)               printf '[Source][mod]\n' ;;
    fenced_definition)           printf '[Source][mod]\n\n```\n[mod]: %s\n```\n' "$link" ;;
    tilde_fenced_definition)     printf '[Source][mod]\n\n~~~\n[mod]: %s\n~~~\n' "$link" ;;
    commented_definition)        printf '[Source][mod]\n\n<!--\n[mod]: %s\n-->\n' "$link" ;;
    inline_commented_definition) printf '[Source][mod]\n\n<!-- [mod]: %s -->\n' "$link" ;;
    indented_definition)         printf '[Source][mod]\n\n    [mod]: %s\n' "$link" ;;
    tab_indented_definition)     printf '[Source][mod]\n\n\t[mod]: %s\n' "$link" ;;
    paragraph_definition)        printf '[Source][mod]\nand more prose\n[mod]: %s\n' "$link" ;;
    quoted_definition)           printf '[Source][mod]\n\n> [mod]: %s\n' "$link" ;;
    listed_definition)           printf '[Source][mod]\n\n- [mod]: %s\n' "$link" ;;
    leading_definition)          printf '[mod]: %s\n\n[Source][mod]\n' "$link" ;;
    image_reference)             printf '![Source][mod]\n\n[mod]: %s\n' "$link" ;;
    code_span_reference)         printf '`[Source][mod]`\n\n[mod]: %s\n' "$link" ;;
  esac > "$tree/$notes"
  check 1 "${context}_does_not_supply_opening_backlink"
done

for context in angle_destination_with_space unclosed_title trailing_junk_in_parens; do
  fixture capacity
  case "$context" in
    angle_destination_with_space) printf '[Source](<../../src/cap acity.rs>)\n' ;;
    unclosed_title)               printf '[Source](%s "the module)\n' "$link" ;;
    trailing_junk_in_parens)      printf '[Source](%s junk)\n' "$link" ;;
  esac > "$tree/$notes"
  check 1 "${context}_is_not_a_backlink"
done

fixture capacity
printf '[Source][mod]\n\n[mod]: ../../src/missing.rs\n' > "$tree/$notes"
check 1 reference_must_resolve_to_own_module

for context in fenced_definition_after_a_blank commented_definition_after_a_blank definition_with_trailing_text; do
  fixture capacity
  case "$context" in
    fenced_definition_after_a_blank)    printf '[Source][mod]\n\n```\n\n[mod]: %s\n```\n' "$link" ;;
    commented_definition_after_a_blank) printf '[Source][mod]\n\n<!--\n\n[mod]: %s\n-->\n' "$link" ;;
    definition_with_trailing_text)      printf '[Source][mod]\n\n[mod]: %s junk\n' "$link" ;;
  esac > "$tree/$notes"
  check 1 "${context}_does_not_supply_opening_backlink"
done

fixture capacity
printf '[mod]: %s\n\n[mod]: %s\n' "$link" "$link" > "$tree/$notes"
check 1 a_definition_cannot_open_the_file

echo "internals notes fixtures: $cases cases passed"
