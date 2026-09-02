# PR #96 — the Windows test leg moves to a self-hosted ephemeral runner: frontier review record

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED on pass 9, five findings, recorded as written; completed on the owner's classification under decisions/2026-09-01-review-effort-rescoped.md, the one reproducible finding repaired in the disclosed delta above the reviewed head** |
| **Reviewed SHA** | `cb8ac1fff431a6490cfdda530fec7f1c9f81a691` |
| Pull request | eventloops/upstroke#96, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 96`, 90-minute per-pass limit |
| Passes | nine, each on its own exact head: `79b9429`, `76a56e9`, `612064d`, `ce40fe4`, `00b2d43`, `2c89dff`, `550b673`, `b5ca535`, `cb8ac1f` |
| Findings | thirty-one across the first eight passes, five on the ninth, one with a reproduction; every one either repaired or rejected by a row showing why, and each carries a ledger row in the pull request body |
| CI at each reviewed SHA | green, `upstroke-pr-policy` included; the eight-command baseline ran on the build box at every head |

## What the nine passes were about, in one paragraph

A gate change gets the serious-P1 bar with `security-trust` in scope and no
exemption, so every serious finding here was fixed and its repaired head went
back for a fresh pass. The first pass judged the change; the rest judged the
contract the change leans on. Moving one platform's test execution to
owner-controlled hardware made a class of pre-existing hole load-bearing:
before it, a false green on `windows-latest` still ran the suite somewhere
GitHub controlled, and after it, the workflow-shape oracle is the only thing
standing between a green `upstroke-ci` and a Windows suite that never ran the
candidate. Most of the findings are that class, and several hold on `master`
today, unchanged by this pull request; the ledger marks those `pre_existing`.
They are repaired here because this change is what made them matter.

The shape of the repair, stated once. Every step of every modelled job is
either an action pinned to a reviewed commit or a script pinned character for
character; every checkout takes no inputs; the toolchain input and its
position, each action's input values, the workflow environment and the token
are pinned as equalities rather than guarded by name. And the self-hosted leg
counts the tests it ran: its suite step ends with a sum of the `test result:
ok.` counts libtest printed and a floor of 1700 against the 1771 it reports
today, so a runner wrapper that executes nothing, a Cargo home outside the
checkout, an ancestor `.cargo/config.toml`, a `CARGO_*RUNNER` in the guest's
environment and a workspace that reselects the package all end in the same
place, with nothing to count.

That last pin is the one the pass history is about. Passes 4 through 8 each
added, and then found a route past, a shell guard that enumerated ways Cargo
can be told to execute nothing. Pass 7 established the limit in principle: no
in-repository check can enforce its own presence, because every one of them
is executed by the thing under test. Pass 8 found two more routes past the
latest guard and three mutation rows that did not reproduce the escapes they
claimed. The repair for pass 8 removed the guards rather than extending them
and replaced the enumeration with the count. The findings per pass ran 3, 4,
4, 4, 4, 3, 4, 5, 5.

## The verdicts, verbatim

Each blockquoted byte-for-byte from the reviewer's saved output on the review
box (`~/review-pr96-<head7>.md`).

### Pass 1 — `79b94296e5f3533c4858bdea4a36da5852cc4e43`

