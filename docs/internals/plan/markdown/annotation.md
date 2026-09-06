# `src/plan/markdown/annotation.rs`

Extended notes for [`src/plan/markdown/annotation.rs`](../../../../src/plan/markdown/annotation.rs).

These notes preserve the module comments after the annotation repairs. Item headings quote source lines for navigation.

## Module

The `<!-- upstroke: ... -->` grammar, and the machinery that finds it.

Two halves. The first finds HTML comments in pulldown-cmark's events — an
annotation is read from the event stream, never regexed out of raw text —
and the stream's shape, measured on pulldown-cmark 0.13.4, decides how a
comment is put back together. Inside an HTML block the parser emits one
`Html` event per source line, so a comment that spans lines is whole only
once the lines of its block are joined. Each event's text is the source at
the event's byte range, but consecutive ranges are not contiguous: the `\r`
of a CRLF line ending is skipped, a container's prefix (`> `, a list item's
indentation) is never emitted, and indentation a tab straddles comes back
as a zero-width `Text` event between two `Html` events of the same block.
The adapter normalizes lone CR to LF for every parser walk, preserving
byte offsets into the original text. This is needed because 0.13.4's
HTML-block scanner advances only at LF. Inline comments arrive as one
`InlineHtml` event per construct, however many lines they span. That
event retains the parser input, so inside a
blockquote its continuation lines still carry the quote's `>` markers
(measured: `<!-- upstroke: id=a\n>min=frontier -->`). The accumulator
removes only container prefixes that each continuation line actually
carries, accounting for list indentation and partially consumed tabs.
Its first line and any extra or lazy-continuation `>` remain literal content;
HTML-block events have already shed their prefixes and are left alone.
An inline `<!--` with no `-->` is not
HTML at all but text. [`HtmlAccumulator`] joins the lines of one block, keeps the source
range of every piece, and maps a comment found in the joined text back to
the source through those pieces, so the span it reports is the comment's
own bytes whatever the block's line endings or container. It never joins
across a block boundary: a comment its block ended before closing is
reported unterminated, not completed by unrelated HTML further down.

The second half parses the `key=value` attributes of an upstroke comment
into an [`Annotation`]. This is where a typo becomes a routing decision, so
every refusal says what the task gets instead:

| comment or attribute | warning | what the task gets |
|---|---|---|
| `<!-- note -->`, `<!-- Upstroke: -->`, `<!-- upstroke handles it -->` | none: an author's comment | nothing; in a section body it stays as written, but `checklist_drafts` (`drafts.rs`) only copies `Text`/`Code` events into a title or body, so the same comment vanishes silently from a checklist task |
| `<!-- upstroke: ...` with no `-->` in its block | unterminated; ignored | nothing; the text it opened stays in the body, section or checklist alike |
| a second upstroke comment on one task | multiple; the first is used | the first comment, and every one is cut from the body |
| `token` with no `=` | malformed; ignored | nothing from that token |
| `wibble=x` | unknown attribute; ignored | nothing from that token |
| `id=` | empty id | an id derived from the title |
| `kind=wat` | unknown kind; heuristics | the title-keyword heuristic |
| `tier=wat` | unknown tier; no suggestion | routing's own choice |
| `min=wat` | unknown min tier; **no floor** | routing's own choice; no valid floor was supplied |
| `kind=fix kind=docs` | repeated; the last applies | the last value, parseable or not; the earlier one is not parsed |

A value's warning is about the value that applies: `min=wat min=frontier`
warns that `min` is repeated and binds `frontier`, and never says no floor
binds. A successful parse returns warnings in `Parsed::warnings`.
The adapter's no-tasks refusal and validation's configuration, graph and
review-planning refusals retain warnings already gathered in a typed
`UpstrokeError::WithWarnings`. A successful `upstroke validate` prints
them under `warnings:`; a run that completes preflight copies them into
its report as `warning:` lines. Later execution-preflight refusals do not
promise a report. `sections` also warns when an unescaped heading text
run contains an unclosed upstroke comment, leaving the extracted title unchanged.
Nothing in the attribute grammar errors: `DESIGN.md` §9 has unknown attributes warn and never
error, and this module takes the same posture for values it cannot parse.
An unparseable `min=` supplies no valid binding floor. Adopting a refusal
policy instead would require a separate change to that design contract.

