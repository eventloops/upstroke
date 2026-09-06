# `src/util.rs`

Extended notes for [`src/util.rs`](../../src/util.rs).

These notes preserve the module comments after the status repairs. Item headings quote source lines for navigation.

## Module

Small shared helpers used across the engine, gates, adapters, and
reporting: text truncation, filename sanitizing, PATH program resolution,
run-artifact writes, and event timestamps.

## `pub fn tail(text: &str, max: usize) -> String {`

Last `max` bytes of trimmed text, cut on a char boundary, with an ellipsis
marker when truncated.

## `let start = (start..trimmed.len())`

No boundary in range (possible only for a tiny `max` landing inside the
final multibyte char) means the whole tail is unusable — keep nothing.

## `pub fn head(text: &str, max: usize) -> String {`

First `max` bytes of trimmed text, cut on a char boundary, with an
ellipsis marker when truncated. For ordered lists whose first entry is the
most important — a reviewer's reasons, say — where [`tail`] would drop
exactly the part that mattered.

## `pub fn fence_for(payload: &str) -> String {`

A fence long enough to quote `payload` without the payload closing it.

Everything the engine quotes back to a model or a human — a diff, an
artifact, an agent's question, an operator's answer — is untrusted text that
routinely contains fences of its own (any repo with markdown does). A fence
that closes early hands the remainder of the payload to the reader as if it
were instructions, so the invariant is load-bearing rather than cosmetic:
it lives in one place so it cannot drift between callers.

## `pub fn filename_component(raw: &str) -> String {`

Make an arbitrary string (task id, gate name — both user-authored) safe to
use as a single file-name component: no separators, no Windows-reserved
characters, no dot-only names, bounded length. Not injective — callers
that need uniqueness must add a discriminator of their own.

## `pub fn executable_extensions() -> Vec<String> {`

Executable extensions to probe on Windows: PATHEXT when set, else a
conservative default. Unix probes the bare name only.

## `pub fn probe_extensions(base: &Path) -> Option<PathBuf> {`

Try `base` plus each executable extension; first hit wins.

## `pub fn user_upstroke_dir() -> Option<PathBuf> {`

The user-level `~/.upstroke` directory: pools live here (§17), and so do the
agent-authored artifacts a run must keep outside the workspace (§15).

`USERPROFILE` wins on Windows because shells like Git Bash set `HOME` to an
MSYS-style path (`/c/Users/...`) that the Windows file APIs cannot open —
trusting it there would write run artifacts somewhere nothing can read them
back. Elsewhere `HOME` is authoritative and `USERPROFILE` is the fallback.

## `pub fn find_program(name: &str) -> Option<PathBuf> {`

Resolve a bare program name against PATH. Empty PATH segments are skipped:
they mean "current directory" to some shells, and resolving a program
against the repo under automation would execute repo-controlled code.

## `#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]`

One durability primitive, as a funnel actually performed it.

The Event lane has had a ledger of these since PR5 opened
(`events::log::SyncRecord`), and `proof_tests[9]` names it — "**the sync
ledger** shows the synced length equal to the file length after open". The
workspace and run-directory lanes had nothing of the kind, and a measured
consequence: deleting the intent file's `fsync`, deleting the containing
directory's `fsync`, and deleting the staged file's `fsync` from every
atomic publication in `rundir` were each invisible to the whole suite
(`PR5-WORKSPACE-015`, `PR5-WORKSPACE-016`, `PR5-RUNDIR-057`). They have to
be: on a machine that does not lose power mid-test, an unsynced file is
byte-for-byte a synced one, and outcomes are all those lanes could check.

The rename is in the ledger beside the two syncs because the claims are
*orderings* — `run_creation` says "write `<name>.tmp`, **fsync**, rename,
**fsync the directory**" — and an ordering is not expressible over a trace
that holds only one of the three things being ordered.

## `Wrote,`

A `write_all` of `len` bytes was performed.

## `Flushed,`

