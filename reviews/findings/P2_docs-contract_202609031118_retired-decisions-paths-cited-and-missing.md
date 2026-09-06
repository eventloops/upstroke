---
id: W2-RETIRED-DECISIONS-PATHS-CITED-AND-MISSING
severity: P2
disposition: deferred
category: docs-contract
pr: 
reviewed_sha:
location: src/engine/classify.rs
provenance: pre_existing
first_bad:
guard: project owner, undirected
---

## Failure sequence

PR #116 retired the `decisions/` directory and left every citation of a file in it naming a path that does not exist. Measured at `3af9696` over the tracked tree by two engines that agree, **with §43's own text excluded because it names dead paths as examples**: **24 distinct `decisions/*.md` paths cited, all 24 missing, 168 occurrences across 53 files** — 131 in `.md`, **26 in `.rs`**, 4 in `.toml`, 4 in `.yml`, 3 in `.sh`. Including §43 the same command returns 25 / 25 / 173, and the twenty-fifth path is one §43 itself introduced. The heaviest single path is `decisions/README.md` at 32. The `.rs` citations are spread over twenty production and test files including `src/engine/classify.rs`, `src/engine/topology/run.rs`, `src/topology/effects/sites.rs` and `src/topology/fold/check_attempt.rs`; `effects/allowlist.toml` and `upstroke.toml` carry two each. **No gate catches it** — `test-docs-consistency.sh` passes at `ac16fff`, `ae2a58f` and `3af9696`

## What the change that takes this up should do

Owner, as the ledger records it: project owner, undirected.

**The rules themselves survive; it is the citations that died, and that distinction was verified rather than assumed.** The clean-base merge rule this programme relies on lived in a `decisions/` file that is gone, but is restated in `DESIGN.md` and `.github/pull_request_template.md`, so it is live — checked before being relied on, because a rule cited to a deleted file is exactly the authority that evaporates on inspection. This is the **deletion** form of a class this programme met three times in one day at smaller scale: a change invalidates prose in files it does not touch, and a deletion invalidates every reference to what it deleted **including references in code comments nobody thinks of as documentation**. Not any packet's to repair — a packet fixes the citations in its own body and no more. The durable fix is a gate that resolves cited repository paths; without one the class recurs on the next directory retirement, which is how it arrived. Full derivation with its command: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
