---
id: SWEEP-HOST-NAMING-004
severity: P2
disposition: deferred     # parked for an owner ruling; see "Status" below
category: correctness
pr: 185
reviewed_sha: e518bb782b97d3bac952b4fece737ee1b6de7d8d
location: src/runner/host/naming.rs:221
provenance: fix_regression   # raised against the pass-1 repair in b9c73630
first_bad: SWEEP-HOST-NAMING-001
guard: `a_candidate_this_platform_cannot_stat_is_never_reported_as_absence` pins the exhausted-search half only
---

## Status

**Parked, owner attention required.** This is a pass-2 blocker on a task whose two review passes
are spent, and it is a design question the implementor is not entitled to settle by overruling a
reviewer. Nothing here is merged; PR #185 is left open at `e518bb78` with the branch retained.

## Failure sequence

A candidate's `stat` returns a non-`NotFound` error (`EACCES`, `ELOOP`, `EIO`, a dead mount) but a
later `PATH` candidate is executable. `resolve_program`'s `Err` arm records the first such error
and continues; the later `Ok(true)` returns that executable, so `HostRunner::program_for` receives
`Ok(path)` and the runner spawns it. The undetermined candidate is never reported, and no caller
learns that one existed.

## The disagreement, stated fairly

Pass 1 (`SWEEP-HOST-NAMING-001`) required, in its own words: "Treat only NotFound as a
non-candidate; propagate every other error as a typed UpstrokeError carrying the metadata
operation and candidate path." The repair in `b9c73630` propagates such an error only when the
search then matches nothing. Pass 2 reads that as not discharging the finding, and on the letter
of pass 1's correction it is right.

The repair was deliberate and is argued in
`docs/internals/runner/host/naming.md`, PR #185's body and the row 42 sweep record. The argument
is platform parity: this function's stated contract is *which file a spawn of that name would
reach*, and the platform walks past an unreadable entry.

**Measured on this box (Linux, non-root, 2026-09-06)**, with `blocked/` at mode 000 holding
`upstroke-probe`, a second copy in `good/`, and `PATH=blocked:good:$PATH`:

- `stat("blocked/upstroke-probe")` → `PermissionError 13` (EACCES), i.e. not `NotFound`;
- `sh -c 'command -v upstroke-probe; upstroke-probe'` → resolves and runs `good/upstroke-probe`;
- a direct `execvp("upstroke-probe")` → runs `good/upstroke-probe`, child exit status 0.

So under pass 2's prescribed remedy the runner would refuse a program that the shell and `execvp`
both run, and a single mode-0700 directory anywhere on the coordinator's own `PATH` would fail
every agent spawn. That is the cost the current shape avoids.

**The strongest form of the reviewer's point**, which the implementor accepts as real: falling
through silently means an undetermined *earlier* candidate can change which installation is
chosen, and nothing anywhere records that it happened. The platform has the same behaviour, but
the platform is not making a certification the way this runner is.

## What the change that takes this up should do

The owner rules between three shapes:

1. **Pass 2's remedy as written** — return `UpstrokeError::Filesystem` at the first non-`NotFound`
   error. Discharges both passes literally; accepts that an unreadable `PATH` entry fails every
   spawn, which the measurement above shows diverges from the platform.
2. **Keep the current fall-through** — absence is claimed only when every miss was `NotFound`,
   which is what §7's sentence actually requires; record that pass 2's reading was considered and
   overruled on platform-parity grounds, in the notes and the sweep row.
3. **Fall through, but observably** — return the resolved path *and* surface the undetermined
   candidate (a warning, a diagnostic, or a runner event), so the choice matches the platform and
   nothing is silently dropped. This is the shape that satisfies both readings, and it needs a
   design sentence about where that observation goes, which is why it is the owner's and not this
   sweep's.

Whichever is chosen, `standards/SWEEP.md` row 42 and
`docs/internals/runner/host/naming.md` must be corrected to state it, and a regression case with
an erroneous first candidate and a valid later one should pin the chosen behaviour.
