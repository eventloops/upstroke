//! Committed-or-Husk: the first-line probe `startup_census` classifies by.
//!
//! `sequential_substrate.startup_census`: "every entry is classified by
//! `rundir::classify_run_dir` as **Committed** (`events.jsonl` exists and its
//! first newline-terminated line is a valid `run_started`) or **Husk**
//! (anything else)". Read-only, and bounded rather than total: the census
//! holds the physical worktree lock across it, so an entry that never
//! classifies is a lock held for ever, and every bound here exists for that.
//! **What `startup_census` is, since this module cites it throughout.** It is a
//! sentence of the retired `sequential_substrate` packet. Neither `DESIGN.md`
//! nor any `design/` section states it at this SHA — measured, not assumed — so
//! every quotation of it in this file is the wording this module was built to
//! and this module's own reasoning for behaving that way, and none of it is
//! design authority. A citation below that reads as though the design required
//! something should be read as: this is what the module does, and this is why.
//! That the design carries no classification rule for a run directory is a gap
//! rather than a licence to invent one here; it is `SWEEP-CLASSIFY-013`, and
//! closing it is an owner-level design change rather than this pull request's.
//!
//! What is not bounded is named where it lives, and it is one syscall.
//! [`first_committed_line`] refuses to open anything whose *name* is not a
//! regular file, and the window between that check and the `open` is still
//! open, so a path swapped inside it for a writer-less fifo blocks in the
//! kernel before any bound on the read can apply.
//!
//! **Measured rather than restated, and then decided.** The block is real
//! (`SWEEP-CLASSIFY-003`, with the reproduction and its output in
//! `reviews/FINDINGS.md`). The usual close on Unix — open with `O_NONBLOCK` and
//! take the file type from the descriptor — reaches a governed primitive:
//! `clippy.toml` denies `std::fs::File::options` and the `std::fs::OpenOptions`
//! it hands out, and `libc::open` and `libc::fcntl` beside them.
//!
//! **That is not a bar to writing it here, and an earlier version of this
//! paragraph wrongly said it was.** `standards/02` admits a per-site
//! `#[expect]` of a governed lint below module level in a file whose
//! `effects/allowlist.toml` row records the lint and the exact annotation
//! count, and `src/effects/tests.rs`'s placement census requires that file to
//! **deny** the lint at module level — so this module's deny posture is the
//! mechanism's precondition rather than the thing an allowance would replace.
//! The close is available locally, at the cost of an allowlist row.
//!
//! It is deferred anyway, as a **preference and not a constraint**: a governed
//! primitive belongs in the funnel parent as a site-taking non-blocking
//! read-only open, and the round that would have added it here is the round a
//! frontier pass found a P1 inside the machinery this file's previous round
//! added — which is the standing signal to narrow rather than to reach for one
//! more mechanism. `SWEEP-CLASSIFY-003` carries that reasoning and the
//! measurement it rests on.

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
///
/// That is the *scan*'s memory, and not the probe's: a first line that is found
/// is then materialised at its own length by [`first_line_within`], which the
/// rule this module implements requires and this constant does not bound. See
/// that function.
pub(super) const SCAN_CHUNK: usize = 64 * 1024;

