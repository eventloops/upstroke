# `src/plan/markdown/sections.rs`

Extended notes for [`src/plan/markdown/sections.rs`](../../../../src/plan/markdown/sections.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Where a task begins: the `##`/`###` headings that delimit sections.

Only container-free headings are boundaries — one nested in a blockquote or
a list item is quoted material, not plan structure — and an `Acceptance` /
`Done when` / `Success criteria` heading labels the section above rather
than opening a new one, so the recognizer for those lives here too and
[`super::drafts`] reuses it when it arms criterion collection.

Upstream of `drafts`; reads `<!-- upstroke: ... -->` comments off a heading
line through [`super::annotation`] and parses with [`super::md_options`].

## `pub(super) struct Section` › `pub(super) content: Range<usize>,`

Byte range of the section body in the original text: from the end of
the heading block to the start of the next `##`/`###` heading.

## `pub(super) struct Section` › `pub(super) inline_annotation: Option<String>,`

Annotation written inline on the heading line itself.

## `struct HeadingScan {`

Heading state while scanning: accumulated title text, the heading block
span, and any inline annotation found on the heading line.

## `pub(super) fn split_sections(raw: &str) -> Vec<Section>` › `let mut container_depth = 0usize;`

Headings nested in blockquotes or list items are quoted material, not
plan structure — only container-free headings delimit tasks.

## `pub(super) fn split_sections(raw: &str) -> Vec<Section>` › `if is_acceptance_header(strip_trailing_colon(&title)) {`

`### Acceptance` and friends label the criteria of the
section above, so they are not task boundaries; the
section body flows through and section_draft arms on it.
