//! The names the `rundir` funnels write on disk, as constants.
//!
//! The marker, the two private records and their staging siblings, the event
//! log and the frozen plan. They are consts rather than a literal at each call
//! site because the sites that have to agree on the byte string sit in
//! different files: the funnels in `src/rundir.rs` publish them, the classifier
//! in `classify.rs` and the ownership proof in `ownership.rs` read them back,
//! and `the_names_on_disk_are_the_names_the_packet_writes` in `tests.rs` binds
//! every const to its literal. A second test,
//! `the_event_log_and_the_plan_are_reached_through_their_constants`, fails when
//! a `RunPaths` accessor and its const disagree; it cannot witness the
//! substitution that put them in agreement, and its own doc says why.
//!
//! **One source of truth for `rundir`, not yet for the crate.** [`EVENT_LOG`]
//! and [`PLAN`] name the two files a command outside this module opens by path
//! rather than through `RunPaths`, and **seven production sites in five files**
//! still spell the byte string for themselves: five of `events.jsonl`
//! (`export.rs`, `validate.rs`, `engine/resume.rs`, `status.rs`, `capacity.rs`)
//! and two of `plan.normalized.json` (`export.rs`, `engine/resume.rs`). One
//! derivation, quoted the same way here, in `SWEEP-NAMES-002` and in the pull
//! request body. A rename of either const is therefore still a multi-file edit.
//! The six names above them are not: no production code outside `rundir` spells
//! any of the six as a path, and the one test that does
//! (`engine/topology/emit/tests.rs`, joining `committed.json`) is
//! `SWEEP-NAMES-003`.
//!
//! **What the proof consults, exactly.** `prove_private_half_ownership` reads
//! two of these back and parses each ([`MARKER`], then [`OWNER_RECORD`] at the
//! locator the marker names); *stats* a third, because conjunct 12 is
//! [`COMMIT_RECORD`]'s **existence** and never its content; and compares a lone
//! directory entry against [`MARKER_STAGED`] to tell a bare husk from a staged
//! one. Four names, three kinds of use — a reader replacing the stat with a
//! read would be weakening the deletion boundary, not tidying it.
//!
//! **What has product authority here, and what does not.** `DESIGN.md` §15's
//! run-directory drawing carries [`EVENT_LOG`] and [`PLAN`]. The other six
//! appear in no `design/` section at all, so this repository states no rule
//! about renaming them — and for these six a rename is not a refactor. A run
//! killed after old P5b still holds a file called `committed.json`; the proof
//! would stat the *new* name, find it absent, mint a deletion token on that
//! absence and delete a private half that had crossed the deletion boundary.
//! The missing compatibility contract is `SWEEP-NAMES-008`, deferred: it
//! belongs in §15 and `DESIGN.md` is the owner's.
//!
//! Nothing in `src/` stands in for that, and nothing here tries to. A retired
//! decision record establishes no product contract, and neither does
//! implementation or test code quoting one, so no citation of
//! `decisions.workspace_candidates.run_creation` appears above. Until §15 says
//! otherwise, the six byte strings below are pinned by
//! `the_names_on_disk_are_the_names_the_packet_writes` and by nothing with
//! design standing.

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

/// `<public>/.creating` — the P1 marker.
pub const MARKER: &str = ".creating";
/// `<public>/.creating.tmp` — the P1 staging file.
pub const MARKER_STAGED: &str = ".creating.tmp";
/// `<private>/owner.json` — the P3b reciprocal ownership record.
pub const OWNER_RECORD: &str = "owner.json";
/// `<private>/owner.json.tmp`.
pub const OWNER_RECORD_STAGED: &str = "owner.json.tmp";
/// `<private>/committed.json` — the P5b private commit record.
pub const COMMIT_RECORD: &str = "committed.json";
/// `<private>/committed.json.tmp`.
pub const COMMIT_RECORD_STAGED: &str = "committed.json.tmp";
/// `<public>/events.jsonl`.
pub const EVENT_LOG: &str = "events.jsonl";
/// `<public>/plan.normalized.json`.
pub const PLAN: &str = "plan.normalized.json";
