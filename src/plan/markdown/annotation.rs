//! The `<!-- upstroke: ... -->` grammar, and the machinery that finds it.
//!
//! Two halves. The first finds HTML comments in pulldown-cmark's events — an
//! annotation is read from the event stream, never regexed out of raw text —
//! and the stream's shape, measured on pulldown-cmark 0.13.4, decides how a
//! comment is put back together. Inside an HTML block the parser emits one
//! `Html` event per source line, so a comment that spans lines is whole only
//! once the lines of its block are joined. Each event's text is the source at
//! the event's byte range, but consecutive ranges are not contiguous: the `\r`
//! of a CRLF line ending is skipped, a container's prefix (`> `, a list item's
//! indentation) is never emitted, and indentation a tab straddles comes back
//! as a zero-width `Text` event between two `Html` events of the same block.
//! Inline HTML arrives as one `InlineHtml` event per construct, however many
//! lines it spans, and an inline `<!--` with no `-->` is not HTML at all but
//! text. [`HtmlAccumulator`] joins the lines of one block, keeps the source
//! range of every piece, and maps a comment found in the joined text back to
//! the source through those pieces, so the span it reports is the comment's
//! own bytes whatever the block's line endings or container. It never joins
//! across a block boundary: a comment its block ended before closing is
//! reported unterminated, not completed by unrelated HTML further down.
//!
//! The second half parses the `key=value` attributes of an upstroke comment
//! into an [`Annotation`]. This is where a typo becomes a routing decision, so
//! every refusal says what the task gets instead:
//!
//! | comment or attribute | warning | what the task gets |
//! |---|---|---|
//! | `<!-- note -->`, `<!-- Upstroke: -->`, `<!-- upstroke handles it -->` | none: an author's comment | nothing; the comment stays in the body |
//! | `<!-- upstroke: ...` with no `-->` in its block | unterminated; ignored | nothing; the text it opened stays in the body |
//! | a second upstroke comment on one task | multiple; the first is used | the first comment, and every one is cut from the body |
//! | `token` with no `=` | malformed; ignored | nothing from that token |
//! | `wibble=x` | unknown attribute; ignored | nothing from that token |
//! | `id=` | empty id | an id derived from the title |
//! | `kind=wat` | unknown kind; heuristics | the title-keyword heuristic |
//! | `tier=wat` | unknown tier; no suggestion | routing's own choice |
//! | `min=wat` | unknown min tier; **no floor** | routing's own choice, below what the author required |
//! | `kind=fix kind=docs` | repeated; the last applies | the last value, parseable or not |
//!
//! Every warning lands in `Parsed::warnings`, which `upstroke validate` prints
//! under `warnings:` and a run copies into its report as `warning:` lines.
//! Nothing here errors: `DESIGN.md` §9 has unknown attributes warn and never
//! error, and this module takes the same posture for values it cannot parse.
//! For `min=` that posture removes a binding floor on a typo, which no later
//! stage restores; whether that should refuse instead is the owner's call and
//! is recorded as a finding rather than decided here.
//!
//! A source of the DAG, not a sink: nothing here reads a section, a draft or a
//! hint.

use std::ops::Range;

use pulldown_cmark::{Event, Tag, TagEnd};

use crate::ir::{TaskKind, Tier};

const OPEN: &str = "<!--";
const CLOSE: &str = "-->";
const MARKER: &str = "upstroke:";

/// The body of a `<!-- upstroke: ... -->` comment, or `None` for an author
/// comment. The marker is matched exactly and at the start of the trimmed
/// inner text: `<!--upstroke:id=x-->` is an annotation; `<!-- Upstroke: -->`
/// and `<!-- upstroke will handle this -->` are an author's.
pub(super) fn upstroke_body(inner: &str) -> Option<&str> {
    inner.trim().strip_prefix(MARKER)
}

// ---------------------------------------------------------------------------
// Finding comments in the event stream
// ---------------------------------------------------------------------------

/// A comment located in one piece of HTML text.
pub(super) struct HtmlComment<'a> {
    /// Span within the text, delimiters included.
    span: Range<usize>,
    /// The text between the delimiters.
    pub(super) inner: &'a str,
}

/// One scan of a piece of HTML: its comments in order, and the opener left
/// without a closer, which swallows everything after it (an unterminated
/// comment is how HTML and CommonMark read that text too).
struct CommentScan<'a> {
    comments: Vec<HtmlComment<'a>>,
    unterminated: Option<HtmlComment<'a>>,
}

/// The one comment grammar: `<!--`, then the nearest `-->`. Offsets come from
/// `split_once`, never from arithmetic on a searched index, so nothing here
/// can slice off a character boundary.
fn scan_comments(html: &str) -> CommentScan<'_> {
    let mut comments = Vec::new();
    let mut rest = html;
    // Bytes of `html` before `rest`.
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

