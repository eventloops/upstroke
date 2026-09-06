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

Recorded 2026-09-06T21:45Z by the parked-PR recovery workflow, task pr197 (earlier record:
2026-09-06T09:14Z, findings-workflow task 2b802b011b29).

The recovery spent its full budget -- an initial review and three repair rounds, four started
reviews of `gpt-5.6-sol` at high effort -- and pass 4 of 4 still returned one blocking P2, so the
pull request is parked again rather than merged. Each round repaired the previous round's finding
in the census this pull request adds and each was confirmed fixed by the next pass; the original
finding's repair has been recorded intact by every pass.

The finding itself is fixed and proved on the branch at
`cbd42faa60df93807602a2e8100f7624193b553a`: the six truncated `SAFETY:` blocks and the `HeldFork`
protocol are restored complete and adjacent, no executable token of the existing module changed,
the nine-command baseline and `test-pr-ready-audit.sh` are green at that head, both required CI
contexts are green at it, and seventeen mutation arms each detect a different way of losing a site
copy or misreading the module.

What blocks the merge is `PR157-ASTRA-SITE-SAFETY-RECOVERY-R4-001` (P2, `correctness`), filed as
`reviews/findings/P2_correctness_202609062143_host-test-census-trusts-macro-names.md`: the census
classifies a keyword token by the terminal name of the macros around it, so a local `macro_rules!`
named `stringify` that forwards its input lets a real unannotated unsafe expression evade the
census. The recovery's three earlier rounds closed raw identifiers, Unicode identifiers and inert
macro input; this is the fourth shape of the same class, a scanner whose notion of an operation is
not the compiler's, and the finding file names the two repairs that would close the class (fail
closed on a shadowed built-in name, or an explicitly anchored census with asserted boundaries).

Preserved for the owner: branch `codex/findings-2b802b011b29`, worktree
`/srv/worktrees/findings-workflow/tasks/2b802b011b29`, draft PR
https://github.com/eventloops/upstroke/pull/197 (not closed), all six verdicts posted verbatim as
SHA-bound comments, the audit at `/home/ubuntu/parked-pr-workflow/tasks/pr197/history.md`, and
the mutation witnesses under `/home/ubuntu/parked-pr-workflow/tasks/pr197/evidence/`.
