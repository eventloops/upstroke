---
id: PR185-NAMING-DOC-001
severity: P3
disposition: deferred     # logged, not fixed, under the recovery workflow's P3 rule; one paragraph in the paired notes
category: docs-contract
pr: 185
reviewed_sha: 6f2ecadb2b9abade1d8ca87809d0e05184f2dea3
location: docs/internals/runner/host/naming.md:20
provenance: fix_regression   # the sentence was true before b9c73630 replaced the probe it names
first_bad: b9c73630f0228036ad9c0baeb344e36ec589ca69
guard: the next change to docs/internals/runner/host/naming.md, which the internals-notes gate holds to its module; the detailed `is_program` and `pathext_entries` sections of the same file are correct
---

## Failure sequence

The "Module" overview of `docs/internals/runner/host/naming.md` still says, in the present tense,
that the only filesystem contact is "`Path::is_file` and the execute bit" and that "the one genuine
`cfg` is `executable_bit`". Since `b9c73630` (`SWEEP-HOST-NAMING-001`) the source takes one
`std::fs::metadata` reading and derives both the regular-file and the Unix execute-bit answers
from it, and since the same commit `pathext_entries` has two `#[cfg]` conversion arms of its own.
A maintainer reading the overview is told the removed two-probe shape and a `cfg` count of one,
and the detailed sections further down the same file contradict it.

Raised by the parked-PR recovery's review 1 of PR #185 (gpt-5.6-sol, high), the only finding of
that pass, against the fix diff's claim that the paired notes match the repaired implementation.

## Why it is logged rather than fixed here

The recovery workflow fixes P0–P2 after a review and records each P3 as its own file and ledger
row; a notes edit after the review would be a substantive change spending another review round.
The behaviour, the tests and the detailed notes are unaffected; only this overview paragraph is
stale.

## What the change that takes this up should do

Rewrite the two sentences: the only filesystem contact is one read-only `fs::metadata` probe per
candidate, from which the file type and, on Unix, the execute bit are both read; the genuine
`cfg`s are `executable_bit` and the two `pathext_entries` conversion arms, everything else being
platform-as-value. No source change is needed and the section headings are untouched, so the
internals-notes gate is unaffected.