A source of the DAG, not a sink: nothing here reads a section, a draft or a
hint.

## `pub(super) fn upstroke_body(inner: &str) -> Option<&str> {`

The body of a `<!-- upstroke: ... -->` comment, or `None` for an author
comment. The marker is matched exactly and at the start of the trimmed
inner text: `<!--upstroke:id=x-->` is an annotation; `<!-- Upstroke: -->`
and `<!-- upstroke will handle this -->` are an author's.

## `pub(super) struct HtmlComment<'a> {`

---------------------------------------------------------------------------
Finding comments in the event stream
---------------------------------------------------------------------------
A comment located in one piece of HTML text.

## `span: Range<usize>,`

Span within the text, delimiters included.

## `pub(super) inner: &'a str,`

The text between the delimiters.

## `struct CommentScan<'a> {`

One scan of a piece of HTML: its comments in order, and the opener left
without a closer, which swallows everything after it (an unterminated
comment is how HTML and CommonMark read that text too).

## `fn scan_comments(html: &str) -> CommentScan<'_> {`

The one comment grammar: `<!--`, then the nearest `-->`. Offsets come from
`split_once`, never from arithmetic on a searched index, so nothing here
can slice off a character boundary.

## `let mut consumed = 0;`

Bytes of `html` before `rest`.

## `pub(super) fn comments_in(html: &str) -> Vec<HtmlComment<'_>> {`

The complete comments in one piece of HTML text, in order. Used on the
inline HTML of a heading line, where pulldown-cmark hands over each
construct whole.

## `pub(super) fn has_unterminated_annotation(text: &str) -> bool {`

A literal text run can contain an opener that the Markdown parser could
not turn into inline HTML. Reuse the comment scanner so an author's
unfinished non-upstroke comment does not expose a nested marker as ours.

## `pub(super) struct FoundComment {`

A comment found in the event stream, placed in the source.

## `pub(super) span: Range<usize>,`

Source bytes of the comment, delimiters included. For an unterminated
comment, from its opener to the end of the HTML block that failed to
close it.

## `pub(super) inner: String,`

The text between the delimiters; for an unterminated comment, all of
the text after the opener.

## `struct Piece {`

One HTML event's contribution to the joined text: where its bytes sit in
the text, and where the same bytes sit in the source.

## `#[derive(Default)]`

Joins the `Html` events of one HTML block, so a comment that spans lines
is scanned whole, and maps what it finds back to source bytes through the
range each event carried. Fed every event of a walk; hands back comments
when the construct holding them is complete — at the block's end, or at
once for inline HTML — and never joins two blocks, so a comment left open
when its block ended is reported unterminated rather than completed by the
next HTML the walk meets.

## `containers: Vec<Option<Container>>,`

Active containers and their measured opening prefixes. An unavailable
prefix prevents normalization rather than guessing at author text.

## `pub(super) fn observe(`

Feeds one event with its source range. Returns the comments that event
completed, which is nothing for most events.

## `_ if self.in_block => Vec::new(),`

Inside a block the only other event is the zero-width text
pulldown-cmark synthesises for indentation a tab straddled; it
carries no source bytes and does not end the block.

## `_ => self.close(),`

HTML the parser emitted outside any block (none is known) ends
at the next event that is not HTML.

## `pub(super) fn finish(mut self) -> Vec<FoundComment> {`

Ends the walk. pulldown-cmark closes every block it opens, so this
normally finds nothing; it is the contract that everything fed is
handed back by `observe` or here.

## `fn source_span(&self, text_span: &Range<usize>) -> Range<usize> {`

The source bytes of a span of the joined text. The end is mapped through
the span's last byte, so a span ending where a piece ends stops at that
piece's last source byte and not after the gap that follows it.

