# 2026-09-01 — the Windows test leg runs on a self-hosted ephemeral runner

**Decision.** `test (windows-latest)` retires. The Windows suite runs in a new
`test-windows` job on `runs-on: [self-hosted, windows, winguest]`: an ephemeral
Windows Server 2025 guest on the build box, booted for each job from a throwaway
qcow2 overlay of a frozen golden image and registered for that one job with a
single-use just-in-time runner config. `lint (windows)` and the Windows MSRV leg
stay on `windows-latest`, and `lint (windows)` gains a `cargo build
--all-targets` step, so GitHub's runner still compiles, code-generates and
links every `#[cfg(windows)]` body on current stable, the library, the binaries
and their test harnesses, in the dev profile. `cargo test` builds the test
profile, which inherits from dev and is identical to it until the manifest says
otherwise, and that compile now happens on the guest with the image's
toolchain; so what moves is execution and the test-profile compile with it, and
a `[profile.test]` override that splits the two is a manifest edit in the diff.
The new job does not merely run the suite: it counts what
libtest reported and fails below a floor, so the one leg that left GitHub's
runners is the one leg that says it ran. `upstroke-ci` requires it like every
other.

## Why

The Windows test job was the whole CI wall clock, and its duration was not a
property of the suite. Measured on 2026-09-01 (all numbers are the lib harness's
own `finished in` time for the same 1792 tests):

- On `windows-latest`, thirteen runs of near-identical code the same afternoon
  took 535, 557, 613, 617, 623, 644, 652, 668, 684, 732, 974, 987 and 1154 s —
  a median of ~650 s inside a job of ~12.5 min, with a tail near 21 min. No
  code change explains the spread; it is the host the runner lands on.
- The suite is intrinsically ~2.4× slower on Windows than on Linux (227 s
  against ~96 s at the same 4-CPU/4-thread shape) because two modules,
  `engine::tests` and `workspace_manager::tests`, spawn `git`, create worktrees
  and write objects in bulk. GitHub's runner then multiplies that by ~2.9× with
  a 2× spread on top.
- Per-run levers do not reach it. Eight libtest threads bought 18% on the
  guest at the CI shape and could not be read at all on the hosted runner
  (one sample, 600 s, inside the noise); excluding the checkout from Defender's
  real-time scan was applied and changed nothing (680 s, at the median). Both
  were measured as pull requests #91 and #92 and #92 is closed as negative.
