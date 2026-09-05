# `src/plan/markdown/sections.rs`

Extended notes for [`src/plan/markdown/sections.rs`](../../../../src/plan/markdown/sections.rs).

These notes preserve the module comments after the annotation repairs. Item headings quote source lines for navigation.

## Module

Where a task begins: the `##`/`###` headings that delimit sections.

Only container-free headings are boundaries — one nested in a blockquote or
a list item is quoted material, not plan structure — and an `Acceptance` /
`Done when` / `Success criteria` heading labels the section above rather
than opening a new one, so the recognizer for those lives here too and
[`super::drafts`] reuses it when it arms criterion collection.

Upstream of `drafts`; reads `<!-- upstroke: ... -->` comments off a heading
line through [`super::annotation`] and parses with [`super::md_options`].

## `pub(super) content: Range<usize>,`

Byte range of the section body in the original text: from the end of
the heading block to the start of the next `##`/`###` heading.

## `pub(super) inline_annotations: Vec<String>,`

Every upstroke annotation written inline on the heading line itself,
in order; the sink takes the first and warns for the rest.

## `struct HeadingScan {`

Heading state while scanning: accumulated title text, the heading block
span, and the inline annotations found on the heading line.

## `plain_text: String,`

Consecutive, unescaped text events, separate from code and HTML.

## `let mut container_depth = 0usize;`

Headings nested in blockquotes or list items are quoted material, not
plan structure — only container-free headings delimit tasks.

## `if is_acceptance_header(strip_trailing_colon(&title)) {`

`### Acceptance` and friends label the criteria of the
section above, so they are not task boundaries; the
section body flows through and section_draft arms on it.

## `let escaped = normalized.get(..range.start).is_some_and(|prefix| {`

Text event ranges omit an escaping backslash, so
equality alone cannot distinguish `\<` from `<`.

## `scan.finish_plain_text();`

An escape or entity produces literal text rather
than an annotation opener in the source.
