# `src/plan/markdown/drafts.rs`

Extended notes for [`src/plan/markdown/drafts.rs`](../../../../src/plan/markdown/drafts.rs).

These notes preserve the module comments after the annotation repairs. Item headings quote source lines for navigation.

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

## `let Some(slice) = raw.get(section.content.clone()) else {`

The range came from `split_sections`' own walk of `raw`, so it lies on
event boundaries of this text; answering its absence beats a panic on a
plan file.

## `let mut annotation_spans: Vec<Range<usize>> = Vec::new();`

Spans of upstroke annotation comments (slice-relative), removed from body.

## `let mut armed = false;`

An `Acceptance:` paragraph or heading arms the next list.

## `let mut item_slots: Vec<usize> = Vec::new();`

Slots in `draft.acceptance`, one per open item, so a criterion with a
nested sub-list keeps both its own text and the children, in order.

## `for comment in html.observe(&event, &range, &normalized) {`

The accumulator sees every event and hands a comment back once the
HTML block or inline construct holding it is complete.

## `Event::Start(Tag::CodeBlock(_)) | Event::Start(Tag::Table(_)) => armed = false,`

Blocks that end an acceptance run; HTML comments and headings
deliberately do not, so an invisible annotation between the
header and its list cannot silently disarm collection.

## `draft.body = match strip_spans(slice, &annotation_spans) {`

The spans are the comments' own source bytes, mapped by the accumulator
through the ranges the parser reported, so a refusal here is a defect in
that mapping and not in the plan; the body is kept whole and says so.

## `fn open_criterion<'a>(`

The criterion the innermost open acceptance item is collecting into. The
slots are indices pushed when the item opened, each beside the criterion
it names, so the lookup cannot miss; answering absence keeps it total.

## `pub(super) fn checklist_drafts(raw: &str, warnings: &mut Vec<String>) -> Vec<Draft> {`

Fallback when a plan has no `##`/`###` sections: top-level checklist items
(`- [ ]` / `- [x]`) and ordered-list steps (`1.` — the common Claude Code
plan-mode shape) become tasks. Plain unordered bullets do not; prose lists
would false-positive. Nested content joins the body.

## `Some((draft, sink)) => {`

The body is built from the text events, which an HTML
block never produces, so the span the sink returns has
nothing to cut here — and the prose an unterminated
comment swallowed is put back from the original source,
preserving its line endings and container prefixes too.

## `None => {`

Top-level HTML before, between or after the items belongs
to no task; an annotation there would bind to nothing.

## `Event::SoftBreak | Event::HardBreak => {`

A wrapped title must not run its words together.

## `for comment in html.finish() {`

Every block is closed by the parser; the contract is that nothing fed
to the accumulator goes unreported.
