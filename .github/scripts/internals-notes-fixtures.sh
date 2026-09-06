#!/usr/bin/env bash
# Exercise the real N1-N5 validator over isolated source/notes trees.
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

# --- N5. Rustdoc-only link forms -------------------------------------------
# `note` appends to the fixture's notes file, below the opening backlink N3
# reads, so each case differs from a passing tree by one paragraph. N5 is a
# lexical refusal, so several of these assert that a form is refused where
# CommonMark would not read it as a link at all. That is the contract.
note() { printf '\n%s\n' "$1" >> "$tree/$notes"; }

# The notes README is the one file allowed to quote a retired form, and only
# the three destinations the validator lists by exact text. `readme` writes
# all three, so a case can add a fourth and watch it refused.
readme() {
  {
    printf '# Internal module notes\n\n'
    printf 'Backlink example: [`src/runner/host.rs`](../../../src/runner/host.rs).\n\n'
    printf 'A `[Source](relative/path.rs)` link satisfies the same contract.\n\n'
    printf 'The retired form is [`census_domain`](crate::effects::census_domain).\n'
  } > "$tree/docs/internals/README.md"
}

fixture effects
note 'A shortcut reference to [`normalize_lint`] renders as bracketed code.'
check 1 shortcut_reference_in_effects_notes_is_rejected

fixture effects/tests
note 'Deeper in the subtree, [`census_domain`] is the same form.'
check 1 shortcut_reference_below_effects_is_rejected

fixture effects
note 'Wrapped across lines, [`declared_whole_file_test_
modules`] is still one code span.'
check 1 shortcut_reference_across_a_line_break_is_rejected

fixture effects
note 'An inline link [`census_domain`](../../src/effects.rs) is the fix.'
check 0 inline_link_in_effects_notes_is_accepted

fixture effects
note '```markdown
[`normalize_lint`] is the form these notes document.
```'
check 1 shortcut_reference_in_a_fenced_block_is_still_rejected

fixture capacity
note 'Outside the effects notes [`Budget`] is not yet converted.'
check 0 shortcut_reference_outside_the_effects_notes_is_out_of_domain

fixture capacity
note 'A Rustdoc destination [`census_domain`](crate::effects::census_domain).'
check 1 rustdoc_crate_destination_is_rejected_tree_wide

fixture capacity
note 'A Rustdoc destination without `crate::`: [`apply`](RunState::apply).'
check 1 rustdoc_item_destination_without_crate_is_rejected

fixture capacity
note 'Wrapped, as prose wraps: [`census_domain`](
crate::effects::census_domain) is still that destination.'
check 1 rustdoc_destination_wrapped_onto_the_next_line_is_rejected

fixture capacity
note "Wrapped and correct: [source](
$link) resolves like any other."
check 0 relative_destination_wrapped_onto_the_next_line_is_accepted

fixture capacity
note 'A destination that is simply wrong: [notes](../missing/other.md).'
check 1 unresolvable_relative_destination_is_rejected

fixture capacity
note 'A destination that leaves the repository: [hosts](../../../../etc/hosts).'
check 1 destination_outside_the_repository_is_rejected

fixture capacity
note 'Parenthesised: [notes](../missing/other(1).md) is one destination.'
check 1 a_parenthesised_destination_is_read_whole

fixture capacity
note 'An external link [DESIGN](https://example.invalid/DESIGN.md#anchor).'
check 0 external_destination_is_not_resolved

fixture capacity
note 'A same-file anchor [above](#module) is the renderer'"'"'s to resolve.'
check 0 same_file_anchor_is_not_resolved

fixture capacity
note "A destination with a title [source]($link \"the module\")."
check 0 destination_with_a_title_is_read_without_it

fixture capacity
note "An angle-bracket destination [source](<$link>)."
check 0 angle_bracket_destination_is_read_without_its_brackets

fixture capacity
note '```a code span```'
note 'A Rustdoc destination [`census_domain`](crate::effects::census_domain).'
check 1 an_info_string_of_backticks_does_not_hide_what_follows

fixture capacity
note '```text
[notes](../missing/other.md)
```'
check 1 destination_in_a_fenced_block_is_still_rejected

fixture capacity
note 'The documented example `[Source](relative/path.rs)` is a code span.'
check 1 destination_inside_a_code_span_is_still_rejected

fixture capacity
note 'A stray backtick ` opens nothing.'
note 'The link [notes](../missing/other.md) is in the next block.'
note 'And `code` closes nothing across it.'
check 1 an_odd_backtick_earlier_changes_nothing

fixture capacity
readme
check 0 the_notes_readme_may_quote_the_three_listed_destinations

fixture capacity
readme
printf 'And an unlisted one: [x](crate::events::RunState).\n' >> "$tree/docs/internals/README.md"
check 1 an_unlisted_destination_in_the_notes_readme_is_not_exempt

fixture capacity
readme
sed '/census_domain/d' "$tree/docs/internals/README.md" > "$tree/trimmed"
mv "$tree/trimmed" "$tree/docs/internals/README.md"
check 1 a_quoted_row_that_stopped_matching_is_an_error

fixture capacity
printf 'pub struct Effects;\n' > "$tree/src/effects.rs"
check 1 effects_module_without_notes_leaves_the_converted_domain_empty

echo "internals notes fixtures: $cases cases passed"
