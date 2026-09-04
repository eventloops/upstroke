## 12. Tests

Tests are executable evidence of a contract. New behaviour and bug fixes come with tests at the
lowest layer that can observe the real failure, plus a higher-level test when composition is the
risk. A regression test fails for the reported defect before the fix and passes after it; prefer a
minimal reproducer naming the violated contract over a broad snapshot that happens to change.

Cover, as applicable: the success path; invalid, missing, malformed and boundary input; each error
category that changes caller behaviour; interrupted writes, partial external output, failed cleanup,
retry and resume; platform semantics; concurrent winner/loser and cancellation paths; persisted
formats and public API compatibility. Property and state-machine tests are worth considering for
parsers, replay, routing, serialization and concurrent protocols.

Tests are deterministic and hermetic:

- unique temporary directories with RAII cleanup;
- injected or controlled clocks, randomness, environment, capacity observations and process
  responses;
- no dependence on network, ambient credentials, user configuration, PATH contents, execution order
  or an installed vendor CLI unless the test is explicitly an integration test;
- no sleeping as synchronization: signal the state the test needs to observe;
- no silent success when a prerequisite is absent: provide it, classify the test out of the default
  suite, or fail with a diagnostic;
- `#[ignore = "reason"]`, never a bare `#[ignore]`;
- assertions on externally meaningful state, not on the implementation copied into the test as its
  own oracle.

**Readiness.** A readiness signal is published after the state it announces is complete and
observable, never alongside it. A file's existence is a signal only if the file is published
atomically: an empty marker created after the state, or a file staged elsewhere and renamed into
place — never a path created in place and written afterwards. A partial record is never readable as
a whole one: a newline-delimited record is complete only when its terminator arrives, and an
unterminated tail fails rather than yielding a short value. Keep the payload inside what the framing
carries; a path is not safely a line. Every wait is bounded, and the bound bounds a wedged producer
rather than timing a healthy one: a deadline short enough to expire on a loaded runner has become
the failure it exists to prevent.

**Flakes.** An intermittent failure is a fact about the repository, handled by measurement rather
than adjective:

- measure a rate — failures over observed runs, with platform, head and the failing assertion —
  before calling anything a flake;
- establish provenance: a byte-identical tree producing both outcomes proves nondeterminism, not
  its origin. Reproduce on a head that predates the change, or argue causally from the diff; with
  neither, a new intermittent failure is a candidate regression, and "it passed on re-run" never
  merges a change;
- fingerprint by platform and by the failing assertion or error code, not by test name;
- name an owner and the consequence: a red matching the fingerprint is this flake until shown
  otherwise, and a red that does not match is a regression;
- an identified mechanism on a supported platform is a defect at any rate, fixed by repairing it;
- keep per-run output so each occurrence's outcome and diagnostic survive together.

Carried flakes live in the finding ledger (`reviews/findings/`, one file per finding; older
ones in `reviews/FINDINGS.md`) with their rate, owner and consequence.

**Instruments and censuses.** Source-based enforcement is code and is tested like code. A census of
Rust structure blanks comments and string, character, byte-string and raw-string literals before
counting, preserves positions and length, and proves the blanker on a fixture with removable
regions. Every census asserts the size and boundaries of the domain it claims and carries a positive
control that injects one violation and sees the expected failure; a control inside a truncated
domain proves nothing about the whole. Test-only items follow every production item in a file,
because a mid-file `#[cfg(test)]` truncates any instrument that treats the first test item as the
end of production. After inserting an item, check that neighbouring doc comments and attributes
still attach where intended. Fixtures derive their field lists from the production type and vary
each field independently. Name each test as the sentence it proves.

Test code may trade abstraction for clarity but does not weaken production invariants by reaching
through private state when the public behaviour can be exercised.

Enforced by: `cargo test --all-targets --all-features` on three platforms; each instrument's own
positive control; review for sufficiency, readiness and flake records.
