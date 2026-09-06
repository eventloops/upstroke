# `src/agent/proc/test_support/readiness.rs`

Extended notes for [`src/agent/proc/test_support/readiness.rs`](../../../../../src/agent/proc/test_support/readiness.rs).

[Source on GitHub](https://github.com/sourcemaps/upstroke/blob/master/src/agent/proc/test_support/readiness.rs).

The code defines current behavior. These notes preserve contracts and implementation
history. Search each backticked heading fragment separately in the source.

References below to `decisions.*` use retired v0.2 planning identifiers.
They record implementation history and do not add current requirements.
[DESIGN.md](https://github.com/sourcemaps/upstroke/blob/master/DESIGN.md#retired-records)
is the living design authority.

## Module

CODING_STANDARDS.md §12's readiness protocol, as the primitives a
producer and a waiter have to agree on.

The rules are about an ordering between two processes, so they bind the
helper as much as the test, and every fixture in this crate that hands a
readiness signal across a process boundary had been re-deriving them by
hand. Three of those hand-derivations were wrong in the same way, which
is what this module exists to stop repeating:

* **Publication was not atomic.** `fs::write` creates the name and then
  fills it, so a waiter polling for the path can open it and read
  nothing — §12's "what is unsound is a path created in place and
  written afterwards". [`publish`] stages a sibling and renames, so the
  name and the bytes become visible together.
* **A partial record read as a whole one.** `str::lines` yields an
  unterminated final line as if it were complete, so a torn write
  surfaces as a short value rather than as a failure. [`read_published`]
  requires the terminator.
* **The bound did not bound the producer it was written for.** A
  deadline checked only *after* a blocking `read_line` returns cannot
  fire while that read is blocked, which is the one case §12 says the
  bound exists for: "the fast path is a producer that fails and closes
  its channel; the bound is for the one that stays alive and silent".
  [`Producer::await_line`] reads on a thread and bounds the *wait*.

**Durability is deliberately not part of this.** §8 separates the
guarantees — "a successful rename is not automatically a durability
guarantee" — and what a readiness signal needs is atomic *visibility*,
which the rename already gives. So nothing here enters the durability
barrier: `util::fsync_file` and `util::fsync_dir` bump a process-wide
counter and a thread-local one that
`rundir::tests::the_durability_ledger_counts_barriers_that_were_actually_
performed` and `runner::container::tests` assert deltas against, and a
test-support fixture that quietly incremented them would contaminate
those assertions from whatever thread it happened to run on.

The two waits differ in what they can learn about the producer, and the
split follows §12's two sound publication forms rather than taste. A
pipe has a channel, so EOF *is* the sanctioned fast path and
[`Producer::await_line`] needs nothing else. A file has no channel to
close, so a producer's exit is the only liveness fact available and
[`await_signal`] takes the [`Child`] in order to have one.

## `#![deny(`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, in
its own row rather than by attachment to `src/agent/proc.rs`. A Rust lint
level is scoped by the **module tree** and not by the file, so an out-of-line
child of a funnel inherits the funnel's allow silently -- which is
`PR6-LANEF-004`, measured twice in the Container subtree, and this file is
the first out-of-line child the Process funnel has ever had. All three
governed lints are therefore stated here rather than inherited, and all three
are stated as DENIALS.

**The denial is the whole statement, and the six exceptions are per site.**
This file used to open with a blanket `#![allow(clippy::disallowed_methods)]`
beside the deny of the other two, and that allowance was measured at the time
to buy nothing at the build: `src/agent/proc.rs`'s own allow reaches here
through the module tree, so deleting the line changed no diagnostic. It was
written as a governance statement -- and a file-scope allow is the wrong
statement to make, because it is a claim about the FILE when what is true is
a claim about six lines of it. A seventh denied call, or a denied call in a
function that has nothing to do with publication, arrives under the same
allowance and nothing says so.

So the lint is denied at file scope like the other two, and each of the six
call sites carries its own `#[expect(clippy::disallowed_methods, reason = …)]`.
That makes the compiler the authority on the count, in both directions and
under the `-D warnings` the gate runs with:

  * a SEVENTH denied call anywhere in this file is a build error, because
    nothing above it allows one; and
  * a site that stops reaching a denied path is `unfulfilled_lint_expectations`
    on its own `#[expect]`, so the annotations cannot outlive the calls they
    were written for.

`effects/allowlist.toml` records the lint and the exact number of sites, and
`effects::tests::the_readiness_expectations_are_per_site_and_both_records_say_so`
asserts the source attributes, the row and these notes agree. The two censuses
read the count from these notes; the source keeps its header pointer and lint
attributes. The rule that admits a
per-site `#[expect]` below module level at all is the lints paragraph of
`standards/02_standards_automated_baseline.md`: the allowlist's own
mechanism otherwise permits an allowance only as module-level attributes,
and this is narrower than the module-level allowance it replaces rather
than a widening of it.

What the six are written against is the staged publication in
`publish_between`: **five distinct denied paths across six sites** --
`File::create_new`, `write_all`, `flush`, `fs::rename` and `fs::remove_file`,
the last of which is called twice, once on the write-side failure path and
once on the rename-side one. Paths and sites are counted separately because
the allowlist row is a claim about which *primitives* this file may reach,
while the census that reads it counts occurrences; running the two together
as "five calls" is what made the row and the file disagree.
`runner::container::tests::the_readiness_allowance_names_the_paths_it_is_\
written_against` derives the set from `clippy.toml` rather than from this
list, so a sixth path arriving here fails whether or not anyone edits this
sentence. `decisions.effect_site_inventory.mechanism` (2), and
`runner::container::tests::every_child_module_of_the_container_funnel_states_its_own_lint_level`
is the census that refuses a Process- or Container-funnel child stating
neither level.

## `const POLL: Duration = Duration::from_millis(10);`

How often a path-shaped wait re-stats its signal.

Reused rather than introduced: 10 ms is the interval every
path-polling readiness wait in this crate already used before these
primitives existed. It is not a bound — the bound is always the
caller's — so no product timeout, cap or policy is decided here.

## `const TERMINATOR: char = '\n';`

The record terminator. A record is complete only once it arrives.

## `const STAGING: &str = ".publishing";`

The suffix every staging name ends in, so residue is recognisable.

## `pub(crate) enum Waited {`

How a bounded wait ended.

Four outcomes rather than an `Option`, because §12 asks a waiter to
tell them apart: a producer that died without publishing is a
different failure from one that is alive and silent, and reporting
the first as the second is how a deadline becomes the signal.

## `pub(crate) enum Waited` › `Ready(Vec<String>),`

The signal arrived, whole. Carries the fields it framed — empty
for the marker form, which announces state it has nothing to say
about.

## `pub(crate) enum Waited` › `ProducerGone(String),`

The producer will never publish: it has exited, or it closed its
channel. The fast path, and it does not wait the bound out.

## `pub(crate) enum Waited` › `TimedOut(Duration),`

The producer is still alive and has published nothing. This is
the outcome the bound exists for, and the bound is the caller's.

## `pub(crate) enum Waited` › `Torn(String),`

The signal appeared but its bytes are not a whole record, or the
producer spent the whole output allowance without framing one.

## `impl Waited` › `pub(crate) fn or_fail(self, what: &str) -> Vec<String> {`

The fields, or a failure that says which outcome ended the wait.

`what` names the state the waiter was promised, so the three
failures read as claims about the producer rather than about the
clock.

## `fn staging_for(signal: &Path) -> PathBuf {`

The staging name for **one** publication, in `signal`'s own directory
so the rename cannot cross a filesystem.

Unique per call, not per signal. A fixed `<signal>.publishing` is one
name shared by every publisher of that signal: two concurrent
publications interleave in it, and — worse — the failure path of
either one deletes whatever is there, which by then may be the other
one's staged record. The process id and a ULID make the name this
call's alone, which is what lets the cleanup below run
unconditionally without ever removing somebody else's file.

## `pub(crate) fn publish(signal: &Path, fields: &[&str]) -> std::io::Result<()> {`

Publish `fields` at `signal` so that the name and the bytes become
visible together.

Each field is one terminated record. A field carrying the framing's
own delimiter is refused rather than written, because §12's "keep the
payload inside what the framing can carry" is a property of the
payload and only the producer can check it: by the time a waiter sees
two records where one was sent, both look complete.

Prefer sending an identifier the waiter can rejoin to a root it
already knows over sending a path.

### Errors

[`std::io::Error`] from the staging write or the rename, and
`InvalidInput` for a field the framing cannot carry. On any of them
the signal name is never created, so a failed publish is not a
readiness claim.

## `pub(crate) fn publish_between(`

[`publish`], with `between` run after the record is staged and before
it is renamed into place.

The seam exists so the atomicity claim can be tested by *arranging*
the interleaving rather than by racing for it. At the moment
`between` runs, the record's bytes are entirely written and the
signal name does not exist — which is the whole of what "published
atomically" means, and a test holding this point can assert it
without depending on the scheduler. It is also the only place a
post-staging failure can be arranged, which is what gives the
cleanup path below a witness.

### Errors

As [`publish`].

## `let mut file = std::fs::File::create_new(&staged)?;`

`create_new`, and the `?` rather than a cleanup: if this fails
nothing was created, and removing the name anyway would be
removing a file this call does not own. Past this line the staging
file is provably ours — a name unique to this call, brought into
existence exclusively — so every failure below may remove it.

Each of the six statements below reaches exactly one denied path and
carries its own expectation. They are separate statements for that
reason: an attribute on the combined `write_all(…).and_then(…)` would
cover two sites with one annotation, and the count is the claim.

## `pub(crate) fn publish_marker(signal: &Path) -> std::io::Result<()> {`

Publish an empty marker at `signal`.

§12's other sound form: "an empty marker created after the state it
announces, where there is nothing to read". Renamed into place like
the record form, which is what keeps an empty published file
unambiguous — see [`read_published`].

### Errors

As [`publish`].

## `pub(crate) fn read_published(signal: &Path) -> std::io::Result<Vec<String>> {`

Read a record [`publish`] wrote, refusing a partial one.

§12: "a partial record MUST NOT be readable as a whole one … an
unterminated final record is a truncated write and MUST fail rather
than yield a short value". `str::lines` does exactly the yielding
this refuses, which is why reading through it is not enough.

An empty file is the marker form and reads as zero fields. That is
not ambiguous with a one-field record truncated to nothing, because
[`publish`] renames: a partial record is never given this name.

### Errors

[`std::io::Error`] from the read, and `UnexpectedEof` for content
that does not end with the terminator.

## `fn await_signal`

Await a file-shaped readiness signal from `producer`, bounded by
`bound`.

A file has no channel to close, so the producer's exit is the only
liveness fact there is: without it a producer that died before
publishing is indistinguishable from a slow one, and the waiter
reports the clock instead of the death.

## `fn await_signal` › `if signal.exists() {`

One last look. The producer may have published and
then exited between the stat above and this call, and
a signal that is on disk is a signal however dead its
producer now is.

## `fn published(signal: &Path) -> Waited {`

[`read_published`] as a [`Waited`].

## `enum Framed {`

What one read off the producer's pipe produced.

## `enum Framed` › `Line(String),`

A complete, terminated record.

## `enum Framed` › `Unterminated(String),`

Bytes arrived and then the channel ended: a truncated write.

## `enum Framed` › `Eof,`

The channel closed cleanly. §12's fast path.

## `enum Framed` › `Flooded(usize),`

The producer spent the whole output allowance without framing a
record. Terminal, so the reader stops rather than growing.

## `enum Framed` › `Failed(String),`

The read itself failed.

## `fn read_frames(stdout: ChildStdout, framed: &Sender<Framed>) {`

Drain `stdout` into `framed`, one record at a time, bounded.

The bound is `super::super::OUTPUT_LIMIT_BYTES` — this module's own
per-stream output allowance, reused rather than a second cap
introduced beside it. It matters because `read_line` against a
producer that never frames anything grows a `String` without limit:
the same shape `rundir::classify::first_line` already refuses, and a
fixture that ran the machine out of memory while waiting would be a
worse failure than the one the wait is bounding.

## `fn read_frames(stdout: ChildStdout, framed: &Sender<Framed>)` › `Framed::Flooded(drained)`

Cut by the allowance rather than by the producer ending.

## `fn read_frames(stdout: ChildStdout, framed: &Sender<Framed>)` › `if framed.send(message).is_err() || !complete {`

Only a complete record leaves the reader anything to do next;
every other message is terminal, and a closed receiver means
the waiter has already ended.

## `pub(crate) struct Producer {`

An adopted child, plus the reader draining its pipe.

The type exists for its destructor. A reader thread blocked in
`read_line` cannot be joined by asking it to stop — only the last
write handle closing ends it — so terminating the child, reaping it
and joining the reader are one ordered operation, and the only place
that ordering can be guaranteed on a panicking path is a `Drop`.

## `impl Producer` › `pub(crate) fn adopt(mut child: Child) -> Self {`

Adopt `child`, draining its stdout if it was piped.

## `impl Producer` › `pub(crate) fn child(&mut self) -> &mut Child {`

The adopted child, for a test that drives the process directly.

## `impl Producer` › `pub(crate) fn alive(&mut self) -> bool {`

Whether the producer is still running.

## `impl Producer` › `pub(crate) fn await_line(&mut self, wanted: &str, bound: Duration) -> Waited {`

Await the line `wanted`, bounded by `bound`.

Lines that are not `wanted` are skipped rather than refused: a
child run under `--nocapture` prints its own harness chatter on
the same pipe, and a waiter that treated the first line as the
answer would be reading the harness.

**The bound is effective on both paths.** The blocking read
happens on the reader thread, so the deadline fires while the
producer is still holding the pipe open — the live-silent case
§12 says the bound exists for. And the noise path is bounded
too: once `remaining` reaches zero `recv_timeout` degenerates to
a non-blocking poll, so a producer that frames records faster
than the bound would keep returning `Ok` and the loop would run
past its own deadline for ever. The explicit check below is what
stops that, and it is the difference between a deadline and a
hope.

## `fn drop(&mut self)` › `let _ = self.child.kill();`

Ordered, and the order is the whole point. Killing the child
closes the pipe's last write handle, which is what lets a
reader blocked in `read_line` reach EOF and end; joining
first would deadlock on exactly the live-silent producer this
module exists to bound.