A `flush` was performed.

## `SyncedData,`

A `sync_data` was performed — the append's own durability barrier, as
distinct from [`Self::SyncedFile`]'s `sync_all`.

## `Truncated,`

A file was truncated to `len` bytes.

## `SyncedFile,`

A staged file's own bytes were made durable (`fsync` / `FlushFileBuffers`).

## `Renamed,`

A staged file was renamed onto its published name.

## `SyncedDirectory,`

A directory entry was made durable. Unix only: `sync_dir` is a
documented no-op on Windows, so a Windows trace has the file syncs and
the renames and no directory syncs, and a reader of the evidence can
see which platform produced it.

## `#[derive(Debug, Clone, PartialEq, Eq)]`

One entry in a [`DurabilityLedger`].

**One entry per attempt**, in order, whether or not the primitive returned
`Ok`. "Exactly one primitive attempt and one error" is a claim the packet
makes about an entered append (`invariants[1]`), and a ledger that recorded
only successes could not distinguish one attempt from a retry that failed
twice.

## `pub step: DurableStep,`

What was done.

## `pub path: PathBuf,`

What it was done to.

## `pub len: u64,`

How much of it. For a sync or a truncation this is the **filesystem's
own answer** rather than a number the funnel carried along — a ledger
that reported its own idea of the length could agree with itself while
the file said something else. For [`DurableStep::Wrote`] it is the
number of bytes handed to `write_all`, which is the quantity the claim
"one `write_all` containing both the JSON and its LF commit marker" is
about. Zero when the path has no length to report.

## `#[derive(Debug, Clone, Default)]`

An ordered record of the durability primitives a funnel performed.

Cloning shares the log, so a caller can hand a clone into a funnel body and
still read what the body recorded. Production never constructs a recording
one: [`Self::off`] holds no allocation and every `record` call on it is a
discriminant test.

## `#[must_use]`

A ledger that records nothing. What production passes.

## `#[must_use]`

A ledger that records. What a test passes.

## `#[must_use]`

Whether this ledger records at all.

## `pub fn record(&self, step: DurableStep, path: &Path, len: u64) {`

Append one entry.

## `#[must_use]`

Everything recorded so far, in order.

## `#[must_use]`

Everything recorded so far about `path`, in order.

## `#[must_use]`

The steps recorded so far, in order, with their paths discarded.

## `pub fn clear(&self) {`

Forget everything recorded so far, so a later sequence can be read on
its own rather than as a suffix of a cumulative log.

The cumulative-log trap is not hypothetical here: an ordering assertion
over the *first* match in a log that already held an earlier, unrelated
occurrence is exactly how `PR5-WORKSPACE-022` survived.

## `pub fn read_file_bounded(path: &Path) -> std::io::Result<Vec<u8>> {`

Every byte of `path`, up to the length the file itself declares.

This is [`std::fs::read`] with the one property `std::fs::read` does not
have: **it terminates**. `read_to_end` loops until a read returns zero, and
an endless source — `/dev/zero`, `/dev/full`, a character device someone
symlinked a log to — never returns zero, so `std::fs::read` on one never
returns and grows memory until it is killed. Every caller here is reading a
file *inside a run directory*, which a startup census must classify before a
write command may proceed (`decisions.sequential_substrate.startup_census`),
so "never returns" is a coordinator that holds the worktree lock for ever.

The bound is the file's **own** length, from `fstat` on the already-open
handle rather than from the path, so it cannot be raced by a swap between
the two calls and cannot be talked out of by an argument. It is not a cap:
a regular file is read in full however large it is, so nothing a caller
might need is hidden — the read is bounded, not the answer. A source with no
length (a device, a fifo, a socket) reports zero and contributes nothing,
which every caller here already treats as "no content", the safe direction.

What it does **not** defend: `File::open` on a fifo with no writer blocks in
the kernel before this function sees a handle. That is `std::fs::read`'s
behaviour too and is unchanged here; a run directory holds regular files.

