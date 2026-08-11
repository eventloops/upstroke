# Decision record — gate config across a resume

**Date:** 2026-08-11
**Status:** Decided, and **the prescribed fix is under active revision** — see "Revision in flight" below. The *finding* stands; the *remedy* recorded here (refuse on mismatch) is being replaced by taking the gates from the record.
**Provenance:** originally Addendum C of `2026-08-11-design-council.md`; split into its own record on 2026-08-11 when one-decision-per-file became the folder convention (`README.md`). Content unchanged below the line.
**Related:** [design council](2026-08-11-design-council.md) (the review that prompted the check), [self-hosting v0.2](2026-08-11-self-hosting-v02.md) (why it matters now: the workspace an implementer edits contains the gates that verify it).

---

## Finding

**Live runs are snapshot-safe by construction:** config is parsed at pre-flight into the analysis and gates execute from memory, so a mid-run edit to `tactus.toml` cannot change a live run's gates.

**Resume has a real gap, confirmed in code:** `resume` re-resolves gates from today's config "exactly as a fresh run does" (`src/engine.rs`, resume path) — unlike the plan hash and the routing chains, which are recorded-and-refused on mismatch. `run_started` records only gate *names*, so a `[[gates]]` command edited between a run and its resume — including by an implementer agent that edited the workspace's own `tactus.toml` before an interruption — is silently adopted; a name-preserving command change (`cmd = "cargo test"` → `cmd = "true"`) is invisible even to a name comparison. The codebase already articulates the governing distinction (budgets are deliberately re-derived because they are an operator ceiling, not identity; chains are refused because they are identity) — gate commands are *verification identity* and belong on the refused side.

## Verdict as recorded (superseded in part)

**Fix (flagged for a separate implement session):** record gate commands (or a fingerprint) in `run_started`; resume refuses on mismatch with the chains-refusal phrasing; logs predating the record warn and re-derive, like the pre-step-9 review record; DESIGN.md §15's refusal list gains gates alongside the plan hash and chains.

**Config hardening before the first self-hosted run:** blast-radius override in this repo's `tactus.toml` — `paths = ["tactus.toml"]` with `second_opinion = "different-vendor"` — so any diff touching gate definitions gets cross-family eyes.

## Rejected options

- **Hard-denying `tactus.toml` edits to implementers.** Self-hosting tasks legitimately touch config, and gate commands execute repository scripts anyway (§21's runner rationale); review plus a resume-side control is the proportionate pair.

## Revision in flight (2026-08-11)

An implementation of the refusal remedy above ([PR #4](https://github.com/keybindings/tactus/pull/4)) was reviewed at max effort and the refusal design was found unsound — the severe defects were consequences of comparing configs at all, not edge cases in the comparison. The revised direction is that **resume rebuilds its gates from `run_started` and runs those**, with a warning naming any difference rather than a refusal, on the argument that it is strictly stronger: a weakened gate never governs anything instead of being detected after the fact, and it matches what the codebase already does for the review plan and `private_dir`.

Recorded here as *in flight, not decided*: that work is unmerged at the time of writing. When it lands, its own record supersedes this one's remedy — the finding above is unaffected either way.
