//! Extended notes: `docs/internals/effects/tests/ci_model.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

pub(super) const CI_WORKFLOW: &str = ".github/workflows/ci.yml";

pub(super) const CLIPPY_GATE: &str = "cargo clippy --all-targets --all-features -- -D warnings";

pub(super) const TEST_COMMAND: &str = "cargo test --all-targets --all-features";

pub(super) const WINDOWS_BUILD_WITNESS: &str = "cargo build --all-targets --all-features";

pub(super) const GIT_IDENTITY_SCRIPT: &str = "git config --global user.email \"ci@upstroke.local\"\ngit config --global user.name \"upstroke CI\"\n";

pub(super) const TEST_SCRIPTS: [&str; 2] = [GIT_IDENTITY_SCRIPT, TEST_COMMAND];

pub(super) const TEST_WINDOWS_SCRIPTS: [&str; 2] = [GIT_IDENTITY_SCRIPT, WINDOWS_TEST_WITNESS];

pub(super) const WINDOWS_TEST_FLOOR: u32 = 1700;

pub(super) const WINDOWS_TEST_WITNESS: &str = "cargo test --all-targets --all-features | Tee-Object -Variable log\nif ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }\n$passed = [int](($log | Select-String -Pattern '^test result: ok\\. (\\d+) passed' | ForEach-Object { [int]$_.Matches[0].Groups[1].Value } | Measure-Object -Sum).Sum)\nif ($passed -lt 1700) { throw \"the suite reported $passed passing tests, below the floor of 1700: Cargo compiled the harnesses and executed almost none of them\" }\n";

pub(super) const FMT_GATE: &str = "cargo fmt --check";
pub(super) const SHELL_GATES: [&str; 6] = [
    "bash .github/scripts/test-release-record.sh",
    "bash .github/scripts/test-pr-policy.sh",
    "bash .github/scripts/test-pr-ledger-evidence.sh",
    "bash .github/scripts/test-docs-consistency.sh",
    "bash .github/scripts/test-internals-notes.sh",
    "bash .github/scripts/test-pr-ready-audit.sh",
];

pub(super) const GATE_SCRIPTS: [&str; 9] = [
    CLIPPY_GATE,
    WINDOWS_BUILD_WITNESS,
    FMT_GATE,
    SHELL_GATES[0],
    SHELL_GATES[1],
    SHELL_GATES[2],
    SHELL_GATES[3],
    SHELL_GATES[4],
    SHELL_GATES[5],
];

pub(super) const PINNED_ACTIONS: [&str; 3] = [
    "actions/checkout@11d5960a326750d5838078e36cf38b85af677262",
    "dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c",
    "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
];

pub(super) const ACTION_INPUTS: [(&str, &[&str]); 3] = [
    ("actions/checkout@", &[]),
    ("dtolnay/rust-toolchain@", &["components", "toolchain"]),
    ("Swatinem/rust-cache@", &[]),
];

pub(super) const TOOLCHAIN_COMPONENTS: [&str; 2] = ["clippy", "rustfmt, clippy"];

pub(super) const TEST_WINDOWS_JOB: &str = "test-windows";
pub(super) const TEST_WINDOWS_LABELS: [&str; 3] = ["self-hosted", "windows", "winguest"];

pub(super) const SELF_HOSTED_TEST_PLATFORM: &str = "windows-latest";

pub(super) const MSRV_JOB: &str = "msrv";
pub(super) const MSRV_COMMAND: &str = "cargo check --locked --all-targets --all-features";

pub(super) const RUSTFLAGS_KEY: &str = "RUSTFLAGS";
pub(super) const RUSTFLAGS_VALUE: &str = "-D warnings";
pub(super) const ENCODED_RUSTFLAGS_KEY: &str = "CARGO_ENCODED_RUSTFLAGS";

pub(super) const AGGREGATE_JOB: &str = "merge-gate";
pub(super) const REQUIRED_CONTEXT: &str = "upstroke-ci";

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

pub(super) const GATE_JOB_FIELDS: [&str; 4] = ["name", "runs-on", "steps", "timeout-minutes"];

pub(super) const STEP_FIELDS: [&str; 5] = ["name", "run", "shell", "uses", "with"];

pub(super) const AGGREGATE_STEP_FIELDS: [&str; 4] = ["env", "name", "run", "shell"];

pub(super) const AGGREGATE_JOB_FIELDS: [&str; 6] =
    ["if", "name", "needs", "runs-on", "steps", "timeout-minutes"];

pub(super) const TEST_JOB_FIELDS: [&str; 5] =
    ["name", "runs-on", "steps", "strategy", "timeout-minutes"];

pub(super) const TEST_WINDOWS_JOB_FIELDS: [&str; 4] =
    ["name", "runs-on", "steps", "timeout-minutes"];

pub(super) const MSRV_JOB_FIELDS: [&str; 5] =
    ["name", "runs-on", "steps", "strategy", "timeout-minutes"];

pub(super) const OPTIONAL_DEFAULTS_FIELD: [&str; 1] = ["defaults"];

pub(super) const WORKFLOW_FIELDS: [&str; 6] =
    ["concurrency", "env", "jobs", "name", "on", "permissions"];

pub(super) const DEFAULTS_FIELDS: [&str; 1] = ["run"];
pub(super) const DEFAULTS_RUN_FIELDS: [&str; 1] = ["shell"];

pub(super) const WORKFLOW_ENV: [(&str, &str); 2] = [
    ("CARGO_TERM_COLOR", "always"),
    (RUSTFLAGS_KEY, RUSTFLAGS_VALUE),
];

pub(super) const WORKFLOW_PERMISSIONS: [(&str, &str); 1] = [("contents", "read")];

pub(super) const OVERRIDING_REPO_FILES: [&str; 4] = [
    "rust-toolchain.toml",
    "rust-toolchain",
    ".cargo/config.toml",
    ".cargo/config",
];

pub(super) const TOOLCHAIN_ACTION: &str = "dtolnay/rust-toolchain@";
pub(super) const STABLE_TOOLCHAIN: &str = "stable";

pub(super) const KNOWN_SHELLS: [&str; 6] = ["bash", "cmd", "powershell", "pwsh", "python", "sh"];

pub(super) const AGGREGATE_SHELL: &str = "bash";

pub(super) struct CiTarget {
    pub(super) runner: &'static str,
    pub(super) triple: &'static str,
    pub(super) default_shell: &'static str,
    pub(super) keys: &'static [(&'static str, &'static str)],
    pub(super) flags: &'static [&'static str],
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
