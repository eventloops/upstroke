//! Extended notes: `docs/internals/plan/markdown/annotation.md`

use std::ops::Range;

use pulldown_cmark::{Event, Tag, TagEnd};

use crate::ir::{TaskKind, Tier};

pub(super) const OPEN: &str = "<!--";
const CLOSE: &str = "-->";
const MARKER: &str = "upstroke:";

pub(super) fn upstroke_body(inner: &str) -> Option<&str> {
    inner.trim().strip_prefix(MARKER)
}

pub(super) struct HtmlComment<'a> {
    span: Range<usize>,
    pub(super) inner: &'a str,
}

struct CommentScan<'a> {
    comments: Vec<HtmlComment<'a>>,
    unterminated: Option<HtmlComment<'a>>,
}

fn scan_comments(html: &str) -> CommentScan<'_> {
    let mut comments = Vec::new();
    let mut rest = html;
    let mut consumed = 0;
    while let Some((before, after_open)) = rest.split_once(OPEN) {
        let open = consumed + before.len();
        let Some((inner, after_close)) = after_open.split_once(CLOSE) else {
            return CommentScan {
                comments,
                unterminated: Some(HtmlComment {
                    span: open..html.len(),
                    inner: after_open,
                }),
            };
        };
        let end = open + OPEN.len() + inner.len() + CLOSE.len();
        comments.push(HtmlComment {
            span: open..end,
            inner,
        });
        consumed = end;
        rest = after_close;
    }
    CommentScan {
        comments,
        unterminated: None,
    }
}

pub(super) fn comments_in(html: &str) -> Vec<HtmlComment<'_>> {
    scan_comments(html).comments
}

pub(super) fn has_unterminated_annotation(text: &str) -> bool {
    scan_comments(text)
        .unterminated
        .is_some_and(|comment| upstroke_body(comment.inner).is_some())
}

pub(super) struct FoundComment {
    pub(super) span: Range<usize>,
    pub(super) inner: String,
    pub(super) terminated: bool,
}

struct Piece {
    text_start: usize,
    source_start: usize,
}

#[derive(Default)]
pub(super) struct HtmlAccumulator {
    text: String,
    pieces: Vec<Piece>,
    in_block: bool,
    containers: Vec<Option<Container>>,
}

impl HtmlAccumulator {
    pub(super) fn observe(
        &mut self,
        event: &Event<'_>,
        range: &Range<usize>,
        source: &str,
    ) -> Vec<FoundComment> {
        match event {
            Event::Start(Tag::BlockQuote(_)) | Event::Start(Tag::Item) => {
                let quote = matches!(event, Event::Start(Tag::BlockQuote(_)));
                let container = Container::open(source, range.start, quote, &self.containers);
                self.containers.push(container);
                self.close()
            }
            Event::End(TagEnd::BlockQuote(_)) | Event::End(TagEnd::Item) => {
                self.containers
                    .truncate(self.containers.len().saturating_sub(1));
                self.close()
            }
            Event::Start(Tag::HtmlBlock) => {
                let pending = self.close();
                self.in_block = true;
                pending
            }
            Event::End(TagEnd::HtmlBlock) => {
                self.in_block = false;
                self.close()
            }
            Event::Html(text) => {
                self.push(text, range.start);
                Vec::new()
            }
            Event::InlineHtml(text) => {
                self.push(text, range.start);
                if self.in_block {
                    Vec::new()
                } else {
                    self.close()
                        .into_iter()
                        .map(|comment| FoundComment {
                            inner: unquote_inline_comment(comment.inner, &self.containers),
                            ..comment
                        })
                        .collect()
                }
            }
            _ if self.in_block => Vec::new(),
            _ => self.close(),
        }
    }

    pub(super) fn finish(mut self) -> Vec<FoundComment> {
        self.close()
    }

    fn push(&mut self, text: &str, source_start: usize) {
        if text.is_empty() {
            return;
        }
        self.pieces.push(Piece {
            text_start: self.text.len(),
            source_start,
        });
        self.text.push_str(text);
    }

    fn close(&mut self) -> Vec<FoundComment> {
        if self.text.is_empty() {
            return Vec::new();
        }
        let scan = scan_comments(&self.text);
        let mut found: Vec<FoundComment> = scan
            .comments
            .iter()
            .map(|c| FoundComment {
                span: self.source_span(&c.span),
                inner: c.inner.to_owned(),
                terminated: true,
            })
            .collect();
        if let Some(open) = scan.unterminated {
            found.push(FoundComment {
                span: self.source_span(&open.span),
                inner: open.inner.to_owned(),
                terminated: false,
            });
        }
        self.text.clear();
        self.pieces.clear();
        found
    }

    fn source_span(&self, text_span: &Range<usize>) -> Range<usize> {
        let start = self.source_offset(text_span.start);
        let end = text_span
            .end
            .checked_sub(1)
            .map_or(start, |last| self.source_offset(last) + 1);
        start..end
    }

    fn source_offset(&self, text_offset: usize) -> usize {
        self.pieces
            .iter()
            .rev()
            .find(|piece| piece.text_start <= text_offset)
            .map_or(text_offset, |piece| {
                piece.source_start + (text_offset - piece.text_start)
            })
    }
}

