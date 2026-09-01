# 2026-08-27 — new proposals are filed privately

**Verdict.** New design proposals are filed in a private companion repository,
engine mechanisms included. The proposals already in this repository's
`proposals/` folder stay where they are, and the folder stays with them. This
reverses the previous rule that engine proposals remain public.

This changes **where a proposal is filed**, and nothing else. A proposal still
binds nothing until a decision cites it; `DESIGN.md` remains the only living
authority for product design; decision records still land here, publicly, and
still name their inputs.

**The reasoning is recorded privately**, with the design work it concerns. This
record carries the verdict and the obligations that follow from it, which is
what a reader of this repository needs in order to act correctly. Recording the
existence of the private rationale rather than omitting it keeps the ledger
honest about what it does and does not hold — the same treatment
[2026-08-22](2026-08-22-strategy-record-private.md) gave the strategy layer.

## Consequences

- The public folder is **not deleted**, and must not be. Decision records cite
  its contents as inputs, and those citations must keep resolving. It is also
  named in `CLAUDE.md`, where `.github/scripts/test-docs-consistency.sh` C1
  fails on a backticked path that does not exist at that head.
- `CLAUDE.md`, `proposals/README.md` and `decisions/README.md` are reconciled in
  the same change as this record, per `CODING_STANDARDS.md` §1.
- Nothing public names, summarises, or cites a private proposal by path. A
  decision record whose input is a private proposal says that it had one without
  reproducing it, and the promotion rule is unchanged: a document becomes public
  only when a pull request here first needs to cite it, carrying its provenance.
- One filing drafted under the previous rule is in flight. Its disposition is
  the owner's and is not settled here; this record is not an instruction to
  withhold it.

## Measured

At this head the public folder holds twelve proposals and a README. C1's path
check scans `CLAUDE.md` and `CONTRIBUTING.md` for backticked repository paths
and fails on one that does not exist unless that occurrence is qualified within
its own window.

## Rejected

- **Delete the folder and move all twelve.** Decision records cite them as
  inputs and the citations would stop resolving; the history keeps the files
  visible regardless, so the deletion would buy nothing but broken links.
- **Rewrite history to remove them.** Every review record and pull-request body
  here cites SHAs, and the crate is published. Rewriting breaks the citations
  that make the ledger auditable, in order to hide documents that are already
  public.

## 2026-09-01 — the keep-in-place pin is superseded

The verdict above that the proposals already public stay where they are was
superseded on 2026-09-01: the dated proposal files were reduced to stubs in
place, full texts relocated to the private companion repository — except the
Decided G2 pass plan, which stays in full because `DESIGN.md` and the freeze
charter rely on its content, and the council critique, which stays in full
because critiques are review-stage records and every stage after filing
remains public. This record's filing rule is unchanged and remains in force.
See [2026-09-01 — the filed proposals relocate to the private archive](2026-09-01-proposals-relocated.md).
