---
id: SWEEP-NAMES-002
severity: P3
disposition: deferred
category: docs-contract
pr: 143
reviewed_sha: 7724ed1d628070b35948819095a68a38cd0c5d0a
location: src/export.rs:140
provenance: pre_existing
first_bad:
guard: the sweeps of `src/export.rs`, `src/status.rs`, `src/capacity.rs`, `src/validate.rs` and `src/engine/resume.rs`
---

## Failure sequence

`src/rundir/names.rs` exports `EVENT_LOG` and `PLAN` so that one byte string
serves every site that must agree on it. Five production sites outside
`src/rundir` open those two files by spelling the byte string for themselves
rather than through the constant or through `RunPaths`:

| Site | Spelling |
|---|---|
| `src/export.rs:140` | `public.join("events.jsonl")` |
| `src/export.rs:181` | `public.join("plan.normalized.json")` |
| `src/validate.rs:322` | `crate::rundir::public_dir(repo_root, &run_id).join("events.jsonl")` |
| `src/engine/resume.rs:61` | `public.join("events.jsonl")` |
| `src/engine/resume.rs:144` | `public.join("plan.normalized.json")` |
| `src/status.rs:125` | `public.join("events.jsonl")` |
| `src/capacity.rs:811` | `crate::rundir::public_dir(&opts.repo_root, &run_id).join("events.jsonl")` |

Seven sites in five files. So a rename of either constant is a multi-file edit
whose omissions surface as a run whose log the exporter, `status`, `resume`,
`validate` and the capacity read cannot find, rather than as a compile error.

Measured at `7724ed1d628070b35948819095a68a38cd0c5d0a`, mutating `EVENT_LOG` to
`"events.jsonlx"` and running `cargo test --lib`: 92 failures, against 100 at
master `74537be`. Routing `rundir`'s own two accessors through the constants
closed 8 of those; the remaining gap is these five files. Nothing is broken
today — the strings agree — and the drift is loudly caught if it ever happens,
which is why this is P3 and not higher.

`src/main.rs:380` also spells `"plan.normalized.json"`, for `upstroke validate
--emit-json`. That is a **different file**: it is written into the current
working directory, not into `<public>/`, and it is deliberately not covered by
`PLAN`. A sweep that routes it through the constant would be binding two
unrelated files to one name.

## What the change that takes this up should do

Each of the five files, in its own sweep: replace the literal with
`rundir::EVENT_LOG` / `rundir::PLAN`, or — where the site already holds both
halves of the run directory — build a `RunPaths` and call `events()` /
`plan_json()`, which is what the accessors are for. Whichever, say in that
pull request's body which of the seven sites moved, so the count in
`src/rundir/names.rs`'s module doc can be corrected in the same breath; that
doc names the five files and will be stale the moment one of them lands.
