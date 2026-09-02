//! What CI runs, on which runners, and with which shells — as constants.
//!
//! **One authority, deliberately.** The workflow oracle in `super::workflow`
//! checks that `.github/workflows/ci.yml` matches these; the cfg census in
//! `super::cfg` decides which of [`CI_TARGETS`]'s runners compiles a predicate.
//! A runner list written down twice is a list where one copy gets updated, and
//! the copy that does not is the one that quietly stops measuring — with
//! nothing failing to say so. Both readers take the same table.
//!
//! The three effect denials are **restored** here rather than inherited.
//! `super`'s module-level allowance exists because that file drives
//! `clippy-driver` over fixtures it has to create; this module names constants
//! and performs no effect at all, so the allowance has no business reaching it.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

/// The workflow every claim in this section is about.
pub(super) const CI_WORKFLOW: &str = ".github/workflows/ci.yml";

/// The command a platform's Clippy leg must run, character for character.
///
/// Character for character is the point. `- run: echo cargo clippy ...` is
/// `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE`'s first escape: it satisfies any
/// `contains` check while the job merely echoes, succeeds, and lets the
/// aggregate settle green over a platform Clippy never examined.
pub(super) const CLIPPY_GATE: &str = "cargo clippy --all-targets --all-features -- -D warnings";

/// The command that executes this file's fixtures, character for character.
pub(super) const TEST_COMMAND: &str = "cargo test --all-targets --all-features";

/// The hosted codegen and link witness for the platform whose tests run
/// self-hosted, character for character.
///
/// `cargo check` and Clippy type-check and stop before codegen, and the
/// self-hosted leg links with the golden image's toolchain, which moves only
/// by re-curation. Without this step nothing on GitHub's current stable ever
/// code-generates or links the Windows tree: a Windows-only codegen or link
/// failure on current stable would pass every hosted leg while the guest, one
/// stable behind, links and passes. `cargo build --all-targets` rather than
/// `cargo test --no-run`: the latter builds only test-profile artifacts, so a
/// `#[cfg(all(windows, not(test)))]` body in a binary is never linked by it,
/// where `--all-targets` links the library and the binaries **and** their
/// unit-test harnesses, plus examples and integration tests. It executes
/// nothing, so the suite's execution stays where the decision record put it.
///
/// In the dev profile. `cargo test` builds the test profile, which inherits
/// from dev and is identical to it until the manifest says otherwise, and that
/// compile happens on the guest with the image's toolchain; so what stays
/// hosted on current stable is the dev-profile compile and link of every
/// target, and what moves with execution is the test-profile compile. A
/// `[profile.test]` override that splits the two is a manifest edit in the
/// diff. A link failure that needs `--release` is not covered here and is not
/// covered on `master` either; `release.yml` builds the shipped artifact with
/// `--release`, and that build failing is a red release, not a green CI over a
/// broken tree.
pub(super) const WINDOWS_BUILD_WITNESS: &str = "cargo build --all-targets --all-features";

/// The script the test jobs run before the suite, character for character:
/// an identity for the commits several tests make.
///
/// Pinned because a `run:` step is a shell with the checkout in front of it.
/// `git fetch origin master && git checkout --detach FETCH_HEAD` appended here
/// would test `master` while the labels, the fields, the shell, the input-free
/// checkout and the pinned test command that follows all still matched.
/// Measured, `MUT-TEST-WINDOWS-RUN-RETARGETED` and `MUT-TEST-RUN-RETARGETED`.
pub(super) const GIT_IDENTITY_SCRIPT: &str = "git config --global user.email \"ci@upstroke.local\"\ngit config --global user.name \"upstroke CI\"\n";

/// Every script a test job may run: the identity script and the suite.
pub(super) const TEST_SCRIPTS: [&str; 2] = [GIT_IDENTITY_SCRIPT, TEST_COMMAND];

/// The scripts the self-hosted leg runs: the identity script, and the suite
/// with the witness that it executed.
pub(super) const TEST_WINDOWS_SCRIPTS: [&str; 2] = [GIT_IDENTITY_SCRIPT, WINDOWS_TEST_WITNESS];

