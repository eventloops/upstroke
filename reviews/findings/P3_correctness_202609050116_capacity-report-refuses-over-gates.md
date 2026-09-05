---
id: SWEEP-CONFIG-PARSE-021
severity: P3
disposition: deferred     # src/capacity.rs is a flat module not yet on the sweep queue
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/capacity.rs:799
provenance: pre_existing
first_bad:
guard: the sweep of `src/capacity.rs` when it joins the queue, or the next change to `capacity::report`
---

## Failure sequence

`upstroke capacity` -> `capacity::report` loads the repo config through `config::load`, which
is the `EngineLimits::Fresh` reading -> the load refuses whatever a run being created now would
refuse, although the report reads only `[pools]` and the latest run's events -> a repo whose
`upstroke.toml` says `[engine] max_parallel = 4` has had no capacity report since that refusal
landed, and since PR #150 the same is true of a `[[gates]]` entry with an unknown key or a
repeated name, even where the run those gates were recorded for resumes fine (its resume takes
the `SequentialResume` reading and warns).

Not a stranded run — nothing about a run is at stake, and the operator fixes the file — but a
read-only report refusing over a section it never reads is the composition the PR #150 review
named for gates, one command over.

## What the change that takes this up should do

Decide what reading a report should take: `load_limits` with `SequentialResume` is the
nearest existing one (warn on what a fresh run refuses, continue), or a reading of the pools
file alone if the repo config contributes nothing the report prints. Test it with a repo config
that a fresh run refuses and assert the report is produced with a warning.
