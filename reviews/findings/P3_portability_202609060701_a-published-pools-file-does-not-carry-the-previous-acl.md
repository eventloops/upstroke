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
guard: deferred: `the_mode_an_operator_gave_their_pools_file_survives_a_forced_rewrite` witnesses the one thing that is carried, the Unix mode bits, under `#[cfg(unix)]`; nothing beyond the mode bits is carried on any platform, and the notes state that limitation rather than claiming the guarantee
---

## Failure sequence

An operator sets access-control metadata on `pools.toml` that ordinary mode bits cannot express —
an explicit ACE on Windows, a POSIX access ACL (`setfacl`) on Linux, a macOS ACL (`chmod +a`) —
-> `upstroke connect --force` publishes the replacement by renaming a staging file the process
created, which carries only what its directory gave it plus, on Unix, the previous file's
`st_mode` bits (`apply_mode`) -> the replacement is a different inode or file object, so the
operator's explicit ACL is not on it, where before this change `fs::write` reused the existing
inode and its ACL survived.

This is a change this pull request makes, stated rather than left to be discovered, and it holds
on every supported platform, not only on Windows: pass 1 of the recovery review
(`SWEEP-CONNECT-011`) found the first version of this record describing it as Windows-only, with
the Unix mode test presented as "the other half". The mode test is the witness for what *is*
carried (the permission bits); it says nothing about extended ACLs, which live beside the mode
bits on the old inode and are lost with it.

On Windows the `#[cfg(not(unix))]` arm carries nothing at all on purpose — a `Permissions` off
Windows' `metadata` is only the read-only attribute, and setting *that* would refuse the
publication over a read-only destination and leave behind a `.tmp` that cannot be removed.

## What the change that takes this up should do

Decide whether upstroke carries access-control metadata beyond the mode bits across a
replacement at all, and say so in `design/17` beside the sentence that makes the pools file
hand-editable. Carrying it means a platform seam this module does not have and should not grow
on its own: on Windows, reading the destination's security descriptor and applying it to the
staging file before the rename (`GetNamedSecurityInfoW`/`SetNamedSecurityInfoW` through
`windows_sys`); on Linux, copying the POSIX ACL (`acl_get_file`/`acl_set_file`, or the
`system.posix_acl_access` extended attribute); on macOS, `acl_get_file`/`acl_set_file` on the
native ACL. Not carrying it means the design says that a `--force` rewrite hands the file the
ACL its directory gives a new file, which is what a new pools file has always had, and the
operator re-applies an explicit ACL if they want one.

Neither the file nor its notes claims the guarantee today: `docs/internals/connect.md` states the
mode preservation as a Unix statement about the mode bits alone and says that no ACL, on any
platform, is attempted or claimed.
