# CLAUDE.md

Context for Claude Code sessions working on this repository.

## What upstroke is

A headless orchestration engine for AI coding agents, in Rust. It normalises an
annotated markdown plan into a dependency graph of typed tasks, dispatches each
to an existing coding-agent CLI with a model chosen per task, verifies results
through objective gates and strong-model review, escalates failures up an
explicit model chain, and commits only what passes.

The one-line framing: **it is the conductor, not an instrument.** It never edits
a file, never implements an agentic loop, and never calls a model API. Agents
edit; the engine subprocesses their official CLIs and owns git.

The binding invariants are **`DESIGN.md` §4**, and there are seven, not three.
The ones a change is most likely to trip over:

- **Agents edit files; the engine owns git.** Agents are told never to commit.
- **The engine never speaks HTTP.** All model interaction is inside agent
  subprocesses. `upstroke connect` shells out to each vendor CLI and asks it about
  its own account — no token is ever handled.
- **Ground truth is the diff, not the transcript.** Gates check, reviewers
  judge, feedback quotes `git diff` captured by the engine.
- **Every state transition is an event.** State is derived by replaying
  `events.jsonl`; resume is replay-then-continue. There is no second path.
- **Official CLIs only.** No ToS-violating proxies. The trust wedge is part of
  the product.

Read §4 in full before touching the engine, the event log, or anything that
handles capacity or questions.

## Where the project is

**v0.1 is complete** and released as `0.1.0` — the sequential conductor works,
end to end, against a real repository. **v0.2 is in progress**: parallel
execution over isolated worktrees, a compare-and-swap merge queue, an optional
container runner, capacity-driven routing, more adapters (Aider, task-master and
other plan formats), notifiers, and `export-decisions`. The capacity engine
currently ships **read-only** — `connect`, `capacity` and dry-run preview
estimate and report; nothing routes on it yet.

The build order is `DESIGN.md` §21 and it is deliberate. Check where the
project actually is before adding something out of sequence.

**`DESIGN.md` is the only living authority for product design.**
`CODING_STANDARDS.md` is normative for implementation quality, and
`MAINTAINING.md` is authoritative for the change lifecycle. `decisions/` are
dated, immutable records of why; `proposals/` are inputs that bind nothing
until a decision cites them. When a decision changes the spec, `DESIGN.md`
gets the compressed edit and the record is cited. Read `decisions/README.md`
before adding a record.

**New proposals are filed privately**, in a private companion repository,
engine mechanisms included
(`decisions/2026-08-27-proposals-private.md`). The dated proposals in
`proposals/` are relocation stubs since 2026-09-01
(`decisions/2026-09-01-proposals-relocated.md`): each keeps its path, title,
and status block verbatim so decision-record citations resolve — except the
Decided G2 pass plan, the council critique, and the portfolio proposal it
cites, which stay in full (content reliance and the public-lifecycle rule).
Read `proposals/README.md` before touching those. Decisions stay public and
still name their inputs.

## Gates

