---
id: PR135-REVIEW1-MANAGER-CHECKOUT-IS-NOT-COVERED-BY-THE-ATTRIBUTES-PIN
severity: P2
disposition: deferred
category: portability
pr: 135
reviewed_sha: af53b8f0a457522b165c5851c7026099a34ef591
location: src/workspace_manager.rs:2898
provenance: pre_existing
first_bad: 61529ab
guard: the sweep of src/workspace_manager.rs (queue row 11), which owns the command builder
---

## Failure sequence

`WorkspaceManager::command` composes its own environment and sets neither `core.attributesFile` nor
`GIT_ATTR_NOSYSTEM`. With an ambient attributes file carrying `* text eol=crlf`, `git worktree add`
under `Fixture::add_task` checks files out with CRLF while the blobs are LF, so a test comparing
checked-out content against what it wrote fails on that machine alone.

The fixture's own door does not reach it: this module clears the environment for the commands **it**
builds, and the manager's are not among them.

## What the change that takes this up should do

Pin the attributes source where the manager's commands are built, beside the `core.hooksPath`,
`core.fsmonitor` and `GIT_NO_REPLACE_OBJECTS` that are already there. The same builder also inherits
Git's repository-locating environment, recorded separately.
