# 2026-08-31 — G2 gate report (artifact 1)

This is **artifact 1** of the G2 checkpoint candidate's evidence, the one
`decisions/2026-08-25-checkpoint-merges.md` names. It is written against the
E5-extended input range, adopted for this candidate by the owner's direct
promotion amendment of 2026-08-31.

Every derived fact below was re-measured on this host at the exact candidate
head. Where a number came from an earlier baseline it is re-derived and the
delta is stated. The private packet records are read but never reproduced: this
report cites them only by stable internal reference
(`cumulative_review_gates.gates[G2].<field>`).

- **Candidate head:** `50ed8c86ec60164011bfd393066c4c3696d3865b`
- **Branch:** `promotion/g2-candidate-assembly`
- **`master` before the promotion:** `76b6a784ae5562ac044d6ff9a15b68397bd9b0e0`
- **Range:** `76b6a784..50ed8c86` — **418 commits, 66 first-parent units**

## Verdict

**The G2 gate does not pass at this candidate.** Two of the eight required
artifacts are producible on this host and are produced or pinned here; six
require execution this host cannot perform, and they are recorded as owed rather
than asserted.

Nothing in this report is a claim that a reviewer reread the candidate diff.

## The eight required artifacts

The gate's `required_artifacts` list has eight entries. Each is named below by
its index and by a public-safe description of what it asks for.

| # | What the artifact is | State | Evidence, or what it is owed to |
|---|---|---|---|
| 1 | The gate report | **Produced** | This file |
| 2 | Host/container parity outputs | **Owed** | Requires the Docker-gated suite. Not runnable as part of assembly; see "Docker" below |
| 3 | Fault-injection evidence table for the G2 sites — event kill and error-return points, the sync-prefix barrier refusal cases, id-unread points, and residue-class evidence (synthetic per element, plus a sampling record with its observed-class histogram) | **Owed** | The globally serialized full suite, which root runs after this commit. The residue sampler's known scheduling hazard is `PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE`, repaired in PR7 |
| 4 | Ref, worktree, snapshot, object, container and run-directory inventory before/after, with the husk census table | **Owed** | The serialized full suite plus the Docker-gated suite |
| 5 | User-checkout inventory diff | **Owed** | The serialized full suite |
| 6 | Docker-gated suite result with the environment noted | **Owed** | A Docker run. Deterministic container names collide across concurrent worktrees on one daemon, so this must be run serially or it reports a defect that is not there |
| 7 | `clippy.toml`, `effects/allowlist.toml`, wrapper classification, `effect_sites.json`, allow-placement scan output | **Inputs pinned; result owed** | All five inputs exist at this head and are hash-pinned below. The allow-placement scan is `every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist` (`src/effects/tests.rs:507`); its *passing* is owed to the serialized suite |
| 8 | Runner identity outputs — run-started/run-resumed runner records, owner-record and intent digests, the per-invocation boundary and image-id log from the fake runners, and the inspection-refusal and probe-refusal transcripts | **Owed** | The serialized full suite plus the Docker-gated suite |

### Artifact 7 — the inputs, pinned

Present at `50ed8c86`, sha256 truncated to 16 hex characters:

| Path | sha256 (first 16) | bytes |
|---|---|---|
| `clippy.toml` | `9c92654bfe30631e` | 18976 |
| `effects/allowlist.toml` | `4fc0aaaaf009bc29` | 38538 |
| `effects/wrappers.toml` | `4a7db53dc1df076c` | 27029 |
| `effects/funnel-modules.json` | `6237938830247a1f` | 1293 |
| `effects/residue-classes.json` | `511be7dee737ddf0` | 4464 |
| `effect_sites.json` | `ab9edaad67abcfc7` | 33547 |

`effect_sites.json` is an array of **70** sites, of which **14** are `run_dir`.
That is the same 70-total/14-`run_dir` census `reviews/FINDINGS.md` §31 records
as unchanged from its base, re-derived here rather than copied.

### Stop record — the six owed artifacts

This host cannot truthfully produce artifacts 2, 3, 4, 5, 6 and 8. The reasons
are specific, not general:

1. **The full suite is globally serialized and root runs it after this commit.**
   Assembly must not run it, so no artifact that depends on suite output can be
   asserted here.
2. **Docker-gated work needs a serial daemon.** Container names are
   deterministic and one daemon is shared across worktrees, so a concurrent run
   reports collisions as defects.
3. **The macOS and Windows evidence is hosted evidence.** A Linux-local result
   is not a substitute: `PR5-MACOS-CLIPPY-NEVER-RUN` and
   `PR7-WIN-READ-RACING-BOUND-TOO-SHORT` are both rows that exist because a
   Linux-only green closed a platform question falsely.

