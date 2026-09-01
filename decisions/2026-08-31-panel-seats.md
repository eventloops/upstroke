# 2026-08-31 — the G2 panel's three seats

*Adopted by the owner on 2026-08-31 as ruling 2, **ratified as amended**. The
base text is the adjudication draft prepared by `promotion_decisions_fable`
(Fable 5), sha256 first-16 `300295070a093f28`; the owner's binding amendment 2a
replaces the draft's fallback section below and is marked. The owner's signature
is what `2026-08-20-automated-review-gate.md` §8 reserves, and it has been
given. The end-to-end seat verification reported below is the dossier author's
measurement of 2026-08-31, carried here as ratified; this record does not
re-verify liveness, and every seat re-probes at convening regardless.*

**Verdict.** The three-model panel that
`2026-08-25-checkpoint-merges.md` requires convenes as three manual CLI
invocations on the merge candidate, read by a human (the §9 interim
mechanism, unchanged), with one seat per model family available on the box:
**OpenAI `gpt-5.6-sol` at `max` via `codex exec`; Anthropic `claude-fable-5`
at `max` via the claude CLI; Google `gemini-3.1-pro-high` via `agy`** (that
family's ceiling — agy exposes no `max`). Every seat was verified end to end
on 2026-08-31 — authentication, a live round-trip to the intended model, and
a complete structured answer — and every seat re-probes at convening time.
Seats run after the assembly push (a push moves PR #18's head), on the same
frozen inputs, blind to one another; the owner reads the three verdicts
together.

## Why

- **One seat per family is the independence the panel exists for.** Three
  families are reachable from this box; allocating them one seat each is the
  maximum genuinely distinct perspective available. Sol is the continuity
  seat — the reviewer of record for the whole slice series. Gemini is the
  fresh seat — a family never used on this repository, unanchored on its
  review history. The Anthropic seat is the **weakest independence claim and
  is recorded as such**: the slice series was implemented by
  `claude-opus-5`, so the seat shares a vendor with the authorship. It is
  mitigated — a different, stronger model than the authoring one, review-only
  invocation — not eliminated. The alternative, a two-family panel, is
  strictly less independent.
- **Effort is `max` where `max` exists, and the family ceiling where it does
  not.** Measured on this box: codex "ultra" is `max` plus task delegation,
  not deeper reasoning; the priority fast tier showed no review benefit
  (2026-08-19). agy's Google catalogue tops out at `gemini-3.1-pro-high`;
  the asymmetry is a fact of the tooling, noted rather than hidden.
- **Each seat carries its verified trap and its guard.** codex's login state
  is judged by output text, never exit code (it exits 0 logged out). The
  claude seat pins `--model claude-fable-5` because the box default is
  `claude-opus-5` at `xhigh` and an unpinned seat silently degrades to the
  implementation model. agy is invoked by absolute path
  (`/home/ubuntu/.local/bin/agy` — non-interactive shells never gain
  `~/.local/bin`), pins `--model` because its catalogue also serves Claude
  and GPT-OSS models, and its output counts only if the envelope reads
  `status: SUCCESS` **and** the verdict marker is present — an empty or
  truncated response is a non-vote, re-run, never an APPROVE.

## Conditions

- Convening follows the assembly push and the PR body publish, on a stable
  head; any subsequent push invalidates the seats' verdicts
  (`2026-08-20-review-invalidation-scope.md`).
- Each seat re-verifies end to end at convening; the 2026-08-31 verification
  establishes liveness on that date only.
- The invocation line of every seat is recorded with its verdict, so the
  model and effort attested are the ones that ran.
- No box-side aggregation or adjudication logic; the owner reads three raw
  verdicts (§9's scaffolding rule).

## The three seats, exactly

Recorded to the letter, because a seat that ran as something other than this is
not the seat the owner ratified.

| Seat | Family | Model | Effort | Invocation guard that makes the seat valid |
|---|---|---|---|---|
| 1 | OpenAI | `gpt-5.6-sol` | `max` | Invoked via `codex exec`. **Login state is judged by output text, never by exit code** — `codex login status` prints "Not logged in" and exits `0`, so a bare `$?` reads a logged-out seat as ready |
| 2 | Anthropic | `claude-fable-5` | `max` | **`--model claude-fable-5` pinned explicitly on every invocation.** The box default is `claude-opus-5`, which is the implementation model; an unpinned seat silently degrades to it and **is invalid** |
| 3 | Google | `gemini-3.1-pro-high` | family ceiling (`agy` exposes no `max`) | Invoked by **absolute path `/home/ubuntu/.local/bin/agy`** — non-interactive shells never gain `~/.local/bin` — with **`--model gemini-3.1-pro-high` pinned**, because the same catalogue also serves Claude and GPT-OSS models. **A verdict counts only on `status: SUCCESS` *and* the explicit verdict marker.** Silence, truncation, or a missing marker is a **non-vote**: re-run, never an `APPROVE` |

## If a seat is unavailable — amendment 2a (binding)

**No fallback is pre-authorized.** The draft's in-family substitutions are
withdrawn by the owner's amendment and are not available.

- On any seat failure at convening: **one repair attempt, then wait for the
  owner.**
- **The panel does not convene partially.** Two families is not a quorum.
- A degraded two-family panel, an Opus-for-Fable substitution, or **any** other
  seat change **requires a separate explicit owner act** — asked for directly,
  with the exact failure evidence attached.

This is stricter than the draft deliberately. A fallback chosen under time
pressure by whoever is convening is the silent substitution the seat guards
above exist to prevent, one level up.

## Convening discipline (binding)

- The seats convene **only after** the assembly branch has landed and the PR #18
  body has been rewritten, on a **stable exact head**.
- Each seat is **re-probed end to end at convening**; the 2026-08-31
  verification establishes liveness on that date only.
- The three run **blind to one another, with no shared transcript**.
- The three **raw verdicts reach the owner unedited**, as durable artifacts with
  paths. No box-side aggregation or adjudication.
- **Any candidate head movement after a seat has run invalidates that seat's
  verdict**, and the panel re-runs on the new head
  (`2026-08-20-review-invalidation-scope.md`).
- The invocation line of every seat is recorded with its verdict, so the model
  and effort attested are the ones that ran.

## Rejected

- **Three seats from fewer families** (e.g. two OpenAI models): sacrifices
  the only independence the panel can offer for redundancy it does not need.
- **Routing any seat through agy's multi-vendor catalogue for convenience**:
  one CLI, one family — a mis-set default sampling a second family from the
  same binary is precisely the silent substitution this decision exists to
  prevent.
- **An automated panel driver that aggregates verdicts**: §9 classes the
  box-side machinery as scaffolding that must stay dumb; adjudication in code
  is stage-2 engine work.

## Cross-references

- `2026-08-25-checkpoint-merges.md` — the panel obligation this fills.
- `2026-08-20-automated-review-gate.md` §8–§9 — the reserved decision and the
  interim mechanism.
- `2026-08-23-retire-app-attestation.md` — the owner's merge remains the
  attestation; the panel informs it and replaces nothing.
- `2026-08-31-inertness-premise-behavioural.md` — the premise the panel's
  inertness confirmation is pointed at; it reviews the behavioural claims, not
  the retired visibility claim.
- `2026-08-31-g2-checkpoint-promotion.md` — obligation 4, which this panel is.
