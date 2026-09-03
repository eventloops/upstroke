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
