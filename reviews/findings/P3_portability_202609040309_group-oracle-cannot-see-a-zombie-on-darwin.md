---
id: PR125-CLOSE-GROUP-ORACLE-CANNOT-SEE-A-ZOMBIE-ON-DARWIN
severity: P3
disposition: deferred
category: portability
pr: 125
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/agent/proc.rs:506
provenance: pre_existing
first_bad: W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT; PR7-MACOS-PROCESS-GROUP-FLAKE
guard: deferred: one experiment on a Mac decides it (fork a child that exits at once, do not reap it, call getpgid on it); if it holds, the oracle needs…
---

## Failure sequence

`child_leads_its_own_group` asks `getpgid(pid) == pid` after the spawn returns and its doc says a zombie still answers -> on XNU `proc_find` excludes exited processes, so a shim child that exits before the parent's look answers ESRCH -> `every_role_reaches_the_containment_points_of_this_platform` fails "the child did not lead its own process group" on macOS, most often on the role whose child does the least work; a hypothesis for the standing row, untested without a Mac

## What the change that takes this up should do

deferred: one experiment on a Mac decides it (fork a child that exits at once, do not reap it, call `getpgid` on it); if it holds, the oracle needs a zombie-aware query, as `group_has_non_zombie_members` already passes the non-zero argument to `proc_pidinfo` for

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
