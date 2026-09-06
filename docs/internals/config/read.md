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
for `--pools`. Once bytes exist, `toml::from_str` decides the rest and this
function adds nothing to what it says: whatever `RawRepoConfig` accepts or
refuses at the wire, this function's own contract is only the two file-absence
branches above. What the wire refuses includes a misspelled top-level section
name — `[budgts]`, `[interation]`, `[runer]` — which `RawRepoConfig` used to
deserialize into nothing with no warning (`SWEEP-CONFIG-PARSE-007`, guarded to
whichever of this file or `src/config.rs` swept first): the struct now denies
unknown fields, the refusal names the typo and the accepted seven sections, and
the regression through `load_captured`, on a capture built in memory, lives in
the parent's suite beside the struct.

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

## `fn converts_exactly(units: i64) -> bool {`

An `as` narrowing needs a nearby invariant that proves the range (§5), and
this one is *checked* rather than assumed. TOML hands `monthly_allowance` the
whole `i64` range, and an `f64` carries 53 significant bits, so not every
integer survives the cast: written `9223372036854775807`, an operator would
have got `9223372036854775808` back — a ceiling nobody set. So the integer
branch refuses, by name, any integer the cast would change, and what survives
is the number that was written.

What decides it is the span between the highest and the lowest set bit of the
magnitude: an integer converts exactly when that span fits in the mantissa,
whatever its size. `2^53 + 1` needs 54 bits and is refused; `2^53 + 2` and
`10^16` (the older writer's integer spelling `design/17` names, which is
`2^16 × 152587890625`) fit and are accepted unchanged. An earlier version of
this check was a blanket ceiling at `2^53`, which refused those two; and the
version before that argued the range instead of enforcing it, reasoning that a
hand-typed spend ceiling is never that large — a claim about operators, not
about accepted input. Both were wrong on the input the parser actually takes.
The predicate is integer arithmetic only, so the proof of the cast does not
itself lean on a cast; the sign is refused just below, where `i64::MIN`, exact
though it is, fails the same check every negative value fails.

The finiteness and sign check below applies uniformly to both the integer
and float branches; for the integer branch it can only ever pass, since a
cast from a finite `i64` is never `NaN` or infinite, and it is written once
for both rather than duplicated per branch. The float branch takes no
exactness check: a `toml` float is already an `f64`, so there is no conversion
to lose anything, and `is_finite` is the whole of what it needs.
[`connect::render::toml_number`] writes an allowance back out as an integer
only below `2^53` and as a float otherwise; that is the writer's choice of
spelling and is unaffected by what the reader accepts.

## `fn absent(tag: &str, required: bool) -> FileSnapshot {`

The absent-file tests need a snapshot whose capture found no file, and they
build one directly rather than pointing `snapshot_file` at a path that is
supposed not to exist. A path is only as absent as the filesystem says it is
at the moment of the read: a fixed name can be satisfied by a leftover, and a
per-process name can be satisfied by a leftover from an earlier process that
was issued the same id. `FileSnapshot`'s fields are private to `src/config.rs`
and visible here because this module is its child, and `Ok(None)` is exactly
what `snapshot_file` records for a file that is not there, so the fixture is
the captured state itself with no filesystem in the oracle. Reaching for `fs`
to manage a real directory would put a governed primitive in a module that
denies all three and takes no `effects/allowlist.toml` row.

## `fn an_integer_allowance_that_converts_exactly_is_accepted_unchanged() {`

The expected values are written out as float literals rather than spelled
`written as f64`, which is the very cast under test: the expected value has to
come from outside the code being proved. The refusal test's `i64::MAX` case is
the value that motivated the check — it casts to `9223372036854775808.0`, one
more than it is, and accepting it would silently store an allowance the
operator did not write.
