---
id: PR40-PROGRAM-PUBLIC-ADAPTER-SEAM
severity: P2
disposition: deferred
category: compatibility
pr: 40
reviewed_sha:
location: src/lib.rs
provenance: undetermined
first_bad:
guard: project owner, carried by G2 W4
---

## Failure sequence

**The `CommandSpec.program` closure is scoped to this repository's adapters and does not reach the crate's public construction seam.** `decisions/2026-08-25-commandspec-program-stays-string.md` closes `PR4-PROGRAM-PATH-NOT-UNICODE` on the evidence that `Invocation::at` is `#[cfg(test)]` and `Invocation::named` takes a `&str`, so no adapter *in this repository* puts a path in the field. That audit does not reach `AgentAdapter`, which is public: `src/lib.rs` declares `pub mod agent`, `src/agent/mod.rs:194` declares `pub trait AgentAdapter` whose `build` returns a data-only `CommandSpec`, and `src/engine/mod.rs:83` declares `pub fn run_with(opts, adapters: &dyn AdapterSource)`. Failure sequence: a downstream crate implements `AgentAdapter` -> it is configured with a Unix agent path whose bytes are not valid UTF-8, such as `/opt/agent-\xff/claude` -> `build` must place that path in `program: String` -> `to_str()` returns `None` and the boundary refuses, or `to_string_lossy()` substitutes `U+FFFD` -> the run refuses with a diagnostic naming no missing installation, or the runner spawns a path that names a different file. `agent::bin::tests::a_program_path_a_string_cannot_carry_is_refused_by_name` guards `Invocation` and places no constraint on a direct `CommandSpec` construction

## What the change that takes this up should do

Owner, as the ledger records it: project owner, carried by G2 W4.

**Accepted as real and deferred, not fixed — owner disposition 2026-08-29.** Found by the frontier review of `7cf4f9971e2b4a8712ca7afa11e129c734921173`, verdict CHANGES_REQUIRED. Deferred deliberately: the repair is a decision about whether the public adapter seam may carry a path at all, which is a question about the shipped API rather than a defect in the documents this pull request lands, and W4 is the venue that owns `CommandSpec.program`. **Revisit at G2 W4**, and sooner if an adapter outside this repository is written against `AgentAdapter`, or if a path-valued adapter or configuration input is added. Until then the closure this pull request lands is to be read as scoped to this repository's adapters, never as a statement about the type

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