# Errors

[`std::io::Error`] from `open`, `fstat` or `read`, verbatim, so a caller can
still distinguish `NotFound` from a real failure.

## `static BARRIERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);`

How many durability barriers this process has performed.

Test-only, and the reason it exists is `PR5-CONF-012`: the durability ledger
is written by the function it certifies. `let outcome = file.sync_all();` →
`let outcome: io::Result<()> = Ok(());` survived the whole suite, because the
ledger entry is written *beside* the syscall and every trace assertion reads
the entry. A counter here cannot see inside `sync_all` either — nothing on a
machine that does not lose power can — but it can see whether the barrier was
**reached**, which is the half the ledger was standing in for. The other half
is a source census: [`crate::effects`]'s
`every_file_durability_barrier_in_a_funnel_module_goes_through_one_call`
pins that the syscall is inside these two functions and nowhere else, so
deleting it is a failure rather than a silent no-op.

Unconditional rather than `#[cfg(test)]`, for two reasons. A relaxed
increment beside an `fsync` is not measurable — the syscall is six orders of
magnitude more expensive — and a `#[cfg(test)]` item in the middle of this
file would truncate the **production region** every source census in
`src/effects/tests.rs` computes, which cuts at the first `#[cfg(test)]`. That
is a census reading half a module and reporting clean, which is the exact
failure this project has a reconciliation table for.

## `#[cfg_attr(not(test), allow(dead_code))]`

How many times [`fsync_file`] or [`fsync_dir`] has been entered.

Only a test reads it — production performs barriers, it does not count them —
so the non-test build is told, in the same idiom
`agent::proc::memoised_outcome` already uses for its per-platform dead code.
Named rather than cited by line: the citation was three lines out before the
`m6-proc` split moved that item's neighbours, and a line number in a file
this one does not otherwise touch goes stale at every edit to it. Not a
linked path either, because the item is `pub(crate)` and a link to it is
unresolved under a plain `cargo rustdoc`, which is the run the census
compares.

The *counter* stays unconditional; see [`BARRIERS`] for why a
`#[cfg(test)]` item here would truncate every source census's production
region.

## `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]`

The same two counts, **per thread and per half**.

[`BARRIERS`] is process-wide, so an assertion on its delta can only be a
*lower bound* while the suite is threaded — and a lower bound is satisfied by
barriers some other test's thread performed, which is exactly the hole
`PR6-LANEF-001` found: with `util::fsync_file` deleted from a funnel and its
ledger entry left in place, every assertion that reads the ledger still
passed and a process-wide lower bound still passed too.

A funnel performs its barriers on the thread that called it, so a
thread-local delta is **exact**: the count a caller sees is the count its own
call produced. Split into the two halves because `fsync_file` and `fsync_dir`
are two independently droppable predicates — "write `<name>.tmp`, fsync,
rename, **fsync the directory**" — and one counter cannot tell which of them
went away.

## `pub file: u64,`

Entries into [`fsync_file`].

## `pub directory: u64,`

Entries into [`fsync_dir`].

## `#[cfg_attr(not(test), allow(dead_code))]`

What this **thread** has entered so far, file half and directory half.

Only a test reads it, in the same idiom as [`barriers_performed`]. The
counters themselves are unconditional; see [`BARRIERS`] for why a
`#[cfg(test)]` item in this file would truncate every source census's
production region.

## `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`

Which half of the barrier was entered.

## `pub(crate) fn fsync_file(file: &std::fs::File) -> std::io::Result<()> {`

The **file** half of the durability barrier (`PR5-CONF-012`).

One call, shared by every funnel that stages a file before publishing it, so
that "the durability step is still here" is a property a source census can
check rather than a line each caller is trusted to keep.

# Errors

[`std::io::Error`] from `fsync`, verbatim.

## `pub(crate) fn fsync_dir(dir: &Path) -> std::io::Result<()> {`