/// The complete comments in one piece of HTML text, in order. Used on the
/// inline HTML of a heading line, where pulldown-cmark hands over each
/// construct whole.
pub(super) fn comments_in(html: &str) -> Vec<HtmlComment<'_>> {
    scan_comments(html).comments
}

/// A comment found in the event stream, placed in the source.
pub(super) struct FoundComment {
    /// Source bytes of the comment, delimiters included. For an unterminated
    /// comment, from its opener to the end of the HTML block that failed to
    /// close it.
    pub(super) span: Range<usize>,
    /// The text between the delimiters; for an unterminated comment, all of
    /// the text after the opener.
    pub(super) inner: String,
    pub(super) terminated: bool,
}

/// One HTML event's contribution to the joined text: where its bytes sit in
/// the text, and where the same bytes sit in the source.
struct Piece {
    text_start: usize,
    source_start: usize,
}

/// Joins the `Html` events of one HTML block, so a comment that spans lines
/// is scanned whole, and maps what it finds back to source bytes through the
/// range each event carried. Fed every event of a walk; hands back comments
/// when the construct holding them is complete — at the block's end, or at
/// once for inline HTML — and never joins two blocks, so a comment left open
/// when its block ended is reported unterminated rather than completed by the
/// next HTML the walk meets.
#[derive(Default)]
pub(super) struct HtmlAccumulator {
    text: String,
    pieces: Vec<Piece>,
    in_block: bool,
}

impl HtmlAccumulator {
    /// Feeds one event with its source range. Returns the comments that event
    /// completed, which is nothing for most events.
    pub(super) fn observe(&mut self, event: &Event<'_>, range: &Range<usize>) -> Vec<FoundComment> {
        match event {
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
                }
            }
            // Inside a block the only other event is the zero-width text
            // pulldown-cmark synthesises for indentation a tab straddled; it
            // carries no source bytes and does not end the block.
            _ if self.in_block => Vec::new(),
            // HTML the parser emitted outside any block (none is known) ends
            // at the next event that is not HTML.
            _ => self.close(),
        }
    }

    /// Ends the walk. pulldown-cmark closes every block it opens, so this
    /// normally finds nothing; it is the contract that everything fed is
    /// handed back by `observe` or here.
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

    /// The source bytes of a span of the joined text. The end is mapped through
    /// the span's last byte, so a span ending where a piece ends stops at that
    /// piece's last source byte and not after the gap that follows it.
    fn source_span(&self, text_span: &Range<usize>) -> Range<usize> {
        let start = self.source_offset(text_span.start);
        let end = text_span
            .end
            .checked_sub(1)
            .map_or(start, |last| self.source_offset(last) + 1);
        start..end
    }

    /// The source byte a byte of the joined text came from. The pieces tile the
    /// text from offset 0 in order, so the last piece starting at or before the
    /// offset holds it; with no pieces the text is empty and the map is the
    /// identity.
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

/// `slice` with `spans` cut out. The spans are byte ranges of `slice`, in
/// ascending order of start; overlapping spans are merged. `None` when a span
/// does not lie within `slice` on character boundaries: the caller keeps the
/// slice whole and says so, since a body cut at the wrong bytes would hand the
/// agent prose with a hole in it.
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

// ---------------------------------------------------------------------------
// Annotation intake
// ---------------------------------------------------------------------------

/// First-wins annotation intake shared by the section and checklist paths.
#[derive(Default)]
pub(super) struct AnnotationSink {
    pub(super) annotation: Option<Annotation>,
}

