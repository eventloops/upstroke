//! The names this module writes on disk, as constants.
//!
//! The marker, the two private records and their staging siblings, the event
//! log and the frozen plan. They are consts rather than literals at each call
//! site because a census, a funnel and a test all have to agree on the same
//! byte string: `run_creation` names each of these files and the ownership
//! proof reads three of them back.

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
