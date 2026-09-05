## 9. Plan ingestion (P1)

**v0.1 adapters:** Claude Code plan-mode markdown (primary) and the annotation grammar that upgrades *any* markdown. **v0.2:** generic checklist, JSON schema, and claude-task-master import — turning the most popular DAG generator into an upstream feeder rather than a competitor.

**Backlog adapters (v0.2+):** Jira, Azure DevOps work items, GitHub Issues. These feed **Phase 1, not Phase 2** — a backlog item is not a plan: no dependency DAG, no acceptance criteria a gate can check, no tier annotations, no conventions brief. The importer emits a *draft* plan that the designer then subjects to question exhaustion (§5); execution still runs only frozen, annotated plans. Feeding a backlog straight to Phase 2 would point unattended agents at under-specified stories, which is the failure the two-phase lifecycle exists to prevent. Invariant 2 holds by subprocessing the vendor's own CLI (`az boards`, `acli`, `gh`) from a separate `upstroke import` command — the network stays out of the engine and reuses auth the user already has. **Write-back is a different seam:** transitioning the item on commit, attaching branch and shas, moving it to Blocked when a question parks the task is a `Notifier` over the event log (§8), not a plan adapter. `Task` gains an `external_ref` so a run traces back to the item that spawned it.

Parsing rules (markdown): each `##`/`###` section or top-level checklist item becomes a task (heading → title, body → body); a bullet list under `Acceptance` / `Done when` / `Success criteria` → acceptance; file paths in the body are collected into `path_hints`; **default dependencies are document order** (task N depends on N−1) unless annotations say otherwise.

Annotation grammar — HTML comments, invisible in rendered markdown:

```markdown
## Design the pagination API
<!-- upstroke: id=api-design kind=design depends= tier=frontier out=api-contract -->

## Fix off-by-one in list endpoint
<!-- upstroke: id=fix-obo kind=fix depends=api-design min=mid needs=api-contract paths=src/api/** -->
```

Attributes: `id`, `kind`, `depends` (empty = none, breaking the chain), `tier` (designer suggestion), `min` (binding floor), `needs`/`out` (artifact wiring), `paths` (globs). Unknown attributes warn, never error. Un-annotated plans still run: kinds by keyword heuristic, dependencies by document order, artifacts defaulting to a conventions brief from the first Design task.

**What `validate` refuses, and what it only warns about.** After parsing, the dependency graph is checked as a whole, so one run reports every structural problem: an id two tasks share, and a `depends` target no task carries, are collected together and refuse the plan. On a graph where every id names one task and every edge resolves, a dependency cycle refuses it, reported as the ids along the cycle with the first repeated at the end (`a -> c -> b -> a`); a task that depends on itself is a cycle of one. Artifact wiring only warns, because the plan is frozen (§5) and the engine invents no edges: a task that `needs` an artifact without depending, directly or transitively, on the task recorded as producing it; and, from the adapter, a `needs` that no task produces at all. A task that `needs` an artifact it is itself recorded as producing is not warned about: the record holds one producer per artifact, a second `out=` for the same artifact has no defined meaning yet, and the check cannot tell such a plan from one with a second declaration on a task it depends on, so it stays silent on both. A warning is shown with the preview and never fails validation.
