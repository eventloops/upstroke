# tactus

A headless orchestration engine for AI coding agents. A frontier model and you design a piece
of work together; `tactus` then executes that plan unattended — normalizing it into a dependency
graph of typed tasks, dispatching each to an existing coding-agent CLI with a model chosen per
task, verifying every result through objective gates and strong-model review, and committing
only what passes.

It never edits a file, never implements an agentic loop, and never calls a model API. It is the
conductor, not an instrument.

> **Status: early. Not yet usable end to end.** The build order is public in `DESIGN.md` §21;
> steps 1–6 are done (plan ingestion, routing, the Claude Code adapter, the sequential engine
> with git ownership, gates, and review). Retry and escalation, the event log, the Copilot
> adapter, and the capacity engine are not built yet.

## What works today

```bash
tactus validate plan.md
```

Parses an annotated markdown plan into a task graph, resolves each task's model escalation
chain, and prints the table with the source of every decision — at zero spend.

```bash
tactus run plan.md --dry-run
```

Everything except the agents: parse, route, show the effective gates, spend nothing.

```bash
tactus run plan.md
```

Executes for real: creates a `tactus/run-<id>` branch, runs one agent per task, captures the
diff itself, runs your gates, has a read-only reviewer judge the diff against the task's
acceptance criteria, and commits per task. Halts on the first failure.

## Plans

Any markdown works — headings, checklists, or numbered steps become tasks, with dependencies in
document order. An HTML-comment annotation grammar upgrades any plan without changing how it
renders:

```markdown
## Design the pagination API
<!-- tactus: id=api-design kind=design depends= tier=frontier out=api-contract -->

## Fix off-by-one in list endpoint
<!-- tactus: id=fix-obo kind=fix depends=api-design min=mid paths=src/api/** -->
```

Unknown attributes warn; they never fail a plan.

## Configuration

A fresh repo runs with zero config. `tactus.toml` overrides the derived defaults:

```toml
[routing]
fix = { chain = ["small", "mid", "frontier"], attempts_per = 2 }
review = { tier = "frontier" }        # or { enabled = false }

[[routing.overrides]]
paths = ["src/auth/**", "migrations/**"]
start_at = "frontier"                  # blast radius beats nominal difficulty

[[gates]]
name = "test"
cmd = "cargo test"
timeout_secs = 1200
```

Gates are derived from the repo's shape when unconfigured (Cargo.toml, go.mod, package.json).

## Design guarantees

These are invariants, not aspirations — they're what make it safe to leave running:

- **Agents edit files; the engine owns git.** Agents are told never to commit. The engine
  branches, stages, commits and rolls back.
- **The engine never calls a model API.** All model interaction happens inside subprocesses of
  official vendor CLIs. No proxies, no spoofed headers, no API keys.
- **Ground truth is the diff, not the transcript.** Gates check and reviewers judge the diff the
  engine captured, never the agent's own account of what it did.
- **Narrow permissions.** Unattended agents get file tools plus exactly the configured gate
  commands — never a skip-all-permissions flag. Reviewers are read-only.
- **An empty diff can never pass.** "Done" claims require changed code.

## Requirements

Rust 1.85+ (edition 2024) and [Claude Code](https://docs.claude.com/en/docs/claude-code/overview)
on `PATH`. Windows, macOS and Linux are all first-class.

## Licence

See `Cargo.toml`. Not yet finalised — do not assume the current declaration is the one this will
ship under.
