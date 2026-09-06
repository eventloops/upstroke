# `src/connect.rs`

Extended notes for [`src/connect.rs`](../../src/connect.rs).

These notes retain the module prose from the landed renderer implementation while
applying the comment migration. Source fragments in headings identify the matching code.

## Module

`upstroke connect` (DESIGN.md §13, §18): discover the agent CLIs on this
machine and write `~/.upstroke/pools.toml`.

**Invariant 2 is the one to watch here.** Connect subprocesses the vendors'
own CLIs and parses what they print. No HTTP, no token ever handled, no
credential file read — a vendor CLI talking to its own vendor is the design,
not a leak, and it is the same posture §9 sets for plan importers.

Two things this deliberately does not do:

- **It never invents a profile.** §13 wants `connect` to enumerate
  credential profiles, not just binaries, so that one vendor can back
  several pools. There is no vendor registry of profiles to enumerate — the
  mechanism is a config-directory environment variable, not a list — so v0.1
  writes one pool per agent and leaves `profile` for the operator to add by
  hand. See [`crate::capacity`]'s module docs for the v0.2 sketch.
- **It never clobbers.** §17 calls the pools file hand-editable, and it is
  the file that says which subscriptions exist. An existing file whose
  *settings* differ is printed and the command exits asking for `--force`;
  one that already says the same thing reports "unchanged" and rewrites
  nothing. `--force` still carries the operator's own keys across, because
  `profile`, `monthly_allowance` and `endpoint` are things discovery cannot
  supply and replacing the file must not quietly delete.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub pools_path: Option<PathBuf>,`

Where to write. `None` takes `~/.upstroke/pools.toml`; tests always set
it, so no test can reach the operator's real pools file.

## `pub force: bool,`

Overwrite an existing file that differs.

## `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`

What `connect` did, so the CLI can render it and a test can assert on it
without parsing prose.

## `Written,`

The file did not exist, or `--force` replaced one that differed.

## `Unchanged,`

Configures exactly what is already there, and says exactly the same
thing about it — compared over [`settings_match`] and [`stable_content`]
rather than over bytes.

## `Refused,`

An existing file differs and `--force` was not given.

## `pub content: String,`

The file `connect` produced — written, or merely proposed when it
refused to clobber.

## `pub agents: Vec<AgentReport>,`

One entry per registered adapter, in registry order.

## `pub outcome: Result<Discovery, String>,`

`Err` means this agent contributed no pool. It never aborts the others:
a machine with Claude Code and no Copilot is the normal case, not a
broken one.

## `pub fn run(opts: &ConnectOptions) -> Result<ConnectReport, UpstrokeError> {`

Discover, render, and write — the whole command.

## `pub fn run_with<'a>(`

The injectable form: `adapters` supplies the implementations and `ids` the
registry order, so a test can drive scripted discovery with no CLI on the
machine at all.

### Errors

Returns a refusal when no destination can be determined, an I/O error
when the existing file cannot be read, or a filesystem error naming a
failed directory creation or file write.

## `Some(path) => path.clone(),`

The returned report owns its path independently of these options.

## `let existing_text = match fs::read_to_string(&path) {`

Read before anything is written: `--force` must not silently discard the
keys only an operator can supply.

## `if seen.contains(&id) {`

Two entries for one agent would render `[pools.<name>]` twice, and
TOML rejects duplicate keys — so `connect` would write a file that
`config::load` then refuses to read. The built-in registry has no
duplicates, but `run_with` is the public seam and takes any ids.

## `let runner = crate::runner::host::HostRunner::new();`

Probe first: §14 already treats a missing or broken binary as a
refusal to start, and discovery on a CLI that cannot even report its
version would be reading tea leaves.
Its own host Runner, for the reason `capacity` states: `connect`
drives no run, so there is no run's boundary to borrow, and it is
not a coordinator so its children are outside INV-18's ambient job.

## `let missing = crate::catalog::missing_from(id, &discovery.models);`

D1's cross-check, at the moment the roster's provenance is
being written into the file. Claude Code and Copilot report
no roster today; Codex reports its local `debug models`
catalog. Any real listing is where a stale shipped entry
should first be caught.

## `let outcome = match &existing {`

Two comparisons, because two different questions are being asked.

*May* this file be replaced turns on the **settings** — the operator's
hand edits are what must not be clobbered, and a comment carries none.
*Should* it be rewritten turns on everything except the one genuinely
volatile line, the header's timestamp. Collapsing the two into a single
settings comparison meant a login between two connects reported
`unchanged` and left the file still saying NOT signed in; collapsing them
the other way made every re-connect a conflict resolvable only by
`--force`, the flag that discards hand edits.

## `write_pools(&path, &content)?;`

The write boundary already names the operation and failed path.

## `fn write_pools(path: &Path, content: &str) -> Result<(), UpstrokeError> {`

Replace the file after the caller has decided that replacement is allowed.

**What this write guarantees (§8 asks for the words): atomic replacement, and
durability of what it publishes — the file, and any directory entry it had to
create to put the file there.** The content goes to a staging file unique to
this call in the destination's own directory, is flushed there, and is then
renamed onto the destination; the directory entry is flushed after the rename.
A directory component that did not exist is created first and its new entry
flushed in *its* parent ([`create_directory_durably`]), so a first `connect`
into a `~/.upstroke/` that does not exist yet is as durable as a rewrite into
one that does. A reader of the pools file sees either the previous file whole
or this one whole, never a truncated or half-written one, and every failure
before the rename leaves the previous file byte-for-byte intact.

Until PR #189 this was one `fs::write`, which opens the destination with
truncation and writes into it. The caller had decided the file *may* be
replaced; it had not decided the file may be **lost**, and those are different
decisions. A kill, a full filesystem or any failure between the truncation and
the last byte left the operator holding an empty or half-written
`pools.toml` — the `profile`, `monthly_allowance` and `endpoint` that only they
can supply gone, and the file no longer one `config::load` accepts. That is the
sequence `SWEEP-CONNECT-001` names.

Replacement is by rename, so the published file is a new inode. Two things
follow, and both are handled rather than discovered later.

Its mode is the destination's, read before the staging file is filled, so an
operator's `chmod 600` survives a rewrite — see [`apply_mode`], which is a Unix
statement and says why it is only that.

And a rename replaces the *name*, so a `pools.toml` that is a **symlink** — the
shape every dotfile manager gives a configuration file it tracks — would be
replaced by a regular file, leaving the operator's real file behind with stale
contents and their link gone. `fs::write` wrote *through* such a link. So does
this: [`publication_target`] resolves the destination first, and what is staged
and renamed is the file the link names.

### Errors

Names the operation that failed and the path it failed on: reading the kind of
a directory on the way to the destination, creating the directory, flushing a
created directory's entry in its parent, resolving a symlinked destination,
reading the destination's mode, creating, restricting, writing or flushing the
staging file, publishing it, or flushing the directory afterwards. Every failure from the staging file's
creation onwards removes that file — on a return through [`Staged::withdraw`],
which reports a removal that itself fails beside the failure that caused it,
and on an unwind through the guard's `Drop`.

## `fn publish_pools(`

[`write_pools`] over an injectable publication step.

`publish` is `fs::rename` in every build. A test substitutes a step that fails,
or that panics, because the paths taken *after* the staging file exists are the
ones a real rename will not take on demand — a full disk, a revoked permission,
a kill — and those are exactly the paths on which the operator's own file has to
survive. `a_real_publication_failure_names_the_destination_and_removes_the_staged_file`
is the same property through the real `fs::rename`, failed by publishing onto a
directory, so the injected step is not the only evidence that the cleanup runs.

The staging name is `.pools-<ULID>.tmp` in the destination's own directory:
inside it because a rename is only atomic within one filesystem, and unique per
call because §8 refuses a fixed temporary name that two writers can collide on.

The staging file is owned by a [`Staged`] guard from the moment `create_new`
makes it this call's alone until it is either published (`published` disarms the
guard: the name is now the operator's file) or withdrawn (`withdraw` removes it
and reports the failure it was withdrawn for). Those are the two ways a return
leaves this function; an unwind is the third way out, and the guard's `Drop`
removes the file on that path. §6 asks for exactly this shape — "a guard or
resource-owning type beats a `start`/`finish` pair whose second half can be
skipped" — and pass 2 of PR #189 (`SWEEP-CONNECT-005`) found the second half
skipped: until this guard, the cleanup was one `if let Err` that every failing
return passed through and an unwind stepped over, leaving the `.tmp` behind.
`an_unwinding_publication_leaves_the_operators_file_byte_for_byte_intact`
now pins both halves of that path: the destination is untouched *and* the
staging file is gone.