/// Classify one run directory. Read-only, and bounded rather than total.
///
/// The read is bounded; acquiring the handle is not. [`first_committed_line`]
/// refuses anything whose name is not a regular file, and a path swapped for a
/// writer-less fifo inside the window between that check and the `open` blocks
/// in the kernel — under the physical worktree lock the census holds across
/// this call. The module doc says what was measured, that the non-blocking open
/// which would close it *is* available here at the cost of an allowlist row,
/// and why the close is deferred to the funnel parent as a preference all the
/// same; this sentence asserts no totality it does not have.
///
/// **Every other way this answers `Husk` is bounded**, including the ones that
/// are failures rather than verdicts: an `events.jsonl` that cannot be stat'd,
/// opened or read is a `Husk`, which is `startup_census`'s "anything else" and
/// not a claim that nothing is there. What the census does with each of those
/// answers is in [`first_committed_line`].
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
///
/// **`None` is two different facts, and this signature holds one.** Two of the
/// folds below are honest absence: a first line that is not UTF-8 and one that
/// is not this header are not a valid `run_started`, which is what the packet
/// asks. The rest — the stat, the `open`, the fstat, the two reads and the seek
/// — fold an I/O failure the filesystem declined to explain into the same
/// `None`, so "I could not read it" and "there is nothing here" reach
/// `startup_census` as one answer. That is what `startup_census`'s "anything
/// else" licenses, and the paragraphs below say what the census then does with
/// it, but the *reason* is lost at this return and no caller can recover it.
/// Giving the census report a reason is `SWEEP-CLASSIFY-010`, a deferred row
/// against `src/engine/topology/startup.rs`: it is a shape this signature
/// cannot carry.
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
    // which is this subsystem's stance wherever the filesystem is undecidable or
    // a path is a link — `super::CommitRecordPresence::Unknown` is treated as
    // `Present` by every caller, and `super::ownership` refuses a locator chain
    // that passes through a reparse point and takes only `NotFound` as proof
    // that `committed.json` is absent.
    //
    // **What makes `Husk` the safe direction here is the directory listing, not
    // the commit record**, and the sentence this one replaces had that wrong. Of
    // `super::PrivateHalfOwnership`'s three answers, two reclaim. `Proven`
    // reclaims both halves and does require `committed.json` to be absent
    // (conjunct 12), which a run that reached `run_started` published at P5b.
    // `NothingBound` reclaims the **public** half through
    // `super::remove_public_husk` with no commit-record check anywhere on the
    // path — and it is what the proof answers as soon as the marker cannot be
    // read, which every committed run past P7 is, since P7 is the step that
    // removes the marker.
    // What stops it is `super::ownership`'s `unbound_shape`, which reclaims only
    // a bare directory or one holding the staging file alone: an `events.jsonl`
    // this probe failed to read is an entry in that listing, so the answer is
    // `RetainReason::MarkerlessWithContent`. Both halves are pinned by
    // `proof_cases`' "marker-less husk carrying run-scoped content" and "bare
    // public directory".
    //
    // The residual is that the listing is a *second* observation of a directory
    // this probe already failed to read once, and `super::read_dir_names` folds
    // two different failures into `[]` — the reclaiming answer. It answers `[]`
    // when `read_dir` itself fails, and its `.flatten()` drops an entry whose
    // iteration step fails, so a directory that opened but could not be walked
    // past `events.jsonl` also reads as bare. A transient whole-process failure
    // (`EMFILE`, `ENFILE`) reaches the `open` below, the marker read and that
    // listing at the same moment, and `remove_public_husk` lists the directory
    // again once it has passed. Neither fold is in this file; both are
    // `SWEEP-CLASSIFY-009`, deferred against `src/rundir.rs` and
    // `src/rundir/ownership.rs`, and `read_dir_names` is PR #139's subject
    // while this is written — do not repair it from here.
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
///
/// **Neither read terminates on a source that answers `Interrupted`, and that
/// is `SWEEP-CLASSIFY-001` — an open P1, not an oversight.** `read_to_end`
/// retries `Interrupted` inside `std::io` without limit and an interrupted read
/// spends none of a `Take`'s budget, so the probe does not return and the
/// census holds the physical worktree lock while it does not. Measured on this
/// box at rustc 1.85: five million interrupted reads with the `Take` limit
/// untouched, and master's own copy of this function did not return in 25
/// seconds against the same reader.
///
/// **A retry cap is the wrong repair and this pull request withdrew one.** An
/// exhausted cap has to answer something, and the only answers this signature
/// has are `Committed` and `Husk`; `Husk` is the *reclaiming* one, so a burst of
/// interruptions above the cap turns a run the source would have delivered into
/// one `list_runs` hides, resume refuses, and the reclaim path may delete. "I
/// could not tell" needs a classification that is neither, which needs
/// `classify_run_dir`'s signature to change, which reaches `RunDirEntry`,
/// `list_runs` and `list_husks`. The row carries the measurement and the
/// reasoning for whoever takes it.
///
/// **The scan is constant memory and the answer is not.** A source with no
/// newline in it costs one [`SCAN_CHUNK`] buffer however long it is, which is
/// the shape the window exists for; a first line that *is* found is then
/// materialised at its own length, because the rule this module implements
/// states no size exception and the parse needs the whole line. So the probe's
/// peak memory is
/// the length of the log's first line, and a census of a directory holding a
/// hostile one pays it. Bounding that is a decision about what the census may
/// spend rather than about what a first line is, so it is `SWEEP-CLASSIFY-012`,
/// a deferred row, rather than a bound invented here.
///
/// **Two reads, and the second is not assumed to agree with the first.** The
/// scan finds an offset in constant memory and then the line is re-read from
/// the start; a source that changed in between could hand the re-read bytes
/// that are not a first line at all. Every property the caller relies on is
/// therefore re-established on the re-read itself, at the site.
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
    // Re-read from the start and re-establish the whole contract on the bytes
    // this function is about to return, rather than carrying the scan's view of
    // a source that may have changed underneath it: `length + 1` bytes, the last
    // of them the terminator and none of the others one. A log that shrank has
    // fewer bytes; one rewritten so a newline lands earlier fails the third
    // check and one rewritten so it lands later fails the second. `Husk` is the
    // safe direction for all three, and it is the one the shrunk log already
    // took — the other two used to return bytes that were not a first line at
    // all, and the terminator requirement this module classifies by is exactly
    // what they broke.
    let want = length.saturating_add(1);
    source.seek(SeekFrom::Start(0)).ok()?;
    let mut line = Vec::new();
    source.by_ref().take(want).read_to_end(&mut line).ok()?;
    if line.len() as u64 != want || line.pop() != Some(b'\n') {
        return None;
    }
    (!line.contains(&b'\n')).then_some(line)
}

