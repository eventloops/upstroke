---
id: PR135-FIXTURE-WRITE-HELPERS-TAKE-ANY-PATH
severity: P3
disposition: deferred
category: correctness
pr: 135
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/fixture.rs:235
provenance: pre_existing
first_bad: 61529ab
guard: the sweep of src/workspace_manager.rs (queue row 11)
---

## Failure sequence

`write_file` and `create_dir` write wherever their caller points them, and callers do point them
outside any scratch root: `effects/attempt-residue-histogram.json` through the first, and
`src/engine/topology/scaffold.rs`'s own `kill_dir` through the second. `effects/allowlist.toml`'s
containment clause for this file therefore describes caller discipline rather than a property of
the types; the clause is corrected to say so in this pull request.

## What the change that takes this up should do

Take a scratch-root token, of the kind `rundir`'s scratch tree carries, so the containment claim is
enforced rather than observed. That token is the same one the adoption finding needs, so the two
should be taken up together.
