# `src/export.rs`

Extended notes for [`src/export.rs`](../../src/export.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Local, read-only projection of a run's recorded routing decisions.

## `#![allow(clippy::disallowed_methods)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub struct Loaded {`

A successful load plus recoverable residue the caller must surface on a
separate channel from the machine-readable export.

## `struct ExportUsage {`

Export-schema-1 usage. This intentionally is not the engine's `Usage`:
adding an internal field must not silently add a public JSON key.

## `pub fn load(repo_root: &Path, wanted: &str) -> Result<Loaded, UpstrokeError> {`

Load and validate one stable run snapshot. No config, source plan, adapter,
or report is consulted.

## `pub fn load(repo_root: &Path, wanted: &str) -> Result<Loade…` › `let projected = (|| {`

Always perform the closing stability check before returning a projection
error. Otherwise a racing resume could make a transient moving view look
like permanently invalid input.

## `fn failure_projection(kind: FailureKind) -> (&'static str, &'static str) {`

Deliberately exhaustive and wildcard-free: a new FailureKind is a compile error here.

## `fn is_supported_rfc3339(value: &str) -> bool {`

Validate the RFC 3339 subset Upstroke can record.

Event timestamps come from `SystemTime` as ordinary Unix seconds, so the
writer can never emit `:60`. Rejecting leap-second notation avoids accepting
it on arbitrary dates (which requires an external announcement table) while
retaining every timestamp an authentic Upstroke writer can produce.

## `fn exported_timestamps_use_the_supported_rfc3339_profile()` › `"2024-02-29T23:59:60.123Z",`

`:60` is not accepted blindly on a leap-year date, and even a
historical leap second is outside the writer's supported subset.

## `fn both_formats_preserve_start_order_reviews_and_frozen_fea…` › `fs::write(`

These current inputs are traps: the exporter must never consult them.
