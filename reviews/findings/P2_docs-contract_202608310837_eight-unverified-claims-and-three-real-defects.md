---
id: PR7-R4-CLAIMS-UNVERIFIED
severity: P2
disposition: deferred
category: docs-contract
pr: 7
reviewed_sha:
location: attempt.rs
provenance: undetermined
first_bad:
guard: project owner — the claims protocol a fresh session carries
---

## Failure sequence

**Eight claims written into commit messages and doc comments of the round-3 repairs are false, and each is one `grep` from disproof.** Round 4 — five lenses over the six commits `0cd2001..040a100`, scoped to that diff alone — returned **27 findings, every one inside it**, on a head green on Linux (1702/0), the Windows guest (1651+10) and CI (10/10). The eight: (1) `an_ending_run_reaches_closure` cited as an existing test whose scoping gap justified a new witness — **the test does not exist**, the name occurs once, in that doc comment; (2) the pool census described as asserting "what actually failed" — it inspects `attempt.rs`/`settle.rs` while the defect was `pool: None` in `run.rs`, and restoring the pre-repair state leaves the whole suite green; (3) "no driver fixture can reach the arm", given as the structural reason a source census was necessary — `the_retaining_incarnation_retries_in_place` reaches it; (4) `AttemptPlans::pool_for` said to give the pool rule "one production implementation" — `capacity::pool_for` has three call sites in `assembly.rs`; (5) the ending witness said to cover "**every** arm" — three of six; (6) the pre-clean repair presented as complete — one of its two callers; (7) the packet-clause census said to have "would have caught… `Spend::replay`" — not among its eleven entries; (8) a fixture said to make two behaviours "not both pass" — its implementer and reviewer share `AGENT`, so both pass, and the mutation measured as killed died for the wrong reason

## What the change that takes this up should do

Owner, as the ledger records it: project owner — **the claims protocol a fresh session carries**.

**Recorded as a ledger correction, not repaired by history surgery.** The commit messages are pushed history and the owner's instruction is that they are corrected here, citing the table, exactly as `80a141b`'s false refutation was. The full table with per-claim citations is `~/tactus-artifacts/pr7/s5/r4/FALSIFICATION-TABLE.md`; the raw lens outputs are beside it. **Three confirmed code defects accompany the claims and are open**: `expected_refs`'s census entry is satisfied by a substring collision (all four `expected_refs(` matches in `workspace_manager.rs` are `refuse_unexpected_refs(`; genuine calls zero); the pre-clean fix is half-applied, leaving the stranger-killing path live at `census/tests.rs:3645`; and `an_ending_run_offers_no_work_from_any_arm` covers three of six arms with `Integrate` in the gap. **What is not in doubt**: rounds 1-3 closed real defects — the E6 promotion stall, a resumed run that forgot its spend, and a path traversal from plan-authored input where the legacy engine sanitised and the extraction did not — and those repairs are behaviourally sound. Round 4 challenged the *claims about* several witnesses, not the fixes beneath them. **The pattern, stated once**: prose asserted at the moment of writing became the evidence for the work it described, and nothing earlier in the chain checks a claim made in a commit message — which is the artifact a reviewer trusts most. **The table itself is now in this file, verbatim, as §19**, with each of the eight disproofs re-run at `cca1276` and its command recorded beside its result — including one place the table over-reached, corrected there under the same rule

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
