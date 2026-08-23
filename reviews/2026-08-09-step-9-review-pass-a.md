# Review: Step 9, Pass A — subprocess mechanics

- **Date:** 2026-08-09
- **Scope:** commit `2d40706`, files `src/agent/copilot.rs`, `src/agent/bin.rs`,
  `src/agent/mod.rs` (the Pass A half of the planned split; the decision path —
  catalog, config, review, engine, events, validate — is Pass B)
- **Level:** max — read, then **empirical verification on Windows** against both
  a batch-echo shim and a true npm-shape shim (`node "%~dp0dump.js" %*`),
  comparing the adapter's real command-construction path against `std`'s
- **Result:** 6 findings — 1 normal, 1 low, 4 nits — **all fixed** in the
  follow-up commit. One suspected command-injection was **tested and refuted**.

The through-line: **the adapter surface is sound; the thing under it is not.**
Copilot's permission model puts user-authored strings — gate commands — onto a
Windows command line for the first time, and the hand-rolled `cmd /C` quoting
that carries them there predates any input that interesting. It has one real
bug, and the reason to fix it is that it should not exist at all.

## What was verified empirically, not just read

The `.cmd`-shim path cannot be reasoned about reliably, so it was measured. A
scratch binary replicated `cmd_c_line` + `quote_for_cmd` + `raw_arg` verbatim
and spawned two shims: a naive `echo ARG:[%~1]` batch file, and a true npm-shape
shim forwarding `%*` to a node script.

That distinction mattered. Against the naive shim, `--allow-tool=shell(echo "hi"
& whoami)` **executed `whoami`** — which reads exactly like argument injection.
Against the npm-shape shim it does not reproduce: the argument arrives intact.
The injection was the *test harness's* flaw (a batch line that expands `%~1` and
then re-parses it), not upstroke's. Reported here because the negative result is
worth as much as the positive one — the CVE-2024-24576 / "BatBadBut" class is
exactly what this code path looks like it should be vulnerable to, and it isn't.

Cases exercised through the real shim: plain gate command; `&`; `|`; `%VAR%`;
embedded double quotes; `^`; single quotes; a path with spaces; an empty
argument; a trailing backslash before a closing quote.

## Findings

| # | Finding | Severity | Verdict |
|---|---------|----------|---------|
| 1 | **`%VAR%` in an argument is expanded by `cmd`, silently corrupting it.** `--allow-tool=shell(echo %PATH%)` arrives at the child as the machine's entire PATH. Gate commands come straight from `[[gates]] cmd` and now reach argv via `--allow-tool=shell(<gate>)`, so a Windows user writing `cargo test --features %FEATURES%` gets a permission grant that no longer matches the command it is meant to authorize — the agent then cannot run its own gate, and the attempt fails for a reason nothing explains. `std::process::Command` does **not** have this bug | normal | CONFIRMED (empirical, both shims) |
| 2 | `-s` is passed on every invocation but is **not** in `REQUIRED_FLAGS`, so `probe()` never checks it. Since none of `Caps`' other fields are read anywhere (only `session_resume` is), the required-flag refusal is essentially the whole protective value of probing — and it covers 4 of the 5 flag families this adapter actually passes. If GitHub drops `-s`, every attempt fails at runtime, which is precisely what §16 says probing exists to prevent. A bare `contains("-s")` is genuinely unsafe (it matches `--settings`, `--share`, `--stdio`), so this needs a boundary-aware check rather than adding it to the list | low | CONFIRMED |
| 3 | `json_output: has("--output-format")` reports a capability **this route never uses** — the adapter neither passes the flag nor parses JSON. Harmless today because nothing reads the field, but it is a claim that would be wrong the moment something did | nit | CONFIRMED |
| 4 | `profile.max_turns` is silently dropped: Claude passes `--max-turns`, Copilot has no documented equivalent, and the adapter neither honours nor mentions it. Latent — every current construction site sets `None` — but it is a spend control, and the failure mode of a spend control is that nobody notices | nit | CONFIRMED (latent) |
| 5 | Gate commands now reach an adapter by **two** independent channels — `TaskRun.gate_cmds` and the `gate_cmds` parameter of `materialize_permissions`. They agree at both call sites today, but nothing makes them; a future caller can set one and not the other, and the permission surface would then disagree with itself | nit | CONFIRMED |
| 6 | `permission_args` denies `write`/`shell` for reviewers but denies nothing for edit profiles, resting §20's "edit profiles get no network tools" entirely on the assumption that un-allowed tools are default-denied. That assumption is reasonable (`--allow-url` existing implies URL access is gated) but **untested** — there is no Copilot binary on this machine — and the code reads as though it were established | nit | CONFIRMED |

