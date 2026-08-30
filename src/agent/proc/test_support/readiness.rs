//! CODING_STANDARDS.md §12's readiness protocol, as the primitives a
//! producer and a waiter have to agree on.
//!
//! The rules are about an ordering between two processes, so they bind the
//! helper as much as the test, and every fixture in this crate that hands a
//! readiness signal across a process boundary had been re-deriving them by
//! hand. Three of those hand-derivations were wrong in the same way, which
//! is what this module exists to stop repeating:
//!
//! * **Publication was not atomic.** `fs::write` creates the name and then
//!   fills it, so a waiter polling for the path can open it and read
//!   nothing — §12's "what is unsound is a path created in place and
//!   written afterwards". [`publish`] stages a sibling and renames, so the
//!   name and the bytes become visible together.
//! * **A partial record read as a whole one.** `str::lines` yields an
//!   unterminated final line as if it were complete, so a torn write
//!   surfaces as a short value rather than as a failure. [`read_published`]
//!   requires the terminator.
//! * **The bound did not bound the producer it was written for.** A
//!   deadline checked only *after* a blocking `read_line` returns cannot
//!   fire while that read is blocked, which is the one case §12 says the
//!   bound exists for: "the fast path is a producer that fails and closes
//!   its channel; the bound is for the one that stays alive and silent".
//!   [`Producer::await_line`] reads on a thread and bounds the *wait*.
//!
//! **Durability is deliberately not part of this.** §8 separates the
//! guarantees — "a successful rename is not automatically a durability
//! guarantee" — and what a readiness signal needs is atomic *visibility*,
//! which the rename already gives. So nothing here enters the durability
//! barrier: `util::fsync_file` and `util::fsync_dir` bump a process-wide
//! counter and a thread-local one that
//! `rundir::tests::the_durability_ledger_counts_barriers_that_were_actually_
//! performed` and `runner::container::tests` assert deltas against, and a
//! test-support fixture that quietly incremented them would contaminate
//! those assertions from whatever thread it happened to run on.
//!
//! The two waits differ in what they can learn about the producer, and the
//! split follows §12's two sound publication forms rather than taste. A
//! pipe has a channel, so EOF *is* the sanctioned fast path and
//! [`Producer::await_line`] needs nothing else. A file has no channel to
//! close, so a producer's exit is the only liveness fact available and
//! [`await_signal`] takes the [`Child`] in order to have one.

// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, in
// its own row rather than by attachment to `src/agent/proc.rs`. A Rust lint
// level is scoped by the **module tree** and not by the file, so an out-of-line
// child of a funnel inherits the funnel's allow silently -- which is
// `PR6-LANEF-004`, measured twice in the Container subtree, and this file is
// the first out-of-line child the Process funnel has ever had. All three
// governed lints are therefore stated here rather than inherited.
//
// **What each of the two attributes actually buys, measured on this tree at
// this commit rather than assumed.** The naive reading of the pair is that the
// allow is what lets the five denied calls below compile and the deny is
// tidiness. It is the other way round:
//
// * Deleting `#![deny(...)]` and planting a `std::process::Command` and a
//   `println!` in this file compiles clean at
//   `cargo clippy --all-targets --all-features -- -D warnings` -- zero
//   `disallowed_types` and zero `disallowed_macros` diagnostics -- because
//   `src/agent/proc.rs`'s inner `#![allow(...)]` reaches here through the
//   module tree. With the deny restored the same plant is two errors. That is
//   `PR6-LANEF-004` reproducing for the Process funnel, and the deny is what
//   closes it.
// * Deleting `#![allow(clippy::disallowed_methods)]` changes **nothing** at the
//   build: the same inheritance covers all six call sites. So the allow is not
//   load-bearing for the compiler, and it is not written for the compiler. It
//   is the governance statement -- `effects/allowlist.toml` records the exact
//   lint set this file names and
//   `effects::tests::every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist`
//   asserts the two are **equal**, so an allowance that widened here without
//   widening the reviewed row is a build failure. Stating it also keeps this
//   file out of the "inherits and says nothing" class that census refuses.
//
// What the row is written against is the staged publication in
// `publish_between`: **five distinct denied method paths across six call
// sites** -- `File::create_new`, `write_all`, `flush`, `fs::rename` and
// `fs::remove_file`, the last of which is called twice, once on the write-side
// failure path and once on the rename-side one. Distinct paths and call sites
// are counted separately because the allowlist row is a claim about which
// *primitives* this file may reach, and the census that reads it counts
// occurrences. `decisions.effect_site_inventory.mechanism` (2), and
// `runner::container::tests::every_child_module_of_the_container_funnel_states_its_own_lint_level`
// is the census that refuses a Process- or Container-funnel child stating
// neither level.
#![allow(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types, clippy::disallowed_macros)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How often a path-shaped wait re-stats its signal.
///
/// Reused rather than introduced: 10 ms is the interval every
/// path-polling readiness wait in this crate already used before these
/// primitives existed. It is not a bound — the bound is always the
/// caller's — so no product timeout, cap or policy is decided here.
const POLL: Duration = Duration::from_millis(10);

