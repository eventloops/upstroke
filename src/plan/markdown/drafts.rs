//! Extended notes: `docs/internals/plan/markdown/drafts.md`

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::annotation::{
    Annotation, AnnotationSink, HtmlAccumulator, OPEN, strip_spans, upstroke_body,
};
use super::hints::{collect_code_hint, collect_text_hints};
use super::sections::{Section, is_acceptance_header, strip_trailing_colon};
use super::{md_options, parser_source};

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
    let mut annotation_spans: Vec<Range<usize>> = Vec::new();
    let mut html = HtmlAccumulator::default();

    let mut para_text = String::new();
    let mut in_para = false;
    let mut heading_text = String::new();
    let mut in_heading = false;
    let mut armed = false;
    let mut acceptance_list_depth = 0usize;
    let mut item_slots: Vec<usize> = Vec::new();

    let normalized = parser_source(slice);
    for (event, range) in Parser::new_ext(&normalized, md_options()).into_offset_iter() {
        for comment in html.observe(&event, &range, &normalized) {
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

fn open_criterion<'a>(
    item_slots: &[usize],
    acceptance: &'a mut [String],
) -> Option<&'a mut String> {
    acceptance.get_mut(*item_slots.last()?)
}

pub(super) fn checklist_drafts(raw: &str, warnings: &mut Vec<String>) -> Vec<Draft> {
    let mut drafts = Vec::new();
    let mut list_depth = 0usize;
    let mut item_depth = 0usize;
    let mut current: Option<(Draft, AnnotationSink)> = None;
    let mut is_task_item = false;
    let mut top_list_ordered = false;
    let mut html = HtmlAccumulator::default();

    let normalized = parser_source(raw);
    for (event, range) in Parser::new_ext(&normalized, md_options()).into_offset_iter() {
        for comment in html.observe(&event, &range, &normalized) {
            match current.as_mut() {
                Some((draft, sink)) => {
                    sink.take(&comment, "checklist item", warnings);
                    if !comment.terminated && upstroke_body(&comment.inner).is_some() {
                        match raw.get(comment.span.clone()) {
                            Some(original) => draft.body.push_str(original),
                            None => {
                                warnings.push(
                                    "internal error: an unterminated annotation span is outside the plan; \
                                     the checklist body keeps the parser's recovered text".to_owned(),
                                );
                                draft.body.push_str(OPEN);
                                draft.body.push_str(&comment.inner);
                            }
                        }
                        draft.body.push(' ');
                    }
                }
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
    for comment in html.finish() {
        if upstroke_body(&comment.inner).is_some() {
            warnings.push("upstroke annotation outside any checklist item; ignored".to_owned());
        }
    }
    drafts
}