## The fix for #1, and why it is a deletion

`bin.rs` spends ~50 lines and two tests hand-building a `cmd /C` command line
via `raw_arg`, which opts out of everything `std::process::Command` does for
batch targets — including the escaping added in Rust 1.77.2 for
CVE-2024-24576. This crate is `edition = "2024"`, so its toolchain floor is
1.85: that fix is unconditionally present.

Measured side by side against the npm-shape shim, plain
`Command::new(shim).args(args)` handled **every** case correctly — `&`, `|`,
`%VAR%`, embedded quotes, `^`, spaces, and the empty argument — including the
one case the hand-rolled version gets wrong.

So finding #1's fix is to delete `Invocation::via_cmd_shell`, `cmd_c_line`,
`quote_for_cmd` and their tests, and let `std` resolve the shim. The module's
own doc comment argues that "two copies of it would be two chances to get it
wrong"; the honest number is zero. That also retires the `#[cfg(windows)]` /
`#[cfg(not(windows))]` split inside `Invocation::command` and the dead
non-Windows branch behind it.

Worth pairing with a Windows-only test that spawns a real `.cmd` shim, since
this is the one property in the crate that unit tests over pure string logic
cannot establish — the current tests assert what the *line* looks like, and the
line looking right is not the same as the child receiving the right argv.

**Applied.** `bin.rs` lost `via_cmd_shell`, `cmd_c_line`, `quote_for_cmd`, both
`cfg(windows)` branches and their string-logic tests; `Invocation::command` is
now `Command::new(path).args(args)`. Two tests replace them: one asserting the
constructed `Command`'s program and args survive verbatim (including `&`, `%`,
an embedded quote and an empty argument), and one Windows-only test that writes
a real `.cmd` shim and spawns it, because only spawning proves the half the old
code got wrong.

## Checked and clean

- **No argument injection** through the shim path (see above) — the case most
  worth worrying about, and it holds.
- `parse_output`'s rate-limit detection is failure-only, so a successful task
  *about* rate limits is not misread as an exhausted pool (step-6's rule,
  correctly mirrored from `claude.rs`, with its own test).
- The success path carries stdout into `detail`, which is the field a reviewer's
  verdict travels in — step-6 finding #1's regression is covered by a test that
  would catch its return.
- `SKIP_ALL_FLAGS` negative test genuinely covers both permission modes, and
  none of the skip-all spellings is a substring of anything the adapter emits.
- `looks_rate_limited`'s widening (`out of credits`, `premium request`,
  `monthly limit`) only ever broadens detection, and only on failures.
- Empty-argument and space-bearing-path handling survives the shim, so
  `--setting-sources ""` — the flag that stops external settings widening
  Claude's sandbox — still arrives as an empty argument rather than vanishing.

## Still open (deliberately)

- **`Caps` is almost entirely inert.** Five of its seven fields are written by
  both adapters and read by nobody; `session_resume` alone drives behaviour.
  That is fine while the capacity engine is unbuilt (step 10 is its first real
  consumer), but it means `probe()` currently buys the version string and the
  flag refusal, and little else. Not a step-9 defect; worth knowing before
  step 10 leans on it.
- **`-s` semantics are unverified.** Whether it truly reduces stdout to the
  agent's response — which `parse_output` assumes wholesale — cannot be checked
  without the binary. If it leaves any decoration, the reviewer's verdict parse
  absorbs it (last JSON object wins), so the failure is graceful; but the
  assumption is load-bearing and untested.
- **Whether `--no-ask-user` covers tool-permission prompts** remains unknown, as
  the module header already records. The attempt timeout is the backstop.
