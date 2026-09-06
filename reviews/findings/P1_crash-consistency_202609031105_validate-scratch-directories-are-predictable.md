---
id: PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED
severity: P1
disposition: deferred
category: crash-consistency
pr: 104
reviewed_sha:
location: src/validate.rs
provenance: pre_existing
first_bad:
guard: the slice that next opens src/validate.rs's test region
---

## Failure sequence

Every temporary directory in `src/validate.rs`'s test region is derived from `env::temp_dir().join(format!("upstroke-validate-<tag>-{}", process::id()))` — **predictable**, created with `create_dir_all` (which accepts an existing directory), stored as a bare `PathBuf`, and never reclaimed: **12 `env::temp_dir()` sites, 12 `create_dir_all` lines and 0 `impl Drop`** at `ae2a58f`. `scratch_root` (`:403`) additionally runs `let _ = fs::remove_dir_all(&dir);` (`:405`) against that predictable path before creating it, deleting whatever a previous run or another process left there and discarding the error. `standards/12_standards_tests.md:16` requires *"unique temporary directories with RAII cleanup"*

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next opens `src/validate.rs`'s test region.

Byte-identical before and after PR #104 and not activated by it; the reviewer said so explicitly and kept it out of the verdict, which turned on the newly introduced instance — and **that instance no longer exists**, since owner ruling 7 reverted the file to `origin/master` entirely. **The harm is measured, not argued**: the pass-7 reviewer pre-created `$TMPDIR/upstroke-validate-sample-<pid>/foreign-sentinel`, ran `sample_plan_renders_expected_table` against the exact-head binary, and the test **passed** with `sentinel=deleted` and `replacement_plan=present`. Full derivation: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P1** here is this migration's judgement from the consequence described above, not the reviewer's own word.
