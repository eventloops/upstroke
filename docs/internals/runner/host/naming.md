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
the Linux suite on every run. The genuine `cfg`s are [`executable_bit`],
which asks this filesystem a question the other platform has no answer to,
and `pathext_entries`'s two conversion arms.

It decides rather than acts. The only filesystem contact is one read-only
`fs::metadata` probe per candidate, from which the file type and, on Unix,
the execute bit are both read, and the memoisation that makes the answer
per-boundary rather than per-spawn is the parent's, in
`HostRunner::program_for`.

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

The name is built with `OsString::push` rather than `format!`, so the
extension arrives as the code units [`Self::extensions`] returned. The
program half is a `&str` and is Unicode by construction; the `PATHEXT`
half is not, and formatting is what would have flattened it.

## `impl ProgramNaming` › `fn extensions(pathext: Option<&OsStr>) -> Vec<OsString> {`

`PATHEXT` as a list, or the platform default when it is unset, empty, or
carries nothing usable.

An entry that does not start with `.` is dropped rather than joined —
`PATHEXT=exe` would otherwise produce `claudeexe` — and an entry list
that ends up empty falls back to the default rather than to "no
candidates at all", because a `PATHEXT` of `;;;` is a malformed variable
and not an instruction that this machine has no programs.

`OsString` and not `String`, because the value is a **name**, and this
function's answer is spliced into a path that is then looked up. An
environment variable is not required to be Unicode on either platform —
arbitrary bytes on Unix, an unpaired surrogate on Windows — and
`to_string_lossy` would replace exactly those units with `U+FFFD` and
hand back a name no file has. That is the whole of `SWEEP-HOST-NAMING-002`:
the conversion was for a diagnostic and it decided identity. The
default list is written here as `&str` and converted, which is lossless
because every byte of it is ASCII.

## `impl ProgramNaming` › `pub(super) fn is_program(self, path: &Path) -> Result<bool, std::io::Error> {`

Whether this file is one a spawn of that name would reach.

Unix checks the execute bit because `execvp` does: a non-executable
`claude` in an early `PATH` directory is skipped there, and a resolution
that stopped at it would refuse — or spawn `EACCES` — where the old code
found the real one further along. Windows has no such bit, so existence
is the whole question there.

**Three answers, not two, and one `stat` for all three.** `Ok(false)` is
*this file is not a program*; `Err` is *this filesystem would not say*.
Folding the second into the first is `SWEEP-HOST-NAMING-001`: `Path::is_file`
answers `false` for `EACCES`, `ELOOP`, `EIO` and a timed-out network
mount exactly as it does for a name that is not there, and the caller
then reported that nothing of that name is on the `PATH` — a definite
statement the search had not earned. Only `NotFound` is absence; §7 says
so in general terms, and here the two are operationally different
things: absence continues the search, an undetermined answer ends it.

The metadata is read **once** and every question is answered from that
one reading. The previous shape asked twice — `Path::is_file`, then
`fs::metadata` again inside `executable_bit` — which is a second syscall
whose failure was discarded separately and a second answer that could
disagree with the first if the path changed between them.

`fs::metadata` follows symlinks, as `Path::is_file` did, so a symlink to
an executable stays a program.

### Errors

The `io::Error` of the `stat`, unchanged and unwrapped, for any failure
that is not `NotFound`. This function has nothing to add to it — the
path is the caller's, and the caller is the layer that knows the program
name and the boundary — so it returns the error as it is rather than
naming an operation twice.

## `fn executable_bit(metadata: &std::fs::Metadata) -> bool {`

The execute bit, where the platform has one, read from the caller's
metadata rather than from a second question to the filesystem.

## `fn executable_bit(_metadata: &std::fs::Metadata) -> bool {`

Windows files carry no execute bit, so `ProgramNaming::Posix` degrades to
existence when a grid drives it there. Nothing in production reaches this.

## `fn pathext_entries(pathext: &OsStr) -> Vec<OsString> {`

`PATHEXT`'s entries in this platform's own code units — bytes under
`#[cfg(unix)]`, UTF-16 code units under `#[cfg(windows)]`.

Two arms because the two platforms spell an `OsStr` differently and std
offers no encoding-independent way to split one; these are the same two
accessors `src/agent/bin.rs` uses for the same reason. There is no third
arm: the crate's supported platforms are the three CI legs, and a target
that is neither should fail to build here loudly rather than silently
take a lossy path.

**Each arm is a conversion and nothing else.** The grammar itself is
[`normalised_extensions`], which is generic over the code-unit width, so
what a Windows machine does to `PATHEXT` is decided by code the Linux
suite executes on every run — the property this module is built around,
and the one whose absence let `PR6D-001` ship.

## `fn normalised_extensions<U>(value: &[U]) -> Vec<Vec<U>>`

The `PATHEXT` grammar: split on `;`, trim, ASCII-fold, keep what is an
extension.

Generic over the code unit rather than written twice, so the two
platforms cannot drift apart and so both instantiations are testable
from any platform — `the_pathext_grammar_reads_the_same_over_both_code_unit_widths`
runs the `u16` one on Linux. Every rule it applies is an ASCII rule, and
both encodings are ASCII-transparent supersets, so the grammar never has
to interpret a unit it cannot read: a non-ASCII unit is passed through
untouched, which is exactly the behaviour the lossy conversion destroyed.

`U: From<u8>` builds the two constants it compares against, and
`U: TryInto<u8>` is how a unit is asked whether it is ASCII at all.

## `fn ascii_trimmed<U>(entry: &[U]) -> &[U]`

The entry without its surrounding ASCII whitespace.

