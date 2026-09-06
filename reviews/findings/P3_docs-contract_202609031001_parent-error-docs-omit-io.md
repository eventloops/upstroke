---
id: PR120-PARENT-ERROR-DOCS-OMIT-IO
severity: P3
disposition: deferred
category: docs-contract
pr: 120
reviewed_sha: d5cfbc34412f785534d7260ddf2d0147cd2b5d0c
location: src/workspace_manager.rs:699
provenance: introduced_by_feature
first_bad: e4bf5dc (`revalidate`'s Io), 61398c6 (`add_worktree`'s)
guard: deferred by owner direction (P3 and lower recorded): the two # Errors lists are owed and are rewritten in the parent's sweep, standards/SWEEP.md…
---

## Failure sequence

`revalidate` documents only containment refusals and Git errors while the regular-file test requires `UpstrokeError::Io` -> `add_worktree` documents no I/O error while the intent-metadata read returns one -> the public contracts do not name a demonstrated outcome

## What the change that takes this up should do

deferred by owner direction (P3 and lower recorded): the two `# Errors` lists are owed and are rewritten in the parent's sweep, `standards/SWEEP.md` queue row 11; `a_regular_file_on_the_chain_is_reported_where_it_stands_and_never_as_nothing_to_remove` and `an_intent_that_cannot_be_read_is_an_error_and_not_an_absent_intent` pin the outcomes the docs omit

Recorded by the PR #120 `src/workspace_manager/containment.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
