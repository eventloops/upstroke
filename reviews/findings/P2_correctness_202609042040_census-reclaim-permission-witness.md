---
id: PR139-CENSUS-WITNESS-BLOCKED-BY-GOVERNANCE
severity: P2
disposition: deferred
category: correctness
pr: 139
reviewed_sha: cc202c81ab289ec74ee6ee527534bb543735a7f8
location: src/engine/topology/startup/tests.rs:1025
provenance: pre_existing
first_bad:
guard: a decision on the effect governance, then either an `effects/allowlist.toml` row for the startup census suite or a seam that lets a test make a directory unlistable without the primitive
---

## Failure sequence

**Narrowed.** This finding was first written as "the startup census's reclaim path cannot be
witnessed end to end". That is no longer true: PR #139 now asserts the composition itself —
`scan_plans_a_retention_for_a_listing_that_did_not_answer_and_a_reclaim_for_a_bare_husk` drives the
real `scan` and the real `apply`, kills the mutation that special-cases `ListingUnreadable` into a
reclaim, and needs no privileged primitive, because a public directory that does not exist answers
`NotFound` to every probe and reaches the same refusal. Someone found a way through, so the finding
narrows rather than staying open at its original width.

What remains blocked is the **permission-based** variant, and it is the one that reproduces the
operator-visible shape:

1. `src/engine/topology/**` is a `TOPOLOGY_MODULE` and may carry no module-level allow of a
   governed lint. `clippy.toml` denies `std::fs::set_permissions`, and an allow may live only as a
   module-level attribute in a file listed in `effects/allowlist.toml`.
2. The census reaches `Planned::ReclaimPublicOnly` on an unreadable listing under a *selective*
   failure: the classification probe's `open` refused, `run.lock` still openable by name so
   `is_running` answers free, and `read_dir` refused. On Unix that shape is a public directory
   carrying `--wx`, and mode bits are the only way to build it.
3. So that fixture needs `fs::set_permissions` inside the module that owns `scan` and `apply`,
   which the rule forbids.
4. The fixture was written and run anyway, outside the tree: with `unbound_shape`'s listing arm
   reverted to `unwrap_or_default()`, it reported `the census planned ReclaimPublicOnly(Bare) on a
   listing it could not read and the outcome was ReclaimedPublicOnly(Bare); the committed log is
   GONE`.

The same rule blocks a second witness in the same suite, and that one is already named in the tree:
`locator_through_reparse_point_retained` cannot plant its link. Two blocked witnesses in one suite
is what makes this a rule to decide about rather than a fixture to work around.

## What the change that takes this up should do

Someone with authority over the effect governance decides between:

- **An `effects/allowlist.toml` row** for `src/engine/topology/startup/tests.rs`, allowing
  `clippy::disallowed_methods` and re-denying the other two, on the model of
  `src/runner/container/census/tests.rs` and `src/runner/container/tests.rs` — which are
  `TOPOLOGY_MODULES` under `src/runner/` and carry exactly that allow, so the decided case already
  exists in the tree. The cost is that the topology census suite stops planting every fixture
  through a funnel, which is the property its header claims.
- **Or a seam** that lets a test make a directory unlistable without naming the primitive — the
  shape of `remove_tree_once_handles_close`'s attempt counter, a `#[cfg(not(test))]` no-op in
  production. The cost is a seam in the census for a fixture's benefit.

Whichever is chosen, the witness to restore is the one already measured: drive `scan` under the
selective failure, restore the permissions, drive `apply`, and assert both that the plan was not
`Skip` and that the committed log survives. Do not rebuild it as a `cfg(unix)` fixture that plants
a non-UTF-8 filename — that is a different fixture with a different platform bound, and PR #139's
`PR139-MACOS-REJECTS-THE-FIXTURE` row records what that costs.
