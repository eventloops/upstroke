# `src/agent/proc/drain.rs`

Extended notes for [`src/agent/proc/drain.rs`](../../../../src/agent/proc/drain.rs).

[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/agent/proc/drain.rs).

The code defines current behavior. These notes preserve contracts and implementation
history. Search each backticked heading fragment separately in the source.

## Module

The pipe reader the supervisor never has to join.

Split out of `src/agent/proc.rs`. A child's descendant can inherit a write
handle and outlive the child, so a reader joined unconditionally would stall
the supervisor for as long as that orphan lives. Each stream accumulates into
a shared buffer that is snapshotted after a bounded grace instead, and the
abandoned reader exits on its own when the last write handle closes.

`PR6-LANEF-004`: it states its own lint level rather than inheriting the
funnel's `#![allow]`, and denies all three governed lints. No
`effects/allowlist.toml` row: a denial needs none. `Drain::start` is this
module's own name and `thread::spawn` is not `std::process::Command::spawn`;
a segment-matching scan flags both and neither is a denied primitive.

## `pub(super) struct Drain {`

A pipe reader whose buffer can be snapshotted without joining the thread,
so an orphan holding the write end can never stall the supervisor.

## `impl Drain` › `pub(super) fn collect(self, grace: Duration) -> (String, bool) {`

Wait up to `grace` for EOF, then snapshot whatever arrived. A reader
abandoned here exits on its own when the last write handle closes.
