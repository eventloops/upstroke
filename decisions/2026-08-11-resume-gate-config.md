# Decision record — gate config across a resume

**Date:** 2026-08-11
**Status:** Decided. The *finding* stands as written; the *remedy* recorded here (refuse on mismatch) was **replaced before it shipped** by taking the gates from the record — see the addendum below, which is the standing decision.
**Provenance:** originally Addendum C of `2026-08-11-design-council.md`; split into its own record on 2026-08-11 when one-decision-per-file became the folder convention (`README.md`). Content unchanged below the line.
**Related:** [design council](2026-08-11-design-council.md) (the review that prompted the check), [self-hosting v0.2](2026-08-11-self-hosting-v02.md) (why it matters now: the workspace an implementer edits contains the gates that verify it).

---

## Finding

**Live runs are snapshot-safe by construction:** config is parsed at pre-flight into the analysis and gates execute from memory, so a mid-run edit to `upstroke.toml` cannot change a live run's gates.

**Resume has a real gap, confirmed in code:** `resume` re-resolves gates from today's config "exactly as a fresh run does" (`src/engine.rs`, resume path) — unlike the plan hash and the routing chains, which are recorded-and-refused on mismatch. `run_started` records only gate *names*, so a `[[gates]]` command edited between a run and its resume — including by an implementer agent that edited the workspace's own `upstroke.toml` before an interruption — is silently adopted; a name-preserving command change (`cmd = "cargo test"` → `cmd = "true"`) is invisible even to a name comparison. The codebase already articulates the governing distinction (budgets are deliberately re-derived because they are an operator ceiling, not identity; chains are refused because they are identity) — gate commands are *verification identity* and belong on the refused side.

## Verdict as recorded (superseded in part)

**Fix (flagged for a separate implement session):** record gate commands (or a fingerprint) in `run_started`; resume refuses on mismatch with the chains-refusal phrasing; logs predating the record warn and re-derive, like the pre-step-9 review record; DESIGN.md §15's refusal list gains gates alongside the plan hash and chains.

**Config hardening before the first self-hosted run:** blast-radius override in this repo's `upstroke.toml` — `paths = ["upstroke.toml"]` with `second_opinion = "different-vendor"` — so any diff touching gate definitions gets cross-family eyes.

## Rejected options

