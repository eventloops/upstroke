# Acceptance plan — a byte-size parser

The v0.1 definition of done (DESIGN.md §21) asks for five things in one run. Four
of them cannot be left to luck, so each task below is shaped to provoke exactly
one of them. The subject matter — parsing `"4KiB"` into `4096` — is deliberately
small, self-contained, and full of real edge cases, so a small model can
plausibly do the easy half and plausibly fumble the hard half.

Read the annotations, not the prose: the `kind` on each task is what selects its
chain from `tactus.toml`, and that is where the provocation lives.

## Document the size-parsing API

<!-- tactus: id=readme kind=docs depends= out=size-api -->

Add a `## Parsing sizes` section to `README.md` describing a public function
`parse_size(input: &str) -> Result<u64, SizeError>` that turns a human-written
size into a number of bytes. Document the accepted units — plain bytes, `KiB`,
`MiB`, `GiB` — and state that parsing is case-insensitive.

Do not write any Rust yet. This section is the contract the next task
implements.

**Acceptance criteria**

- `README.md` contains a `## Parsing sizes` section.
- The section names `parse_size`, its argument type, and its return type.
- The section lists all four accepted units.
- No files under `src/` are modified.

<!-- §21(a): `docs` routes to `small` only. Changing one markdown file passes
     `check`, `lint` and `test` untouched, so this should commit on the first
     attempt with no escalation. If it does not, the run has found something
     more interesting than the acceptance criteria. -->

## Implement the happy path

<!-- tactus: id=parse-basic kind=implement depends=readme needs=size-api paths=src/** -->

Create `src/size.rs` and wire it into `src/lib.rs`. Implement `parse_size` for
the cases the README documents: a bare integer is a count of bytes, and the
suffixes `KiB`, `MiB` and `GiB` multiply by 1024, 1024², and 1024³. Matching is
case-insensitive. Define a `SizeError` type for anything that does not parse.

Add unit tests covering one example of each unit.

**Acceptance criteria**

- `parse_size("512")` is `Ok(512)`.
- `parse_size("4KiB")`, `parse_size("4kib")` and `parse_size("4 KiB")` are all
  `Ok(4096)`.
- `parse_size("2MiB")` is `Ok(2097152)`.
- `SizeError` implements `std::error::Error` and `Display`.
- Tests exist for each unit and `cargo test` passes.

<!-- §21(b): `implement` gets `attempts_per = 2` on a two-rung chain, so a
     first-attempt failure retries the SAME rung with the failure fed back and
     the session resumed. The `lint` gate (`clippy -D warnings`) is the lever:
     new Rust from a small model trips a pedantic lint far more often than it
     fails to compile, and the clippy output is precisely the kind of feedback a
     resumed session fixes in one turn. -->

## Handle the edge cases

<!-- tactus: id=parse-edges kind=fix depends=parse-basic paths=src/** -->

Harden `parse_size` against everything the happy path ignores. Every case below
must be handled deliberately — a panic, a silent truncation, or a wrong answer
is a failure, and so is an error type that cannot tell the cases apart.

- The empty string, and a string of only whitespace.
- A unit with no number (`"KiB"`), and a number with an unrecognized unit
  (`"4PB"`, `"4 potatoes"`).
- A negative number (`"-1"`).
- A fractional value (`"1.5KiB"`) — reject it rather than rounding.
- A bare value greater than `u64::MAX` (`"18446744073709551616"`), and a value
  that overflows `u64` when multiplied by its unit (`"17179869184GiB"`). Both
  must return the overflow error, not wrap or truncate.
- Leading and trailing whitespace around an otherwise valid input.

**Acceptance criteria**

- Every case above has a test asserting the specific error variant, not merely
  that some error occurred.
- `SizeError` distinguishes at least: empty input, missing number, unknown unit,
  negative value, fractional value, and overflow.
- `parse_size` returns a `Result` without panicking for every `&str`; a test
  covers malformed non-ASCII input.
- A numeric value outside `u64`, whether before or during unit scaling, returns
  `SizeError::Overflow`; it never wraps or truncates.
- `cargo test` and `cargo clippy --all-targets -- -D warnings` both pass.

<!-- §21(c): `fix` gets `attempts_per = 1` across three rungs, so ANY first
     failure escalates rather than retrying. The two overflow paths and the
     non-ASCII case are the traps: an implementation can wrap, truncate, or
     confuse character and byte boundaries while still looking correct on the
     happy path. A reviewer can reject those observable failures without making
     a judgement call about which Rust idioms are allowed. -->

## Decide the rendering contract

<!-- tactus: id=format-policy kind=implement depends=parse-edges paths=src/** -->

Add the inverse function, `format_size(bytes: u64) -> String`.

There is a decision here that this repository cannot settle and that changes
what "correct" means for every downstream consumer, so it is not yours to make:
**when a value is not an exact multiple of its unit, what should `format_size`
do?** Rounding to the nearest unit (`1536` → `"2KiB"`) is lossy and means
`parse_size(format_size(n)) != n`. Emitting a fraction (`"1.5KiB"`) round-trips
visually but `parse_size` rejects fractions, so it does not round-trip in fact.
Falling back to bare bytes (`"1536"`) always round-trips but reads poorly for
large values.

Each choice is defensible, each breaks a different promise, and the repository
contains no precedent to infer from.

**Acceptance criteria**

- `format_size` exists and is documented.
- Exact multiples render with their largest exact unit: `4096` → `"4KiB"`.
- Inexact values follow the policy the operator chose, and a comment cites that
  choice as the reason.
- A round-trip test asserts whatever the chosen policy actually guarantees.

<!-- §21(d), first half: this is under-specified in a way that changes what
     correct means and cannot be resolved by reading the code — which is the
     exact condition the prompt teaches for `TACTUS-QUESTION:`. When the agent
     stops to ask, THIS task parks and nothing else about it is lost. -->

## Start a changelog

<!-- tactus: id=changelog kind=docs depends= -->

Create `CHANGELOG.md` with a `## Unreleased` section, and under it a single
bullet noting that size parsing was added.

Do not describe the individual tasks above or their history — one line about the
feature is the whole job.

**Acceptance criteria**

- `CHANGELOG.md` exists at the repository root.
- It contains an `## Unreleased` heading with at least one bullet under it.
- No files under `src/` are modified.

<!-- §21(d), second half: `depends=` is empty, so this task is ready from the
     start — but it is LAST in plan order, and the scheduler takes the lowest
     ready index. So it does not run until `format-policy` parks, and then it
     does. That is invariant 6 made visible: a question parks exactly the task
     that raised it, and the runnable frontier keeps draining. -->