- The guest runs the same suite in 101 s at 16 vCPU (93 s at 32), with ±1%
  run-to-run variance, and the proof-of-concept job (#95) took 2 min 25 s end
  to end: checkout, an incremental build against the image's warm target, the
  harness. CI's wall clock becomes the macOS leg.

## What is measured and what is assumed

Measured: every number above; the ephemeral revert (destroy, fresh overlay,
boot to ssh) at 14.3 s and 14.6 s with a marker file gone both times; the
runner registering with a just-in-time config, taking the job two seconds after
the pull request opened, deregistering itself on exit and leaving nothing on
the host. Assumed, and therefore owed as obligations below: that the golden
image is re-curated when the toolchain should move (its `stable` is whatever
was current at curation, where `windows-latest` installs the latest on every
run; until then the hosted `cargo build --all-targets` step is what
code-generates and links the tree on current stable); that the host's own build
load, which
shares the same cores, does not
push the job past its 20-minute timeout; that the loop that recycles the guest
is kept running.

## The trust boundary

Self-hosted runners on a public repository are a known hazard because a pull
request from a fork can execute its workflow on the host. What bounds it here:

- The repository's fork policy is already `all_external_contributors`: no
  workflow from an outside contributor runs until the owner approves it.
- Every job runs in a guest destroyed afterwards, on an overlay deleted with it;
  the golden base is never booted while an overlay exists.
- The registration is per job and single-use, minted by the host loop; the
  workflow's token stays `contents: read` and the repository holds no secrets
  the job could reach.
- GitHub's runner is still the witness that the Windows tree compiles clean,
  code-generates and links on current stable (`cargo build --all-targets` in
  `lint (windows)`, binaries and test harnesses both, in the dev profile), and
  holds the MSRV floor. What moves to owner-controlled
  hardware is test execution, and the owner's merge was already the attestation
  ([2026-08-23](2026-08-23-retire-app-attestation.md)).

## Rejected

- **Sharding the hosted leg** (two or three `windows-latest` shards via
  `cargo-nextest` partitions): the only hosted option that shrinks the tail as
  well as the median, but each shard is still a draw from the same lottery, it
  triples the job count against the concurrency limit, and it is a larger
  contract change for a smaller result (~6–7 min typical).
- **A larger paid runner**: dedicated cores would likely cut the variance, but
  the per-test cost of Azure's disk is unmeasured, and it costs money to find
  out.
- **Threads or Defender on the hosted runner**: measured; see above.
- **Status quo**: 12.5 minutes typical, 21 in the tail, for every push.

## Contract

The workflow-shape oracle (`src/effects/tests/ci_model.rs`,
`src/effects/tests/workflow.rs`) pins the new job the way it pins the others:
the labels as an exact set, the test command character for character, the
platform-default shell on every `run:` step, a field set that admits no `if:` or
`continue-on-error:`, and the aggregate's `needs`, environment and loop derived
from the job graph. Every step of every modelled job is either an action
pinned to a reviewed commit or a script pinned character for character (the
identity script and the suite on the test jobs; Clippy, the witness, the
formatter and the four shell gates on the gate jobs; the locked check on the
MSRV leg), and every checkout step takes no inputs: an unpinned step, or an
input on the checkout, can put another tree in front of the pinned command,
and `test-windows` would test `master` while every other pin still held. Three
further knobs are pinned for the same reason, each of which produced a green
aggregate over a suite that never ran the candidate: the toolchain action's
`toolchain:` input (`stable` on every leg but the MSRV floor, which pins the
manifest's), because the action's commit does not decide which compiler it
installs; `defaults.run.working-directory`, refused outright, because a job
default directory points the pinned command at another crate; and the
workflow's whole `env:` mapping as an equality, because a Cargo target runner
bound there builds every Windows harness and executes none, and a guard per
name can only refuse the names somebody thought of. The `permissions:` mapping
is pinned the same way: the field's presence was checked, its value was not,
and a token widened to `write-all` reaches every build script and test the
candidate ships with the checkout's credential still configured, which no guest
teardown recalls.

Pinning the action by commit says which code runs; three further pins say what
it is told to do and when. Each action's `with:` inputs are an allowlist by key
*and* by value, because `Swatinem/rust-cache` accepts a `cmd-format` input and
wraps the commands it runs in it, and the toolchain action builds shell text
from `components` and interpolates it into a Bash line — so one input on an
allowlisted action at a pinned commit puts a `git checkout` of another tree in
front of every gate in the job, by either route.
The toolchain install must be the step after the checkout, because the action
makes its toolchain the rustup default and anything above it runs whatever the
runner image preinstalled -- during a release rollout, the previous stable,
which is exactly what the current-stable witness exists to rule out. And the self-hosted leg counts
the tests it ran. That is the one structural point in this contract. Cargo can
be asked for this suite and execute none of it: a `target.<triple>.runner`
bound in a repository `.cargo/config.toml`, in `$CARGO_HOME`, in any directory
above the checkout or in the process environment hands every compiled harness
to a wrapper that exits zero, and a `[workspace]` in the root manifest whose
`default-members` name another crate builds no harness of this one at all.
Three of those five are written where nothing reading this repository can see
them; the fifth has as many spellings as TOML has whitespace; and the list
closes only if Cargo stops growing. Two drafts of this change tried to refuse
the routes, one shell step per machine, and the review found a route past each
draft. So the leg checks the effect instead: `cargo test` on the guest is
followed, in the same pinned step, by a sum of the `test result: ok.` counts
libtest printed and a floor of 1700 — at `cb8ac1f` it reports 1771 there, 1761
from the lib harness and 10 from the bin one. A wrapper that executes nothing
and prints nothing leaves no such line, whichever route installed it, and that
is every misconfiguration and every lazy substitution. It is not every
substitution: a wrapper written to print libtest's summary line clears the
floor, and so does a decoy crate whose `harness = false` main prints it. Both
are a forgery, on the machine or in the diff, and the next paragraph is where
forgeries are bounded. One equality over one script, in the class every pin
here belongs to, in place of an enumeration that was wrong twice.

**What none of this can do, stated plainly.** A pull request that edits
`ci.yml` can delete those steps, and the oracle that would notice is a Rust
test the same edit can arrange not to run. There is no arrangement of
in-repository checks that closes that loop, because every one of them is
executed by the thing under test. `MAINTAINING.md` already says where the
boundary actually is: `upstroke-ci` and `upstroke-pr-policy` are
candidate-controlled and are "not the security boundary … the boundary is that
only the owner merges, after an independent review of the exact head, and that
the diff the owner reviews includes any change to the gates themselves." What
the pins above buy is not self-enforcement. It is that any such edit is a
small, named, reviewable line in that diff rather than a plausible-looking
change to a workflow nobody reads closely.

Two files outside the workflow can make any reading of it false, and
`no_repository_file_overrides_what_ci_compiles_or_runs` holds both absent: a
`rust-toolchain.toml`, which overrides the rustup default the pinned action
sets and so replaces the compiler every leg runs, witness and MSRV floor
included; and a `.cargo/config.toml`, which can bind `target.<triple>.runner`
and make `cargo test` build every Windows harness while a wrapper reports
success without executing one. It is a Rust test, and a Rust test is precisely
what the second of those switches off. That is deliberate and it is why the
test is not written as a defence: what it catches is the file committed by
accident, on a developer's machine and in CI alike, and what it says in a diff
is that adding one is a decision this contract has to be extended for. The
count on the self-hosted leg is what covers the platform whose execution moved.

These were all reported against this change and are older than it: they hold on
`master` today, on every leg, and the self-hosted leg is what made the Windows
one load-bearing by leaving that platform's execution in one place. Closing
them for the hosted legs is not in this change and is not claimed by it. The
contract also pins the hosted witness: exactly one `windows-latest` job carries `cargo
build --all-targets --all-features` exactly once and that job is the Windows
Clippy gate, since `cargo check` and Clippy stop before codegen and the guest
links with the image's toolchain; `--all-targets` rather than `test --no-run`
because the latter links only test-profile artifacts and never a
`#[cfg(all(windows, not(test)))]` body in a binary. `windows-latest` keeps its
`CI_TARGETS` entry because its Clippy, witness and MSRV legs keep it. The
`MUT-TEST-WINDOWS-*` rows execute the refusals: the job rehosted on
`windows-latest`, its labels loosened, its command deleted, its test step
disabled at step level (a job-level `if:` reports `skipped`, which the
aggregate already refuses), its checkout pointed at `master`, its identity
step retargeted to `master`, and the job renamed away (a lost handle rather
than a false green, since the aggregate's derived `needs` refuses that too);
the two `MUT-WINDOWS-WITNESS-*` rows drop the count and lower its floor to
zero, which are the two ways a step that reads as a witness stops being one;
`MUT-TEST-MATRIX-KEEPS-WINDOWS`, `MUT-TEST-CHECKOUT-REF` and
`MUT-TEST-RUN-RETARGETED` cover the hosted matrix; `MUT-GATE-RUN-RETARGETED`,
`MUT-MSRV-RUN-RETARGETED`, `MUT-WITNESS-CHECKOUT-REF` and
`MUT-STEP-USES-UNPINNED` cover the other legs; and the two
`MUT-WINDOWS-BUILD-WITNESS-*` rows the witness deleted or narrowed back to
`test --no-run`.

The runner mechanism is operator tooling and lives in the private companion
tree ([2026-09-01](2026-09-01-infra-private.md)): the golden-image curation
(runner 2.337.0 unconfigured, PowerShell 7.6.5, a shared pre-warmed
`CARGO_TARGET_DIR`, Defender exclusions on the runner and target roots), the
overlay-per-job loop, and the guest provisioning that reproduces the image.

## Obligations

1. The host loop runs continuously — the `winguest-ci` systemd unit on the
   build box — under a dedicated fine-grained token scoped to this one
   repository with *Administration: write*, the permission a just-in-time
   registration needs. The token is readable by the loop alone and is
   deliberately not the box's shared token, which every agent session there
   inherits and which must never be able to edit branch protection. Without
   a listening runner the job queues and `upstroke-ci` cannot settle; the
   token's expiry is therefore a dated obligation of its own.
2. Re-curation of the golden image is a deliberate act — shut both guests down,
   boot the base, change, shut down, recreate the overlays — recorded when it
   happens, and due whenever the repository's `stable` expectations move.
3. The floor on the self-hosted leg is a measured number, and measured
   numbers age. It is 1700 against the 1771 reported at `cb8ac1f`. A change that removes
   that many Windows tests, or that gates them behind a feature, edits the
   floor in the same change and says so; a change that finds the leg failing on
   the floor has found either that, or a suite that stopped executing.
4. Rollback is a revert of the pull request that lands this record: the
   `windows-latest` test leg returns and the oracle returns with it. The
   guest, the image and the loop are outside the repository and need no
   change to be ignored.

Inputs: the 2026-09-01 measurement session (winguest sweeps, the per-module
profile of run 33533228348, pull requests #91, #92 and #95).
