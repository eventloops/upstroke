---
id: PR64-CLEANUP-003-SCRATCH-PRECLEAN
severity: P1
disposition: deferred
category: crash-consistency
pr: 64
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: the project owner — the bound startup, recover and create migration follow-up
---

## Failure sequence

The predictable scratch helper meets another process's occupied root, recursively pre-cleans
it **before acquiring ownership**, and deletes that process's content. The sequence is live, not
hypothesised: the helper's name is predictable, the root can already be occupied when it runs, and
the pre-clean happens on the way in rather than after the claim succeeds.

## What the change that takes this up should do

Preserve an occupied root instead of pre-cleaning it: acquire ownership first, refuse when
the root is already owned, and stop discarding the cleanup result. No pre-clean and no discarded
cleanup result are both required — a repair that keeps either one keeps the sequence.

Read the disposition history before trusting any earlier note on this row. §32 marked it fixed by
reusing the same identifier for a distinct emit helper, which was a mislabelling; §33 corrected the
disposition back to deferred; §38 confirms it stays deferred and names it as one of the two rows
whose omission from §35's audit is why §38 exists — "an audit that does not count it is an audit
that would let it through".

Recorded in `reviews/FINDINGS.md` §33. The row carried no P-label; **P1** here is this migration's judgement, from a demonstrated cross-process deletion.