The **directory** half of the durability barrier, on every platform this
ships on (`PR5-CONF-013`).

A rename is not durable because the renamed file was synced: the durable
thing is the *directory entry*, and it needs its own barrier.
`run_creation` says "write `<name>.tmp`, fsync, rename, **fsync the
directory**"; `scope` requires `Event.OpenLog`'s "directory fsync" and "file
**and directory** after a truncation". Neither carries a platform exception
and Windows is a first-class target (DESIGN.md §1), so the three call sites
used to return `Ok(())` without opening anything on non-unix and the suite
pinned that omission in both directions on purpose.

**Why this is not `File::open(dir)?.sync_all()` everywhere.** Measured on a
Windows Server 2025 guest: std's open refuses a directory outright —
`Os { code: 5, kind: PermissionDenied, message: "Access is denied." }`, 14
tests down — because it does not pass `FILE_FLAG_BACKUP_SEMANTICS`, which is
the flag that makes `CreateFileW` return a *directory* handle at all. So the
documented boundary was a platform fact rather than a preference, and the
way through it is the Win32 call std does not expose.

# Errors

[`std::io::Error`] from the open or from the flush, verbatim, so a caller can
still tell a missing directory from a refused barrier.

## `#[cfg(windows)]`

The access mask [`fsync_dir`] opens a directory with on Windows.

`FlushFileBuffers` documents that the handle must carry **write** access, and
a directory grants `GENERIC_WRITE` as "may add a file or a subdirectory" —
it is not a request to write the directory's bytes, which no caller can do.
Named rather than inlined so that
[`the_directory_barrier_needs_exactly_the_access_it_asks_for`] can drive the
same code path with a mask that is *not* enough and show which half refuses.

## `#[cfg(windows)]`

[`fsync_dir`]'s Windows body, over any access mask.

## `let mut wide: Vec<u16> = dir.as_os_str().encode_wide().collect();`

`CreateFileW` takes a NUL-terminated UTF-16 string, and an interior NUL
would silently truncate the path — so it is refused rather than trimmed.

## `pub mod duration_millis {`

Serialize a `Duration` as whole milliseconds.

Durations ride in both the event log and the report, and serde's default
`{"secs":3,"nanos":120000000}` is neither readable in a JSONL ops log nor
stable across serde's internally-tagged buffering path. Milliseconds are
finer than anything the ledger reports and survive both.

## `pub fn rfc3339_utc_now() -> String {`

Now, as an RFC 3339 UTC timestamp — the `ts` on every event (§15).

Std-only rather than a date dependency: this is one field on one line of
JSON, and the conversion below is a closed-form algorithm with no table and
no locale. A clock that cannot read (`SystemTime` before the epoch) yields
the epoch rather than failing — a timestamp is metadata on the event, and
losing the event to a clock problem would be the worse trade.

## `fn civil_from_days(days: i64) -> (i64, i64, i64) {`

Civil date from a day count since 1970-01-01 (Howard Hinnant's
`civil_from_days`). The era starts on 0000-03-01 so that a leap day always
lands at the end of a cycle, which is what lets the month and day fall out
of integer arithmetic instead of a lookup table.

## `let month = if shifted_month < 10 {`

March is month 0 in the shifted era; roll January and February into the
following calendar year.

## `#[cfg(test)]`

Whether `left` and `right` name the same directory or file on disk.

Test-only, and shared rather than local because the defect it removes is a
class rather than a site. `PathBuf == PathBuf` is a comparison of two
strings, and a test that asserts one against a path production derived is
asserting that two independent spellings of one directory came out
identical. Three environment facts break that, and a Linux CI cell has none
of them:

* macOS symlinks `/var` to `/private/var`, so anything canonicalised
  disagrees textually with the `std::env::temp_dir()` path it came from;
* Windows hands back the 8.3 short name of a directory whose real name is
  long (`C:\Users\RUNNER~1\…` for `runneradmin`), and which spelling you get
  depends on whose user name is long — the CI runner's is, so CI saw it and
  a short-named developer box never can;
