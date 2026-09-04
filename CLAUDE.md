# CLAUDE.md

Context for Claude Code sessions working on this repository.

## What upstroke is

A headless orchestration engine for AI coding agents, in Rust. It normalises an annotated markdown
plan into a dependency graph of typed tasks, dispatches each to an existing coding-agent CLI with a
model chosen per task, verifies results through objective gates and strong-model review, escalates
failures up an explicit model chain, and commits only what passes.

**It is the conductor, not an instrument.** It never edits a file, never implements an agentic
loop, and never calls a model API. Agents edit; the engine subprocesses their official CLIs and
owns git.

The binding invariants are `DESIGN.md` §4 (`design/04_design_invariants.md`). There are seven.
The ones a change is most likely to trip over:

- **Agents edit files; the engine owns git.** Agents are told never to commit.
- **The engine never speaks HTTP.** All model interaction is inside agent subprocesses.
- **Ground truth is the diff, not the transcript.**
- **Every state transition is an event.** State is derived by replaying `events.jsonl`; resume is
  replay-then-continue. There is no second path.
- **Official CLIs only.** No ToS-violating proxies.

Read §4 in full before touching the engine, the event log, or anything that handles capacity or
questions.

## Where the project is

`0.1.0` is released: the sequential conductor works end to end. v0.2 is in progress. Its
parallel-execution machinery (worktree-per-task, the compare-and-swap merge queue, the optional
container runner, the topology layer) reached master on 2026-09-01, inert by default: the v0.1
path is unchanged and the schema-4 machinery engages only by explicit schema choice.
`export-decisions` landed 2026-08-12. Still to come: capacity-driven routing (the capacity engine
ships read-only), more adapters, notifiers. No `0.2.0` tag exists. The build order is `DESIGN.md`
§21 and it is deliberate; check where the project is before adding something out of sequence.

