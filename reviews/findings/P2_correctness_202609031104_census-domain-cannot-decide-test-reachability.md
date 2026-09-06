---
id: PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY
severity: P2
disposition: deferred
category: correctness
pr: 103
reviewed_sha:
location: src/effects.rs:1742
provenance: pre_existing
first_bad:
guard: the slice that next changes CrateRoots or declared_whole_file_test_modules in src/effects.rs, or W3 when it takes up the deferred registry extraction
---

## Failure sequence

Two gaps in `census_domain`, each established by a separate frontier pass. **(1) Target kind is discarded**: `CrateRoots` (`src/effects.rs:1742`) keeps a `package_dir` and a `BTreeSet<PathBuf>` and nothing else — its doc comment states the choice, *"Kinds are **not** filtered"* — so a `[[test]]` root, which Cargo compiles with `cfg(test)` on and which can therefore be exclusively test code, is indistinguishable from a `[[bin]]` or `[[example]]` root. **(2) Non-test declarations are ignored**: `declared_whole_file_test_modules` (`:2050`) skips every declaration that is not test-only (`:2076`), so membership proves *"some test declaration resolves here"* and never *"only test declarations reach here"*. The reviewer's sequence: a `#[cfg(test)] mod fixture;` whose file is also declared unconditionally by a binary root that calls it — production-reachable, and invisible to the resolver

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next changes `CrateRoots` or `declared_whole_file_test_modules` in `src/effects.rs`, or W3 when it takes up the deferred registry extraction.

Not confined to the closed pull request that found it: **two shipped censuses derive their skip sets from this resolver at `ae2a58f`**, both adopted under `PR7-R5-ATT-001` — **an attestation key carried in the source, not a row in this file**; it resolves at `src/effects/tests/source_oracles.rs:1569`, `src/runner/mod.rs:1456`, `src/events/log/tests.rs:3412` and twice in `src/engine/topology/recover/tests.rs`, and a reader should not look for a ledger row of that name — `runner::tests::production_sources_by_path` (`src/runner/mod.rs:1458`) and the fold census (`src/events/log/tests.rs:3414`) — so both carry the blind spot on `master`. The shape of the repair is recorded so it need not be re-derived: retain target kind, and add a query for "is this path declared unconditionally anywhere in the walk"; neither changes what `whole_file_test_modules` returns, which is what killed #103's round 2. Full derivation: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