* the same Windows path arrives with `/` from git and `\` from the OS.

[`std::fs::canonicalize`] is the normalisation because its contract is
exactly the property wanted: "the canonical, absolute form of the path with
all intermediate components normalized and symbolic links resolved". Two
names for one existing directory therefore canonicalise to one string on
every platform std supports, which makes this comparison mean "the same
directory" on all of them rather than "the same spelling" on one of them.

A path that does not resolve is not the same object as one that does, so
exactly one failure answers `false` — which keeps the negative form
(`!same_path(…)`) honest for a workspace the run has already cleaned up.

# Panics

When *neither* side resolves. Nothing can be concluded from comparing two
absent paths, and answering `false` there would be the same silent pass
this helper exists to remove.

## `assert_eq!(tail("é", 1), "…");`

Cut lands inside the trailing multibyte char: keep nothing rather
than panic on a non-boundary index.

## `assert_eq!(rfc3339_utc(1_709_164_800), "2024-02-29T00:00:00Z");`

Both leap rules: 2024 by the /4 rule, 2000 by the /400 exception.

## `assert_eq!(rfc3339_utc(86_399), "1970-01-01T23:59:59Z");`

A day boundary and the last second before one.

## `let mut stamps = [`

The log is read back with a plain string compare in places; the
zero-padded fixed-width form is what makes that legitimate.

## `let dir = std::env::temp_dir().join(format!("upstroke-util-path-{}", std::process::id()));`

The empty-PATH-segment guard in find_program rests on this: a bare
name must not resolve against the process CWD. Verified by probing
a file that exists in a scratch dir under its bare name.

## `assert!(find_program("bait.txt").is_none());`

find_program must not consult any directory-less candidate.

## `#[test]`

Two spellings of one directory are one directory, and two directories
are not.

The fixture is `.` and `..` rather than a symlink because those are the
one pair of "different string, same directory" that every platform
std supports normalises identically — Windows has no unprivileged
directory symlink, and the macOS `/var` case and the Windows 8.3 case
this helper exists for cannot be built on demand anywhere else. It is
the same mechanism either way: `canonicalize` resolves the path to the
object, and the object is what the assertion means.

## `#[test]`

The directory barrier runs, and runs on **this** platform
(`PR5-CONF-013`).

The two axes this crosses are the *operation* and the *platform*. Every
caller's ledger assertion holds the operation constant — stage, rename,
then a `SyncedDirectory` record — and until this round those assertions
forked on `cfg!(unix)`, so the Windows cell asserted the barrier's
**absence** and the pair "the barrier, on Windows" was never built. What
varies here is the platform, and nothing is `cfg`-gated away: the call
must succeed wherever the suite runs.

A ledger record is not enough on its own — a caller records beside the
call — so this drives the primitive directly, and drives it against a
directory that has just changed, which is the only state the barrier is
ever asked about.

## `let staged = root.join("record.tmp");`

A directory entry that was just created, then just renamed: the two
changes `run_creation` asks to be made durable.

## `let absent = fsync_dir(&root.join("absent"));`

A directory that is not there is an error rather than a silent
success, so a caller cannot be told a name is durable when nothing
was opened at all — which is exactly what the non-unix arm used to do.

## `#[cfg(windows)]`

The Windows access mask is the one the barrier actually needs, and a
weaker one is refused (`PR5-CONF-013`).

`FlushFileBuffers` documents that its handle must carry write access, and
a claim like that is worth exactly as much as the run that checks it —
this project has shipped a "documented" platform boundary that was a
missing flag twice now. So the same code path is driven with
`GENERIC_READ` alone: if that succeeded, `WINDOWS_DIRECTORY_ACCESS` would
be asking for a right it does not need, and if it fails the constant is
pinned to a measured requirement rather than to a doc sentence.

Held constant: the directory, the flags and the share mode. Varying: the
desired access.
