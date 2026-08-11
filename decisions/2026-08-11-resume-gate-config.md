# Decision record — gate config across a resume

**Date:** 2026-08-11
**Status:** Decided. The *finding* stands as written; the *remedy* recorded here (refuse on mismatch) was **replaced before it shipped** by taking the gates from the record — see the addendum below, which is the standing decision.
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

## Addendum (2026-08-11) — the remedy, revised: resume takes the gates from the record

**Verdict: `run_started` records each effective gate in full — name, command, shell, timeout — and a resume rebuilds and runs *those*.** A config that differs today produces a warning naming each difference, not a refusal. This replaces the "refuse on mismatch" remedy above, which was implemented ([PR #4](https://github.com/keybindings/tactus/pull/4)) and then withdrawn before it shipped.

**What changed our mind.** A max-effort review of the refusal implementation (10 finder angles, adversarial verification with executed probes) returned ~25 confirmed defects, and the severe ones were not edge cases in the comparison — they were consequences of comparing at all:

- **The self-hosting case it was built for became unresumable.** A gate edit committed by the run's own gate-passed, cross-family-reviewed task refuses, and both remedies the message offered are self-defeating: restoring the config uncommitted passes the check and is then destroyed by §14's own residue discard (reported back to the operator as "uncommitted path(s) left by the interrupted run"), while committing the restore trips the HEAD check. Verified by probe.
- **The verdict depended on which branch the operator was standing on**, since config is read at pre-flight, before the branch switch. The same run, same instant, refused from `master` and resumed cleanly from the run branch.
- **It refused over crash residue that resume itself discards** eleven lines later, failing identically on every retry until someone ran `git checkout -- tactus.toml` by hand.
- **It broke §15's orphan-commit adoption promise**, refusing before the adoption block could run.
- Plus: derived gates (no `[[gates]]`) treated as config identity, so a committed `Cargo.toml` refused the run forever with a remedy naming a config that never existed; refusal ahead of the already-completed check; refusal with zero tasks committed, breaking the one-command budget-stop recovery; `resolve_programs` preempting the refusal with a message telling the operator to *install the tampered tool*; and a message reporting edits nobody made whenever gate names repeated, which `[[gates]]` permits.

**Why taking the record is better, not merely different.** It is *stronger*: refusing detects a weakened gate and stops the run; taking the record means the weakened gate never governs anything and the run continues. It matches what the codebase already does twice — the review plan and `private_dir` are read from the record for the same stated reason, "a fact about the run, not about today's machine" — and what a live run does by construction, which is this record's own opening finding: config is parsed once into the analysis, so a mid-run edit cannot change a live run's gates. Honouring that snapshot across an interruption is what makes a resume the same run rather than a new one wearing its branch.

**What it costs, stated plainly.** An operator who genuinely needs different gates — a typo'd command, a gate that cannot pass — must start a new run rather than resume; the warning says so. That is the same constraint a live run has, now honest instead of silently re-derived.

**Also revised:** `shell` and `timeout` are recorded. The first implementation omitted `shell` on portability grounds the review found unsupported (DESIGN.md §15 claims no such portability, §21 recommends running the whole conductor under WSL, and `private_dir` records an absolute host path, so a Windows-started/WSL-resumed run is already impossible). Shell is half of what a command means: `cmd = "true"` — the finding's own example of a weakened gate — always passes under `sh` and is not a program at all under `cmd.exe`.

**The pre-record population is closed too.** A log written before the record existed has nowhere to carry it, so its first resume must re-derive — and if that resume records nothing, so must every resume after it, leaving a gate weakened between two of them silently adopted. That resume now writes what it settled on into its own `run_resumed`, and every resume after it is an ordinary record-bearing one.

**The hardening half of the original verdict, now landed:** the blast-radius override this record prescribed did not exist, because the repo had no `tactus.toml` at all — so a diff touching gate definitions would have gone to ordinary same-family review. Added in the same change, with gates matching CI. The "proportionate pair" now reads: cross-family review of config diffs, plus the record governing what a resume verifies against.
