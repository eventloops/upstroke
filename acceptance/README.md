# The v0.1 acceptance run

DESIGN.md §21's definition of done, staged. **This spends real quota** — it is
operator-triggered, never launched by an agent.

The two files beside this one go into a *scratch* repository, not into tactus:

- `plan.md` — five tasks, each shaped to provoke one §21 criterion
- `tactus.toml` — the routing and gates that do the provoking

Each is commented with *why* its knobs are set the way they are. Read those
before changing anything; most of the values look arbitrary and are not.

## What has to be true first

```bash
claude auth status --json
```

Must report `"loggedIn": true`. Pre-flight refuses to start otherwise, and
logging in is a user-driven step tactus never automates.

The Copilot CLI is optional. Without it you still satisfy §21 — but the ledger's
per-pool section only ever shows one pool, and the cross-vendor second opinion
stays commented out in the config.

## Set up the target repo

Not tactus itself: agents editing the engine mid-run is a bad time, and §14
rolls a failed attempt back with `git clean -fd`.

```bash
cargo new --lib acceptance-target
```

```bash
cd acceptance-target && git add -A && git commit -m "seed"
```

Then copy `plan.md` and `tactus.toml` in beside `Cargo.toml`. The working tree
must be clean — the engine refuses a dirty one.

## Rehearse at zero spend

Free, so do it until the table says exactly what you expect:

```bash
tactus run plan.md --dry-run --budget 15
```

Check four things before spending anything:

1. **Five tasks, no cycles**, and `changelog` shows `deps -` (it must be
   independent, or §21(d)'s second half cannot happen).
2. **`readme` routes to `small` only**; `parse-edges` shows three rungs.
3. **`gates: check, lint, test [from config]`** — if it says `[derived]`, the
   config is not being read and `--config` is pointing somewhere wrong.
4. **The capacity block names your pools.** If it says `not connected`, run
   `tactus connect` first.

## Run it

```bash
tactus run plan.md --budget 15
```

### Which answer channel you get

This decides how §21(d) plays out, and it is not a config setting — it is
whether stdin is a terminal:

- **Attached to a terminal** → the engine prompts you on stdin when it hard-
  blocks. Simplest, but it is not the `tactus answer` path §21 names.
- **Detached** (stdin redirected, CI, a background job) → the engine waits
  `wait_on_block_secs` for an answer to arrive as a file, which is what
  `tactus answer` writes. This is the one to demonstrate.

The deterministic way to force the second, without fighting PowerShell's stdin
redirection:

```bash
tactus run plan.md --budget 15 --interaction never
```

That parks the question and ends the run at **exit 2** instead of waiting. Then,
in the same terminal:

```bash
tactus status
```

```bash
tactus answer <question-id>
```

```bash
tactus resume <run-id>
```

Which also exercises exit 2, the out-of-band answer, and resume — more surface
than sitting at a prompt would.

## The kill test

§21 ends with "kill the engine mid-run and `resume` finishes it". Do it while an
attempt is actually running — during a gate is ideal, since that is the longest
window:

```bash
tactus resume <run-id>
```

What should happen, and what to check in `tactus status` afterwards: the
interrupted attempt appears in the ledger with **unknown cost** (`—`, not
`$0.0000`), its rung's allowance is **not** spent, any half-written files were
discarded, and the task completes on the retry.

## Reading the result

```bash
tactus status <run-id>
```

§21(e) wants per-task attempts, models, api-equivalent cost, and per-pool drain.
The ledger has all four. Things worth looking at rather than skimming:

- **The `trail` column** is where (b) and (c) show up: `small×2 ok` is a
  same-rung retry, `small failed → mid ok` is an escalation.
- **A `?` on a total** means a reviewer's route reported no spend — expected if
  Copilot judged anything, wrong if only Claude Code ran.
- **`per-pool drain`** should name your pool. "no pool is connected for the
  agents this run used" means `connect` never ran or wrote somewhere else.

## When it does not go to plan

The two criteria that cannot be forced are **(b)** and **(c)** — you cannot make
a model fail on cue. If everything passes first try:

- Tighten the `lint` gate (add `-W clippy::pedantic`) to make (b) more likely.
- Make `parse-edges` harsher — more required error variants, an explicit "no
  `unwrap` anywhere" criterion — to make a frontier reviewer more likely to
  reject the first pass.

Re-running is cheap in effort and not in money, so change one lever at a time.

And if a criterion fires for a reason the plan did not intend — the run finds a
real defect in the engine rather than in the plan — that is a better outcome
than a clean pass. Write it down before fixing it.
