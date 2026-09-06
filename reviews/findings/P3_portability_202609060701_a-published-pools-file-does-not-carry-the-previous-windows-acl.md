---
id: SWEEP-CONNECT-003
severity: P3
disposition: deferred
category: portability
pr: 189
reviewed_sha: 45bb0725d9420226cd6e2a7f386a16250b46f913
location: src/connect.rs:317
provenance: introduced_by_feature
first_bad: f658080c
guard: deferred: `the_mode_an_operator_gave_their_pools_file_survives_a_forced_rewrite` is the Unix half and runs under `#[cfg(unix)]`; the Windows half has no witness here because there is nothing yet to witness, and the notes state the limitation rather than claiming the guarantee
---

## Failure sequence

An operator on Windows sets an explicit ACE on `pools.toml` -> `upstroke connect --force`
publishes the replacement by renaming a staging file the process created, which carries the ACL
it inherited from the parent directory -> the replacement is a different file object, so the
operator's explicit ACE is not on it, where before this change `fs::write` reused the existing
file object and its ACL survived.

This is a change this pull request makes, stated rather than left to be discovered: on Unix the
mode is read from the destination and applied to the staging file, so the equivalent loss does
not happen there (`apply_mode`, `#[cfg(unix)]`), and the `#[cfg(not(unix))]` arm carries nothing
on purpose — a `Permissions` off Windows' `metadata` is only the read-only attribute, and setting
*that* would refuse the publication over a read-only destination and leave behind a `.tmp` that
cannot be removed.

## What the change that takes this up should do

Decide whether upstroke carries a Windows ACL across a replacement at all, and say so in
`design/17` beside the sentence that makes the pools file hand-editable. Carrying it means reading
the destination's security descriptor and applying it to the staging file before the rename
(`GetNamedSecurityInfoW`/`SetNamedSecurityInfoW` through `windows_sys`), which is a platform seam
this module does not have and should not grow on its own; not carrying it means the design says
that a `--force` rewrite hands the file the directory's inherited ACL, which is what a new pools
file has always had, and the operator re-applies an explicit ACE if they want one.

Neither the file nor its notes claims the guarantee today: `docs/internals/connect.md` states the
mode preservation as a Unix statement and says that a Windows ACL is not attempted and not
claimed.
