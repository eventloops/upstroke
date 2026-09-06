# `src/effects/tests/workflow.rs`

Extended notes for [`src/effects/tests/workflow.rs`](../../../../src/effects/tests/workflow.rs).

The code is the authority for what it does. These notes started as the module's source prose.
Each code fragment in a heading is an exact source substring. When a heading names an enclosing
item before `›`, find that item first, then the following fragment within it.

## Module

The CI workflow's structural oracle: what is wrong with `ci.yml`, and the
mutations that prove each complaint fires.

Every claim here is an equality over a parsed mapping or an exact scalar
pin, never a `contains` over text — `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE`
is the row that records what a substring reading of this surface misses.
`WORKFLOW_ESCAPES` is the other half: each entry is a document that *does*
the forbidden thing, and a complaint **code** it must be refused as. A
refusal for an unrelated reason is not a refusal of that escape.

The shape being checked against is `super::ci_model`'s, not a second copy of
it. The `#[test]` wrappers that drive these functions stay in `super`: this
module is the oracle, not the harness, and every name in it is deliberately
not a test name.

The three effect denials are **restored** here rather than inherited.
`super`'s module-level allowance exists because that file drives
`clippy-driver` over fixtures it has to create; this module reads two files
and writes none, so the allowance has no business reaching it.

## `pub(super) fn ci_workflow_text() -> String {`

`.github/workflows/ci.yml`, with CRLF collapsed.

The collapse is not cosmetic. This test runs on `windows-latest`, where the
checkout can land CRLF line endings, and every claim below is an equality
against an exact scalar. Normalising here makes the oracle read the same
document on all three runners instead of depending on the scanner's line-break
handling -- the platform-shaped half of a mutation is the half that is only
ever measured on Linux.

## `pub(super) fn parse_workflow(text: &str) -> Result<Yaml, String> {`

Parse a workflow as YAML 1.2, refusing duplicate keys.

Duplicate-key rejection is a property of the parser this crate depends on for
exactly this reason, and
`the_workflow_parser_rejects_duplicate_keys_and_reads_on_as_a_string`
executes it: under last-one-wins every equality below reads the winning entry
and a mutation hides in the loser.

## `pub(super) fn field<'a>(node: &'a Yaml, key: &str) -> Option<&'a Yaml> {`

The value of `key` in a mapping node.

## `pub(super) fn field_names(node: &Yaml) -> BTreeSet<String> {`

Every key of a mapping node. A non-string key is rendered rather than
dropped, so `on:` read as a boolean by a YAML 1.1 parser would show up here
as an unexpected field rather than as a silent absence.

## `fn scalar_set(node: &Yaml) -> Option<BTreeSet<String>> {`

A sequence of scalars as a set, or `None` if the node is not that.

## `fn scalar_map(node: &Yaml) -> Option<BTreeMap<String, String>> {`

A mapping of scalar to scalar, or `None` if the node is not that.

## `fn gate_stem(job: &str) -> String {`

The environment variable the aggregate's loop reads for `job`.

## `fn field_complaints(node: &Yaml, required: &[&str], optional: &[&str]) -> Vec<String> {`

Every field the contract requires but the node does not declare, and every
field it declares that the contract does not know about.

## `fn defaults_shell<'a>(node: &'a Yaml, where_: &str, out: &mut Vec<String>) -> Option<&'a str> {`

The `shell:` a `defaults:` mapping sets, and every way its shape is wrong.

## `fn effective_shell(`

The shell a `run:` step actually executes under.

GitHub resolves it step, then job `defaults.run.shell`, then workflow
`defaults.run.shell`, then the runner's platform default. Reading only the
step is how a workflow-level default silently swaps the interpreter under
every gate at once -- measured, `MUT-WORKFLOW-DEFAULT-SHELL-SWAPPED`.

## `fn shell_complaints(`

Complain unless `step` resolves to exactly `expected`, and unless that is a
shell GitHub defines rather than a command template it will run instead.

## `if field(step, "shell").is_some() && scalar(step, "shell").is_none() {`

A `shell:` key whose value is not a string resolves to nothing at all
here, and this contract would then read the platform default and pass.
YAML reads the bare word `true` as a boolean, so that is not hypothetical.

