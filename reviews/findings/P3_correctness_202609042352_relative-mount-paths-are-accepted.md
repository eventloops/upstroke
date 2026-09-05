---
id: SWEEP-CONFIG-PARSE-009
severity: P3
disposition: deferred     # the field is parsed and carried but nothing acts on it in this build
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/config/parse.rs:153
provenance: pre_existing
first_bad:
guard: the change that first acts on `RunnerSelection.mounts` (the container funnel, `src/runner/container.rs`), which must decide this before the first mount reaches a runtime
---

## Failure sequence

`[runner]` contains `mounts = [{ source = "cache", target = "models" }]` -> `read_runner`
checks only that `target` is not blank and `source` is not empty, so both are accepted ->
the selection is carried, and today nothing acts on it (`RunnerSelection.mounts` documents
"nothing in this slice acts on them", and INV-23's `RunnerPolicy` has no mount field) -> when
a runner does hand the list to a runtime, a relative `source` is not a host path at all to
Docker (`-v cache:/models` names a **volume** called `cache`, created empty on first use),
and a relative `target` is refused by the runtime as "mount path must be absolute" -> the
operator who wanted `./cache` on the host gets an empty volume, with the config reading as
though a directory had been mounted.

A relative `source` has no defined base in this engine either: the process's working
directory is not the repository root (`config::load` says so for discovery), and no design
sentence names one.

## What the change that takes this up should do

Refuse, at parse, a `source` that is not absolute (`Path::is_absolute`, which is
platform-shaped: a `/host/path` fixture passes on Linux and fails on Windows, so the test
routes its fixture through the platform helper) and a `target` that is not an absolute
container-side path (a leading `/`; the container is Linux by the design's own statement
that the repository lives WSL-side on Windows). Say which in the refusal. Land it in the same
change that gives mounts an effect, and add the two shapes to
`the_runner_section_refuses_every_shape_it_cannot_act_on`.
