# `src/plan/markdown/drafts.rs`

Extended notes for [`src/plan/markdown/drafts.rs`](../../../../src/plan/markdown/drafts.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Drafts: one per task-to-be, before ids and dependencies are finalized.

Two intake shapes, one output type. [`section_draft`] walks the body of a
`##`/`###` section, splitting off its title, its acceptance criteria —
armed by an `Acceptance:` paragraph or heading and collected through nested
sub-lists — its path hints, and its annotation, whose comment spans are
then cut out of the body text. [`checklist_drafts`] is the fallback for a
plan with no sections: top-level `- [ ]` items and ordered `1.` steps
become tasks, plain prose bullets do not.

The confluence of the DAG: [`super::sections`], [`super::annotation`] and
[`super::hints`] all feed it, and [`super::assemble`] consumes what it
produces.

## `pub(super) fn section_draft(raw: &str, section: &Section, w…` › `let mut annotation_spans: Vec<Range<usize>> = Vec::new();`

Spans of upstroke annotation comments (slice-relative), removed from body.

## `pub(super) fn section_draft(raw: &str, section: &Section, w…` › `let mut armed = false;`

An `Acceptance:` paragraph or heading arms the next list.

## `pub(super) fn section_draft(raw: &str, section: &Section, w…` › `let mut item_slots: Vec<usize> = Vec::new();`

Slots in `draft.acceptance`, one per open item, so a criterion with a
nested sub-list keeps both its own text and the children, in order.

## `pub(super) fn section_draft(raw: &str, section: &Section, w…` › `if let Event::Html(t) | Event::InlineHtml(t) = &event {`

HTML accumulates across events; everything else flushes it first.

## `pub(super) fn section_draft(raw: &str, section: &Section, w…` › `Event::Start(Tag::CodeBlock(_)) | Event::Start(Tag::Table(_)) => armed = false,`

Blocks that end an acceptance run; HTML comments and headings
deliberately do not, so an invisible annotation between the
header and its list cannot silently disarm collection.

## `pub(super) fn checklist_drafts(raw: &str, warnings: &mut Vec<String>) -> Vec<Draft> {`

Fallback when a plan has no `##`/`###` sections: top-level checklist items
(`- [ ]` / `- [x]`) and ordered-list steps (`1.` — the common Claude Code
plan-mode shape) become tasks. Plain unordered bullets do not; prose lists
would false-positive. Nested content joins the body.

## `pub(super) fn checklist_drafts(raw: &str, warnings: &mut Ve…` › `Event::SoftBreak | Event::HardBreak => {`

A wrapped title must not run its words together.
