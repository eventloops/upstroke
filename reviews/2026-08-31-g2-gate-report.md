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

**Revision, 2026-08-31 (second).** This report was first committed at
`50a84acd3ebf5f0ecffc35a7a5b4ea68960310f9` with all six execution-dependent
artifacts recorded as owed. Root has since run the globally serialized suite at
that exact head. The verdict, the artifact table and the owed-work section below
are **revised in place** to record that result — a gate report that still said
"not run" after the run would be the stale artifact this file exists to prevent.
What changed is confined to those three places plus the new "The serialized gate
run" section; the inertness proofs, the bridge evidence and the CI evidence are
untouched. Nothing is removed, and no artifact is upgraded past what the
executing tests substantiate.

## Verdict

**The G2 gate does not pass at this candidate.** The serialized suite has now
run green at the exact candidate head, including the Docker-gated tests against
a live daemon, and that discharges more than assembly could. It does not
discharge the gate, for two reasons that are not fixable by another Linux run:

1. **The artifacts are captured evidence, not merely passing oracles.** Six of
   the eight ask for a table, a transcript, a log, a diff or a histogram. Their
   oracles executed and passed; the artifacts themselves were not captured. A
   passing test is the ground the artifact stands on, not the artifact.
2. **The run is Linux-only.** macOS and Windows are hosted evidence. This
   repository carries two rows — `PR5-MACOS-CLIPPY-NEVER-RUN` and
   `PR7-WIN-READ-RACING-BOUND-TOO-SHORT` — that exist precisely because a
   Linux-only green closed a platform question falsely.

And the gate's own pass rule requires a completed review with no open
critical/high finding. **No panel has been convened**: the three-model panel of
`decisions/2026-08-25-checkpoint-merges.md` has not run, and its membership is
not settled. That blocker is untouched by any test result.

Nothing in this report is a claim that a reviewer reread the candidate diff.

## The eight required artifacts

The gate's `required_artifacts` list has eight entries. Each is named below by
its index and by a public-safe description of what it asks for.

States are deliberately narrow. **Produced** means the artifact exists.
**Oracles passed** means every in-suite test the artifact rests on executed and
passed at this head, but the artifact's captured form — the table, log,
transcript, diff or histogram it names — was not collected. **Owed** means
neither.

| # | What the artifact is | State | What the run substantiates, and what it does not |
|---|---|---|---|
| 1 | The gate report | **Produced** | This file |
| 2 | Host/container parity outputs | **Oracles passed (Linux)** | The parity oracles ran against the live daemon and passed — `real_docker_adapter_parsing_matches_the_host_table` (`src/runner/container/exec.rs:6653`) is the named host-versus-container comparison. **Not captured:** the parity *outputs*. **Not covered:** macOS and Windows |
| 3 | Fault-injection evidence table for the G2 sites — event kill and error-return points, the sync-prefix barrier refusal cases, id-unread points, and residue-class evidence (synthetic per element, plus a sampling record with its observed-class histogram) | **Oracles passed (Linux)** | The kill-point, error-return, sync-prefix-barrier, id-unread and residue-class tests are inside the green suite, and the kill children they re-invoke as subprocesses ran with them. **Not captured:** the evidence *table*, and the sampling record's **observed-class histogram** — the suite asserts the sampler's premise, it does not emit a histogram. The sampler's own scheduling hazard is `PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE`, repaired in PR7 |
| 4 | Ref, worktree, snapshot, object, container and run-directory inventory before/after, with the husk census table | **Oracles passed (Linux)** | The husk-census and inventory oracles passed, including the Docker-backed container reclaim tests. **Not captured:** the before/after inventory and the husk census table, per shape, with the creator-error cases |
| 5 | User-checkout inventory diff | **Oracles passed (Linux)** | The checkout-inventory oracles passed. **Not captured:** the diff itself |
| 6 | Docker-gated suite result with the environment noted | **Produced (Linux)** | This is the one artifact the run yields directly. `rc=0`, fresh compile, lib 1801 passed / 0 failed / 34 ignored, main 8 passed / 0 failed, example 0 tests; the `real_docker_*` tests exercised a live **Docker server 29.7.2**. Environment noted below. **Not covered:** macOS and Windows |
| 7 | `clippy.toml`, `effects/allowlist.toml`, wrapper classification, `effect_sites.json`, allow-placement scan output | **Inputs pinned; scan passed** | All five inputs exist at this head and are hash-pinned below, and the allow-placement scan `every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist` (`src/effects/tests.rs:507`) passed in the green suite, as did `every_allowlist_entry_carries_its_justification_and_names_a_real_file` (`:898`). **Not captured:** the scan's printed output as a standalone artifact |
| 8 | Runner identity outputs — run-started/run-resumed runner records, owner-record and intent digests, the per-invocation boundary and image-id log from the fake runners, and the inspection-refusal and probe-refusal transcripts | **Oracles passed (Linux)** | The runner-record, owner-record, intent-digest, image-id and refusal oracles passed. **Not captured:** the per-invocation boundary and image-id **log**, and the inspection-refusal and probe-refusal **transcripts**. These are the artifact, and the suite does not emit them |

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

## The serialized gate run

Run by root through the globally serialized build wrapper at the exact committed
candidate head `50a84acd3ebf5f0ecffc35a7a5b4ea68960310f9`:

```
/home/ubuntu/bin/upstroke-build cargo test --all-targets --all-features
```

| Fact | Value |
|---|---|
| Exit status | `rc=0` |
| Compile | fresh — not a cached-binary run |
| Library target | **1801 passed, 0 failed, 34 ignored** |
| Binary target (`main`) | **8 passed, 0 failed** |
| Example target | 0 tests |
| Docker | live daemon, **Docker server 29.7.2**; the `real_docker_*` tests used it and passed |
| Platform | Linux, this host only |

**On the 34 ignored.** They are `#[ignore]`-marked **subprocess entry points** —
`*_kill_child`, `*_helper`, `*_child` functions that a parent test re-invokes as
a child process — not skipped assertions. They ran, as children, under the
parents that spawned them. This report does not enumerate the 34 by name against
the run output, because root supplied counts rather than a list; the
characterisation is from the `#[ignore]` sites in the tree.

**Why serialization mattered.** Container names in this suite are deterministic
and one daemon is shared across worktrees, so a concurrent run reports
collisions as defects that are not there. This run was serialized, which is what
makes its Docker result readable.

### What the run does, and does not, discharge

**Discharged:** artifact 6 outright, on Linux. Artifact 7's scan result. The
oracles beneath artifacts 2, 3, 4, 5 and 8.

**Strengthened, not newly claimed:** the inertness proofs below were already
structural — verified by reading the tree, not by running it. The green suite
adds that their guards execute and hold, including
`max_parallel_above_one_is_refused_rather_than_read_past`
(`src/config.rs:2949`) and the compile-time schema assertions, which a fresh
compile necessarily evaluated.

**Not discharged, and not dischargeable by another Linux run:**

1. **The captured artifacts.** Six of the eight name a table, a log, a
   transcript, a diff or a histogram. Those were not collected, and this report
   does not claim them. An oracle passing is evidence *for* the artifact; it is
   not the artifact.
2. **macOS and Windows.** Hosted evidence, not produced here. The two platform
   rows named in the verdict are the standing reason a Linux green is not a
   substitute.
3. **The panel.** The gate's pass rule requires questions answered and no open
   critical/high finding, and the checkpoint record requires a three-model panel
   to attest the code and the audited ledger together. **No panel has been
   convened and its membership is not settled.** No test result touches this.

**Consequently the candidate is still not gate-passed, and no part of this
record should be read as saying it is.**

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
