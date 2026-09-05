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

### Errors

[`FoldError::RewrittenLog`] naming the first committed line that is not
a valid event.

## `pub fn parse_log(bytes: &[u8]) -> Result<Vec<TopologyEvent>…` › `events.push(`

Every committed line is one event, including a blank or
whitespace-only one. refusals[23] is about the *commit marker*,
not about what the bytes look like: a newline-terminated line
that is not a valid event means the log was rewritten, and a line
that is empty is not a valid event. Skipping it would fold a log
whose physical shape nobody can account for.
