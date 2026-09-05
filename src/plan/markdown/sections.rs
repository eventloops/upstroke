//! Extended notes: `docs/internals/plan/markdown/sections.md`

use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use super::annotation::{comments_in, upstroke_body};
use super::md_options;

pub(super) struct Section {
    pub(super) title: String,
    pub(super) content: Range<usize>,
    pub(super) inline_annotation: Option<String>,
}

struct HeadingScan {
    title: String,
    span: Range<usize>,
    annotation: Option<String>,
}

pub(super) fn split_sections(raw: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut in_heading: Option<HeadingScan> = None;
    let mut container_depth = 0usize;

    for (event, range) in Parser::new_ext(raw, md_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_)) | Event::Start(Tag::Item) => container_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) | Event::End(TagEnd::Item) => {
                container_depth = container_depth.saturating_sub(1);
            }
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2 | HeadingLevel::H3,
                ..
            }) if container_depth == 0 => {
                in_heading = Some(HeadingScan {
                    title: String::new(),
                    span: range,
                    annotation: None,
                });
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2 | HeadingLevel::H3)) => {
                if let Some(scan) = in_heading.take() {
                    let title = scan.title.trim().to_owned();
                    if is_acceptance_header(strip_trailing_colon(&title)) {
                        continue;
                    }
                    if let Some(prev) = sections.last_mut() {
                        prev.content.end = scan.span.start;
                    }
                    sections.push(Section {
                        title,
                        content: scan.span.end..raw.len(),
                        inline_annotation: scan.annotation,
                    });
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(scan) = in_heading.as_mut() {
                    scan.title.push_str(&t);
                }
            }
            Event::InlineHtml(t) | Event::Html(t) => {
                if let Some(scan) = in_heading.as_mut() {
                    for comment in comments_in(&t) {
                        if let Some(body) = upstroke_body(comment.inner) {
                            if scan.annotation.is_none() {
                                scan.annotation = Some(body.to_owned());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    sections
}

pub(super) fn strip_trailing_colon(text: &str) -> &str {
    text.trim().trim_end_matches(':').trim_end()
}

pub(super) fn is_acceptance_header(text: &str) -> bool {
    ["acceptance", "done when", "success criteria"]
        .iter()
        .any(|h| text.eq_ignore_ascii_case(h))
}