## `fn stem_collisions(jobs: &BTreeSet<String>) -> BTreeMap<String, Vec<String>> {`

The job ids whose gate stems collide.

`gate_stem` upper-cases and turns `-` into `_`, so `lint-windows` and
`lint_windows` are one variable name. Every collection built from stems is a
map or a set, so a collision **collapses** rather than duplicating: the env
mapping loses an entry, the loop's expected set loses a member, and both
equalities below compare a shorter list against a shorter list and pass. The
collision is therefore checked before anything is derived from the stems.
Measured, `MUT-AGGREGATE-STEM-COLLISION`.

## `fn ci_gate_complaints(doc: &Yaml) -> Vec<String> {`

Every way the parsed workflow fails the gate-wiring contract.

One function so the same contract runs against the real document and against
each mutation in `WORKFLOW_ESCAPES`; an oracle only ever run on conforming
input is an oracle nobody has seen refuse anything. Each complaint opens with
a `[kebab-code]`, and the escape table names the code it must provoke -- so a
mutation that fails for an unrelated reason does not count as refused.

## `fn ci_gate_complaints(doc: &Yaml) -> Vec<String>` › `if scalar(step, "run").is_some() {`

Only `run:` steps have a shell; a `uses:` step runs an action.

## `fn aggregate_complaints(doc: &Yaml, jobs: &Yaml, job_names: &BTreeSet<String>) -> Vec<String> {`

Every way the aggregate fails to make each gate a required one.

## `fn aggregate_complaints(doc: &Yaml, jobs: &Yaml, job_names: &BTreeSet<String>) -> Vec<String> {` › `let mut expected_needs: BTreeSet<String> = job_names.clone();`

The `needs` set is DERIVED: every job but the aggregate itself. A job that
is not needed cannot fail the aggregate, so adding one and forgetting to
wire it is the same defect as dropping one, and this equality refuses both.

## `fn aggregate_complaints(doc: &Yaml, jobs: &Yaml, job_names: &BTreeSet<String>) -> Vec<String> {` › `let collisions = stem_collisions(&expected_needs);`

Before ANY collection is derived from the stems. Two job ids that
normalise to one variable name collapse every set and map built below, and
a collapsed expectation compares equal to a collapsed reality.

## `fn aggregate_complaints(doc: &Yaml, jobs: &Yaml, job_names: &BTreeSet<String>) -> Vec<String> {` › `let on_ubuntu = CI_TARGETS`

The aggregate runs on `ubuntu-latest`, and its script is bash. The check
is on the RESOLVED shell, not on the declaration: a workflow-level
`defaults.run.shell` reaches this step too, and a custom shell would let
the required check pass without reading a single gate result.

## `fn aggregate_complaints(doc: &Yaml, jobs: &Yaml, job_names: &BTreeSet<String>) -> Vec<String> {` › `let env = field(step, "env").and_then(scalar_map);`

The binding, not its existence. `LINT_MACOS_RESULT: ${{ needs.lint-windows
.result }}` is a copy-paste that satisfies any existence check, reads a
passing sibling, and reports the required context green over a red leaf.
Measured: an earlier version of this assertion accepted exactly that.

## `fn aggregate_complaints(doc: &Yaml, jobs: &Yaml, job_names: &BTreeSet<String>) -> Vec<String> {` › `let expected_stems: BTreeSet<String> = wired.iter().map(|job| gate_stem(job)).collect();`

Re-derived from `needs`, so the pin above is checked against the job graph
rather than trusted as a copy. `for gate in LINT LINT_WINDOWS MSRV TEST; do
: LINT_MACOS` is the enumerated escape: it satisfies a search for the
omitted name while the loop never reads it.

## `pub(super) fn ci_test_job_complaints(doc: &Yaml) -> Vec<String> {`

Every way the job that executes this file's fixtures fails its contract.

The predecessor read the job's text with comments stripped and asked whether
the word `clippy` appeared on a `components:` line and whether the file
contained the test command anywhere. Both are satisfied by an `echo`, and the
strip existed only because the job's nine-line comment spelled the needle --
`PR4-CENSUS-COMMENT-ORACLE` in the test whose purpose is to answer "which
command runs this?". A parsed document has no comments in it at all, so that
class is gone by construction rather than by a strip whose bite had to be
asserted.

