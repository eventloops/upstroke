---
id: PR5-C-LEGACY-APPEND-ERROR-CENSUS
severity: P3
disposition: deferred
category: correctness
pr: 5
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR7 implementer, or whichever lane plumbs an observer through engine::Harness
---

## Failure sequence

`production_effect` promises "the legacy engine's handling of a returned append error is unchanged — it reports and stops". `events::log::tests::the_legacy_engine_reports_and_stops_on_a_returned_append_error` proves it as a **source census** (the error branch returns, emits nothing, and the engine has exactly one append call site), not as a behavioural test

## What the change that takes this up should do

Owner, as the ledger records it: PR7 implementer, or whichever lane plumbs an observer through `engine::Harness`.

**Boundary stated rather than hidden**, on the test's own doc comment. The legacy engine opens its own `EventLog` through `EventLog::open` and takes no observer, so no test can make one of its appends fail without threading hooks through `engine::Harness` — a file PR5 lane C does not own and a change with reach far beyond this claim. What *is* checkable locally is the property the promise rests on: the error branch returns and appends nothing, so the handle poisoning this slice adds is unobservable to it. A behavioural version becomes cheap the moment the coordinator takes an `EventHooks`, which is what the append-error protocol needs anyway

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
