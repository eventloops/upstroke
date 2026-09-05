//! Where a task begins: the `##`/`###` headings that delimit sections.
//!
//! Only container-free headings are boundaries — one nested in a blockquote or
//! a list item is quoted material, not plan structure — and an `Acceptance` /
//! `Done when` / `Success criteria` heading labels the section above rather
//! than opening a new one, so the recognizer for those lives here too and
//! [`super::drafts`] reuses it when it arms criterion collection.
//!
//! Upstream of `drafts`; reads `<!-- upstroke: ... -->` comments off a heading
//! line through [`super::annotation`] and parses with [`super::md_options`].

use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use super::annotation::{comments_in, upstroke_body};
use super::md_options;

pub(super) struct Section {
    pub(super) title: String,
    /// Byte range of the section body in the original text: from the end of
    /// the heading block to the start of the next `##`/`###` heading.
    pub(super) content: Range<usize>,
    /// Every upstroke annotation written inline on the heading line itself,
    /// in order; the sink takes the first and warns for the rest.
    pub(super) inline_annotations: Vec<String>,
}

/// Heading state while scanning: accumulated title text, the heading block
/// span, and the inline annotations found on the heading line.
struct HeadingScan {
    title: String,
    span: Range<usize>,
    annotations: Vec<String>,
}

pub(super) fn split_sections(raw: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut in_heading: Option<HeadingScan> = None;
    // Headings nested in blockquotes or list items are quoted material, not
    // plan structure — only container-free headings delimit tasks.
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
                    annotations: Vec::new(),
                });
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2 | HeadingLevel::H3)) => {
                if let Some(scan) = in_heading.take() {
                    let title = scan.title.trim().to_owned();
                    // `### Acceptance` and friends label the criteria of the
                    // section above, so they are not task boundaries; the
                    // section body flows through and section_draft arms on it.
                    if is_acceptance_header(strip_trailing_colon(&title)) {
                        continue;
                    }
                    if let Some(prev) = sections.last_mut() {
                        prev.content.end = scan.span.start;
                    }
                    sections.push(Section {
                        title,
                        content: scan.span.end..raw.len(),
                        inline_annotations: scan.annotations,
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
                            scan.annotations.push(body.to_owned());
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