## `pub(super) fn ci_test_job_complaints(doc: &Yaml) -> Vec<String> {` › `if scalar(step, "run").is_some() {`

This job is a matrix, so each `run:` step resolves a shell once per
hosted runner. Each must be the platform default: a workflow-level
default swaps every one of them at once, which is the mutation the
step-only reading could not see.

## `pub(super) fn ci_test_job_complaints(doc: &Yaml) -> Vec<String> {` › `let expected_runners: BTreeSet<String> = CI_TARGETS`

The matrix is the platform half of the same claim, and it is compared
against the same derived runner set the Clippy legs are, less the one
platform whose suite runs self-hosted: a fixture that runs on one
platform proves nothing about the other.

## `pub(super) fn ci_test_job_complaints(doc: &Yaml) -> Vec<String> {` › `match field(job, "strategy") {`

The WHOLE strategy mapping, not just `matrix.os`. `exclude:` removes
combinations that `os:` still lists, so a job that names three platforms
can run on one while every check that reads `os:` passes; `include:` can
add a fourth nobody declared, and `max-parallel`/`fail-fast` are the rest
of what a strategy may say. An equality over the field set refuses all of
them, including the ones GitHub adds next. Measured,
`MUT-TEST-MATRIX-EXCLUDED`.

## `pub(super) fn ci_test_job_complaints(doc: &Yaml) -> Vec<String> {` › `let toolchains: Vec<&Yaml> = steps_of(job)`

`clippy` is a TEST dependency of this job, not only a lint one:
`every_declared_effect_denial_refuses_for_the_reason_it_declares` drives
`clippy-driver` over one fixture per resolution shape, and
`dtolnay/rust-toolchain` installs the minimal profile. Measured, mutation
`MUT-CI-STOPS-INSTALLING-CLIPPY`.

## `fn hosts_tests(target: &CiTarget) -> bool {`

Whether a `CI_TARGETS` runner hosts its platform's tests in the `test`
matrix -- every platform but the one whose suite is self-hosted.

## `pub(super) fn ci_test_windows_job_complaints(doc: &Yaml) -> Vec<String> {`

Every way the self-hosted Windows job fails its contract.

The same shape as `ci_test_job_complaints` without the matrix, and with
the one thing that job cannot say: which machine. A hosted runner is named
by a scalar `runs-on:`; a self-hosted one by the set of labels a runner must
carry, and the set is compared whole -- a subset admits every Windows
machine the account registers, and a scalar `windows-latest` is the leg this
contract retired coming back with every step still matching.

No toolchain step is required: the guest's image carries `clippy-driver`
for the fixtures, and the decision record binds re-curation to that claim,
which a document parser cannot check.

## `pub(super) fn ci_test_windows_job_complaints(doc: &Yaml) -> Vec<String> {` › `for (index, step) in steps_of(job).iter().enumerate() {`

The image carries the compiler this leg runs, and re-curation is how it
moves. So an install step here is not a convenience: it selects a
toolchain the workflow never curated, and the hosted legs' pin on
`stable` never reaches this job, since `toolchain_complaints` is asked
about the jobs that install and this one does not. The action and its
`toolchain` input are allowlisted for those jobs, which is why the
step-pin check alone accepted it. Zero installs, as an equality.
Measured, `MUT-TEST-WINDOWS-TOOLCHAIN-INSTALLED`.

## `pub(super) fn ci_test_windows_job_complaints(doc: &Yaml) -> Vec<String> {` › `let running = steps_of(job)`

The whole step, not the command inside it. The command says which suite
Cargo was asked for; the lines after it are what say the suite ran, and
an equality over the step is what stops those lines being dropped, or
their floor lowered to a number an unexecuted suite clears. Measured,
`MUT-WINDOWS-WITNESS-COUNT-DROPPED` and `MUT-WINDOWS-WITNESS-FLOOR-DROPPED`.

## `fn checkout_complaints(job: &Yaml, named: &str, code: &str) -> Vec<String> {`

