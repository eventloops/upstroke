# `src/connect/render.rs`

Extended notes for [`src/connect/render.rs`](../../../src/connect/render.rs).

These notes retain the module prose from the landed renderer implementation while
applying the comment migration. Source fragments in headings identify the matching code.

## Module

What `connect` renders: the pools file it writes, and the summary the CLI
prints once it has.

Both are text over decisions the parent has already made. Nothing here
probes a CLI, derives a pool, reads the file that is already on disk, or
writes anything — `run_with` does all four and hands the results down. That
is the whole cut: the parent keeps discovery, pool derivation, the operator
keys it carries across a `--force`, the two comparisons that decide whether
to rewrite, and the write itself; this module turns what they produced into
strings.

**One input is not the parent's: the clock.** The header records when
`connect` ran, and [`pools_file`] reads the clock to say so. That is the
module's only impure line; [`pools_file_at`] is the same rendering with the
timestamp supplied, and it is what the tests render.

**The pools file is a persisted format, and this module is its writer.** Two
readers parse it back: `config::read` whenever `upstroke run`, `validate` or
`capacity` loads pools, and the parent's `operator_keys` on the next
`connect`. So nothing this module did not write itself is placed in the file
raw. A value or a table key goes through [`toml_string`] or [`toml_key`], a
number through [`toml_number`], and anything written into a `#` line through
[`comment`]. The payloads are a CLI's output (a discovery note), an adapter's
wording and the operator's own keys, none of which promises to be one clean
line of printable text — and a raw one produced a file `config::read`
refused, which stops every command that loads pools, while on the `--force`
path it corrupted the very keys the carrying exists to keep.

**No name here is a public path.** `render_report` stays in `super` under
the name `main` calls and `effects/wrappers.toml` classifies, delegating to
[`report`]; the declaration is a plain private `mod`, so nothing nests under
`connect::render` and `connect`'s externally reachable surface is the same
four functions the wrapper census already records.

## `#![deny(clippy::disallowed_methods, clippy::disallowed_macros)]`

The two effect denials are **restored** here rather than inherited. A lint
level is scoped by the module tree and not by the file, so `super`'s
`#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]` — which it
carries because it creates a directory and writes the operator's pools file —
would otherwise reach every line below. Nothing here touches a file or a
process, so that allowance has no business here, and re-denying is what keeps
this module out of `effects/allowlist.toml`: an allowance is what that file
records, and this module takes none.

Not in tension with a file written entirely with `writeln!`: these render
onto a `String`, which is `std::fmt::Write::write_fmt`, and `clippy.toml`
says in its own words that this is "a different DefId" from the
`std::io::Write::write_fmt` it denies. The `let _ =` on each of them discards
a `fmt::Result` that `String`'s implementation never makes `Err`; it is the
idiom for an infallible write, not a folded error.

## ``pub(super) const WRITTEN_BY: &str = "# Written by `upstroke connect`";``

How every pools file `connect` writes begins, up to the version.

The parent's `stable_content` reads this same constant when deciding whether
to omit the first line from the rewrite comparison. The controlled-timestamp
test in the parent exercises that decision across distinct timestamps.

## `pub(super) fn pools_file(agents: &[AgentReport]) -> String {`

Render the pools file: §17's shape, plus a header saying who wrote it, when,
and where the model roster came from.

This is the one line of the module that reads the clock. The rendering
itself is [`pools_file_at`].

## `pub(super) fn pools_file_at(agents: &[AgentReport], written_at: &str) -> String {`

[`pools_file`] with the header's timestamp supplied.

`written_at` is interpolated into the first comment line and nothing parses
it back, so it is text here rather than a time; the parent's
`stable_content` is what knows that line moves.

## `comment(`

The roster sentence defers to the per-agent notes on purpose. Whether a
CLI lists its models non-interactively is an adapter's finding, stated
in its own note (Codex does and its note says so; Claude Code and
Copilot do not and theirs say that), and a blanket claim here was false
for one of the three.

## `const KIND_IS_A_DEFAULT: &str =`

The sentence the file carries under a pool whose `kind` discovery could not
determine. The summary marks the same pool inline, so an operator who never
opens the file still sees that the kind is a guess.

## `fn usable(report: &AgentReport) -> Result<(&Discovery, &Pool), &str> {`

Whether an agent contributed a pool, and if so which, with the reason where
it did not.

`AgentReport` carries a `Result` and an `Option` that the parent always sets
together — `Ok` with `Some`, `Err` with `None` — so the two other
combinations are unreachable from `run_with`; but the type admits them, and
the two renderers used to read the pair separately, the file printing a
line for `(Ok, None)` and the summary nothing. One reading, here: an agent
is usable when both halves say so, and `Err` means what the field's doc
says — no pool — whatever the other half holds.

## `fn pool_section(out: &mut String, pool: &Pool) {`

One `[pools.<name>]` table.

Every value goes through the encoder for its type. `safety_margin` and
`reserve` are written with two decimals because that is §17's spelling and
both are always §13's defaults here (`Pool::discovered` sets them and
nothing the parent carries changes them), so the rounding is exact.

## `if let Some(profile) = &pool.profile {`

The operator's own keys, written back out. `connect` never invents any of
these — it cannot discover which account, how large an allowance is, or
where a local model lives — but once one is in the file it has to survive
being rewritten, or `--force` would delete exactly what the refusal it
overrides existed to protect. They are the operator's text, parsed by the
parent from whatever spelling the operator chose, so they are the values
most likely to need an escape: a Windows path in `profile` holds
backslashes, and written raw those read as TOML escapes.

