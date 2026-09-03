//! Committed-or-Husk: the first-line probe `startup_census` classifies by.
//!
//! `sequential_substrate.startup_census`: "every entry is classified by
//! `rundir::classify_run_dir` as **Committed** (`events.jsonl` exists and its
//! first newline-terminated line is a valid `run_started`) or **Husk**
//! (anything else)". Read-only and total: the census holds the physical
//! worktree lock across it, so an entry that never classifies is a lock held
//! for ever, and every bound here exists for that.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree and not by the file, so an out-of-line
// child of `src/rundir.rs` would otherwise inherit that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// -- `PR6-LANEF-004`, measured twice in the Container subtree and made again,
// independently, by two W1 pull requests. Nothing here reaches a governed
// primitive, so all three are DENIED rather than allowed, and this module takes
// no `effects/allowlist.toml` row: an allowance is what that file records, and
// this module takes none.
//
// **Measured, not believed.** A probe of three lines -- a `std::fs::write`, a
// `std::process::Command` and a `println!` -- is refused three times here, once
// per lint, with this attribute cited as the level; the identical three lines in
// `src/rundir.rs` emit no `disallowed_*` at all, under that file's own allow. So
// the deny is load-bearing rather than a restatement of an ambient rule.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::EVENT_LOG;

/// What a directory under `<repo>/.upstroke/runs` is.
///
/// `sequential_substrate.startup_census`: "every entry is classified by
/// `rundir::classify_run_dir` as **Committed** (`events.jsonl` exists and its
/// first newline-terminated line is a valid `run_started`) or **Husk**
/// (anything else)".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunDirClass {
    /// A run exists here and is resumable.
    Committed,
    /// No committed `run_started`. Nothing about a marker changes this, in
    /// either direction.
    Husk,
}

/// How much of `events.jsonl` the first-line probe reads in one go.
///
/// **A performance constant, not a classification bound.** A `run_started` line
/// records the plan path, the gate commands and the runner policy — kilobytes,
/// not megabytes — so a megabyte reaches the newline in a single read for every
/// log this project has ever written. A longer first line is still a first
/// line: `startup_census` defines `Committed` as "`events.jsonl` exists and its
/// first newline-terminated line is a valid `run_started`" and states no size
/// exception, so [`first_line`] falls back to a scan rather than answering
/// `Husk`. Changing this value changes how many syscalls the census makes and
/// nothing else — `classification_does_not_depend_on_the_probe_window` is the
/// assertion, and it is why the name no longer says "cap".
///
/// It exists at all because this runs once per directory in a census: reading
/// the whole file to look for a newline that is not there is the one shape that
/// must stay cheap, and [`newline_offset_from`] handles it in a fixed-size
/// buffer that never grows.
///
/// (`PR5-CORRECTNESS-002`: as a *cap* this was a classification bound, and a
/// valid `run_started` past it was hidden from every reader.)
pub(super) const FIRST_LINE_WINDOW: u64 = 1 << 20;

/// The fixed buffer [`newline_offset_from`] scans through. Never allocated per
/// byte read and never grown, so a log with no newline at all — the shape the
/// window exists for — costs one stack buffer however large the file is.
pub(super) const SCAN_CHUNK: usize = 64 * 1024;

/// Classify one run directory. Read-only, and total.
#[must_use]
pub fn classify_run_dir(public: &Path) -> RunDirClass {
    match first_committed_line(public) {
        Some(_) => RunDirClass::Committed,
        None => RunDirClass::Husk,
    }
}

/// The header of a committed first line, or `None` if there is not one.
///
/// Deliberately not `events::started_of`: recovery step (a0) probes this
/// header and only *then* "select[s] the engine by schema", so classification
/// cannot be schema-specific — a schema-4 log must classify through the same
/// call as a schema-1 one, and each engine's own event type refuses the other's.
fn first_committed_line(public: &Path) -> Option<RunStartedHeader> {
    let path = public.join(EVENT_LOG);
    // `open(2)` runs *before* the read, and the read's bound cannot defend it
    // (`PR5-CONF-001`). `open` on a fifo with no writer blocks in the kernel and
    // never returns a handle at all, so `first_line`'s fstat bound — which is
    // taken on a handle this function has already been given — is not reached.
    // That is `PR5-RD-001`'s consequence one syscall earlier: `startup_census`
    // requires *every* entry to classify before a write command proceeds, and
    // the command holds the physical worktree lock across the census, so an
    // entry that never classifies is a lock held for ever.
    //
    // The guard is `symlink_metadata`, not `metadata`, and the difference is
    // deliberate. `stat(2)` on a fifo answers immediately, so either would
    // terminate; what following the link would leave open is a **swap of the
    // link's target** between the check and the open. Refusing the link itself
    // narrows the residual race to replacing a directory entry the census owns.
    // A symlinked `events.jsonl` is therefore a `Husk` whatever it points at,
    // which is this module's stance elsewhere (`:764`, `:1595`) and is the safe
    // direction: a husk is never deleted on shape alone — deletion additionally
    // requires the ownership proof, which requires `committed.json` to be
    // absent, and a run that reached `run_started` published one at P5b.
    if !fs::symlink_metadata(&path).is_ok_and(|entry| entry.is_file()) {
        return None;
    }
    let mut file = File::open(&path).ok()?;
    let line = first_line(&mut file)?;
    let line = std::str::from_utf8(&line).ok()?;
    let header: RunStartedHeader = serde_json::from_str(line).ok()?;
    (header.event == "run_started" && header.data.schema >= 1 && !header.data.run_id.is_empty())
        .then_some(header)
}

