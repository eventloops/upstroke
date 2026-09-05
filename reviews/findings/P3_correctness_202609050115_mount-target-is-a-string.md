---
id: SWEEP-CONFIG-PARSE-020
severity: P3
disposition: deferred     # the type is the parent's (row 54); recorded here so the parse.rs sweep does not claim §8 over a path it holds as text
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/config.rs:69
provenance: pre_existing
first_bad:
guard: queue row 54 (`src/config.rs`), or the change that first acts on `RunnerSelection.mounts`, whichever comes first
---

## Failure sequence

`[runner] mounts = [{ source = "/opt/toolchain", target = "/opt/toolchain" }]` -> `read_runner`
reads `target` into `RunnerMount.target: String` (the parent's documented decision: "a
container-side absolute path, so it is a `String` and not a `PathBuf`: it is never resolved on
this machine") -> the value is a path in the container's filesystem held as text, compared and
printed as text -> §8's rule that every path is `Path`, `PathBuf`, `OsStr` or `OsString` is
not met for this field, and the parse.rs sweep (PR #150) had claimed the file's only path was the
`source`. Nothing misbehaves today, because nothing acts on a mount
(`SWEEP-CONFIG-PARSE-009`); the debt is that a container path has no type, so nothing can
refuse a relative one, a Windows-shaped one, or one with a NUL, at the point it is read.

## What the change that takes this up should do

Give the container-side path a type — a `ContainerPath` newtype validated at construction
(absolute, POSIX-shaped, no NUL), which is what §8 asks of a path-valued input the moment one
exists — in `src/config.rs` beside `RunnerMount`, and read `target` through it in
`read_runner`. Land it with `SWEEP-CONFIG-PARSE-009`, whose relative-`target` refusal is
that constructor's first rule. Until then this row records the exception so no sweep claims §8
over the field.