`split_first` and `split_last` rather than a range: §7's panic surface
includes slicing, and this walks in from both ends with no index to get
wrong.

## `fn ascii_byte<U>(unit: U) -> Option<u8>`

The unit as an ASCII byte, or `None` when it is not one.

Not a discarded error: the `TryInto` failure means "this unit is wider
than a byte", which together with the `is_ascii` test is a total
classification of the unit rather than an operation that failed. Every
rule above is stated in terms of this one question, so a non-ASCII unit
is never whitespace, never folds, and survives verbatim.

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

[`UpstrokeError::Filesystem`] — `operation: "stat"`, the candidate path,
and the `io::Error` as its `#[source]` — at the first candidate the
filesystem would not answer for, whatever a later `PATH` entry holds.
This refusal is a different claim from the one above and says so: not
"it is not there" but "this is what stopped me being able to tell". The
variant carries the source error rather than a rendering of it, so the
kind survives for a caller that wants to distinguish a permission
problem from a broken mount. Why the first and not the last is under
`Err(source) => {` below.

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

## `Err(source) => {`

The first candidate this filesystem would not answer for ends the
search. `NotFound` is the only miss that continues it.

**The search does not continue past it, and the cost is named here.**
`execvp` and `CreateProcessW` both walk straight past an unreadable
`PATH` entry; measured on the build box (Linux, non-root, 2026-09-06)
with a mode-000 directory holding a copy of a probe ahead of a readable
one, `stat` answers `EACCES`, and both `sh -c 'command -v ...'` and a
direct `execvp` run the later copy. So this function now refuses, at
that entry, a program the platform would have reached two entries
further along, and one directory the coordinator cannot read anywhere
on its own `PATH` fails every spawn through it until the entry is
removed or made readable. The refusal names that entry, which is the
repair an operator makes.

Why that side. The alternative — remember the first such candidate,
keep searching, report it only if nothing matches — was the shape
`b9c73630` shipped, and it discards the error whenever a later entry
holds the program: an unreadable earlier installation changes which
file is certified and nothing records that it happened. §7 forbids
discarding an error through a catch-all match unless the operation is
explicitly best-effort with its observability defined, and this search
has no channel to define it on; a warning or a runner event would need a
design sentence this module cannot write. Pass 2 of PR #185
(`SWEEP-HOST-NAMING-004`) read pass 1's correction the same way. Parity
with `execvp` was never the whole contract in any case: the relative
`PATH` entry above is one the platform searches and this function
refuses, on purpose.

Absence is still claimed only when every miss was `NotFound`, which is
what §7's sentence requires; the change is that an undetermined miss is
reported at once rather than at exhaustion. The error type carries one
path, and that is the path the search stopped at.

## `mod tests {`

Six tests, in the file rather than in `src/runner/host/tests.rs`,
because this module denies the three governed lints and its suite
therefore cannot build a fixture that writes to a filesystem — every
`std::fs` creation primitive is on `clippy.toml`'s disallowed list. All
six are built from values instead, which is also why they are cheap
enough to sit here. Three fixtures serve them, none of which creates
anything: `undeterminable_directory` is a `PATH` entry with an interior
NUL, which every platform rejects before the syscall with `InvalidInput`
rather than `NotFound`, so the undetermined path is reached identically
on all three legs with no permission games — and it asserts the platform
really did answer something other than `NotFound` before returning, so
no test over it can pass vacuously. `never_created_directory` is a name
under the temporary directory that this process owns and never creates
(process id, a per-process counter and the clock), asserted absent with
`symlink_metadata` before use: every candidate under it is a genuine
`NotFound`, whatever another process or a retained fixture has left in
the shared temporary directory (`SWEEP-HOST-NAMING-005`, which is what
pointing `PATH` at the ambient directory itself was). `this_test_binary`
is the directory and bare file name of the running test executable — the
one program every platform has installed and executable by construction,
and so the only "later match" a module that cannot write a file can
offer.

`a_candidate_this_platform_cannot_stat_is_never_reported_as_absence`
searches the undeterminable entry alone and requires the `stat` refusal,
not absence. `a_candidate_that_is_merely_absent_is_still_absence` is its
control on the other side of the same boundary, over the never-created
directory, and each fails under the mutation that removes the arm it is
about.

`an_undetermined_candidate_stops_the_search_before_a_later_match` is
`SWEEP-HOST-NAMING-004`'s witness: the undeterminable entry first, the
test binary's own directory second, and the answer must be the
`Filesystem` variant naming the undeterminable candidate — matched on
the variant's fields, not on its rendering. Restoring `b9c73630`'s
fall-through fails this test and nothing else in the `runner::host`
suite. `a_directory_that_is_merely_absent_is_walked_past_to_a_later_match`
is its control: the never-created directory first, the binary second,
and the binary must be found. Making `NotFound` propagate fails it, the
absence control above, and twenty-one tests of row 44's suite.

`a_pathext_entry_no_string_can_carry_reaches_the_candidate_intact`
drives `ProgramNaming::Windows` — on every platform, per the grid — with
a `PATHEXT` entry the running platform's `OsStr` can hold and no
`String` can: a bare `0x80` byte on Unix, an unpaired surrogate on
Windows. It asserts the fixture is not Unicode first, then that the
candidate carries those units with only the ASCII half folded, then that
the lossy spelling is *not* among the candidates.

`the_pathext_grammar_reads_the_same_over_both_code_unit_widths` is the
one that makes the Windows arm honest on a Linux run: it applies
[`normalised_extensions`] to the same input as bytes and as UTF-16 code
units and requires the same answer.
