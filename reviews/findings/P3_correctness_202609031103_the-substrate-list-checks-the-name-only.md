---
id: PR103-CONTAINER-SUBSTRATE-LIST-CHECKS-NAME-ONLY
severity: P3
disposition: deferred
category: correctness
pr: 103
reviewed_sha:
location: src/runner/container/tests.rs:4883
provenance: pre_existing
first_bad:
guard: the slice that next changes the container removal census
---

## Failure sequence

`every_view_discard_removes_through_the_one_racing_removal` (`src/runner/container/tests.rs:4883`) excludes out-of-line test substrate by name through a `SUBSTRATE` const (`:4888`, six entries), and the only assertion over that list is `assert_eq!(excluded, SUBSTRATE.len(), …)` at `:4931` — a check that each name **is met**, not that each name **is still test substrate**. A listed file that later becomes production-reachable — compiled as a Cargo target, or declared unconditionally by a production parent — stays excluded and nothing notices. Failure sequence: add an `[[example]]` target whose `src_path` is a listed file, give it a `#[cfg(not(test))] main` reaching a governed primitive, and the census skips it

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next changes the container removal census.

Byte-identical before and after PR #103 and not activated by it. **A claim this finding used to carry is withdrawn**: it said PR #103 closed the same gap in its own better-guarded list, but #103 was **closed unmerged**, so that list never landed and the comparison has no second term — text describing code that does not exist, which is the failure mode §43 is written against. The two guards (an entry must not be a crate root, and must be a member of `cfg::WHOLE_FILE_TEST_MODULES`) remain the shape of the repair; they are implemented nowhere. Full derivation: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
