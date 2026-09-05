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
guard: `scratch_with` retries `Occupied` up to `SCRATCH_DRAWS`, preserves every occupied root and reports all refused names; fixture availability against deliberate exhaustion is outside this bounded setup contract
---

## Failure sequence

A launcher computes the names a fixture will draw and precreates them. Every
exclusive acquisition returns `Occupied`. After three draws, the settle fixture
panics with each refused path before executing the settlement. This is an
explicit setup failure. It neither adopts nor deletes an occupied root.

The recorded Linux reproduction used the old XOR/splitmix generator at
`86369700d519aef8918fdb25be40695840da89e5`, before retries. In a PID namespace,
the launcher predicted the first draw at pid 4 and nonce 0 over a five-second
window and precreated 5,001 names. The reclaimed-tree witness failed on the
predicted occupied root. Precreating nonce 1 instead passed. At the reviewed
retry head, occupying nonce 0 alone passed on the second draw; occupying nonces
0, 1 and 2 reached the three-draw refusal. These historical runs remain in the
PR's diagnosis record. They are not measurements of the current generator.

The current generator hashes separately encoded timestamp, pid and nonce
fields. This removes the old cancelling seed pairs but makes no unpredictable
or collision-free naming promise. The deterministic retry witnesses establish
bounded behavior, not the frequency of collisions in live harnesses. The old
complete Linux library harness made 6,017 draws and ended at nonce 6,016, below
the old pair's threshold of 32,768 in one process. Concurrent processes do not
combine their counters.

An aborted parent can leave a tree because it runs no destructor. A filesystem
refusal during normal cleanup panics and leaves the tree; during unwinding,
cleanup reports the refusal without raising a second panic. These are possible
leftovers, not evidence that an ordinary current harness will draw their names.

## Scope of the accepted risk

The accepted residual is deliberate exhaustion of a finite allocation policy.
The fixture must preserve occupied roots, stop after a bounded number of draws
and refuse undecidable allocation. A longer bound cannot guarantee availability
against a launcher that occupies every attempted name.

The earlier version of this file also described pathname substitution and
production private-root adoption. Those are separate defects, not accepted by
this disposition. PR #149 adds original-directory identity checks at scratch
use and reclaim, plus exclusive fresh-run reservation. Their witnesses and
fixed dispositions remain in the PR ledger under
`PR149-REVIEW-12-EXCLUSIVE-CREATE-DOES-NOT-KEEP-THE-TREE` and
`PR149-ULID-SEED-COLLIDES-ACROSS-CONCURRENT-PROCESSES`. Identity checks retain
the documented mkdir-to-open and later check-to-use intervals.

## What the change that takes this up should do

If the product needs tests to remain available against deliberate setup
interference, give the harness an isolated temporary parent through the runner
and test that boundary. A different deterministic seed or a larger retry bound
does not establish that guarantee. Preserve the existing exclusive-allocation,
bounded-retry and undecidable-refusal witnesses.
