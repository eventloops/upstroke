---
id: SWEEP-AMBIENT-010
severity: P3
disposition: deferred
category: correctness
pr: 147
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/agent/proc.rs:4247
provenance: pre_existing
first_bad:
guard: the sweep of `src/agent/proc.rs` (queue row 51), which owns `termination`; `container_scope_for_a_new_reaper` and `render_container_argv` are the two sites
---

## Failure sequence

`set_container_reclaim_scope(Some(&scope))` renders the scope once to refuse what a reaper could
not exec — `render_container_argv` resolves a bare program name against this process's `PATH`
through `resolve_reaper_program` and refuses one it cannot resolve — and then stores the
**unresolved** scope -> every reaper started afterwards calls `container_scope_for_a_new_reaper`,
which renders the scope again, resolving the bare name against `PATH` a second time, and folds any
failure into `None` with `.ok()` -> a program that resolved when the scope was armed and does not
when a reaper starts (the binary moved or removed, or `PATH` changed in this process) forks that
reaper with **no container scope**; the coordinator's death then leaves its labeled containers
running, with no diagnostic anywhere, because the arming call reported success and a reaper has no
error channel.

The fold is the shape §7 names: a decision taken at arm time, with an error channel, is retaken at
fork time and discarded. P3 because the trigger needs the runtime's binary to leave `PATH` between
arming and a reaper start — in which case `docker kill` would not have run either — and nothing in
production changes `PATH` in-process (`grep -rn set_var src --include=*.rs` outside test modules:
none at `425ad55`).

## What the change that takes this up should do

Resolve once. Either store the scope with its program already resolved — a `ReaperContainerScope`
whose `program` is the absolute path `resolve_reaper_program` returned — so the fork-time render
can fail only on the interior-NUL check arm time already passed, or store the rendered `CString`s
and build the pointer array per fork. Then `container_scope_for_a_new_reaper` has nothing to fold,
and `set_container_reclaim_scope`'s module doc in `ambient.rs`, which now says the `PATH` read
happens when the scope is armed and again as each reaper starts, says it happens once.
