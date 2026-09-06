# upstroke

[![CI](https://github.com/sourcemaps/upstroke/actions/workflows/ci.yml/badge.svg)](https://github.com/sourcemaps/upstroke/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/upstroke.svg)](https://crates.io/crates/upstroke)
[![license](https://img.shields.io/github/license/sourcemaps/upstroke.svg)](LICENSE)

A headless orchestration engine for AI coding agents. You and a frontier model design a piece of
work; `upstroke` executes the plan unattended: it turns the plan into a dependency graph of typed
tasks, dispatches each to an existing coding-agent CLI with a model chosen per task, verifies every
result through objective gates and a strong-model review — cross-family where a second model
family is available — and commits only what passes.

It never edits a file, never runs an agentic loop, and never calls a model API. It is the
conductor, not an instrument.

**Status.** `0.1.0` is released: the sequential conductor works end to end, passed its acceptance
run on 2026-08-10, and has since run against a real published library. v0.2 (worktree-per-task
parallelism, a merge queue, capacity-driven routing, more adapters) is in progress; its machinery
is on `master`, inert by default. [See a run, start to finish →](https://upstroke.rs/)

## Usage

| Command | What it does |
|---|---|
| `upstroke connect` | Finds the agent CLIs on this machine, asks each about its own account, writes `~/.upstroke/pools.toml`. No HTTP, no token handled. |
| `upstroke validate plan.md` | Parses the plan into a task graph and prints each task's model chain with the source of every decision. Zero spend. |
| `upstroke run plan.md --dry-run` | Everything except the agents: parse, route, effective gates, per-pool capacity preview. |
| `upstroke capacity` | Every pool: auth state, estimated remaining with confidence, resets, and what each strategy would do. |
| `upstroke run plan.md --budget 15` | Runs for real on a `upstroke/run-<id>` branch: one agent per task, the engine captures the diff, runs your gates, has a read-only reviewer judge it, commits per task. |
| `upstroke status [<run-id>]` | The ledger: per-task attempts, models, api-equivalent cost, per-pool drain. |
| `upstroke answer <question-id>` | Answers a question that parked a task. |
| `upstroke resume <run-id> [--budget 30]` | Continues an interrupted, parked or budget-stopped run from its event log. Gates, reviewers and bindings come from the run record. |
| `upstroke export-decisions <run-id> [--format csv]` | Exports a finished run's routing decisions to stdout, one row per attempt. Read-only; live runs are refused. |

Exit codes: `0` complete · `1` a task failed and the run halted · `2` tasks parked on unanswered
questions · `3` stopped at its budget (raise it and `resume`).

## Plans

Any markdown works: headings, checklists or numbered steps become tasks, with dependencies in
document order. An HTML-comment annotation grammar refines any plan without changing how it
renders; unknown attributes warn, never fail.

```markdown
## Design the pagination API
<!-- upstroke: id=api-design kind=design depends= tier=frontier out=api-contract -->

## Fix off-by-one in list endpoint
<!-- upstroke: id=fix-obo kind=fix depends=api-design min=mid paths=src/api/** -->
```

A task is ready for unattended execution when its worker and reviewer can reach the same verdict
from what they receive: acceptance criteria as observable outcomes rather than preferred idioms
("must return an overflow error rather than panic" is reviewable; "no `unwrap`" is ambiguous),
boundary conditions and failure behaviour named, evidence for each criterion inspectable (a test,
a command result, a property of the diff), and choices that change what "correct" means resolved
while you are present.

## Configuration

Pools are user-level (your subscriptions travel with you); routing, gates and budgets are
repo-level overrides on derived defaults. A fresh repo runs with zero config; gates are derived
from the repo's shape (`Cargo.toml`, `go.mod`, `package.json`) when unconfigured.

`upstroke.toml`, in the repo:

```toml
[routing]
fix = { chain = ["small", "mid", "frontier"], attempts_per = 2 }
review = { tier = "frontier", timeout_secs = 5400 }   # per pass; or { enabled = false }

[routing.effort]
implementation = "xhigh"               # reasoning depth for every worker attempt
review = "max"                         # and every review pass

[[routing.overrides]]
paths = ["src/auth/**", "migrations/**"]
start_at = "frontier"                  # blast radius beats nominal difficulty
second_opinion = "different-vendor"    # a second reviewer from another model family; both must pass

[budgets]
run_usd  = 15.0                        # api-equivalent; either ceiling ends the run (exit 3)
task_usd = 4.0

[interaction]
ask_before = { frontier_escalation_over_usd = 5.0 }

[[gates]]
name = "test"
cmd = "cargo test"
timeout_secs = 1200
```

`~/.upstroke/pools.toml`, written by `upstroke connect` and yours to edit:

```toml
[pools.claude-code]
kind = "subscription-window"           # credits | request-pool | api-key | unmetered
agent = "claude-code"
window = "5h"
weekly = true
sources = ["signals", "self"]          # trust order
safety_margin = 0.15                   # usage on your other machines is invisible
reserve = 0.20                         # headroom kept for your interactive sessions
profile = "work"                       # optional: which account, when one vendor backs several pools
```

Pools are listed in file order; the first one matching an agent is the one attempts are
attributed to. Spend reported by the CLIs is api-equivalent and, on subscriptions, notional; where
a route reports nothing (Copilot's does not) the ledger marks the total `?`.

## Guarantees

These are invariants, and they are what make it safe to leave running:

- **Agents edit files; the engine owns git.** Agents are told never to commit. The engine
  branches, stages, commits and rolls back.
- **The engine never calls a model API.** All model interaction happens inside subprocesses of
  official vendor CLIs. No proxies, no spoofed headers, no API keys.
- **Ground truth is the diff, not the transcript.** Gates check and reviewers judge the diff the
  engine captured, never the agent's account of what it did.
- **Narrow permissions.** Unattended agents get file tools plus exactly the configured gate
  commands, never a skip-all-permissions flag. Reviewers are read-only.
- **Independent review is preferred and its fallback is explicit.** Where a task runs on the exact
  model that would review it, upstroke rebinds the review to another model family. If that
  alternative cannot be probed, the run warns with the affected tasks and falls back to the
  frozen same-model reviewer instead of claiming independence. A configured blast-radius
  second opinion is stricter: another model family must be available, both reviewers judge the
  same diff independently, and both must pass.
- **An empty diff can never pass.** "Done" claims require changed code.
- **Estimates are never optimistic.** An unmeasured pool reads as *unknown*, not full; a figure
  derived only from what upstroke spent is shown as a ceiling (`≤`); every estimate is discounted
  by a safety margin and a reserve floor.

## Requirements

Rust 1.85+ (edition 2024), Git 2.40+ (`git check-attr --source`), and
[Claude Code](https://docs.claude.com/en/docs/claude-code/overview) on `PATH`. The
[GitHub Copilot CLI](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference)
and the Codex CLI are optional and unlock cross-family review. Windows, macOS and Linux are all
first-class.

On Windows every supervised command is assigned to a kill-on-close Job Object before its primary
thread runs, so ordinary descendants are terminated after a direct-child exit, a timeout, or
abrupt conductor death; Unix host runs retain a cleanup lease for ordinary descendants.

Host-run mode is for trusted repositories and plans: gates execute the candidate's own build and
test code as your OS account, and nothing shipped stops that code from reaching `.upstroke` run
files. Use a dedicated OS account or VM for untrusted input.

## Licence

**Apache License 2.0**, see [LICENSE](LICENSE) and [NOTICE](NOTICE). The name is not part of the
grant (Apache-2.0 §6): use "upstroke" to refer to this project, not to present a modified version
as the original.

Contributions are welcome under the CLA and workflow in [CONTRIBUTING.md](CONTRIBUTING.md); Rust
changes follow the [coding standards](CODING_STANDARDS.md), and the design lives in
[DESIGN.md](DESIGN.md).
