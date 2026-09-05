---
id: W1-CLASSIFIED-MODULES-IS-A-HAND-MAINTAINED-ROLL-CALL
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha:
location: src/effects.rs:968
provenance: pre_existing
first_bad:
guard: the slice that next changes CLASSIFIED_MODULES in src/effects.rs
---

## Failure sequence

`mechanism` (3)'s classification census takes its domain from `CLASSIFIED_MODULES` (`src/effects.rs:968`) — **56 entries at `3af9696` and not one directory prefix** — and `reachable_fns_are_classified` (`src/effects/tests/classification.rs:99`) asserts set equality against it before reading each entry from disk. Its doc comment at `:95` says *"The domain is **derived from the modules**, not listed"*, which is true of the **function-level** domain and not of the **module-level** one: a new production child file is graded only if somebody enrols it by hand. **Twenty-one** `.rs` files sit under a directory whose `.rs` parent is listed and are not themselves listed, and **the last two arrived after this row was written**: M7 split `src/config.rs` into `parse.rs` and `read.rs`, which carry **nine `pub(super)` functions between them** — all new names, none of them previously a `pub*` item of the parent. `declares_visibility` inside `externally_reachable_fns` counts `pub(super)` alongside `pub` and `pub(crate)`, so those nine are exactly the kind of item this census exists to force somebody to classify, and **because their files are not in the roll-call, nothing requires it and nothing fails**

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next changes `CLASSIFIED_MODULES` in `src/effects.rs`.

A consequence of the extraction pattern rather than of any one packet, and the fix is a decision about how the list should treat child directories. **The `TOPOLOGY_MODULES` half is already fixed** — `f1918e0` added the `src/workspace_manager/` prefix — and the two lists are matched differently on purpose (`src/effects.rs:903-911`), so the repair for the surviving half is not "add a prefix" but to derive the module domain or to state and execute the roll-call's semantics. The `m3-rundir`, `m5-host` and `m6-proc` splits each enrolled their children and each cited this finding by its working-record key while doing so; **M7 did not, and no gate noticed** — which is the same mechanism seen from the other side. Neither choice can be called wrong, because **the criterion is nowhere stated**: three splits enrolled, one did not, and the tree says only that the list is hand-maintained. That is the finding, and the asymmetry is what sustains it — enrolling a child costs classification rows for every reachable item in it, while not enrolling one costs nothing and is checked by nothing. Full derivation, with the command that reproduces the nineteen: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
