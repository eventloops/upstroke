---
id: PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK
severity: P2
disposition: deferred
category: correctness
pr: 107
reviewed_sha:
location: src/runner/container/tests.rs
provenance: pre_existing
first_bad:
guard: the slice that next changes the child-lint census
---

## Failure sequence

The child-lint census in `src/runner/container/tests.rs` derives its domain by walking each funnel's directory — `const FUNNELS` (`:3146`), then `let arm = walk(&directory);` (`:3170`) — so **a `#[path]` relocation is invisible to it by construction**. M4's repairs are on `master` and are correct: `assert_eq!(with_children, FUNNELS.len())` (`:3183`) and a per-arm `assert!(!arm.is_empty())` (`:3171`), stated over the class. They do not reach this variant. At `ae2a58f` the walk finds **38 children, 16 of them named individually and 22 named by nothing but the walk**; relocating the 22 with `#[path]` less one file kept per arm leaves **20 ungraded with every assertion still green** — union 18 over a floor of 9, `with_children` 5, no arm empty, all 16 named files present

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next changes the child-lint census.

Pre-existing at `1cbdccd`; neither M3's nor M4's split activates it or makes it worse, and a mechanism change to a census gets its own review. **By-name pinning is not the answer** — it catches this only if a pinned file happens to be a relocated one, and the pinned count has gone 1 → 6 → 16 across three packets each adding its own. **The prescription, so a repair need not re-derive it: derive the domain from the module declarations rather than from a directory walk**; the repository already holds the pattern in `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`. Derived independently twice, by #110's reviewer and by M4's steward. Full derivation with its command: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