Every way a test job's checkout points the suite at a tree other than the
head under test.

`actions/checkout` with no inputs checks out the event's own ref: for a
pull request, the candidate merged onto its base. Any input -- `ref:`,
`repository:`, `path:` -- selects something else, and `ref: master` there
tests `master` while every other leg reads the candidate. `STEP_FIELDS`
admits `with:` because the toolchain and cache actions need it; on a
checkout step it is refused whole. Measured, `MUT-TEST-WINDOWS-CHECKOUT-REF`
and `MUT-TEST-CHECKOUT-REF`.

## `fn step_pin_complaints(job: &Yaml, named: &str, code: &str, scripts: &[&str]) -> Vec<String> {`

Every way a step of a modelled job is something this contract did not pin.

The field and shell checks say how a step runs; this says what. A `run:`
step whose script is not in the job's pinned set can move the checkout --
`git fetch origin master && git checkout --detach FETCH_HEAD` -- before the
pinned command runs, so that command runs against another tree while the
labels, fields, shell and input-free checkout all still match. A `uses:`
step off `PINNED_ACTIONS` is code nobody here reviewed, with a checkout
of its own. Measured, `MUT-TEST-WINDOWS-RUN-RETARGETED`,
`MUT-TEST-RUN-RETARGETED`, `MUT-GATE-RUN-RETARGETED`,
`MUT-MSRV-RUN-RETARGETED` and `MUT-STEP-USES-UNPINNED`.

## `fn step_pin_complaints(job: &Yaml, named: &str, code: &str, scripts: &[&str]) -> Vec<String> {` › `let Some((_, allowed)) = ACTION_INPUTS`

The commit says which code runs; the inputs say what it is told to
do. `rust-cache`'s `cmd-format` wraps the commands it runs, so one
input on an allowlisted action at a pinned commit is enough to put
`git checkout` in front of every gate in the job.

## `fn step_pin_complaints(job: &Yaml, named: &str, code: &str, scripts: &[&str]) -> Vec<String> {` › `if uses.starts_with(TOOLCHAIN_ACTION) {`

The values, not only the key names. The toolchain action builds shell
text from `components` and interpolates it into a Bash line, so an
allowlisted key with an unpinned value is a command the candidate
chose running inside an action this contract calls pinned.

## `fn toolchain_complaints(`

Every way a job installs a compiler other than the pinned one.

`PINNED_ACTIONS` pins the action by commit; this pins the input that
decides which compiler it installs. A gate downgraded from `stable` to the
version the golden image already carries leaves no leg compiling on current
stable, and every other pin in this contract still matches. The MSRV leg is
not checked here: it pins its own floor against the manifest, in
`ci_msrv_job_complaints`. Measured, `MUT-GATE-TOOLCHAIN-DOWNGRADED`.

## `out.push(format!(`

Zero is legitimate only where the image carries the toolchain, which
is the self-hosted leg, and that job is not checked here.

## `if *install != 1 {`

Position, not merely presence. The action installs a toolchain and makes
it the rustup default, so a step above it runs whatever the runner image
happened to preinstall -- during a release rollout that is the previous
stable, and the witness that exists to compile *current* stable would be
compiling something else while every other pin still matched.

## `fn workflow_env_complaints(doc: &Yaml) -> Vec<String> {`

Every way the workflow's own `env:` is not the pinned mapping.

The whole map, as an equality. A guard per name can only refuse names
somebody thought of, and the dangerous ones are the others: a Cargo target
runner bound here makes `cargo test` build every harness and run none, on
one target, which no other platform's leg would notice. Refusing the map
wholesale refuses that class rather than that instance.
Measured, `MUT-WORKFLOW-ENV-TARGET-RUNNER`.

## `fn workflow_permissions_complaints(doc: &Yaml) -> Vec<String> {`

Every way the workflow's `permissions:` is not the pinned mapping.

The whole map, as an equality, for the reason the `env:` map is pinned
whole: the field's presence is not the property that matters, its value is.
A widened token is available to every build script and test the candidate
ships, with the checkout's credential still configured, and no guest
teardown recalls what it pushed.
Measured, `MUT-WORKFLOW-PERMISSIONS-WIDENED`.

