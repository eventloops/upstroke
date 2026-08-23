# upstroke — Step 1 Kickoff Brief

Pair this file with `DESIGN.md` (v2.1) in a fresh Claude Code session. DESIGN.md is the spec;
this file is the scope. Where they seem to conflict, this file wins — it exists to keep the
first session small.

**Suggested opening prompt for the session:**

> Read DESIGN.md and KICKOFF.md in full. Build exactly what KICKOFF.md scopes as Step 1 —
> nothing from later steps. Work in plan mode first: propose the module skeleton and the
> parsing approach, ask any clarifying questions, then implement. Acceptance criteria are
> the checklist at the bottom of KICKOFF.md; the task is done when `cargo fmt --check`,
> `cargo clippy --all-targets -- -D warnings`, and `cargo test` are green and
> `upstroke validate fixtures/sample-plan.md` prints the expected table.

---

## Deliverable

A working `upstroke validate <plan>` — parse an annotated markdown plan into the IR, load
config (tolerating its absence), derive routing chains, and print a task table with the
source of every decision. No execution of anything.

## In scope (Step 1 = DESIGN.md §21 step 1, plus just enough of steps 2's parser to be real)

- **IR types** (DESIGN.md §7 subset): `Plan`, `Task`, `TaskKind`, `Tier` (Small | Mid |
  Frontier), `Artifact` stubs. Serde-serializable; `plan.normalized.json` output behind
  `--emit-json`.
- **Markdown plan adapter** (§9): headings/checklist items → tasks; `Acceptance` / `Done
  when` bullet lists; document-order default dependencies; keyword-heuristic kinds;
  path-hint collection; the full `<!-- upstroke: ... -->` annotation grammar (`id`, `kind`,
  `depends`, `tier`, `min`, `needs`, `out`, `paths`; unknown attrs warn, never error).
- **Config load**: repo `upstroke.toml` (overrides only) and user `~/.upstroke/pools.toml` —
  both optional; sensible derived defaults when absent. Just the shapes needed by
  validate: `[routing]` chains, `[[routing.overrides]]`, `[[pins]]`, `[routing.strategy]`
  (parsed, echoed, not acted on).
- **Static capability catalog**: a small built-in table (model → tier) covering current
  Claude Code and Copilot models; unknown model in a pin = hard error with a helpful
  message.
- **Routing resolution + binder PREVIEW** (§10): defaults ⊕ path-glob floors ⊕ annotation
  `tier=`/`min=` ⊕ pins → per-task chain, each element annotated `(preview)` with a
  catalog-derived example binding. No capacity math — print `capacity: not connected`.
- **Validation**: dependency cycle detection (error, naming the cycle), unknown `depends`
  ids, duplicate ids.
- **CLI**: `upstroke validate <plan> [--emit-json] [--config <path>]` via clap derive.
  Output: a plain aligned table — id | kind | deps | chain (with source: default /
  annotation / override / pin) — then warnings, then `ok: N tasks, no cycles`.

## Explicitly OUT of scope — do not build, stub, or scaffold

Agent adapters or any subprocess code; git operations; gates; review; the event log;
scheduler; questions/notifiers; live capacity or `upstroke connect`; `upstroke run`; tokio
(Step 1 is fully synchronous); any HTTP client (never, per invariant 2). Declaring the §8
traits as empty definitions is allowed if it clarifies structure; implementing them is not.

## Crate layout

Single crate, lib + thin bin for testability:

```
upstroke/
  Cargo.toml
  src/
    lib.rs          # re-exports
    ir.rs           # Plan, Task, TaskKind, Tier, Artifact
    plan/mod.rs     # PlanAdapter trait + markdown.rs (adapter + annotation grammar)
    config.rs       # repo + pools TOML, derived defaults
    catalog.rs      # static model → tier table
    route.rs        # chain resolution + binder preview
    validate.rs     # orchestrates parse → resolve → report
    error.rs        # thiserror types
    main.rs         # clap CLI, anyhow at the edge
  fixtures/
    sample-plan.md  # the file below
    bare-plan.md    # no annotations — heuristics path
    cyclic-plan.md  # must fail with a named cycle
```

## Dependencies (keep to exactly these)

`clap` (derive), `serde` + `serde_json`, `toml`, `thiserror` (lib errors), `anyhow` (bin
edge only), `globset` (path floors), `pulldown-cmark` (markdown events; parse annotations
from HTML-comment events, not regex over raw text). No tokio, no reqwest, nothing else
without asking.

## Conventions

Edition 2024. `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings` clean. No
`unwrap`/`expect` outside tests. All paths via `std::path` — this must run on Windows
first-class (PowerShell examples in docs, no hardcoded `/`). Unit tests colocated;
fixture-driven tests for the parser covering: annotated plan, bare plan, cycle error,
unknown-depends error, annotation attribute edge cases (empty `depends=`, unknown attr
warning). Table output asserted loosely (contains rows), not byte-snapshotted.

## Fixture: `fixtures/sample-plan.md`

```markdown
# Pagination rework

## Design the pagination API
<!-- upstroke: id=api-design kind=design depends= tier=frontier out=api-contract -->
Define cursor format, page-size limits, and error contract.

Acceptance:
- Cursor format documented
- Error contract covers empty pages

## Implement cursor encoding
<!-- upstroke: id=cursors kind=implement depends=api-design needs=api-contract paths=src/api/** -->
Implement opaque cursor encode/decode per the contract.

## Fix off-by-one in list endpoint
<!-- upstroke: id=fix-obo kind=fix depends=cursors min=mid paths=src/api/** -->

## Update API docs
<!-- upstroke: id=docs kind=docs depends=fix-obo -->
```

Expected shape of `upstroke validate fixtures/sample-plan.md`: four rows; `api-design`
chain starts at frontier (annotation); `fix-obo` chain starts no lower than mid (min
clip) and carries the `src/api/**` floor note; `docs` shows the derived default; every
chain element tagged with its source and `(preview)` binding; `ok: 4 tasks, no cycles`.

## Acceptance checklist

- [ ] `upstroke validate fixtures/sample-plan.md` prints the table above with decision sources
- [ ] `bare-plan.md` parses via heuristics (document-order deps, keyword kinds)
- [ ] `cyclic-plan.md` fails with the cycle named; unknown `depends` ids fail clearly
- [ ] Missing `upstroke.toml` and missing pools file both fall back to derived defaults silently
- [ ] `--emit-json` writes `plan.normalized.json` matching the IR
- [ ] fmt, clippy `-D warnings`, and tests green; runs on Windows and Linux
