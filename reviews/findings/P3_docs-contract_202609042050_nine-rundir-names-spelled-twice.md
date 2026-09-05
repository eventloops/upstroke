---
id: SWEEP-NAMES-004
severity: P3
disposition: deferred
category: docs-contract
pr: 143
reviewed_sha: 7724ed1d628070b35948819095a68a38cd0c5d0a
location: src/rundir.rs:130
provenance: pre_existing
first_bad:
guard: queue row 19, the sweep of `src/rundir.rs`
---

## Failure sequence

The defect this pull request fixes for `EVENT_LOG` and `PLAN` — one file name,
two independent spellings in one file, one of them the accessor everything else
calls — is a class, and nine more names in `src/rundir.rs` are in it. None of
the nine is owned by `src/rundir/names.rs`, so fixing them was outside this
sweep's bound.

| Name | Spelt at | And again at |
|---|---|---|
| `report.json` | `src/rundir.rs:130` (`RunPaths::report_json`) | `src/rundir.rs:804` (`write_report`) |
| `artifacts` | `src/rundir.rs:48` (`PUBLIC_DIRS`, which `create_hooked` makes) | `src/rundir.rs:148` (`RunPaths::artifacts`) |
| `questions` | `src/rundir.rs:48` | `src/rundir.rs:139` |
| `answers` | `src/rundir.rs:48` | `src/rundir.rs:144` |
| `transcripts` | `src/rundir.rs:51` (`PRIVATE_DIRS`) | `src/rundir.rs:152` |
| `reviews` | `src/rundir.rs:51` | `src/rundir.rs:156` |
| `settings` | `src/rundir.rs:51` | `src/rundir.rs:160` |
| `gates` | `src/rundir.rs:51` | `src/rundir.rs:164` |
| `gate-worktrees` | `src/rundir.rs:51` | `src/rundir.rs:171` |

Nine names, eighteen spellings. For the eight directories the two spellings are
the creator and the reader of the same directory: `create_hooked` iterates
`PUBLIC_DIRS`/`PRIVATE_DIRS` to make them, and each accessor re-spells the one
it returns. Editing the array without editing the accessor gives a run whose
skeleton is created at one name and read at another.

`run.lock` is **not** in this class and is the shape the rest should reach:
`lock_file` (`src/rundir.rs:994`) holds the one spelling and `RunPaths::lock_file`
delegates to it.

## What the change that takes this up should do

Row 19's sweep decides between two shapes and states which, because they are not
equivalent:

* Move the nine into `src/rundir/names.rs` beside the eight already there, and
  build `PUBLIC_DIRS`/`PRIVATE_DIRS` from those constants so the array and the
  accessor cannot disagree. This widens that module's stated domain, which today
  is exactly "the marker, the two private records and their staging siblings, the
  event log and the frozen plan" — so the module doc changes with it.
* Or leave them in `src/rundir.rs` and have each accessor read its array element,
  which fixes the eight directories and leaves `report.json` needing a constant
  of its own either way.

Whichever, the guard is the one this pull request added for the other two:
assert the accessor against the name the creator used, not against a literal, so
the mutation that separates them is caught by a test that says so.
