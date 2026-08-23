# The v0.1 acceptance run

DESIGN.md §21's definition of done, staged. **This spends real quota** — it is
operator-triggered, never launched by an agent.

> **This run has happened.** It passed on 2026-08-10 and shipped as `0.1.0`.
> [`RESULT.md`](RESULT.md) is the write-up: which run demonstrated each
> criterion, the ledger lines, and the three engine defects this run found —
> plus the fourth that the first real-library run turned up afterwards. What
> follows is still the run book — keep it working, because §21's criteria are
> the regression suite for the engine's behaviour, not a one-off ceremony.
>
> It has been run that way since: [`RESULT-2026-08-11.md`](RESULT-2026-08-11.md)
> is the first re-run, against the post-review engine. One file per run, so the
> certification above stays about the run that earned it.

The two files beside this one go into a *scratch* repository, not into upstroke:

- `plan.md` — five tasks, each shaped to provoke one §21 criterion
- `upstroke.toml` — the routing and gates that do the provoking

Each is commented with *why* its knobs are set the way they are. Read those
before changing anything; most of the values look arbitrary and are not.

## What has to be true first

```bash
claude auth status --json
```

Must report `"loggedIn": true`. Pre-flight refuses to start otherwise, and
logging in is a user-driven step upstroke never automates.

The Copilot CLI is optional. Without it you still satisfy §21 — but the ledger's
per-pool section only ever shows one pool, and the cross-vendor second opinion
stays commented out in the config.

## Set up the target repo

Not upstroke itself: agents editing the engine mid-run is a bad time, and §14
rolls a failed attempt back with `git clean -fd`.

```bash
cargo new --lib acceptance-target
```

```bash
cd acceptance-target && git add -A && git commit -m "seed"
```

Then copy `plan.md` and `upstroke.toml` in beside `Cargo.toml`. The working tree
must be clean — the engine refuses a dirty one.

## Rehearse at zero spend

Free, so do it until the table says exactly what you expect:

```bash
upstroke run plan.md --dry-run --budget 15
```

Check four things before spending anything:

1. **Five tasks, no cycles**, and `changelog` shows `deps -` (it must be
   independent, or §21(d)'s second half cannot happen).
2. **`readme` routes to `small` only**; `parse-edges` shows three rungs.
3. **`gates: check, lint, test [from config]`** — if it says `[derived]`, the
   config is not being read and `--config` is pointing somewhere wrong.
4. **The capacity block names your pools.** If it says `not connected`, run
   `upstroke connect` first.

## Run it

```bash
upstroke run plan.md --budget 15
```

### Which answer channel you get

This decides how §21(d) plays out, and it is not a config setting — it is
whether stdin is a terminal:

- **Attached to a terminal** → the engine prompts you on stdin when it hard-
  blocks. Simplest, but it is not the `upstroke answer` path §21 names.
- **Detached** (stdin redirected, CI, a background job) → the engine waits
  `wait_on_block_secs` for an answer to arrive as a file, which is what
  `upstroke answer` writes. This is the one to demonstrate.

The deterministic way to force the second, without fighting PowerShell's stdin
redirection:

```bash
upstroke run plan.md --budget 15 --interaction never
```

That parks the question and ends the run at **exit 2** instead of waiting. Then,
in the same terminal:

```bash
upstroke status
```

```bash
upstroke answer <question-id>
```

```bash
upstroke resume <run-id>
```

Which also exercises exit 2, the out-of-band answer, and resume — more surface
than sitting at a prompt would.

## The kill test

§21 ends with "kill the engine mid-run and `resume` finishes it". Do it while an
attempt is actually running — during a gate is ideal, since that is the longest
window:

```bash
upstroke resume <run-id>
```

What should happen, and what to check in `upstroke status` afterwards: the
interrupted attempt appears in the ledger with **unknown cost** (`—`, not
`$0.0000`), its rung's allowance is **not** spent, any half-written files were
discarded, and the task completes on the retry.

## Reading the result

```bash
upstroke status <run-id>
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
- Make `parse-edges` genuinely harder with another boundary whose expected
  result is explicit — for example, accept exactly `u64::MAX` and return the
  overflow variant for the next value.

Do not add an arbitrary syntax ban merely to manufacture a review failure. If
no model fails, record the criterion as not demonstrated rather than replacing
observable behaviour with a judgement call.

Re-running is cheap in effort and not in money, so change one lever at a time.

And if a criterion fires for a reason the plan did not intend — the run finds a
real defect in the engine rather than in the plan — that is a better outcome
than a clean pass. Write it down before fixing it.
