---
id: PR161-ASTRA-BLANKER-CONTRACT
severity: P2
disposition: deferred
category: docs-contract
pr: 161
reviewed_sha: 976eae7b49e10b3560a96d2c28eb343c82cea016
location: docs/internals/effects.md:220
provenance: pre_existing
first_bad:
guard: Correct the two blanker contracts when the effects notes or source census helpers next change
---

Deferred by owner authorization on 2026-09-05 under DOCS_FAST_TRACK.md and
STACK_STOP_RULE.md. This record preserves the finding without claiming a fix.

## Failure sequence

A reader chooses `blank_comments` from its contract at `docs/internals/effects.md:218`. Lines 220 through 228 promise to replace comments and string literals with spaces while preserving byte offsets. The function at `src/effects.rs:54` instead deletes comment bytes and preserves literal bytes. For example, `/*why*/let x = "docker";` becomes `let x = "docker";`. A census following the first contract can count a needle inside a literal or use shifted offsets against the original input.

Lines 232 and 246 through 250 describe the actual behavior and contradict the opening contract. The length-preserving contract belongs to `blank_comments_and_strings`, defined at `src/effects.rs:176`. The contradiction already exists in the source comments at declared base `323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b`; this migration carries it into the notes.

The independent review inspected both implementations and the existing raw-string and comment fixtures at `src/effects/tests/source_oracles.rs:685`. The exact-candidate baseline log records their wrapper test passing. This finding concerns the contract, not a new runtime regression.

## What the change that takes this up should do

Give each helper its own contract. State that `blank_comments` removes comment bytes, preserves literals and line breaks, and does not preserve byte offsets. Put the contract for blanking both comments and literals without changing offsets under `blank_comments_and_strings`. Keep examples that distinguish the two outputs.
