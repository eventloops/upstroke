# `src/plan/markdown/hints.rs`

Extended notes for [`src/plan/markdown/hints.rs`](../../../../src/plan/markdown/hints.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Path-hint heuristics: the file paths a task's prose mentions.

A bare word is a hint when it looks like a path rather than a sentence —
it contains a separator, is not a URL, and either carries a known source
extension, globs, or is at least two segments deep. Inline code is judged
more loosely, since a backticked token is already a deliberate reference.

A source of the DAG with two consumers: [`super::drafts`] harvests hints
while walking a task's events, and [`super::assemble`] reuses
[`push_unique`] to merge them behind the annotation's own `paths=`.
