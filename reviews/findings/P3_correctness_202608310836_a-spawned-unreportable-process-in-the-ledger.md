---
id: R3-SEAMS-006-ATT003-REPAIRED-POSTHOC
severity: P3
disposition: deferred
category: correctness
pr: 
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: project owner, if the residual is worth a row of its own
---

## Failure sequence

**Refuted as described, with a residual question that is not the same claim.** Sol's independent `seams` read, round 3: "a first reviewer whose Runner returns an error -> `run_review` reports `invocations: 0` -> the post-hoc loop performs no registration or cancellation", concluding R4 is not held on the error path. **Inspected `src/review.rs:786-797`, the `runner.run(&request)` match arm inside `run_review`'s invocation loop** — the item, the file and the lines, per §4's refutation rule. That arm does **not** return `Err`: it returns `Ok(unavailable_after_error("review process failed", error, cost, invocation - 1, last_path))`. So `judge` receives an outcome, the reconciliation **does** run, and it registers `invocation - 1` = 0 for a first pass. The described mechanism — an `Err` bypassing the loop — does not occur

## What the change that takes this up should do

Owner, as the ledger records it: project owner, if the residual is worth a row of its own.

**The residual, stated separately because it is a different claim and I nearly repaired the wrong one.** `unavailable_after_error`'s `invocation - 1` is "how many invocations *completed*", and a Runner error means none did — but the Runner may have **spawned** a process before failing. Whether a spawned-and-unreportable process belongs in the ledger is a real question about `permits.protocol`'s "registered exactly once"; it is not the question Sol asked, and the answer is not obviously yes, since registering one that never started is the opposite failure and is the reason the reconciliation is post-hoc at all. **What was almost shipped**: an error arm in `judge` registering and cancelling the pass, written against Sol's description before its reachability was checked. It compiled, the suite stayed green, and a witness built for it **failed** — `judge` returned `Ok` — which is what surfaced the refutation. Reverted rather than kept: an arm whose reachability is unestablished is the same defect as a function with no production caller, filed one commit earlier as this slice's most recurrent class

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
