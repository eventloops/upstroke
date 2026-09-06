# `src/effects/tests/policy.rs`

Extended notes for [`src/effects/tests/policy.rs`](../../../../src/effects/tests/policy.rs).

The code is the authority for what it does. These notes started as the module's source prose.
Each code fragment in a heading is an exact source substring. When a heading names an enclosing
item before `›`, find that item first, then the following fragment within it.

## Module

The policy tables and the placement scan's prologue reader: the parts of
the allowlist, denylist and refusal tests that are **definitions** rather
than machinery.

Three of the four items here are tables transcribed from a source outside
this crate -- `decisions.effect_site_inventory.mechanism`'s own sentence,
and the set of denied paths a given host cannot resolve. The fourth reads
the prologue above an attribute. None of them performs an effect, and the
`#[test]` wrappers that drive them all stay in `super`: this module is the
definition, not the harness, and no name in it is a test name.

**The effectful build helpers deliberately did not come with them.**
`unresolved_paths`, `extern_dependencies`, `lint_fixture`, `clippy_driver`,
`crate_under_test` and `scratch_dir` drive `clippy-driver` over fixtures
they create, and they stay in `super` because a file reached by a plain
`mod` declaration is inside every whole-tree census's domain --
`census_domain::whole_file_test_modules` derives its skip set from the
crate's `cfg(test)` module declarations alone, and this file is not one of
them. Measured, not assumed: with those six helpers moved here,
`runner::tests::every_production_process_start_is_classified` gains a
`("src/effects/tests/policy.rs", 3, 0, 0)` row and
`every_production_command_spec_payload_is_classified` a `(0, 2, 0)` row,
and closing either means classifying a test file in `src/runner/mod.rs`'s
production tables -- a boundary claim, not a mechanical consequence of a
move. The narrow cut is the one that needs no such claim.

That declaration form is deliberately not spelled out anywhere in this
file. One written inside a comment is the exact shape that once derived a
phantom skip and removed a real file from every census below it, and the
blanking that now defeats it is not a reason to write another.

The three effect denials are **restored** here rather than inherited.
`super` allows them because it drives a compiler over fixtures; nothing in
this file does, so the allowance has no business reaching it. That is also
what keeps this module out of `effects/allowlist.toml`: an allowance is
what that file records, and this module takes none.

## `pub(super) fn marker_before(source: &str, line: usize, inner: bool) -> String {`

---------------------------------------------------------------------------
(2) The allow-placement scan
---------------------------------------------------------------------------

## `pub(super) fn marker_before(source: &str, line: usize, inner: bool) -> String {`

The prologue text on the ten lines above the attribute, from the original
source — comments included, because the marker *is* a comment.

## `pub(super) fn marker_before(source: &str, line: usize, inner: bool) -> String {` › `let start = if inner { 0 } else { line.saturating_sub(13) };`

A file-level inner attribute is preceded by the module's whole prologue,
and lane A's `# LEGACY-EFFECT` sections are doc-comment headings sixteen
lines long. An outer attribute on an inner `mod` gets a window.

## `pub(super) const PACKET_PRIMITIVES: &[&str] = &[`

---------------------------------------------------------------------------
(1) The denylist
---------------------------------------------------------------------------

## `pub(super) const PACKET_PRIMITIVES: &[&str] = &[`

The primitives `mechanism` (1) enumerates, transcribed from the packet.

An independent table, which is the whole point: checking `clippy.toml`
against itself would pass however much of the sentence it had dropped. The
sentence, in order, is

> "std::fs write/create/remove_file/remove_dir/remove_dir_all/rename/copy/
> hard_link/set_permissions/create_dir/create_dir_all/DirBuilder,
> File::create/create_new/options/set_len/sync_data/sync_all,
> io::Write::write_all/flush on files, OpenOptions, symlink creation on both
> platforms, std::process::Command (type) and its spawn/output/status, libc
> fork/kill/setpgid/setsid/flock/fcntl/exec*, windows_sys process, job, and
> LockFileEx/UnlockFileEx functions, docker invocation helpers, and every
> crate-internal effectful wrapper identified by the wrapper classification
> (e.g., util::write_json)".

## `pub(super) const PACKET_TYPES: &[&str] = &[`

The types and the macro list the same sentence names.

## `pub(super) fn host_conditional_paths() -> Vec<&'static str> {`

The paths this host cannot resolve, and the reason each one is here.

On a Unix host `std::os::windows::fs::*` is a module that does not exist, so
clippy reports it. `windows_sys::*` is a crate that is not linked at all, and
clippy reports **nothing** for those — measured — which is why they are
cross-checked against the tree's own Windows source instead, by
`every_platform_conditional_denial_names_something_real`.

## `pub(super) fn host_conditional_paths() -> Vec<&'static str>` › `vec![`

`libc::pipe2` is Linux-only: the `libc` crate does not define it for
Darwin, so the denial resolves on Linux and does not here. That is the
"a denial that enforces nothing" class `clippy.toml`'s header warns
about -- but it is **vacuous** rather than a hole, because a path that
does not resolve is also a path no macOS code can call. Recorded here
rather than suppressed, so the set stays asserted on every host.

Found by CI, not locally: this project has a Windows guest and no
macOS host, and `PR5-MACOS-CLIPPY-NEVER-RUN` predicted this exact test
would be the one to see it.
