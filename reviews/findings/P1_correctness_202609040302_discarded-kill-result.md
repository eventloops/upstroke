---
id: PR125-CLOSE-DISCARDED-KILL-RESULT
severity: P1
disposition: deferred
category: correctness
pr: 125
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/agent/proc.rs:2320
provenance: pre_existing
first_bad: 6798089
guard: deferred: the end of a helper reports what kill returned (0, ESRCH, EPERM) and what each waitpid returned, and nothing else; a kill that failed is…
---

## Failure sequence

every one of the five sites writes `let _ = libc::kill(pid, libc::SIGKILL)` -> a sandbox or LSM answers EPERM, or an ESRCH race lands -> the helper is not signalled and may stay in its pre-READY `open` or `close` holding its cleanup lease, while the caller proceeds as if it were dead; a bounded end that then reports "sent SIGKILL" and "left running with the signal pending" invents a history that was not observed, which is what pass 8's second P1 found in the closed pull request

## What the change that takes this up should do

deferred: the end of a helper reports what `kill` returned (0, ESRCH, EPERM) and what each `waitpid` returned, and nothing else; a `kill` that failed is a distinct outcome the READY failure message carries, and §7 forbids discarding the result of a signal the caller depends on

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