impl AnnotationSink {
    /// Takes one comment the walk found. An author comment is not this
    /// module's and is left alone; an unterminated upstroke comment warns and
    /// applies nothing; a terminated one is parsed by [`Self::accept`].
    /// Returns the span to cut from the task body: every terminated upstroke
    /// comment, used or not, since it is machine text and never prose. An
    /// unterminated one stays in the body as the author left it.
    pub(super) fn take(
        &mut self,
        comment: &FoundComment,
        ctx: &str,
        warnings: &mut Vec<String>,
    ) -> Option<Range<usize>> {
        // Absence here is "not ours", not a failure: nothing to cut.
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

    /// Parses the body of an upstroke comment (the text after `upstroke:`)
    /// into this sink. The first comment wins; a later one warns and is
    /// dropped whole.
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

// ---------------------------------------------------------------------------
// Annotation grammar
// ---------------------------------------------------------------------------

/// What an upstroke comment says about its task. Every field is absent until
/// the comment sets it, and `assemble` fills what is absent from the
/// heuristics: a slug for the id, the title keywords for the kind, document
/// order for the dependencies.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Annotation {
    /// `id=`: reserved before any slug is derived. An empty value is absent.
    pub(super) id: Option<String>,
    /// `kind=`: overrides the title-keyword heuristic.
    pub(super) kind: Option<TaskKind>,
    /// `depends=`: `Some(vec![])` means `depends=` — explicitly no
    /// dependencies, breaking the document-order default chain. `None` means
    /// the attribute is absent.
    pub(super) depends: Option<Vec<String>>,
    /// `tier=`: the designer's suggestion; routing may choose otherwise.
    pub(super) tier: Option<Tier>,
    /// `min=`: the binding floor. `route` never runs the task below it.
    pub(super) min: Option<Tier>,
    /// `needs=`: artifacts consumed, comma-separated.
    pub(super) needs: Vec<String>,
    /// `out=`: artifacts produced, comma-separated.
    pub(super) out: Vec<String>,
    /// `paths=`: globs, placed ahead of the hints harvested from the prose.
    pub(super) paths: Vec<String>,
}

/// Whitespace-separated `key=value` tokens. The module doc's table is the
/// contract; each refusal below says what the task gets instead.
fn parse_annotation(body: &str, ctx: &str, warnings: &mut Vec<String>) -> Annotation {
    let mut ann = Annotation::default();
    let mut seen: Vec<&str> = Vec::new();
    for token in body.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            warnings.push(format!(
                "malformed annotation attribute `{token}` in {ctx} (expected key=value); ignored"
            ));
            continue;
        };
        let mut known = true;
        match key {
            "id" => {
                ann.id = non_empty(value);
                if ann.id.is_none() {
                    warnings.push(format!(
                        "empty id in {ctx}; the id is derived from the title"
                    ));
                }
            }
            "kind" => {
                ann.kind = TaskKind::parse(value);
                if ann.kind.is_none() {
                    warnings.push(format!(
                        "unknown kind `{value}` in {ctx}; falling back to heuristics"
                    ));
                }
            }
            "depends" => ann.depends = Some(csv(value)),
            "tier" => {
                ann.tier = Tier::parse(value);
                if ann.tier.is_none() {
                    warnings.push(format!(
                        "unknown tier `{value}` in {ctx}; ignored, routing chooses the tier"
                    ));
                }
            }
            "min" => {
                ann.min = Tier::parse(value);
                if ann.min.is_none() {
                    warnings.push(format!(
                        "unknown min tier `{value}` in {ctx}; ignored, so no floor binds and \
                         the task may run at any tier"
                    ));
                }
            }
            "needs" => ann.needs = csv(value),
            "out" => ann.out = csv(value),
            "paths" => ann.paths = csv(value),
            _ => {
                known = false;
                warnings.push(format!(
                    "unknown annotation attribute `{key}` in {ctx}; ignored"
                ));
            }
        }
        if known {
            if seen.contains(&key) {
                warnings.push(format!(
                    "annotation attribute `{key}` repeated in {ctx}; the last one applies"
                ));
            } else {
                seen.push(key);
            }
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

    // --- the comment scanner -------------------------------------------------

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
        // `<!-->` and `<!--->` are openers whose hyphens must not be read as
        // the start of their own closer.
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

    // --- spans and stripping ---------------------------------------------------

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

    // --- reassembly through the parser ------------------------------------------

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
        // A list item's continuation lines are emitted without the item's
        // indentation, so the joined text is shorter than the source; the
        // last line ends with a two-byte character right before the closer,
        // which puts a span computed from joined-text offsets inside it.
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

        // The same across a blockquote's `> ` prefix.
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
        // A tab that straddles the item's indentation makes pulldown-cmark
        // emit a zero-width text event between the block's two lines; the
        // block is one construct and the comment is whole.
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

    // --- the refusals, each with what the task gets ------------------------------

    #[test]
    fn an_unterminated_annotation_warns_and_applies_nothing() {
        // With no `-->` the HTML block runs to the end of the document, the
        // next heading included; CommonMark reads it so and so does this.
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
        // The list item's HTML block ends when the item does, at the
        // unindented prose; the author comment two lines down is a different
        // block and must not supply the closer.
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

        // The heading's inline annotation is the first one.
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
            found.extend(html.observe(&event, &range));
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

        // A CRLF block: the `\r` before each `\n` is in no event, and the
        // span ends after `>` and not after the gap that follows it.
        let raw = "<!-- upstroke: id=a\r\nkind=fix -->\r\n";
        let mut html = HtmlAccumulator::default();
        let mut found = Vec::new();
        for (event, range) in Parser::new_ext(raw, super::super::md_options()).into_offset_iter() {
            found.extend(html.observe(&event, &range));
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
