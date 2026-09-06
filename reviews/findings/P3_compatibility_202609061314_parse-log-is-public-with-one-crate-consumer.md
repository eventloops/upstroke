---
id: SWEEP-FOLD_PARSE-003
severity: P3
disposition: deferred     # the visibility belongs to the type's own sweep, and narrowing it is a SemVer decision
category: compatibility
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/parse.rs:6
provenance: pre_existing
first_bad:
guard: `src/topology/fold.rs`, queue row 40, which owns `TopologyFold`'s public surface and `pub fn replay` beside it
---

## Failure sequence

Not a runtime failure: a §5 question this file cannot settle alone.
`TopologyFold::parse_log` is `pub` on a `pub` type in `pub mod topology`, so it is
a published API of a published crate. It has exactly one production consumer,
`establish_stable_prefix` in `src/events/log.rs`, and a crate-wide census in
`src/events/log/tests.rs` (`FOLD_ENTRIES`) requires it to have exactly one — the
barrier is meant to be the only fold source for a topology write command. Nothing
outside the crate is a consumer: `examples/probe.rs` names neither `TopologyFold`
nor `parse_log`. §5 asks for `pub(crate)` for crate-internal collaboration and
`pub` only for a supported external contract, and that is what this is not.

Narrowing it is not this file's call. `pub fn replay` sits beside it in
`src/topology/fold.rs` with the same shape and the same single caller, the two are
the barrier's pair, and narrowing either is a SemVer-assessed change to a published
crate (`CODING_STANDARDS.md` §5: a public API change is assessed for SemVer impact
in review). The decision belongs to the sweep of the type, with both halves of the
pair in scope.

## What the change that takes this up should do

When `src/topology/fold.rs` (queue row 40) is swept, decide `TopologyFold`'s public
surface as a whole: whether `parse_log` and `replay` are a supported external
contract or crate-internal collaboration, and either narrow both to `pub(crate)`
with the SemVer impact stated, or record why the pair is public. If they are
narrowed, check the `FOLD_ENTRIES` census in `src/events/log/tests.rs` still names
what it means to name.