## `fn source_offset(&self, text_offset: usize) -> usize {`

The source byte a byte of the joined text came from. The pieces tile the
text from offset 0 in order, so the last piece starting at or before the
offset holds it; with no pieces the text is empty and the map is the
identity.

## `struct Container {`

A parser-reported container. The opening position lets another container
on the same line begin after its parent's actual marker, while later lines
use the parent's continuation rule. Positions copy only byte/column counts.

## `fn open(source: &str, start: usize, quote: bool, parents: &[Option<Self>]) -> Option<Self> {`

Checked ranges and prefix recognition can be absent only if the supplied
event does not describe this source. Propagate that absence to disable
normalization; a guessed prefix could turn an unknown key into a binding.

## `let after_marker = prefix;`

The parser gives list markers one to four padding columns;
five or more, or an empty item, uses the one-column default.

## `#[derive(Clone, Copy, Default)]`

A small, borrowed cursor over a line's container prefix. Copying it is a
rollback of scalar offsets and a borrowed suffix, not shared ownership.

## `if self.marker('>') {`

Match pulldown-cmark 0.13.4: the marker may follow a
partially consumed tab; its optional space consumes the
remaining virtual column before any following source byte.

## `fn unquote_inline_comment(inner: String, containers: &[Option<Container>]) -> String {`

Inline comments retain raw continuation prefixes in pulldown-cmark 0.13.4.
Consume only the active containers that each line actually continues.
A missing prefix ends that scan, leaving an indented literal `>` intact.
The opener's line has no prefix; block HTML has already shed its prefixes.

## `let mut lines = inner.split_inclusive(['\r', '\n']);`

Splitting inclusively preserves each original line ending. A CRLF has
an empty-content LF piece, which has no quote marker to remove.

## `pub(super) fn strip_spans(slice: &str, spans: &[Range<usize>]) -> Option<String> {`

`slice` with `spans` cut out. The spans are byte ranges of `slice`, in
ascending order of start; overlapping spans are merged. `None` when a span
does not lie within `slice` on character boundaries: the caller keeps the
slice whole and says so, since a body cut at the wrong bytes would hand the
agent prose with a hole in it.

## `#[derive(Default)]`

---------------------------------------------------------------------------
Annotation intake
---------------------------------------------------------------------------
First-wins annotation intake shared by the section and checklist paths.

## `pub(super) fn take(`

Takes one comment the walk found. An author comment is not this
module's and is left alone; an unterminated upstroke comment warns and
applies nothing; a terminated one is parsed by [`Self::accept`].
Returns the span to cut from the task body: every terminated upstroke
comment, used or not, since it is machine text and never prose. An
unterminated one stays in the body as the author left it.

## `let body = upstroke_body(&comment.inner)?;`

Absence here is "not ours", not a failure: nothing to cut.

## `pub(super) fn accept(&mut self, body: &str, ctx: &str, warnings: &mut Vec<String>) {`

Parses the body of an upstroke comment (the text after `upstroke:`)
into this sink. The first comment wins; a later one warns and is
dropped whole.

## `#[derive(Debug, Default, Clone, PartialEq, Eq)]`

---------------------------------------------------------------------------
Annotation grammar
---------------------------------------------------------------------------
What an upstroke comment says about its task. Every field is absent until
the comment sets it, and `assemble` fills what is absent from the
heuristics: a slug for the id, the title keywords for the kind, document
order for the dependencies.
Clone supports the unchanged `Draft::annotation` callers in `assemble`;
removing those copies belongs to their separate ownership cleanup.

## `pub(super) id: Option<String>,`

`id=`: reserved before any slug is derived. An empty value is absent.

## `pub(super) kind: Option<TaskKind>,`

`kind=`: overrides the title-keyword heuristic.

## `pub(super) depends: Option<Vec<String>>,`

`depends=`: `Some(vec![])` means `depends=` — explicitly no
dependencies, breaking the document-order default chain. `None` means
the attribute is absent.

