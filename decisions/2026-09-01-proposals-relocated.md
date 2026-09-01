# 2026-09-01 — the filed proposals relocate to the private archive

**Verdict.** The thirteen dated files in `proposals/` — twelve proposals and one
critique, all filed on or before 2026-08-24 — are replaced in place by stubs;
their full texts move to the private companion repository's `proposals/`
folder. This supersedes the 2026-08-27 pin that the already-public proposals
stay where they are, and narrows 2026-08-22's "engine proposals stay public"
clause to match the filing rule in force since 2026-08-27: the first stage of
the design lifecycle is private end-to-end. `proposals/README.md` remains as
the public, citation-bearing index, and every stub keeps its path, title, and
status block so that every existing citation still resolves.

## Why

- **The owner's call, recorded as such:** the public repository should show the
  engine moving — code, decisions, reviews — rather than a shelf of parked
  drafts. This is a presentation ruling, not a privacy one: the docs-privacy
  triage (124 rows at `0a25698`) found no privacy defect in these files.
- The 2026-08-27 record kept the files in place because citations must keep
  resolving and history keeps them visible regardless. Both concerns remain
  true and both are honored by mechanism rather than by pin: the stubs keep
  every citation resolving, and this record repeats the standing caveat —
  relocation does not remove a file from public git history; the value is
  organizational and prospective.

## The rule

- The stub is the permanent public form of a relocated proposal: H1, status
  block, and the pointer to this record — never the content, and never a path
  or name inside the private repository (the 2026-08-22 rule, unchanged).
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
- **Leaving the files in place.** The 2026-08-27 status quo, overruled by the
  owner's presentation preference above.

## Cross-references

- [2026-08-27 — proposals are filed privately](2026-08-27-proposals-private.md)
  — the filing rule this record leaves in force, and the keep-in-place pin it
  supersedes.
- [2026-08-22 — the strategy layer lives outside the public repository](2026-08-22-strategy-record-private.md)
  — the stub mechanism and the no-private-references rule, applied here
  unchanged.
