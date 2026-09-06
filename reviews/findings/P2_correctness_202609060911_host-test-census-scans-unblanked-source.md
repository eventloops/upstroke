---
id: PR157-ASTRA-SITE-SAFETY-R2-001
severity: P2
disposition: deferred
category: correctness
pr: 197
reviewed_sha: 943c4714dfcb33fb071a5b030d026dfae241850e
location: src/runner/host/tests.rs:5803
provenance: introduced_by_feature
first_bad: 82e7367a848b654162e930a4ea596431e9b3c4e0
guard: the change that next opens `every_site_obligation_is_complete_and_agrees_with_its_notes_copy`, or the PR #197 branch `codex/findings-2b802b011b29` if it is resumed
---

`every_site_obligation_is_complete_and_agrees_with_its_notes_copy` finds the operations it
censuses by splitting each raw source line on non-identifier characters and looking for the
keyword. The only thing it excludes first is a whole line whose trimmed text starts with `//`. A
string literal, a block comment, or a trailing comment after code that contains the word is
therefore read as an operation, and the census demands an adjacent obligation for it.

`standards/12_standards_tests.md` requires a census of Rust structure to work from a
position-preserving view with comments and literals blanked, and to prove that blanker with a
fixture. The crate already has that machinery in `src/effects.rs` — `blank_comments` and
`blank_comments_and_strings` — and this census uses neither.

## Failure sequence

Append `const _: &str = "unsafe";` to `src/runner/host/tests.rs` at
`943c4714dfcb33fb071a5b030d026dfae241850e` — a valid, harmless item that introduces no unsafe
operation — and run the census:

```
test runner::host::tests::every_site_obligation_is_complete_and_agrees_with_its_notes_copy ... FAILED
panicked at src/runner/host/tests.rs:5820:13:
src/runner/host/tests.rs:5888: the operation carries no adjacent obligation
```

Reproduced by the implementor on 2026-09-06 at that exact head, and independently by the reviewer's
in-memory model of the scanner, which reports the same false failure for `/* unsafe */`. The
converse also holds and is the reason this is P2 rather than cosmetic: the scanner's notion of "an
operation" is not the language's, so the census's negative result — "every operation carries an
obligation" — is not established for a module whose text it misreads. Its positive results are
unaffected: the four mutation arms in the pull request body all still detect, because they act on
real operations and real comment blocks.

## What the change that takes this up should do

Locate the keyword in a position-preserving lexical view with comments and Rust literals blanked —
`crate::effects::blank_comments_and_strings` is the existing one, and reusing it puts this census
on the blanker the effects census already proves — then read the adjacent obligation out of the
original source at that position, so the comment text is still compared verbatim. Add executable
controls to the same test: a literal and a block comment containing the keyword must be ignored,
and a real operation with no adjacent obligation must still be rejected. Both controls belong in
the census itself, not in a separate fixture, so the two cannot drift.