## `fn comment(out: &mut String, prefix: &str, text: &str) {`

Write `text` as comment lines, one per line of `text`, each beginning with
`prefix`.

TOML forbids control characters other than tab in a comment (U+0000 to
U+0008, U+000A to U+001F and U+007F), and a line break is the one that does
more than fail the parse: it ends the comment, and the rest of the payload
reaches the reader as a setting. So the payload is split on its line breaks
and each line gets the prefix, and every other forbidden character becomes
a space. An empty payload still occupies one line, so a caller writing one
comment always gets one.

## `let marker = prefix.trim_end_matches(' ');`

A bare `#` on an empty line rather than `# `: the prefix's trailing
spaces are written only ahead of text.

## `fn is_toml_control(c: char) -> bool {`

The characters TOML admits in neither a comment nor an unescaped basic
string: the C0 controls other than tab, and DEL.

## `fn toml_string(text: &str) -> String {`

`text` as a TOML basic string, quotes included.

Always the `"…"` form and never a literal `'…'` one, so all string values
have one deterministic spelling. The escapes are TOML's own — `\"`, `\\`, `\b`, `\t`, `\n`,
`\f`, `\r`, and `\uXXXX` for every other control character, U+007F
included — so the reader gets back exactly the text it was given.

## `fn toml_key(name: &str) -> String {`

`name` as a TOML table key: bare where TOML allows it, quoted otherwise.

A bare key is ASCII letters, digits, `-` and `_`. Every registered adapter
id is one, but `run_with` is a public seam that takes any ids, and a name
with a `.` in it written bare would nest a table rather than name one.

## `fn toml_number(units: f64) -> String {`

A positive, finite allowance as TOML that parses back to the same `f64`.

A whole number is written as an integer, which is how the operator wrote
`300` and what `Display` gives for `300.0`; `Display` never uses an
exponent, though, so `1e300` would become three hundred digits and read as
an integer too large to hold — a syntax error for the whole file. So
anything else takes `Debug`'s shortest round-trip form, which switches to
an exponent where the magnitude needs one. The parent rejects invalid
allowances before building the rendered pool. Whole numbers are bounded at 2^53, below
which every one of them is exact in an `f64` and inside an `i64`.

## `const EXACT_INTEGERS: f64 = 9_007_199_254_740_992.0;`

2^53

## `pub(super) fn report(report: &ConnectReport) -> String {`

What the CLI prints.

Every agent gets a summary line, usable or not, followed by indented lines
for its discovery notes. Each physical note line is prefixed separately and
control characters are replaced with spaces. Thus "no change" and
"could not tell" read differently: the first is the `unchanged:` line at the
end, the second is an agent's auth state on its own line, and a pool whose
kind is a default says so on the same line the file does under the pool.

## `let _ = writeln!(`

The proposed text is the whole answer to "what would --force
do": it is what the parent would write. Whether the operator's
`profile`, `monthly_allowance` and `endpoint` are in it is not
this module's to promise — the parent reads the existing file
leniently and a file it cannot parse carries nothing — so the
refusal names the keys, says when they are carried, and sends
the operator to the text to check, rather than asserting that
they were (pass 1 of PR #168 caught the earlier wording doing so).

## `fn usable_agent(id: &str, discovery: Discovery, pool: Pool) -> AgentReport {`

A usable agent: discovery answered and a pool was derived, as `run_with`
builds one.

## `fn parsed_pool(file: &str, name: &str) -> toml::Table {`

The pools file parsed by the same library `config::read` parses it
with, then one pool's table.

## `let nasty = [`

The operator's keys are parsed from their spelling and written back
in this module's; a Windows path in `profile` is the documented use
(§13 calls it a config-directory path) and it holds backslashes,
which are TOML escapes when written raw. `#` inside a value is the
TOML comment-boundary case, and the rest are the characters TOML
requires escaped in a basic string.

## `for name in ["with.dot", "with space", "quote\"d"] {`

`run_with` takes any ids; `default_pool_name` makes the pool name the
id. Written bare, `a.b` names table `b` under table `a`, and a space
is a parse error.

## `let discovery = signed_in(`

A note is a CLI's output: the Codex adapter quotes up to 120
characters of an unrecognised `login status` answer, line breaks
included. Written raw, the second line reaches the parser as a
setting — here one that would flip `weekly`. The test's oracle is
the parse and the value, not the comment's spelling.

## `let cases: [(f64, &str, toml::Value); 4] = [`

`Display` for `f64` never uses an exponent: `1e300` is three hundred
digits, which the reader takes for an integer and refuses as too
large — a syntax error for the file, over a value `config::read`
accepts (finite, positive). The whole numbers keep the operator's
integer spelling, which the parent's test reads back by text.

## `let agents = [usable_agent(`

The parent's `stable_content` filters exactly one line by prefix; if
the timestamp ever appeared on a second line, or the first line
stopped starting with the prefix, every re-connect would rewrite.

## `let pool = || Pool::discovered("x", PoolKind::Credits, "x", vec![Source::Signals]);`

The type admits four combinations of outcome and pool; the parent
produces two. Both renderers now decide usability once, so an agent
has a pool in the file exactly when the summary says it has one,
and every agent is named in the summary exactly once — the two
unreachable combinations included, where the summary used to say
nothing for one of them.

## `assert!(summary.contains("\nunchanged: pools.toml\n"), "{summary}");`

"No change" and "could not tell" are two different lines.

## `for (shape, marked) in [(None, true), (Some(PoolKind::Credits), false)] {`

The file already says so under the pool; the summary is what the
operator sees when connect writes nothing, and it showed `[credits]`
as though detected.
