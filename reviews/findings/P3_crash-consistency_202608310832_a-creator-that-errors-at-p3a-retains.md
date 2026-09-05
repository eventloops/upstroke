---
id: PR7-P3A-CREATOR-RETAINS
severity: P3
disposition: accepted-risk
category: crash-consistency
pr: 7
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR7/PR12 implementer
---

## Failure sequence

A creator that errors at exactly P3a has no owner record, so `prove_private_half_ownership` mints no `PrivateHalfProof`; the creator therefore removes **neither** half, and the startup census retains and reports both. The packet's deletion boundary is satisfied, but an operator sees two retained directories where the failing step created one usable pair

## What the change that takes this up should do

Owner, as the ledger records it: PR7/PR12 implementer.

**Accepted risk, and the alternative is worse.** ST-19 tables this shape as content-free by ordering — nothing has been written into either half at P3a — and `creator_error_at_p3a_retains_both_halves_and_reports_them` covers both windows, so the behaviour is asserted rather than incidental. Removing the retention needs a second constructor for `PrivateHalfProof`, and that type's **single-constructor property is compile-fail-tested**: the proof exists precisely so that no path can delete a private half without having proved it owns it. Trading a compile-time guarantee for a tidier failure directory is the wrong direction, and the retained pair is reported, not silent

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
