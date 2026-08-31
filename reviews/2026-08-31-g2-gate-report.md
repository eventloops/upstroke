# 2026-08-31 — G2 gate report (artifact 1)

This is **artifact 1** of the G2 checkpoint's evidence, the one
`decisions/2026-08-25-checkpoint-merges.md` names. It is written against the
E5-extended input range, adopted for this promotion by the owner's direct
promotion amendment of 2026-08-31.

Every derived fact below was re-measured on this host at the exact
pre-assembly baseline. Where a number came from an earlier baseline it is
re-derived and the delta is stated. The private packet records are read but
never reproduced: this report cites them only by stable internal reference
(`cumulative_review_gates.gates[G2].<field>`).

- **Pre-assembly baseline:** `50ed8c86ec60164011bfd393066c4c3696d3865b` — **not a cut candidate**; see "What `50ed8c86` is" below
- **Branch:** `promotion/g2-candidate-assembly`
- **`master` before the promotion:** `76b6a784ae5562ac044d6ff9a15b68397bd9b0e0`
- **Range:** `76b6a784..50ed8c86` — **418 commits, 66 first-parent units**

**Revision, 2026-08-31 (fourth).** The PR #80 review returned CHANGES_REQUIRED
with four validated blockers, and all four were right. Two land on this file.
**(2)** This report called `50ed8c86` the *candidate*, which reverses the
ordering `decisions/2026-08-25-checkpoint-merges.md` fixes: the gate passes and
the eight artifacts exist **before** a candidate is cut. `50ed8c86` is the
**pre-assembly baseline**; no candidate has been cut. **(4)** The verdict said
panel membership was unsettled while the same head adopted the three ratified
seats — a current artifact cannot assert both. Both are corrected below. The
other two blockers land on `reviews/FINDINGS.md` §38 and
`reviews/2026-08-31-g2-first-parent-coverage.md`.

