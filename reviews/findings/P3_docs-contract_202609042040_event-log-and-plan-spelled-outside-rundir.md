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
serves every site that must agree on it. **Seven production sites in five
files** outside `src/rundir` open those two files by spelling the byte string
for themselves rather than through the constant or through `RunPaths` — five of
`events.jsonl` and two of `plan.normalized.json`. This is the one derivation;
`src/rundir/names.rs`'s module doc and the pull request body quote it in the
same words.

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

**No mutation evidence is offered for this row, deliberately.** An earlier version cited the
`EVENT_LOG` mutation — 100 crate-suite failures at master against 92 on the branch — as this
finding's witness. That was wrong twice over and the citation is withdrawn rather than reworded:

* It mutates `EVENT_LOG` only, so it measures nothing at all about the two `plan.normalized.json`
  sites in the table above.
* The tests it newly reddens are **literal readers inside the suite** —
  `src/engine/topology/settle/tests.rs`'s `committed()` helper and `src/rundir/tests.rs:105` —
  not the seven production sites. Route all seven and those tests still fail, because they spell
  the byte string themselves. So the experiment demonstrates writer/test coupling; it does not
  isolate this census finding, and "the 92 that remain are the seven production spellings" was
  false.

That measurement belongs where it does witness something: the repair in `SWEEP-NAMES-001`, which is
where the pull request body now keeps it. This row stands on the census — seven sites, listed above,
each one greppable — and on nothing else.

`MAINTAINING.md`'s rule that a finding carrying a mutation witness is fixed whatever its severity is
the reason this matters rather than being a wording preference: citing that measurement here would
have obliged a fix of seven sites across five files outside `rundir`, which is a different pull
request. The row exists to say so.

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
