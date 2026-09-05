---
id: W2-EXPECTED-REFS-COUNT-STALE-AFTER-EXTRACTION
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha:
location: src/effects.rs:1370
provenance: pre_existing
first_bad:
guard: the packet that next holds the pin-maintenance grid lock for its own reasons
---

## Failure sequence

`production_calls`' doc comment (`src/effects.rs:1370`) asserts *"Measured on this tree: `workspace_manager.rs` carries four occurrences of the substring `expected_refs(`"* and then reasons from that number. **The root file carries one**; the other three moved to `src/workspace_manager/tests.rs` when W1 extracted the test region, and every other file under `src/workspace_manager/` carries none. The number is right about the **subsystem** and wrong about the **file it names**, which is why it survived: a reader who recounts across the directory reproduces "four" and moves on

## What the change that takes this up should do

Owner, as the ledger records it: the packet that next holds the pin-maintenance grid lock for its own reasons.

Already stale before W2 began — W1's extraction caused it, and no W2 packet causes or worsens it; the steward checked both directions before proposing it. **The repair must not be another count.** Whoever makes it should state the property the sentence exists to make — that a substring needle is satisfied by a longer identifier — rather than re-measure a number the next extraction falsifies again; a count in prose beside a list that moves is the same hazard this row is an instance of. Full derivation, both files, two engines: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
