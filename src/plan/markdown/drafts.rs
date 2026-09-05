//! Drafts: one per task-to-be, before ids and dependencies are finalized.
//!
//! Two intake shapes, one output type. [`section_draft`] walks the body of a
//! `##`/`###` section, splitting off its title, its acceptance criteria —
//! armed by an `Acceptance:` paragraph or heading and collected through nested
//! sub-lists — its path hints, and its annotation, whose comment spans are
//! then cut out of the body text. [`checklist_drafts`] is the fallback for a
//! plan with no sections: top-level `- [ ]` items and ordered `1.` steps
//! become tasks, plain prose bullets do not.
//!
//! The confluence of the DAG: [`super::sections`], [`super::annotation`] and
//! [`super::hints`] all feed it, and [`super::assemble`] consumes what it
//! produces.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::annotation::{
    Annotation, AnnotationSink, HtmlAccumulator, OPEN, strip_spans, upstroke_body,
};
use super::hints::{collect_code_hint, collect_text_hints};
use super::md_options;
use super::sections::{Section, is_acceptance_header, strip_trailing_colon};

#[derive(Default)]
pub(super) struct Draft {
    pub(super) title: String,
    pub(super) body: String,
    pub(super) acceptance: Vec<String>,
    pub(super) hints: Vec<String>,
    ann: Option<Annotation>,
}

impl Draft {
    pub(super) fn annotation(&self) -> Annotation {
        self.ann.clone().unwrap_or_default()
    }
}

pub(super) fn section_draft(raw: &str, section: &Section, warnings: &mut Vec<String>) -> Draft {
    let ctx = format!("section `{}`", section.title);
    let mut draft = Draft {
        title: section.title.clone(),
        ..Draft::default()
    };
    // The range came from `split_sections`' own walk of `raw`, so it lies on
    // event boundaries of this text; answering its absence beats a panic on a
    // plan file.
    let Some(slice) = raw.get(section.content.clone()) else {
        warnings.push(format!(
            "internal error: the body range {:?} of {ctx} is not within the plan text; the body is left empty",
            section.content
        ));
        return draft;
    };
    let mut sink = AnnotationSink::default();
    for inline in &section.inline_annotations {
        sink.accept(inline, &ctx, warnings);
    }
    // Spans of upstroke annotation comments (slice-relative), removed from body.
    let mut annotation_spans: Vec<Range<usize>> = Vec::new();
    let mut html = HtmlAccumulator::default();

    let mut para_text = String::new();
    let mut in_para = false;
    let mut heading_text = String::new();
    let mut in_heading = false;
    // An `Acceptance:` paragraph or heading arms the next list.
    let mut armed = false;
    let mut acceptance_list_depth = 0usize;
    // Slots in `draft.acceptance`, one per open item, so a criterion with a
    // nested sub-list keeps both its own text and the children, in order.
    let mut item_slots: Vec<usize> = Vec::new();

    for (event, range) in Parser::new_ext(slice, md_options()).into_offset_iter() {
        // The accumulator sees every event and hands a comment back once the
        // HTML block or inline construct holding it is complete.
        for comment in html.observe(&event, &range) {
            if let Some(span) = sink.take(&comment, &ctx, warnings) {
                annotation_spans.push(span);
            }
        }

        match event {
            Event::Start(Tag::Heading { .. }) => {
                in_heading = true;
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                if is_acceptance_header(strip_trailing_colon(&heading_text)) {
                    armed = true;
                }
            }
            Event::Start(Tag::Paragraph) => {
                in_para = true;
                para_text.clear();
                armed = false;
            }
            Event::End(TagEnd::Paragraph) => {
                in_para = false;
                if is_acceptance_header(strip_trailing_colon(&para_text)) {
                    armed = true;
                }
            }
            // Blocks that end an acceptance run; HTML comments and headings
            // deliberately do not, so an invisible annotation between the
            // header and its list cannot silently disarm collection.
            Event::Start(Tag::CodeBlock(_)) | Event::Start(Tag::Table(_)) => armed = false,
            Event::Start(Tag::List(_)) => {
                if armed || acceptance_list_depth > 0 {
                    acceptance_list_depth += 1;
                }
                armed = false;
            }
            Event::End(TagEnd::List(_)) => {
                acceptance_list_depth = acceptance_list_depth.saturating_sub(1);
            }
            Event::Start(Tag::Item) => {
                if acceptance_list_depth > 0 {
                    item_slots.push(draft.acceptance.len());
                    draft.acceptance.push(String::new());
                }
            }
            Event::End(TagEnd::Item) => {
                item_slots.pop();
            }
            Event::Text(t) => {
                if let Some(criterion) = open_criterion(&item_slots, &mut draft.acceptance) {
                    criterion.push_str(&t);
                }
                if in_para {
                    para_text.push_str(&t);
                }
                if in_heading {
                    heading_text.push_str(&t);
                }
                collect_text_hints(&t, &mut draft.hints);
            }
            Event::Code(t) => {
                if let Some(criterion) = open_criterion(&item_slots, &mut draft.acceptance) {
                    criterion.push_str(&t);
                }
                if in_para {
                    para_text.push_str(&t);
                }
                if in_heading {
                    heading_text.push_str(&t);
                }
                collect_code_hint(&t, &mut draft.hints);
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(criterion) = open_criterion(&item_slots, &mut draft.acceptance) {
                    criterion.push(' ');
                }
                if in_para {
                    para_text.push(' ');
                }
                if in_heading {
                    heading_text.push(' ');
                }
            }
            _ => {}
        }
    }
    for comment in html.finish() {
        if let Some(span) = sink.take(&comment, &ctx, warnings) {
            annotation_spans.push(span);
        }
    }

    draft.acceptance = draft
        .acceptance
        .into_iter()
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
        .collect();
    draft.ann = sink.annotation;
    annotation_spans.sort_by_key(|s| s.start);
    // The spans are the comments' own source bytes, mapped by the accumulator
    // through the ranges the parser reported, so a refusal here is a defect in
    // that mapping and not in the plan; the body is kept whole and says so.
    draft.body = match strip_spans(slice, &annotation_spans) {
        Some(body) => body.trim().to_owned(),
        None => {
            warnings.push(format!(
                "internal error: the annotation spans {annotation_spans:?} of {ctx} do not lie on \
                 its text; the body keeps its annotation comments"
            ));
            slice.trim().to_owned()
        }
    };
    draft
}

