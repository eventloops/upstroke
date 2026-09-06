# `src/config/read.rs`

Extended notes for [`src/config/read.rs`](../../../src/config/read.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Turning captured bytes into raw and typed shapes.

Every function here that reads a file's contents takes a [`FileSnapshot`]
and never a path. That is the whole of what makes a pre-lock validation
worth its ordering: the bytes that were validated and the bytes that are
parsed are the same object, so there is no second read for the file to
change between. A reader here that reached for the path instead would
reintroduce exactly the window the capture exists to close.

`parse_pool` is the exception to the shape and not to the rule: it takes a
`&Path` and never reads through it. The path is what it names in the error
it returns and in one diagnostic string, nothing more. A later change that
wants a file's contents here reaches for the snapshot, never for that path.

`[pools]` keeps the temperament the rest of the configuration surface has:
anything that would silently change what the estimator does is an error,
anything that only degrades what it can say is a warning that names the key.
Pool order is file order, and file order is preference — the span each entry
carries is what preserves it through a map.

## `#![deny(`

**This child states its own lint level and inherits nothing.** A Rust lint
level is scoped by the module tree rather than by the file, so an out-of-line
child of `src/config.rs` inherits that file's inner
`#![allow(clippy::disallowed_methods)]` unless it says otherwise --
`PR6-LANEF-004`, and the mistake two W1 pull requests then made
independently (#100 and #102). Nothing here reaches a governed primitive, so
all three governed lints are DENIED and this module takes no
`effects/allowlist.toml` row: a row records an allowance, and this module
takes none.

The three are not equally load-bearing, and which is which is worth stating.
`src/config.rs` allows `clippy::disallowed_methods` and that lint alone, so
the first line below is the one that restores a level the parent removed
outright: without it, a denied method here raises no diagnostic at all. The
other two raise this module from clippy's default `warn` to `deny`, so a
denied type or macro fails here on its own rather than only under CI's
`-D warnings`. All three are written out because what decides the first one
is a property of the parent's attribute rather than of this file, and a
parent's attribute can widen without this file changing.

## `pub(super) fn read_repo_config(`

An absent, explicit `--config` path is an error ("file not found"); an absent,
discovered one is the normal fresh case and returns [`RawRepoConfig::default`]
silently — the same explicit/discovered asymmetry [`read_pools`] states below
for `--pools`. Once bytes exist, `toml::from_str` decides the rest, and
`RawRepoConfig` denies unknown fields (`SWEEP-CONFIG-PARSE-007`): a misspelled
top-level section name — `[budgts]`, `[interation]`, `[runer]` — used to
deserialize into nothing and vanish with no warning, deleting a whole section
the same way a typo in one of its keys is already refused two levels down
(§17's "what the repo-level file refuses"). It is now the same refusal as
those keys: an error naming the section, not a warning and not silence. The
seven accepted names are exactly `RawRepoConfig`'s fields and exactly what
`design/17` documents for this file, so the attribute costs no
forward-compatible key.

## `pub(super) fn read_pools(`

Read `~/.upstroke/pools.toml` into typed pools (§17).

Temperament matches the rest of this file: anything that would silently
change what the estimator does is an error, and anything that only degrades
what it can say is a warning.

- unknown `kind` → **error**; it decides which estimator rule runs.
- unknown `sources` entry → **error**; dropping `signals` by typo would
  discard §13's ground truth while the file still claims to have it.
- `safety_margin` / `reserve` outside `0.0..=1.0` → **error**; both are
  fractions, and a "150% margin" has no reading that is merely degraded.
- `agent` with no adapter in this build → **warn**, pool kept and marked
  unusable. §17's own example ships `[pools.local] agent = "aider"`, so
  erroring would brick anyone who copied the documented file.
- unknown keys → **warn**, by name.

An **explicit** `--pools` path that does not exist is an error, the way an
explicit `--config` is in [`read_repo_config`]: a path someone typed and
that is not there is a typo, and answering it with "no pools connected —
run `upstroke connect`" sends them to regenerate a file that was never the
problem. A *discovered* one that is absent is the normal fresh case and
stays silent.

## `let mut entries: Vec<(String, toml::Spanned<toml::Value>)> =`

Back into the order they were written in — see [`RawPools`].

## `if name.trim().is_empty() {`

A pool's name is its identity everywhere downstream — it is what an
attempt is attributed to and what the ledger prints. A blank one is
indistinguishable from "no pool" by the time it reaches the engine
(`pool_option` maps `""` to `None`), so the attribution would vanish
while the pool still matched for routing. Same reasoning as the
non-empty `[[gates]]` `name`.

## `Some(toml::Value::Integer(units)) => Allowance::Units(units as f64),`

An `as` narrowing needs a nearby invariant that proves the range (§5); here
the invariant is what `monthly_allowance` means rather than a bound checked
in the file. It is a spend ceiling an operator typed by hand, always many
orders of magnitude below `f64`'s 2^53 exact-integer boundary — the same
boundary [`connect::render::toml_number`] rounds at when writing this value
back out — so the cast cannot silently change what the operator wrote. The
finiteness and sign check just below applies uniformly to both the integer
and float branches; for the integer branch it can only ever pass, since a
cast from a finite `i64` is never `NaN` or infinite, and it is written once
for both rather than duplicated per branch.