## `pub(super) tier: Option<Tier>,`

`tier=`: the designer's suggestion; routing may choose otherwise.

## `pub(super) min: Option<Tier>,`

`min=`: the binding floor. `route` never runs the task below it.

## `pub(super) needs: Vec<String>,`

`needs=`: artifacts consumed, comma-separated.

## `pub(super) out: Vec<String>,`

`out=`: artifacts produced, comma-separated.

## `pub(super) paths: Vec<String>,`

`paths=`: globs, placed ahead of the hints harvested from the prose.

## `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`

The attribute names the grammar knows, one table for parsing and naming.

## `fn parse_annotation(body: &str, ctx: &str, warnings: &mut Vec<String>) -> Annotation {`

Whitespace-separated `key=value` tokens. The module doc's table is the
contract; each refusal below says what the task gets instead. Tokens are
gathered first and the last value of each attribute is parsed afterwards,
so a warning about a value is always about the value that applies.

## `let mut values: Vec<(Key, &str)> = Vec::new();`

The last value of each attribute, in first-seen order.

## `#[test]`

--- the comment scanner -------------------------------------------------

## `let scan = scan_comments("<!-->");`

`<!-->` and `<!--->` are openers whose hyphens must not be read as
the start of their own closer.

## `#[test]`

--- spans and stripping ---------------------------------------------------

## `#[test]`

--- reassembly through the parser ------------------------------------------

## `let raw = "## Task\n- item\n  <!-- upstroke: id=a\n     paths=src/api/**\n     out=résumé-->\nAfter.\n";`

A list item's continuation lines are emitted without the item's
indentation, so the joined text is shorter than the source; the
last line ends with a two-byte character right before the closer,
which puts a span computed from joined-text offsets inside it.

## `let raw = "## Task\n> <!-- upstroke: id=q\n> kind=fix -->\n\nAfter.\n";`

The same across a blockquote's `> ` prefix.

## `let raw = "- [ ] Design the widget\n  <!-- upstroke: id=widget\n\tkind=design -->\n";`

A tab that straddles the item's indentation makes pulldown-cmark
emit a zero-width text event between the block's two lines; the
block is one construct and the comment is whole.

## `#[test]`

--- the refusals, each with what the task gets ------------------------------

## `let raw = "## Design it\n<!-- upstroke: id=a kind=fix\nBody.\n\n## Next\nmore\n";`

With no `-->` the HTML block runs to the end of the document, the
next heading included; CommonMark reads it so and so does this.

## `let raw =`

The list item's HTML block ends when the item does, at the
unindented prose; the author comment two lines down is a different
block and must not supply the closer.

## `let raw = "## Task <!-- upstroke: id=inline -->\n<!-- upstroke: id=body -->\n";`

The heading's inline annotation is the first one.

## `let parsed = parse(`

Pass 1 finding 3: the warning about a value is about the value
that applies, so a later duplicate that parses leaves no warning
saying the floor, kind, tier or id is missing.

## `for raw in [`

Pass 1 finding 2: one `InlineHtml` event carrying the blockquote's
`>` on its continuation line, with and without the space, nested
in a list item, and at top level of the quote.

## `let raw =`

Only a line's leading `>` run is container syntax; after `key=`
the character is the author's, on the first line and on a quoted
continuation line alike.

## `let plain = raw`

Removing only the comment delimiters exposes the same paragraph
continuation as ordinary text. Its events distinguish a literal
`>id` from a container marker independently of our normalization.

## `let parsed = parse(`

Pass 1 finding 4: both comments sit on the heading line, so the
second reaches the sink through `split_sections`, not the body walk.

## `let parsed =`

Pass 1 finding 1: the checklist body is built from text events,
which an HTML block never produces, so the swallowed prose has to
be put back for the warning's promise to hold.

## `let raw = "<!-- upstroke: id=a\r\nkind=fix -->\r\n";`

A CRLF block: the `\r` before each `\n` is in no event, and the
span ends after `>` and not after the gap that follows it.