/// The count the self-hosted leg's suite must report before the job is allowed
/// to succeed.
///
/// At `cb8ac1f` it reports 1771 there: 1761 from the lib harness, 10 from the
/// bin one, 0 from the example. The floor sits below that by a margin wide enough that
/// ordinary churn does not touch it and narrow enough that a suite which
/// silently stopped running cannot clear it. Dropping this many Windows tests
/// is a deliberate act, and it edits this number in the same change.
pub(super) const WINDOWS_TEST_FLOOR: u32 = 1700;

/// The self-hosted leg's suite step, character for character: the pinned test
/// command, its exit status, and a count of what libtest reported.
///
/// Every other pin in this contract is an equality over `ci.yml`, and each
/// refuses one named way of arriving at a green job over a suite that never
/// ran. This one refuses the arrival itself. Cargo can be told to compile every
/// harness and execute none by a `target.<triple>.runner` in a repository
/// `.cargo/config.toml`, in `$CARGO_HOME`, in any directory above the checkout,
/// or in the process environment, and it can be told to build no harness of
/// this crate at all by a root `[workspace]` whose `default-members` name
/// another one. Three of those five are written where nothing reading this
/// repository can see them, the fifth has as many spellings as TOML has
/// whitespace, and the list closes only if Cargo stops growing. A count does
/// not enumerate them: a wrapper that executes nothing and prints nothing
/// leaves no `test result: ok.` line, whichever route installed it, and that
/// is every misconfiguration and every lazy substitution. It is not every
/// substitution. A wrapper written to print libtest's summary line clears the
/// floor, and so does a decoy crate whose `harness = false` main prints it;
/// both are a forgery, on the machine or in the diff, and the paragraph below
/// is where forgeries are bounded.
///
/// What it does not do is bound a hostile candidate, and no step in this file
/// does. An edit to `ci.yml` deletes this one as easily as the guards it
/// replaces, and the decision record says so where it says where the boundary
/// actually is. What it bounds is the guest: the machine that now executes this
/// platform's suite is provisioned outside the repository, so its Cargo home
/// and its environment are inputs no reviewer of a diff can check, and this is
/// the leg saying that it ran what it claims to have run.
/// Measured, `MUT-WINDOWS-WITNESS-FLOOR-DROPPED` and
/// `MUT-WINDOWS-WITNESS-COUNT-DROPPED`.
pub(super) const WINDOWS_TEST_WITNESS: &str = "cargo test --all-targets --all-features | Tee-Object -Variable log\nif ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }\n$passed = [int](($log | Select-String -Pattern '^test result: ok\\. (\\d+) passed' | ForEach-Object { [int]$_.Matches[0].Groups[1].Value } | Measure-Object -Sum).Sum)\nif ($passed -lt 1700) { throw \"the suite reported $passed passing tests, below the floor of 1700: Cargo compiled the harnesses and executed almost none of them\" }\n";

/// The formatter gate and the four shell gates the `lint` job runs from the
/// repository root, character for character.
pub(super) const FMT_GATE: &str = "cargo fmt --check";
pub(super) const SHELL_GATES: [&str; 4] = [
    "bash .github/scripts/test-release-record.sh",
    "bash .github/scripts/test-pr-policy.sh",
    "bash .github/scripts/test-pr-ledger-evidence.sh",
    "bash .github/scripts/test-docs-consistency.sh",
];

/// Every script a gate job may run. A union across the three gate jobs rather
/// than a set per job: `lint (macos)` running the formatter would be
/// redundant, not an escape, and the one command that must sit on exactly one
/// job, the Windows build witness, is pinned to its carrier separately.
pub(super) const GATE_SCRIPTS: [&str; 7] = [
    CLIPPY_GATE,
    WINDOWS_BUILD_WITNESS,
    FMT_GATE,
    SHELL_GATES[0],
    SHELL_GATES[1],
    SHELL_GATES[2],
    SHELL_GATES[3],
];

