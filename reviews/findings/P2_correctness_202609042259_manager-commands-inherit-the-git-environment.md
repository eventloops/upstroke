---
id: PR135-PARENT-READ-ONLY-GIT-INHERITS-THE-REPOSITORY-LOCATING-ENVIRONMENT
severity: P2
disposition: deferred
category: correctness
pr: 135
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager.rs:3108
provenance: pre_existing
first_bad: 61529ab
guard: the sweep of src/workspace_manager.rs (queue row 11)
---

## Failure sequence

`read_only_git` and `WorkspaceManager::command` build their commands with the process's whole
environment. `common_git_dir` runs `rev-parse --path-format=absolute --git-common-dir` through
`read_only_git`, so under an ambient `GIT_DIR` it answers with the repository that variable names.
`WorkspaceManager::derive` then computes `repo_key` — and therefore the execution root — from
another repository's common git dir while managing this one.

Measured on git 2.43.0 for the fixture's own commands: with `GIT_DIR` set, `git -C <fresh> init -q
-b main` creates no repository at `<fresh>` at all, re-initialises the one `GIT_DIR` names, and exits
0. The fixture's commands are immune from this pull request; the manager's are not.

## Why this pull request does not fix it

Pre-existing, in `src/workspace_manager.rs`, the parent, which is queue row 11 and reserved last by
amendment 8. Under `MAINTAINING.md`'s triage clause a finding against an unswept file is logged and
blocks nothing; this pull request's brief forbids editing the parent for the same reason.

## What the change that takes this up should do

Clear and rebuild the environment where the manager's commands are built, as this pull request does
for the fixture's. The reproduction already exists: `the_fixture_is_immune_to_the_ambient_git_environment`
runs a child under a hostile `GIT_DIR`, and its `derive` call is the unguarded path.
