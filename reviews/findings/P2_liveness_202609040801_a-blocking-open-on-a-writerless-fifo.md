---
id: SWEEP-CLASSIFY-003
severity: P2
disposition: deferred
category: liveness
pr: 137
reviewed_sha: 5f661fa7f8d5c45471cc33746a70df1cd192c61e
location: src/rundir/classify.rs:139
provenance: pre_existing
first_bad: 7a83e69
guard: deferred as a preference, not a constraint, and the row said constraint until pass 1 corrected it (PR137-CLASSIFY-FIFO-DEFERRAL-FALSE-CONSTRAINT)
---

## Failure sequence

`first_committed_line` takes the file type from the *name* and then opens it, so the window between the two is open -> a path swapped inside it for a writer-less fifo blocks in `open(2)` before any bound on the read applies -> the census never classifies that entry and holds the physical worktree lock for ever; measured on this box at Linux 6.8.0-137-generic, where `std::fs::File::open` on a writer-less fifo was still blocked when a 5-second timeout killed it (exit 124) and the same open with `O_NONBLOCK` returned in 3.737 microseconds

## What the change that takes this up should do

deferred as a **preference, not a constraint**, and the row said constraint until pass 1 corrected it (`PR137-CLASSIFY-FIFO-DEFERRAL-FALSE-CONSTRAINT`). Every non-blocking open is a governed primitive here -- measured: clippy's disallowed-method error for `std::fs::File::options` and its disallowed-type error for `std::fs::OpenOptions`, both citing this module's own `#![deny(` as the level -- but that does not make the repair impossible. A per-site `#[expect]` below module level is admitted in a file whose `effects/allowlist.toml` row records the lint and the exact annotation count, and the placement census in `src/effects/tests.rs` requires that file to deny the lint at module level, so this module's posture is the mechanism's precondition rather than its casualty. What remains is a preference: a governed primitive belongs in the funnel parent as a site-taking non-blocking read-only open, and the round that would have added it here is the round a pass found a P1 inside the machinery the previous round added. The reproduction is the durable half; `a_run_directory_whose_log_blocks_on_open_is_still_classified` keeps the guard that narrows the window

Recorded by the PR #137 pass over `src/rundir/classify.rs`; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
