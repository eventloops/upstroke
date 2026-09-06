---
id: PR43-WINDOWS-TOPOLOGY-KILL-FINGERPRINT
severity: P3
disposition: deferred
category: correctness
pr: 43
reviewed_sha:
location: reviews/2026-08-28-windows-topology-kill-single-failure.md
provenance: undetermined
first_bad:
guard: project owner / the slice that next opens the Windows topology kill harness
---

## Failure sequence

One Windows run produced two topology kill-test failures together: a `git worktree prune` ran outside a repository after snapshot-add kill, and the retry helper exited 101 where the parent required an abort; whether they share a cause is unresolved, and no rate has been measured

## What the change that takes this up should do

Owner, as the ledger records it: project owner / the slice that next opens the Windows topology kill harness.

**Open as one unexplained run, not classified as a flake or regression.** Durable provenance, byte-exact assertion sites and the limits of the Windows abort oracle are in `reviews/2026-08-28-windows-topology-kill-single-failure.md`. Run `33169116985`, attempt 1, at `02b7399`; one of three same-source Windows jobs failed, an opportunistic observation rather than a designed rate. Exit 101 identifies a panic but discarded child output cannot show why, and the prune failure does not prove which process removed or invalidated the repository. This row fulfills the companion record's deferred §2 commitment after the PR #42 serialization boundary without merging the two messages into a guessed mechanism.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
