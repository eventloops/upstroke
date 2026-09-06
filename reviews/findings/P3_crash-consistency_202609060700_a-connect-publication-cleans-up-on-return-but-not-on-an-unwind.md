---
id: SWEEP-CONNECT-002
severity: P3
disposition: deferred
category: crash-consistency
pr: 189
reviewed_sha: 45bb0725d9420226cd6e2a7f386a16250b46f913
location: src/connect.rs:208
provenance: introduced_by_feature
first_bad: f658080c
guard: deferred: `an_unwinding_publication_leaves_the_operators_file_byte_for_byte_intact` pins the property that matters (the destination is untouched) on this path; the leftover itself is unpinned deliberately, so that a later Drop guard is not a test to rewrite
---

## Failure sequence

`publish_pools` stages `.pools-<ULID>.tmp`, publishes it by rename, and removes it at one
cleanup point on every failing *return* -> a panic between the staging file's creation and its
publication unwinds past that point instead of through it -> the staging file stays in the
operator's `~/.upstroke/` until something else removes it. Nothing between those two points
panics today (the steps are `write_all`, `util::fsync_file` and the rename), so the sequence is
reachable only through the injected publication step a test supplies, or a future edit that puts
a panicking expression between them.

## What the change that takes this up should do

Own the staging file with an RAII guard whose `Drop` removes it, so the unwind is covered by the
same mechanism as the return, which is what §6 asks for ("cleanup happens on early return, error,
panic unwinding and cancellation as far as the platform allows") and what pass 1 of PR #189 asked
for in as many words.

**Why this pull request did not do it, measured rather than argued.**
`effects::externally_reachable_fns` collects every `fn` inside any `impl ... for ... { }` span
regardless of the implementing type's visibility, so an `impl Drop for Staged` in
`src/connect.rs` — even for a private type — adds `drop` to the set that
`effects::tests::every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified`
derives for this module. That test then fails with `src/connect.rs unclassified: ["drop"]` until
`"drop"` is added to `src/connect.rs`'s `[[module]]` row in `effects/wrappers.toml`
(`src/rundir.rs` carries exactly such a row for its own two `Drop` impls). The probe was run:
a throwaway `impl Drop` for a private type in this file's production region reproduces that
failure exactly. `effects/wrappers.toml` is outside the file scope of a standards-sweep pull
request, so the guard and its one-line classification belong to a change that may edit both.

The cost of leaving it is one uniquely named `.tmp` in the pools directory on a path production
cannot currently take. No reader of that directory interprets the name, `config::load` does not
read it, and the next publication cannot collide with it, because the name carries a fresh ULID.
