---
id: PR149-ARRANGED-OCCUPIED-REFUSAL-IN-SETTLE-KILL-TESTS
severity: P3
disposition: accepted-risk
category: correctness
pr: 149
reviewed_sha: 86369700d519aef8918fdb25be40695840da89e5
location: src/engine/topology/settle/tests.rs:1569
provenance: introduced_by_feature
first_bad: 54ef9d5
guard: `scratch` in src/engine/topology/settle/tests.rs; a later pass labelling this P1 or P2 escalates to the owner, because the remedy would be a retry in the fixture or a change to `crate::ulid`, and the second is outside every session's reach
---

## Failure sequence

`scratch(label)` in the settle kill tests acquires `%TEMP%\upstroke-scratch-pr7h-<label>-<ULID>`
through `rundir::scratch_tree::acquire`, one exclusive `create_dir`. The ULID is not unpredictable:
`crate::ulid` is a millisecond timestamp and eighty bits of `splitmix64` seeded as
`now_ms ^ (pid << 32) ^ nonce.rotate_left(17)`, nothing cryptographic. A launcher that knows the
pid it is about to `exec` the harness with and the nonce of the fixture's first draw can compute
the names of a future millisecond window, precreate them, and `exec` the harness into that window:
`create_dir` returns `AlreadyExists`, `acquire` returns `Occupied`, and `scratch(...).expect(...)`
panics before the settlement is tested — a red that names the root and is unrelated to the product.

Reproduced on Linux, 2026-09-05, at `8636970`'s harness: a Python mirror of `crate::ulid`'s
construction reproduces the harness's first draw exactly (in a PID namespace the harness is pid 4
and the fixture's first draw is nonce 0: `ulid(1788573379703, 4, 0)` is the observed
`01M1QMFV3Q21R096DW2XZXBSDA`). Precreating `upstroke-scratch-pr7h-reclaimed-<ULID(ms, 4, 0)>` for
every millisecond of a five-second window (5,001 names) and `exec`ing the witness into the window:

```
test engine::topology::settle::tests::a_kill_tests_scratch_tree_is_reclaimed_when_its_guard_drops ... FAILED
thread '...' (5) panicked at src/engine/topology/settle/tests.rs:1570:76:
a scratch tree: Occupied { root: ".../upstroke-scratch-pr7h-reclaimed-01M1QMTYV2C73CF2J59HX1WYHM" }
```

The same window computed for nonce 1 instead: `ok`. The red names the root and the refusal; it
never reads a leftover's content and never adopts a neighbour's directory.

## Why it is accepted rather than fixed

The refusal needs the arranged name, and the argument for that is not a platform's. **The ULID's
first component is the absolute epoch millisecond of the draw, so a directory left by any earlier
run carries an earlier millisecond and cannot be redrawn — on any platform, whatever the pid,
however the operating system recycles pids, and whether or not the winguest image carries
leftovers.** The pid-keyed fixture this replaced collided across runs *because* its name had no
time in it; this one's name has the time in it first. The one cross-run path left is a clock that
revisits a past millisecond with the same pid and nonce; the CI guest takes its clock from the
host. Within a run, two draws differ by nonce; across two live harnesses, a collision needs one
millisecond and one of the seed-equal pid-and-nonce pairs.

A Linux check that the implementation matches the argument, not the argument itself: with the pid
pinned by a PID namespace and the nonce sequence identical run to run, the millisecond was the only
input that moved, and 100 serial runs of the witness and both kill tests drew 300 roots with 300
distinct names, 100 distinct milliseconds per label, and 0 refusals, `strace` at the exclusive
`mkdir` recording every draw. A leftover of this family is only an aborted parent's — the guard
reclaims on return and on unwind.

A bounded retry on `Occupied` would guard the arranged case only. It cannot be witnessed from this
file — the name's parts are the process's own and `acquire_named` is private — and the arranged
case is a red that says what happened, which is the shape a refusal should have. The pid-keyed
fixture this replaced reused a leftover silently and failed two layers away; the trade is a silent
false result for a loud, arrangeable one.

## What the change that takes this up should do

If a later pass labels this P1 or P2, escalate to the owner rather than re-defer: the remedy is
either a retry in the fixture, which would be added machinery guarding a case no run reaches on
its own, or a different ULID seed in `crate::ulid`, which no sweep session may change. Whoever
takes it up should first re-run the pinned-pid measurement above against the head of the day.