/// The actions a step may `uses:`, each pinned to the commit it was reviewed
/// at. An action off this list is code nobody here reviewed running with a
/// checkout of its own; a floating tag (`@v4`) is that action at whatever
/// commit the tag points to tomorrow. Measured, `MUT-STEP-USES-UNPINNED`.
pub(super) const PINNED_ACTIONS: [&str; 3] = [
    "actions/checkout@11d5960a326750d5838078e36cf38b85af677262",
    "dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c",
    "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
];

/// The inputs each pinned action may be given, by action prefix.
///
/// Pinning the commit says which code runs; this says what it is told to do,
/// and the difference is not academic. `Swatinem/rust-cache` accepts a
/// `cmd-format` input and wraps the commands it runs in it, so a cache step
/// with one input can run `git checkout` against another tree before Clippy and
/// the build witness ever see the candidate -- with the action allowlisted, the
/// checkout input-free, and every `run:` still exact. An input this contract
/// does not model is refused for the same reason an unknown field is.
/// Measured, `MUT-CACHE-CMD-FORMAT`.
pub(super) const ACTION_INPUTS: [(&str, &[&str]); 3] = [
    ("actions/checkout@", &[]),
    ("dtolnay/rust-toolchain@", &["components", "toolchain"]),
    ("Swatinem/rust-cache@", &[]),
];

/// The `components` values the toolchain action may be given.
///
/// The values, not just the key. The action builds a shell command from this
/// input and interpolates it into a later Bash line, so a value carrying
/// `;git fetch origin master && git checkout --detach FETCH_HEAD;` installs
/// Clippy, checks out another tree, and exits zero — through an allowlisted
/// action at a pinned commit with an allowlisted input name. Measured,
/// `MUT-COMPONENTS-INJECTED`.
pub(super) const TOOLCHAIN_COMPONENTS: [&str; 2] = ["clippy", "rustfmt, clippy"];

/// The job that runs those fixtures for the one platform whose test execution
/// left GitHub's runners, and the labels it must run on -- exactly.
///
/// The Windows suite is spawn- and worktree-heavy, and on `windows-latest` it
/// was the whole CI wall clock: a median of 12.5 minutes and a tail of 21 on
/// identical code, because the harness varied 535-1154 s with the host the
/// runner landed on. It runs instead on an ephemeral self-hosted guest -- a
/// throwaway overlay of a frozen image, one job per boot, registered with a
/// single-use just-in-time config -- in about two and a half minutes
/// (`decisions/2026-09-01-self-hosted-windows-test-leg.md`).
///
/// The labels are an equality because a looser `runs-on:` is a different
/// machine: `[self-hosted, windows]` admits any Windows runner the account ever
/// registers, and the third label is what names the curated image. The
/// platform's Clippy and MSRV legs stay on [`CI_TARGETS`]'s `windows-latest`,
/// which is why that entry is unchanged: GitHub's runner is still the witness
/// that compiles every `#[cfg(windows)]` body, and through
/// [`WINDOWS_BUILD_WITNESS`] the one that code-generates and links it on
/// current stable, shipped binaries and test harnesses alike.
pub(super) const TEST_WINDOWS_JOB: &str = "test-windows";
pub(super) const TEST_WINDOWS_LABELS: [&str; 3] = ["self-hosted", "windows", "winguest"];

/// The [`CI_TARGETS`] runner whose tests run in [`TEST_WINDOWS_JOB`] rather than
/// in the `test` matrix. Its shell and cfg valuations carry over: the guest
/// carries PowerShell 7, so a `run:` step resolves to `pwsh` there exactly as
/// on `windows-latest`, and it builds the same MSVC tuple.
pub(super) const SELF_HOSTED_TEST_PLATFORM: &str = "windows-latest";

