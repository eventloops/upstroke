# `src/connect.rs`

Extended notes for [`src/connect.rs`](../../src/connect.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

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

## `pub struct ConnectOptions` › `pub pools_path: Option<PathBuf>,`

Where to write. `None` takes `~/.upstroke/pools.toml`; tests always set
it, so no test can reach the operator's real pools file.

## `pub struct ConnectOptions` › `pub force: bool,`

Overwrite an existing file that differs.

## `pub enum Wrote {`

What `connect` did, so the CLI can render it and a test can assert on it
without parsing prose.

## `pub enum Wrote` › `Written,`

The file did not exist, or `--force` replaced one that differed.

## `pub enum Wrote` › `Unchanged,`

Configures exactly what is already there, and says exactly the same
thing about it — compared over [`settings_of`] and [`stable_content`]
rather than over bytes.

## `pub enum Wrote` › `Refused,`

An existing file differs and `--force` was not given.

## `pub struct ConnectReport` › `pub content: String,`

The file `connect` produced — written, or merely proposed when it
refused to clobber.

## `pub struct ConnectReport` › `pub agents: Vec<AgentReport>,`

One entry per registered adapter, in registry order.

## `pub struct AgentReport` › `pub outcome: Result<Discovery, String>,`

`Err` means this agent contributed no pool. It never aborts the others:
a machine with Claude Code and no Copilot is the normal case, not a
broken one.

## `pub fn run(opts: &ConnectOptions) -> Result<ConnectReport, UpstrokeError> {`

Discover, render, and write — the whole command.

## `pub fn run_with<'a>(`

The injectable form: `adapters` supplies the implementations and `ids` the
registry order, so a test can drive scripted discovery with no CLI on the
machine at all.

## `let existing_text = fs::read_to_string(&path).ok();`

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

## `fn settings_of(text: &str) -> Vec<String> {`

The settings a pools file actually carries — comments and blank lines
dropped.

"Differs" has to mean *differs in what it configures*, not "differs in
bytes". The header names the write date, so a byte comparison would make
two runs a second apart look like a conflict: every re-`connect` would
refuse, and the only way past it would be `--force`, which is exactly the
flag that discards hand edits. A refusal an operator is trained to bypass
protects nothing.

The other direction holds too: an operator who edits only a comment has
changed no setting, and being told their file conflicts would be noise.

## `fn strip_comment(line: &str) -> String {`

A line with any comment removed, whole-line or trailing.

Trailing matters because `connect` writes one: `render::pool_section` decorates
`reserve` with `# headroom kept for your own interactive sessions`, so the
single line an operator is most likely to tidy is the one a whole-line-only
filter would treat as a changed setting. `#` inside a quoted value is not a
comment, so quotes are tracked.

## `fn stable_content(text: &str) -> Vec<&str> {`

Everything except the one line that moves on its own.

The header records when `connect` ran, so comparing whole bytes would call
two runs a second apart different. Everything else — including every
discovery note and the auth line — is content a reader relies on being
current, so it belongs in the comparison that decides whether to rewrite.

## `fn pool_for_agent(agent: &str, discovery: &Discovery) -> Pool {`

One pool per (agent × discovered account) — today exactly one per agent,
because nothing enumerates credential profiles (see the module docs).

## `fn pool_for_agent(agent: &str, discovery: &Discovery) -> Po…` › `let kind = discovery.shape.unwrap_or(match agent {`

§13's default where the CLI could not say: Copilot's post-Jun-2026
billing is credits, and everything else that reports nothing is treated
as a subscription window — the shape whose estimator is the most
conservative of the two. The rendered file carries a comment saying so,
because a default the operator cannot see is a guess wearing a fact's
clothes.

## `fn pool_for_agent(agent: &str, discovery: &Discovery) -> Po…` › `Pool::discovered(`

§13's trust order, minus the sources v0.1 does not read: writing
`local-logs` into a fresh file would promise interactive-usage awareness
that has not been built. An operator who wants it recorded can add it —
the parser accepts it and the estimate says it is unread.

## `struct OperatorKeys {`

The keys only an operator can supply, carried across a `--force`.

`connect` discovers subscriptions; it cannot discover *which account*
(`profile`), *how big* an allowance is (`monthly_allowance`), or where a
local model lives (`endpoint`). All three are hand-written, and rewriting
the file without them would delete the operator's own work — with the
refusal message that recommends `--force` never saying so. `profile` in
particular is the entire point of §13's multi-account seam, and
`monthly_allowance` is the only thing that makes a self-metered estimate
possible at all (`Auto` yields `Unknown`).

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

## `mod tests` › `struct FakeAdapter {`

A scripted stand-in, so these tests run on a machine with no agent CLI
installed at all.

## `fn machine() -> Machine` › `FakeAdapter {`

Installed nowhere: the normal single-vendor machine.

## `fn what_connect_writes_parses_back_into_the_pools_it_descri…` › `let path = scratch("roundtrip");`

The round trip is the whole contract: a file this command writes must
be one `config::load` accepts, or `upstroke capacity` reports on
something `connect` cannot produce.

## `fn an_existing_file_that_differs_is_never_clobbered()` › `let path = scratch("clobber");`

§17 says the file is hand-editable, so silently overwriting a hand
edit destroys the operator's own record of their subscriptions.

## `fn an_existing_file_that_differs_is_never_clobbered()` › `let forced = connect(&path, true);`

--force is the escape hatch, and it really does replace — but it
carries the operator's own keys across. `profile` is the whole point
of §13's multi-account seam and discovery cannot supply it, so a
replacement that dropped it would silently delete the one setting
the refusal above existed to protect.

## `fn re_connecting_an_unchanged_machine_reports_unchanged_rat…` › `let path = scratch("idempotent");`

The header names the write date, so a byte comparison would call
every second run a conflict — and the only way past a conflict is
`--force`, the flag that discards hand edits. A refusal an operator
is trained to bypass protects nothing, so the comparison is over
settings, not bytes.

## `fn re_connecting_an_unchanged_machine_reports_unchanged_rat…` › `fs::write(&path, format!("# my own note\n{first}")).expect("annotate");`

A comment-only difference is never a *conflict* — settings are what
may not be clobbered — but it is a rewrite, because the comments are
where discovery's findings live. The trade is deliberate: a note an
operator adds is regenerated away, and in exchange a login between
two connects cannot leave the file insisting they are signed out.
Their real edits (`profile`, `monthly_allowance`, `endpoint`) survive
both paths — see `an_existing_file_that_differs_is_never_clobbered`.

## `fn a_login_between_connects_updates_the_file()` › `let path = scratch("relogin");`

Auth state is rendered only as a comment, so a settings-only
comparison reported `unchanged` and left the file telling an operator
who had just logged in that they were not signed in.

## `fn a_cli_that_lists_models_is_cross_checked_against_the_cat…` › `let machine = Machine {`

D1's guard. It cannot fire against a real CLI today — neither
enumerates models — so it is driven through a scripted discovery
that does, which is the shape the check exists for.

## `fn a_cli_that_lists_models_is_cross_checked_against_the_cat…` › `models: [`

A roster that has moved on without the catalog.
Overlaps the roster — zero overlap is a format
mismatch, not a stale catalog — but has moved on from
the frontier slug the second opinion depends on.

## `fn an_undetectable_plan_shape_takes_a_default_and_says_so()` › `let machine = Machine {`

The Copilot case: §13 gives it two billing shapes and the CLI
distinguishes neither. A default is fine; a silent default is not.

## `fn discovery_against_the_real_claude_binary_when_present()` › `let runner = crate::runner::host::HostRunner::new();`

§13's discovery is a claim about a real CLI, so it is checked against
one where the machine has it — and skipped cleanly where it does not,
which is the shape every other binary-touching test here takes.

## `fn discovery_against_the_real_claude_binary_when_present()` › `assert!(`

Whatever it answers, it must be one of the three states and it must
explain itself — including when the answer is "could not tell".
