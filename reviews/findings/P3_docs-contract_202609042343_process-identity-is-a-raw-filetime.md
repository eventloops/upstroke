---
id: SWEEP-AMBIENT-011
severity: P3
disposition: deferred
category: docs-contract
pr: 147
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/agent/proc/ambient.rs:148
provenance: pre_existing
first_bad:
guard: the sweep of `src/agent/proc.rs` (queue row 51), which owns `windows_job` and the representation; the pid-identity question is the open owner decision `PR125-CLOSE-PID-IDENTITY-UNDER-A-HOST-WILDCARD-WAITER`, and this row does not reopen it
---

## Failure sequence

`process_alive(pid: u32, creation_time: u64)` takes the identity half of a Windows process as a
bare `u64` — a raw `FILETIME` — and `process_creation_time` returns one -> any `u64` is assignable
there: a duration, a byte count, the wrong column of a helper's whitespace-split record
(`runner::host::tests::one_crash_cycle` parses `pid` then `created` from such text) -> the mix-up
compiles, and `process_alive` answers `false` for it, which every caller reads as "the process is
gone" — the answer that passes a containment test. §5: identifiers and timestamps get a dedicated
type wherever a mix-up is possible. No mix-up is known; this is the type not ruling one out.

## What the change that takes this up should do

A `ProcessCreationTime(u64)` newtype — or `(pid, creation)` as one `ProcessIdentity` —
constructed only by `windows_job::creation_time` and consumed by `process_alive`, with the two
wrappers in `ambient.rs` re-typed. **Not done in the `ambient.rs` sweep, deliberately:**
`process_alive` is how a Windows helper is addressed, the family's pid-identity question is an
open owner decision, and the brief for that row said to say so and stop. The representation lives
in `windows_job` (row 51), and the change touches Windows-only tests in three files that no Linux
run can exercise.
