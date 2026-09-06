# `src/topology/fold/parse.rs`

Extended notes for [`src/topology/fold/parse.rs`](../../../../src/topology/fold/parse.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Reading a topology log into events.

The commit marker is the newline, so a torn tail is dropped and a committed
line that will not parse is a rewritten log rather than a short read.

## `impl TopologyFold` › `pub fn parse_log(bytes: &[u8]) -> Result<Vec<TopologyEvent>, FoldError> {`

Every committed line of a topology log, in order.

The newline is the commit marker: an unterminated final line is a torn
tail and is dropped, exactly as [`crate::events`] drops it. A
newline-terminated line that will not parse is the opposite situation —
the line was committed and is not an event, which means the log was
rewritten rather than appended to, and no amount of reading further
recovers it.

The log is read one committed line at a time, in order, so the line a
refusal names is the first committed line that is not an event. Until
the sweep of 2026-09-06 the bytes were read in two passes — one
`from_utf8` over the whole committed prefix, then a parse of each line —
and the first pass answered first: a log whose line 1 was not an event
and whose line 3 was not UTF-8 was refused as line 3, against this
section's own promise below. Reading each line in turn also removes the
recomputation that pass needed, a count of the newlines before
`Utf8Error::valid_up_to()`, whose answer was only the line number
because the slice it counted over was a prefix of the slice `from_utf8`
had been given. `position` is that line number directly.

### Errors

[`FoldError::RewrittenLog`] naming the first committed line that is not
a valid event, whether it is not UTF-8 or is not an event record.

## `pub fn parse_log(bytes: &[u8]) -> Result<Vec<TopologyEvent>…` › `let Some(committed) = chunk.strip_suffix(b"\n") else {`

`split_inclusive` leaves the commit marker on each chunk and can only
leave the **last** chunk without one, so the missing marker is the torn
tail and `break` is the whole of dropping it. Nothing past this point
reads a byte the marker does not cover, which is what makes an
interrupted write inside a multi-byte character a torn tail rather than
a log that is not UTF-8.

## `pub fn parse_log(bytes: &[u8]) -> Result<Vec<TopologyEvent>…` › `let text = std::str::from_utf8(committed).map_err(|error| FoldError::RewrittenLog {`

A committed line is validated as UTF-8 by itself. That is the same
acceptance as validating the whole committed prefix at once — `\n` is
ASCII, so it can never fall inside a multi-byte sequence and a line
boundary never splits a character — and it is what puts the two
refusals in log order. The `detail` carries the `Utf8Error`, whose byte
index is now the index within the line rather than within the log.

## `pub fn parse_log(bytes: &[u8]) -> Result<Vec<TopologyEvent>…` › `events.push(event);`

Every committed line is one event, including a blank or
whitespace-only one. refusals[23] is about the *commit marker*,
not about what the bytes look like: a newline-terminated line
that is not a valid event means the log was rewritten, and a line
that is empty is not a valid event. Skipping it would fold a log
whose physical shape nobody can account for.

## `#[cfg(test)] mod tests`

Three tests of `parse_log`'s own contract, in this file rather than in
`src/topology/fold/tests.rs` where the rest of the fold's tests live —
that file is queue row 39 and the sweep that added these could not edit
it. They are the second such block in the `fold` family, after
`src/topology/fold/check_candidate.rs`, and for the same reason.

`a_committed_line_that_is_not_an_event_is_a_rewritten_log`, in the
sibling suite, already pins the whole-log, torn-tail, blank-line and
per-line-number claims. These pin what it does not, each measured
against the whole `topology::fold` suite at the mutation named:

- the **torn tail's bytes are never read**, witnessed by a tail that is
  half of a character — the shape an interrupted write actually leaves
  behind. Validating UTF-8 over the whole input instead of over each
  committed line, with the line number master computed, leaves the
  sibling suite green (132 passed) and is caught only here and by the
  first-line test;
- a log that has **not reached a commit marker at all** — empty, or one
  interrupted first line — holds no committed line. Master's
  `map_or(0, …)` default answering `map_or(bytes.len(), …)`, so an
  unterminated log is read as committed, is caught by this test and by
  nothing else in the fold suite;
- the refusal names the **first** committed line that is not an event,
  in both orders and at a line that is neither the first nor the last.
  This is the regression test for the two-pass ordering above: master's
  body under these tests answers line 3 where the first committed
  non-event is line 1.