/// The record terminator. A record is complete only once it arrives.
const TERMINATOR: char = '\n';

/// The suffix every staging name ends in, so residue is recognisable.
const STAGING: &str = ".publishing";

/// How a bounded wait ended.
///
/// Four outcomes rather than an `Option`, because §12 asks a waiter to
/// tell them apart: a producer that died without publishing is a
/// different failure from one that is alive and silent, and reporting
/// the first as the second is how a deadline becomes the signal.
#[derive(Debug)]
pub(crate) enum Waited {
    /// The signal arrived, whole. Carries the fields it framed — empty
    /// for the marker form, which announces state it has nothing to say
    /// about.
    Ready(Vec<String>),
    /// The producer will never publish: it has exited, or it closed its
    /// channel. The fast path, and it does not wait the bound out.
    ProducerGone(String),
    /// The producer is still alive and has published nothing. This is
    /// the outcome the bound exists for, and the bound is the caller's.
    TimedOut(Duration),
    /// The signal appeared but its bytes are not a whole record, or the
    /// producer spent the whole output allowance without framing one.
    Torn(String),
}

impl Waited {
    /// The fields, or a failure that says which outcome ended the wait.
    ///
    /// `what` names the state the waiter was promised, so the three
    /// failures read as claims about the producer rather than about the
    /// clock.
    pub(crate) fn or_fail(self, what: &str) -> Vec<String> {
        match self {
            Self::Ready(fields) => fields,
            Self::ProducerGone(why) => {
                panic!("{what}: the producer will never publish it ({why})")
            }
            Self::TimedOut(bound) => panic!(
                "{what}: the producer is still alive and had published nothing after \
                 {bound:?}"
            ),
            Self::Torn(why) => panic!("{what}: {why}"),
        }
    }
}

/// The staging name for **one** publication, in `signal`'s own directory
/// so the rename cannot cross a filesystem.
///
/// Unique per call, not per signal. A fixed `<signal>.publishing` is one
/// name shared by every publisher of that signal: two concurrent
/// publications interleave in it, and — worse — the failure path of
/// either one deletes whatever is there, which by then may be the other
/// one's staged record. The process id and a ULID make the name this
/// call's alone, which is what lets the cleanup below run
/// unconditionally without ever removing somebody else's file.
fn staging_for(signal: &Path) -> PathBuf {
    let name = signal
        .file_name()
        .expect("a readiness signal is a file, so it has a file name");
    let mut staged = name.to_os_string();
    staged.push(format!(
        ".{}.{}{STAGING}",
        std::process::id(),
        crate::ulid::ulid()
    ));
    signal.with_file_name(staged)
}

/// Publish `fields` at `signal` so that the name and the bytes become
/// visible together.
///
/// Each field is one terminated record. A field carrying the framing's
/// own delimiter is refused rather than written, because §12's "keep the
/// payload inside what the framing can carry" is a property of the
/// payload and only the producer can check it: by the time a waiter sees
/// two records where one was sent, both look complete.
///
/// Prefer sending an identifier the waiter can rejoin to a root it
/// already knows over sending a path.
///
/// # Errors
///
/// [`std::io::Error`] from the staging write or the rename, and
/// `InvalidInput` for a field the framing cannot carry. On any of them
/// the signal name is never created, so a failed publish is not a
/// readiness claim.
pub(crate) fn publish(signal: &Path, fields: &[&str]) -> std::io::Result<()> {
    publish_between(signal, fields, &mut || {})
}

/// [`publish`], with `between` run after the record is staged and before
/// it is renamed into place.
///
/// The seam exists so the atomicity claim can be tested by *arranging*
/// the interleaving rather than by racing for it. At the moment
/// `between` runs, the record's bytes are entirely written and the
/// signal name does not exist — which is the whole of what "published
/// atomically" means, and a test holding this point can assert it
/// without depending on the scheduler. It is also the only place a
/// post-staging failure can be arranged, which is what gives the
/// cleanup path below a witness.
///
/// # Errors
///
/// As [`publish`].
pub(crate) fn publish_between(
    signal: &Path,
    fields: &[&str],
    between: &mut dyn FnMut(),
) -> std::io::Result<()> {
    let mut record = String::new();
    for field in fields {
        if field.contains('\n') || field.contains('\r') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "a readiness field carries the framing's own delimiter and would \
                     arrive as more than one record: {field:?}"
                ),
            ));
        }
        record.push_str(field);
        record.push(TERMINATOR);
    }
    let staged = staging_for(signal);
    // `create_new`, and the `?` rather than a cleanup: if this fails
    // nothing was created, and removing the name anyway would be
    // removing a file this call does not own. Past this line the staging
    // file is provably ours — a name unique to this call, brought into
    // existence exclusively — so every failure below may remove it.
    let mut file = std::fs::File::create_new(&staged)?;
    let staged_write = file
        .write_all(record.as_bytes())
        .and_then(|()| file.flush());
    drop(file);
    if let Err(error) = staged_write {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    between();
    if let Err(error) = std::fs::rename(&staged, signal) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    Ok(())
}