> ## Findings
>
> 1. **The hosted Windows runner is no longer a codegen/link witness, contrary to the PR’s “only test execution moves” claim.** The remaining hosted jobs run `cargo check` and `cargo clippy` ([ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:93), [ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:123)). Clippy dispatches Cargo’s `check` subcommand, while Cargo documents that `check` skips final code generation and can miss codegen-only errors ([Clippy source](https://github.com/rust-lang/rust-clippy/blob/master/src/main.rs), [Cargo documentation](https://doc.rust-lang.org/cargo/commands/cargo-check.html)). Concrete failure sequence:
>
>    1. Candidate code type-checks but triggers a Windows codegen/link failure specific to stable 1.98.
>    2. Hosted Clippy 1.98 and MSRV `check` pass without producing the affected executable.
>    3. The guest’s 1.97.1 `cargo test` code-generates, links, and passes.
>    4. `upstroke-ci` is green, but a Windows user on current stable cannot build.
>
>    The former hosted `cargo test` caught this class. The toolchain-drift disclosure does not acknowledge that current stable is never code-generated or linked. Retain a hosted no-execution build witness, such as `cargo test --no-run --all-targets --all-features`.
>
> 2. **The new oracle accepts testing `master` instead of the candidate.** [`ci_test_windows_job_complaints`](/srv/worktrees/pr96/src/effects/tests/workflow.rs:718) checks only step field names; `with` is allowed, but checkout inputs are never inspected. This mutation therefore produces no complaint:
>
>    ```yaml
>    - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262
>      with:
>        ref: master
>    ```
>
>    `ref` explicitly selects another branch/SHA ([checkout input contract](https://github.com/actions/checkout/blob/main/README.md)). A Windows-only logic regression then type-checks in hosted Clippy, Linux/macOS tests ignore it, `test-windows` tests `master`, and the aggregate goes green. This contradicts the new test’s claim that it pins everything readable needed to run these fixtures ([tests.rs](/srv/worktrees/pr96/src/effects/tests.rs:1625)). Pin the candidate checkout and add this mutation witness.
>
> 3. **The claimed “disabled” mutation is not an escape.** [`MUT-TEST-WINDOWS-DISABLED`](/srv/worktrees/pr96/src/effects/tests/workflow.rs:1534) adds job-level `if: false`. That yields `needs.test-windows.result == "skipped"`; the aggregate runs under `always()` and rejects every result other than `success`. GitHub documents `skipped` as a distinct `needs` result ([contexts reference](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#needs-context)), and this diff itself states the correct behavior at [ci_model.rs](/srv/worktrees/pr96/src/effects/tests/ci_model.rs:168). A step-level `if: false` on the cargo-test step would be the genuine false-green mutation.
>
>    The body also claims six new `MUT-TEST-WINDOWS-*` rows, but only five have that prefix; the sixth is `MUT-TEST-MATRIX-KEEPS-WINDOWS`. Thus both the scope and validation claims are literally unsupported.
>
> I found no separate `unwrap`/`expect`, `anyhow`, path-handling, or decision-immutability violation: the added `expect` is test-only, no production path/error code changed, and no existing decision record was edited.
>
> VERDICT: CHANGES_REQUIRED

### Pass 2 — `76a56e90a1c9d6c6f9e314182a4bbdafd8a0806f`

> CHANGES_REQUIRED.
>
> 1. **P1 — the hosted witness does not link the shipped Windows binary.** [.github/workflows/ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:143) uses `cargo test --no-run --all-targets`. On this exact head, its artifact stream contained a normal library plus test-profile library, binary, and example artifacts—but no non-test binary artifact. That matches [Cargo’s documented test-target behavior](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection).
>
>    Concrete failure: add a native-library reference and call in `src/main.rs` under `#[cfg(all(windows, not(test)))]`. Clippy and MSRV only type-check it; both hosted and self-hosted `cargo test` compile the binary with `cfg(test)`, excluding it; Unix legs exclude Windows. CI is green, but `cargo build` on Windows fails at link time. This directly contradicts the claims in [tests.rs](/srv/worktrees/pr96/src/effects/tests.rs:1648) that any Windows-only link failure cannot pass and that every Windows body is code-generated and linked.
>
> 2. **P1 — the checkout repair does not ensure tests execute the checked-out candidate.** [workflow.rs](/srv/worktrees/pr96/src/effects/tests/workflow.rs:719) permits arbitrary `run:` contents and validates only their fields and shell. `checkout_complaints` at [line 782](/srv/worktrees/pr96/src/effects/tests/workflow.rs:782) examines only `with:` on `actions/checkout`.
>
>    Append this to the existing “Configure git identity” step:
>
>    ```powershell
>    git fetch origin master
>    git checkout --detach FETCH_HEAD
>    ```
>
>    The labels, fields, shell, input-free checkout, and exact test command all remain accepted. A candidate can then contain a compiling Windows-only runtime regression; Linux/macOS tests cannot see it, the hosted witness does not execute it, and `test-windows` tests `master` and succeeds. The aggregate goes green. Thus the body’s “so neither can test a tree other than the head under test” claim is false.
>
> 3. **P1 — the hosted link witness itself is not bound to the candidate tree.** [ci_windows_build_witness_complaints](/srv/worktrees/pr96/src/effects/tests/workflow.rs:815) never invokes `checkout_complaints` for its carrier. Adding `with: { ref: master }` to the `lint-windows` checkout at [ci.yml:134](/srv/worktrees/pr96/.github/workflows/ci.yml:134) is accepted by the oracle.
>
>    Concrete sequence: candidate `build.rs` emits a missing Windows link library only on stable 1.98; the 1.97 guest builds and passes; hosted `lint-windows` checks out `master`, so its current-stable witness passes; MSRV does not link and Unix legs are excluded. CI is green while a current-stable Windows user cannot build the candidate.
>
> 4. **P2 — `MUT-TEST-WINDOWS-RENAMED-AWAY` is not a genuine escape.** The mutation at [workflow.rs:1659](/srv/worktrees/pr96/src/effects/tests/workflow.rs:1659) changes only the job key. The aggregate still needs and reads `test-windows` at [ci.yml:189](/srv/worktrees/pr96/.github/workflows/ci.yml:189), making the workflow invalid or blocked rather than falsely green. The mutation test proves only that a complaint code appears, not the claimed failure sequence—essentially the same evidentiary defect repaired for the disabled-job mutation.
>
> The “macOS becomes the critical path” claim is also unsupported at this head: the cited live runs predate the newly added hosted witness, and the exact-head CI checkboxes remain pending.
>
> I found no prohibited non-test `unwrap`/`expect`, no new `anyhow` or path portability violation, and no undisclosed changed files.
>
> VERDICT: CHANGES_REQUIRED

### Pass 3 — `612064de1b43519a33a0c23c6e86e19780c6d483`

> Changes are required.
>
> 1. **P1 — “current stable” is not pinned.** `step_pin_complaints` validates only the action SHA; `with:` remains unrestricted. `ci_windows_build_witness_complaints` never checks the toolchain action or its order. Changing `lint-windows` from `toolchain: stable` to `1.97.1` leaves every oracle complaint empty. The pinned action’s input selects the toolchain and makes it the rustup default ([action definition](https://raw.githubusercontent.com/dtolnay/rust-toolchain/4360b52568e2003a75bf9bc1d59f33a8e3fc893c/action.yml)). Concrete sequence: add a build script that emits a missing Windows link library only on rustc ≥1.98; downgrade this input to 1.97.1; the guest and witness both pass on 1.97, MSRV passes on 1.85, Unix targets do not emit the link directive, while a current-stable Windows user fails. This directly defeats the PR’s principal witness claim.
>
> 2. **P1 — the Windows test command can run in another crate.** `ci_test_windows_job_complaints` allows `defaults`; `DEFAULTS_RUN_FIELDS` allows `working-directory`; `defaults_shell` validates only the shell. GitHub applies a job-level default directory to every `run` step ([workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_iddefaultsrunworking-directory)). Add:
>
>    ```yaml
>    defaults:
>      run:
>        working-directory: ci-pass
>    ```
>
>    plus a trivial `ci-pass/Cargo.toml`. The exact pinned command then tests that empty crate. Hosted tests still execute the real oracle and accept this shape; Windows Clippy/build compile the real project but do not execute it. A Windows-only runtime regression therefore produces a green aggregate.
>
> 3. **P1 — workflow `env` can suppress every Windows test executable.** The top-level environment is not pinned; `rustflags_complaints` guards only `RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS`. Add `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER: pwsh -NoProfile -File .github/scripts/pass.ps1`, where the script exits zero. Cargo officially applies that target-runner setting to `cargo test` ([Cargo configuration](https://doc.rust-lang.org/cargo/reference/config.html#targettriplerunner)). Every harness compiles, but none executes; Unix legs ignore the Windows-target variable, and the oracle accepts it. This is another concrete false green despite the exact command, shell, labels, and checkout pins.
>
> 4. **P2 — the hosted timing evidence cites the wrong command.** [pr.md](/tmp/tmp.0xNNOy25HV/pr.md:191) says the full `cargo build --all-targets` witness was measured at `76a56e9`, but that commit ran `cargo test --no-run --all-targets --all-features`; the build command first appears at `612064d`. The actual exact-head job has since passed, but the supplied body’s stated measurement provenance is unsupported.
>
> The changed-file set matches the disclosed scope, and I found no separate unwrap/expect, anyhow, path-portability, immutable-decision, or silent-scope violation.
>
> VERDICT: CHANGES_REQUIRED

### Pass 4 — `ce40fe479eb74709f7b19a6b6037415b70985813`

> Changes are required.
>
> 1. **P1 — Windows tests can still be compiled but never executed.** The new guard only pins workflow-level `env:` ([pr.diff](/tmp/tmp.lnaO7ei6LQ/pr.diff:826)). A candidate can instead commit:
>
>    ```toml
>    [target.x86_64-pc-windows-msvc]
>    runner = ["cmd.exe", "/D", "/C", ".cargo\\pass-runner.cmd"]
>    ```
>
>    where `pass-runner.cmd` ignores the test executable and exits zero. Cargo applies target runners to `cargo test` ([Cargo reference](https://doc.rust-lang.org/cargo/reference/config.html#targettriplerunner)). Sequence: introduce a Windows-only runtime regression → the self-hosted leg compiles every harness but the wrapper returns success without executing one → hosted Windows jobs only Clippy/build/check → Unix tests cannot see it → `upstroke-ci` is green. The oracle reads only `ci.yml`, so its new environment equality and mutation both pass unchanged.
>
> 2. **P1 — `toolchain: stable` does not pin the compiler that runs.** `toolchain_complaints` checks only existing action inputs and is vacuously satisfied if the toolchain action is deleted ([pr.diff](/tmp/tmp.lnaO7ei6LQ/pr.diff:799)). More importantly, the pinned action installs and sets a rustup *default* ([action source](https://github.com/dtolnay/rust-toolchain/blob/4360b52568e2003a75bf9bc1d59f33a8e3fc893c/action.yml)); a repository `rust-toolchain.toml` outranks that default ([rustup precedence](https://rust-lang.github.io/rustup/overrides.html)).
>
>    Concrete sequence: commit `rust-toolchain.toml` selecting 1.97.1 plus Clippy → all bare `cargo` commands use 1.97.1 despite the YAML saying `stable` → add a Windows build script that emits a missing link library only on rustc 1.98+ → guest and hosted witness both pass on 1.97.1 → a current-stable Windows consumer fails. The MSRV leg is similarly redirected away from 1.85.0. Thus the body’s current-stable witness claim is stronger than the implementation.
>
> 3. **P1 — the stated read-only token boundary is not enforced.** `WORKFLOW_FIELDS` requires only that a `permissions` key exists; nothing validates its value ([pr.diff](/tmp/tmp.lnaO7ei6LQ/pr.diff:498)). Changing it to `permissions: write-all` produces no oracle complaint. On a same-repository PR/push—or a fork configuration permitting write tokens—checkout v4’s default credential persistence lets candidate build scripts/tests issue authenticated Git commands. GitHub documents both `write-all` and fork adjustments ([workflow permissions](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#permissions)), while checkout documents that credentials remain available to later Git commands ([checkout v4](https://github.com/actions/checkout/blob/11d5960a326750d5838078e36cf38b85af677262/README.md)). Guest destruction cannot undo a remote push. This contradicts the body’s trust-boundary claim that the token “stays `contents: read`.”
>
> 4. **The body is stale and silently omits exact-head scope.** `ce40fe4` is a child of the body’s claimed current head `612064d`. It adds 146 lines covering workflow-env equality, toolchain validation, default-directory rejection, three mutations, and decision-record changes, but none appears in the explicit scope list ([pr.md](/tmp/tmp.lnaO7ei6LQ/pr.md:27)). Validation is recorded only at `612064d` ([pr.md](/tmp/tmp.lnaO7ei6LQ/pr.md:74)); pass 3 and the reviewed SHA remain pending ([pr.md](/tmp/tmp.lnaO7ei6LQ/pr.md:151)). That directly violates the rule against silently widening scope. The focused oracle tests pass at `ce40fe4`, but they do not cover the bypasses above and are not the claimed eight-command validation.
>
> I found no new non-test `unwrap`/`expect`, `anyhow`, path-portability, or direct `DESIGN.md` contradiction; the explicit project-rule breach is the undisclosed scope/head movement.
>
> VERDICT: CHANGES_REQUIRED

### Pass 5 — `00b2d438c4a6d845eb8247306d8db7e2d5d224a8`

> Blocking findings:
>
> 1. **P1 — “Pinned” actions can still retarget the checkout.** `STEP_FIELDS` permits `with:`, while `step_pin_complaints` checks only the action SHA (`workflow.rs:835-855`). The pinned cache action exposes an executable `cmd-format` input ([manifest](https://github.com/Swatinem/rust-cache/blob/6323deb102c322ba6fcbdcafc7e3dddab59af2b6/action.yml), [implementation](https://github.com/Swatinem/rust-cache/blob/6323deb102c322ba6fcbdcafc7e3dddab59af2b6/src/config.ts)). Add this to the existing `lint-windows` cache step:
>
>    ```yaml
>    with:
>      cmd-format: 'cmd /d /s /c "git fetch origin master && git checkout --detach FETCH_HEAD && {0}"'
>    ```
>
>    The oracle accepts it: the SHA is allowlisted, the checkout itself has no inputs, and every `run:` remains exact. The cache action switches to `master`; Clippy and the build witness then examine `master`. A candidate containing a Windows link failure specific to the guest’s newer compiler passes on the older guest, while the hosted witness builds `master`; `upstroke-ci` goes green. The central “no leg can be pointed at another tree” claim is false.
>
> 2. **P1 — `.cargo/config.toml` can disable its own absence guard.** `no_repository_file_overrides_what_ci_compiles_or_runs` is itself only a Rust test (`tests.rs:1676-1706`). Add `.cargo/config.toml` with runners for every CI host triple, each invoking an always-successful shell/PowerShell script. Cargo then builds every harness but executes the wrappers instead, so this guard and the entire workflow oracle never run. Clippy, `cargo build`, and MSRV `cargo check` do not execute test harnesses; a Windows runtime regression therefore passes every leg. Cargo explicitly applies target runners to `cargo test` ([Cargo reference](https://doc.rust-lang.org/cargo/reference/config.html#targettriplerunner)). A check intended to forbid Cargo configuration cannot safely run behind that same Cargo configuration.
>
> 3. **P1 — The current-stable witness does not require current stable before the witness.** `toolchain_complaints` validates only toolchain actions that happen to exist and never checks their order (`workflow.rs:866-883`). Move the existing stable-toolchain action below `cargo build`; every oracle check still passes. During a release rollout where rustup stable is 1.99 but `windows-latest` still has 1.98 preinstalled, Clippy and the witness use 1.98, the action installs 1.99 afterward, and the frozen guest uses 1.97. A failure introduced only on 1.99 is unseen despite the body’s current-stable claim. Require exactly one stable installation before cache and every Cargo command.
>
> 4. **The exact-head scope and evidence are materially false.** `pr.md:29-48` says two tests and omits the new global env/permissions equalities, toolchain/default-directory policy, four extra mutations, repository-file ban, and third test. Those are broad gate-policy changes, violating the rule against silently widening scope. Validation calls `612064d` the current head (`pr.md:74-88`), but the requested head is `00b2d438…`, two material validator commits later; pass 3 and the reviewed SHA remain pending, and the five later repairs have no ledger rows. The exact-head eight-command baseline required before a push is not recorded.
>
> I found no new production `unwrap`/`expect`, `anyhow`, or non-`std::path` violation; the Rust changes are test-only.
>
> VERDICT: CHANGES_REQUIRED

### Pass 6 — `2c89dff10704318402eecd0a5dca1f4c14615b7f`

> 1. **P1 — action inputs remain capable of retargeting the checkout.** [`ACTION_INPUTS`](/tmp/upstroke-review-2c89dff/src/effects/tests/ci_model.rs:108) allowlists the `components` key, while [`step_pin_complaints`](/tmp/upstroke-review-2c89dff/src/effects/tests/workflow.rs:883) validates only key names, never values. The pinned toolchain action constructs shell text from `components` and interpolates it directly into a later Bash command ([exact action source](https://github.com/dtolnay/rust-toolchain/blob/4360b52568e2003a75bf9bc1d59f33a8e3fc893c/action.yml#L58-L66), [execution line](https://github.com/dtolnay/rust-toolchain/blob/4360b52568e2003a75bf9bc1d59f33a8e3fc893c/action.yml#L92-L96)). For example, this `lint-windows` value passes every oracle check:
>
>    ```yaml
>    components: 'clippy;git${IFS}fetch${IFS}origin${IFS}master&&git${IFS}checkout${IFS}--detach${IFS}FETCH_HEAD;true'
>    ```
>
>    The action installs Clippy, checks out `master`, and finishes successfully. A candidate can then introduce a Windows link failure triggered only on stable 1.98: the 1.97 guest and MSRV pass the candidate, while the hosted Clippy/build witness runs against `master`. `upstroke-ci` becomes green although current-stable Windows cannot build the candidate.
>
> 2. **P1 — the workflow-env pin can suppress its own oracle.** [`workflow_env_complaints`](/tmp/upstroke-review-2c89dff/src/effects/tests/workflow.rs:998) runs only inside the Rust test harness. The new non-Cargo guard checks four repository filenames only ([ci.yml](/tmp/upstroke-review-2c89dff/.github/workflows/ci.yml:180)). Add Cargo runner variables for all three CI triples, pointing to trivial scripts that exit zero. Every `cargo test` then compiles but executes no harness, including the test that rejects the changed environment; Clippy, `cargo build`, MSRV checks, and the filename guard still pass. A Windows runtime regression therefore reaches a green aggregate. I reproduced the key behavior at this exact head: the oracle control ran one test, while `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=true` made the same Cargo command exit successfully without running the harness.
>
> 3. **The PR body does not describe the reviewed head and silently omits its final scope.** It says “five commits” while naming six older commits, omits `2c89dff` entirely, and calls `00b2d43` the current head ([pr.md](/tmp/tmp.8sAnkxG0Fn/pr.md:36), [validation](/tmp/tmp.8sAnkxG0Fn/pr.md:93)). It still calls the three pass-5 findings unrepaired and recommends splitting them out ([pr.md](/tmp/tmp.8sAnkxG0Fn/pr.md:172)), although `2c89dff` adds their purported repairs—220 insertions across four paths. The Scope also omits `ACTION_INPUTS`, `REPO_FILE_GUARD`, `ci_repo_file_guard_complaints`, and the three new mutations. Consequently, neither the stated scope nor the recorded validation/review SHA supports the exact head. This violates the rule against silently widening a change’s scope.
>
> VERDICT: CHANGES_REQUIRED

### Pass 7 — `550b673fdde49c849500d1d2ae0951aaa4645940`

> I found three false-green paths and one material accuracy problem.
>
> 1. **P1 — Cargo package selection is unpinned.**
>
>    Every CI Cargo command uses `--all-targets --all-features`, but none selects `--workspace` or `--package upstroke`; see the [test command](/srv/worktrees/pr96/.github/workflows/ci.yml:67). Cargo applies `--all-targets` only to the selected packages, and a workspace root without package-selection flags selects `workspace.default-members`. [Cargo documents this behavior explicitly.](https://doc.rust-lang.org/cargo/commands/cargo-test.html#package-selection)
>
>    Concrete false-green sequence:
>
>    1. Add a trivial `ci-pass` crate with zero tests.
>    2. Add to the root manifest:
>       ```toml
>       [workspace]
>       members = ["ci-pass"]
>       default-members = ["ci-pass"]
>       ```
>       and update `Cargo.lock`.
>    3. Add a compiling Windows runtime regression to `upstroke`.
>    4. Test, Clippy, MSRV, and the Windows build witness all select only `ci-pass`.
>    5. The workflow oracle and `no_repository_file_overrides_what_ci_compiles_or_runs` belong to `upstroke`, so neither executes.
>    6. The Bash guard sees no forbidden file or runner variable; every job succeeds; `upstroke-ci` is green.
>
>    This is another way Cargo can execute none of the candidate suite, directly contradicting the body’s exhaustive “the ways Cargo can be told to execute nothing” claim.
>
> 2. **P1 — the Bash guard still cannot guard its own presence.**
>
>    The guard is external to Cargo at [ci.yml:184](/srv/worktrees/pr96/.github/workflows/ci.yml:184), but its required presence is enforced only by the Rust function at [workflow.rs:981](/srv/worktrees/pr96/src/effects/tests/workflow.rs:981). Therefore the claimed structural repair remains circular.
>
>    Concrete false-green sequence:
>
>    1. Delete the Bash guard step.
>    2. Add `.cargo/config.toml` assigning no-op runners to the Linux, macOS, and Windows CI triples; each wrapper ignores the appended test executable and exits zero.
>    3. `cargo test` compiles every harness, then invokes the wrappers, so no Rust test executes—including the mutation oracle that should reject the deleted guard and the test that should reject `.cargo/config.toml`.
>    4. Clippy, MSRV, the hosted Windows build witness, and the four shell gates still pass.
>    5. All three test platforms report success without running the candidate suite, and the aggregate becomes green.
>
>    Cargo expressly applies `target.<triple>.runner` to `cargo test`. [Cargo configuration reference.](https://doc.rust-lang.org/cargo/reference/config.html#targettriplerunner) `MUT-REPO-FILE-GUARD-DELETED` does not demonstrate prevention of this composed case because that mutation is itself executed by the suppressed Rust suite.
>
> 3. **P1 — the “effective environment” guard observes the wrong runner.**
>
>    The environment check runs exactly once, on hosted Ubuntu, not on `test-windows`. A concrete operational failure is:
>
>    1. The Windows golden image or runner service carries `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=C:\ci\pass.cmd`, or its `%USERPROFILE%\.cargo\config.toml` contains the equivalent runner.
>    2. The Ubuntu guard sees neither the Windows process environment nor its Cargo home and passes.
>    3. Linux/macOS execute the oracle, but it only parses repository state and `ci.yml`, so they also pass.
>    4. Windows compiles its harnesses and executes the no-op wrapper; a Windows runtime regression is never run.
>    5. Hosted Windows Clippy/build/MSRV pass and the aggregate is green.
>
>    Cargo searches `$CARGO_HOME/config.toml`, including `%USERPROFILE%\.cargo\config.toml` on Windows, and GitHub run steps can read runner-environment variables. [Cargo configuration hierarchy](https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure), [GitHub variables documentation](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-variables). Thus the body and source comment claiming the guard catches a binding “wherever it was written” are too strong. The actual Windows runner needs its own pinned preflight, or clean image environment/config must be stated as an unverified obligation.
>
> 4. **P2 — the PR body’s exact scope and review accounting are false.**
>
>    [pr.md:39](/tmp/tmp.B0HlJd7WRV/pr.md:39) says “seven commits” but lists eight hashes and omits the actual initial implementation commit `a141f4b`. The first-parent range contains nine commits total: eight authored commits plus merge `79b9429`; `31dd944` is a documentation follow-up, not “the change itself.”
>
>    It also says seventeen mutation rows were added or reworded, while the diff adds 24 `MUT-*` rows. Finally, [pr.md:148](/tmp/tmp.B0HlJd7WRV/pr.md:148) says “Five passes” before describing six, and the verdict record still says five verdicts. Those errors undermine the body’s claim that the earlier stale-scope findings are repaired.
>
> I found no new `unwrap`/`expect` outside test-only modules, no new `anyhow` use, no non-`std::path` path handling, and no contradiction with DESIGN.md. The extra oracle hardening is substantively disclosed, but its commit/mutation/review accounting is not exact.
>
> VERDICT: CHANGES_REQUIRED

### Pass 8 — `b5ca535ca000a2debecc6fce1d2383b96e8746b4`

> I verified the repository is at the exact requested SHA and that the supplied `pr.diff` matches it. I found five issues.
>
> 1. **P1 — The workspace guard is trivially bypassed by valid TOML syntax.**
>
>    Both workflow guards match only the literal `^[workspace]` spelling ([ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:95), [ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:191)); the Rust fallback likewise compares against exactly `"[workspace]"` after trimming only exterior whitespace ([tests.rs](/srv/worktrees/pr96/src/effects/tests.rs:1707)).
>
>    Concrete false-green sequence:
>
>    - Add a trivial `ci-pass` crate and this valid root declaration:
>      ```toml
>      [ workspace ]
>      members = ["ci-pass"]
>      default-members = ["ci-pass"]
>      ```
>    - Both shell guards and the Rust check miss the internal whitespace.
>    - Every relevant Cargo command lacks `--workspace` or `--package`, so it selects only `ci-pass`.
>    - The upstroke tests, workflow oracle, Clippy target and Windows witness never build or run.
>    - A Windows regression in upstroke reaches a green `upstroke-ci`.
>
>    Therefore the PR96-PACKAGE-SELECTION-UNPINNED repair is incomplete.
>
> 2. **P1 — The Windows guard does not inspect Cargo’s effective configuration hierarchy.**
>
>    The guest guard checks repository config files and `%USERPROFILE%\.cargo`, but not `$env:CARGO_HOME` or ancestor `.cargo` directories ([ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:95)). This contradicts the claim that it reads the guest’s Cargo home.
>
>    Concrete sequence:
>
>    - Guest provisioning sets `CARGO_HOME=D:\cargo-home`.
>    - `D:\cargo-home\config.toml` binds the Windows target runner to a wrapper that ignores its executable argument and exits zero.
>    - The guard ignores `CARGO_HOME`; its environment test only rejects variable names ending in `RUNNER`.
>    - `cargo test` compiles each harness and invokes the no-op runner, executing no Windows tests.
>    - The hosted guard cannot inspect the guest filesystem, so every job succeeds.
>
>    The same problem exists for a `.cargo/config.toml` in an ancestor of the checkout.
>
> 3. **P1 — The hosted witness does not build the shipped profile.**
>
>    The witness is `cargo build --all-targets --all-features` ([ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:156)). Executed at this head, it builds the `dev` profile. The actual shipped Windows artifact uses `cargo build --release` ([release.yml](/srv/worktrees/pr96/.github/workflows/release.yml:106)).
>
>    Concrete sequence:
>
>    - A build script emits a missing Windows link library only when `TARGET` is Windows and `PROFILE=release`.
>    - Clippy, MSRV, the guest test profile and the hosted dev-profile witness all pass.
>    - `upstroke-ci` is green.
>    - The release build or a user’s `cargo build --release` fails to link.
>
>    Thus “links shipped binaries as shipped” is stronger than the diff supports.
>
> 4. **P2 — Several advertised mutation witnesses do not reproduce their claimed escape.**
>
>    Examples:
>
>    - `MUT-WINDOWS-GUARD-NEUTERED` changes only the first checked filename; the Cargo-home, environment and workspace checks remain active ([workflow.rs](/srv/worktrees/pr96/src/effects/tests/workflow.rs:2055)).
>    - `MUT-REPO-FILE-GUARD-NEUTERED` removes only `for f in rust-toolchain.toml`, leaving a syntactically invalid Bash command that fails red rather than green ([workflow.rs](/srv/worktrees/pr96/src/effects/tests/workflow.rs:2091)).
>    - `MUT-WORKFLOW-ENV-TARGET-RUNNER` names `.github/scripts/pass.ps1`, which does not exist, so that runner fails rather than silently succeeding ([workflow.rs](/srv/worktrees/pr96/src/effects/tests/workflow.rs:2114)).
>
>    The mutation test passes because it only checks that parsing the mutated YAML produces the expected complaint code; it never executes or otherwise validates the claimed failure sequence. These rows are complaint-code fixtures, not evidence of the documented false greens.
>
> 5. **P2 — The PR body remains internally stale and omits diff scope.**
>
>    It says “Five passes” while enumerating seven completed passes plus pass 8 pending ([pr.md](/tmp/tmp.elBiDNlYXM/pr.md:150)), records only five saved verdicts ([pr.md](/tmp/tmp.elBiDNlYXM/pr.md:225)), and still describes merging “on a passing pass 7” even though pass 7 required changes ([pr.md](/tmp/tmp.elBiDNlYXM/pr.md:247)). Its supposedly exhaustive `ci_model.rs gains` list also omits `TEST_WINDOWS_SCRIPTS` and `WINDOWS_REPO_GUARD`, both added by the diff. That breaches the stated rule against silently widening or inaccurately describing scope.
>
> I found no new production `unwrap`/`expect`, `anyhow`, non-`std::path` path handling, modification of an existing immutable decision, or direct contradiction with DESIGN.md’s Windows-first-class requirement.
>
> VERDICT: CHANGES_REQUIRED

### Pass 9 — `cb8ac1fff431a6490cfdda530fec7f1c9f81a691`

> Changes are required.
>
> 1. **P1 — “Only test execution moves” is false.** The hosted witness runs `cargo build`, which uses the `dev` profile; `cargo test` uses the distinct `test` profile ([Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)). Thus current-stable Windows test-profile compilation and linking also moved to the older guest toolchain.
>
>    Concrete false green:
>
>    - Set `[profile.test] debug-assertions = false`.
>    - Add a Windows/test/non-debug body gated to Rust 1.98+, containing a compile failure.
>    - Hosted Rust 1.98 `cargo build --all-targets` uses dev/debug assertions and excludes it.
>    - Guest Rust 1.97 `cargo test` excludes it by version.
>    - Unix excludes it by `cfg(windows)`.
>    - CI passes, while `cargo test` on current-stable Windows fails.
>
>    This contradicts [ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:145), [ci_model.rs](/srv/worktrees/pr96/src/effects/tests/ci_model.rs:50), and the PR’s repeated assertion that only execution moved.
>
> 2. **P1 — The count witnesses forgeable text, not Upstroke’s tests.** The script sums any stdout line matching `test result: ok. N passed`; it does not associate that line with an expected harness ([ci.yml](/srv/worktrees/pr96/.github/workflows/ci.yml:110)).
>
>    Concrete machine-side bypass:
>
>    - Configure Cargo’s Windows target runner to a wrapper.
>    - The wrapper ignores the appended harness path, prints `test result: ok. 1700 passed`, and exits zero.
>    - Cargo invokes it instead of every harness and succeeds.
>    - `$passed` reaches the floor; no Upstroke test executed.
>
>    Cargo explicitly appends the target executable and arguments to the configured runner ([Cargo configuration](https://doc.rust-lang.org/cargo/reference/config.html)). A repository-only bypass is also possible: select a decoy workspace default member with `harness = false` and a custom `main` printing that line; Cargo supports custom test executables ([cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html)).
>
>    Therefore the body’s absolute claim that “no arrangement of runners, Cargo homes or manifests” can manufacture the lines is unsupported and false.
>
> 3. **P1 — The oracle permits an arbitrary toolchain install in `test-windows`.** `step_pin_complaints` permits the pinned toolchain action and permits its `toolchain` input ([ci_model.rs](/srv/worktrees/pr96/src/effects/tests/ci_model.rs:138), [workflow.rs](/srv/worktrees/pr96/src/effects/tests/workflow.rs:855)). Unlike every hosted job, `ci_test_windows_job_complaints` never calls `toolchain_complaints` ([workflow.rs](/srv/worktrees/pr96/src/effects/tests/workflow.rs:721)).
>
>    Inserting this after checkout is therefore accepted:
>
>    ```yaml
>    - uses: dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c
>      with:
>        toolchain: 1.96.0
>        components: clippy
>    ```
>
>    The suite then runs on 1.96 rather than the curated image’s 1.97 toolchain. A Windows test enabled only on 1.97+ is compiled but not executed by hosted jobs and omitted on the guest, producing a green aggregate. This directly disproves the PR claim that toolchain inputs, positions, and every action input value are pinned as equalities.
>
> 4. **P2 — The workspace convention check still is not a TOML-aware check.** [tests.rs](/srv/worktrees/pr96/src/effects/tests.rs:1713) only recognizes a line whose trimmed text ends exactly in `]`. Valid TOML such as:
>
>    ```toml
>    [workspace] # workspace selection
>    ```
>
>    or root dotted keys such as `workspace.default-members = [...]` passes undetected. This repeats the same spelling-enumeration defect the PR says was repaired. Although documented as a convention check rather than a security boundary, its stated behavior remains false.
>
> 5. **P2 — The immutable decision record already contains stale evidence.** It records 1770 tests and 1760 library tests ([decision record](/srv/worktrees/pr96/decisions/2026-09-01-self-hosted-windows-test-leg.md:141)), while the exact-head job reported 1761 library plus 10 binary tests, totaling 1771 ([exact-head job](https://github.com/eventloops/upstroke/actions/runs/33627375290/job/100238284908)). The PR body has the corrected 1771, but the new dated record—immutable after landing—does not.
>
> I found no new production `unwrap`/`expect`, `anyhow`, or non-`std::path` violation; the blockers are correctness and overstated contract claims.
>
> VERDICT: CHANGES_REQUIRED

## Disposition

Every finding of passes 1 through 8 carries a row in the pull request's ledger
with its own failure sequence and the guard that now refuses it, or the reason
it was rejected. In summary, by pass:

**Pass 1** — the hosted Windows legs ran only `cargo check` and Clippy, so
nothing on GitHub's current stable code-generated or linked the Windows tree;
the oracle never read a checkout step's inputs, so `ref: master` on the
self-hosted job was accepted; and the disabled-job mutation documented an
escape the aggregate already refuses.

**Pass 2** — the witness added for the first of those was `cargo test
--no-run`, which links only test-profile artifacts and so never links a
`#[cfg(all(windows, not(test)))]` body in a binary; the oracle pinned no
`run:` script, so a step could fetch and check out `master` ahead of the pinned
command; the witness carrier's own checkout was unbound; and the renamed-away
mutation claimed a false green the aggregate's derived `needs` already refuses.

**Pass 3** — the toolchain action's `toolchain:` input was unpinned, so a gate
downgraded to the guest's own compiler left current stable compiled by no leg;
`defaults.run.working-directory` was admitted, so a job default directory
points the pinned command at another crate; the workflow's `env:` was guarded
by name rather than pinned whole, so a Cargo target runner bound there builds
every Windows harness and executes none; and the body's timing evidence cited
the build witness at a head that ran the narrower command.

**Pass 4** — the same target-runner escape through a repository
`.cargo/config.toml`, which the oracle cannot see; a `rust-toolchain.toml`,
which outranks the rustup default the pinned action sets and so replaces the
compiler every leg runs; the `permissions:` value unpinned, so `write-all`
hands every build script and test a token that can push; and a body that had
not yet been brought forward to the head it described.

**Pass 5** — a pinned action's inputs were unconstrained, and
`Swatinem/rust-cache` accepts a `cmd-format` that wraps every command it runs,
so a cache step could check out `master` ahead of Clippy and the witness; the
absence guard added for pass 4 was a Rust test, and the `.cargo/config.toml`
it forbids is what stops `cargo test` from running any test; the toolchain
input was pinned but not the step's position; and the body again.

**Pass 6** — the input allowlist checked keys but not values, and the
toolchain action interpolates `components` into a shell line; the shell guard
closed the repository-file route but not the environment route; and the body.

**Pass 7** — package selection was unpinned, so a root `[workspace]` whose
`default-members` name a decoy crate makes every Cargo command test that crate
instead; the shell guard could not enforce its own presence, which is the one
finding rejected, on the reasoning below; the environment half of the guard
read the Ubuntu runner only; and the body's accounting was inexact.

**Pass 8** — the workspace guard matched one spelling and TOML accepts more;
the guest guard read one Cargo home and Cargo reads several; the hosted
witness builds the dev profile and the body said "shipped"; three mutation
rows did not reproduce the escapes they described; and the body. The repair
removed both guards, the oracle function that required them and the two rows
that exercised them, replaced the enumeration with the count on the
self-hosted leg, made the remaining mutation row bind a runner that exists,
narrowed the profile claim to what the command does, and took every count in
the body from `git`.

**Pass 9** — five findings, recorded as `CHANGES_REQUIRED` as written, and
the pull request is completed on the owner's classification under
[2026-09-01](../decisions/2026-09-01-review-effort-rescoped.md) rather than
returned for a tenth pass. One finding carries a reproduction and is repaired
whatever its label: the self-hosted job was never asked by the toolchain
check, so a pinned toolchain action with an allowlisted `toolchain: 1.96.0`
input inserted after its checkout passed every step-level pin. The mutation
row is the reviewer's own snippet, run against the unfixed oracle first and
not refused, then against the repair and refused. The workspace convention
check now asks TOML's parser rather than scanning for a third spelling. Two
findings are overstated claims rather than code defects and are repaired in
the record and the oracle documentation: `cargo build` is the dev profile and
`cargo test` the test profile, so the test-profile compile moves with
execution and the sentence "only execution moves" was wrong; and a wrapper
written to print libtest's summary line clears the count, so "no arrangement
of runners, Cargo homes or manifests produces those lines" was wrong too,
where the honest claim is that a wrapper which executes nothing and says
nothing cannot. The fifth is this record's own counts, which were the
previous head's and are now the reviewed head's. None of the three P1s
reaches the serious bar on the owner's classification: each needs a hostile
candidate to edit the manifest or the guest job, or a wrapper on the owner's
own machine written to forge test output, and then the owner to merge that
diff, which is the boundary `MAINTAINING.md` already places outside the
required contexts. The repair-only delta is disclosed in the pull request
body with both SHAs, touches no workflow file, and is the owner's to verify
at merge.

The one rejection is pass 7's second finding, and it is rejected because it is
right. A pull request that edits `ci.yml` can delete any step `ci.yml` runs,
and the oracle that would notice is a Rust test the same edit can arrange not
to run. No arrangement of in-repository checks closes that loop. That is not a
defect in this change; it is the reason the guards it describes were removed
rather than extended, and the reason the decision record now states the
boundary in as many words.

## What this record does not claim

The oracle reads `.github/workflows/ci.yml`, holds two repository files absent
as a convention, and requires the self-hosted leg to count what it ran. It
does not close the routes it names on the hosted `test` matrix, where they hold
on `master` today, and the pull request's Scope says so. And no document here
should be read as saying CI cannot be subverted by a candidate that edits the
gates. It cannot, by construction: `upstroke-pr-policy` and `upstroke-ci` are
candidate-controlled, which `MAINTAINING.md`'s trust-boundary section states
in as many words. The boundary is that only the owner merges, after an
independent review whose diff includes any change to the gates themselves.
What these nine passes bought is that the diff a reviewer must read to see
such a change is now small and named, rather than diffuse and unpinned.