## `pub(super) fn ci_windows_build_witness_complaints(doc: &Yaml) -> Vec<String> {`

Every way the hosted Windows codegen witness fails its contract.

The self-hosted leg executes the Windows suite with the golden image's
toolchain, which moves only by re-curation. `cargo check` and Clippy on
`windows-latest` type-check current stable and stop before codegen, so
without this witness nothing on GitHub's current stable ever code-generates
or links the Windows tree: a Windows-only codegen or link failure there
would pass every hosted leg while the guest, one stable behind, links and
passes. `WINDOWS_BUILD_WITNESS` builds every test binary and executes
none. It lives in the Windows Clippy gate's job, whose field set and shells
the gate contract pins; this pins the command, exactly once, on exactly one
hosted Windows job. Measured, `MUT-WINDOWS-BUILD-WITNESS-*`.

## `pub(super) fn ci_windows_build_witness_complaints(doc: &Yaml) -> Vec<String> {` › `if !steps_of(job)`

The carrier must be the Windows Clippy gate's job: that job's fields,
shells, scripts and checkout are pinned by the gate contract, so the
witness inherits every one of those pins. A witness on some other
`windows-latest` job would be a pinned command on an unpinned job.

## `pub(super) fn declared_rust_version() -> String {`

`Cargo.toml`'s `[package] rust-version`, as it is written there.

## `pub(super) fn declared_msrv_toolchain() -> String {`

The toolchain name the MSRV leg must install, derived from the manifest.

Derived rather than transcribed: a literal `"1.85.0"` here would make this
section its own oracle for the one fact it exists to hold, and a bump to
`rust-version` would leave the leg checking a floor the crate no longer
publishes.

The pin is exact, which `.github/scripts/test-docs-consistency.sh`'s C2 is
deliberately not: C2 accepts `rust-version` "or a patch release of it", so it
reads `toolchain: 1.85` as agreement. `dtolnay/rust-toolchain` resolves a
two-component name to the newest patch in the series, which is not the
`cargo +1.85.0` that `CODING_STANDARDS.md` §2, `CONTRIBUTING.md` and
`CLAUDE.md` all publish.

## `pub(super) fn three_component(version: &str) -> String {`

`1.85` as the toolchain name `1.85.0`; anything else unchanged.

Unchanged rather than repaired. A manifest value this does not understand
must reach the equality below and fail there with both strings quoted, not be
normalised into agreement with whatever the workflow happens to say.

## `pub(super) fn ci_msrv_job_complaints(doc: &Yaml) -> Vec<String> {`

Every way the MSRV leg fails to check the floor this crate publishes.

Nothing above this function reaches that job. `ci_gate_complaints` selects
a job by its `runs-on:` *and* a step whose `run:` is `CLIPPY_GATE`, and
`msrv` matches neither -- it runs on `${{ matrix.os }}` and it runs
`cargo check`. So until this existed the only structural claim on the MSRV
leg was that the aggregate needs a job with that id: its matrix could be
narrowed to one runner or hollowed out with `exclude:`, its command could
lose `--locked` or become an `echo`, and its step could be absolved, with
every check in this section still passing.

