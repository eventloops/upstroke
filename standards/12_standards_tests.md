## 12. Tests

Tests are executable evidence of a contract. New behaviour and bug fixes MUST include tests at the
lowest layer that can observe the real failure, plus a higher-level test when composition is the
risk.

A sufficient test set covers, as applicable:

- the ordinary success path;
- invalid, missing, malformed, and boundary input;
- each error category that changes caller behaviour;
- interrupted writes, partial external output, failed cleanup, retry, and resume;
- platform-specific semantics;
- concurrent winner/loser and cancellation paths;
- compatibility with persisted formats and public API contracts.

Tests MUST be deterministic and hermetic by default:

- use unique temporary directories with RAII cleanup;
- inject or control clocks, randomness, environment, capacity observations, and process responses;
- do not depend on network access, ambient credentials, user configuration, PATH contents, test
  execution order, or an installed vendor CLI unless the test is explicitly an integration test;
- do not use sleeping as synchronization; signal the state the test needs to observe, under the
  readiness rules below;
- do not silently return success when a prerequisite is absent. Either provide the dependency,
  classify the test outside the default suite, or fail with a useful diagnostic;
- state the reason in the attribute when a test is disabled (`#[ignore = "…"]`); a bare
  `#[ignore]` is a parking space, not a classification;
- assert externally meaningful state and side effects, not an implementation copied into the
  test as its own oracle.

### Readiness protocols

A readiness signal is a claim about state, and a waiter is entitled to assert everything the claim
names. The producer's ordering is what makes that claim true, so these rules bind the helper as
much as the test:

- A readiness signal MUST be published only after the state it announces is complete and
  observable by the waiter. Publish it last, not alongside the work it describes.
- **A file's existence is a readiness signal only if the file is published atomically.** Creation
  and content are otherwise separate events, so a waiter polling for a path that is created and
  then written can open it and read nothing. Two forms are sound: an empty marker created after
  the state it announces, where there is nothing to read; and a file staged elsewhere and moved
  into place by atomic rename under §8, where the name and its contents become visible together,
  so a waiter may await the path and then read it. What is unsound is a path created in place and
  written afterwards, because its existence is observable before the state it stands for.
- A partial record MUST NOT be readable as a whole one. A record delimited by a terminator — a
  newline on a pipe — is complete only once the terminator arrives; an unterminated final record
  is a truncated write and MUST fail rather than yield a short value.
- Keep the payload inside what the framing can carry. A path is not safely a line: an ancestor may
  contain the delimiter, or bytes that are not text at all. Send an identifier the receiver can
  rejoin to a root it already knows.
- The wait MUST be bounded, and the bound MUST bound a producer that has wedged rather than time
  one that is healthy. A deadline short enough to expire on a loaded runner has become the signal
  itself, which is the failure this rule exists to prevent. The fast path is a producer that fails
  and closes its channel; the bound is for the one that stays alive and silent.

### Flakes

A test that fails intermittently is a fact about the repository, and it is handled by measurement
rather than by adjective. "Occasionally" and "flaky under load" are not records.

When a test is observed failing without a change that explains it:

- **Measure a rate before naming it a flake.** Record a numerator over a denominator of observed
  runs — "one failure in 31 full-suite runs" — with the platform, the head, and the assertion
  that failed. One failure and no denominator is an observation, not a rate.
- **Establish provenance, and do not mistake nondeterminism for it.** Re-running the failed job at
  the same commit is cheap and worth doing, but one byte-identical tree producing both outcomes
  shows only that the failure is nondeterministic. It does not show where the nondeterminism came
  from: a change that introduces a race produces exactly that signature. Provenance needs one of
  two things — the same failure reproduced on a base or prior head that predates the change, or a
  causal argument, made from what the diff actually touches, that it cannot reach the behaviour.
  With neither, a newly observed intermittent failure is a candidate regression and is triaged as
  one. "It passed on re-run" MUST NOT be the reason a change merges.
- **Fingerprint an occurrence by platform and by the assertion or error it produced**, not by test
  name alone. One name can cover several causes, and one cause can surface under several names, so
  match on the failing assertion, together with the error code where the failure carries one — a
  panicked assertion has a message and a location and no code, and is fingerprinted by those.
- **Name an owner and state the consequence**, so a later red is triaged instead of re-diagnosed:
  which fingerprint, and that a failure matching it is this flake until proven otherwise. A red
  that does not match the recorded fingerprint is a regression until someone shows otherwise.
- **The classification is provisional, and a cause retires it.** A failure with an identified
  mechanism on a supported platform is a defect at whatever rate it occurs, triaged by repairing
  that mechanism rather than by re-running. A rate does not settle the category; it is what makes
  the category arguable.
- **Preserve the evidence a rate is made of.** A harness that overwrites one output path each run
  loses the auditable fingerprint and diagnostic provenance of every occurrence but the last,
  because the next run destroys the failure the previous one caught. A rate can still be measured
  if outcomes and diagnostics are retained separately — a numerator and denominator kept durably
  outside that path carry the arithmetic — but occurrences that can no longer be re-examined are
  not evidence that any of them matched the recorded fingerprint. Write per-run output: that is
  the preferred form, because it keeps each occurrence's outcome and diagnostic together. Quote
  the diagnostic — the message, and the error code when the failure has one — rather than the
  bare fact of a failure.

A red that recurs with no rate, no owner, and no stated consequence trains reviewers to discount
CI, which costs more than the test does.

### Instruments and censuses

Source-based enforcement is code and needs tests of its own:

- A census of Rust structure MUST blank comments and string, character, byte-string, and raw-string
  literals before counting. The blanker MUST preserve positions and output length. Every scan MUST
  assert a non-zero blanked-region count unless its declared domain intentionally contains none;
  the blanker's tests MUST prove this with a fixture containing removable regions. Prefer a parser
  or structural match to a substring search.
- Every census MUST assert the size and boundaries of the domain it claims and carry a positive
  control that injects one violation and observes the expected failure. A positive control inside
  a truncated domain does not prove that the whole named domain was scanned.
- Test-only items MUST follow every production item in a source file. A mid-file `#[cfg(test)]`
  boundary can silently truncate any instrument that defines production as the prefix before the
  first test item.
- After inserting an item, verify that the neighbouring doc comments and attributes still attach
  to their intended items. In particular, check `#[cfg]`, `#[test]`, lint attributes, derives, and
  rustdoc on both sides of the insertion.
- Derive a fixture's field list from the production type, not intuition. Vary every independently
  meaningful field independently and assert hostility with distinct-value counts or a complete
  written-out table; correlated values do not prove the field named by the test.
- Name each test as the sentence it proves, so a failure reads as the broken claim rather than as
  an implementation detail.

A periodic mutation pass (for example `cargo mutants`) over gates and instruments is the
systematic form of the injected-control discipline and SHOULD accompany changes to enforcement
code. It has no named repository gate at adoption.

A regression test MUST fail for the reported defect before the fix and pass after it. Prefer a
minimal reproducer that names the violated contract over a broad snapshot that happens to change.
Property tests and state-machine tests SHOULD be considered for parsers, replay, routing,
serialization, and concurrent protocols where example cases leave a large state space uncovered.
Parsers over untrusted input — the plan format, agent stdout — SHOULD additionally get fuzz
coverage once a fuzzing target exists in the repository; until then that is a triggered review
consideration.

Test code may trade some abstraction for clarity, but it must not weaken production invariants by
reaching through private state when the public behaviour can be exercised directly.
