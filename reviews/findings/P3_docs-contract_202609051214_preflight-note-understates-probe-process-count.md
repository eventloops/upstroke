---
id: ASTRA158-005
severity: P3
disposition: deferred
category: docs-contract
pr: 158
reviewed_sha: 105c9e1509efe6cbfbe6d93e8d930c289056f041
location: docs/internals/engine/topology/preflight.md:42
provenance: pre_existing
first_bad:
guard: "Read the process-count statement against CodexAdapter's probe and strict-config loop: one version command, two help commands, six parser commands and one catalog command on the successful path. This documentation finding is nonblocking under the owner's documentation review direction recorded in the PR."
---

## Failure sequence

A reader uses the preflight note's four-process count to reason about invocation registration -> the successful Codex probe actually runs ten processes -> the description understates what the wrapper must register and conflicts with the later paragraph in the same note.

## What the change that takes this up should do

Update the count or describe the registration requirement without fixing a count owned by the adapter. Compare the statement with the probe and strict-config loop, including both fresh and resume surfaces.

## Review history and evidence

ASTRA158-005 was independently reported as P3/docs-contract at 105c9e1509efe6cbfbe6d93e8d930c289056f041. The stale count pre-existed in the declared base's source comments and was migrated by a08ea236ddab3b3d9f9471a126a4203494db0645. The exact first-bad commit is not established.

preflight.md lines 42 and 46 say four processes, while the paragraph beginning near line 202 describes ten. src/agent/codex.rs contains one version command, two help commands, a two-surface loop with three strict-config parser commands per surface, and one model-catalog command. The review and this record preparation used source inspection; no agent CLI was invoked.

[Independent review of 105c9e1509efe6cbfbe6d93e8d930c289056f041](https://github.com/eventloops/upstroke/pull/158#issuecomment-5551707422).
