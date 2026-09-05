---
id: PR120-REPO-KEY-DOCS-CITE-RETIRED-RECORD
severity: P3
disposition: deferred
category: docs-contract
pr: 120
reviewed_sha: 41facd4d402270bd6e94976ae2f4257c2874f02e
location: src/workspace_manager.rs:337
provenance: pre_existing
first_bad: 7a83e69
guard: deferred by owner direction: the parent's sweep, standards/SWEEP.md queue row 11, rewrites the docs; a01eecb carried the shape to reinstate — a…
---

## Failure sequence

the body says every citation of the retired execution-root record in both files now cites DESIGN §15 -> `REPO_KEY_V1_DOMAIN`'s and `repo_key_v1`'s docs still cite `decisions.workspace_candidates.execution_root` as the normative source of the repo-key formula -> the record is absent at the head and DESIGN does not state the formula

## What the change that takes this up should do

deferred by owner direction: the parent's sweep, `standards/SWEEP.md` queue row 11, rewrites the docs; a01eecb carried the shape to reinstate — a `repo_key` v1 sentence in `design/15_design_event_log_resume_run_layout.md` and both docs citing it — and f90935542720d76b2ffea43b08cdab1f104d99e8 withdrew it; `the_repo_key_is_the_packets_digest_and_not_a_neighbouring_one` pins the formula meanwhile

Recorded by the PR #120 `src/workspace_manager/containment.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
