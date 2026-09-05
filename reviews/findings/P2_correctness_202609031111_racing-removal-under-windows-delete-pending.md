---
id: W2-WINDOWS-RACING-REMOVAL-DELETE-PENDING
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha:
location: src/runner/container.rs:1437
provenance: pre_existing
first_bad:
guard: project owner, undirected
---

## Failure sequence

`racing_removal` (`src/runner/container.rs:1437`) retries a removal `RACING_ACCESS_ATTEMPTS` times — `= 64` at `:404` — then returns `UpstrokeError::Io`. On the Windows guest it exhausts that budget against an R19 view directory under **delete-pending** semantics, at roughly **2%** of runs on a 16-vCPU guest. It is a defect in **production code**, not in the harness or the build box. **It is not concurrency and not Docker**: the guest has no Docker and its jobs never overlap — 123 executions, zero overlaps — so the contention hypothesis this programme carried through W1 is wrong, and this row supersedes every earlier characterisation

## What the change that takes this up should do

Owner, as the ledger records it: project owner, undirected.

**Two traps, both pointing at the wrong subsystem.** (1) A `failed to read <path>` message on that path means a **removal** failed: `UpstrokeError::Io` has one `Display` — `#[error("failed to read {}: {source}", .path.display())]` at `src/error.rs:23` — so read, write, create, sync and remove all render identically; the message names the `Display` impl, not the operation. (2) `0123456789abcdef` in those paths is the fixture constant `REPO_KEY_A` (`src/runner/container/census/tests.rs:89`), **not** an unset `CARGO_TARGET_DIR` slot key — dangerous precisely because that hex is the slot-pool trap's visual signature. **How to tell it from a compile break**: three Windows legs failing together is a compile error; `test (winguest)` alone on a `racing_removal` signature is this race. A rerun on this signature is legitimate and disclosed as such — the only one of §43's six CI signatures carrying that licence, and it has it because the mechanism is established. **Raising the 64 is not the fix** and is an infrastructure decision for the owner. Full derivation: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