/// The job that holds this crate to the floor it publishes, and the command it
/// must run -- character for character, for `--locked`'s sake.
///
/// `--locked` is the load-bearing flag and the reason this is a pin rather than
/// a prefix. Without it Cargo may resolve `Cargo.lock` forward, and this
/// manifest carries two exact-version pins placed for precisely that hazard:
/// `globset =0.4.19`, because 0.4.20 raised its MSRV to 1.88, and
/// `yaml-rust2 =0.12.0`, because 0.12.0 declares 1.85.0 -- this crate's floor
/// exactly, so its next minor is free to leave the contract. An unlocked MSRV
/// leg compiles a dependency set no release ships and reports green over a floor
/// it never tested. `CODING_STANDARDS.md` §2 and `CONTRIBUTING.md` both publish
/// the command with the flag.
pub(super) const MSRV_JOB: &str = "msrv";
pub(super) const MSRV_COMMAND: &str = "cargo check --locked --all-targets --all-features";

/// The workflow-scope compiler flags, and the two names that decide what any
/// compilation in this workflow actually sees.
///
/// `CODING_STANDARDS.md` §11 makes this load-bearing rather than a convenience.
/// `unfulfilled_lint_expectations` is warn-by-default, so an `#[expect(...)]` on
/// a rustc lint "retires a suppression only where warnings are promoted to
/// errors. `ci.yml` sets `RUSTFLAGS: -D warnings` at workflow scope, so today
/// that is every leg -- which means narrowing it to a single job would silently
/// take the self-retirement guarantee with it." The word in that sentence this
/// contract answers is *silently*: nothing read the setting until now.
///
/// The pin is an equality because a `contains` reading is exactly what fails
/// here. `-D warnings -A clippy::disallowed_methods` contains `-D warnings` and
/// switches off the effect denylist this whole file exists to enforce.
///
/// `CARGO_ENCODED_RUSTFLAGS` is modelled because *effective* is the claim, not
/// *declared*. Cargo reads it in preference to `RUSTFLAGS` and ignores
/// `RUSTFLAGS` entirely when it is set, so binding it anywhere in this workflow
/// is the same defect as rewriting the value -- with the pinned line left in
/// place to read past.
pub(super) const RUSTFLAGS_KEY: &str = "RUSTFLAGS";
pub(super) const RUSTFLAGS_VALUE: &str = "-D warnings";
pub(super) const ENCODED_RUSTFLAGS_KEY: &str = "CARGO_ENCODED_RUSTFLAGS";

/// The job that aggregates the gates, and the branch-protection context it
/// publishes. `MAINTAINING.md` points the external rule at this one name, so a
/// rename leaves branch protection guarding a context nothing produces.
pub(super) const AGGREGATE_JOB: &str = "merge-gate";
pub(super) const REQUIRED_CONTEXT: &str = "upstroke-ci";

/// The aggregate's script, pinned.
///
/// A pin rather than a family of substring predicates, which is what the
/// standing row forbids growing more of. The enumerated escape is
/// `for gate in LINT LINT_WINDOWS MSRV TEST; do : LINT_MACOS` -- a loop that
/// omits a gate while a `contains` check for the omitted name still passes --
/// and a pin refuses it outright. The pin is not its own oracle: the loop's gate
/// list is re-derived from `needs:` below and compared, so this literal being a
/// faithful copy is itself checked against the job graph.
pub(super) const AGGREGATE_SCRIPT: &str = r#"failed=0
for gate in LINT LINT_WINDOWS LINT_MACOS MSRV TEST TEST_WINDOWS; do
  result_var="${gate}_RESULT"
  result="${!result_var}"
  if [[ "$result" != "success" ]]; then
    echo "$gate did not succeed: $result" >&2
    failed=1
  fi
done
exit "$failed"
"#;

/// The fields a platform Clippy job may declare -- an equality, not a denylist.
///
/// `if:` and `continue-on-error:` are absent by construction rather than by
/// enumeration, which is the difference between this and the whitespace-
/// normalised `if: false` search it replaces: that search could not refuse
/// `if: ${{ false }}`, an `env`-driven expression, or any disabling form nobody
/// had thought of. An unknown field is refused, so the next one costs a review
/// rather than a finding.
pub(super) const GATE_JOB_FIELDS: [&str; 4] = ["name", "runs-on", "steps", "timeout-minutes"];