**`DESIGN.md` is the only living authority for product design**, indexed at the root and split
into `design/` by section. `CODING_STANDARDS.md` indexes the implementation standards in
`standards/`. `MAINTAINING.md` is the change lifecycle. When a decision changes the design, the
section changes in the same pull request; there is no separate decision record to keep in step
(the decisions, proposals and acceptance directories were retired on 2026-09-03; the DESIGN.md
index says where each record's substance now lives).

## Gates

CI enforces these on every pull request to master. Run them from the repository root:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo +1.85.0 check --locked --all-targets --all-features
bash .github/scripts/test-release-record.sh
bash .github/scripts/test-pr-policy.sh
bash .github/scripts/test-pr-ledger-evidence.sh
bash .github/scripts/test-docs-consistency.sh
```

The crate defines no features today, so `--all-features` is a no-op; use the CI form regardless.
`1.85.0` is the MSRV and CI pins it; there is **no** `rust-toolchain.toml`, so toolchain selection
is explicit at call sites. The 4 `test-*.sh` gates in `.github/scripts/` run in CI's `lint` job.
`test-release-record.sh` needs `jq`. `test-pr-policy.sh` derives its own directory with
`${BASH_SOURCE[0]%/*}`, which works from any directory except one: invoked by bare name from
inside `.github/scripts/`, the expansion strips nothing and it fails. Invoke it by path, as above.
These are CI's commands; a shared build machine may wrap `cargo` and assign target directories
itself, and its wrapper is the way to run them there.

## Hard conventions

Read the `standards/` sections a change touches. In particular:

- **Edition 2024, MSRV 1.85.**
- **The §7 panic policy is lint-enforced.** `.unwrap()` is denied everywhere, tests included;
  `.expect()` and `panic!` are denied outside tests via `Cargo.toml`'s `[lints]`.
- **`anyhow` only at the binary edge.** Libraries return `thiserror` types.
- **All paths through `std::path`.** Windows is a first-class target; CI runs the full matrix on
  ubuntu, macos and windows.
- **No shared ownership, locks, or non-trivial clones without a stated reason, and every `?`
  deliberate** (§6, §7). These bind the code a change adds or rewrites; `standards/SWEEP.md`
  states the activation rule and tracks the file-by-file cleanup of the existing tree.
- **Conventional commit titles**, enforced on PR titles by `.github/scripts/validate-pr-body.sh`:
  `type(optional-scope): summary`, type one of feat, fix, docs, refactor, test, chore, ci, build,
  perf, security, release, revert.

## How a change lands

`MAINTAINING.md` is authoritative. In outline: draft PR early; the eight gates and both required
contexts (`upstroke-ci`, `upstroke-pr-policy`) green; one frontier review of the exact green head
(`gpt-5.6-sol` at `max`, the verdict posted to the PR as one SHA-bound comment); triage: serious
P1s relevant to the change are fixed and re-reviewed; a `MUST` deviation in touched code and any
finding carrying a failing test, reproduction or mutation witness are fixed whatever their label;
everything else is fixed or logged as a tech-debt ledger row; merge commit once green. The PR body
must carry the six sections and the exact canonical ledger header; run `validate-pr-body.sh`
against it locally.

Merging is the owner's act unless the owner has delegated it in writing for that PR; say so in the
body when it has been.

## Where things are

| Path | What |
|---|---|
| `DESIGN.md`, `design/` | The design; §4 invariants, §21 build order, §25 export schema |
| `CODING_STANDARDS.md`, `standards/` | Implementation standards, one section per file; `standards/SWEEP.md` |
| `MAINTAINING.md` | Change lifecycle, trust boundary, release contract |
| `CONTRIBUTING.md` | Contributor rules and CLA |
| `.github/scripts/` | The 4 `test-*.sh` gates and the `validate-*` helpers they exercise |
| `reviews/` | `reviews/FINDINGS.md`, the standing finding ledger; historical review records moved to the private lab repository on 2026-09-04 |
| `effects/` | The effect-governance allowlists the `src/effects/tests.rs` census enforces |

`src/` is one crate: `plan/` (ingestion), `agent/` (the Claude Code, Copilot and Codex adapters,
with `proc/` for subprocess handling), `engine/` (the conductor), `topology/` (the v0.2 execution
topology), `runner/` (host and container execution), `events/`, `validate/`, `status/`,
`effects/`, `connect/`, and flat modules such as `review.rs`, `gates.rs`, `ladder.rs`, `route.rs`,
`capacity.rs`, `rundir.rs`, `workspace.rs`, `workspace_manager.rs`, `export.rs`. Large modules
are being split into per-concern children; check the tree rather than this list.

## Traps that have already cost time

**Exit codes and output disagree.** `codex login status` prints "Not logged in" and exits **0**.
`git rev-parse <unknown-ref>` echoes its argument to stdout and errors only on stderr. Use
`git rev-parse --verify --quiet`, and never trust a bare `$?`.

**Non-interactive shells get nothing from `~/.bashrc`.** It opens with
`case $- in *i*) ;; *) return;; esac`. Agent subprocesses and `ssh host 'cmd'` are
non-interactive; anything they need must be sourced above that guard.

**Verify by hash, not by line count.** A one-character edit does not move a line count.

**A worker that dies without a report may have left a mutation applied.** Diff against the last
verified state before trusting a tree.

**Imperative success is not persistent success.** Verify that state survives a reboot.

**Concurrent suites must not share an unset `CARGO_TARGET_DIR`.** The container pre-clean key is
fixed when the variable is unset (`src/runner/container/fake.rs`, `R5-SEAMS-006`), so two bare
`cargo test` runs on one machine remove each other's live Docker containers mid-run. Concurrent
runs need distinct target directories, assigned however the machine assigns them (a build wrapper
that owns them, or an explicit `CARGO_TARGET_DIR` per run where nothing does); never two runs on
one unset value. CI is unaffected.

**`src/export.rs` pins sentences in the docs.** Its tests `include_str!` `README.md`,
`MAINTAINING.md`, `design/15_design_event_log_resume_run_layout.md` and
`design/25_design_export_decisions_schema.md`; a rewording there fails the suite.
