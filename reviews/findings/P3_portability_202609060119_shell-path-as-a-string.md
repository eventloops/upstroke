---
id: PR173-SHELL-PATH-AS-A-STRING
severity: P3
disposition: deferred
category: portability
pr: 173
reviewed_sha: 429afd082e5628e8131627bc822dcc882de29aed
location: src/agent/proc/tests.rs:356
provenance: introduced_by_feature
first_bad:
guard: the next push to PR #173, or the sweep of `src/agent/proc/tests.rs`
---

## Failure sequence

Both new tests build their child with `Command::new("/bin/sh")` (from `src/agent/proc/tests.rs:356` and `:413`), a path as a string -> `standards/08_standards_filesystems_persistence_paths.md` puts every path through `std::path`, and `#[cfg(unix)]` keeps the string off Windows without satisfying that rule or a Unix whose shell is elsewhere. The pre-existing test above them, `a_child_registered_pre_exec_is_settled_when_the_parent_never_registers_it`, does the same, so this is the file's shape rather than new to these two; recorded because the pass named it in touched code.

## What the change that takes this up should do

Build the program as a `Path` (`Path::new("/bin/sh")`, or the crate's own shell resolution where the test does not need a fixed binary), for the two new tests and the older one beside them in the same change.

Recorded from the frontier pass of 2026-09-06 (`gpt-5.6-sol`, max effort) on PR #173 at `429afd0`, posted as https://github.com/eventloops/upstroke/pull/173#issuecomment-5556000412. Filed as the reviewer wrote it, with the author's reading beneath.