/// The fields a step of a *gate* job may declare. Same reasoning, and `env:` is
/// absent for a reason of its own: a step-level environment is enough to
/// retarget the compile the gate performs -- `CARGO_BUILD_TARGET` on the macOS
/// leg lints a target no `#[cfg(target_os = "macos")]` body belongs to, while
/// the `run:` scalar still matches character for character. Measured,
/// `MUT-GATE-STEP-RETARGETED`.
pub(super) const STEP_FIELDS: [&str; 5] = ["name", "run", "shell", "uses", "with"];

/// The fields the aggregate's one step may declare. It is the only step in this
/// contract that needs an environment, and the mapping is pinned key by key.
pub(super) const AGGREGATE_STEP_FIELDS: [&str; 4] = ["env", "name", "run", "shell"];

/// The fields the aggregate declares. `if:` is *required* here and pinned to
/// `always()` below: without it a failed dependency skips the aggregate, which
/// leaves the required context missing rather than red.
pub(super) const AGGREGATE_JOB_FIELDS: [&str; 6] =
    ["if", "name", "needs", "runs-on", "steps", "timeout-minutes"];

/// The fields the job that runs these fixtures declares.
pub(super) const TEST_JOB_FIELDS: [&str; 5] =
    ["name", "runs-on", "steps", "strategy", "timeout-minutes"];

/// The fields the self-hosted test job declares: the `test` job's set without a
/// `strategy:`, since one runner needs no matrix. `if:` and `continue-on-error:`
/// are absent by construction, as everywhere in this contract.
pub(super) const TEST_WINDOWS_JOB_FIELDS: [&str; 4] =
    ["name", "runs-on", "steps", "timeout-minutes"];

/// The fields the MSRV leg declares. The same shape as the `test` job, and named
/// separately because the reason one of its absences matters is its own.
///
/// `continue-on-error:` is the field this refuses that nothing else would have.
/// A *disabled* leg (`if: false`) still reports `skipped` to the aggregate,
/// whose loop demands `success` and fails; an *absolved* leg reports `success`
/// after its check failed, so the aggregate reads success, `upstroke-ci` settles
/// green, and the floor `CODING_STANDARDS.md` §2 publishes went unverified on
/// all three platforms at once.
pub(super) const MSRV_JOB_FIELDS: [&str; 5] =
    ["name", "runs-on", "steps", "strategy", "timeout-minutes"];

/// The one field a job -- or the workflow itself -- may declare in addition to
/// its required set.
///
/// `defaults:` is optional rather than forbidden because forbidding it would
/// make the effective-shell resolution below unreachable, and an unreachable
/// resolution is an untested one. It is modelled instead: declared, its shape is
/// checked and the shell it resolves to is compared against what this contract
/// expects for that runner.
pub(super) const OPTIONAL_DEFAULTS_FIELD: [&str; 1] = ["defaults"];

/// The workflow's own top-level fields: required, then the same optional one.
pub(super) const WORKFLOW_FIELDS: [&str; 6] =
    ["concurrency", "env", "jobs", "name", "on", "permissions"];

/// The fields a `defaults:` mapping and its `run:` mapping may declare.
///
/// `working-directory:` is **not** among them. GitHub applies a job- or
/// workflow-level default directory to every `run:` step, so
/// `working-directory: ci-pass` beside a trivial `ci-pass/Cargo.toml` makes the
/// pinned test command test that empty crate -- with the labels, the shells,
/// the checkout and the command all still matching character for character.
/// No job here needs one, so the field is refused rather than modelled.
/// Measured, `MUT-DEFAULTS-WORKING-DIRECTORY`.
pub(super) const DEFAULTS_FIELDS: [&str; 1] = ["run"];
pub(super) const DEFAULTS_RUN_FIELDS: [&str; 1] = ["shell"];

