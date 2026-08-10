# tactus

A headless orchestration engine for AI coding agents. A frontier model and you design a piece
of work together; `tactus` then executes that plan unattended — normalizing it into a dependency
graph of typed tasks, dispatching each to an existing coding-agent CLI with a model chosen per
task, verifying every result through objective gates and strong-model review, and committing
only what passes.

It never edits a file, never implements an agentic loop, and never calls a model API. It is the
conductor, not an instrument.

> **Status: v0.1 feature-complete, not yet proven end to end.** The build order is public in
> `DESIGN.md` §21; steps 1–10 are done — plan ingestion, routing, the Claude Code and Copilot
> adapters, the sequential engine with git ownership, gates, cross-family review, the
> verification ladder, the event log with resume and status, and the capacity engine
> **read-only**. What remains for v0.1 is the acceptance run on a real repository. Parallelism,
> worktrees, Aider, and capacity-driven routing are v0.2.

## What works today

```bash
tactus connect
```

Discovers the agent CLIs installed on this machine, asks each one about its own account, and
writes `~/.tactus/pools.toml`. No HTTP, no token ever handled — it subprocesses the vendors'
own CLIs. The file is hand-editable and never overwritten without `--force`.

```bash
tactus validate plan.md
```

Parses an annotated markdown plan into a task graph, resolves each task's model escalation
chain, and prints the table with the source of every decision — at zero spend.

```bash
tactus run plan.md --dry-run
```

Everything except the agents: parse, route, show the effective gates, and preview each pool's
estimated capacity and what your strategy would do with it. Spends nothing, probes nothing.

```bash
tactus capacity
```

Every pool: auth state, estimated remaining with its confidence, resets, margins, and what
`conserve` / `value-max` / `deadline` *would* do. Estimates are conservative by construction —
an unmeasured pool reads as **unknown**, never as full.

```bash
tactus run plan.md --budget 15
```

Executes for real: creates a `tactus/run-<id>` branch, runs one agent per task, captures the
diff itself, runs your gates, has a read-only reviewer judge the diff against the task's
acceptance criteria, and commits per task. Every transition lands in `events.jsonl`, which is
what `tactus status`, `tactus answer`, and `tactus resume` read back.

```bash
tactus resume <run-id> --budget 30
```

Continues a run that was interrupted, ended with tasks parked on questions, or stopped at its
budget. Budgets are re-read at resume, so raising the ceiling and continuing is one command.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | complete |
| 1 | a task failed and the run halted |
| 2 | the run ended with tasks parked on unanswered questions |
| 3 | the run stopped at its budget — raise it and `tactus resume` |

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

Config splits along its natural seam: **pools are user-level** — your subscriptions travel with
you — while **routing, gates and budgets are repo-level** overrides on derived defaults. A fresh
repo runs with zero config.

`tactus.toml`, in the repo:

```toml
[routing]
fix = { chain = ["small", "mid", "frontier"], attempts_per = 2 }
review = { tier = "frontier" }        # or { enabled = false }

[[routing.overrides]]
paths = ["src/auth/**", "migrations/**"]
start_at = "frontier"                  # blast radius beats nominal difficulty
second_opinion = "different-vendor"    # a second reviewer from another model family;
                                       # both must pass. Needs the Copilot CLI installed.

[budgets]
run_usd  = 15.0                        # api-equivalent; omit for unlimited
task_usd = 4.0                         # either ceiling ends the run (exit 3)

[interaction]
ask_before = { frontier_escalation_over_usd = 5.0 }
                                       # park and ask before escalating onto a frontier
                                       # rung once the run has reported this much spend

[[gates]]
name = "test"
cmd = "cargo test"
timeout_secs = 1200
```

Gates are derived from the repo's shape when unconfigured (Cargo.toml, go.mod, package.json).

`~/.tactus/pools.toml`, written by `tactus connect` and yours to edit:

```toml
[pools.claude-code]
kind = "subscription-window"           # credits | request-pool | api-key | unmetered
agent = "claude-code"
window = "5h"
weekly = true
sources = ["signals", "self"]          # trust order; local-logs and provider-endpoint
                                       # are parsed but not read in v0.1
safety_margin = 0.15                   # usage on your other machines is invisible
reserve = 0.20                         # headroom kept for your interactive sessions
profile = "work"                       # optional: which account, when one vendor backs
                                       # several pools. Yours to write; connect keeps it
```

Pools are listed in **file order**, and the first one matching an agent is the one attempts
are attributed to — so moving a pool up the file promotes it.

Spend reported by the CLIs is api-equivalent and, on subscriptions, notional. Where a route
reports nothing at all — Copilot's does not — the ledger marks the total `?` rather than
presenting a floor as a figure.

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
- **Nothing reviews its own work.** Where a task runs on the model that would have reviewed it,
  the reviewer rebinds to a different model family; on blast-radius paths a second reviewer from
  another family judges the same diff independently, and both must pass.
- **An empty diff can never pass.** "Done" claims require changed code.
- **Estimates are never optimistic.** A capacity pool nothing has measured reads as *unknown*,
  not as full; a figure derived from what tactus alone has spent is shown as a ceiling (`≤`)
  rather than a measurement, because everything it cannot see — other repositories, your own
  interactive sessions — only ever drew *more*; and every estimate is discounted by a safety
  margin and a reserve floor before it is shown.

## Requirements

Rust 1.85+ (edition 2024) and [Claude Code](https://docs.claude.com/en/docs/claude-code/overview)
on `PATH`. The [GitHub Copilot CLI](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference)
is optional and unlocks the cross-family second opinion. Windows, macOS and Linux are all
first-class.

## Licence

**GNU AGPL v3 only** — see [LICENSE](LICENSE).

In plain terms, for the two things people usually want to know:

- **Running it costs you nothing and obliges you to nothing.** The AGPL's obligations attach to
  *distributing* the software or *offering a modified version to others over a network*. Using
  tactus on your own machine — including at work, including on proprietary code — triggers
  neither. Nothing links into your code: tactus is a separate process that shells out to other
  separate processes, so your repository is not a derivative work of it.
- **Building on it means sharing back.** Fork it, modify it, sell it if you like — but the source
  stays open under the same licence, including if you offer it to others as a hosted service.

If that doesn't suit — an internal policy that prohibits AGPL, or a product you need to keep
closed — a **commercial licence** is available. Open an issue or get in touch.

Contributions are welcome under the CLA in [CONTRIBUTING.md](CONTRIBUTING.md), which keeps that
dual-licensing possible.
