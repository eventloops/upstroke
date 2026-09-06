---
id: SWEEP-FOLD_PARSE-002
severity: P3
disposition: deferred     # no consumer is wrong today; the two rules the module states disagree and nothing decides between them
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/parse.rs:9
provenance: pre_existing
first_bad:
guard: the sweep that decides the topology log's line framing — `src/topology/fold.rs` (queue row 40), which owns `FoldError::RewrittenLog`, together with `src/events/log.rs::parse_bytes`, which has the same tolerance
---

## Failure sequence

A topology log is rewritten so that every committed line ends `\r\n` rather than
`\n` — an editor, a `core.autocrlf` checkout, or any filter that normalises line
endings. `TopologyFold::parse_log` walks the committed lines and hands each to
`serde_json::from_str`, which treats the trailing `\r` as whitespace after the
value. Every line parses. The fold answers `Ok(events)`, and the run continues on
a log whose bytes are not the bytes the engine appended.

Before the sweep of 2026-09-06 the same tolerance came from `str::lines()`, which
strips a trailing `\r`; after it, from `serde_json` accepting the `\r` the
`strip_suffix(b"\n")` leaves on the line. The behaviour is master's either way.

The state derived is the same state — a CRLF rewrite preserves each record's
JSON — which is why this is P3 and not higher. What is lost is the detection: the
module's own contract says a committed line that is not what was appended means
"the log has been rewritten, and state derived from what is left would be
confidently wrong", and it refuses a blank committed line on exactly that ground
("Skipping it would fold a log whose physical shape nobody can account for",
`docs/internals/topology/fold/parse.md`). A `\r`-decorated line is equally
unaccountable and is folded. The barrier catches this rewrite by other means when
a commit record exists — `first_line_digest` in `src/events/log.rs` digests the
committed first line — and does not when one does not.

## What the change that takes this up should do

Decide which of the two rules governs the topology log's line framing, and say so
in `docs/internals/topology/fold/parse.md` and in `design/15_*` if the answer is a
contract rather than a local reading:

- if the commit marker is the only framing that matters, record that a committed
  line's bytes are not compared against what was appended, and that the blank-line
  refusal is about JSON validity rather than about physical shape; or
- if a committed line must be exactly what the engine appended, refuse a line whose
  last byte before the marker is `\r`, with a test for each direction, and take
  `src/events/log.rs::parse_bytes` with it — the v0.1 reader has the same tolerance
  and the two are meant to drop a torn tail alike.

Either way the two readers should answer the same, and neither should be changed
alone.
