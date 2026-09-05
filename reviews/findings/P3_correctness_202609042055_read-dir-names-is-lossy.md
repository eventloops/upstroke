---
id: SWEEP-NAMES-005
severity: P3
disposition: deferred
category: correctness
pr: 143
reviewed_sha: 7724ed1d628070b35948819095a68a38cd0c5d0a
location: src/rundir.rs:895
provenance: pre_existing
first_bad:
guard: queue row 19, the sweep of `src/rundir.rs`
---

## Failure sequence

`read_dir_names` (`src/rundir.rs:889`) converts every directory entry with
`entry.file_name().to_string_lossy().into_owned()`. It is the reader behind both
sites that compare a directory entry against one of this module's name
constants, which is how this sweep reached it:

* `remove_public_husk` (`src/rundir.rs:834`) skips the entry equal to `MARKER`
  and removes the rest by `public.join(&entry)`;
* `unbound_shape` (`src/rundir/ownership.rs:295`) answers `StagedMarkerOnly`
  when the sole entry equals `MARKER_STAGED`.

A name that is not valid UTF-8 does not survive that conversion. On Linux a file
name is an arbitrary byte sequence, and `design/15` is explicit that
repository-controlled gate code "can discover the source worktree and modify
`.upstroke`", so the public half is reachable by code that can create one.

The sequence: a public husk holds `<public>/a\xffb` -> `read_dir_names` yields
`a\u{FFFD}b` -> `public.join("a\u{FFFD}b")` names a path that does not exist ->
`fs::remove_file` returns `NotFound` -> `remove_public_husk` returns
`UpstrokeError::Io` and stops -> the husk is never reclaimed, and every later
`startup_census` reports it again.

Both failures are in the safe direction and that is why this is P3, not higher.
`remove_public_husk` errors rather than deleting a path it did not mean to: the
lossy name it builds is either absent or inside `<public>`, never outside it.
`unbound_shape` answers `Retained(MarkerlessWithContent)` for a lone unconvertible
entry, which retains and reports rather than reclaiming. Nothing is deleted that
should not be; a husk simply becomes permanent.

`CODING_STANDARDS.md` §8 is the standard: "Paths are `Path`, `PathBuf`, `OsStr`
or `OsString`: never string concatenation, never assumed UTF-8. A lossy display
string is for diagnostics only, never identity." `read_dir_names`' output is used
as identity at both sites above.

## What the change that takes this up should do

Return `Vec<OsString>` from `read_dir_names` and compare with
`OsStr::new(MARKER)`, which is exact and needs no conversion at either site; the
sort stays, on the `OsString`s. The name constants stay `&str` — see the note in
this pull request's body on why `&'static Path` is not available on MSRV 1.85 —
and `OsStr::new` bridges them at the two comparison sites at no cost.

Witness it with a husk holding one entry whose name is not valid UTF-8
(`std::os::unix::ffi::OsStrExt::from_bytes`, Unix-only), asserting that
`remove_public_husk` returns `Ok` and the directory is gone; on master's reader
that test fails with the `NotFound` above.
