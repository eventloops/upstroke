---
id: PR135-FIXTURE-SCRATCH-TAG-IS-NOT-ONE-PATH-COMPONENT
severity: P3
disposition: deferred
category: correctness
pr: 135
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/fixture.rs:33
provenance: pre_existing
first_bad: 61529ab
guard: whichever change next touches `scratch` -- it cannot be touched alone without fixing the §8 MUST beside it
---

## Failure sequence

`scratch` interpolates its `tag` into a directory name with no validation and creates the result
with `create_dir_all`. A tag carrying a separator makes a nested path, which `create_dir_all`
creates; `Fixture::drop` then removes the leaf it was given and leaves the directory the tag's
first component made, so the temporary directory accumulates trees nothing owns, and two tags
sharing a first component share a parent.

Every tag in the tree today is a plain `[a-z0-9-]` literal, so no caller is wrong now.

## Why this pull request does not fix it

It did, for four passes: two lines routing the tag through the manager's own `safe_component`. The
fifth pass pointed out that those two lines made `scratch` *materially touched*, and
`MAINTAINING.md` requires a MUST deviation in touched code -- the §8 pre-clean two lines below --
to be fixed or the standard amended, which the stopping rule does not override. So the validation
is withdrawn with the rest of the reclaim work, and `scratch` is master's byte for byte.

## What the change that takes this up should do

Route the tag through `safe_component`, in the same change that puts `scratch` on §8's token --
they cannot be separated, because touching the function activates the MUST beside them.
