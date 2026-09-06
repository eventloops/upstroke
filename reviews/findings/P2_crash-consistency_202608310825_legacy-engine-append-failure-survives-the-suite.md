---
id: PR5-R2-LEGACY-ENGINE-APPEND-FAILURE
severity: P2
disposition: deferred
category: crash-consistency
pr: 5
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR7 implementer, or whichever lane plumbs an observer through engine::Harness
---

## Failure sequence

`PR5-EVENTS-054` and `PR5-EVENTS-055`. `Run::emit` swallowing an `EventLog::append` error into `self.warnings` and returning `Ok(())`, and deleting the partial-report construction from `drain_and_report`'s error branch, both survive the whole suite — because no test ever makes a legacy append fail **inside a live `Run`**. Every append-failure fixture operates on an `EventLog` directly

## What the change that takes this up should do

Owner, as the ledger records it: PR7 implementer, or whichever lane plumbs an observer through `engine::Harness`.

**Carried, and it is the behavioural half of `PR5-C-LEGACY-APPEND-ERROR-CENSUS` above.** The engine opens its own `EventLog` through `EventLog::open` and takes no observer, and its run directory is created with a generated run id, so neither an injected failure nor a prepared path (a `/dev/full` symlink, which is what made `PR5-EVENTS-044` measurable in the Event lane) can be aimed at it from outside. The live passage is `production_effect` — "the legacy engine's handling of a returned append error is unchanged: it reports and stops" — and the source census that stands in for it is already filed. Both become cheap the moment the coordinator takes an `EventHooks`

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
