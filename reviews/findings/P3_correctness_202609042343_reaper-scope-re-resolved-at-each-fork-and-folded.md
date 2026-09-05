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
whose `program` is the path `resolve_reaper_program` returned, which is a `PATH` entry joined to the
name and is relative when that entry was (measured; `find_program` does not canonicalise) — so the
fork-time render can fail only on the interior-NUL check arm time already passed, or store the
rendered `CString`s and build the pointer array per fork. Then `container_scope_for_a_new_reaper`
has nothing to fold, and the module doc in `ambient.rs`, which now says the `PATH` read happens when
the scope is armed and again as each reaper starts, says it happens once. A relative stored path
still depends on the working directory at fork time; resolving to an absolute path at arm time
closes that too, and is the better of the two.

## Measured

At `7150ea9`, with an uncommitted probe inside `termination`'s test module (private functions), run
alone under `--exact`, tree restored afterwards. A directory holding an executable stub named
`upstroke-probe-010-docker` was prepended to `PATH`; a scope was built over that bare name;
`set_container_reclaim_scope(Some(&scope))` returned `Ok`; `container_scope_for_a_new_reaper()`
returned `Some`; the stub file was removed; `container_scope_for_a_new_reaper()` returned `None`.

```
PROBE010 armed-render-some=true after-removal-render-some=false
```

The sequence above fires as written: arming succeeded and the next reaper would have been forked
with no container scope and no diagnostic. The full probe is quoted in PR #147's comment of
2026-09-05 00:28Z.