/// Publish an empty marker at `signal`.
///
/// §12's other sound form: "an empty marker created after the state it
/// announces, where there is nothing to read". Renamed into place like
/// the record form, which is what keeps an empty published file
/// unambiguous — see [`read_published`].
///
/// # Errors
///
/// As [`publish`].
pub(crate) fn publish_marker(signal: &Path) -> std::io::Result<()> {
    publish(signal, &[])
}

/// Read a record [`publish`] wrote, refusing a partial one.
///
/// §12: "a partial record MUST NOT be readable as a whole one … an
/// unterminated final record is a truncated write and MUST fail rather
/// than yield a short value". `str::lines` does exactly the yielding
/// this refuses, which is why reading through it is not enough.
///
/// An empty file is the marker form and reads as zero fields. That is
/// not ambiguous with a one-field record truncated to nothing, because
/// [`publish`] renames: a partial record is never given this name.
///
/// # Errors
///
/// [`std::io::Error`] from the read, and `UnexpectedEof` for content
/// that does not end with the terminator.
pub(crate) fn read_published(signal: &Path) -> std::io::Result<Vec<String>> {
    let text = std::fs::read_to_string(signal)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    if !text.ends_with(TERMINATOR) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!(
                "{} ends without the record terminator, so its last {} byte(s) are a \
                 truncated write rather than a record",
                signal.display(),
                text.len() - text.rfind(TERMINATOR).map_or(0, |at| at + 1)
            ),
        ));
    }
    Ok(text.lines().map(str::to_owned).collect())
}

/// Await a file-shaped readiness signal from `producer`, bounded by
/// `bound`.
///
/// A file has no channel to close, so the producer's exit is the only
/// liveness fact there is: without it a producer that died before
/// publishing is indistinguishable from a slow one, and the waiter
/// reports the clock instead of the death.
pub(crate) fn await_signal(signal: &Path, producer: &mut Child, bound: Duration) -> Waited {
    let deadline = Instant::now() + bound;
    loop {
        if signal.exists() {
            return published(signal);
        }
        match producer.try_wait() {
            Ok(Some(status)) => {
                // One last look. The producer may have published and
                // then exited between the stat above and this call, and
                // a signal that is on disk is a signal however dead its
                // producer now is.
                if signal.exists() {
                    return published(signal);
                }
                return Waited::ProducerGone(format!(
                    "it exited {status:?} without publishing {}",
                    signal.display()
                ));
            }
            Ok(None) => {}
            Err(error) => {
                return Waited::ProducerGone(format!("waiting on it failed: {error}"));
            }
        }
        if Instant::now() >= deadline {
            return Waited::TimedOut(bound);
        }
        thread::sleep(POLL);
    }
}

/// [`read_published`] as a [`Waited`].
fn published(signal: &Path) -> Waited {
    match read_published(signal) {
        Ok(fields) => Waited::Ready(fields),
        Err(error) => Waited::Torn(error.to_string()),
    }
}

/// What one read off the producer's pipe produced.
enum Framed {
    /// A complete, terminated record.
    Line(String),
    /// Bytes arrived and then the channel ended: a truncated write.
    Unterminated(String),
    /// The channel closed cleanly. §12's fast path.
    Eof,
    /// The producer spent the whole output allowance without framing a
    /// record. Terminal, so the reader stops rather than growing.
    Flooded(usize),
    /// The read itself failed.
    Failed(String),
}

