---
id: SWEEP-GRAPH-009
severity: P3
disposition: deferred
category: correctness
pr: 167
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/plan/markdown/assemble.rs:134
provenance: pre_existing
first_bad:
guard: the `src/plan/markdown/` family sweep (`assemble.rs`), which owns `collect_artifacts`
---

## Failure sequence

A markdown plan in which two tasks declare the same artifact as output, and a third depends on the
second of them and needs the artifact:

```
## D1
<!-- upstroke: id=d1 out=contract depends= -->

## D2
<!-- upstroke: id=d2 out=contract depends= -->

## B
<!-- upstroke: id=b depends=d2 needs=contract -->
```

`collect_artifacts` walks the tasks in document order and records an `Artifact` for an `out=` only
when no artifact of that id exists yet, so `contract` is recorded as produced by `d1` and `d2`'s
claim is dropped, with no warning. `validate::graph::check_artifact_wiring` then reads the one
recorded producer and warns:

```
task `b` needs artifact `contract` produced by `d1` but does not depend on it (directly or transitively)
```

Measured through the adapter at the reviewed SHA (PR 167, Validation, `PROBE`): the parsed plan
carries one artifact, `contract` produced by `d1`; the adapter emits no warning; the graph check
emits the line above. The author wired `b` to the task that produces what it needs and is told the
opposite; and a plan in which two tasks genuinely produce the same artifact — a conflict the design
does not define an order for — passes without a word.

The graph check cannot repair this: a producer the adapter did not record is not in `plan.artifacts`,
and `Artifact.produced_by` holds one `TaskId`.

## What the change that takes this up should do

Decide, in `src/plan/markdown/assemble.rs`, what a second `out=` for one artifact means — most
likely a refusal or a warning naming both tasks, since `Artifact` has one `produced_by` and
`DESIGN.md` §9 gives the annotation no multi-producer meaning — and say it in `design/09`. If the
answer is "the last producer wins" or "every producer is a producer", `Artifact` changes shape and
`check_artifact_wiring` follows it. Pin the sequence above at the adapter, where the information
is lost, not at the graph check.