What a killed process leaves behind is still one uniquely named `.tmp` that no
reader of the directory interprets and that the next publication cannot collide
with; no guard runs in a process that is gone.

## `fn create_directory_durably(directory: &Path) -> Result<(), UpstrokeError> {`

`create_dir_all`, with the durability `create_dir_all` does not give. A new
directory is a new entry in its parent, and an entry that has not been flushed
is one a power loss can take back — together with everything published inside
it, however carefully that was flushed. So the missing suffix of the path is
found first (walking up until a component exists; a component that is there
but is not a directory is left for `create_dir_all` to refuse, naming the
directory the operator asked for), the directories are created by the one
primitive, and then each created component's parent is flushed, outermost
first, so that by the time the destination directory itself is flushed after
the rename every entry above it is on disk too. Pass 1 of the recovery review
(`SWEEP-CONNECT-009`) found the first `connect` into a `~/.upstroke/` that did
not exist claiming a durability it had only for the file.

The walk stops at `.` or a root, which always exist. A race — another process
creating the same directory between the walk and `create_dir_all` — costs one
flush of an entry that was already there, never a missing one.

## `struct Staged {`

The staging file, owned: a path this call created with `create_new` and is
therefore entitled to remove, and a flag that says whether it still holds it.
The `bool` rather than an `Option<PathBuf>`: the path is read on every arm and
never absent, so an `Option` would put an `expect` where a field read belongs.

