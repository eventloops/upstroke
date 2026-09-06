---
status: owner attention required
id: PR157-ASTRA-SITE-SAFETY
severity: P2
disposition: deferred
category: docs-contract
pr: 157
reviewed_sha: af2c3efe93673acf5a3ae0c849db5f7657ec5579
location: src/runner/host/tests.rs:4026
provenance: introduced_by_feature
first_bad:
guard: Future host-test documentation maintenance must retain complete adjacent safety and concurrency reasoning under standards sections 10, 11 and 13
---

This documentation finding is deferred under the owner's 2026-09-05 docs
fast-track policy. It does not require another repair cycle before this PR
lands. No change to the unsafe operations or runtime regression was found.

## Failure sequence

Open `hold_inherited_descriptors` at the reviewed SHA and inspect the `unsafe`
block beginning at `src/runner/host/tests.rs:4028`. Its adjacent safety comment
ends with the unfinished words "The child" at line 4026. The diff against base
`735ef2142238885041f30d82cc3409a67863a0d1` removes the remaining explanation:
the child uses only async-signal-safe operations, neither allocates nor unwinds
nor drops Rust owners, and closes its inherited copy of the parent's endpoint.
The complete proof now appears only in `docs/internals/runner/host/tests.md:2900`.

This also happens to the safety comments at source lines 3997 and 4068, whose
sentences end after "a live," and "for this". The `HeldFork` lifetime and
cleanup protocol above the struct at line 3974 moves entirely to notes at
line 2883. A reader checking the unsafe sites therefore lacks the complete
local obligations that `standards/11_standards_unsafe_and_platform_code.md:4`
requires. Section 13 explicitly preserves that placement requirement even
when a module has notes.

The comparison changes comments at these sites, not their executable tokens.
The independent delta audit retained at
`/srv/worktrees/astra-20260905/agents/astra_review_157/delta-audit-af2c3efe.json`
confirms that the executable changes in this test module are confined to the
separate HostRunner construction census.

## What the change that takes this up should do

Restore the complete safety arguments beside the affected unsafe operations
and the `HeldFork` ownership protocol beside its type. Keep the notes copy if
useful. Check complete comment blocks when moving prose, rather than retaining
only their first `SAFETY:` line. This is future documentation maintenance,
not a prerequisite repair for this review's PASS.

## Owner attention required

Recorded 2026-09-06T09:14:20.472649+00:00. Workflow task 2b802b011b29.

Review pass 2 of 2 returned CHANGES_REQUIRED with one blocking P2, so both passes of this
workflow's budget are spent and the ticket is parked for owner attention rather than merged.

The finding itself is fixed on the branch and proved: the six truncated `SAFETY:` blocks and the
`HeldFork` protocol are restored complete and adjacent at
`943c4714dfcb33fb071a5b030d026dfae241850e`, no executable token of the existing module changed,
the nine-command baseline is green at that head, both required CI contexts are green at it, and
four separate mutation arms each detect a different way of losing the site copy. Both pass-1
findings were fixed, not deferred.

What blocks the merge is `PR157-ASTRA-SITE-SAFETY-R2-001` (P2, `correctness`), a defect in the
census this pull request adds: it looks for the keyword in raw source lines, excluding only
whole-line `//` comments, so a string literal or block comment containing the word is read as an
operation. `standards/12_standards_tests.md` requires a census of Rust structure to work from a
position-preserving view with comments and literals blanked, proved by a fixture; the crate has
`blank_comments_and_strings` in `src/effects.rs` and this census uses neither it nor any
equivalent. The implementor reproduced the reviewer's witness at the exact reviewed head: appending
`const _: &str = "unsafe";` — a valid item introducing no unsafe operation — makes the census
report `src/runner/host/tests.rs:5888: the operation carries no adjacent obligation`. That is a
concrete failing reproduction in code this pull request adds, which the workflow forbids waving
through at any severity, and repairing it is a substantive change that would need a third review
pass this workflow does not have.

The repair is small and is written down in
`reviews/findings/P2_correctness_202609060911_host-test-census-scans-unblanked-source.md`: route
the keyword scan through `crate::effects::blank_comments_and_strings`, read the adjacent obligation
out of the original source at the blanked position so the comment text is still compared verbatim,
and add two controls to the census itself — a literal and a block comment containing the keyword
must be ignored, and a real operation missing its obligation must still be rejected.

Preserved for the owner: branch `codex/findings-2b802b011b29`, worktree
`/srv/worktrees/findings-workflow/tasks/2b802b011b29`, draft PR
https://github.com/eventloops/upstroke/pull/197 (not closed), both verdicts posted verbatim as
SHA-bound comments (pass 1
https://github.com/eventloops/upstroke/pull/197#issuecomment-5558138768, pass 2
https://github.com/eventloops/upstroke/pull/197#issuecomment-5558249888), and the mutation
witnesses under `/home/ubuntu/findings-workflow/tasks/2b802b011b29/evidence/`.
