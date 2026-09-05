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

use super::annotation::{comments_in, has_unterminated_annotation, upstroke_body};
use super::{md_options, parser_source};

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
    /// Consecutive, unescaped text events, separate from code and HTML.
    plain_text: String,
    unterminated_annotation: bool,
}

impl HeadingScan {
    fn finish_plain_text(&mut self) {
        self.unterminated_annotation |= has_unterminated_annotation(&self.plain_text);
        self.plain_text.clear();
    }
}

pub(super) fn split_sections(raw: &str, warnings: &mut Vec<String>) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut in_heading: Option<HeadingScan> = None;
    // Headings nested in blockquotes or list items are quoted material, not
    // plan structure — only container-free headings delimit tasks.
    let mut container_depth = 0usize;

    let normalized = parser_source(raw);
    for (event, range) in Parser::new_ext(&normalized, md_options()).into_offset_iter() {
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
                    plain_text: String::new(),
                    unterminated_annotation: false,
                });
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2 | HeadingLevel::H3)) => {
                if let Some(mut scan) = in_heading.take() {
                    scan.finish_plain_text();
                    let title = scan.title.trim().to_owned();
                    if scan.unterminated_annotation {
                        warnings.push(format!(
                            "unterminated upstroke annotation in heading `{title}` \
                             (no `-->` on the heading line); ignored, and the title is unchanged"
                        ));
                    }
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
            Event::Text(t) => {
                if let Some(scan) = in_heading.as_mut() {
                    scan.title.push_str(&t);
                    // Text event ranges omit an escaping backslash, so
                    // equality alone cannot distinguish `\<` from `<`.
                    let escaped = normalized.get(..range.start).is_some_and(|prefix| {
                        prefix
                            .bytes()
                            .rev()
                            .take_while(|byte| *byte == b'\\')
                            .count()
                            % 2
                            == 1
                    });
                    if !escaped && normalized.get(range) == Some(t.as_ref()) {
                        scan.plain_text.push_str(&t);
                    } else {
                        // An escape or entity produces literal text rather
                        // than an annotation opener in the source.
                        scan.finish_plain_text();
                    }
                }
            }
            Event::Code(t) => {
                if let Some(scan) = in_heading.as_mut() {
                    scan.finish_plain_text();
                    scan.title.push_str(&t);
                }
            }
            Event::InlineHtml(t) | Event::Html(t) => {
                if let Some(scan) = in_heading.as_mut() {
                    scan.finish_plain_text();
                    for comment in comments_in(&t) {
                        if let Some(body) = upstroke_body(comment.inner) {
                            scan.annotations.push(body.to_owned());
                        }
                    }
                }
            }
            _ => {
                if let Some(scan) = in_heading.as_mut() {
                    scan.finish_plain_text();
                }
            }
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
