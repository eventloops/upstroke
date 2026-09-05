---
id: PR162-ASTRA-HOST-CENSUS-LITERALS
severity: P2
disposition: deferred
category: correctness
pr: 162
reviewed_sha: a408608703fa34ea4e5de857bc20dd76626ac9b6
location: src/runner/host/tests.rs:5347
provenance: pre_existing
first_bad: 11d0a44837bd0e6155799153a0611b62f52018dd
guard: production_reaches_a_spawn_through_one_host_runner_per_run
---

# The host-runner census still counts constructors inside string literals

Owner-authorized deferred under STACK_STOP_RULE.md. This finding reports only the demonstrated false-positive mechanism under a harmless source edit, not an unproved concealed product defect.

## Failure sequence

Add a used diagnostic or fixture literal containing HostRunner::new( above the test boundary in one of the six scanned engine modules, without constructing another runner. production_region returns the original unblanked text. The local filter removes only lines beginning with //, so the string contributes an extra constructor count and the census rejects the harmless edit.

src/effects.rs:542 uses blanked text to find the test boundary but returns the original source region. The rewritten census filters comment-only lines at src/runner/host/tests.rs:5340 and then counts raw substrings at line 5347. The planted STRIP-CONTROL checks whole-line comment removal, not literal handling. This follows directly from the inspected code; no candidate mutation was run for this finding. Standards section 12 requires source censuses to blank literals.

## What the change that takes this up should do

Count actual constructor code after Rust-aware blanking, with a harmless-string control and an injected real-constructor control. The PR already names the stronger HostRunner census from #157 as an integration dependency; verify the actual integrated source before marking this finding fixed.