struct Container {
    kind: ContainerKind,
    line_start: usize,
    opening: PrefixPosition,
}

enum ContainerKind {
    Quote,
    Item(usize),
}

impl Container {
    fn open(source: &str, start: usize, quote: bool, parents: &[Option<Self>]) -> Option<Self> {
        let before = source.get(..start)?;
        let line_start = before.rfind(['\r', '\n']).map_or(0, |end| end + 1);
        let line = source.get(line_start..)?;
        let mut prefix = LinePrefix::new(line);
        if let Some(parent) = parents.last() {
            let parent = parent.as_ref()?;
            if parent.line_start == line_start {
                prefix = LinePrefix::at(line, parent.opening)?;
            } else {
                for parent in parents {
                    if !prefix.continue_container(&parent.as_ref()?.kind) {
                        return None;
                    }
                }
            }
        }
        let initial_column = prefix.position.column;
        prefix.spaces(3);
        let kind = if quote {
            if !prefix.marker('>') {
                return None;
            }
            prefix.spaces(1);
            ContainerKind::Quote
        } else {
            if !(prefix.marker('-') || prefix.marker('+') || prefix.marker('*')) {
                let mut digits = 0;
                while digits < 9 {
                    let Some(digit) = prefix.rest.chars().next().filter(char::is_ascii_digit)
                    else {
                        break;
                    };
                    prefix.marker(digit);
                    digits += 1;
                }
                if digits == 0 || !(prefix.marker('.') || prefix.marker(')')) {
                    return None;
                }
            }
            let after_marker = prefix;
            let padding = prefix.spaces(5);
            let default_padding = padding == 5 || prefix.at_end_of_line();
            let indent = if default_padding {
                prefix = after_marker;
                prefix.spaces(1);
                after_marker.position.column - initial_column + 1
            } else if padding == 0 {
                return None;
            } else {
                prefix.position.column - initial_column
            };
            ContainerKind::Item(indent)
        };
        Some(Self {
            kind,
            line_start,
            opening: prefix.position,
        })
    }
}

#[derive(Clone, Copy, Default)]
struct PrefixPosition {
    bytes: usize,
    column: usize,
    tab_remaining: usize,
}

#[derive(Clone, Copy)]
struct LinePrefix<'a> {
    rest: &'a str,
    position: PrefixPosition,
}

impl<'a> LinePrefix<'a> {
    fn new(line: &'a str) -> Self {
        Self {
            rest: line,
            position: PrefixPosition::default(),
        }
    }

    fn at(line: &'a str, position: PrefixPosition) -> Option<Self> {
        line.get(position.bytes..)
            .map(|rest| Self { rest, position })
    }

    fn spaces(&mut self, limit: usize) -> usize {
        let mut consumed = self.position.tab_remaining.min(limit);
        self.position.tab_remaining -= consumed;
        self.position.column += consumed;
        while consumed < limit {
            if let Some(rest) = self.rest.strip_prefix(' ') {
                self.rest = rest;
                self.position.bytes += 1;
                self.position.column += 1;
                consumed += 1;
            } else if let Some(rest) = self.rest.strip_prefix('\t') {
                self.rest = rest;
                self.position.bytes += 1;
                let width = 4 - self.position.column % 4;
                let taken = width.min(limit - consumed);
                self.position.column += taken;
                self.position.tab_remaining = width - taken;
                consumed += taken;
            } else {
                break;
            }
        }
        consumed
    }

    fn marker(&mut self, marker: char) -> bool {
        let Some(rest) = self.rest.strip_prefix(marker) else {
            return false;
        };
        self.rest = rest;
        self.position.bytes += marker.len_utf8();
        self.position.column += 1;
        true
    }

    fn at_end_of_line(&self) -> bool {
        self.rest.is_empty() || self.rest.starts_with(['\r', '\n'])
    }

    fn continue_container(&mut self, kind: &ContainerKind) -> bool {
        let saved = *self;
        let matched = match kind {
            ContainerKind::Quote => {
                self.spaces(3);
                if self.marker('>') {
                    self.spaces(1);
                    true
                } else {
                    false
                }
            }
            ContainerKind::Item(indent) => self.spaces(*indent) == *indent || self.at_end_of_line(),
        };
        if !matched {
            *self = saved;
        }
        matched
    }
}

fn unquote_inline_comment(inner: String, containers: &[Option<Container>]) -> String {
    if !containers.iter().any(|container| {
        matches!(
            container,
            Some(Container {
                kind: ContainerKind::Quote,
                ..
            })
        )
    }) || containers.iter().any(Option::is_none)
        || !inner.contains(['\r', '\n'])
    {
        return inner;
    }
    let mut normalized = String::with_capacity(inner.len());
    let mut lines = inner.split_inclusive(['\r', '\n']);
    if let Some(first) = lines.next() {
        normalized.push_str(first);
    }
    for line in lines {
        let mut prefix = LinePrefix::new(line);
        for container in containers.iter().flatten() {
            if !prefix.continue_container(&container.kind) {
                break;
            }
        }
        normalized.push_str(prefix.rest);
    }
    normalized
}