/// The workflow's `env:` mapping, pinned whole.
///
/// An equality rather than a guard per name. [`RUSTFLAGS_KEY`] and
/// [`ENCODED_RUSTFLAGS_KEY`] were guarded by name because they decide what the
/// compiler sees; but a *name this contract never thought of* decides whether
/// the compiled thing runs at all. `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER`
/// pointed at a script that exits zero makes `cargo test` build every harness
/// and execute none, on the Windows target only, which no Unix leg would
/// notice. Cargo documents that setting as applying to `cargo test`. Pinning
/// the whole map refuses every such name at once, including the ones nobody
/// here has thought of yet. Measured, `MUT-WORKFLOW-ENV-TARGET-RUNNER`.
pub(super) const WORKFLOW_ENV: [(&str, &str); 2] = [
    ("CARGO_TERM_COLOR", "always"),
    (RUSTFLAGS_KEY, RUSTFLAGS_VALUE),
];

/// The workflow's `permissions:` mapping, pinned whole.
///
/// The trust story rests on this one binding: the job token is read-only, so a
/// candidate's build scripts and tests cannot push. `permissions: write-all`,
/// or a `contents: write` here, hands every step of every job a token that can,
/// and `actions/checkout` leaves that credential configured for the Git
/// commands a test makes afterwards. Destroying the guest afterwards does not
/// recall a push. Requiring the key to *exist* is not the check; the value is.
/// Measured, `MUT-WORKFLOW-PERMISSIONS-WIDENED`.
pub(super) const WORKFLOW_PERMISSIONS: [(&str, &str); 1] = [("contents", "read")];

/// Repository files that outrank what `ci.yml` says, and must therefore not
/// exist while this contract reads only `ci.yml`.
///
/// A `rust-toolchain.toml` overrides the rustup default the pinned toolchain
/// action sets, so every bare `cargo` command in every job would run a compiler
/// the workflow never names -- including the witness that exists to compile
/// current stable. A `.cargo/config.toml` can bind
/// `target.<triple>.runner`, which Cargo applies to `cargo test`: every Windows
/// harness would build and a wrapper would report success without executing
/// one. Both are invisible to a reader of `ci.yml`. `CLAUDE.md` already states
/// the project has no toolchain file and selects toolchains at call sites; this
/// is that convention made enforceable. Adding either file is a deliberate act
/// that must extend this contract in the same change.
///
/// A Rust test holds this, and a Rust test is exactly what a `.cargo/config.toml`
/// runner switches off. That is a real limit and it is why this is not written
/// as a defence: it catches the file committed by accident, on a developer's
/// machine and in CI, and it says in the diff that the convention is deliberate.
/// The leg whose execution left GitHub's runners is covered instead by
/// [`WINDOWS_TEST_WITNESS`], which counts what actually ran.
pub(super) const OVERRIDING_REPO_FILES: [&str; 4] = [
    "rust-toolchain.toml",
    "rust-toolchain",
    ".cargo/config.toml",
    ".cargo/config",
];

/// The toolchain every leg but the MSRV floor installs, and the action that
/// installs it.
///
/// The action is pinned by commit in [`PINNED_ACTIONS`]; this pins its
/// **input**, which is the part that decides which compiler runs. A
/// `lint (windows)` downgraded from `stable` to the guest's own `1.97.1` makes
/// the hosted witness link the same toolchain the self-hosted leg already
/// links, so nothing in CI code-generates the Windows tree on current stable
/// and a build script emitting a bad link directive only on newer rustc goes
/// green. The MSRV leg pins its own floor separately, from the manifest.
/// Measured, `MUT-GATE-TOOLCHAIN-DOWNGRADED`.
pub(super) const TOOLCHAIN_ACTION: &str = "dtolnay/rust-toolchain@";
pub(super) const STABLE_TOOLCHAIN: &str = "stable";

