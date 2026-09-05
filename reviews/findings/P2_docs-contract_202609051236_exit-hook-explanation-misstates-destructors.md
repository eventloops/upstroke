---
id: PR156-ASTRA-EXIT-DESTRUCTORS
severity: P2
disposition: deferred
category: docs-contract
pr: 156
reviewed_sha: 21e32e65e9773baf2f52e21b15a2195eff22baf5
location: docs/internals/agent/proc/hooks.md:84
provenance: pre_existing
first_bad: 1a9cb205b4466a8a20d6e7918d9b2afa4aa1c3ed
guard: "Compare the kill-hook rationale against Rust's process::exit, abort, and panic unwinding contracts; distinguish stack destructors from registered process-exit handlers."
---

## Failure sequence

A reader choosing how to model abrupt coordinator death reads that both panic
and process exit run Rust destructors, then reasons from that claim about closing
the job handle. `std::process::exit` skips Rust stack destructors, so the
explanation misstates the distinction. The implementation still uses `abort`;
the review did not establish a containment runtime failure.

## What the change that takes this up should do

Correct the explanation to distinguish unwinding, process-exit handlers, and
abort. Check the wording against the
[Rust exit contract](https://doc.rust-lang.org/std/process/fn.exit.html).
The independent GPT-6 Astra/max review reported this as a pre-existing P2
documentation finding. It remains deferred under the owner's 2026-09-05
documentation direction, with its permanent disposition in PR #156's ledger.