CI enforces these on every pull request to master. Run them before proposing a
change:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo +1.85.0 check --locked --all-targets --all-features   # MSRV
```

`--all-features` is what CI runs, and `CONTRIBUTING.md`, the pull-request
template and `CODING_STANDARDS.md` §2 state the same four cargo commands. The
crate defines no features today, so the flag is a no-op for now — use the CI
form regardless, so the two stay equivalent when that changes.

`1.85.0` is required and pinned by CI. There is **no `rust-toolchain.toml`** —
toolchain selection is explicit at call sites, so nothing auto-corrects a wrong
default. Install 1.85.0 alongside stable.

4 `test-*.sh` gates in `.github/scripts/` also run in CI's `lint` job. With the
four cargo commands above they are the eight-command baseline — the same eight
`CODING_STANDARDS.md` §2 states normatively and `CONTRIBUTING.md` and the
pull-request template carry. Invoke them **from the repository root**, the way
`ci.yml` does:

```bash
bash .github/scripts/test-release-record.sh
bash .github/scripts/test-pr-policy.sh
bash .github/scripts/test-pr-ledger-evidence.sh
bash .github/scripts/test-docs-consistency.sh
```

Repository-root invocation is the convention, not a shared implementation
requirement: most of the gates resolve their own directory and run from anywhere.
The one that does not is `test-pr-policy.sh`, which derives its location with
`${BASH_SOURCE[0]%/*}` -- that strips nothing when the argument carries no slash,
so it fails outright from inside `.github/scripts/`. Run them all the way `ci.yml`
does and the difference never matters. One of the four, `test-release-record.sh`,
needs `jq`.

## Hard conventions

Read and follow `CODING_STANDARDS.md` before changing Rust. In particular:

- **Edition 2024**, MSRV 1.85.
- **The §7 panic policy is lint-enforced.** `.unwrap()` is denied everywhere — tests included —
  and `.expect()`/`panic!` are denied outside tests, via Cargo.toml's `[lints]`;
  `#[expect(..., reason)]` marks the few documented invariant sites.
- **`anyhow` only at the binary edge.** Libraries return `thiserror` types.
- **All paths through `std::path`.** Windows is a first-class target — CI runs
  the full test and MSRV matrix on ubuntu, macos and windows.
- **Conventional commit titles**, enforced on PR titles by
  `.github/scripts/validate-pr-body.sh`: `type(optional-scope): summary`, where type is
  one of feat, fix, docs, refactor, test, chore, ci, build, perf, security,
  release, revert.

## How a change lands

`MAINTAINING.md` is authoritative for the full lifecycle. In outline:

1. Open a **draft PR early**.
2. Deterministic CI and PR-policy gates go green. `upstroke-ci` aggregates lint,
   msrv and the test matrix and is a required context.
3. An independent **frontier-model review** of the exact green head, recorded
   in the PR body: the reviewed SHA and a durable link to the verdict.
4. **Merge commits only.** Resolve every review conversation first. The
   owner's merge is the attestation; there is no machine-minted review check
   (`decisions/2026-08-23-retire-app-attestation.md`).

The PR body must contain all six sections — Summary, Scope, Validation, Review
evidence, Risk and rollback, Review finding ledger — and the ledger must use the
exact canonical header. `validate-pr-body.sh` rejects anything else; run it
locally against your body before pushing.

**A new push means the review no longer binds to the head.** A serious P1
repair returns for a fresh pass; a `MUST` deviation in materially touched
code and a finding carrying a failing test, reproduction, or mutation
witness are fixed before merge whatever their label; a repair-only delta
after a single-reviewer pass that found no serious P1 — the reviewed
findings' fixes and nothing else, never a gate change — is owner-verified
against the findings and disclosed in the
body (`decisions/2026-09-01-review-effort-rescoped.md`, which defines the
serious-P1 bar with MAINTAINING.md); anything wider is a new change and is
re-reviewed; a push confined to `reviews/FINDINGS.md` keeps the review
outright
(`decisions/2026-08-20-review-invalidation-scope.md`) — record both SHAs and
confirm the exempt-only diff yourself before merging. A conflict-free merge
of `master` that leaves the diff against `master` byte-identical, with CI
green on the merged head, keeps it too
(`decisions/2026-09-01-clean-base-merge-keeps-review.md`): record both
SHAs, both base SHAs, and the diff hash before and after. A completed
`CHANGES_REQUIRED` pass whose findings all landed as repairs or logged
baggage is recorded as that verdict, never as a pass.
Agents never merge to `master`: merging is the owner's act, and an agent
session has no standing to do it even when it runs on the owner's token.

## Where things are

| Path | What |
|---|---|
| `DESIGN.md` | The design; §4 invariants, §21 build order |
| `CODING_STANDARDS.md` | Normative Rust implementation and review standard |
| `MAINTAINING.md` | Full merge lifecycle, trust boundary, release contract |
| `CONTRIBUTING.md` | Contributor rules and CLA |
| `decisions/` | Dated, immutable decision records |
| `proposals/` | Design proposals filed before 2026-08-27; new ones are private |
| `.github/scripts/` | The 4 `test-*.sh` gates and the `validate-*` helpers they exercise |
| `acceptance/RESULT.md` | The v0.1 acceptance run write-up |
| `reviews/` | Review records, and the standing finding ledger once it lands |

`src/` is one crate. On master it is `plan/`, `agent/` (Claude Code, Copilot,
Codex adapters), and flat modules — `engine.rs`, `events.rs`, `review.rs`,
`gates.rs`, `ladder.rs`, `route.rs`, `capacity.rs`, `rundir.rs`, `workspace.rs`
among them. **Check the tree rather than this list**: the v0.2 topology work
splits `engine.rs` into `engine/` and adds `topology/`, and it reaches master
only when its pull request merges.

## Traps that have already cost time

**Exit codes and output disagree.** `codex login status` prints "Not logged in"
and exits **0**. `git rev-parse <unknown-ref>` echoes its argument to stdout and
errors only on stderr, so `[ -n "$sha" ]` passes with a literal `origin/branch`.
Use `git rev-parse --verify --quiet`, and never trust a bare `$?`.

**Non-interactive shells get nothing from `~/.bashrc`.** It opens with
`case $- in *i*) ;; *) return;; esac`. Agent subprocesses and `ssh host 'cmd'`
are non-interactive; anything they need must be sourced above that guard.

**Verify by hash, not by line count.** A one-character edit to production code
does not move a line count. When checking that a region is unchanged, hash it.

**A worker that dies without a report may have left a mutation applied.** No
completion record means no report *and* a possibly contaminated tree. Diff
against the last verified state before trusting it.

**Imperative success is not persistent success.** Verify that state survives a
reboot, not just that a command returned 0.

**Concurrent suites must not share an unset `CARGO_TARGET_DIR`.** The
container pre-clean key is fixed when the variable is unset
(`src/runner/container/fake.rs`, `R5-SEAMS-006`), so two bare `cargo test`
runs on one machine derive the same key and remove each other's live Docker
containers mid-run. Give each concurrent run its own target directory. CI is
unaffected — each job runs alone on its machine.
