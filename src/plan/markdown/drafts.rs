//! Extended notes: `docs/internals/plan/markdown/drafts.md`

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::annotation::{Annotation, AnnotationSink, HtmlAccumulator, strip_spans, upstroke_body};
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
    let slice = &raw[section.content.clone()];
    let ctx = format!("section `{}`", section.title);
    let mut draft = Draft {
        title: section.title.clone(),
        ..Draft::default()
    };
    let mut sink = AnnotationSink::default();
    if let Some(inline) = &section.inline_annotation {
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

    for (event, range) in Parser::new_ext(slice, md_options()).into_offset_iter() {
        if let Event::Html(t) | Event::InlineHtml(t) = &event {
            html.push(t, &range);
        } else {
            for (span, inner) in html.take_comments() {
                if let Some(body) = upstroke_body(&inner) {
                    sink.accept(body, &ctx, warnings);
                    annotation_spans.push(span);
                }
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
                if let Some(slot) = item_slots.last() {
                    draft.acceptance[*slot].push_str(&t);
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
                if let Some(slot) = item_slots.last() {
                    draft.acceptance[*slot].push_str(&t);
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
                if let Some(slot) = item_slots.last() {
                    draft.acceptance[*slot].push(' ');
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
    for (span, inner) in html.take_comments() {
        if let Some(body) = upstroke_body(&inner) {
            sink.accept(body, &ctx, warnings);
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
    draft.body = strip_spans(slice, &annotation_spans).trim().to_owned();
    draft
}

pub(super) fn checklist_drafts(raw: &str, warnings: &mut Vec<String>) -> Vec<Draft> {
    let mut drafts = Vec::new();
    let mut list_depth = 0usize;
    let mut item_depth = 0usize;
    let mut current: Option<(Draft, AnnotationSink)> = None;
    let mut is_task_item = false;
    let mut top_list_ordered = false;
    let mut html = HtmlAccumulator::default();

    for (event, range) in Parser::new_ext(raw, md_options()).into_offset_iter() {
        if let Event::Html(t) | Event::InlineHtml(t) = &event {
            html.push(t, &range);
        } else if let Some((_, sink)) = current.as_mut() {
            for (_, inner) in html.take_comments() {
                if let Some(body) = upstroke_body(&inner) {
                    sink.accept(body, "checklist item", warnings);
                }
            }
        } else {
            let _ = html.take_comments();
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
    drafts
}
