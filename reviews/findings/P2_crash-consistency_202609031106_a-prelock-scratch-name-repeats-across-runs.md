---
id: PR104-PRELOCK-SCRATCH-NAME-REPRODUCIBLE-ACROSS-RUNS
severity: P2
disposition: deferred
category: crash-consistency
pr: 104
reviewed_sha:
location: src/engine/topology/prelock/tests.rs:200
provenance: pre_existing
first_bad:
guard: the slice that next changes src/engine/topology/prelock/tests.rs
---

## Failure sequence

`Scratch::new` (`src/engine/topology/prelock/tests.rs:200`) names its root `upstroke-prelock-{tag}-{pid}-{ThreadId}`. Every component resets when the process does, so the name is **reproducible across runs**: a killed run leaves a root behind, and a later run that reuses the pid and gets the same thread id computes the same path. The allocator then **adopts** it silently rather than refusing — `create_private_dir` (`src/rundir.rs:634`) → `create_dir` (`:575`) → `fs::create_dir_all`, which succeeds on an existing directory

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next changes `src/engine/topology/prelock/tests.rs`.

Byte-identical across PR #104 and not called by it; the reviewer said so explicitly. **Worth recording because of what it is**: this is the precedent PR #104 was told to copy, on the strength of its measured success against leaking — 5050 `upstroke-prelock-*` roots by 2026-08-30 and none after, recorded in its own doc comment at `:181`. It is a good precedent for **reclamation** and it carries a defect in **allocation**, and the packet that copied it inherited both. Copying a precedent copies its weaknesses. Related: `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED`, the same allocation weakness ten times over in the file that copied this one. Full derivation: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
