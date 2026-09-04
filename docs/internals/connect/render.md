# `src/connect/render.rs`

Extended notes for [`src/connect/render.rs`](../../../src/connect/render.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

What `connect` renders: the pools file it writes, and the summary the CLI
prints once it has.

Both are pure text over decisions the parent has already made. Nothing here
probes a CLI, derives a pool, reads the file that is already on disk, or
writes anything — `run_with` does all four and hands the results down. That
is the whole cut: the parent keeps discovery, pool derivation, the operator
keys it carries across a `--force`, the two comparisons that decide whether
to rewrite, and the write itself; this module turns what they produced into
strings.

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
`std::io::Write::write_fmt` it denies.

## `pub(super) fn pools_file(agents: &[AgentReport]) -> String {`

Render the pools file: §17's shape, plus a header saying who wrote it, when,
and where the model roster came from.

## `fn pool_section(pool: &Pool) -> String` › `if let Some(profile) = &pool.profile {`

The operator's own keys, written back out. `connect` never invents any of
these — it cannot discover which account, how large an allowance is, or
where a local model lives — but once one is in the file it has to survive
being rewritten, or `--force` would delete exactly what the refusal it
overrides existed to protect.

## `pub(super) fn report(report: &ConnectReport) -> String {`

What the CLI prints.