One claim here is held elsewhere too, and its neighbour states its own
limits. `.github/scripts/test-docs-consistency.sh` C2 compares the toolchain
scalar with `rust-version` by grepping a text block; that file's `WITHDRAWN,
DELIBERATELY` note records that the gate makes "NO claim about which cargo
commands CI runs, whether CI executes them", because a command "can be
present and skipped (`if: false`)". A parsed document is what lets that claim
come back as an equality -- the same trade the rest of this section made, and
the reason `MUT-CI-CARGO-TEST-STEP-SKIPPED` is already a kill here rather
than history.

## `pub(super) fn ci_msrv_job_complaints(doc: &Yaml) -> Vec<String> {` › `if scalar(step, "run").is_some() {`

A matrix job, so each `run:` step resolves a shell once per runner, and
a workflow-level default reaches all three of them at once.

## `pub(super) fn ci_msrv_job_complaints(doc: &Yaml) -> Vec<String> {` › `out.extend(toolchain_complaints(job, MSRV_JOB, "msrv-toolchain", None));`

Position only: this leg installs the manifest's floor, not `stable`, and
the value is checked against `Cargo.toml` above.

## `pub(super) fn ci_msrv_job_complaints(doc: &Yaml) -> Vec<String> {` › `let expected_runners: BTreeSet<String> = CI_TARGETS`

The platform half, compared against the same derived runner set the Clippy
legs and the `test` matrix are. A floor is a per-platform fact: a
dependency that raises its MSRV behind a `cfg` fails on that target only.

## `pub(super) fn ci_msrv_job_complaints(doc: &Yaml) -> Vec<String> {` › `let install_at = steps_of(job).iter().position(|step| {`

Order, not merely presence. `dtolnay/rust-toolchain` selects the toolchain
for the steps that FOLLOW it, so a check above it compiles on whatever the
runner image preinstalled -- stable -- while both steps are present, both
are exact, and every equality above passes. Measured,
`MUT-MSRV-CHECK-BEFORE-TOOLCHAIN`.

The install step is located by the toolchain it installs, not merely by the
action it uses: an install of something other than the derived floor is
already refused above, and pairing the order claim to the same exact value
keeps the two from drifting apart.

## `pub(super) fn rustflags_complaints(doc: &Yaml) -> Vec<String> {`

Every way the workflow-scope `-D warnings` fails to reach a compilation.

Two claims. The first is the pin: the workflow's own `env:` binds
`RUSTFLAGS_KEY` to exactly `RUSTFLAGS_VALUE`. The second is what makes
the first *effective*: no job and no step rebinds that name, and nothing
anywhere binds `ENCODED_RUSTFLAGS_KEY`, which Cargo reads in preference to
it.

The override scan walks every job and every step rather than the jobs this
contract models, and that is why it is written separately from the field
sets. On today's document it is defence in depth -- `GATE_JOB_FIELDS`,
`TEST_JOB_FIELDS`, `MSRV_JOB_FIELDS`, `AGGREGATE_JOB_FIELDS` and
`STEP_FIELDS` already refuse an `env:` almost everywhere it could go. But the
`msrv` leg had no field set at all until this change, the aggregate's step is
the one step in this contract that is *allowed* an `env:`, and a job added
tomorrow has no field set until someone writes one. A rebinding anywhere is
refused by this scan on its own, which is what
`the_workflow_scope_rustflags_pin_refuses_weakening_and_every_override`
measures on documents the rest of the contract does not reach.

## `pub(super) fn rustflags_complaints(doc: &Yaml) -> Vec<String> {` › `for key in field_names(env) {`

Every other guarded binding at workflow scope, case-insensitively.
Two distinct defects share this arm: `CARGO_ENCODED_RUSTFLAGS`,
which Cargo reads instead of the pinned name, and a case variant of
`RUSTFLAGS` itself, which collides with the pinned line on Windows
and does nothing on Linux. The pinned key is skipped because the
match above already decided it.

## `fn guarded_env_key(key: &str) -> Option<&'static str> {`

The canonical guarded name `key` is, ignoring case, or `None`.

Whole-key equality, never a substring. `RUSTFLAGS_EXTRA`, `RUST_FLAGS` and
`CARGO_TERM_COLOR` are unrelated variables and a `contains` reading would
refuse bindings that never touch the warning policy.

Case-insensitive, because the environment on `windows-latest` is. GitHub
merges `env:` mappings by exact key and hands the result to the runner, which
sets them into a process environment where `rustflags` and `RUSTFLAGS` are one
variable. A lowercase job-level binding is therefore inert on Linux and
authoritative on Windows -- exactly the half of a mutation that only ever gets
measured on Linux, and the reason this comparison is not `==`.

## `fn env_name_tokens(script: &str) -> BTreeSet<String> {`

Every `[A-Za-z0-9_]` run of `script`, so a variable name is matched whole.

A `run:` scalar is an opaque script rather than a mapping, so the finest
granularity available over it is a token. Tokens are still enough to keep the
discipline the rest of this section keeps: `RUSTFLAGS_EXTRA` is one token and
is not `RUSTFLAGS`, which a `contains` reading could not tell apart.

## `fn writes_the_job_env_file(script: &str) -> bool {`

Whether `script` names the job-scoped environment file, in any of its forms.

`$GITHUB_ENV`, `${GITHUB_ENV}`, `$env:GITHUB_ENV`, `%GITHUB_ENV%` and
`${{ github.env }}` are one file, reached from bash, pwsh and cmd
respectively. A line written to it becomes an environment variable for every
later step of the same job.

## `fn rustflags_script_complaints(script: &str, named: &str) -> Vec<String> {`

Every way a `run:` step reaches the warning policy from inside its script.

The `env:` mappings this contract compares are declarations. A `run:` scalar
is not, and one line of one --
`echo "RUSTFLAGS=-A warnings" >> "$GITHUB_ENV"` -- rebinds the variable for
**every later step of the same job** while the document declares an `env:`
nowhere. Every field-set equality passes, the pinned workflow line is
untouched, and the Cargo steps that follow compile under a policy no mapping
in this file states. `MUT-RUSTFLAGS-PERSISTED-VIA-GITHUB-ENV` and its three
siblings are that line in bash, in a bash heredoc, in PowerShell and through
the `${{ github.env }}` expression.

Position is deliberately not a precondition. Refusing the write wherever it
appears is strictly stronger than refusing it only where a Cargo step
follows -- which a reorder would defeat -- and no leg of this workflow has a
benign reason to name the variable at all.

**What a token scan over an opaque script can and cannot do**, stated rather
than left to be discovered. It refuses every form that spells the name,
including forms that are not writes at all: `RUSTFLAGS=-A warnings cargo
build` scopes the flags to one command without touching the env file, and it
is refused too, because the policy is set once at workflow scope and a script
that names it is doing something this contract has no model for. It does
**not** refuse a script that assembles the name from pieces. That residual is
the same one `AGGREGATE_SCRIPT`'s pin carries: a script is not a document, and
the honest bound is the one written down.

## `fn rustflags_override_complaints(node: &Yaml, named: &str) -> Vec<String> {`

The guarded names a node's own `env:` may not bind, wherever that node sits.

Keys are matched case-insensitively and whole. A `RustFlags:` binding is a
no-op on the two Unix legs and the authoritative value on `windows-latest`,
so a case-sensitive reading refuses it on no platform and a substring reading
would refuse `RUSTFLAGS_EXTRA` on all three.

## `pub(super) fn workflow_complaints(doc: &Yaml) -> Vec<String> {`

Every audit, so a mutation is refused by the contract as a whole.

## `pub(super) fn complaint_codes(complaints: &[String]) -> BTreeSet<String> {`

The `[kebab-code]` each complaint opens with.

## `pub(super) struct WorkflowEscape {`

An escape this oracle must refuse, as a mutation of the real workflow.

Every row is a change that the substring oracle this section replaces
accepted, or that its own doc comment enumerated as still open. The first two
are `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE`'s; the `MUT-CI-*` names are
kept from `.github/scripts/test-docs-consistency.sh`, which recorded them as
history when that gate withdrew its claim over this surface -- a parsed
document is what lets the claim come back as an equality.

## `pub(super) struct WorkflowEscape` › `pub(super) name: &'static str,`

The name this escape is recorded under.

## `pub(super) struct WorkflowEscape` › `pub(super) escape: &'static str,`

What passes while the gate does not run.

## `pub(super) struct WorkflowEscape` › `pub(super) job: Option<&'static str>,`

The job whose block the anchor must appear in exactly once, or `None`
when the mutation is above the jobs (a workflow-level `defaults:`) or
spans a job header (a whole added job), where the anchor must be unique
in the document instead.

## `pub(super) struct WorkflowEscape` › `pub(super) refused_as: &'static str,`

The complaint code the contract must produce. A code, not a phrase: a
mutation refused for an unrelated reason is not a refusal of this escape.

## `pub(super) fn mutate_workflow(`

Replace `anchor` with `replacement` inside one job's block.

Scoped to the block, and the anchor must occur in it exactly once: a mutation
that lands somewhere unintended, or that no longer matches because the
workflow moved, is a mutation nobody measured. Both are failures here rather
than a quietly weaker test.
