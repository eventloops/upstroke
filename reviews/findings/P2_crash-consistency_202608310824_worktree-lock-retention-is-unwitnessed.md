---
id: PR5-R2-WORKTREE-LOCK-RETENTION
severity: P2
disposition: deferred
category: crash-consistency
pr: 5
reviewed_sha:
location: coordinator.rs:93
provenance: undetermined
first_bad:
guard: PR6/PR7 implementer (the slice that can pause a run)
---

## Failure sequence

`PR5-RUNDIR-070`. The physical worktree lock is taken before the startup census and held for the whole run (`coordinator.rs:93` fresh, `resume.rs:108` on resume, both `let _worktree_lock = …` to end of scope). Dropping the guard immediately after the census is invisible: the two lease tests take a competing lease **first** and then check the run refuses, which exercises acquisition, not retention

## What the change that takes this up should do

Owner, as the ledger records it: PR6/PR7 implementer (the slice that can pause a run).

**Carried: the killing assertion needs a paused run and nothing in the suite pauses one.** "While run A is paused after census but before termination, a second write command for run B in the same physical worktree is refused; it succeeds only after run A releases its guard" needs a run held open across a second command — a coordinator seam PR5 does not own. `run_creation`'s "only then takes the physical worktree lock … holding it across the startup census and the whole run" is the live passage. Same shape as `PR4-R28-NEXT-COORDINATOR-UNWITNESSED`: a lifetime claim about a guard, unwitnessed because no fixture holds two coordinators

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
