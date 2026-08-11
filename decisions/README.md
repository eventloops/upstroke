# decisions/

Dated, append-only decision records: verdict first, the reasoning that earned it,
measured vs. assumed named explicitly, rejected options recorded with why.

The contract that keeps this folder safe:

- **DESIGN.md remains the only living authority.** Records here are history, not
  spec. When a record's outcome changes the spec, DESIGN.md gets the compressed
  edit at the time of the decision, citing the record.
- **Records are immutable once landed.** Corrections and follow-ups are dated
  addenda, never silent edits.
- **Design documents do not live here.** Proposals and drafts stay outside the
  repo; a record cites its inputs.

When design work runs through tactus itself, council ledgers land as run
artifacts (§15); records promoted here are the durable subset.
