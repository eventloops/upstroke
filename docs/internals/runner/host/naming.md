# `src/runner/host/naming.rs`

Extended notes for [`src/runner/host/naming.rs`](../../../../src/runner/host/naming.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Program naming and resolution: which file a program **name** is, at this
boundary.

`PR4-ADAPTER-RESOLVES-ON-THE-HOST`'s second clause and `PR6D-001`'s grid.
Platform-as-value throughout: [`ProgramNaming`]'s two variants are
constructible on both platforms, so the Windows naming rule is executed by
the Linux suite on every run. The one genuine `cfg` is [`executable_bit`],
which asks this filesystem a question the other platform has no answer to.

It decides rather than acts. The only filesystem contact is the read-only
probing a search *is* -- `Path::is_file` and the execute bit -- and the
memoisation that makes the answer per-boundary rather than per-spawn is the
parent's, in `HostRunner::program_for`.

## `#![deny(`

**This child states its own lint level and inherits nothing.** A Rust lint
level is scoped by the module tree and not by the file, so an out-of-line
child of `src/runner/host.rs` would otherwise inherit that file's inner
`#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
-- `PR6-LANEF-004`, and the mistake two W1 pull requests each made
independently. Nothing here reaches a governed primitive, so all three are
DENIED rather than allowed, and this module takes no `effects/allowlist.toml`
row: an allowance is what that file records, and this module takes none.
`runner::container::tests::every_child_module_of_the_container_funnel_states_\
its_own_lint_level` already walks `src/runner/host/`, so this file was graded
against all three from its first commit.

## `pub(super) enum ProgramNaming {`

How a platform turns a program **name** into the file names it may be.

A type rather than a `cfg!` at each comparison, for [`KeyCase`]'s reason and
with more at stake: `PR6D-001` is a rule whose Windows arm no Linux machine
could reach, and it shipped because every fixture that could have caught it
was `#[cfg(windows)]` and every Windows fixture used an absolute path. Both
variants are constructible on both platforms, so the Windows naming rule is
executed by the Linux suite on every run and not only by the guest.

The *file* predicate is platform-native and cannot be gridded — Windows has
no mode bits — so [`Self::is_program`] degrades to "is a file" wherever the
bits do not exist. Everything above it is pure string work.

## `pub(super) enum ProgramNaming` › `Posix,`

Unix. `/` separates; there are no executable extensions, so `execvp`
tries the name itself in each `PATH` directory and skips a file whose
execute bit is clear rather than failing on it.

## `pub(super) enum ProgramNaming` › `Windows,`

Windows. `\`, `/` and `:` all separate, and an extensionless name is not
a program: a shell appends `PATHEXT`'s entries in order. `CreateProcessW`
appends `.exe` **only**, which is the whole of `PR6D-001` — `PATHEXT`
lists `.CMD`, a shell finds `claude.cmd`, and `std::process::Command`
does not.

## `impl ProgramNaming` › `pub(super) const fn current() -> Self {`

What this platform does.

## `impl ProgramNaming` › `const DEFAULT_PATHEXT: &'static [&'static str] = &[".com", ".exe", ".bat", ".cmd"];`

`PATHEXT`'s entries when nothing sets it.

`cmd.exe`'s own built-in default, **in its order**: `.COM` before `.EXE`
before `.BAT` before `.CMD`. Written here from the platform rather than
borrowed from [`crate::util::executable_extensions`], whose default is a
different order and which also probes the extensionless name — a rule
for a diagnostic, not for a spawn.

## `impl ProgramNaming` › `pub(super) fn is_bare_name(self, program: &str) -> bool {`

Whether `program` is a name for this boundary to resolve, rather than a
location to use as given.

The same partition the platform itself draws: `execvp` searches `PATH`
for a name and never for something containing `/`, and std's Windows
search is reached only by `is_file_name`. A location is therefore handed
to `Command` byte for byte, which is what makes "an absolute program
spawns exactly as it did before this repair" true by construction rather
than by a fixture.

## `pub(super) fn is_bare_name(self, program: &str) -> bool` › `Self::Windows => matches!(c, '/' | '\\' | ':'),`

`:` because `C:file` is drive-relative and `f:s` names an
alternate data stream; neither is a name to search `PATH` for.

## `impl ProgramNaming` › `fn candidates(self, program: &str, pathext: Option<&OsStr>) -> Vec<OsString> {`

The file names `program` may be, in the order a shell tries them.

Windows: a name that already carries an extension is tried verbatim
first and then with each `PATHEXT` entry appended; a name without one is
**not** tried verbatim, because an extensionless file is not a program
there — `CreateProcessW` appends `.exe` to it and `cmd.exe` appends
`PATHEXT`. Trying it anyway would let a data file called `claude` sitting
in a `PATH` directory shadow the real `claude.exe`.

Unix: the name, and nothing else.

## `impl ProgramNaming` › `fn extensions(pathext: Option<&OsStr>) -> Vec<String> {`

`PATHEXT` as a list, or the platform default when it is unset, empty, or
carries nothing usable.

An entry that does not start with `.` is dropped rather than joined —
`PATHEXT=exe` would otherwise produce `claudeexe` — and an entry list
that ends up empty falls back to the default rather than to "no
candidates at all", because a `PATHEXT` of `;;;` is a malformed variable
and not an instruction that this machine has no programs.

## `impl ProgramNaming` › `pub(super) fn is_program(self, path: &Path) -> bool {`

Whether this file is one a spawn of that name would reach.

Unix checks the execute bit because `execvp` does: a non-executable
`claude` in an early `PATH` directory is skipped there, and a resolution
that stopped at it would refuse — or spawn `EACCES` — where the old code
found the real one further along. Windows has no such bit, so existence
is the whole question there.

## `fn executable_bit(path: &Path) -> bool {`

The execute bit, where the platform has one.

## `fn executable_bit(_path: &Path) -> bool {`

Windows files carry no execute bit, so `ProgramNaming::Posix` degrades to
existence when a grid drives it there. Nothing in production reaches this.

## `pub(super) fn composed_value<'a>(`

The value of `key` in a composed environment, under this platform's name
rule.

## `pub(super) fn resolve_program(`

Which file `program` names, at this boundary.

The second clause of `PR4-ADAPTER-RESOLVES-ON-THE-HOST`: the adapter names
the CLI and consults no filesystem, and the runner resolves that name
against **the environment it composes**. `composed` is that environment —
the one the child is about to be given — so pre-flight and the attempt
resolve identically because they compose identically (DESIGN.md §8).

One rule for every program this boundary runs. `gates::ShellKind::spec` has
always shipped a bare `sh`, `bash`, `cmd` or `pwsh` and the three agent CLIs
now do too; a second rule for one of them is how `PR6D-001` happened.

**What it deliberately does not search.** std's Windows fallbacks — the
application directory, the system directory, the Windows directory, and the
*parent* process's `PATH` — are not consulted. A runner that owns the
environment (DESIGN.md §6) and then reaches outside it for a program is
composing one environment and resolving against another, which is the class
of bug this function exists to close. In production the composed `PATH` is
the coordinator process's own (`PATH` is reserved, so no overlay can move
it), and `%SystemRoot%\System32` is on it on every Windows installation, so
the narrowing is reachable only by a caller that supplies a `HostEnvironment`
with a `PATH` of its own — which is exactly the caller that meant it.

It also does not search a `PATH` entry that is not absolute — the empty
entry of `PR6-LANED-003` and every other spelling of "the current
directory". The reason is in the loop below; the short form is that this
runner's current directory is the workspace.

### Errors

[`UpstrokeError::Refused`] naming the program, the boundary and the `PATH` it
searched, when a bare name matches nothing. Fail-closed on purpose: the
alternative is handing the name to `Command` anyway and letting the spawn
fail with a bare `NotFound` that names no boundary, which on Windows is
precisely the failure an operator could not diagnose.

## `return Ok(PathBuf::from(program));`

A location, used as given — no probing, no extension, nothing this
machine contributed. This is every absolute program the suite and the
v0.1 product already spawn, and it must not change.

## `if !dir.is_absolute() {`

**`PR6-LANED-003`.** A `PATH` entry that does not name a location on
its own names one *relative to a current directory*, and this
runner's current directory is the workspace — repository content,
under automation. DESIGN.md §15 is explicit that repository
content executing with this process's authority is the threat the
container runner exists to bound; the host runner cannot bound it for
gate code, but the *agent* is not gate code and must not become a way
in. An **empty** entry is the finding's own case and the degenerate
one — POSIX gives a null prefix the meaning "the current directory",
so `PATH=:/usr/bin` with a `claude` in the workspace is a
workspace-controlled agent.

Fail-closed, and it costs a real capability: a program reachable only
through a relative `PATH` entry is refused rather than run. That is
the right side to fail on. The alternative is worse than it looks —
this predicate runs against the *coordinator's* current directory
while the child runs against the *workspace* — so a relative entry
does not merely widen the search, it lets the runner certify one file
and execute another, which is DESIGN.md §21 in the same breath.

`Path::is_absolute` rather than a [`ProgramNaming`] rule: like
[`ProgramNaming::is_program`], this is a question about *this*
filesystem's paths rather than about how a name is spelled, and
`std::env::split_paths` is already the platform's own splitter. The
rule the grid does execute on both platforms is the one above it.

## `for candidate in &candidates {`

Directory outermost, candidate innermost: `PATH` order decides
between installations and `PATHEXT` order decides only within one
directory. The other nesting promotes a later directory over an
earlier installation, which is the shape the deleted
`find_program_candidates` test pinned.