- **Hard-denying `upstroke.toml` edits to implementers.** Self-hosting tasks legitimately touch config, and gate commands execute repository scripts anyway (§21's runner rationale); review plus a resume-side control is the proportionate pair.

## Addendum (2026-08-11) — the remedy, revised: resume takes the gates from the record

**Verdict: `run_started` records each effective gate in full — name, command, shell, timeout — and a resume rebuilds and runs *those*.** A config that differs today produces a warning naming each difference, not a refusal. This replaces the "refuse on mismatch" remedy above, which was implemented ([PR #4](https://github.com/eventloops/upstroke/pull/4)) and then withdrawn before it shipped.

**What changed our mind.** A max-effort review of the refusal implementation (10 finder angles, adversarial verification with executed probes) returned ~25 confirmed defects, and the severe ones were not edge cases in the comparison — they were consequences of comparing at all:

- **The self-hosting case it was built for became unresumable.** A gate edit committed by the run's own gate-passed, cross-family-reviewed task refuses, and both remedies the message offered are self-defeating: restoring the config uncommitted passes the check and is then destroyed by §14's own residue discard (reported back to the operator as "uncommitted path(s) left by the interrupted run"), while committing the restore trips the HEAD check. Verified by probe.
- **The verdict depended on which branch the operator was standing on**, since config is read at pre-flight, before the branch switch. The same run, same instant, refused from `master` and resumed cleanly from the run branch.
- **It refused over crash residue that resume itself discards** eleven lines later, failing identically on every retry until someone ran `git checkout -- upstroke.toml` by hand.
- **It broke §15's orphan-commit adoption promise**, refusing before the adoption block could run.
- Plus: derived gates (no `[[gates]]`) treated as config identity, so a committed `Cargo.toml` refused the run forever with a remedy naming a config that never existed; refusal ahead of the already-completed check; refusal with zero tasks committed, breaking the one-command budget-stop recovery; `resolve_programs` preempting the refusal with a message telling the operator to *install the tampered tool*; and a message reporting edits nobody made whenever gate names repeated, which `[[gates]]` permits.

**Why taking the record is better, not merely different.** It is *stronger*: refusing detects a weakened gate and stops the run; taking the record means the weakened gate never governs anything and the run continues. It matches what the codebase already does twice — the review plan and `private_dir` are read from the record for the same stated reason, "a fact about the run, not about today's machine" — and what a live run does by construction, which is this record's own opening finding: config is parsed once into the analysis, so a mid-run edit cannot change a live run's gates. Honouring that snapshot across an interruption is what makes a resume the same run rather than a new one wearing its branch.

**What it costs, stated plainly.** An operator who wants different gates on an existing run cannot have them: they start a new run, and the warning says so. That is the same constraint a live run has, now honest instead of silently re-derived. Whether that cost is ever worth buying back is the deferred question below.

**Also revised:** `shell` and `timeout` are recorded. The first implementation omitted `shell` on portability grounds the review found unsupported (DESIGN.md §15 claims no such portability, §21 recommends running the whole conductor under WSL, and `private_dir` records an absolute host path, so a Windows-started/WSL-resumed run is already impossible). Shell is half of what a command means: `cmd = "true"` — the finding's own example of a weakened gate — always passes under `sh` and is not a program at all under `cmd.exe`.

**The pre-record population is closed too.** A log written before the record existed has nowhere to carry it, so its first resume must re-derive — and if that resume records nothing, so must every resume after it, leaving a gate weakened between two of them silently adopted. That resume now writes what it settled on into its own `run_resumed`, and every resume after it is an ordinary record-bearing one.

**The hardening half of the original verdict, now landed:** the blast-radius override this record prescribed did not exist, because the repo had no `upstroke.toml` at all — so a diff touching gate definitions would have gone to ordinary same-family review. Added in the same change, with gates matching CI. The "proportionate pair" now reads: cross-family review of config diffs, plus the record governing what a resume verifies against.

## Verified live (2026-08-11, after merge)

Run **`01KZSCV96PPHVNR8ATPF3C80A2`**, WSL-side: two tasks, the engine killed with `SIGKILL` mid-attempt on the second, the `check` gate then weakened from `cargo check --all-targets` to `false`, then resumed.

- **The recorded gate ran, not the weakened one.** The task committed, which `false` makes impossible, and the gate log for that attempt carries cargo's own output. The resume said so in the same breath: *"`check` runs `cargo check --all-targets` and today's config says `false`. Start a new run to adopt them."*
- **The interruption was settled as §15 promises.** `attempt_interrupted` for `bytes` attempt 1 with `cost_usd: null` and `usage: null` — recorded with unknown spend, and the run continued rather than counting it against the rung.
- **The one-home rule held.** `run_resumed.gates` is `null`, because `run_started.gate_cmds` already answered the question; only a pre-record log's first resume writes that field.
- **The record carries all four fields**, `shell: "sh"` and `timeout_ms` included, so the gate is reproducible rather than merely named.
- **The residue discard behaved exactly as the review predicted**: the uncommitted weakening was reported and destroyed — `discarded: [" M upstroke.toml"]` — which is why an uncommitted config edit could never have been the way to change a run's gates anyway.

Both tasks committed; `$0.2258` reported with the Codex review halves unpriced.

## Deferred candidate — adopting new gates mid-run

Once the record governs a resume, an operator cannot change what verifies a run without starting a new one. **That is the design working, not a gap** — a review finding claiming otherwise was withdrawn, because the example did not survive being taken apart: a typo that always *fails* commits nothing, so a new run costs almost nothing, and a typo that always *passes* commits work that a new run is the correct answer to.

What remains is narrower: a gate that was legitimately fine and needs *adjusting* after real commits — plausibly a `timeout_secs` too tight for a slower attempt — where the operator must redo honest work to widen a limit. An opt-in `upstroke resume --adopt-gates`, recording the switch as an event so the ledger can say which tasks were verified against which standard, is the obvious shape.

**Deliberately not built**, under this project's own evidence rule: the scenario has not occurred here, the shape above is a guess rather than a measured requirement, and the first real occurrence will say more about what it should be than any amount of reasoning now. Revisit when it bites.
