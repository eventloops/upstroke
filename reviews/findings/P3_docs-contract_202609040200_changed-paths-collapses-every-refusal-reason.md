---
id: SWEEP-PARSERS-007
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: f458cfc6d4470970744c3950f20a5a108ac0d1fe
location: src/workspace_manager/parsers.rs:161
provenance: pre_existing
first_bad: —
guard: deferred to the parent's sweep, queue row 11, which owns changed_paths, the layer that knows the slot and the base; changed_path_records is the…
---

## Failure sequence

`decode_changed_paths` returns `RepoWide` for every refusal and nothing records why -> `changed_paths` in the parent hands the merge queue a region that serialises every task -> an operator sees tasks serialise and has no diagnostic naming the undecodable path or the cut-off record

## What the change that takes this up should do

deferred to the parent's sweep, queue row 11, which owns `changed_paths`, the layer that knows the slot and the base; `changed_path_records` is the typed reader it can adopt, and the doc comment on `decode_changed_paths` says the reason ends there until the parent decides

Recorded by the `src/workspace_manager/parsers.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
