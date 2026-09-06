---
id: PR7-APPEND-REPORT-READABLE-UNDISCHARGED
severity: P3
disposition: deferred
category: correctness
pr: 7
reviewed_sha: 
location: src/engine/topology/emit.rs:103
provenance: pre_existing
first_bad: 
guard: the round that can afford to move the six `emit` tests that assert the report's operator text
---

## Failure sequence

The commit that moved obligation (3) to the caller claimed the append-error report is
"unreachable while invocations still run, as a compile error". `EmitFailure` and `EmitError` both
implemented `Display` by delegating to the token's, so `failure.to_string()` — the thing every `?`
path does on its way to an operator — rendered the entire report without discharging anything.
`EmitFailure::Undischarged` and `EmitError::AppendFailed` were repaired to render only what a
caller may know before discharging. `UncancelledAppend` itself still implements `Display`
(`src/engine/topology/emit.rs:103`, verified at `735ef21`), so a caller that destructures the error
deliberately can still read the prose without discharging the report.

## What the change that takes this up should do

Remove `Display` from `UncancelledAppend`, which is the complete fix. It ripples into six
`emit` tests that assert the report's operator text directly, and doing that hastily is the "a fix
that introduced a new defect" class this project has paid for five times — which is why the round
that raised it stopped at the narrower honest claim: the count and the discharge cannot be skipped;
the prose can.

Recorded in `reviews/FINDINGS.md` §3 on 2026-08-25 as *partially repaired*, with this residue “named rather than closed”. Severity is this migration's judgement: the count and the discharge still bind, so what leaks is prose, not authority.
