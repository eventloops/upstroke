---
id: PR125-CLOSE-SOURCE-CENSUS-BELOW-THE-STANDARD
severity: P2
disposition: deferred
category: docs-contract
pr: 125
reviewed_sha: 33604e648aa06fdd0551526b3b8f95d3676df7ae
location: src/agent/proc.rs:5809
provenance: introduced_by_feature
first_bad: 77be7c3
guard: deferred: if a source census is ever written for this, it blanks comments and every literal kind before matching, proves the blanker on a fixture,…
---

## Failure sequence

the closed pull request pinned "no production wildcard wait in this crate" with a test that matched two literal substrings in raw source text with `#[cfg(test)]` blocks stripped -> it cannot see `waitid(P_ALL, ..)`, an imported `waitpid`, or a `-1` held in a variable, it did not blank comments or literals, and it carried no positive control that injects a violation into the whole domain -> §12's census requirements were not met, and no census of this crate can see an embedding host's code, so it could not carry the identity proof it was written for

## What the change that takes this up should do

deferred: if a source census is ever written for this, it blanks comments and every literal kind before matching, proves the blanker on a fixture, asserts the size and boundaries of its domain, recognises every spelling of a wildcard wait, and carries a positive control that injects one violation and sees the expected failure (standards/12_standards_tests.md); and it is evidence about this crate only, never about a host

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
