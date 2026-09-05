//! Extended notes: `docs/internals/plan/markdown/annotation.md`

use std::ops::Range;

use crate::ir::{TaskKind, Tier};

pub(super) fn upstroke_body(inner: &str) -> Option<&str> {
    inner.trim().strip_prefix("upstroke:")
}

#[derive(Default)]
pub(super) struct HtmlAccumulator {
    buffer: String,
    start: usize,
}

impl HtmlAccumulator {
    pub(super) fn push(&mut self, text: &str, range: &Range<usize>) {
        if self.buffer.is_empty() {
            self.start = range.start;
        }
        self.buffer.push_str(text);
    }

    pub(super) fn take_comments(&mut self) -> Vec<(Range<usize>, String)> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let found: Vec<(Range<usize>, String)> = comments_in(&self.buffer)
            .into_iter()
            .map(|c| {
                (
                    self.start + c.span.start..self.start + c.span.end,
                    c.inner.to_owned(),
                )
            })
            .collect();
        match self.buffer.rfind("<!--") {
            Some(open) if !self.buffer[open..].contains("-->") => {
                self.start += open;
                self.buffer = self.buffer[open..].to_owned();
            }
            _ => {
                self.buffer.clear();
            }
        }
        found
    }
}

#[derive(Default)]
pub(super) struct AnnotationSink {
    pub(super) annotation: Option<Annotation>,
}

impl AnnotationSink {
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

#[derive(Default, Clone)]
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

fn parse_annotation(body: &str, ctx: &str, warnings: &mut Vec<String>) -> Annotation {
    let mut ann = Annotation::default();
    for token in body.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            warnings.push(format!(
                "malformed annotation attribute `{token}` in {ctx} (expected key=value); ignored"
            ));
            continue;
        };
        match key {
            "id" => ann.id = non_empty(value),
            "kind" => match TaskKind::parse(value) {
                Some(kind) => ann.kind = Some(kind),
                None => warnings.push(format!(
                    "unknown kind `{value}` in {ctx}; falling back to heuristics"
                )),
            },
            "depends" => ann.depends = Some(csv(value)),
            "tier" => match Tier::parse(value) {
                Some(tier) => ann.tier = Some(tier),
                None => warnings.push(format!("unknown tier `{value}` in {ctx}; ignored")),
            },
            "min" => match Tier::parse(value) {
                Some(tier) => ann.min = Some(tier),
                None => warnings.push(format!("unknown min tier `{value}` in {ctx}; ignored")),
            },
            "needs" => ann.needs = csv(value),
            "out" => ann.out = csv(value),
            "paths" => ann.paths = csv(value),
            _ => warnings.push(format!(
                "unknown annotation attribute `{key}` in {ctx}; ignored"
            )),
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

pub(super) struct HtmlComment<'a> {
    span: Range<usize>,
    pub(super) inner: &'a str,
}

pub(super) fn comments_in(html: &str) -> Vec<HtmlComment<'_>> {
    let mut comments = Vec::new();
    let mut pos = 0;
    while let Some(open_rel) = html[pos..].find("<!--") {
        let open = pos + open_rel;
        let inner_start = open + 4;
        let Some(close_rel) = html[inner_start..].find("-->") else {
            break;
        };
        let close = inner_start + close_rel;
        comments.push(HtmlComment {
            span: open..close + 3,
            inner: &html[inner_start..close],
        });
        pos = close + 3;
    }
    comments
}

pub(super) fn strip_spans(slice: &str, spans: &[Range<usize>]) -> String {
    let mut out = String::with_capacity(slice.len());
    let mut pos = 0;
    for span in spans {
        if span.start > pos {
            out.push_str(&slice[pos..span.start]);
        }
        pos = pos.max(span.end);
    }
    out.push_str(&slice[pos..]);
    out
}