pub(super) fn strip_spans(slice: &str, spans: &[Range<usize>]) -> Option<String> {
    let mut out = String::with_capacity(slice.len());
    let mut pos = 0;
    for span in spans {
        if span.start > pos {
            out.push_str(slice.get(pos..span.start)?);
        }
        pos = pos.max(span.end);
    }
    out.push_str(slice.get(pos..)?);
    Some(out)
}

#[derive(Default)]
pub(super) struct AnnotationSink {
    pub(super) annotation: Option<Annotation>,
}

impl AnnotationSink {
    pub(super) fn take(
        &mut self,
        comment: &FoundComment,
        ctx: &str,
        warnings: &mut Vec<String>,
    ) -> Option<Range<usize>> {
        let body = upstroke_body(&comment.inner)?;
        if !comment.terminated {
            warnings.push(format!(
                "unterminated upstroke annotation in {ctx} (no `-->` before its HTML block ends); \
                 ignored, and the text it opened stays in the body"
            ));
            return None;
        }
        self.accept(body, ctx, warnings);
        Some(comment.span.clone())
    }

    pub(super) fn accept(&mut self, body: &str, ctx: &str, warnings: &mut Vec<String>) {
        if self.annotation.is_some() {
            warnings.push(format!(
                "multiple upstroke annotations in {ctx}; using the first"
            ));
            return;
        }
        self.annotation = Some(parse_annotation(body, ctx, warnings));
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Annotation {
    pub(super) id: Option<String>,
    pub(super) kind: Option<TaskKind>,
    pub(super) depends: Option<Vec<String>>,
    pub(super) tier: Option<Tier>,
    pub(super) min: Option<Tier>,
    pub(super) needs: Vec<String>,
    pub(super) out: Vec<String>,
    pub(super) paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Id,
    Kind,
    Depends,
    Tier,
    Min,
    Needs,
    Out,
    Paths,
}

impl Key {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "id" => Self::Id,
            "kind" => Self::Kind,
            "depends" => Self::Depends,
            "tier" => Self::Tier,
            "min" => Self::Min,
            "needs" => Self::Needs,
            "out" => Self::Out,
            "paths" => Self::Paths,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Kind => "kind",
            Self::Depends => "depends",
            Self::Tier => "tier",
            Self::Min => "min",
            Self::Needs => "needs",
            Self::Out => "out",
            Self::Paths => "paths",
        }
    }
}

fn parse_annotation(body: &str, ctx: &str, warnings: &mut Vec<String>) -> Annotation {
    let mut values: Vec<(Key, &str)> = Vec::new();
    for token in body.split_whitespace() {
        let Some((name, value)) = token.split_once('=') else {
            warnings.push(format!(
                "malformed annotation attribute `{token}` in {ctx} (expected key=value); ignored"
            ));
            continue;
        };
        let Some(key) = Key::parse(name) else {
            warnings.push(format!(
                "unknown annotation attribute `{name}` in {ctx}; ignored"
            ));
            continue;
        };
        match values.iter_mut().find(|(seen, _)| *seen == key) {
            Some(slot) => {
                warnings.push(format!(
                    "annotation attribute `{}` repeated in {ctx}; the last one applies",
                    key.name()
                ));
                slot.1 = value;
            }
            None => values.push((key, value)),
        }
    }
    let mut ann = Annotation::default();
    for (key, value) in values {
        match key {
            Key::Id => {
                ann.id = non_empty(value);
                if ann.id.is_none() {
                    warnings.push(format!(
                        "empty id in {ctx}; the id is derived from the title"
                    ));
                }
            }
            Key::Kind => {
                ann.kind = TaskKind::parse(value);
                if ann.kind.is_none() {
                    warnings.push(format!(
                        "unknown kind `{value}` in {ctx}; falling back to heuristics"
                    ));
                }
            }
            Key::Depends => ann.depends = Some(csv(value)),
            Key::Tier => {
                ann.tier = Tier::parse(value);
                if ann.tier.is_none() {
                    warnings.push(format!(
                        "unknown tier `{value}` in {ctx}; ignored, routing chooses the tier"
                    ));
                }
            }
            Key::Min => {
                ann.min = Tier::parse(value);
                if ann.min.is_none() {
                    warnings.push(format!(
                        "unknown min tier `{value}` in {ctx}; ignored, so no floor binds and \
                         the task may run at any tier"
                    ));
                }
            }
            Key::Needs => ann.needs = csv(value),
            Key::Out => ann.out = csv(value),
            Key::Paths => ann.paths = csv(value),
        }
    }
    ann
}