/// The criterion the innermost open acceptance item is collecting into. The
/// slots are indices pushed when the item opened, each beside the criterion
/// it names, so the lookup cannot miss; answering absence keeps it total.
fn open_criterion<'a>(
    item_slots: &[usize],
    acceptance: &'a mut [String],
) -> Option<&'a mut String> {
    acceptance.get_mut(*item_slots.last()?)
}

/// Fallback when a plan has no `##`/`###` sections: top-level checklist items
/// (`- [ ]` / `- [x]`) and ordered-list steps (`1.` — the common Claude Code
/// plan-mode shape) become tasks. Plain unordered bullets do not; prose lists
/// would false-positive. Nested content joins the body.
pub(super) fn checklist_drafts(raw: &str, warnings: &mut Vec<String>) -> Vec<Draft> {
    let mut drafts = Vec::new();
    let mut list_depth = 0usize;
    let mut item_depth = 0usize;
    let mut current: Option<(Draft, AnnotationSink)> = None;
    let mut is_task_item = false;
    let mut top_list_ordered = false;
    let mut html = HtmlAccumulator::default();

    for (event, range) in Parser::new_ext(raw, md_options()).into_offset_iter() {
        for comment in html.observe(&event, &range) {
            match current.as_mut() {
                // The body is built from the text events, which an HTML
                // block never produces, so the span the sink returns has
                // nothing to cut here — and the prose an unterminated
                // comment swallowed is put back by hand, as the block held
                // it, which is what the sink's warning promises.
                Some((draft, sink)) => {
                    sink.take(&comment, "checklist item", warnings);
                    if !comment.terminated && upstroke_body(&comment.inner).is_some() {
                        draft.body.push_str(OPEN);
                        draft.body.push_str(&comment.inner);
                        draft.body.push(' ');
                    }
                }
                // Top-level HTML before, between or after the items belongs
                // to no task; an annotation there would bind to nothing.
                None => {
                    if upstroke_body(&comment.inner).is_some() {
                        warnings.push(
                            "upstroke annotation outside any checklist item; ignored".to_owned(),
                        );
                    }
                }
            }
        }

        match event {
            Event::Start(Tag::List(start)) => {
                if list_depth == 0 {
                    top_list_ordered = start.is_some();
                }
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => {
                item_depth += 1;
                if list_depth == 1 && item_depth == 1 {
                    current = Some((Draft::default(), AnnotationSink::default()));
                    is_task_item = top_list_ordered;
                }
            }
            Event::End(TagEnd::Item) => {
                if list_depth == 1 && item_depth == 1 {
                    if let Some((mut draft, sink)) = current.take() {
                        draft.title = draft.title.trim().to_owned();
                        draft.body = draft.body.trim().to_owned();
                        draft.ann = sink.annotation;
                        if is_task_item && !draft.title.is_empty() {
                            drafts.push(draft);
                        }
                    }
                }
                item_depth = item_depth.saturating_sub(1);
            }
            Event::TaskListMarker(_) => {
                if list_depth == 1 && item_depth == 1 {
                    is_task_item = true;
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((draft, _)) = current.as_mut() {
                    if list_depth == 1 && item_depth == 1 {
                        draft.title.push_str(&t);
                    } else {
                        draft.body.push_str(&t);
                        draft.body.push(' ');
                    }
                    collect_text_hints(&t, &mut draft.hints);
                }
            }
            // A wrapped title must not run its words together.
            Event::SoftBreak | Event::HardBreak => {
                if let Some((draft, _)) = current.as_mut() {
                    if list_depth == 1 && item_depth == 1 {
                        draft.title.push(' ');
                    } else {
                        draft.body.push(' ');
                    }
                }
            }
            _ => {}
        }
    }
    // Every block is closed by the parser; the contract is that nothing fed
    // to the accumulator goes unreported.
    for comment in html.finish() {
        if upstroke_body(&comment.inner).is_some() {
            warnings.push("upstroke annotation outside any checklist item; ignored".to_owned());
        }
    }
    drafts
}
