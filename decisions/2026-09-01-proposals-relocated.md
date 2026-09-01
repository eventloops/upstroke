# 2026-09-01 — the filed proposals relocate to the private archive

**Verdict.** Twelve of the thirteen dated files in `proposals/` — eleven
proposals and one critique, all filed on or before 2026-08-22 — are replaced
in place by stubs; their full texts move to the private companion repository's
`proposals/` folder. The thirteenth, the Decided
[G2 pass plan](../proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md), stays
public in full: `DESIGN.md` and the freeze charter rely on its ten workstreams
and exit criteria as *content*, and 2026-08-22's own retained-content rule —
what other documents rely on stays — applies here at file granularity. This
record supersedes the 2026-08-27 pin that the already-public proposals stay
where they are, and narrows 2026-08-22's "engine proposals stay public" clause
to match the filing rule in force since 2026-08-27: the first stage of the
design lifecycle is private end-to-end. Both prior records carry dated
forward notices appended in this same change. `proposals/README.md` remains as
the public, citation-bearing index, and every stub keeps its path, title, and
status block **verbatim** so that every existing citation still resolves.

## Why

- **The owner's call, recorded as such:** the public repository should show the
  engine moving — code, decisions, reviews — rather than a shelf of parked
  drafts. This is a presentation ruling, not a privacy one: an owner-side
  documentation-privacy triage of the whole tree at `0a25698`, held outside
  the repository, found no privacy defect in these files.
- The 2026-08-27 record kept the files in place because citations must keep
  resolving and history keeps them visible regardless. Both concerns remain
  true and both are honored by mechanism rather than by pin: the stubs keep
  every citation resolving, the one content-relied-on file stays whole, and
  this record repeats the standing caveat — relocation does not remove a file
  from public git history; the value is organizational and prospective.

## The rule

- The stub is the permanent public form of a relocated proposal: the original
  H1 and status block byte-for-byte, plus the pointer to this record — never
  the content, and never a path or name inside the private repository (the
  2026-08-22 rule, unchanged).
- A proposal whose content — not merely whose path — is relied on by a living
  authority or an immutable record is not stubbed; it stays public in full.
  Today that is exactly the Decided G2 pass plan.
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
- **Stubbing all thirteen.** A stub of the G2 pass plan leaves
  `DESIGN.md`'s "the pass's ten workstreams and exit criteria are planned
  [there]" pointing at an empty file, and the freeze charter — immutable —
  assigns the pass's scope to it. Content reliance beats uniformity.
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