**Consequently the candidate is not gate-passed, and no part of this record
should be read as saying it is.**

## Inert by default — verified, not assumed

The checkpoint record's condition is that inertness is *verified*. These five are
verified by construction at `50ed8c86`. Each is a fact a reader can re-check
from the paths named.

### 1. The legacy v0.1 path is unchanged

`RunState::apply` — the legacy replay fold, and the whole of how v0.1 derives
state from `events.jsonl` — is **byte-identical** between `master` and the
candidate:

| Side | Location | sha256 of the function region |
|---|---|---|
| `76b6a784` (master) | `src/events.rs:1069-1271` | `f5e8f1d632681b39a8b9d7c1d4b13c4dff9b04b3194da65e484b0af6a282b29d` |
| `50ed8c86` (candidate) | `src/events/mod.rs:1165-1367` | `f5e8f1d632681b39a8b9d7c1d4b13c4dff9b04b3194da65e484b0af6a282b29d` |

The file moved (`src/events.rs` → `src/events/mod.rs`) as part of the module
split; the function did not change. Hashed rather than line-counted, because a
one-character edit does not move a line count.

`events::SCHEMA_VERSION` is `3` on both sides — `src/events.rs:52` on master,
`src/events/mod.rs:64` on the candidate.

### 2. `max_parallel` above 1 is refused until activation

`src/config.rs:371` — `pub const DEFAULT_MAX_PARALLEL: u32 = 1`, documented as
"the only value this engine accepts".

`src/config.rs:1445-1465` splits on `(EngineLimits, configured > 1)`:

- **`Fresh` and above 1 → a hard `Err`.** Raised in `parse_engine`, which runs
  *before* a lock, a workspace, or a run directory exists. The message names the
  configured value and tells the operator to set 1 or omit the key "until
  parallel execution ships".
- **`SequentialResume` and above 1 → a warning, and the value is clamped** to
  `DEFAULT_MAX_PARALLEL`. A run keeps the execution shape it started with; the
  warning says a fresh run refuses the value outright.
- **At or below 1 → the configured value passes through.**

The refusal is total over the two-by-two, with no default arm.

### 3. No shipped command writes schema 4

`src/topology/schema.rs` holds the whole switch:

- `LATEST_LEGACY_SCHEMA = 3` (`:44`), `TOPOLOGY_SCHEMA = LATEST_LEGACY_SCHEMA + 1` (`:48`).
- `TOPOLOGY_ACTIVATION: TopologyActivation = TopologyActivation::Inactive` (`:73`) — the single activation constant.
- `MAX_READABLE_SCHEMA = max_readable_schema(TOPOLOGY_ACTIVATION)` (`:89`), which is `3` while activation is `Inactive`.
- Three compile-time assertions pin it (`:99-101`): `MAX_READABLE_SCHEMA == LATEST_LEGACY_SCHEMA`, `MAX_READABLE_SCHEMA == 3`, `TOPOLOGY_SCHEMA == LATEST_LEGACY_SCHEMA + 1`. Flipping activation without revisiting them fails the build rather than shipping quietly.
- `fresh_writer_schema(selector)` (`:128`) maps `WriterSelector::Production` to 3 and `WriterSelector::TopologyPreview` to 4.

**`WriterSelector::TopologyPreview` has no construction site outside
`src/topology/schema.rs`.** Every occurrence in the tree is that file's own
definition, its match arm, or its own unit tests. `fresh_writer_schema` likewise
has no caller outside that file. No shipped command reaches either.

There is also **no migration into schema 4**: `check_upgrade_transition`
(`src/topology/schema.rs:282`) returns `SchemaRefusal::NoUpgradePath` for any
`to >= TOPOLOGY_SCHEMA`, on the stated ground that schemas 3-and-below and
schema 4 are different execution models rather than successive versions.
Independently, the legacy reader refuses a log whose effective schema exceeds
`SCHEMA_VERSION` (`src/events/mod.rs:1789`).

### 4. The schema-4 surface — stated precisely, because the obvious wording is wrong

**The schema-4 surface is not `pub(crate)`, and this report will not say that it
is.** `src/lib.rs:49` is `pub mod topology;`, and `src/topology/mod.rs` declares
`census`, `effects`, `events`, `fold`, `leases`, `paths`, `queue`, `registry`
and `schema` as `pub mod`. `TOPOLOGY_SCHEMA`, `MAX_READABLE_SCHEMA`,
`TOPOLOGY_ACTIVATION`, `WriterSelector` and `fresh_writer_schema` are all `pub`.
A library consumer can name the schema-4 vocabulary.

