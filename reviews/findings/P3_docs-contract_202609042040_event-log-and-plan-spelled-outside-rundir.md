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

Seven sites in five files, and the consequence differs per constant rather than
being one blanket claim:

* Rename **`EVENT_LOG`** and five sites go stale — `export.rs`, `validate.rs`,
  `engine/resume.rs`, `status.rs`, `capacity.rs` — each of which then looks for
  a log the run no longer writes.
* Rename **`PLAN`** and only two do, `export.rs` and `engine/resume.rs`.
  `status`, `validate` and `capacity` never open the frozen plan, so they are
  unaffected; an earlier version of this row said "either constant" and named
  all five for both, which is wrong.

Either way the omission surfaces at run time rather than as a compile error,
which is the property that makes this a finding at all.

**No mutation evidence is offered for this row, deliberately.** An earlier version cited the
`EVENT_LOG` mutation as this finding's witness. It is not one, and the citation is withdrawn rather
than reworded: that experiment mutates `EVENT_LOG` alone, so it measures nothing about the two
`plan.normalized.json` sites above, and the tests it newly reddens are literal readers inside the
suite rather than production sites — route all seven and those tests still fail. It measures
writer/test coupling, not this census.

The measurement itself is stated once, in the pull request body's Validation section, together with
what it does and does not witness. It is not restated here, and no number from it appears in this
row.

`MAINTAINING.md`'s rule that a finding carrying a mutation witness is fixed whatever its severity is
why this is a correction rather than a wording preference: citing that measurement here would have
obliged fixing seven sites across five files outside `rundir`, which is a different pull request.
The row exists to say so.

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