/// The bytes of the first newline-terminated line, without its newline, or
/// `None` when the file holds no newline at all.
///
/// The read is bounded by the file's **own length**, taken by `fstat` on the
/// handle that is about to be read (`PR5-RD-001`). Two properties have to hold
/// at once here and an earlier repair traded one for the other:
///
/// * **A committed run is never hidden.** `startup_census` defines `Committed`
///   as "`events.jsonl` exists and its first newline-terminated line is a valid
///   `run_started`" and states no size exception, so a first line past the
///   window must still be found. It is: the bound is the whole file, so the
///   scan reaches any newline a regular file actually contains, however far in.
/// * **Classification terminates.** Before this, the scan ran until a read
///   returned zero — which an endless source never does. A public run directory
///   whose `events.jsonl` was a symlink to `/dev/zero` was therefore never
///   classified at all, and since `startup_census` requires *every* entry to be
///   `Committed` or `Husk` before a write command proceeds, the command held
///   the worktree lock for ever. The file's own length is a bound the source
///   cannot argue with: a source that declares no length is read zero bytes.
///
/// The bound is the *read*, never the answer — the distinction the removed
/// `FIRST_LINE_CAP` got wrong.
///
/// It bounds a handle it is **given**, so it says nothing about how that handle
/// was obtained, and the earlier version of this comment overstated itself by
/// concluding "a device or a fifo … is a `Husk`" (`PR5-CONF-001`). That is true
/// of a device, whose `open` returns; it was never true of a writer-less fifo,
/// whose `open` does not. [`first_committed_line`] carries that half now, by
/// refusing to open anything that is not a regular file, and this function
/// still carries the endless-*device* half — both are measured together in
/// [`a_run_directory_whose_log_never_ends_is_still_classified`].
pub(super) fn first_line(file: &mut File) -> Option<Vec<u8>> {
    let bound = file.metadata().ok()?.len();
    first_line_within(file, bound)
}

/// [`first_line`] over any source, with the byte budget given explicitly.
///
/// Split out so the budget is a *value a test can supply* rather than a
/// property of a file a test would have to construct. The endless source the
/// production bound defends against is `/dev/zero`, which exists on one of the
/// two platforms this ships on and cannot be built at all on the other; over
/// this signature the same source is a twenty-line reader, so the termination
/// claim is measured on every host rather than on Linux only.
pub(super) fn first_line_within<R: Read + Seek>(source: &mut R, bound: u64) -> Option<Vec<u8>> {
    let mut window = Vec::new();
    source
        .by_ref()
        .take(FIRST_LINE_WINDOW.min(bound))
        .read_to_end(&mut window)
        .ok()?;
    if let Some(newline) = window.iter().position(|byte| *byte == b'\n') {
        window.truncate(newline);
        return Some(window);
    }
    // The cursor is at `window.len()`, so the scan continues from there rather
    // than re-reading what the window already proved newline-free, and spends
    // only what the window did not.
    let scanned = window.len() as u64;
    let length = newline_offset_from(source, scanned, bound.saturating_sub(scanned))?;
    source.seek(SeekFrom::Start(0)).ok()?;
    let mut line = Vec::new();
    source.by_ref().take(length).read_to_end(&mut line).ok()?;
    // A log that shrank between the scan and the re-read has no first line this
    // probe can vouch for; `Husk` is the safe direction.
    (line.len() as u64 == length).then_some(line)
}

/// The absolute offset of the first `\n` at or after `offset`, in constant
/// memory, or `None` when there is none within `budget` further bytes.
///
/// `source`'s cursor must already be at `offset`. The offset of the newline is
/// also the length of the line that precedes it, which is what the caller
/// wants.
///
/// **Termination**: every iteration either returns or spends at least one byte
/// of `budget`, which is finite. The single branch that spends nothing is
/// `Interrupted`, which is `std::io`'s own convention for "this read did not
/// happen" and which a regular file does not produce; treating it as an end
/// instead would classify a committed run as a husk, which is the direction
/// that must never be taken.
fn newline_offset_from<R: Read>(source: &mut R, mut offset: u64, mut budget: u64) -> Option<u64> {
    let mut chunk = [0_u8; SCAN_CHUNK];
    while budget > 0 {
        let want = usize::try_from(budget.min(SCAN_CHUNK as u64)).ok()?;
        // A short read is normal, not an end: only zero means end of file.
        let read = match source.read(&mut chunk[..want]) {
            Ok(0) => return None,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if let Some(at) = chunk[..read].iter().position(|byte| *byte == b'\n') {
            return Some(offset + at as u64);
        }
        offset += read as u64;
        budget -= read as u64;
    }
    None
}

/// The header of a committed first line: the envelope's tag, and the two
/// identifying fields inside its payload.
///
/// The wire is `{"ts": …, "event": "run_started", "data": {"schema": …,
/// "run_id": …, …}}` for every schema — `Event`/`TopologyEventBody` both tag on
/// `event` and both nest the record under `data`.
///
/// Unknown fields are allowed here and only here: this reads the *header* of a
/// line each schema's own type owns in full, and rejecting a schema-5 field
/// would classify a future run as a husk. What it does insist on is the shape
/// that makes the line a `run_started` at all — recovery step (a0) "probe[s]
/// the header of the committed first line" and then "select[s] the engine by
/// schema", so a line with no schema to select by is not one.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct RunStartedHeader {
    pub(super) event: String,
    data: RunStartedIdentity,
}

#[derive(Debug, Clone, Deserialize)]
struct RunStartedIdentity {
    schema: u32,
    run_id: String,
}

/// The digest of a `run_started` line's exact bytes, for the commit record.
///
/// `run_creation`: "run_started_sha256 = the digest of the exact run_started
/// line bytes about to be appended".
#[must_use]
pub fn run_started_sha256(line: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(line))
}