## `fn create(path: PathBuf) -> Result<(Self, fs::File), UpstrokeError> {`

`create_new`, then the guard. A creation that fails owns nothing, so the error
carries the path and no guard exists to remove a file that was never made. The
open handle is returned beside the guard rather than held in it: [`stage`] must
close it before the caller renames or removes the name, because Windows can
refuse both while a handle is open, and a handle inside the guard would live
exactly as long as the name.

## `fn published(mut self) {`

Disarm. The name is now the operator's `pools.toml`; there is nothing of this
call's left to remove.

## `fn withdraw(mut self, error: UpstrokeError) -> UpstrokeError {`

Remove the staging file after a failed publication, reporting a removal that
itself fails beside the failure that caused it. Disarms first, and takes the
path out of the guard rather than cloning it: `self` is consumed, so `Drop`
runs at the end of this method and finds nothing to do.

A `NotFound` is not a failure to report: it means the file this call staged is
already gone, which is the state the removal was for.

## `impl Drop for Staged {`

The unwind path. `published` and `withdraw` are the two ways a return leaves
[`publish_pools`] and both disarm, so an armed guard reaching `drop` is one
whose owner panicked between `create_new` and the rename. The removal is
attempted; a removal that fails cannot be reported from here — the thread is
unwinding, and a panic raised while unwinding aborts the process, which would
cost the diagnosis of whatever actually failed — so that one arm is suppressed,
and the notes say so where the code does. The destination is intact on this
path (nothing renames until `stage` has fully succeeded), so what a suppressed
removal leaves is one uniquely named `.tmp`, the same residue a kill leaves.

