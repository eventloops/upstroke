---
id: PR160-NOTES-RUSTDOC-LINKS
severity: P3
disposition: deferred
category: docs-contract
pr: 160
reviewed_sha: 41eb825d32a598f9c1b19e5ae93ae510786b3d8f
location: docs/internals/engine/topology/settle.md:40
provenance: introduced_by_feature
first_bad: 1e8050258fbf58dc45e5d7664356ca30c53c7ec6
guard: Render the notes as GitHub Markdown and verify that the Epoch and IncarnationId references link to their actual definitions in src/topology/events.rs.
---

## Failure sequence

A reader opens the migrated settlement notes and follows the type references distinguishing Epoch from IncarnationId. Lines 40 and 43 retain rustdoc destinations beginning with crate::, which the Markdown renderer does not resolve as Rust items. GitHub's rendered response for the exact reviewed SHA contains plain code elements for these references and no hyperlinks. The reader loses the navigation that rustdoc supplied before the migration.

The independent review preserved that response in evidence-41eb825d/settle-rendered.html. The corresponding definitions are src/topology/events.rs:274 and src/topology/events.rs:259. Other shortcut rustdoc references in these notes also render as bracketed text, so the follow-up should inspect those references too.

## What the change that takes this up should do

Convert the moved rustdoc references to Markdown links that work in the intended reader context, with source or notes targets for the named items. A repository backlink to settle.rs does not identify these definitions in events.rs. Record this finding and defer repair under DOCS_FAST_TRACK.md.

## Update, 2026-09-06

The two destinations this record cites, `docs/internals/engine/topology/settle.md:40`
and `:43`, are converted. They now point at `../../topology/events.md`, the notes for
`src/topology/events.rs`, where `Epoch` and `IncarnationId` are documented. Every other
destination of that shape in `docs/internals/` went with them — eleven in all, three of
which a `crate::` search does not find — and
`.github/scripts/validate-internals-notes.sh`'s N5 resolves every inline destination in
the notes tree from that head on, so the form cannot come back.

This record stays open for the half it names in passing. The other rustdoc shortcut
references in these notes still render as bracketed code: 23 in `settle.md` alone and
1743 across 111 notes files. Only the ten effects notes were converted, because they are
the ten an independent CommonMark rendering audit measured.
`PR161-NOTES-SHORTCUT-REFERENCES-TREE` is the standing entry for the rest, and N5's
second half is scoped to the converted files so the domain widens with the conversion
rather than ahead of it.
