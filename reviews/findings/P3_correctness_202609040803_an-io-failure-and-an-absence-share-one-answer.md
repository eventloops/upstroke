---
id: SWEEP-CLASSIFY-010
severity: P3
disposition: deferred
category: correctness
pr: 137
reviewed_sha: 5f661fa7f8d5c45471cc33746a70df1cd192c61e
location: src/engine/topology/startup.rs:807
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to src/engine/topology/startup.rs, which owns the census report: startup_census defines the classification as Committed or "anything…
---

## Failure sequence

five of the folds in `first_committed_line` turn an I/O failure the filesystem declined to explain into the same `None` as an honest absence -> `classify_run_dir` returns `Committed` or `Husk` and has nowhere to put the difference, and the census stores only that answer in `RunDirEntry` -> an operator whose `events.jsonl` could not be read is told the run "never recorded a committed run_started", `list_runs` drops it and resume refuses; nothing is deleted, because the directory listing retains it

## What the change that takes this up should do

deferred to `src/engine/topology/startup.rs`, which owns the census report: `startup_census` defines the classification as `Committed` or "anything else", so the reason belongs beside the class in `RunDirEntry` rather than inside the binary answer, and adding a third class here would contradict the packet. `first_committed_line` states which folds are honest absence and which are not, at the return that loses the difference

Recorded by the PR #137 pass over `src/rundir/classify.rs`; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