What is true, and what the condition actually needs, is **behavioural
inertness**: the vocabulary is reachable, and nothing the binary ships drives
it. That is what §3 above establishes — activation `Inactive`, the read ceiling
pinned to 3 by compile-time assertion, the only schema-4 writer selector
unconstructed outside its own module, and every upgrade into schema 4 refused.

Recording this as "the surface remains `pub(crate)`" would have been a false
statement about the tree, and a later reader checking it would find `pub mod
topology` and be entitled to distrust the rest of the report.

### 5. This promotion authorizes no `0.2.0` tag

`Cargo.toml` is `version = "0.1.0"` on both `76b6a784` and `50ed8c86`. The only
tag in the repository is `v0.1.0`. `CHANGELOG.md` carries an `## Unreleased`
section and no `0.2.0` heading. Nothing in this candidate creates, moves, or
authorizes a tag.

## Bridge and promotion evidence

### PR #78 — the last slice merge before the bridge

| Fact | Value |
|---|---|
| True merged head | `6e5bb33aacafda72c1a7ed883db77afd3bfd172c` |
| Merge commit | `0bd12cb7630813d5ebf080e3519fa54576f16d7b` |
| Merge parents | `82874ef70dd4acf074cbf1453e28651d78af4db3` and `6e5bb33aacafda72c1a7ed883db77afd3bfd172c` |
| Merge author | Cameron Lambert `<38257252+eventloops@users.noreply.github.com>` (org `eventloops`) |
| Merge committer | GitHub `<noreply@github.com>` |
| Merged at | 2026-08-31T08:42:11+01:00 |

`6e5bb33` is `docs(findings): record PR78 convergence repair`, whose parent is
the Fable convergence repair `6a217865f978b5319007c55c269fb48e26823dc3`
(`reviews/FINDINGS.md` §34). Post-merge checks were green.

### PR #79 — the master-forward bridge

| Fact | Value |
|---|---|
| Reviewed head | `3348ce8cbe5f38561afd3712748e335b98e261ea` |
| Merge commit | `50ed8c86ec60164011bfd393066c4c3696d3865b` |
| Merge parents | `0bd12cb7630813d5ebf080e3519fa54576f16d7b` and `3348ce8cbe5f38561afd3712748e335b98e261ea` |
| Merge author | Cameron Lambert `<38257252+eventloops@users.noreply.github.com>` (org `eventloops`) |
| Merged at | 2026-08-31T11:52:07+01:00 |
| Review | exact-head, **PASS** |

`3348ce8` is itself `Merge master into promotion candidate`, with parents
`0bd12cb` and `76b6a784` — which is how `master` reached the integration line.

**Bridge delta, measured:** `git diff --name-status 0bd12cb 50ed8c86` is exactly

```
A	docs/CNAME
```

one file, one insertion, no deletions. **docs/CNAME only**, as recorded.

**A property worth keeping:** the tree of `3348ce8` and the tree of `50ed8c86`
are *identical objects*. The exact-head review of `3348ce8` therefore reviewed
byte-for-byte the tree the merge published — the strongest form the review
invalidation scope admits, and not something that has to be argued from a small
diff.

This also closes the earlier preservation blocker: `master`'s `docs/CNAME` is
already incorporated in the candidate, by merge `50ed8c86`. Nothing about
master-forward remains outstanding.

### CI evidence at `50ed8c86`

Bridge-triggered, at the candidate head:

| Run | What | Result |
|---|---|---|
| `33384344044` | PR #18 `synchronize` | **green** on all Linux / macOS / Windows lint, test and MSRV leaves, and on the aggregate |
| `33384340392` | integration push | **green** on all Linux / macOS / Windows lint, test and MSRV leaves, and on the aggregate |
| `33384344035` | policy | **green** |

**Pre-bridge CI is not cited as platform evidence anywhere in this record**, and
must not be: the bridge changed the head, and a run that concluded before it
speaks about a different commit.

**Custody of this claim.** These three results are recorded as supplied by root.
`gh` on this host is unauthenticated, so assembly could not re-query them; the
run identifiers are recorded exactly so a later reader can. That is an
attribution, not an independent verification, and it is written that way on
purpose.

## Cross-references

- `decisions/2026-08-31-g2-checkpoint-promotion.md` — the obligation-by-obligation reconciliation
- `reviews/2026-08-31-g2-first-parent-coverage.md` — the review coverage map for `76b6a784..50ed8c86`
- `reviews/FINDINGS.md` §35 — the checkpoint full-ledger audit and the recurrence-class review
