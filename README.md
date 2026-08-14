# tactus

[![CI](https://github.com/keybindings/tactus/actions/workflows/ci.yml/badge.svg)](https://github.com/keybindings/tactus/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/tactus.svg)](https://crates.io/crates/tactus)
[![license](https://img.shields.io/crates/l/tactus.svg)](LICENSE)

A headless orchestration engine for AI coding agents. A frontier model and you design a piece
of work together; `tactus` then executes that plan unattended — normalizing it into a dependency
graph of typed tasks, dispatching each to an existing coding-agent CLI with a model chosen per
task, verifying every result through objective gates and strong-model review, and committing
only what passes.

It never edits a file, never implements an agentic loop, and never calls a model API. It is the
conductor, not an instrument.

> **Status: v0.1 done — the acceptance run passed, and the engine has since been used on a real
> published library.** The build order is public in `DESIGN.md` §21; steps 1–10 are done — plan
> ingestion, routing, the Claude Code and Copilot adapters, the sequential engine with git
> ownership, gates, cross-family review, the verification ladder, the event log with resume and
> status, and the capacity engine **read-only**. §21's acceptance run passed on 2026-08-10,
> across two runs against a *scratch* repository: the first was killed mid-attempt and resumed
> to completion, and the second took a five-task plan through unattended for $6.04 — a small
> model committing first try, a same-rung retry recovering from a reviewer's verdict, an
> escalation to a stronger rung, and a design question parking one task while an independent one
> kept moving. It found three engine defects, all fixed and two of them re-verified live on the
> run that found them; a fourth turned up on the first real-library run afterwards.
> `acceptance/RESULT.md` is the write-up. Parallelism, worktrees, Aider, and capacity-driven
> routing are v0.2.
>
> **[See a run, start to finish →](https://keybindings.github.io/tactus/)** — captured output from
> both: the verdict where a reviewer rejected a fix that built clean and passed all 722 tests
> comes from the run against a published .NET library, and the interactive replay and ledger come
> from the acceptance run. The page says which is which.

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
Gates, reviewers, resolved worker bindings, and the implementation/review effort policy come from
the run record; editing them applies to a new run rather than changing the standard halfway
through this one.

```bash
tactus export-decisions <run-id>
tactus export-decisions <run-id> --format csv
```

Exports one non-live run's local routing decisions to stdout, as JSONL by default or RFC
4180-style CSV. It reads only that run's event log and frozen normalized plan, makes no network
request, and writes nothing; redirect stdout if you want to keep a file. A run id may be an
unambiguous prefix. Live runs are refused because their dataset is still moving. Null JSON values,
empty CSV cells, and `selection_origin: "unknown"` mean an older run did not record that fact—not
that tactus inferred it from today's plan or configuration. Recoverable crash residue at the end
of an otherwise valid log is reported on stderr without contaminating the exported stdout stream.

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

### Definition of Ready for agents

An unattended task is ready when its worker and reviewer can reach the same
verdict from the artifacts they receive. Before running a plan:

- State acceptance criteria as observable outcomes, not preferred implementation
  idioms.
- Name boundary conditions and failure behaviour, with representative examples
  where the edge would otherwise be ambiguous.
- Make the evidence for each criterion inspectable: a test, command result,
  generated artifact, or property of the diff.
- Resolve choices that change what "correct" means while the user is present. If
  a choice is intentionally deferred, make the question and its affected tasks
  explicit.
- Keep implementation constraints separate from acceptance behaviour. When a
  particular syntax, dependency, or platform mechanism really is required, say
  why and make the constraint mechanically unambiguous.

For example, "must return an overflow error rather than panic, wrap, or
truncate" is reviewable behaviour. "No `unwrap`" is not: it can be read as
banning either the panicking `.unwrap()` call or every non-panicking method
whose name shares that prefix.

## Configuration

Config splits along its natural seam: **pools are user-level** — your subscriptions travel with
you — while **routing, gates and budgets are repo-level** overrides on derived defaults. A fresh
repo runs with zero config.

`tactus.toml`, in the repo:

```toml
[routing]
fix = { chain = ["small", "mid", "frontier"], attempts_per = 2 }
review = { tier = "frontier", timeout_secs = 5400 }
                                        # timeout is per pass and includes one format re-ask;
                                        # or use { enabled = false }

[routing.effort]
implementation = "xhigh"              # every worker attempt, regardless of tier
review = "max"                         # every review pass
# Effort controls reasoning depth, not model tier. To require frontier models
# for all workers, set every task kind's chain to ["frontier"] explicitly.

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
- **Independent review is preferred and its fallback is explicit.** Where a task runs on the exact
  model that would have reviewed it, Tactus opportunistically rebinds to another model family. If
  that alternative cannot be probed, the run warns with the affected tasks and falls back to the
  frozen same-model reviewer instead of silently claiming independence. A configured blast-radius
  second opinion is stricter: another model family must be available, both reviewers judge the same
  diff independently, and both must pass.
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

One crash-containment caveat is explicit: the current Windows host runner kills a process tree
when it observes a timeout, but cannot retain ownership if the Tactus conductor itself is killed.
Until the planned Job Object plus external cleanup guardian lands, run the conductor under WSL
when a hard conductor crash must not leave an ordinary agent descendant running or spending. The
external/container runner is a v0.2 design commitment, not a shipped option. Unix and WSL host runs
retain a cleanup lease for ordinary descendants; deliberately daemonised escape remains outside
the current host-runner contract on every platform.

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