`effects/wrappers.toml` classifies this `drop` as `effectful_unnameable`, the
class `src/workspace.rs` uses for its own `Drop`: it performs a filesystem
effect, and a trait method on a private type has no path clippy could deny.
`effects::externally_reachable_fns` derives `drop` from any `impl … for …`
span whatever the type's visibility, so without that row
`every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified`
fails with `src/connect.rs unclassified: ["drop"]` — measured, which is why
the guard and the row land in one change.

## `const SYMLINK_FOLLOW_LIMIT: usize = 40;`

How many links [`publication_target`] follows before calling the chain a cycle:
a chain of exactly this many links is published through, one more is refused.
The value is Linux's own limit for path resolution (`MAXSYMLINKS`), so on
Linux the chains this refuses are exactly the chains the kernel refuses
`fs::write` through with `ELOOP`. It is upstroke's limit, not the platform's:
macOS resolves at most 32 and Windows caps reparse points lower still, and on
those platforms a chain longer than the platform allows never reaches this
walk — `run_with` reads the existing file *through* the chain first, and that
read fails with the platform's own error.

## `fn publication_target(path: &Path) -> Result<PathBuf, UpstrokeError> {`

The file a publication actually replaces: the destination, or — when the
destination is a symlink — the first non-link the chain of links reaches,
existing or not.

A rename replaces a name. Without this, `upstroke connect --force` over a
symlinked `pools.toml` would leave a regular file where the operator's link was
and their real file untouched and stale, and it would do it silently. `fs::write`
followed the link, and so does this.

The chain is walked one link at a time, each relative target resolved against
the directory of the link that holds it ([`named_target`]), until the path is
not a symlink: a regular file, which is replaced, or nothing at all, which is
created — the path `fs::write` would have created through the same chain, so a
dangling link is filled in rather than replaced. That is the shape
`SWEEP-CONNECT-006` asked for. The first version resolved a live chain with
`canonicalize` and fell back to *one* `read_link` when `canonicalize` reported
`NotFound`; for `pools.toml -> intermediate -> absent-final` that fallback
published onto `intermediate`, turning the operator's second link into a
regular file and leaving `absent-final` absent, where `fs::write` created it
with both links intact. `a_chain_of_pools_symlinks_publishes_the_file_the_last_link_names`
is that chain, with the intermediate link in a subdirectory and a relative
target, so a resolution against the wrong directory is caught too.

The walk is bounded by [`SYMLINK_FOLLOW_LIMIT`], and the bound is applied to
the *link about to be followed*, after the path reached by the previous follow
has been inspected: a chain of exactly the limit's length ends on a non-link
that the last inspection accepts, and only the link past it is refused. Pass 1
of the recovery review (`SWEEP-CONNECT-010`) caught the first version
counting follows in a `for` and refusing a chain of exactly 40, which Linux
itself follows;
`a_chain_at_the_follow_limit_is_published_and_one_past_it_is_refused` now
publishes through 39 and 40 links and refuses 41. A cycle is refused by the
same bound, because a cycle is a chain with no end: after the limit's worth of
follows it is still a link. The refusal names the destination the operator
gave, not the link the walk happened to stop on. There is deliberately no
cycle-shaped fixture: a test that hands a genuine cycle to a walk whose bound
a mutation has removed does not fail, it never returns, and the bound's
witness is a chain one link longer than the limit, which such a walk publishes
through and the test then sees as a wrong answer rather than no answer. Only a
real `NotFound` on the path being examined is absence (§7); anything else the
stat refuses is reported rather than treated as "no link here".

The directory that is *created* is still the one the operator named, never one
behind a link: `create_dir_all` runs before this resolution, on `path`'s own
parent, so a `--pools` whose parent is a regular file still reports a failed
directory creation naming that parent, and a link into a directory that does
not exist fails at the staging file rather than quietly building a tree
somewhere else.

## `fn named_target(link: &Path) -> Result<PathBuf, UpstrokeError> {`

The path one link names, resolved against that link's own directory when it is
relative — which is how the kernel resolves it, and how `fs::write` through
that link would have. Called once per link of a chain, so "the link's own
directory" is the directory of the link being read, not of the destination the
operator named.

## `fn publication_directory(path: &Path) -> Option<&Path> {`

