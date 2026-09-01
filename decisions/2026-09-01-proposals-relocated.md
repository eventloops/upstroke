# 2026-09-01 — the filed proposals relocate to the private archive

**Verdict.** Ten of the thirteen dated files in `proposals/` — ten proposals,
all filed on or before 2026-08-22 — are replaced in place by stubs; their
full texts move to the private companion repository's `proposals/` folder.
Three files stay public in full, for three connected reasons: the Decided
[G2 pass plan](../proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md), because
`DESIGN.md` and the freeze charter rely on its ten workstreams and exit
criteria as *content* (2026-08-22's retained-content rule at file
granularity); the
[council critique](../proposals/2026-08-13-v0.5-portfolio-coordination-critique-claude.md),
because a critique is a review-stage record — the folder contract's rule that
every lifecycle stage after filing stays public applies to it, not the filing
rule; and the
[portfolio proposal](../proposals/2026-08-13-v0.5-portfolio-coordination.md)
that critique reviews, because the retained critique cites its sections by
number throughout — the retained-content rule applies transitively to
whatever a retained public record relies on. The ten relocated files carry no
content-level public citation that *uses* their content: the one record
naming them is a review coverage manifest, satisfied by path resolution, and
the folder index's summaries are not a reliance but the deliberate public
replacement (the rule below). This record supersedes the
2026-08-27 pin that the already-public
proposals stay where they are, and narrows 2026-08-22's "engine proposals
stay public" clause to match the filing rule in force since 2026-08-27: the
*first* stage of the design lifecycle is private end-to-end, and every later
stage — critiques, decisions, implementation, reviews — is unchanged and
public. Both prior records carry dated forward notices appended in this same
change. `proposals/README.md` remains as the public, citation-bearing index,
and every stub keeps its path, title, and status block **verbatim** so that
every existing citation still resolves.

**Evidence that this is relocation, not deletion.** The private intake landed
on the companion repository's default branch, at its commit
`359953f54f6c1dd08b2d2d1b36b1a1635a72e26b`, before this record merged; all
thirteen dated files there are blob-for-blob identical to the public
originals at `0a25698` (verified by object ID, per file). And the standing
caveat holds either way: relocation does not remove a file from public git
history; the value is organizational and prospective.

## Why

- **The owner's call, recorded as such:** the public repository should show the
  engine moving — code, decisions, reviews — rather than a shelf of parked
  drafts. This is a presentation ruling, not a privacy one: an owner-side
  documentation-privacy triage of the whole tree at `0a25698`, held outside
  the repository, found no privacy defect in these files.
- The 2026-08-27 record kept the files in place because citations must keep
  resolving and history keeps them visible regardless. Both concerns remain
  true and both are honored by mechanism rather than by pin: the stubs keep
  every citation resolving, the two content-relied-on files stay whole, and
  the intake evidence above makes the move verifiable rather than asserted.

## The rule

- The stub is the permanent public form of a relocated proposal: the original
  H1 and status block byte-for-byte, plus the pointer to this record — never
  the content. **The 2026-08-22 no-private-references rule is narrowed here,
  and honestly:** a relocated proposal's private counterpart shares its
  public filename by construction — that identity is what keeps citations
  resolving — and the index deliberately names and summarizes every relocated
  proposal, so the exact private paths are derivable from public material.
  What remains forbidden is pointing beyond the public tree: no link, path,
  or commit into the private repository appears anywhere public except a
  sanctioned `Provenance:` line.
- **An index summary is not content reliance.** The index's one-line
  summaries are the deliberate public replacement for the relocated texts —
  the record of what each argued — not a use of their content. The transitive
  retention rule counts citations that *use* content (a charter assigning
  scope, a critique citing sections by number), not descriptions that replace
  it; verifying a summary beyond the index is exactly the activity that moved
  private, reachable through history or the archive.
- **A stub's status block is a frozen snapshot as of relocation.** The living
  status is tracked with the private text; lifecycle movement surfaces
  publicly the way the filing rule already provides — a decision record that
  cites the proposal. A stub is not updated to track private status, and the
  byte-for-byte rule binds at relocation time, once.
- A file whose content — not merely whose path — is relied on by a living
  authority, an immutable record, or a retained public record is not stubbed;
  nor is any record of a lifecycle stage after filing (critiques included).
  The rule is transitive and terminates: today it retains exactly the Decided
  G2 pass plan, the portfolio critique, and the portfolio proposal whose
  sections that critique cites.
- `proposals/README.md` stays, closed to new filings, as the index; its
  per-file summaries are the public record of what each proposal argued.
- Filing rules are otherwise unchanged from 2026-08-27: new proposals are
  private, and a private document reaches this repository only when a pull
  request here first needs to cite it, arriving with a `Provenance:` line.

## Rejected

- **Deleting the folder.** `proposals/` and `proposals/README.md` are
  C1-scanned backticked paths in `CLAUDE.md`, and decision records cite the
  dated files as inputs; deletion breaks both. Rejected on the same grounds
  2026-08-27 rejected it.
- **Stubbing all thirteen.** A stub of the G2 pass plan leaves `DESIGN.md`'s
  "the pass's ten workstreams and exit criteria are planned [there]" pointing
  at an empty file while the freeze charter — immutable — assigns the pass's
  scope to it; a stub of the critique removes a review-stage record the
  folder contract promises stays public; and a stub of the portfolio proposal
  strands the retained critique's section-by-number citations. Content
  reliance and the lifecycle rule beat uniformity, transitively.
- **Leaving the files in place.** The 2026-08-27 status quo, overruled by the
  owner's presentation preference above.

## Cross-references

- [2026-08-27 — proposals are filed privately](2026-08-27-proposals-private.md)
  — the filing rule this record leaves in force, and the keep-in-place pin it
  supersedes; carries the appended forward notice.
- [2026-08-22 — the strategy layer lives outside the public repository](2026-08-22-strategy-record-private.md)
  — the stub mechanism, the retained-content rule, and the no-private-references
  rule, applied here unchanged; carries the appended forward notice.
- [2026-08-24 — the PR3-layer freeze charter](2026-08-24-pr3-layer-freeze-charter.md)
  — the immutable record whose reliance keeps the G2 pass plan public in full.
