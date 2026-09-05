---
id: SWEEP-CONFIG-PARSE-021
severity: P3
disposition: deferred     # src/capacity.rs is a flat module not yet on the sweep queue
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/capacity.rs:799
provenance: undetermined   # the fresh reading is master's; the [interaction] and [[gates]] refusals it now applies are PR #150's
first_bad:
guard: the sweep of `src/capacity.rs` when it joins the queue, or the next change to `capacity::report`
---

## Failure sequence

`upstroke capacity` -> `capacity::report` loads the repo config through `config::load`, which
is the `EngineLimits::Fresh` reading -> the load refuses whatever a run being created now would
refuse, although the report reads only `[pools]`, `[routing.strategy] mode` (printed as the
report's `strategy`) and the latest run's events -> a repo whose `upstroke.toml` says
`[engine] max_parallel = 4` has had no capacity report since that refusal landed (master's
behaviour); since PR #150 the same is true of an unknown key in `[interaction]`, an unknown key
in `[engine]`, and a `[[gates]]` entry with an unknown key or a repeated name — refusals that
pull request introduced — even where the run those gates were recorded for resumes fine under
its own reading.

Not a stranded run — nothing about a run is at stake, and the operator fixes the file — but a
read-only report refusing over sections it never reads is the composition the PR #150 review
named for gates, one command over, and PR #150 widened the set of files it refuses.

## What the change that takes this up should do

Decide what reading a report should take. It cannot be "the pools file alone": the report
prints `strategy.mode`, so the repo config is read for that one key. The nearest existing
reading is `load_limits` with the resume reading, which warns where a fresh run refuses and
continues; a dedicated report reading that parses `[pools]` and `[routing.strategy]` and warns
on everything else is the precise answer. Test it with a repo config that a fresh run refuses
(each of the shapes above) and assert the report is produced, its `strategy` is the file's, and
a warning names what was refused.