The directory a destination is published into.

Its parent, except that the parent of a bare relative name is `""`, which names
the working directory to `join` and nothing at all to `create_dir_all` or
`File::open`. `--pools pools.toml` is a spelling an operator can pass, so the
empty parent is spelled `.` instead. `None` is a filesystem root, which names no
file to publish and is refused rather than written to.

## `fn destination_mode(path: &Path) -> Result<Option<fs::Permissions>, UpstrokeError> {`

The mode to carry onto the replacement, or `None` when there is nothing at the
destination to carry one from.

Only a real absence is absence (§7): a destination whose mode cannot be read is
not one to republish under wider permissions than the operator gave it, so a
stat that fails for any reason other than `NotFound` refuses.

## `fn stage(`

Restrict the staging file, fill it, and flush it. The file was created by
[`Staged::create`] — `create_new` rather than a create-or-truncate, so an
existing name of any kind, including a symlink planted in the directory, is
refused instead of followed (§8's exclusivity rule), which is what makes the
staged file this call's alone to publish or remove.

The mode is applied before the content, not after: a file the operator had
restricted must not exist under wider bits while it holds anything.

The flush is `util::fsync_file`, before the rename rather than after it, because
a rename publishes a *name* and §8 counts atomicity and durability separately —
"a successful rename is not durability". The handle closes at the end of this
function, before the caller renames or removes the name, because Windows can
refuse both while a handle is open.

## `#[cfg(unix)]`

Carry the destination's mode onto the staging file, on the platform where a
`Permissions` is the mode bits.

## `#[cfg(not(unix))]`

Carry nothing, deliberately.

A `Permissions` off Windows' `metadata` carries only the read-only attribute.
Setting it on the staging file would refuse the publication over a read-only
destination *and* leave behind a `.tmp` that cannot be removed, in exchange for
an attribute the rename does not carry anyway. A file created here takes the ACL
its directory gives it, which is what a *new* pools file always took; carrying a
Windows ACL across a replacement is not attempted and is not claimed.

The same limit holds on Unix for anything beyond the mode bits. What
[`apply_mode`] carries is the `st_mode` permission bits and nothing else: a
POSIX access ACL (`setfacl`), a macOS ACL (`chmod +a`), or any other extended
attribute the operator put on the previous file is on the previous *inode*, and
the rename publishes a new one that took only what its directory gave it. The
in-place `fs::write` this replaced kept the inode and therefore kept those.
`SWEEP-CONNECT-003` records that consequence for every platform, not for
Windows alone (pass 1 of the recovery review, `SWEEP-CONNECT-011`, found it
recorded as Windows-only).

## `fn settings_match(existing: &str, proposed: &str) -> bool {`

Compare complete TOML documents, preserving the values of every key.

Parsing handles quoted keys, escapes, multiline strings, and comments with
one grammar. The previous line scanner collapsed whitespace inside strings
and could overwrite an operator's renamed pool. Formatting and table order
do not change parsed settings; integer and float values remain distinct so
comparison never rounds an integer into a different value.

A parse failure means there is no evidence that replacement preserves the
settings. The caller reports Refused unless the operator supplied --force.

## `fn stable_content(text: &str) -> Vec<&str> {`

Everything except the first line when it is the generated timestamp header.

The header records when `connect` ran, so comparing whole bytes would call
two runs a second apart different. Everything else — including every
discovery note and the auth line — is content a reader relies on being
current, so it belongs in the comparison that decides whether to rewrite.

## `fn pool_for_agent(agent: &str, discovery: &Discovery) -> Pool {`

One pool per (agent × discovered account) — today exactly one per agent,
because nothing enumerates credential profiles (see the module docs).

## `let kind = discovery.shape.unwrap_or(match agent {`

§13's default where the CLI could not say: Copilot's post-Jun-2026
billing is credits, and everything else that reports nothing is treated
as a subscription window — the shape whose estimator is the most
conservative of the two. The rendered file carries a comment saying so,
because a default the operator cannot see is a guess wearing a fact's
clothes.

## `Pool::discovered(`

§13's trust order, minus the sources v0.1 does not read: writing
`local-logs` into a fresh file would promise interactive-usage awareness
that has not been built. An operator who wants it recorded can add it —
the parser accepts it and the estimate says it is unread.

## `#[derive(Debug, Default, PartialEq, serde::Deserialize)]`

The keys only an operator can supply, carried across a `--force`.

`connect` discovers subscriptions; it cannot discover *which account*
(`profile`), *how big* an allowance is (`monthly_allowance`), or where a
local model lives (`endpoint`). All three are hand-written, and rewriting
the file without them would delete the operator's own work — with the
refusal message that recommends `--force` never saying so. `profile` in
particular is the entire point of §13's multi-account seam, and
`monthly_allowance` is the only thing that makes a self-metered estimate
possible at all (`Auto` yields `Unknown`).

## `fn apply(self, pool: &mut Pool) -> Result<(), InvalidAllowance> {`

Move the operator's valid keys into the new pool. An invalid allowance
leaves the discovered Auto default and reports why to the caller, which
adds the pool name to the visible warning.

## `pool.monthly_allowance = allowance_of(&value)?;`

The error identifies this setting; run_with supplies the pool
context and decides how to report the rejected value.

## `Ok(crate::capacity::Allowance::Units(*units as f64))`

Every positive i64 fits the finite f64 range. Conversion uses
the same capacity representation as config::read.

## `fn operator_keys(text: &str) -> std::collections::BTreeMap<String, OperatorKeys> {`

Pull the operator-written keys out of an existing pools file, by pool name.

Parsed leniently on purpose: a file this cannot read is one `--force` was
always going to replace, and failing the whole command over it would be
worse than losing keys that were unreadable anyway.

## `fn default_pool_name(agent: &str) -> &str {`

The pool name for an agent: the agent's own id.

Deliberately not a plan name. Naming every Claude Code pool `claude-max`
asserted a subscription tier discovery never established — a Pro subscriber,
or someone on API-key billing, got a pool claiming a plan they do not have,
in the one file whose whole purpose is to describe their actual
subscriptions, from a module that marks its other defaults as defaults. It
also put a per-agent alias table here, so adding an adapter meant editing
`connect`. Renaming the pool is the operator's call, and the file is
hand-editable precisely so they can make it.

## `pub fn render_report(report: &ConnectReport) -> String {`

What the CLI prints.

The body is `render::report`. This name stays here because it is the one
`main` calls and the one `effects/wrappers.toml` classifies, and moving it
would change a public path and a census anchor rather than a file boundary.

## `impl ConnectReport {`

A refusal to clobber is not an error the operator can fix by retrying, and
exit status is how a script tells the difference.

## `struct FakeAdapter {`

A scripted stand-in, so these tests run on a machine with no agent CLI
installed at all.

## `FakeAdapter {`

Installed nowhere: the normal single-vendor machine.

## `const OPERATORS: u32 = 0o740;`

A mode ordinary file creation cannot produce. A fresh file's mode is
`0o666 & !umask`, which never carries an execute bit, so `0o740` differs from
what any umask hands the staging file and the assertion witnesses the carrying
under every umask a runner can have. The previous witness used `0o640` and a
control file that refused to proceed when a fresh file already had that mode —
which is exactly what umask `0027`, a common hardened default, gives a fresh
file (`0o666 & !0o027 = 0o640`), so the test failed there before `connect` was
reached (`SWEEP-CONNECT-007`). The control is gone with the reason for it.

## `fn chain_of(tree: &Path, count: usize) -> (PathBuf, PathBuf) {`

A chain of `count` relative links ending on a file that does not exist, so the
boundary tests can be spelled in the limit's own terms: one under, at, and one
over. Every test of the bound goes through `write_pools` rather than `connect`,
because `connect` reads the existing file *through* the chain first and the
platform's own follow limit (32 on macOS) would fail that read before the walk
under test ran.

## `let old = first.content.replace(`

The previous renderer used f64 Display, which spells 1e16 as this
valid i64. TOML integers and floats remain distinct in comparison.

## `let annotated = format!("{}{} operator note\n", first.content, render::WRITTEN_BY);`

The first line is the only generated timestamp header. A later
comment using the same words remains part of the content comparison.

## `crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-roundtrip")`

The round trip is the whole contract: a file this command writes must
be one `config::load` accepts, or `upstroke capacity` reports on
something `connect` cannot produce.

## `crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-clobber")`

§17 says the file is hand-editable, so silently overwriting a hand
edit destroys the operator's own record of their subscriptions.

## `let forced = connect(&path, true);`

--force is the escape hatch, and it really does replace — but it
carries the operator's own keys across. `profile` is the whole point
of §13's multi-account seam and discovery cannot supply it, so a
replacement that dropped it would silently delete the one setting
the refusal above existed to protect.

## `crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-escapes")`

§13 calls `profile` a config-directory path, and on Windows a path
holds backslashes. The parent parses the operator's spelling and the
renderer writes the value back; written raw, `\U` and `\.` are TOML
escapes, so `--force` produced a file `config::load` refused with a
parse error and the next `connect` read no keys from — the loss the
carrying exists to prevent, on the path that recommends `--force`.

## `let again = connect(&path, false);`

The written spelling reads back into the same keys, so a second
connect carries them again and finds nothing to rewrite — which is
the round trip the two comparisons in `run_with` depend on.

## `let machine = Machine {`

Pass 1 of PR #168: `run_with` is public and takes any adapter id, and
the renderer now quotes a name that is not a bare key, so an id
holding `"` followed by `#` writes `[pools."x\"#A"]`. A `strip_comment`
that read `\"` as the closing quote cut both that header and the
operator's renamed `[pools."x\"#B"]` down to `[pools."x\"`, called
the settings equal, and — since the text differed — rewrote the file,
undoing the rename without `--force`.

## `crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-unparseable")`

`operator_keys` reads the existing file leniently: one it cannot
parse carries no keys at all. The refusal must therefore not tell the
operator their keys "are carried" — pass 1 of PR #168 found that it
did — but send them to the proposed text, which is what `--force`
writes, and say when carrying happens.

## `crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-idempotent")`

The header names the write date, so a byte comparison would call
every second run a conflict — and the only way past a conflict is
`--force`, the flag that discards hand edits. A refusal an operator
is trained to bypass protects nothing, so the comparison is over
settings, not bytes.

## `fs::write(&path, format!("# my own note\n{first}")).expect("annotate");`

A comment-only difference is never a *conflict* — settings are what
may not be clobbered — but it is a rewrite, because the comments are
where discovery's findings live. The trade is deliberate: a note an
operator adds is regenerated away, and in exchange a login between
two connects cannot leave the file insisting they are signed out.
Their real edits (`profile`, `monthly_allowance`, `endpoint`) survive
both paths — see `an_existing_file_that_differs_is_never_clobbered`.

## `crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-relogin")`

Auth state is rendered only as a comment, so a settings-only
comparison reported `unchanged` and left the file telling an operator
who had just logged in that they were not signed in.

## `let machine = Machine {`

D1's guard. It cannot fire against a real CLI today — neither
enumerates models — so it is driven through a scripted discovery
that does, which is the shape the check exists for.

## `models: [`

A roster that has moved on without the catalog.
Overlaps the roster — zero overlap is a format
mismatch, not a stale catalog — but has moved on from
the frontier slug the second opinion depends on.

## `let machine = Machine {`

The Copilot case: §13 gives it two billing shapes and the CLI
distinguishes neither. A default is fine; a silent default is not.

## `let runner = crate::runner::host::HostRunner::new();`

§13's discovery is a claim about a real CLI, so it is checked against
one where the machine has it — and skipped cleanly where it does not,
which is the shape every other binary-touching test here takes.

## `assert!(`

Whatever it answers, it must be one of the three states and it must
explain itself — including when the answer is "could not tell".