/// The absolute offset of the first `\n` at or after `offset`, in constant
/// memory, or `None` when there is none within `budget` further bytes.
///
/// `source`'s cursor must already be at `offset`. The offset of the newline is
/// also the length of the line that precedes it, which is what the caller
/// wants.
///
/// **This loop does not terminate on a source that answers `Interrupted`, and
/// no claim is made that it does.** Every iteration returns or spends a byte of
/// `budget` *except* the `Interrupted` retry, which spends nothing — so an
/// endless stream of them is scanned for ever. That is `SWEEP-CLASSIFY-001`,
/// which is the same defect [`first_line_within`]'s two `read_to_end` calls
/// have, in this module's own code rather than in `std::io`'s.
///
/// An earlier version of this doc asserted the termination and then named the
/// branch that made the assertion false, two clauses later. The behaviour here
/// is master's; only the claim is withdrawn. Treating `Interrupted` as an end
/// of file instead would classify a committed run as a husk, and so would
/// giving up after a fixed number of them: `Husk` is the reclaiming answer, and
/// the row says why that makes a cap the wrong repair.
fn newline_offset_from<R: Read>(source: &mut R, mut offset: u64, mut budget: u64) -> Option<u64> {
    let mut chunk = [0_u8; SCAN_CHUNK];
    while budget > 0 {
        let want = chunk_len(budget);
        // A short read is normal, not an end: only zero means end of file.
        let read = match source.read(&mut chunk[..want]) {
            Ok(0) => return None,
            // Clamped, so a `Read` implementation answering more than it was
            // given cannot index past the buffer or underflow the budget. No
            // claim is made about which implementations do that; the clamp
            // costs one comparison and the claim would have to be defended.
            Ok(read) => read.min(want),
            // Unbounded, and that is `SWEEP-CLASSIFY-001` rather than an
            // oversight: see this function's doc.
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

/// How much of the scan buffer a budget of `remaining` bytes may use.
///
/// The smaller of two `usize` values and not a conversion that can fail: the
/// `usize::try_from(..).ok()?` this replaces answered `Husk` for a case no
/// target this crate builds for can reach, which is a `?` that decided nothing
/// (§7).
fn chunk_len(remaining: u64) -> usize {
    match usize::try_from(remaining) {
        Ok(fits) if fits < SCAN_CHUNK => fits,
        _ => SCAN_CHUNK,
    }
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
