# `src/status/render.rs`

Extended notes for [`src/status/render.rs`](../../../src/status/render.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The settled view and the one-line event descriptions (DESIGN.md §15).

The half of `status` that touches nothing. It takes a `RunStatus` the parent
has already folded out of the log and returns a `String`, and it turns one
`Event` into one line for `--follow`. What is left in the parent is
everything that does reach the world — reading the log, probing the lock,
and streaming to a sink — so the two halves have one reason to change each
(CODING_STANDARDS.md §3).

Splitting the rendering out does not change what it renders: the parent's
`render` and `describe` are the public surface and delegate here, and this
module is private.

### Why the effect denials are restored here

`status` carries a module-level allow of `clippy::disallowed_methods` and
`clippy::disallowed_types`, recorded in the **frozen legacy section** of
`effects/allowlist.toml` — earned by `follow`, which writes to an
`io::Write` sink, and by the husk fixtures, which build run directories with
raw `fs` and a `git` subprocess. Lint levels descend through the module
tree, so that allowance would reach this file for free.

It has no business doing so. Nothing below writes a file, starts a process,
or streams to a sink: the view is accumulated into a `String` through
`std::fmt::Write`, whose `write_fmt` is a different `DefId` from the denied
`io::Write::write_fmt` and is not an effect. Restoring the two denials makes
an effect added here a build error rather than something the parent's
allowance quietly covers — and it is why this file needs no allowlist row of
its own, since an allowance is what that file records and this module takes
none.

## `pub(super) fn render(status: &RunStatus) -> String {`

The settled view, assembled: the report and its ledger, then the trailing
lines that say whether it is still moving and what it is waiting for.

## `pub(super) fn render(status: &RunStatus) -> String` › `if status.running {`

Liveness first among the trailing lines, because it decides whether any
of the above is still moving.

## `pub(super) fn render(status: &RunStatus) -> String` › `let _ = writeln!(`

Finished, and somebody has claimed it anyway — a `resume` between
taking the lock and writing `run_resumed`. The outcome above is still
this run's outcome; it may just not be the last word for long.

## `pub(super) fn describe(event: &Event) -> String {`

One line per event: the wall-clock time out of the record's own timestamp,
then the body.