/// Drain `stdout` into `framed`, one record at a time, bounded.
///
/// The bound is `super::super::OUTPUT_LIMIT_BYTES` — this module's own
/// per-stream output allowance, reused rather than a second cap
/// introduced beside it. It matters because `read_line` against a
/// producer that never frames anything grows a `String` without limit:
/// the same shape `rundir::first_line` already refuses, and a fixture
/// that ran the machine out of memory while waiting would be a worse
/// failure than the one the wait is bounding.
fn read_frames(stdout: ChildStdout, framed: &Sender<Framed>) {
    let mut reader = BufReader::new(stdout);
    let mut drained = 0_usize;
    loop {
        let allowance = super::super::OUTPUT_LIMIT_BYTES.saturating_sub(drained);
        if allowance == 0 {
            let _ = framed.send(Framed::Flooded(drained));
            return;
        }
        let mut line = String::new();
        let read = match (&mut reader).take(allowance as u64).read_line(&mut line) {
            Ok(read) => read,
            Err(error) => {
                let _ = framed.send(Framed::Failed(error.to_string()));
                return;
            }
        };
        if read == 0 {
            let _ = framed.send(Framed::Eof);
            return;
        }
        drained = drained.saturating_add(read);
        let complete = line.ends_with(TERMINATOR);
        let message = if complete {
            Framed::Line(line)
        } else if drained >= super::super::OUTPUT_LIMIT_BYTES {
            // Cut by the allowance rather than by the producer ending.
            Framed::Flooded(drained)
        } else {
            Framed::Unterminated(line)
        };
        // Only a complete record leaves the reader anything to do next;
        // every other message is terminal, and a closed receiver means
        // the waiter has already ended.
        if framed.send(message).is_err() || !complete {
            return;
        }
    }
}

/// An adopted child, plus the reader draining its pipe.
///
/// The type exists for its destructor. A reader thread blocked in
/// `read_line` cannot be joined by asking it to stop — only the last
/// write handle closing ends it — so terminating the child, reaping it
/// and joining the reader are one ordered operation, and the only place
/// that ordering can be guaranteed on a panicking path is a `Drop`.
pub(crate) struct Producer {
    child: Child,
    reader: Option<JoinHandle<()>>,
    framed: Receiver<Framed>,
}

impl Producer {
    /// Adopt `child`, draining its stdout if it was piped.
    pub(crate) fn adopt(mut child: Child) -> Self {
        let (sender, framed) = mpsc::channel();
        let reader = child
            .stdout
            .take()
            .map(|stdout| thread::spawn(move || read_frames(stdout, &sender)));
        Self {
            child,
            reader,
            framed,
        }
    }

    /// The adopted child, for a test that drives the process directly.
    pub(crate) fn child(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Whether the producer is still running.
    pub(crate) fn alive(&mut self) -> bool {
        self.child
            .try_wait()
            .expect("wait on the readiness producer")
            .is_none()
    }

    /// Await the line `wanted`, bounded by `bound`.
    ///
    /// Lines that are not `wanted` are skipped rather than refused: a
    /// child run under `--nocapture` prints its own harness chatter on
    /// the same pipe, and a waiter that treated the first line as the
    /// answer would be reading the harness.
    ///
    /// **The bound is effective on both paths.** The blocking read
    /// happens on the reader thread, so the deadline fires while the
    /// producer is still holding the pipe open — the live-silent case
    /// §12 says the bound exists for. And the noise path is bounded
    /// too: once `remaining` reaches zero `recv_timeout` degenerates to
    /// a non-blocking poll, so a producer that frames records faster
    /// than the bound would keep returning `Ok` and the loop would run
    /// past its own deadline for ever. The explicit check below is what
    /// stops that, and it is the difference between a deadline and a
    /// hope.
    pub(crate) fn await_line(&mut self, wanted: &str, bound: Duration) -> Waited {
        let deadline = Instant::now() + bound;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.framed.recv_timeout(remaining) {
                Ok(Framed::Line(line)) => {
                    if line.trim() == wanted {
                        return Waited::Ready(vec![line.trim().to_owned()]);
                    }
                    if Instant::now() >= deadline {
                        return Waited::TimedOut(bound);
                    }
                }
                Ok(Framed::Unterminated(partial)) => {
                    return Waited::Torn(format!(
                        "the producer's channel ended mid-record: {partial:?} is a \
                         truncated write, not a line"
                    ));
                }
                Ok(Framed::Flooded(bytes)) => {
                    return Waited::Torn(format!(
                        "the producer wrote {bytes} byte(s) without ever framing \
                         `{wanted}`, which is the whole per-stream output allowance"
                    ));
                }
                Ok(Framed::Eof) => {
                    return Waited::ProducerGone(format!(
                        "it closed its channel without framing `{wanted}`"
                    ));
                }
                Ok(Framed::Failed(error)) => {
                    return Waited::ProducerGone(format!("reading its channel failed: {error}"));
                }
                Err(RecvTimeoutError::Timeout) => return Waited::TimedOut(bound),
                Err(RecvTimeoutError::Disconnected) => {
                    return Waited::ProducerGone(
                        "the reader ended without reaching a verdict".to_owned(),
                    );
                }
            }
        }
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        // Ordered, and the order is the whole point. Killing the child
        // closes the pipe's last write handle, which is what lets a
        // reader blocked in `read_line` reach EOF and end; joining
        // first would deadlock on exactly the live-silent producer this
        // module exists to bound.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