/// The shell keywords GitHub resolves to an interpreter it defines.
///
/// Anything else is a **custom shell**: GitHub builds the command line
/// `<template> {0}` where `{0}` is the file it wrote the script to. So
/// `shell: true` runs `true /path/to/script` -- the step succeeds and the script
/// never executes. On a Clippy gate that is a green job that never linted, and
/// on the aggregate it is a required check that never read a single result.
/// Measured, `MUT-GATE-STEP-CUSTOM-SHELL`.
pub(super) const KNOWN_SHELLS: [&str; 6] = ["bash", "cmd", "powershell", "pwsh", "python", "sh"];

/// The shell the aggregate's script needs. It is written in bash -- `[[ ]]` and
/// `${!name}` are bash, not POSIX sh and not PowerShell.
pub(super) const AGGREGATE_SHELL: &str = "bash";

/// A runner CI uses, the target it compiles, and the shell its `run:` steps get.
///
/// The tuple is the load-bearing half. What discharges a platform's clause is
/// not a job *name* but the target whose `#[cfg(...)]` bodies that job compiles,
/// and [`cfg_regions`] evaluates predicates against the valuations these
/// invocations actually set rather than collecting the names inside them.
pub(super) struct CiTarget {
    /// The `runs-on:` value.
    pub(super) runner: &'static str,
    /// The target tuple that runner's stable toolchain builds by default.
    pub(super) triple: &'static str,
    /// The shell a `run:` step resolves to on this runner when neither the step,
    /// the job nor the workflow declares one. GitHub's documented default is
    /// `bash` on Linux and macOS and `pwsh` on Windows, and it is pinned rather
    /// than assumed because a resolved shell that is not what this contract
    /// expects is a step that may not run the command at all.
    pub(super) default_shell: &'static str,
    /// The `key = "value"` cfg pairs this runner's compilations set. **Every**
    /// key this census models is listed: a key absent from this table is not
    /// set by the invocation, which makes `key = "value"` false rather than
    /// unknown.
    pub(super) keys: &'static [(&'static str, &'static str)],
    /// The bare cfg flags set by every compilation this runner performs.
    pub(super) flags: &'static [&'static str],
    /// The flags set by *some* compilation and not others.
    ///
    /// `cargo clippy --all-targets` and `cargo test --all-targets` compile the
    /// library twice: once as a library, and once as a test harness with `test`
    /// set. Coverage asks whether **some** invocation compiles the body, so the
    /// valuations are enumerated rather than merged -- merging them would make
    /// `all(test, not(test))` look reachable.
    pub(super) per_invocation_flags: &'static [&'static [&'static str]],
}

pub(super) const CI_TARGETS: [CiTarget; 3] = [
    CiTarget {
        runner: "ubuntu-latest",
        default_shell: "bash",
        triple: "x86_64-unknown-linux-gnu",
        keys: &[
            ("target_os", "linux"),
            ("target_family", "unix"),
            ("target_env", "gnu"),
            ("target_arch", "x86_64"),
            ("target_vendor", "unknown"),
            ("target_pointer_width", "64"),
            ("target_endian", "little"),
        ],
        flags: &["unix"],
        per_invocation_flags: &[&[], &["test"]],
    },
    CiTarget {
        runner: "macos-latest",
        default_shell: "bash",
        triple: "aarch64-apple-darwin",
        keys: &[
            ("target_os", "macos"),
            ("target_family", "unix"),
            ("target_env", ""),
            ("target_arch", "aarch64"),
            ("target_vendor", "apple"),
            ("target_pointer_width", "64"),
            ("target_endian", "little"),
        ],
        flags: &["unix"],
        per_invocation_flags: &[&[], &["test"]],
    },
    CiTarget {
        runner: "windows-latest",
        default_shell: "pwsh",
        triple: "x86_64-pc-windows-msvc",
        keys: &[
            ("target_os", "windows"),
            ("target_family", "windows"),
            ("target_env", "msvc"),
            ("target_arch", "x86_64"),
            ("target_vendor", "pc"),
            ("target_pointer_width", "64"),
            ("target_endian", "little"),
        ],
        flags: &["windows"],
        per_invocation_flags: &[&[], &["test"]],
    },
];