fn csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::super::MarkdownPlanAdapter;
    use super::*;
    use crate::ir::{Task, TaskId};
    use crate::plan::{Parsed, PlanAdapter};

    fn parse(raw: &str) -> Parsed {
        MarkdownPlanAdapter
            .parse_with_warnings(raw)
            .expect("plan should parse")
    }

    fn task(parsed: &Parsed, index: usize) -> &Task {
        parsed
            .plan
            .tasks
            .get(index)
            .expect("the plan has a task at that index")
    }

    fn warnings_mentioning<'a>(parsed: &'a Parsed, needle: &str) -> Vec<&'a str> {
        parsed
            .warnings
            .iter()
            .filter(|w| w.contains(needle))
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn the_scanner_finds_each_comment_once_and_reports_the_opener_left_open() {
        let scan = scan_comments("a <!-- one --> b <!-- two --> c <!-- three");
        let inner: Vec<&str> = scan.comments.iter().map(|c| c.inner).collect();
        assert_eq!(inner, [" one ", " two "]);
        let spans: Vec<Range<usize>> = scan.comments.iter().map(|c| c.span.clone()).collect();
        assert_eq!(spans, [2..14, 17..29]);
        let open = scan.unterminated.expect("the third opener has no closer");
        assert_eq!(open.span, 32..42);
        assert_eq!(open.inner, " three");
    }

    #[test]
    fn the_scanner_looks_for_the_closer_after_the_whole_opener() {
        let scan = scan_comments("<!-->");
        assert!(scan.comments.is_empty());
        assert!(scan.unterminated.is_some(), "`<!-->` is an open comment");
        let scan = scan_comments("<!--->");
        assert!(scan.comments.is_empty());
        assert!(scan.unterminated.is_some(), "`<!--->` is an open comment");
        let scan = scan_comments("<!---->");
        assert_eq!(scan.comments.len(), 1, "`<!---->` is an empty comment");
        assert!(scan.unterminated.is_none());
    }

    #[test]
    fn the_marker_is_exact_and_leading() {
        assert_eq!(upstroke_body(" upstroke: id=a "), Some(" id=a"));
        assert_eq!(upstroke_body("upstroke:id=a"), Some("id=a"));
        assert_eq!(upstroke_body(" Upstroke: id=a "), None);
        assert_eq!(upstroke_body(" upstroke handles this "), None);
        assert_eq!(upstroke_body(" see upstroke: id=a "), None);
    }

    #[test]
    fn strip_spans_cuts_sorted_spans_and_merges_overlaps() {
        let text = "ab<!-- x -->cd<!-- y -->ef";
        assert_eq!(
            strip_spans(text, &[2..12, 14..24]).as_deref(),
            Some("abcdef")
        );
        assert_eq!(
            strip_spans(text, &[2..12, 4..8, 14..24]).as_deref(),
            Some("abcdef"),
            "a span inside an earlier one cuts nothing twice and moves nothing back"
        );
        assert_eq!(strip_spans(text, &[]).as_deref(), Some(text));
    }

    #[test]
    fn strip_spans_refuses_a_span_off_a_character_boundary_or_past_the_end() {
        let text = "é<!-- x -->";
        let one = |span: Range<usize>| strip_spans(text, std::slice::from_ref(&span));
        assert_eq!(one(2..12).as_deref(), Some("é"));
        assert!(
            one(1..12).is_none(),
            "a span starting inside `é` is refused, not cut"
        );
        assert!(one(2..13).is_none(), "past the end");
    }

    #[test]
    fn a_multi_line_annotation_in_a_crlf_plan_leaves_nothing_in_the_body() {
        let raw = "## Design\r\n<!-- upstroke: id=api kind=design\r\n     depends= tier=frontier -->\r\nBody.\r\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("api"));
        assert_eq!(t.kind, TaskKind::Design);
        assert_eq!(t.suggested_tier, Some(Tier::Frontier));
        assert_eq!(
            t.body, "Body.",
            "the comment is cut at its own bytes, `\\r` and all"
        );
        assert!(
            parsed.warnings.is_empty(),
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn a_multi_line_annotation_inside_a_container_is_cut_at_its_own_bytes() {
        let raw = "## Task\n- item\n  <!-- upstroke: id=a\n     paths=src/api/**\n     out=résumé-->\nAfter.\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("a"));
        assert_eq!(t.path_hints, ["src/api/**"]);
        assert_eq!(t.artifacts_out.len(), 1);
        assert!(
            !t.body.contains("-->") && !t.body.contains("upstroke"),
            "body: {}",
            t.body
        );
        assert!(t.body.contains("- item"), "body: {}", t.body);
        assert!(t.body.contains("After."), "body: {}", t.body);
        assert!(
            parsed.warnings.is_empty(),
            "warnings: {:?}",
            parsed.warnings
        );

        let raw = "## Task\n> <!-- upstroke: id=q\n> kind=fix -->\n\nAfter.\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("q"));
        assert_eq!(t.kind, TaskKind::Fix);
        assert!(!t.body.contains("upstroke"), "body: {}", t.body);
        assert!(t.body.contains("After."), "body: {}", t.body);
    }

    #[test]
    fn a_tab_indented_continuation_line_still_joins_its_comment() {
        let raw = "- [ ] Design the widget\n  <!-- upstroke: id=widget\n\tkind=design -->\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("widget"));
        assert_eq!(t.kind, TaskKind::Design);
        assert!(
            parsed.warnings.is_empty(),
            "warnings: {:?}",
            parsed.warnings
        );

        let raw = "## Task\n- item\n  <!-- upstroke: id=widget\n\tkind=design -->\nAfter.\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("widget"));
        assert!(
            !t.body.contains("-->") && !t.body.contains("upstroke"),
            "body: {}",
            t.body
        );
        assert!(t.body.contains("After."), "body: {}", t.body);
    }

    #[test]
    fn an_annotation_spanning_lines_inside_a_paragraph_is_one_comment() {
        let raw = "## Task\nSome text <!-- upstroke: id=inline\n    kind=fix --> more.\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("inline"));
        assert_eq!(t.kind, TaskKind::Fix);
        assert_eq!(t.body, "Some text  more.");
        assert!(
            parsed.warnings.is_empty(),
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn an_unterminated_annotation_warns_and_applies_nothing() {
        let raw = "## Design it\n<!-- upstroke: id=a kind=fix\nBody.\n\n## Next\nmore\n";
        let parsed = parse(raw);
        assert_eq!(parsed.plan.tasks.len(), 1);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("design-it"), "nothing of `id=a` applied");
        assert_eq!(t.kind, TaskKind::Design, "nothing of `kind=fix` applied");
        assert!(
            t.body.contains("<!-- upstroke: id=a"),
            "the text stays as the author left it: {}",
            t.body
        );
        let found = warnings_mentioning(&parsed, "unterminated upstroke annotation");
        assert_eq!(found.len(), 1, "warnings: {:?}", parsed.warnings);
        assert!(found[0].contains("section `Design it`"), "{}", found[0]);
    }

    #[test]
    fn a_comment_its_block_left_open_is_not_completed_by_later_html() {
        let raw =
            "## Task\n- item\n  <!-- upstroke: id=a\nProse — with a dash.\n<!-- note -->\nAfter.\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("task"), "nothing of `id=a` applied");
        assert!(
            t.body.contains("Prose — with a dash."),
            "the prose between the two blocks survives: {}",
            t.body
        );
        assert!(t.body.contains("After."), "body: {}", t.body);
        assert_eq!(
            warnings_mentioning(&parsed, "unterminated upstroke annotation").len(),
            1,
            "warnings: {:?}",
            parsed.warnings
        );
        assert!(
            warnings_mentioning(&parsed, "malformed").is_empty(),
            "the author comment's words are not parsed as attributes: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn a_second_annotation_on_one_task_warns_and_the_first_wins() {
        let raw =
            "## Task\n<!-- upstroke: id=first -->\n<!-- upstroke: id=second kind=fix -->\nBody.\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("first"));
        assert_eq!(
            t.kind,
            TaskKind::Implement,
            "the second comment is dropped whole"
        );
        assert_eq!(t.body, "Body.", "both comments are cut from the body");
        assert_eq!(
            warnings_mentioning(&parsed, "multiple upstroke annotations in section `Task`").len(),
            1,
            "warnings: {:?}",
            parsed.warnings
        );

        let raw = "## Task <!-- upstroke: id=inline -->\n<!-- upstroke: id=body -->\n";
        let parsed = parse(raw);
        assert_eq!(task(&parsed, 0).id, TaskId::from("inline"));
        assert_eq!(
            warnings_mentioning(&parsed, "multiple upstroke annotations").len(),
            1
        );
    }

    #[test]
    fn an_unknown_min_tier_warns_that_no_floor_binds() {
        let raw = "## Task\n<!-- upstroke: min=galactic tier=mid -->\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.min_tier, None);
        assert_eq!(t.suggested_tier, Some(Tier::Mid));
        let found = warnings_mentioning(&parsed, "unknown min tier `galactic`");
        assert_eq!(found.len(), 1, "warnings: {:?}", parsed.warnings);
        assert!(
            found[0].contains("no floor binds"),
            "the warning says what the task gets: {}",
            found[0]
        );
    }

    #[test]
    fn an_empty_id_warns_and_the_id_is_derived() {
        let parsed = parse("## Fix the parser\n<!-- upstroke: id= kind=fix -->\n");
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("fix-the-parser"));
        assert_eq!(t.kind, TaskKind::Fix);
        assert_eq!(
            warnings_mentioning(&parsed, "empty id in section `Fix the parser`").len(),
            1,
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn a_repeated_attribute_warns_and_the_last_value_applies() {
        let parsed = parse("## Do it\n<!-- upstroke: id=a id=b kind=fix kind=wat -->\n");
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("b"));
        assert_eq!(
            t.kind,
            TaskKind::Implement,
            "`kind=wat` is the last value and it falls back to the heuristic"
        );
        assert_eq!(
            warnings_mentioning(&parsed, "attribute `id` repeated").len(),
            1,
            "warnings: {:?}",
            parsed.warnings
        );
        assert_eq!(
            warnings_mentioning(&parsed, "attribute `kind` repeated").len(),
            1
        );
        assert_eq!(warnings_mentioning(&parsed, "unknown kind `wat`").len(), 1);
    }

    #[test]
    fn a_later_valid_value_leaves_no_false_consequence_warning() {
        let parsed = parse(
            "## Fix bug\n<!-- upstroke: min=wat min=frontier id= id=actual kind=wat kind=fix tier=wat tier=mid -->\n",
        );
        let t = task(&parsed, 0);
        assert_eq!(t.min_tier, Some(Tier::Frontier));
        assert_eq!(t.id, TaskId::from("actual"));
        assert_eq!(t.kind, TaskKind::Fix);
        assert_eq!(t.suggested_tier, Some(Tier::Mid));
        for false_claim in [
            "no floor binds",
            "unknown min tier",
            "empty id",
            "unknown kind",
            "unknown tier",
        ] {
            assert!(
                warnings_mentioning(&parsed, false_claim).is_empty(),
                "`{false_claim}` is not true of what the task got: {:?}",
                parsed.warnings
            );
        }
        assert_eq!(
            warnings_mentioning(&parsed, "repeated").len(),
            4,
            "{:?}",
            parsed.warnings
        );
    }

    #[test]
    fn an_inline_annotation_spanning_quoted_lines_keeps_its_floor() {
        for raw in [
            "## Fix bug\n> Context <!-- upstroke: id=a\n>min=frontier --> more.\n",
            "## Fix bug\n> Context <!-- upstroke: id=a\n> min=frontier --> more.\n",
            "## Fix bug\n- > Context <!-- upstroke: id=a\n  > min=frontier --> more.\n",
            "## Fix bug\n- Context <!-- upstroke: id=a\n  min=frontier --> more.\n",
        ] {
            let parsed = parse(raw);
            let t = task(&parsed, 0);
            assert_eq!(t.id, TaskId::from("a"), "{raw:?}");
            assert_eq!(
                t.min_tier,
                Some(Tier::Frontier),
                "{raw:?}: {:?}",
                parsed.warnings
            );
            assert!(parsed.warnings.is_empty(), "{raw:?}: {:?}", parsed.warnings);
            assert!(!t.body.contains("upstroke"), "{raw:?}: {}", t.body);
            assert!(t.body.contains("more."), "{raw:?}: {}", t.body);
        }
    }

    #[test]
    fn a_value_that_begins_with_a_quote_marker_is_kept() {
        let raw =
            "## T\n> Context <!-- upstroke: id=a paths=>a/**,>b/**\n> out=>c needs=>d --> more.\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("a"));
        assert_eq!(t.path_hints, [">a/**", ">b/**"]);
        let out: Vec<String> = t.artifacts_out.iter().map(ToString::to_string).collect();
        let needs: Vec<String> = t.artifacts_in.iter().map(ToString::to_string).collect();
        assert_eq!(out, [">c"]);
        assert_eq!(needs, [">d"]);
        assert!(
            warnings_mentioning(&parsed, "unknown annotation attribute").is_empty(),
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn inline_and_block_annotations_keep_their_floor_in_each_container_and_line_ending() {
        for source in [
            "## Fix bug\nContext <!-- upstroke: id=a\nmin=frontier --> more.\n",
            "## Fix bug\n> Context <!-- upstroke: id=a\n>min=frontier --> more.\n",
            "## Fix bug\n> Context <!-- upstroke: id=a\n> min=frontier --> more.\n",
            "## Fix bug\n>> Context <!-- upstroke: id=a\n>>min=frontier --> more.\n",
            "## Fix bug\n> > Context <!-- upstroke: id=a\n> > min=frontier --> more.\n",
            "## Fix bug\n- > Context <!-- upstroke: id=a\n  > min=frontier --> more.\n",
            "## Fix bug\n> - Context <!-- upstroke: id=a\n>   min=frontier --> more.\n",
            "## Fix bug\n> Context <!-- upstroke: id=a\nmin=frontier --> more.\n",
            "## Fix bug\n<!-- upstroke: id=a\nmin=frontier -->\nmore.\n",
            "## Fix bug\n> <!-- upstroke: id=a\n>min=frontier -->\n\nmore.\n",
            "## Fix bug\n> > <!-- upstroke: id=a\n> > min=frontier -->\n\nmore.\n",
            "- [ ] Fix bug\n  > Context <!-- upstroke: id=a\n  > min=frontier --> more.\n",
            "- [ ] Fix bug\n  <!-- upstroke: id=a\n  min=frontier -->\n\n  more.\n",
            "- [ ] Fix bug\n  > <!-- upstroke: id=a\n  >min=frontier -->\n\n  more.\n",
        ] {
            for ending in ["\n", "\r\n", "\r"] {
                let raw = source.replace('\n', ending);
                let parsed = parse(&raw);
                let t = task(&parsed, 0);
                assert_eq!(t.id.as_str(), "a", "{raw:?}");
                assert_eq!(t.min_tier, Some(Tier::Frontier), "{raw:?}");
                assert!(parsed.warnings.is_empty(), "{raw:?}: {:?}", parsed.warnings);
                assert!(!t.body.contains("upstroke"), "{raw:?}: {}", t.body);
                assert!(
                    t.body.contains("more.") || t.title.contains("more."),
                    "{raw:?}: title={:?}, body={:?}",
                    t.title,
                    t.body
                );
            }
        }
    }

    #[test]
    fn quote_containers_leave_literal_quote_markers_in_attribute_names_and_values() {
        for source in [
            "## Fix bug\n<!-- upstroke: >id=wrong paths=>keep -->\n",
            "## Fix bug\nContext <!-- upstroke: >id=wrong paths=>keep --> more.\n",
            "## Fix bug\n<!-- upstroke:\n>id=wrong paths=>keep -->\n",
            "## Fix bug\n> Context <!-- upstroke: >id=wrong paths=>keep --> more.\n",
            "## Fix bug\n> Context <!-- upstroke:\n>     >id=wrong paths=>keep --> more.\n",
            "## Fix bug\n>> Context <!-- upstroke:\n>>     >id=wrong paths=>keep --> more.\n",
            "## Fix bug\n> <!-- upstroke:\n> >id=wrong paths=>keep -->\n",
            "## Fix bug\n> > <!-- upstroke:\n> > >id=wrong paths=>keep -->\n",
        ] {
            for ending in ["\n", "\r\n", "\r"] {
                let raw = source.replace('\n', ending);
                let parsed = parse(&raw);
                let t = task(&parsed, 0);
                assert_eq!(t.id.as_str(), "fix-bug", "{raw:?}");
                assert_eq!(t.path_hints, [">keep"], "{raw:?}");
                assert_eq!(
                    warnings_mentioning(&parsed, "unknown annotation attribute `>id`").len(),
                    1,
                    "{raw:?}: {:?}",
                    parsed.warnings
                );
            }
        }
    }

    #[test]
    fn a_new_quote_container_interrupts_an_unclosed_inline_comment() {
        use pulldown_cmark::Parser;

        for (raw, quote_count) in [
            (
                "## Fix bug\nContext <!-- upstroke:\n>id=wrong paths=>keep --> more.\n",
                1,
            ),
            (
                "## Fix bug\n> Context <!-- upstroke:\n> >id=wrong paths=>keep --> more.\n",
                2,
            ),
        ] {
            let events: Vec<_> = Parser::new_ext(raw, super::super::md_options()).collect();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, Event::Start(Tag::BlockQuote(_))))
                    .count(),
                quote_count,
                "{raw:?}: {events:?}"
            );
            assert!(
                !events.iter().any(|event| matches!(
                    event, Event::InlineHtml(text) | Event::Html(text) if text.contains("upstroke:")
                )),
                "the interrupted opener is text, not an HTML comment: {events:?}"
            );
            let parsed = parse(raw);
            assert_eq!(task(&parsed, 0).id.as_str(), "fix-bug");
            assert!(task(&parsed, 0).path_hints.is_empty());
        }
    }

    #[test]
    fn indented_lazy_quote_continuations_keep_literal_attribute_markers() {
        use pulldown_cmark::Parser;

        let mut failures = Vec::new();
        for (label, opener, continuation, literal) in [
            ("zero spaces", "> Context ", ">", false),
            ("one space", "> Context ", " >", false),
            ("two spaces", "> Context ", "  >", false),
            ("three spaces", "> Context ", "   >", false),
            ("four spaces", "> Context ", "    >", true),
            ("tab to column four", "> Context ", "\t>", false),
            ("space and tab", "> Context ", " \t>", false),
            ("two spaces and tab", "> Context ", "  \t>", false),
            ("three spaces and tab", "> Context ", "   \t>", true),
            ("nested quote marker", ">> Context ", ">    >", false),
            ("nested lazy literal", ">> Context ", ">     >", true),
            ("nested tab marker", ">> Context ", "> \t>", false),
            (
                "nested indented tab literal",
                ">> Context ",
                ">    \t>",
                true,
            ),
            ("list quote marker", "- item\n  > Context ", "    >", false),
            ("list lazy literal", "- item\n  > Context ", "      >", true),
            ("list tab marker", "- item\n  > Context ", "  \t>", false),
            (
                "nested list quote marker",
                "- item\n  - nested\n    > Context ",
                "    >",
                false,
            ),
            ("same-line list quote", "- > Context ", "  >", false),
            (
                "same-line nested containers",
                "- > - > Context ",
                "  >   >",
                false,
            ),
            (
                "wide ordered item",
                "123456789. > Context ",
                "           >",
                false,
            ),
            ("empty item quote", "-\n  > Context ", "  >", false),
            ("quote after a blank item", "-\n\n> Context ", ">", false),
        ] {
            let raw = format!(
                "## Fix bug\n{opener}<!-- upstroke:\n{continuation}id=wrong paths=>keep --> more.\n"
            );
            let plain = raw
                .replace("<!-- upstroke:", "upstroke:")
                .replace("-->", "");
            let plain_events: Vec<_> = Parser::new_ext(&plain, super::super::md_options())
                .into_offset_iter()
                .collect();
            let plain_text: String = plain_events
                .iter()
                .filter_map(|(event, _)| match event {
                    Event::Text(text) => Some(text.as_ref()),
                    _ => None,
                })
                .collect();
            let measured_literal = plain_text.contains(">id=wrong");
            if measured_literal != literal {
                failures.push(format!(
                    "{label}: control expected literal={literal}, measured literal={measured_literal}; events {plain_events:?}"
                ));
            }
            let events: Vec<_> = Parser::new_ext(&raw, super::super::md_options())
                .into_offset_iter()
                .collect();
            assert!(
                events.iter().any(|(event, _)| matches!(event, Event::InlineHtml(text) if text.contains("upstroke:"))),
                "{label} must remain an inline comment: {events:?}"
            );
            let parsed = parse(&raw);
            let t = task(&parsed, 0);
            let expected_id = if measured_literal { "fix-bug" } else { "wrong" };
            let expected_warnings = usize::from(measured_literal);
            if t.id.as_str() != expected_id
                || warnings_mentioning(&parsed, "unknown annotation attribute `>id`").len()
                    != expected_warnings
                || t.path_hints != [">keep"]
            {
                failures.push(format!(
                    "{label}: input {raw:?}; expected id {expected_id} and {expected_warnings} unknown-key warnings; task {t:?}; warnings {:?}; events {events:?}; control events {plain_events:?}",
                    parsed.warnings
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn an_unterminated_checklist_comment_keeps_its_original_line_endings_and_container_text() {
        for (source, body) in [
            (
                "- [ ] Deploy\n  <!-- upstroke: id=deploy\n  Do not deploy before é.\n",
                "<!-- upstroke: id=deploy\n  Do not deploy before é.",
            ),
            (
                "- [ ] Deploy\n  > <!-- upstroke: id=deploy\n  > Do not deploy before é.\n",
                "<!-- upstroke: id=deploy\n  > Do not deploy before é.",
            ),
        ] {
            for ending in ["\n", "\r\n", "\r"] {
                let raw = source.replace('\n', ending);
                let parsed = parse(&raw);
                let t = task(&parsed, 0);
                assert_eq!(t.title, "Deploy");
                assert_eq!(t.body, body.replace('\n', ending), "{raw:?}");
                assert_eq!(
                    warnings_mentioning(&parsed, "unterminated upstroke annotation").len(),
                    1,
                    "{raw:?}: {:?}",
                    parsed.warnings
                );
            }
        }
    }

    #[test]
    fn two_inline_heading_annotations_warn_and_the_first_wins() {
        let parsed = parse(
            "## Task <!-- upstroke: id=first --><!-- upstroke: id=second kind=fix -->\nBody.\n",
        );
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("first"));
        assert_eq!(t.kind, TaskKind::Implement);
        assert_eq!(
            warnings_mentioning(&parsed, "multiple upstroke annotations in section `Task`").len(),
            1,
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn a_checklist_item_keeps_the_text_an_unterminated_annotation_swallowed() {
        let parsed =
            parse("- [ ] Deploy\n  <!-- upstroke: id=deploy\n  Do not deploy before backup.\n");
        let t = task(&parsed, 0);
        assert!(
            t.body.contains("Do not deploy before backup."),
            "the safety instruction reaches the agent: {:?}",
            t.body
        );
        assert!(
            t.body.contains("<!-- upstroke: id=deploy"),
            "as written: {:?}",
            t.body
        );
        assert_eq!(
            warnings_mentioning(
                &parsed,
                "unterminated upstroke annotation in checklist item"
            )
            .len(),
            1,
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn a_checklist_annotation_outside_any_item_warns() {
        let raw = "# Plan\n\n<!-- upstroke: id=orphan -->\n\n- [ ] Design it\n- [ ] Build it\n";
        let parsed = parse(raw);
        assert_eq!(parsed.plan.tasks.len(), 2);
        assert_eq!(task(&parsed, 0).id, TaskId::from("design-it"));
        assert_eq!(
            warnings_mentioning(&parsed, "outside any checklist item").len(),
            1,
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn an_author_comment_is_left_alone_without_a_warning() {
        let raw =
            "## Task\n<!-- Upstroke: id=nope -->\n<!-- upstroke handles the rest -->\nBody.\n";
        let parsed = parse(raw);
        let t = task(&parsed, 0);
        assert_eq!(t.id, TaskId::from("task"));
        assert!(
            t.body.contains("<!-- Upstroke: id=nope -->"),
            "an author comment stays in the body: {}",
            t.body
        );
        assert!(
            parsed.warnings.is_empty(),
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn a_direct_walk_hands_every_comment_back_once() {
        use pulldown_cmark::Parser;

        let raw = "<!-- upstroke: id=a -->\n\nText <!-- b --> more\n\n<!-- open";
        let mut html = HtmlAccumulator::default();
        let mut found = Vec::new();
        for (event, range) in Parser::new_ext(raw, super::super::md_options()).into_offset_iter() {
            found.extend(html.observe(&event, &range, raw));
        }
        found.extend(html.finish());
        let seen: Vec<(Range<usize>, &str, bool)> = found
            .iter()
            .map(|c| (c.span.clone(), c.inner.as_str(), c.terminated))
            .collect();
        assert_eq!(
            seen,
            [
                (0..23, " upstroke: id=a ", true),
                (30..40, " b ", true),
                (47..56, " open", false),
            ]
        );

        let raw = "<!-- upstroke: id=a\r\nkind=fix -->\r\n";
        let mut html = HtmlAccumulator::default();
        let mut found = Vec::new();
        for (event, range) in Parser::new_ext(raw, super::super::md_options()).into_offset_iter() {
            found.extend(html.observe(&event, &range, raw));
        }
        found.extend(html.finish());
        let seen: Vec<(Range<usize>, &str)> = found
            .iter()
            .map(|c| (c.span.clone(), c.inner.as_str()))
            .collect();
        assert_eq!(seen, [(0..33, " upstroke: id=a\nkind=fix ")]);
        assert_eq!(raw.get(0..33), Some("<!-- upstroke: id=a\r\nkind=fix -->"));
    }
}
