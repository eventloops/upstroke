---
id: PR128-NON-UTF8-TEST-CANNOT-RUN-ON-MACOS
severity: P2
disposition: deferred
category: portability
pr: 128
reviewed_sha: f5b75d7a8088c801f7efa599ee1afe04c0ba6eb9
location: src/workspace_manager/tests.rs:6810
provenance: introduced_by_feature
first_bad: 55ed9d3
guard: the repair is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found…
---

## Failure sequence

the §8 witness built its fixture with a `0xff` byte in the directory name under `#[cfg(unix)]` -> macOS rejects a filename that is not valid UTF-8 at creation, APFS answering `EILSEQ` -> CI's `test (macos-latest)` leg failed at `f5b75d7` ("a repository directory whose name is not UTF-8: Os { code: 92, kind: Uncategorized, message: \"Illegal byte sequence\" }", 1892 passed, 1 failed), a defect in this pull request's own test and not in the code it guards, since every other job was green

## What the change that takes this up should do

the repair is **reverted** at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found the same defect classes in the machinery each round added: the parent helper is master's again and this row is deferred to the sweep of `src/workspace_manager.rs`, the queue's last row of this family, with the reproduction it carries. Making an inspection's absence trustworthy means reading a repository the way Git reads it, which is that file's work; a design sentence follows that code rather than preceding it

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