**Revision, 2026-08-31 (fifth).** The follow-up review of `ada79bd7` returned
three P1 findings, and all three are right. The one that lands on this file:
the candidate label still stood on `50ed8c86` and `50a84acd` in passages the
fourth revision did not sweep — the checkpoint-order blocker recurring through
terminology. Every remaining current-status use of "candidate" for either sha
in this report now reads **pre-assembly baseline** (`50ed8c86`) or **committed
evidence head** (`50a84acd`); quotations and the future sense ("the candidate
will be the integration landing head", "before a candidate is cut") are
unchanged. The artifact table below remains the **sole enumerator** of the
owed captured set; revisions and decision records refer to that table rather
than restating its members. The other two findings land on
`decisions/2026-08-31-g2-checkpoint-promotion.md` (fourth addendum) and
`reviews/FINDINGS.md` §39.

## What `50ed8c86` is

`50ed8c86` is the **green post-PR79 baseline this evidence is measured against**.
It is not a candidate and this report does not treat it as one.

`decisions/2026-08-25-checkpoint-merges.md` orders the sequence: the G2 gate
passes on the cleaned tree, the eight artifacts exist, the full-ledger audit is
done — **and then** the candidate is cut. That ordering is controlling and is
**not narrowed here**. The evidence in this file, the coverage map, and the
ledger audit are all *inputs* to that gate, produced on the baseline.

**The candidate will be the integration landing head** — the head
`codex/parallelism-design` carries once this evidence branch lands under the
standing per-head ceremony and the outstanding artifacts and gates are complete.
It does not exist yet, and nothing in this repository should be read as saying it
does. The earlier framing — "the candidate is cut at `50ed8c86`" — was wrong in a
way that mattered: the ledger audit itself was written *after* `50ed8c86`, so
that commit could not have carried it.

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

**Revision, 2026-08-31 (third).** The owner's ruling 1 of 2026-08-31
(`decisions/2026-08-31-inertness-premise-behavioural.md`) found §4 below
**understated**: it said a library consumer can *name* the schema-4 vocabulary,
when in fact a consumer can **write** schema-4 durable state through the checked
funnel, using public API only and with no write-side activation check. Binding
amendment 1b corrects that sentence, and it is corrected in place here, recorded
the same way the serialized-run revision above is recorded. The finding is also
carried as a ledger row — `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED`,
`reviews/FINDINGS.md` §37 — under binding amendment 1a. **The promotion premise
does not move**: inertness is behavioural, it holds at this head, and no
visibility change to the code is authorized. §3's const-assertion count is
corrected from three to four in the same pass.

## Verdict

**The G2 gate does not pass at this baseline.** The serialized suite has now
run green at the committed evidence head
`50a84acd3ebf5f0ecffc35a7a5b4ea68960310f9`, including the Docker-gated tests
against a live daemon, and that discharges more than assembly could. It does not
discharge the gate, for two reasons that are not fixable by another Linux run:

1. **The artifacts are captured evidence, not merely passing oracles.** Six of
   the eight ask for a table, a transcript, a log, a diff or a histogram. Their
   oracles executed and passed; the artifacts themselves were not captured. A
   passing test is the ground the artifact stands on, not the artifact.
2. **The run is Linux-only.** macOS and Windows are hosted evidence. This
   repository carries two rows — `PR5-MACOS-CLIPPY-NEVER-RUN` and
   `PR7-WIN-READ-RACING-BOUND-TOO-SHORT` — that exist precisely because a
   Linux-only green closed a platform question falsely.

3. **The candidate was cut before capture.** That violated the ordered
   precondition and is recorded, not normalized away, in `reviews/FINDINGS.md`
   §42. This capture closes the missing-form defect; it does not waive the
   sequencing breach.

And the gate's own pass rule requires a completed review with no open
critical/high finding. **The panel's membership is settled. Round one ran at
`47dc9a35`, returned OpenAI BLOCK, Anthropic PASS, and a non-conforming Google
BLOCK/non-vote, and is therefore invalid as a panel.** The three seats are
ratified by the owner in
`decisions/2026-08-31-panel-seats.md` — `gpt-5.6-sol` at `max` via `codex exec`
with login judged by output text; `claude-fable-5` at `max` with `--model`
pinned on every invocation; `gemini-3.1-pro-high` via `/home/ubuntu/.local/bin/agy`
by absolute path with `--model` pinned, counting only on `status: SUCCESS` plus
the explicit verdict marker. For round two the owner authorized two additional
Google invocation repairs; one successful dry probe has been consumed and one
remains. All three seats must reconvene fresh and blind at the advanced head;
no test result substitutes for that panel.

Nothing in this report is a claim that a reviewer reread the promotion diff.

## The eight required artifacts

The gate's `required_artifacts` list has eight entries. Each is named below by
its index and by a public-safe description of what it asks for.

States are deliberately narrow. **Produced** means the artifact exists.
**Oracles passed** means every in-suite test the artifact rests on executed and
passed at this head. **Produced** additionally means the named table, log,
transcript, diff or histogram exists in the hash-pinned capture directory.
**Owed** means neither.

| # | What the artifact is | State | What the run substantiates, and what it does not |
|---|---|---|---|
| 1 | The gate report | **Produced** | This file |
| 2 | Host/container parity outputs | **Produced (Linux)** | `02-host-container-parity.txt`, sha256 `ad31058f5a7f8b8ad7ffd04cc5990abff2b2355b5632990355159bf42d6f938c`; live Docker 29.7.2 plus every executed host/container parity and real-Docker oracle. macOS and Windows are the exact-head hosted capture below, not invented local output |
| 3 | Fault-injection evidence table for the G2 sites — event kill and error-return points, the sync-prefix barrier refusal cases, id-unread points, and residue-class evidence (synthetic per element, plus a sampling record with its observed-class histogram) | **Produced (Linux)** | `03-fault-injection-evidence-table.txt`, sha256 `db581bd932a8d956fb58d15a72c3d87bf24a4a8dc191b887ab210935965d3bca`, contains all 70 site rows, named oracle outcomes, and both emitted observed-class histograms; the standalone histogram hashes are pinned in the manifest |
| 4 | Ref, worktree, snapshot, object, container and run-directory inventory before/after, with the husk census table | **Produced (Linux)** | Normalized before/after inventories have identical sha256 `048271a07907c4d86f5ab16057de906c38e9df7072a10c5f9224e5dc20e3fe65`; their diff is empty. `04-inventory-husk-census.txt`, sha256 `a1fdf7a8cfdcbaa208553b1e4f5751b35cb1c376d4634817db2670fa5baa524f`, records group/residue tables and every husk/creator-error oracle |
| 5 | User-checkout inventory diff | **Produced (Linux)** | `05-user-checkout-before.txt` and `05-user-checkout-after.txt` are byte-identical, sha256 `c33b71398e6e2112efe4696ed6ffcee297559a646f0d6c2acf9641dc9bd86c0f`; `05-user-checkout-inventory.diff` is the empty diff, sha256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| 6 | Docker-gated suite result with the environment noted | **Produced (Linux)** | This is the one artifact the run yields directly. `rc=0`, fresh compile, lib 1801 passed / 0 failed / 34 ignored, main 8 passed / 0 failed, example 0 tests; the `real_docker_*` tests exercised a live **Docker server 29.7.2**. Environment noted below. **Not covered:** macOS and Windows |
| 7 | `clippy.toml`, `effects/allowlist.toml`, wrapper classification, `effect_sites.json`, allow-placement scan output | **Produced (Linux)** | Inputs remain pinned below. `07-allow-placement-scan.txt`, sha256 `dfe7dbbde61c8690fe4de5579c87da3bc5a32a4249d8d468bb552633d1ebf308`, is the standalone scan output and includes its four input hashes |
| 8 | Runner identity outputs — run-started/run-resumed runner records, owner-record and intent digests, the per-invocation boundary and image-id log from the fake runners, and the inspection-refusal and probe-refusal transcripts | **Produced (Linux)** | `08-runner-identity-refusal-transcripts.txt`, sha256 `1e16c27f6e520b84e4c88312a4bebad287fece83f5e08ed237b80b685b828485`, records the executed identity, boundary, digest and refusal witnesses from the instrumented suite |

### Capture directory and manifest

All side outputs live under
`/home/ubuntu/tactus-artifacts/promotion/g2-captures/`. `SHA256SUMS` is the
complete selected-file manifest and hashes to
`96b9fec94b9bc0d40c069190ac332930c1d2f4e8749befaea3d4b36a775fcc55`.
The capture anchor is `47dc9a35f6e6af59160ece49570d9934a4450dec`, tree
`7f7b03017997cca503f2c28822d75d4f622d731e`. The capture commit changes only
this gate report, the coverage-map addendum, and the append-only ledger row; it
therefore leaves every tested source and configuration byte unchanged. The
standalone latest-disposition projection is
`10-ledger-projection-capture.txt`, sha256
`a2ea0cd89e59c2e162d16caf5701a57dc38de592e34835bca83f3d801ab5ec8d`.

### Artifact 7 — the inputs, pinned

Present at the capture anchor `47dc9a35`, sha256 truncated to 16 hex characters:

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

Run by root through the globally serialized build wrapper at exact capture
anchor `47dc9a35f6e6af59160ece49570d9934a4450dec` with `--nocapture` so the
machine-varying residue histograms and named oracle outcomes were durable:

```
RUST_TEST_NOCAPTURE=1 /home/ubuntu/bin/upstroke-build \
  cargo test --all-targets --all-features -- --nocapture
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

The complete log is `06-serialized-full-suite.log`, sha256
`462170d58b1e800322d23bc99b1b830f2b0bfef33c57b73655394f87ce05f055`.

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

**Discharged:** artifacts 2 through 8 are now captured on Linux, including the
standalone forms rather than only their oracles. Artifact 1 is this report.

**Strengthened, not newly claimed:** the inertness proofs below were already
structural — verified by reading the tree, not by running it. The green suite
adds that their guards execute and hold, including
`max_parallel_above_one_is_refused_rather_than_read_past`
(`src/config.rs:2949`) and the compile-time schema assertions, which a fresh
compile necessarily evaluated.

**Not discharged, and not dischargeable by another Linux run:**

1. **macOS and Windows.** Hosted evidence, not produced here. The two platform
   rows named in the verdict are the standing reason a Linux green is not a
   substitute.
2. **The panel.** The gate's pass rule requires questions answered and no open
   critical/high finding, and the checkpoint record requires a three-model panel
   to attest the code and the audited ledger together. Round one at `47dc9a35`
   is durable but invalid because Google was a non-vote; OpenAI also returned
   two High blockers. This capture closes those named evidence defects, but a
   fresh blind three-seat round over the advanced head remains required.

**Consequently the captured-artifact obligation is discharged, but the G2 gate
does not pass until the fresh panel returns three conforming votes with no open
Critical or High finding.** The sequencing breach from cutting the candidate
first remains recorded in `reviews/FINDINGS.md` §42; this capture does not waive
or erase it.

## Inert by default — verified, not assumed

The checkpoint record's condition is that inertness is *verified*. These five are
verified by construction at `50ed8c86`. Each is a fact a reader can re-check
from the paths named.

### 1. The legacy v0.1 path is unchanged

`RunState::apply` — the legacy replay fold, and the whole of how v0.1 derives
state from `events.jsonl` — is **byte-identical** between `master` and the
baseline:

| Side | Location | sha256 of the function region |
|---|---|---|
| `76b6a784` (master) | `src/events.rs:1069-1271` | `f5e8f1d632681b39a8b9d7c1d4b13c4dff9b04b3194da65e484b0af6a282b29d` |
| `50ed8c86` (baseline) | `src/events/mod.rs:1165-1367` | `f5e8f1d632681b39a8b9d7c1d4b13c4dff9b04b3194da65e484b0af6a282b29d` |

The file moved (`src/events.rs` → `src/events/mod.rs`) as part of the module
split; the function did not change. Hashed rather than line-counted, because a
one-character edit does not move a line count.

`events::SCHEMA_VERSION` is `3` on both sides — `src/events.rs:52` on master,
`src/events/mod.rs:64` on the baseline.

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
- **Four** compile-time assertions pin it (`:98-101`): `matches!(TOPOLOGY_ACTIVATION, TopologyActivation::Inactive)`, `MAX_READABLE_SCHEMA == LATEST_LEGACY_SCHEMA`, `MAX_READABLE_SCHEMA == 3`, and `TOPOLOGY_SCHEMA == LATEST_LEGACY_SCHEMA + 1`. They are evaluated in the ordinary build — the one `src/main.rs` links — so flipping activation without revisiting them fails the build rather than shipping quietly. *(Corrected from "three, `:99-101`" in the first revision of this report; the activation assertion at `:98` was missed.)*
- `fresh_writer_schema(selector)` (`:128`) maps `WriterSelector::Production` to 3 and `WriterSelector::TopologyPreview` to 4.

**`WriterSelector::TopologyPreview` has no construction site outside
`src/topology/schema.rs`.** Every occurrence in the tree is that file's own
definition, its match arm, or its own unit tests. `fresh_writer_schema` likewise
has no caller outside that file. No shipped command reaches either.

**The scope of this heading is exactly "shipped command".** It is not a claim
that schema-4 state cannot be written at all: a *library consumer* can write it
through the public funnel, which §4 states and `reviews/FINDINGS.md` §37 carries
as `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED`. Read the two together.

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

**A library consumer can do more than name the schema-4 vocabulary: it can
durably WRITE schema-4 state through the checked funnel, using public API only,
and no write-side activation check stands in the way.** The path is three
explicit topology choices — construct `RunStarted4 { schema: TOPOLOGY_SCHEMA, … }`
(25 fields, all `pub`, no `#[non_exhaustive]`, `src/topology/events.rs:600`),
check it with `TopologyLine::round_trip` (`src/events/log.rs:1242`), then open
the funnel with `EventLog::open` (`:466`) and commit with
`append_topology(site_for(&body), …)` (`:796`, `:1064`). `append_topology`
delegates straight to `append_topology_hooked` (`:809`) and applies no ceiling
test; `TOPOLOGY_ACTIVATION` and `MAX_READABLE_SCHEMA` appear **nowhere** in
`src/events/log.rs`. **Activation gates reading, not writing.** The log so
produced is state this same binary's resume refuses by name,
`SchemaRefusal::TopologyLogUnreadable`.

*This sentence replaces "a library consumer can name the schema-4 vocabulary",
which was understated. Owner ruling 1, binding amendment 1b, 2026-08-31. The
finding is carried as `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED` in
`reviews/FINDINGS.md` §37.*

What is true, and what the condition actually needs, is **behavioural
inertness**: the machinery is reachable, and nothing the binary ships drives it.
That is what §3 above establishes — activation `Inactive`, the read ceiling
pinned to 3 by four compile-time assertions, the only schema-4 writer selector
unconstructed outside its own module, production's sole `run_started` mint
stamping schema 3 (`src/engine/coordinator.rs:164`), the topology coordinator
`pub(crate)` with no non-test callers (`src/engine/mod.rs:61`), and every upgrade
into schema 4 refused.

**And the stronger guarantee is unachievable, not merely unmet.** Visibility
cannot deliver it: the legacy funnel already accepts any `pub u32` in
`RunStarted.schema` (`src/events/mod.rs:315`), and plain `std::fs` binds no
downstream crate at all. Log bytes are untrusted input, and this code has always
treated them so. The property the project actually builds and pins is *"a
schema-4 log cannot get itself read"* — refused loudly, precisely, and without
misfolding — and that property holds.

Recording this as "the surface remains `pub(crate)`" would have been a false
statement about the tree, and a later reader checking it would find `pub mod
topology` and be entitled to distrust the rest of the report.

### 5. This promotion authorizes no `0.2.0` tag

`Cargo.toml` is `version = "0.1.0"` on both `76b6a784` and `50ed8c86`. The only
tag in the repository is `v0.1.0`. `CHANGELOG.md` carries an `## Unreleased`
section and no `0.2.0` heading. Nothing in this promotion creates, moves, or
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
already incorporated in the baseline, by merge `50ed8c86`. Nothing about
master-forward remains outstanding.

### CI evidence at `50ed8c86`

Bridge-triggered, at the baseline `50ed8c86`:

| Run | What | Result |
|---|---|---|
| `33384344044` | PR #18 `synchronize` | **green** on all Linux / macOS / Windows lint, test and MSRV leaves, and on the aggregate |
| `33384340392` | integration push | **green** on all Linux / macOS / Windows lint, test and MSRV leaves, and on the aggregate |
| `33384344035` | policy | **green** |

**Pre-bridge CI is not cited as platform evidence anywhere in this record**, and
must not be: the bridge changed the head, and a run that concluded before it
speaks about a different commit.

### Cross-platform capture at `47dc9a35`

The capture anchor's hosted run `33434846757` completed successfully with every
Linux, macOS and Windows test, lint and Rust-1.85 MSRV leaf plus the aggregate
green. Policy run `33434846790` also completed successfully. The complete CI
metadata and logs are captured as `09-hosted-ci-33434846757.json` (sha256
`9aeca30f30f6564d330ffeaee7c4b665c0e028b0be54beb8f5594940f3d6f524`),
`09-hosted-ci-33434846757.log` (sha256
`c86dc1f734a09a844b0cf504debf008dfd7923fd1d626c0d0f6f2beeef9803fe`),
and `09-hosted-policy-33434846790.json` (sha256
`579b5c67706059a52d1a2fc963cb620ea2de8f1dd9dba8713ae4932caff86206`).
These are the cross-platform capture; the local instrumented forms remain
Linux-specific. The capture commit must receive its own hosted exact-head green
board before round two convenes.

**Custody of this claim.** These three results are recorded as supplied by root.
`gh` on this host is unauthenticated, so assembly could not re-query them; the
run identifiers are recorded exactly so a later reader can. That is an
attribution, not an independent verification, and it is written that way on
purpose.

## Cross-references

- `decisions/2026-08-31-g2-checkpoint-promotion.md` — the obligation-by-obligation reconciliation
- `reviews/2026-08-31-g2-first-parent-coverage.md` — the review coverage map for `76b6a784..50ed8c86`
- `reviews/FINDINGS.md` §§35, 38 and 39 — the complete checkpoint full-ledger audit and the recurrence-class review; §35 is the initial projection, §38 supplies the rows it omitted, and §39 supplies the reproducible corrected projection
- `reviews/FINDINGS.md` §37 — `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED`, the carried row for the public write path
- `decisions/2026-08-31-inertness-premise-behavioural.md` — the ruling that corrected §4
- `decisions/2026-08-31-panel-seats.md` — the three ratified panel seats
