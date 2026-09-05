---
id: SWEEP-CONNECT-RENDER-010
severity: P3
disposition: deferred
category: correctness
pr: 168
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/connect.rs:233
provenance: pre_existing
first_bad:
guard: the sweep of `src/connect.rs` (`standards/SWEEP.md` row 62)
---

## Failure sequence

`settings_of` decides whether the existing file *may* be replaced by comparing lines with comments
stripped and whitespace normalised — text, not values. Every TOML spelling that differs from the
renderer's is therefore a conflict:

1. The operator's file says `reserve = 0.2`, or `profile = 'work'`, or
   `sources = ["signals","self"]`. Each is the value `connect` would write.
2. `connect` renders `reserve = 0.20`, `profile = "work"`, `sources = ["signals", "self"]`.
3. `settings_of` differ, so `connect` refuses: "already exists and differs from what connect
   would write", and recommends `--force`, for a file that configures exactly what it would write.

The refusal exists so that a hand edit is never silently overwritten; a refusal that fires on
spelling trains the operator to reach for the flag that discards hand edits, which is the failure
the parent's own doc names ("a refusal an operator is trained to bypass protects nothing").
PR #168 makes the renderer's spelling canonical and deterministic (basic strings, integers for
whole allowances), so the refusal now fires at most once per hand-written file and never between
two files `connect` wrote; it does not remove the class.

## Why this is recorded rather than fixed

The comparison is the parent's, and the fix changes what "differs" means for the command.

## What the change that takes this up should do

Compare values, not text: parse both texts as `toml::Table` (the parent already parses the existing
file for `operator_keys`) and compare the tables. Comment-only differences fall out of the parse;
an existing file that does not parse is a conflict, which is what it is. `strip_comment` and its
string tracking can then go — since PR #168's pass 1 it understands basic-string escapes and
literal strings, because the renderer now writes escaped keys and a `\"` read as a closing quote
made two headers differing after a `#` compare equal, the false-equality half of this class; that
half is closed, the spelling-sensitivity half above is not.
