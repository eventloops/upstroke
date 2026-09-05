---
id: PR149-ARRANGED-OCCUPIED-REFUSAL-IN-SETTLE-KILL-TESTS
severity: P3
disposition: accepted-risk
category: correctness
pr: 149
reviewed_sha: 8a24eba08afff4b94b27af7fe4b59cf23695de30
location: src/engine/topology/settle/tests.rs:1557
provenance: introduced_by_feature
first_bad: 54ef9d5
guard: `scratch_with` in src/engine/topology/settle/tests.rs draws again on `Occupied` up to `SCRATCH_DRAWS`; a later pass labelling this P1 or P2 escalates to the owner, because the remedy past the bound is a different seed in `crate::ulid`, which is outside every session's reach
---

## Failure sequence

`scratch(label)` in the settle kill tests acquires `%TEMP%\upstroke-scratch-pr7h-<label>-<ULID>`
through `rundir::scratch_tree::acquire`, one exclusive `create_dir`, and draws again on `Occupied`
up to `SCRATCH_DRAWS` (3) times. The ULID is not unpredictable and not unique across processes:
`crate::ulid` is a millisecond timestamp and eighty bits of `splitmix64` seeded as
`now_ms ^ (pid << 32) ^ nonce.rotate_left(17)`, nothing cryptographic.

**Unarranged.** Two live harnesses draw one name when they draw the same tag in one millisecond
with seeds that coincide, which needs `nonce_a ≡ nonce_b (mod 2^15)` and
`pid_a ^ pid_b == (nonce_a ^ nonce_b) >> 15` — so a nonce of at least 2^15 in one of the two, the
nonce being the per-process count of ULIDs minted so far. `(pid 100, nonce 32768)` and
`(pid 101, nonce 0)` and `(100, 65536)` with `(102, 0)` are examples, not an enumeration —
`(100, 32769)` with `(101, 1)` collides too, and every pair the condition admits does; a Python
mirror of the construction confirms the examples collide and `(100, 32768)` against `(101, 1)`
does not. The first draw is
refused; the retry draws again at the next nonce, which meets the same partner only if it too drew
again in the same millisecond at the matching nonce. Past three draws the fixture panics naming
every refused root. One full run of the lib harness on Linux, every `ulid()` call printed by a measurement-only mutant, minted 6017 ULIDs and ended at nonce 6016, so a single harness does not reach the nonce the smallest pair needs; a long-lived conductor minting question and incarnation ids does.

**Arranged.** A launcher that knows the pid it is about to `exec` the harness with and the nonce
of the fixture's first draw computes the names of a future millisecond window for the nonces the
retry will draw, precreates them, and `exec`s the harness into the window: every draw is
`Occupied`, and the fixture panics before the settlement is tested — a red that names each root
and is unrelated to the product.

Reproduced on Linux, 2026-09-05, at `8636970`'s harness (before the retry): a Python mirror of
`crate::ulid`'s construction reproduces the harness's first draw exactly (in a PID namespace the
harness is pid 4 and the fixture's first draw is nonce 0: `ulid(1788573379703, 4, 0)` is the
observed `01M1QMFV3Q21R096DW2XZXBSDA`). Precreating
`upstroke-scratch-pr7h-reclaimed-<ULID(ms, 4, 0)>` for every millisecond of a five-second window
(5,001 names) and `exec`ing the witness into the window:

```
test engine::topology::settle::tests::a_kill_tests_scratch_tree_is_reclaimed_when_its_guard_drops ... FAILED
thread '...' (5) panicked at src/engine/topology/settle/tests.rs:1570:76:
a scratch tree: Occupied { root: ".../upstroke-scratch-pr7h-reclaimed-01M1QMTYV2C73CF2J59HX1WYHM" }
```

The same window computed for nonce 1 instead: `ok`. Against the retry, at the head this finding
was recorded at: the same window precreated for nonce 0 alone is `ok` (the second draw is nonce 1), and precreated for nonces 0, 1 and 2 is the bound's panic, `3 draws refused as occupied`, naming all three roots; both runs are in the pull request body.

**Substituted, which no draw sees (pass 5).** The allocator's ownership is a path: `ScratchTree`
holds the root's path and no handle or identity, and its reclaim removes by that path, as its own
documentation states. A same-user process that removes the freshly created root and renames a
stale tree containing `public/events.jsonl` into its place, between `acquire` and
`spawn_kill_child`, is not refused by anything: the child's `create_dir_all` accepts the
substitute, the child appends to the stale log, replay fails with `AlreadyStarted`, and the guard
then removes the substitute by pathname. The retry never sees `Occupied`. Nothing path-based
defends against a same-user process acting against the test between two of its steps, and no
fixture in this suite does; holding identity across the lifetime — a marker the acquisition writes
and the reclaim checks, or a handle on platforms that let one pin a directory — would be the
allocator's change, not this fixture's, and is named here as the remedy without being made.

## Why the residual is accepted rather than fixed

The retry closes the unarranged case to the depth of coincidence it can reach: a second draw meets
the same partner only through a second same-millisecond, seed-equal draw, and the bound reports
rather than spins. What remains is a launcher that precreates every nonce the retry will draw, one that substitutes
the tree after the draw, or a coincidence three deep; the first and last are reds that name their
own cause, the second is the stale-log replay failure with the substitute reclaimed by path — the pid-keyed fixture this
replaced reused a leftover silently and failed two layers away. Closing it entirely means a
different seed in `crate::ulid`, which is a production change outside this pull request and every
sweep session; the production consequence of the same arithmetic is recorded separately as
`PR149-ULID-SEED-COLLIDES-ACROSS-CONCURRENT-PROCESSES`.

What this family leaves behind, which is the population a collision needs: a tree whose parent
aborted and ran no `Drop`, or a tree whose removal the filesystem refused — on the normal path
that refusal is a panic and the tree stays, while unwinding it is reported and the tree stays. A
leftover can be drawn again only by a process at the same pid and nonce in the millisecond it was
made in, which is a clock revisit; between two *live* processes the millisecond is not the only
input that moves, and the arithmetic above is the whole condition.

## What the change that takes this up should do

If a later pass labels this P1 or P2, escalate to the owner rather than re-defer. The fixture's
side is bounded and witnessed (`an_occupied_draw_is_drawn_again`,
`the_draws_are_bounded_and_every_refused_root_is_named`,
`an_undecidable_refusal_is_not_drawn_again`); what is left is the seed.
