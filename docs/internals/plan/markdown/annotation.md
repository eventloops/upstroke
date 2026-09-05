# `src/plan/markdown/annotation.rs`

Extended notes for [`src/plan/markdown/annotation.rs`](../../../../src/plan/markdown/annotation.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The `<!-- upstroke: ... -->` grammar, and the machinery that finds it.

Two halves. The first extracts comments from pulldown-cmark HTML events —
annotations are read from the event stream, never regexed out of raw text,
and because the parser emits one event per line inside an HTML block a
multi-line comment is only whole once consecutive events are joined. The
second parses the `key=value` attributes of an upstroke comment into an
[`Annotation`]. Unknown keys and unparseable values warn and never error.

A source of the DAG, not a sink: nothing here reads a section, a draft or a
hint.

## `pub(super) fn upstroke_body(inner: &str) -> Option<&str> {`

The body of a `<!-- upstroke: ... -->` comment, or `None` for ordinary
author comments.

## `pub(super) struct HtmlAccumulator {`

Accumulates consecutive HTML events before scanning for annotations:
pulldown-cmark emits one event per line inside an HTML block, so a
multi-line `<!-- upstroke: ... -->` comment is only complete once its
neighbours are joined. Consecutive events are contiguous in the source, so
buffer offsets map linearly back to absolute spans.

## `impl HtmlAccumulator` › `pub(super) fn take_comments(&mut self) -> Vec<(Range<usize>, String)> {`

Complete comments in the buffer, as (absolute span, inner text).

## `pub(super) fn take_comments(&mut self) -> Vec<(Range<usize>…` › `match self.buffer.rfind("<!--") {`

Keep a trailing partial comment buffered for the next event.

## `pub(super) struct AnnotationSink {`

First-wins annotation intake shared by the section and checklist paths.

## `pub(super) struct Annotation {`

---------------------------------------------------------------------------
Annotation grammar
---------------------------------------------------------------------------

## `pub(super) struct Annotation` › `pub(super) depends: Option<Vec<String>>,`

`Some(vec![])` means `depends=` — explicitly no dependencies, breaking
the document-order default chain. `None` means the attribute is absent.

## `pub(super) struct HtmlComment<'a>` › `span: Range<usize>,`

Span within the HTML event text, including the delimiters.
