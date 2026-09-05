---
id: PR101-CFG-ATTR-APPLIED-CFG-INVISIBLE-TO-THE-SCAN
severity: P2
disposition: deferred
category: correctness
pr: 101
reviewed_sha:
location: src/effects.rs:2207
provenance: pre_existing
first_bad:
guard: the slice that next widens scan_module_declarations in src/effects.rs
---

## Failure sequence

`scan_module_declarations` (`src/effects.rs:2207`) treats a `cfg_attr` as significant **only when its text contains `path`** — `"cfg_attr" if raw.contains("path") => pending_path = true,` at `:2282` — so `#[cfg_attr(all(), cfg(test))] mod hidden_tests;`, which rustc applies as `#[cfg(test)]` and compiles only under test, is read as an **unconditional** declaration and the file it names stays in every census's domain as production. A fixture call in that file then sits inside the production censuses, where it can mask the deletion of a real production call — the exact failure the skip sets exist to prevent

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next widens `scan_module_declarations` in `src/effects.rs`.

Predates W1 by months; no W1 or W2 diff touches it. Widening the scan to **decide** `cfg_attr` predicates changes what every census in the crate scans, and a measurement change gets its own review. Already recorded in the tree as a stated limit, with the measurement that established it, in `declared_whole_file_test_modules`' doc comment (`src/effects.rs:2022-2039`) — so a repair must update that paragraph in the same change or leave a comment describing a hole the code no longer has. Full derivation, venue and required evidence: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
