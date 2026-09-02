//! The CI workflow's structural oracle: what is wrong with `ci.yml`, and the
//! mutations that prove each complaint fires.
//!
//! Every claim here is an equality over a parsed mapping or an exact scalar
//! pin, never a `contains` over text — `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE`
//! is the row that records what a substring reading of this surface misses.
//! [`WORKFLOW_ESCAPES`] is the other half: each entry is a document that *does*
//! the forbidden thing, and a complaint **code** it must be refused as. A
//! refusal for an unrelated reason is not a refusal of that escape.
//!
//! The shape being checked against is `super::ci_model`'s, not a second copy of
//! it. The `#[test]` wrappers that drive these functions stay in `super`: this
//! module is the oracle, not the harness, and every name in it is deliberately
//! not a test name.
//!
//! The three effect denials are **restored** here rather than inherited.
//! `super`'s module-level allowance exists because that file drives
//! `clippy-driver` over fixtures it has to create; this module reads two files
//! and writes none, so the allowance has no business reaching it.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use yaml_rust2::{Yaml, YamlLoader};

use super::ci_model::{
    AGGREGATE_JOB, AGGREGATE_JOB_FIELDS, AGGREGATE_SCRIPT, AGGREGATE_SHELL, AGGREGATE_STEP_FIELDS,
    CI_TARGETS, CI_WORKFLOW, CLIPPY_GATE, CiTarget, DEFAULTS_FIELDS, DEFAULTS_RUN_FIELDS,
    ENCODED_RUSTFLAGS_KEY, GATE_JOB_FIELDS, GATE_SCRIPTS, KNOWN_SHELLS, MSRV_COMMAND, MSRV_JOB,
    MSRV_JOB_FIELDS, OPTIONAL_DEFAULTS_FIELD, PINNED_ACTIONS, REQUIRED_CONTEXT, RUSTFLAGS_KEY,
    RUSTFLAGS_VALUE, SELF_HOSTED_TEST_PLATFORM, STEP_FIELDS, TEST_COMMAND, TEST_JOB_FIELDS,
    TEST_SCRIPTS, TEST_WINDOWS_JOB, TEST_WINDOWS_JOB_FIELDS, TEST_WINDOWS_LABELS,
    WINDOWS_BUILD_WITNESS, WORKFLOW_FIELDS,
};
use super::repo_root;

/// `.github/workflows/ci.yml`, with CRLF collapsed.
///
/// The collapse is not cosmetic. This test runs on `windows-latest`, where the
/// checkout can land CRLF line endings, and every claim below is an equality
/// against an exact scalar. Normalising here makes the oracle read the same
/// document on all three runners instead of depending on the scanner's line-break
/// handling -- the platform-shaped half of a mutation is the half that is only
/// ever measured on Linux.
pub(super) fn ci_workflow_text() -> String {
    fs::read_to_string(repo_root().join(CI_WORKFLOW))
        .expect(CI_WORKFLOW)
        .replace("\r\n", "\n")
}

/// Parse a workflow as YAML 1.2, refusing duplicate keys.
///
/// Duplicate-key rejection is a property of the parser this crate depends on for
/// exactly this reason, and
/// [`the_workflow_parser_rejects_duplicate_keys_and_reads_on_as_a_string`]
/// executes it: under last-one-wins every equality below reads the winning entry
/// and a mutation hides in the loser.
pub(super) fn parse_workflow(text: &str) -> Result<Yaml, String> {
    let mut documents =
        YamlLoader::load_from_str(text).map_err(|error| format!("[parse] {error}"))?;
    if documents.len() != 1 {
        return Err(format!(
            "[parse] expected exactly one YAML document, found {}",
            documents.len()
        ));
    }
    Ok(documents.remove(0))
}

/// The value of `key` in a mapping node.
pub(super) fn field<'a>(node: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    node.as_hash()?
        .iter()
        .find(|(name, _)| name.as_str() == Some(key))
        .map(|(_, value)| value)
}

/// Every key of a mapping node. A non-string key is rendered rather than
/// dropped, so `on:` read as a boolean by a YAML 1.1 parser would show up here
/// as an unexpected field rather than as a silent absence.
pub(super) fn field_names(node: &Yaml) -> BTreeSet<String> {
    match node.as_hash() {
        Some(hash) => hash
            .keys()
            .map(|key| match key.as_str() {
                Some(name) => name.to_owned(),
                None => format!("{key:?}"),
            })
            .collect(),
        None => BTreeSet::new(),
    }
}

pub(super) fn scalar<'a>(node: &'a Yaml, key: &str) -> Option<&'a str> {
    field(node, key).and_then(Yaml::as_str)
}

pub(super) fn steps_of(job: &Yaml) -> &[Yaml] {
    field(job, "steps")
        .and_then(Yaml::as_vec)
        .map_or(&[][..], Vec::as_slice)
}

/// A sequence of scalars as a set, or `None` if the node is not that.
fn scalar_set(node: &Yaml) -> Option<BTreeSet<String>> {
    node.as_vec()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

/// A mapping of scalar to scalar, or `None` if the node is not that.
fn scalar_map(node: &Yaml) -> Option<BTreeMap<String, String>> {
    node.as_hash()?
        .iter()
        .map(|(key, value)| Some((key.as_str()?.to_owned(), value.as_str()?.to_owned())))
        .collect()
}

/// The environment variable the aggregate's loop reads for `job`.
fn gate_stem(job: &str) -> String {
    job.to_uppercase().replace('-', "_")
}

fn unexpected(found: &BTreeSet<String>, allowed: &[&str]) -> Vec<String> {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    found
        .iter()
        .filter(|name| !allowed.contains(name.as_str()))
        .cloned()
        .collect()
}

/// Every field the contract requires but the node does not declare, and every
/// field it declares that the contract does not know about.
fn field_complaints(node: &Yaml, required: &[&str], optional: &[&str]) -> Vec<String> {
    let declared = field_names(node);
    let allowed: Vec<&str> = required.iter().chain(optional.iter()).copied().collect();
    let mut out: Vec<String> = unexpected(&declared, &allowed)
        .into_iter()
        .map(|name| format!("declares `{name}`, which this contract does not model"))
        .collect();
    for name in required {
        if !declared.contains(*name) {
            out.push(format!("does not declare `{name}`"));
        }
    }
    out
}

/// The `shell:` a `defaults:` mapping sets, and every way its shape is wrong.
fn defaults_shell<'a>(node: &'a Yaml, where_: &str, out: &mut Vec<String>) -> Option<&'a str> {
    let defaults = field(node, "defaults")?;
    for complaint in field_complaints(defaults, &DEFAULTS_FIELDS, &[]) {
        out.push(format!(
            "[defaults-shape] {where_}'s `defaults:` {complaint}"
        ));
    }
    let run = field(defaults, "run")?;
    for complaint in field_complaints(run, &[], &DEFAULTS_RUN_FIELDS) {
        out.push(format!(
            "[defaults-shape] {where_}'s `defaults.run:` {complaint}"
        ));
    }
    scalar(run, "shell")
}

/// The shell a `run:` step actually executes under.
///
/// GitHub resolves it step, then job `defaults.run.shell`, then workflow
/// `defaults.run.shell`, then the runner's platform default. Reading only the
/// step is how a workflow-level default silently swaps the interpreter under
/// every gate at once -- measured, `MUT-WORKFLOW-DEFAULT-SHELL-SWAPPED`.
fn effective_shell(
    doc: &Yaml,
    job: &Yaml,
    step: &Yaml,
    target: &CiTarget,
    out: &mut Vec<String>,
) -> String {
    let job_default = defaults_shell(job, "the job", out);
    let workflow_default = defaults_shell(doc, "the workflow", out);
    scalar(step, "shell")
        .or(job_default)
        .or(workflow_default)
        .unwrap_or(target.default_shell)
        .to_owned()
}

/// Complain unless `step` resolves to exactly `expected`, and unless that is a
/// shell GitHub defines rather than a command template it will run instead.
fn shell_complaints(
    doc: &Yaml,
    job: &Yaml,
    step: &Yaml,
    target: &CiTarget,
    named: &str,
    expected: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    // A `shell:` key whose value is not a string resolves to nothing at all
    // here, and this contract would then read the platform default and pass.
    // YAML reads the bare word `true` as a boolean, so that is not hypothetical.
    if field(step, "shell").is_some() && scalar(step, "shell").is_none() {
        out.push(format!(
            "[step-shell] {named} declares a `shell:` that is not a string, so what it \
             resolves to is not something this contract can read"
        ));
    }
    let resolved = effective_shell(doc, job, step, target, &mut out);
    if !KNOWN_SHELLS.contains(&resolved.as_str()) {
        out.push(format!(
            "[step-shell] {named} resolves to shell `{resolved}`, which is not one GitHub \
             defines. A custom shell is a command template run as `<template> <script file>`, \
             so it can succeed without executing the script at all."
        ));
    } else if resolved != expected {
        out.push(format!(
            "[step-shell] {named} resolves to shell `{resolved}` on `{}`, not `{expected}`. \
             The resolution is step, then job `defaults.run.shell`, then workflow \
             `defaults.run.shell`, then the platform default.",
            target.runner
        ));
    }
    out
}

/// The job ids whose gate stems collide.
///
/// `gate_stem` upper-cases and turns `-` into `_`, so `lint-windows` and
/// `lint_windows` are one variable name. Every collection built from stems is a
/// map or a set, so a collision **collapses** rather than duplicating: the env
/// mapping loses an entry, the loop's expected set loses a member, and both
/// equalities below compare a shorter list against a shorter list and pass. The
/// collision is therefore checked before anything is derived from the stems.
/// Measured, `MUT-AGGREGATE-STEM-COLLISION`.
fn stem_collisions(jobs: &BTreeSet<String>) -> BTreeMap<String, Vec<String>> {
    let mut by_stem: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for job in jobs {
        by_stem.entry(gate_stem(job)).or_default().push(job.clone());
    }
    by_stem.retain(|_, named| named.len() > 1);
    by_stem
}

/// Every way the parsed workflow fails the gate-wiring contract.
///
/// One function so the same contract runs against the real document and against
/// each mutation in [`WORKFLOW_ESCAPES`]; an oracle only ever run on conforming
/// input is an oracle nobody has seen refuse anything. Each complaint opens with
/// a `[kebab-code]`, and the escape table names the code it must provoke -- so a
/// mutation that fails for an unrelated reason does not count as refused.
fn ci_gate_complaints(doc: &Yaml) -> Vec<String> {
    let mut out = Vec::new();
    for complaint in field_complaints(doc, &WORKFLOW_FIELDS, &OPTIONAL_DEFAULTS_FIELD) {
        out.push(format!(
            "[unexpected-workflow-field] the workflow {complaint}"
        ));
    }
    let Some(jobs) = field(doc, "jobs") else {
        out.push("[jobs] the workflow declares no `jobs:` mapping".to_owned());
        return out;
    };
    let job_names = field_names(jobs);
    if job_names.is_empty() {
        out.push("[jobs] `jobs:` is not a mapping with named jobs".to_owned());
        return out;
    }

    for target in &CI_TARGETS {
        let gates: Vec<&String> = job_names
            .iter()
            .filter(|name| {
                let Some(job) = field(jobs, name) else {
                    return false;
                };
                scalar(job, "runs-on") == Some(target.runner)
                    && steps_of(job)
                        .iter()
                        .any(|step| scalar(step, "run") == Some(CLIPPY_GATE))
            })
            .collect();
        if gates.len() != 1 {
            out.push(format!(
                "[no-gate-job] expected exactly one job whose `runs-on:` is `{}` and one of \
                 whose steps has `run:` equal to `{CLIPPY_GATE}`, found {gates:?}. Without it \
                 every body the `{}` tuple compiles is outside the denylist's reach on every \
                 job CI runs.",
                target.runner, target.triple
            ));
            continue;
        }
        let gate = gates[0];
        let Some(job) = field(jobs, gate) else {
            continue;
        };

        for complaint in field_complaints(job, &GATE_JOB_FIELDS, &OPTIONAL_DEFAULTS_FIELD) {
            out.push(format!(
                "[unexpected-job-field] `{gate}` {complaint}. A field this contract does not \
                 model can disable the job (`if:`) or absolve its failure \
                 (`continue-on-error:`) while every other check here still passes."
            ));
        }
        for (index, step) in steps_of(job).iter().enumerate() {
            let names = field_names(step);
            let strange = unexpected(&names, &STEP_FIELDS);
            if !strange.is_empty() {
                out.push(format!(
                    "[unexpected-step-field] `{gate}` step {index} declares {strange:?}, which \
                     this contract does not know about; `if:` and `continue-on-error:` are \
                     absent from the allowed set deliberately."
                ));
            }
            // Only `run:` steps have a shell; a `uses:` step runs an action.
            if scalar(step, "run").is_some() {
                out.extend(shell_complaints(
                    doc,
                    job,
                    step,
                    target,
                    &format!("`{gate}` step {index}"),
                    target.default_shell,
                ));
            }
        }
        out.extend(step_pin_complaints(
            job,
            gate,
            "gate-run-script",
            &GATE_SCRIPTS,
        ));
        out.extend(checkout_complaints(job, gate, "gate-checkout"));
    }

    out.extend(aggregate_complaints(doc, jobs, &job_names));
    out
}

/// Every way the aggregate fails to make each gate a required one.
fn aggregate_complaints(doc: &Yaml, jobs: &Yaml, job_names: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(aggregate) = field(jobs, AGGREGATE_JOB) else {
        return vec![format!(
            "[aggregate-missing] no `{AGGREGATE_JOB}` job, so nothing publishes the \
             `{REQUIRED_CONTEXT}` context branch protection requires"
        )];
    };

    for complaint in field_complaints(aggregate, &AGGREGATE_JOB_FIELDS, &OPTIONAL_DEFAULTS_FIELD) {
        out.push(format!(
            "[unexpected-job-field] `{AGGREGATE_JOB}` {complaint}"
        ));
    }
    if scalar(aggregate, "name") != Some(REQUIRED_CONTEXT) {
        out.push(format!(
            "[aggregate-context-name] `{AGGREGATE_JOB}` publishes `{:?}`, not \
             `{REQUIRED_CONTEXT}`. Branch protection names one context; a job that \
             publishes another leaves the required one missing forever.",
            scalar(aggregate, "name")
        ));
    }
    if scalar(aggregate, "if") != Some("always()") {
        out.push(format!(
            "[aggregate-condition] `{AGGREGATE_JOB}` runs under `{:?}`, not `always()`. \
             Without `always()` a failed or cancelled dependency *skips* the aggregate, \
             and a skipped required check never settles rather than settling red.",
            scalar(aggregate, "if")
        ));
    }

    // The `needs` set is DERIVED: every job but the aggregate itself. A job that
    // is not needed cannot fail the aggregate, so adding one and forgetting to
    // wire it is the same defect as dropping one, and this equality refuses both.
    let mut expected_needs: BTreeSet<String> = job_names.clone();
    expected_needs.remove(AGGREGATE_JOB);

    // Before ANY collection is derived from the stems. Two job ids that
    // normalise to one variable name collapse every set and map built below, and
    // a collapsed expectation compares equal to a collapsed reality.
    let collisions = stem_collisions(&expected_needs);
    if !collisions.is_empty() {
        out.push(format!(
            "[aggregate-stem-collision] these job ids share a gate variable stem: \
             {collisions:?}. `{AGGREGATE_JOB}` reads one `<STEM>_RESULT` per gate, so one of \
             each colliding pair is unreadable -- and because the expected env mapping and \
             the expected loop list are built from the same stems, both would compare equal \
             to a workflow that checks only one of them. Nothing below is derived while this \
             holds."
        ));
        return out;
    }

    let needs = field(aggregate, "needs").and_then(scalar_set);
    if needs.as_ref() != Some(&expected_needs) {
        out.push(format!(
            "[aggregate-needs] `{AGGREGATE_JOB}` needs {needs:?}, not exactly \
             {expected_needs:?} -- the set of every other job in this workflow. A gate the \
             aggregate does not depend on can fail while branch protection settles green."
        ));
    }
    let wired = needs.unwrap_or(expected_needs);

    let expected_env: BTreeMap<String, String> = wired
        .iter()
        .map(|job| {
            (
                format!("{}_RESULT", gate_stem(job)),
                format!("${{{{ needs.{job}.result }}}}"),
            )
        })
        .collect();

    let steps = steps_of(aggregate);
    if steps.len() != 1 {
        out.push(format!(
            "[aggregate-steps] `{AGGREGATE_JOB}` has {} steps, not exactly one",
            steps.len()
        ));
        return out;
    }
    let step = &steps[0];
    let strange = unexpected(&field_names(step), &AGGREGATE_STEP_FIELDS);
    if !strange.is_empty() {
        out.push(format!(
            "[unexpected-step-field] `{AGGREGATE_JOB}`'s step declares {strange:?}"
        ));
    }

    // The aggregate runs on `ubuntu-latest`, and its script is bash. The check
    // is on the RESOLVED shell, not on the declaration: a workflow-level
    // `defaults.run.shell` reaches this step too, and a custom shell would let
    // the required check pass without reading a single gate result.
    let on_ubuntu = CI_TARGETS
        .iter()
        .find(|target| Some(target.runner) == scalar(aggregate, "runs-on"));
    match on_ubuntu {
        Some(target) => out.extend(shell_complaints(
            doc,
            aggregate,
            step,
            target,
            &format!("`{AGGREGATE_JOB}`'s step"),
            AGGREGATE_SHELL,
        )),
        None => out.push(format!(
            "[step-shell] `{AGGREGATE_JOB}` runs on {:?}, which is not a runner this contract \
             models, so the shell its script resolves to is undecidable here",
            scalar(aggregate, "runs-on")
        )),
    }

    // The binding, not its existence. `LINT_MACOS_RESULT: ${{ needs.lint-windows
    // .result }}` is a copy-paste that satisfies any existence check, reads a
    // passing sibling, and reports the required context green over a red leaf.
    // Measured: an earlier version of this assertion accepted exactly that.
    let env = field(step, "env").and_then(scalar_map);
    if env.as_ref() != Some(&expected_env) {
        out.push(format!(
            "[aggregate-env] `{AGGREGATE_JOB}`'s step binds {env:#?}, not exactly \
             {expected_env:#?}. Each name must read the result of the job it names; a \
             binding that reads a sibling passes an existence check and reports green \
             over this platform's failure."
        ));
    }

    let script = scalar(step, "run");
    if script != Some(AGGREGATE_SCRIPT) {
        out.push(format!(
            "[aggregate-script] `{AGGREGATE_JOB}`'s script is not the pinned one. \
             Found:\n{script:?}\nPinned:\n{AGGREGATE_SCRIPT:?}"
        ));
    }

    // Re-derived from `needs`, so the pin above is checked against the job graph
    // rather than trusted as a copy. `for gate in LINT LINT_WINDOWS MSRV TEST; do
    // : LINT_MACOS` is the enumerated escape: it satisfies a search for the
    // omitted name while the loop never reads it.
    let expected_stems: BTreeSet<String> = wired.iter().map(|job| gate_stem(job)).collect();
    let headers: Vec<&str> = script
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("for "))
        .collect();
    if headers.len() != 1 {
        out.push(format!(
            "[aggregate-loop] `{AGGREGATE_JOB}`'s script has {} `for` headers, not exactly \
             one: {headers:?}",
            headers.len()
        ));
    } else if let Some(listed) = headers[0]
        .strip_prefix("for gate in ")
        .and_then(|rest| rest.strip_suffix("; do"))
    {
        let named: BTreeSet<String> = listed.split_whitespace().map(str::to_owned).collect();
        if named != expected_stems {
            out.push(format!(
                "[aggregate-loop] the required-gate loop names {named:?}, not exactly \
                 {expected_stems:?} -- the stems of every job in `needs:`. A gate in `needs` \
                 that the loop never reads can fail without failing the aggregate."
            ));
        }
    } else {
        out.push(format!(
            "[aggregate-loop] the loop header is not `for gate in <gates>; do`: {:?}",
            headers[0]
        ));
    }

    out
}

/// Every way the job that executes this file's fixtures fails its contract.
///
/// The predecessor read the job's text with comments stripped and asked whether
/// the word `clippy` appeared on a `components:` line and whether the file
/// contained the test command anywhere. Both are satisfied by an `echo`, and the
/// strip existed only because the job's nine-line comment spelled the needle --
/// `PR4-CENSUS-COMMENT-ORACLE` in the test whose purpose is to answer "which
/// command runs this?". A parsed document has no comments in it at all, so that
/// class is gone by construction rather than by a strip whose bite had to be
/// asserted.
pub(super) fn ci_test_job_complaints(doc: &Yaml) -> Vec<String> {
    let mut out = Vec::new();
    let Some(jobs) = field(doc, "jobs") else {
        return vec!["[jobs] the workflow declares no `jobs:` mapping".to_owned()];
    };
    let Some(job) = field(jobs, "test") else {
        return vec!["[test-job-missing] no `test` job, so nothing runs these fixtures".to_owned()];
    };

    for complaint in field_complaints(job, &TEST_JOB_FIELDS, &OPTIONAL_DEFAULTS_FIELD) {
        out.push(format!("[unexpected-job-field] `test` {complaint}"));
    }
    for (index, step) in steps_of(job).iter().enumerate() {
        let strange = unexpected(&field_names(step), &STEP_FIELDS);
        if !strange.is_empty() {
            out.push(format!(
                "[unexpected-step-field] `test` step {index} declares {strange:?}"
            ));
        }
        // This job is a matrix, so each `run:` step resolves a shell once per
        // hosted runner. Each must be the platform default: a workflow-level
        // default swaps every one of them at once, which is the mutation the
        // step-only reading could not see.
        if scalar(step, "run").is_some() {
            for target in CI_TARGETS.iter().filter(|target| hosts_tests(target)) {
                out.extend(shell_complaints(
                    doc,
                    job,
                    step,
                    target,
                    &format!("`test` step {index} on `{}`", target.runner),
                    target.default_shell,
                ));
            }
        }
    }

    let running = steps_of(job)
        .iter()
        .filter(|step| scalar(step, "run") == Some(TEST_COMMAND))
        .count();
    if running != 1 {
        out.push(format!(
            "[test-job-command] the `test` job has {running} steps whose `run:` is exactly \
             `{TEST_COMMAND}`, not one. These fixtures are only evidence of anything in a \
             job that executes them."
        ));
    }
    out.extend(checkout_complaints(job, "test", "test-job-checkout"));
    out.extend(step_pin_complaints(
        job,
        "test",
        "test-job-run-script",
        &TEST_SCRIPTS,
    ));

    // The matrix is the platform half of the same claim, and it is compared
    // against the same derived runner set the Clippy legs are, less the one
    // platform whose suite runs self-hosted: a fixture that runs on one
    // platform proves nothing about the other.
    let expected_runners: BTreeSet<String> = CI_TARGETS
        .iter()
        .filter(|target| hosts_tests(target))
        .map(|target| target.runner.to_owned())
        .collect();
    // The WHOLE strategy mapping, not just `matrix.os`. `exclude:` removes
    // combinations that `os:` still lists, so a job that names three platforms
    // can run on one while every check that reads `os:` passes; `include:` can
    // add a fourth nobody declared, and `max-parallel`/`fail-fast` are the rest
    // of what a strategy may say. An equality over the field set refuses all of
    // them, including the ones GitHub adds next. Measured,
    // `MUT-TEST-MATRIX-EXCLUDED`.
    match field(job, "strategy") {
        None => out.push(
            "[test-job-matrix] the `test` job declares no `strategy:`, so it runs on one \
             platform"
                .to_owned(),
        ),
        Some(strategy) => {
            for complaint in field_complaints(strategy, &["fail-fast", "matrix"], &[]) {
                out.push(format!(
                    "[test-job-matrix] the `test` job's `strategy:` {complaint}. `exclude:` \
                     removes a runner `os:` still lists and `include:` adds one it does not."
                ));
            }
            if field(strategy, "fail-fast").and_then(Yaml::as_bool) != Some(false) {
                out.push(
                    "[test-job-matrix] the `test` job's `fail-fast:` is not `false`, so one \
                     platform's failure cancels the other before it reports"
                        .to_owned(),
                );
            }
            if let Some(matrix) = field(strategy, "matrix") {
                for complaint in field_complaints(matrix, &["os"], &[]) {
                    out.push(format!(
                        "[test-job-matrix] the `test` job's `matrix:` {complaint}"
                    ));
                }
            }
        }
    }
    let matrix = field(job, "strategy").and_then(|strategy| field(strategy, "matrix"));
    let listed = matrix
        .and_then(|matrix| field(matrix, "os"))
        .and_then(scalar_set);
    if listed.as_ref() != Some(&expected_runners) {
        out.push(format!(
            "[test-job-matrix] the `test` job's matrix runs on {listed:?}, not exactly \
             {expected_runners:?}"
        ));
    }
    if scalar(job, "runs-on") != Some("${{ matrix.os }}") {
        out.push(format!(
            "[test-job-matrix] the `test` job's `runs-on:` is {:?}, so the matrix above \
             decides nothing",
            scalar(job, "runs-on")
        ));
    }

    // `clippy` is a TEST dependency of this job, not only a lint one:
    // `every_declared_effect_denial_refuses_for_the_reason_it_declares` drives
    // `clippy-driver` over one fixture per resolution shape, and
    // `dtolnay/rust-toolchain` installs the minimal profile. Measured, mutation
    // `MUT-CI-STOPS-INSTALLING-CLIPPY`.
    let toolchains: Vec<&Yaml> = steps_of(job)
        .iter()
        .filter(|step| {
            scalar(step, "uses").is_some_and(|uses| uses.starts_with("dtolnay/rust-toolchain@"))
        })
        .collect();
    if toolchains.len() != 1 {
        out.push(format!(
            "[test-job-toolchain] the `test` job has {} `dtolnay/rust-toolchain` steps, not \
             one",
            toolchains.len()
        ));
        return out;
    }
    let with = field(toolchains[0], "with");
    let components: BTreeSet<&str> = with
        .and_then(|with| scalar(with, "components"))
        .map(|list| list.split(',').map(str::trim).collect())
        .unwrap_or_default();
    if !components.contains("clippy") {
        out.push(format!(
            "[test-job-toolchain] the `test` job installs components {components:?}, which \
             do not include `clippy`, so `every_declared_effect_denial_refuses_for_the_reason\
             _it_declares` cannot run there: `dtolnay/rust-toolchain` installs the minimal \
             profile and `clippy-driver` is not in it."
        ));
    }
    if with.and_then(|with| scalar(with, "toolchain")) != Some("stable") {
        out.push(format!(
            "[test-job-toolchain] the `test` job selects toolchain {:?}, not `stable`",
            with.and_then(|with| scalar(with, "toolchain"))
        ));
    }
    out
}

/// Whether a [`CI_TARGETS`] runner hosts its platform's tests in the `test`
/// matrix -- every platform but the one whose suite is self-hosted.
fn hosts_tests(target: &CiTarget) -> bool {
    target.runner != SELF_HOSTED_TEST_PLATFORM
}

/// Every way the self-hosted Windows job fails its contract.
///
/// The same shape as [`ci_test_job_complaints`] without the matrix, and with
/// the one thing that job cannot say: which machine. A hosted runner is named
/// by a scalar `runs-on:`; a self-hosted one by the set of labels a runner must
/// carry, and the set is compared whole -- a subset admits every Windows
/// machine the account registers, and a scalar `windows-latest` is the leg this
/// contract retired coming back with every step still matching.
///
/// No toolchain step is required: the guest's image carries `clippy-driver`
/// for the fixtures, and the decision record binds re-curation to that claim,
/// which a document parser cannot check.
pub(super) fn ci_test_windows_job_complaints(doc: &Yaml) -> Vec<String> {
    let mut out = Vec::new();
    let Some(jobs) = field(doc, "jobs") else {
        return vec!["[jobs] the workflow declares no `jobs:` mapping".to_owned()];
    };
    let Some(job) = field(jobs, TEST_WINDOWS_JOB) else {
        return vec![format!(
            "[test-windows-missing] no `{TEST_WINDOWS_JOB}` job, so nothing runs the Windows \
             suite"
        )];
    };
    let Some(platform) = CI_TARGETS
        .iter()
        .find(|target| target.runner == SELF_HOSTED_TEST_PLATFORM)
    else {
        return vec![format!(
            "[test-windows-platform] `{SELF_HOSTED_TEST_PLATFORM}` is not a runner this \
             contract models, so the shell its steps resolve to is undecidable here"
        )];
    };

    for complaint in field_complaints(job, &TEST_WINDOWS_JOB_FIELDS, &OPTIONAL_DEFAULTS_FIELD) {
        out.push(format!(
            "[unexpected-job-field] `{TEST_WINDOWS_JOB}` {complaint}"
        ));
    }
    for (index, step) in steps_of(job).iter().enumerate() {
        let strange = unexpected(&field_names(step), &STEP_FIELDS);
        if !strange.is_empty() {
            out.push(format!(
                "[unexpected-step-field] `{TEST_WINDOWS_JOB}` step {index} declares {strange:?}"
            ));
        }
        if scalar(step, "run").is_some() {
            out.extend(shell_complaints(
                doc,
                job,
                step,
                platform,
                &format!("`{TEST_WINDOWS_JOB}` step {index}"),
                platform.default_shell,
            ));
        }
    }

    let expected_labels: BTreeSet<String> = TEST_WINDOWS_LABELS
        .iter()
        .copied()
        .map(str::to_owned)
        .collect();
    let labels = field(job, "runs-on").and_then(scalar_set);
    if labels.as_ref() != Some(&expected_labels) {
        out.push(format!(
            "[test-windows-runner] `{TEST_WINDOWS_JOB}` runs on {:?}, not exactly the label \
             set {expected_labels:?}. A scalar here is a hosted runner, a subset is any \
             self-hosted Windows machine the account registers, and the pinned set names the \
             curated image.",
            field(job, "runs-on")
        ));
    }

    let running = steps_of(job)
        .iter()
        .filter(|step| scalar(step, "run") == Some(TEST_COMMAND))
        .count();
    if running != 1 {
        out.push(format!(
            "[test-windows-command] `{TEST_WINDOWS_JOB}` has {running} steps whose `run:` is \
             exactly `{TEST_COMMAND}`, not one"
        ));
    }
    out.extend(checkout_complaints(
        job,
        TEST_WINDOWS_JOB,
        "test-windows-checkout",
    ));
    out.extend(step_pin_complaints(
        job,
        TEST_WINDOWS_JOB,
        "test-windows-run-script",
        &TEST_SCRIPTS,
    ));
    out
}

/// Every way a test job's checkout points the suite at a tree other than the
/// head under test.
///
/// `actions/checkout` with no inputs checks out the event's own ref: for a
/// pull request, the candidate merged onto its base. Any input -- `ref:`,
/// `repository:`, `path:` -- selects something else, and `ref: master` there
/// tests `master` while every other leg reads the candidate. [`STEP_FIELDS`]
/// admits `with:` because the toolchain and cache actions need it; on a
/// checkout step it is refused whole. Measured, `MUT-TEST-WINDOWS-CHECKOUT-REF`
/// and `MUT-TEST-CHECKOUT-REF`.
fn checkout_complaints(job: &Yaml, named: &str, code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (index, step) in steps_of(job).iter().enumerate() {
        let checks_out =
            scalar(step, "uses").is_some_and(|uses| uses.starts_with("actions/checkout@"));
        if !checks_out {
            continue;
        }
        let Some(inputs) = field(step, "with") else {
            continue;
        };
        out.push(format!(
            "[{code}] `{named}` step {index} checks out with inputs {:?}. With no inputs the \
             action checks out the head under test; any input can point this leg at another \
             tree while every other leg reads the candidate.",
            field_names(inputs)
        ));
    }
    out
}

/// Every way a step of a modelled job is something this contract did not pin.
///
/// The field and shell checks say how a step runs; this says what. A `run:`
/// step whose script is not in the job's pinned set can move the checkout --
/// `git fetch origin master && git checkout --detach FETCH_HEAD` -- before the
/// pinned command runs, so that command runs against another tree while the
/// labels, fields, shell and input-free checkout all still match. A `uses:`
/// step off [`PINNED_ACTIONS`] is code nobody here reviewed, with a checkout
/// of its own. Measured, `MUT-TEST-WINDOWS-RUN-RETARGETED`,
/// `MUT-TEST-RUN-RETARGETED`, `MUT-GATE-RUN-RETARGETED`,
/// `MUT-MSRV-RUN-RETARGETED` and `MUT-STEP-USES-UNPINNED`.
fn step_pin_complaints(job: &Yaml, named: &str, code: &str, scripts: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for (index, step) in steps_of(job).iter().enumerate() {
        let unpinned_script = scalar(step, "run").filter(|script| !scripts.contains(script));
        if let Some(script) = unpinned_script {
            out.push(format!(
                "[{code}] `{named}` step {index} runs a script this contract does not pin: \
                 {script:?}. An unpinned step can move the checkout before the pinned command \
                 runs, so the pinned command runs against another tree."
            ));
        }
        let unpinned_action = scalar(step, "uses").filter(|uses| !PINNED_ACTIONS.contains(uses));
        if let Some(uses) = unpinned_action {
            out.push(format!(
                "[unpinned-action] `{named}` step {index} uses {uses:?}, which is not one of the \
                 pinned actions {PINNED_ACTIONS:?}; an unreviewed action, or a reviewed one at a \
                 floating tag, can check out any tree it likes."
            ));
        }
    }
    out
}

/// Every way the hosted Windows codegen witness fails its contract.
///
/// The self-hosted leg executes the Windows suite with the golden image's
/// toolchain, which moves only by re-curation. `cargo check` and Clippy on
/// `windows-latest` type-check current stable and stop before codegen, so
/// without this witness nothing on GitHub's current stable ever code-generates
/// or links the Windows tree: a Windows-only codegen or link failure there
/// would pass every hosted leg while the guest, one stable behind, links and
/// passes. [`WINDOWS_BUILD_WITNESS`] builds every test binary and executes
/// none. It lives in the Windows Clippy gate's job, whose field set and shells
/// the gate contract pins; this pins the command, exactly once, on exactly one
/// hosted Windows job. Measured, `MUT-WINDOWS-BUILD-WITNESS-*`.
pub(super) fn ci_windows_build_witness_complaints(doc: &Yaml) -> Vec<String> {
    let mut out = Vec::new();
    let Some(jobs) = field(doc, "jobs") else {
        return vec!["[jobs] the workflow declares no `jobs:` mapping".to_owned()];
    };
    let Some(platform) = CI_TARGETS
        .iter()
        .find(|target| target.runner == SELF_HOSTED_TEST_PLATFORM)
    else {
        return vec![format!(
            "[windows-build-witness] `{SELF_HOSTED_TEST_PLATFORM}` is not a runner this \
             contract models, so the shell its steps resolve to is undecidable here"
        )];
    };
    let carriers: Vec<String> = field_names(jobs)
        .into_iter()
        .filter(|name| {
            field(jobs, name).is_some_and(|job| {
                scalar(job, "runs-on") == Some(SELF_HOSTED_TEST_PLATFORM)
                    && steps_of(job)
                        .iter()
                        .any(|step| scalar(step, "run") == Some(WINDOWS_BUILD_WITNESS))
            })
        })
        .collect();
    let [carrier] = carriers.as_slice() else {
        out.push(format!(
            "[windows-build-witness] expected exactly one job whose `runs-on:` is \
             `{SELF_HOSTED_TEST_PLATFORM}` and one of whose steps has `run:` equal to \
             `{WINDOWS_BUILD_WITNESS}`, found {carriers:?}. Without it no hosted leg \
             code-generates or links the Windows tree on current stable: `cargo check` and \
             Clippy stop before codegen, and the self-hosted leg builds with the image's \
             toolchain."
        ));
        return out;
    };
    let Some(job) = field(jobs, carrier) else {
        return out;
    };
    let witnesses = steps_of(job)
        .iter()
        .filter(|step| scalar(step, "run") == Some(WINDOWS_BUILD_WITNESS))
        .count();
    if witnesses != 1 {
        out.push(format!(
            "[windows-build-witness] `{carrier}` has {witnesses} steps whose `run:` is \
             exactly `{WINDOWS_BUILD_WITNESS}`, not one"
        ));
    }
    // The carrier must be the Windows Clippy gate's job: that job's fields,
    // shells, scripts and checkout are pinned by the gate contract, so the
    // witness inherits every one of those pins. A witness on some other
    // `windows-latest` job would be a pinned command on an unpinned job.
    if !steps_of(job)
        .iter()
        .any(|step| scalar(step, "run") == Some(CLIPPY_GATE))
    {
        out.push(format!(
            "[windows-build-witness] `{carrier}` carries the witness but not the Windows Clippy \
             gate `{CLIPPY_GATE}`; the witness rides the gate job so the gate contract's pins \
             on fields, shells, scripts and the checkout cover it"
        ));
    }
    for (index, step) in steps_of(job).iter().enumerate() {
        if scalar(step, "run") == Some(WINDOWS_BUILD_WITNESS) {
            out.extend(shell_complaints(
                doc,
                job,
                step,
                platform,
                &format!("`{carrier}` step {index}"),
                platform.default_shell,
            ));
        }
    }
    out
}

/// `Cargo.toml`'s `[package] rust-version`, as it is written there.
pub(super) fn declared_rust_version() -> String {
    let text = fs::read_to_string(repo_root().join("Cargo.toml")).expect("Cargo.toml");
    let manifest: toml::Value = toml::from_str(&text).expect("Cargo.toml parses");
    manifest
        .get("package")
        .and_then(|package| package.get("rust-version"))
        .and_then(toml::Value::as_str)
        .expect("Cargo.toml declares a `[package] rust-version` to pin the msrv leg against")
        .to_owned()
}

/// The toolchain name the MSRV leg must install, derived from the manifest.
///
/// Derived rather than transcribed: a literal `"1.85.0"` here would make this
/// section its own oracle for the one fact it exists to hold, and a bump to
/// `rust-version` would leave the leg checking a floor the crate no longer
/// publishes.
///
/// The pin is exact, which `.github/scripts/test-docs-consistency.sh`'s C2 is
/// deliberately not: C2 accepts `rust-version` "or a patch release of it", so it
/// reads `toolchain: 1.85` as agreement. `dtolnay/rust-toolchain` resolves a
/// two-component name to the newest patch in the series, which is not the
/// `cargo +1.85.0` that `CODING_STANDARDS.md` §2, `CONTRIBUTING.md` and
/// `CLAUDE.md` all publish.
pub(super) fn declared_msrv_toolchain() -> String {
    three_component(&declared_rust_version())
}

/// `1.85` as the toolchain name `1.85.0`; anything else unchanged.
///
/// Unchanged rather than repaired. A manifest value this does not understand
/// must reach the equality below and fail there with both strings quoted, not be
/// normalised into agreement with whatever the workflow happens to say.
pub(super) fn three_component(version: &str) -> String {
    if version.split('.').count() == 2 {
        format!("{version}.0")
    } else {
        version.to_owned()
    }
}

/// Every way the MSRV leg fails to check the floor this crate publishes.
///
/// Nothing above this function reaches that job. [`ci_gate_complaints`] selects
/// a job by its `runs-on:` *and* a step whose `run:` is [`CLIPPY_GATE`], and
/// `msrv` matches neither -- it runs on `${{ matrix.os }}` and it runs
/// `cargo check`. So until this existed the only structural claim on the MSRV
/// leg was that the aggregate needs a job with that id: its matrix could be
/// narrowed to one runner or hollowed out with `exclude:`, its command could
/// lose `--locked` or become an `echo`, and its step could be absolved, with
/// every check in this section still passing.
///
/// One claim here is held elsewhere too, and its neighbour states its own
/// limits. `.github/scripts/test-docs-consistency.sh` C2 compares the toolchain
/// scalar with `rust-version` by grepping a text block; that file's `WITHDRAWN,
/// DELIBERATELY` note records that the gate makes "NO claim about which cargo
/// commands CI runs, whether CI executes them", because a command "can be
/// present and skipped (`if: false`)". A parsed document is what lets that claim
/// come back as an equality -- the same trade the rest of this section made, and
/// the reason `MUT-CI-CARGO-TEST-STEP-SKIPPED` is already a kill here rather
/// than history.
pub(super) fn ci_msrv_job_complaints(doc: &Yaml) -> Vec<String> {
    let mut out = Vec::new();
    let Some(jobs) = field(doc, "jobs") else {
        return vec!["[jobs] the workflow declares no `jobs:` mapping".to_owned()];
    };
    let Some(job) = field(jobs, MSRV_JOB) else {
        return vec![format!(
            "[msrv-job-missing] no `{MSRV_JOB}` job, so nothing compiles this crate on the \
             floor `Cargo.toml`'s `rust-version` publishes"
        )];
    };

    for complaint in field_complaints(job, &MSRV_JOB_FIELDS, &OPTIONAL_DEFAULTS_FIELD) {
        out.push(format!(
            "[msrv-job-field] `{MSRV_JOB}` {complaint}. `continue-on-error:` reports success \
             over a failed check -- which the aggregate then reads as success -- and `if:` \
             stops the leg running at all."
        ));
    }
    for (index, step) in steps_of(job).iter().enumerate() {
        let strange = unexpected(&field_names(step), &STEP_FIELDS);
        if !strange.is_empty() {
            out.push(format!(
                "[msrv-job-step-field] `{MSRV_JOB}` step {index} declares {strange:?}, which \
                 this contract does not model"
            ));
        }
        // A matrix job, so each `run:` step resolves a shell once per runner, and
        // a workflow-level default reaches all three of them at once.
        if scalar(step, "run").is_some() {
            for target in &CI_TARGETS {
                out.extend(shell_complaints(
                    doc,
                    job,
                    step,
                    target,
                    &format!("`{MSRV_JOB}` step {index} on `{}`", target.runner),
                    target.default_shell,
                ));
            }
        }
    }

    let running = steps_of(job)
        .iter()
        .filter(|step| scalar(step, "run") == Some(MSRV_COMMAND))
        .count();
    if running != 1 {
        out.push(format!(
            "[msrv-job-command] the `{MSRV_JOB}` job has {running} steps whose `run:` is \
             exactly `{MSRV_COMMAND}`, not one. Dropping `--locked` lets Cargo resolve past \
             the exact pins this manifest carries for the floor, and an `echo` satisfies \
             every substring reading of the same line."
        ));
    }
    out.extend(step_pin_complaints(
        job,
        MSRV_JOB,
        "msrv-run-script",
        &[MSRV_COMMAND],
    ));
    out.extend(checkout_complaints(job, MSRV_JOB, "msrv-checkout"));

    // The platform half, compared against the same derived runner set the Clippy
    // legs and the `test` matrix are. A floor is a per-platform fact: a
    // dependency that raises its MSRV behind a `cfg` fails on that target only.
    let expected_runners: BTreeSet<String> = CI_TARGETS
        .iter()
        .map(|target| target.runner.to_owned())
        .collect();
    match field(job, "strategy") {
        None => out.push(format!(
            "[msrv-job-matrix] the `{MSRV_JOB}` job declares no `strategy:`, so it checks the \
             floor on one platform"
        )),
        Some(strategy) => {
            for complaint in field_complaints(strategy, &["fail-fast", "matrix"], &[]) {
                out.push(format!(
                    "[msrv-job-matrix] the `{MSRV_JOB}` job's `strategy:` {complaint}. \
                     `exclude:` removes a runner `os:` still lists and `include:` adds one it \
                     does not."
                ));
            }
            if field(strategy, "fail-fast").and_then(Yaml::as_bool) != Some(false) {
                out.push(format!(
                    "[msrv-job-matrix] the `{MSRV_JOB}` job's `fail-fast:` is not `false`, so \
                     one platform's floor failure cancels the other two before they report"
                ));
            }
            if let Some(matrix) = field(strategy, "matrix") {
                for complaint in field_complaints(matrix, &["os"], &[]) {
                    out.push(format!(
                        "[msrv-job-matrix] the `{MSRV_JOB}` job's `matrix:` {complaint}"
                    ));
                }
            }
        }
    }
    let listed = field(job, "strategy")
        .and_then(|strategy| field(strategy, "matrix"))
        .and_then(|matrix| field(matrix, "os"))
        .and_then(scalar_set);
    if listed.as_ref() != Some(&expected_runners) {
        out.push(format!(
            "[msrv-job-matrix] the `{MSRV_JOB}` job's matrix runs on {listed:?}, not exactly \
             {expected_runners:?}"
        ));
    }
    if scalar(job, "runs-on") != Some("${{ matrix.os }}") {
        out.push(format!(
            "[msrv-job-matrix] the `{MSRV_JOB}` job's `runs-on:` is {:?}, so the matrix above \
             decides nothing",
            scalar(job, "runs-on")
        ));
    }

    let toolchains: Vec<&Yaml> = steps_of(job)
        .iter()
        .filter(|step| {
            scalar(step, "uses").is_some_and(|uses| uses.starts_with("dtolnay/rust-toolchain@"))
        })
        .collect();
    if toolchains.len() != 1 {
        out.push(format!(
            "[msrv-job-toolchain] the `{MSRV_JOB}` job has {} `dtolnay/rust-toolchain` steps, \
             not one, so which toolchain checks the floor is not decidable here",
            toolchains.len()
        ));
        return out;
    }
    let selected = field(toolchains[0], "with").and_then(|with| scalar(with, "toolchain"));
    let expected = declared_msrv_toolchain();
    if selected != Some(expected.as_str()) {
        out.push(format!(
            "[msrv-job-toolchain] the `{MSRV_JOB}` job installs toolchain {selected:?}, not \
             `{expected}` -- the three-component form of `Cargo.toml`'s `rust-version`, which \
             is `{}`. A leg named for the floor that installs something else is green about a \
             version it never compiled.",
            declared_rust_version()
        ));
    }

    // Order, not merely presence. `dtolnay/rust-toolchain` selects the toolchain
    // for the steps that FOLLOW it, so a check above it compiles on whatever the
    // runner image preinstalled -- stable -- while both steps are present, both
    // are exact, and every equality above passes. Measured,
    // `MUT-MSRV-CHECK-BEFORE-TOOLCHAIN`.
    //
    // The install step is located by the toolchain it installs, not merely by the
    // action it uses: an install of something other than the derived floor is
    // already refused above, and pairing the order claim to the same exact value
    // keeps the two from drifting apart.
    let install_at = steps_of(job).iter().position(|step| {
        scalar(step, "uses").is_some_and(|uses| uses.starts_with("dtolnay/rust-toolchain@"))
            && field(step, "with").and_then(|with| scalar(with, "toolchain"))
                == Some(expected.as_str())
    });
    let check_at = steps_of(job)
        .iter()
        .position(|step| scalar(step, "run") == Some(MSRV_COMMAND));
    if let Some((install_at, check_at)) = install_at
        .zip(check_at)
        .filter(|(install, check)| install > check)
    {
        out.push(format!(
            "[msrv-job-order] the `{MSRV_JOB}` job runs `{MSRV_COMMAND}` at step {check_at} \
             and installs toolchain `{expected}` at step {install_at}. The install selects the \
             toolchain for the steps that follow it, so a check above it compiles on whatever \
             the runner image shipped and the leg reports green about a version this crate \
             publishes no floor for."
        ));
    }
    out
}

/// Every way the workflow-scope `-D warnings` fails to reach a compilation.
///
/// Two claims. The first is the pin: the workflow's own `env:` binds
/// [`RUSTFLAGS_KEY`] to exactly [`RUSTFLAGS_VALUE`]. The second is what makes
/// the first *effective*: no job and no step rebinds that name, and nothing
/// anywhere binds [`ENCODED_RUSTFLAGS_KEY`], which Cargo reads in preference to
/// it.
///
/// The override scan walks every job and every step rather than the jobs this
/// contract models, and that is why it is written separately from the field
/// sets. On today's document it is defence in depth -- `GATE_JOB_FIELDS`,
/// `TEST_JOB_FIELDS`, `MSRV_JOB_FIELDS`, `AGGREGATE_JOB_FIELDS` and
/// `STEP_FIELDS` already refuse an `env:` almost everywhere it could go. But the
/// `msrv` leg had no field set at all until this change, the aggregate's step is
/// the one step in this contract that is *allowed* an `env:`, and a job added
/// tomorrow has no field set until someone writes one. A rebinding anywhere is
/// refused by this scan on its own, which is what
/// `the_workflow_scope_rustflags_pin_refuses_weakening_and_every_override`
/// measures on documents the rest of the contract does not reach.
pub(super) fn rustflags_complaints(doc: &Yaml) -> Vec<String> {
    let mut out = Vec::new();
    match field(doc, "env") {
        None => out.push(format!(
            "[rustflags] the workflow declares no `env:`, so nothing sets `{RUSTFLAGS_KEY}` at \
             workflow scope and no leg promotes a rustc warning to an error"
        )),
        Some(env) => {
            match field(env, RUSTFLAGS_KEY).map(Yaml::as_str) {
                None => out.push(format!(
                    "[rustflags] the workflow's `env:` does not bind `{RUSTFLAGS_KEY}`. \
                     `CODING_STANDARDS.md` §11 rests every leg's rustc-lint and \
                     `#[expect]`-retirement evidence on this one binding."
                )),
                Some(None) => out.push(format!(
                    "[rustflags] the workflow binds `{RUSTFLAGS_KEY}` to something YAML does \
                     not read as a string, so what the legs compile under is not something \
                     this contract can read -- and an unreadable value is not `{RUSTFLAGS_VALUE}`"
                )),
                Some(Some(found)) if found != RUSTFLAGS_VALUE => out.push(format!(
                    "[rustflags] the workflow binds `{RUSTFLAGS_KEY}` to `{found}`, not exactly \
                     `{RUSTFLAGS_VALUE}`. An equality rather than a `contains`: \
                     `-D warnings -A clippy::disallowed_methods` contains the pinned text and \
                     switches off the denylist this file exists to enforce."
                )),
                Some(Some(_)) => {}
            }
            // Every other guarded binding at workflow scope, case-insensitively.
            // Two distinct defects share this arm: `CARGO_ENCODED_RUSTFLAGS`,
            // which Cargo reads instead of the pinned name, and a case variant of
            // `RUSTFLAGS` itself, which collides with the pinned line on Windows
            // and does nothing on Linux. The pinned key is skipped because the
            // match above already decided it.
            for key in field_names(env) {
                let Some(guarded) = guarded_env_key(&key) else {
                    continue;
                };
                if key == RUSTFLAGS_KEY {
                    continue;
                }
                let why = if guarded == ENCODED_RUSTFLAGS_KEY {
                    format!(
                        "Cargo reads `{ENCODED_RUSTFLAGS_KEY}` in preference to \
                         `{RUSTFLAGS_KEY}` and ignores `{RUSTFLAGS_KEY}` entirely when it is \
                         set, so the pinned line is read past rather than obeyed"
                    )
                } else {
                    format!(
                        "`{key}` and `{RUSTFLAGS_KEY}` are one variable on `windows-latest`, \
                         where the process environment is case-insensitive, so which value \
                         the Windows legs compile under is not decided by this document"
                    )
                };
                out.push(format!(
                    "[rustflags] the workflow's `env:` binds `{key}` beside the pinned \
                     `{RUSTFLAGS_KEY}: {RUSTFLAGS_VALUE}`. {why}."
                ));
            }
        }
    }

    let Some(jobs) = field(doc, "jobs") else {
        return out;
    };
    for name in field_names(jobs) {
        let Some(job) = field(jobs, &name) else {
            continue;
        };
        out.extend(rustflags_override_complaints(job, &format!("job `{name}`")));
        for (index, step) in steps_of(job).iter().enumerate() {
            let named = format!("`{name}` step {index}");
            out.extend(rustflags_override_complaints(step, &named));
            if let Some(script) = scalar(step, "run") {
                out.extend(rustflags_script_complaints(script, &named));
            }
        }
    }
    out
}

/// The canonical guarded name `key` is, ignoring case, or `None`.
///
/// Whole-key equality, never a substring. `RUSTFLAGS_EXTRA`, `RUST_FLAGS` and
/// `CARGO_TERM_COLOR` are unrelated variables and a `contains` reading would
/// refuse bindings that never touch the warning policy.
///
/// Case-insensitive, because the environment on `windows-latest` is. GitHub
/// merges `env:` mappings by exact key and hands the result to the runner, which
/// sets them into a process environment where `rustflags` and `RUSTFLAGS` are one
/// variable. A lowercase job-level binding is therefore inert on Linux and
/// authoritative on Windows -- exactly the half of a mutation that only ever gets
/// measured on Linux, and the reason this comparison is not `==`.
fn guarded_env_key(key: &str) -> Option<&'static str> {
    [RUSTFLAGS_KEY, ENCODED_RUSTFLAGS_KEY]
        .into_iter()
        .find(|guarded| key.eq_ignore_ascii_case(guarded))
}

/// Every `[A-Za-z0-9_]` run of `script`, so a variable name is matched whole.
///
/// A `run:` scalar is an opaque script rather than a mapping, so the finest
/// granularity available over it is a token. Tokens are still enough to keep the
/// discipline the rest of this section keeps: `RUSTFLAGS_EXTRA` is one token and
/// is not `RUSTFLAGS`, which a `contains` reading could not tell apart.
fn env_name_tokens(script: &str) -> BTreeSet<String> {
    script
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `script` names the job-scoped environment file, in any of its forms.
///
/// `$GITHUB_ENV`, `${GITHUB_ENV}`, `$env:GITHUB_ENV`, `%GITHUB_ENV%` and
/// `${{ github.env }}` are one file, reached from bash, pwsh and cmd
/// respectively. A line written to it becomes an environment variable for every
/// later step of the same job.
fn writes_the_job_env_file(script: &str) -> bool {
    let lowered = script.to_ascii_lowercase();
    lowered.contains("github_env") || lowered.contains("github.env")
}

/// Every way a `run:` step reaches the warning policy from inside its script.
///
/// The `env:` mappings this contract compares are declarations. A `run:` scalar
/// is not, and one line of one --
/// `echo "RUSTFLAGS=-A warnings" >> "$GITHUB_ENV"` -- rebinds the variable for
/// **every later step of the same job** while the document declares an `env:`
/// nowhere. Every field-set equality passes, the pinned workflow line is
/// untouched, and the Cargo steps that follow compile under a policy no mapping
/// in this file states. `MUT-RUSTFLAGS-PERSISTED-VIA-GITHUB-ENV` and its three
/// siblings are that line in bash, in a bash heredoc, in PowerShell and through
/// the `${{ github.env }}` expression.
///
/// Position is deliberately not a precondition. Refusing the write wherever it
/// appears is strictly stronger than refusing it only where a Cargo step
/// follows -- which a reorder would defeat -- and no leg of this workflow has a
/// benign reason to name the variable at all.
///
/// **What a token scan over an opaque script can and cannot do**, stated rather
/// than left to be discovered. It refuses every form that spells the name,
/// including forms that are not writes at all: `RUSTFLAGS=-A warnings cargo
/// build` scopes the flags to one command without touching the env file, and it
/// is refused too, because the policy is set once at workflow scope and a script
/// that names it is doing something this contract has no model for. It does
/// **not** refuse a script that assembles the name from pieces. That residual is
/// the same one `AGGREGATE_SCRIPT`'s pin carries: a script is not a document, and
/// the honest bound is the one written down.
fn rustflags_script_complaints(script: &str, named: &str) -> Vec<String> {
    let named_keys: BTreeSet<&'static str> = env_name_tokens(script)
        .iter()
        .filter_map(|token| guarded_env_key(token))
        .collect();
    let persists = writes_the_job_env_file(script);
    named_keys
        .into_iter()
        .map(|key| {
            if persists {
                format!(
                    "[rustflags-persisted] {named} names `{key}` in a script that writes the \
                     job-scoped environment file. A line written there binds the variable for \
                     every later step of this job, so the workflow-scope \
                     `{RUSTFLAGS_KEY}: {RUSTFLAGS_VALUE}` is narrowed without any `env:` \
                     mapping in this document saying so, and every field-set equality here \
                     still passes."
                )
            } else {
                format!(
                    "[rustflags-in-script] {named} names `{key}` in its script. The warning \
                     policy is set once, at workflow scope; a script that names it is either \
                     scoping different flags to its own command or persisting them, and this \
                     workflow has no leg that does either."
                )
            }
        })
        .collect()
}

/// The guarded names a node's own `env:` may not bind, wherever that node sits.
///
/// Keys are matched case-insensitively and whole. A `RustFlags:` binding is a
/// no-op on the two Unix legs and the authoritative value on `windows-latest`,
/// so a case-sensitive reading refuses it on no platform and a substring reading
/// would refuse `RUSTFLAGS_EXTRA` on all three.
fn rustflags_override_complaints(node: &Yaml, named: &str) -> Vec<String> {
    let Some(env) = field(node, "env") else {
        return Vec::new();
    };
    field_names(env)
        .into_iter()
        .filter_map(|key| guarded_env_key(&key).map(|guarded| (key, guarded)))
        .map(|(key, guarded)| {
            let case = if key == guarded {
                String::new()
            } else {
                format!(
                    " `{key}` and `{guarded}` are one variable on `windows-latest`, where the \
                     process environment is case-insensitive, so this binding is inert on the \
                     Unix legs and authoritative on the Windows one."
                )
            };
            format!(
                "[rustflags-override] {named} binds `{key}` in its own `env:`, which shadows \
                 the workflow-scope `{RUSTFLAGS_KEY}: {RUSTFLAGS_VALUE}` for everything that \
                 node covers.{case} `CODING_STANDARDS.md` §11 records that narrowing the \
                 workflow-scope setting takes the `#[expect]` self-retirement guarantee with \
                 it, silently."
            )
        })
        .collect()
}

/// Every audit, so a mutation is refused by the contract as a whole.
pub(super) fn workflow_complaints(doc: &Yaml) -> Vec<String> {
    let mut out = ci_gate_complaints(doc);
    out.extend(ci_test_job_complaints(doc));
    out.extend(ci_test_windows_job_complaints(doc));
    out.extend(ci_windows_build_witness_complaints(doc));
    out.extend(ci_msrv_job_complaints(doc));
    out.extend(rustflags_complaints(doc));
    out
}

/// The `[kebab-code]` each complaint opens with.
pub(super) fn complaint_codes(complaints: &[String]) -> BTreeSet<String> {
    complaints
        .iter()
        .filter_map(|complaint| {
            complaint
                .strip_prefix('[')
                .and_then(|rest| rest.split(']').next())
                .map(str::to_owned)
        })
        .collect()
}

/// An escape this oracle must refuse, as a mutation of the real workflow.
///
/// Every row is a change that the substring oracle this section replaces
/// accepted, or that its own doc comment enumerated as still open. The first two
/// are `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE`'s; the `MUT-CI-*` names are
/// kept from `.github/scripts/test-docs-consistency.sh`, which recorded them as
/// history when that gate withdrew its claim over this surface -- a parsed
/// document is what lets the claim come back as an equality.
pub(super) struct WorkflowEscape {
    /// The name this escape is recorded under.
    pub(super) name: &'static str,
    /// What passes while the gate does not run.
    pub(super) escape: &'static str,
    /// The job whose block the anchor must appear in exactly once, or `None`
    /// when the mutation is above the jobs (a workflow-level `defaults:`) or
    /// spans a job header (a whole added job), where the anchor must be unique
    /// in the document instead.
    pub(super) job: Option<&'static str>,
    pub(super) anchor: &'static str,
    pub(super) replacement: &'static str,
    /// The complaint code the contract must produce. A code, not a phrase: a
    /// mutation refused for an unrelated reason is not a refusal of this escape.
    pub(super) refused_as: &'static str,
}

pub(super) const WORKFLOW_ESCAPES: &[WorkflowEscape] = &[
    WorkflowEscape {
        name: "MUT-GATE-ECHOED",
        escape: "the job echoes the gate command, succeeds, and the aggregate settles green \
                 while Clippy never examines a denied call in that platform's code. The \
                 standing row's first escape, verbatim.",
        job: Some("lint-macos"),
        anchor: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        replacement: "      - run: echo cargo clippy --all-targets --all-features -- -D warnings\n",
        refused_as: "no-gate-job",
    },
    WorkflowEscape {
        name: "MUT-GATE-JOB-DISABLED",
        escape: "a job-level `if: false` reports success without running the gate",
        job: Some("lint-windows"),
        anchor: "    runs-on: windows-latest\n",
        replacement: "    runs-on: windows-latest\n    if: false\n",
        refused_as: "unexpected-job-field",
    },
    WorkflowEscape {
        name: "MUT-GATE-JOB-DISABLED-EXPRESSION",
        escape: "the same disabling written as an expression. The predecessor searched the \
                 whitespace-normalised block for the literal `if: false` and could not refuse \
                 this form; the field-set equality does not have to know the form.",
        job: Some("lint-macos"),
        anchor: "    runs-on: macos-latest\n",
        replacement: "    runs-on: macos-latest\n    if: ${{ false }}\n",
        refused_as: "unexpected-job-field",
    },
    WorkflowEscape {
        name: "MUT-GATE-JOB-ABSOLVED",
        escape: "`continue-on-error` at job level: the gate runs, fails, and reports success",
        job: Some("lint"),
        anchor: "    runs-on: ubuntu-latest\n",
        replacement: "    runs-on: ubuntu-latest\n    continue-on-error: true\n",
        refused_as: "unexpected-job-field",
    },
    WorkflowEscape {
        name: "MUT-GATE-STEP-ABSOLVED",
        escape: "the same absolution one level down, on the step that runs the gate",
        job: Some("lint-windows"),
        anchor: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        replacement: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n\
                      \x20       continue-on-error: true\n",
        refused_as: "unexpected-step-field",
    },
    WorkflowEscape {
        name: "MUT-GATE-STEP-RETARGETED",
        escape: "a step-level environment retargets the compile. The `run:` scalar still \
                 matches character for character, the job is green, and Clippy examined a \
                 target whose `#[cfg]` bodies are not this platform's.",
        job: Some("lint-macos"),
        anchor: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        replacement: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n\
                      \x20       env:\n\
                      \x20         CARGO_BUILD_TARGET: x86_64-unknown-linux-gnu\n",
        refused_as: "unexpected-step-field",
    },
    WorkflowEscape {
        name: "MUT-GATE-STEP-CUSTOM-SHELL",
        escape: "a custom shell. GitHub runs `<template> <script file>`, so `shell: 'true'` \
                 runs `true /path/to/script`: the step succeeds, Clippy never runs, and the \
                 `run:` scalar still matches this contract character for character.",
        job: Some("lint-macos"),
        anchor: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        replacement: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n\
                      \x20       shell: 'true'\n",
        refused_as: "step-shell",
    },
    WorkflowEscape {
        name: "MUT-WORKFLOW-DEFAULT-SHELL-SWAPPED",
        escape: "a workflow-level `defaults.run.shell` swaps the interpreter under every \
                 `run:` step at once, and no step declares anything for a step-only reading \
                 to notice. The aggregate's bash script is the one that breaks loudest; the \
                 gates are the ones that break quietly.",
        job: None,
        anchor: "\njobs:\n",
        replacement: "\ndefaults:\n  run:\n    shell: pwsh\n\njobs:\n",
        refused_as: "step-shell",
    },
    WorkflowEscape {
        name: "MUT-TEST-MATRIX-EXCLUDED",
        escape: "`exclude:` removes a runner that `os:` still lists. Every check that reads \
                 `os:` passes while the fixtures never run on that platform -- the matrix \
                 half of `PR5-MACOS-CLIPPY-NEVER-RUN`.",
        job: Some("test"),
        anchor: "        os: [ubuntu-latest, macos-latest]\n",
        replacement: "        os: [ubuntu-latest, macos-latest]\n\
                      \x20       exclude:\n\
                      \x20         - os: macos-latest\n",
        refused_as: "test-job-matrix",
    },
    WorkflowEscape {
        name: "MUT-AGGREGATE-STEM-COLLISION",
        escape: "two job ids that normalise to one gate variable stem. Every collection the \
                 aggregate's checks derive from stems is a set or a map, so the collision \
                 collapses both the expectation and the reality and the equalities compare \
                 equal -- one of the two gates is then unreadable and unrequired.",
        job: None,
        anchor: "  merge-gate:\n",
        replacement: "  lint_windows:\n\
                      \x20   name: lint (collision)\n\
                      \x20   runs-on: ubuntu-latest\n\
                      \x20   timeout-minutes: 5\n\
                      \x20   steps:\n\
                      \x20     - run: echo collision\n\
                      \n\
                      \x20 merge-gate:\n",
        refused_as: "aggregate-stem-collision",
    },
    WorkflowEscape {
        name: "MUT-GATE-RUNNER-DRIFT",
        escape: "the macOS leg moved to Ubuntu: two jobs run the gate on one platform and no \
                 job compiles `#[cfg(target_os = \"macos\")]` under Clippy. \
                 `PR5-MACOS-CLIPPY-NEVER-RUN` is the ledger row for the state this restores.",
        job: Some("lint-macos"),
        anchor: "    runs-on: macos-latest\n",
        replacement: "    runs-on: ubuntu-latest\n",
        refused_as: "no-gate-job",
    },
    WorkflowEscape {
        name: "MUT-AGGREGATE-NEEDS-DROPPED",
        escape: "a gate the aggregate does not depend on: branch protection settles green \
                 while the Windows denial gate is red. `PR5D-MSVC-CLIPPY-NEVER-RUN`.",
        job: Some("merge-gate"),
        anchor: "    needs: [lint, lint-windows, lint-macos, msrv, test, test-windows]\n",
        replacement: "    needs: [lint, lint-macos, msrv, test, test-windows]\n",
        refused_as: "aggregate-needs",
    },
    WorkflowEscape {
        name: "MUT-AGGREGATE-ENV-DECOY",
        escape: "a binding that names one gate and reads another's result. It satisfies any \
                 existence check, reads a passing sibling, and with `always()` on the \
                 aggregate reports the required check green over a red leaf. Measured: the \
                 predecessor of this assertion accepted exactly this.",
        job: Some("merge-gate"),
        anchor: "          LINT_MACOS_RESULT: ${{ needs.lint-macos.result }}\n",
        replacement: "          LINT_MACOS_RESULT: ${{ needs.lint-windows.result }}\n",
        refused_as: "aggregate-env",
    },
    WorkflowEscape {
        name: "MUT-AGGREGATE-LOOP-DECOY",
        escape: "the loop omits a gate while the omitted name still appears on the line. The \
                 shape the predecessor's doc enumerated as the escape it could not close: \
                 `requires.split_whitespace().any(|word| word == looped)` passes on the \
                 trailing mention.",
        job: Some("merge-gate"),
        anchor: "          for gate in LINT LINT_WINDOWS LINT_MACOS MSRV TEST TEST_WINDOWS; do\n",
        replacement: "          for gate in LINT LINT_WINDOWS MSRV TEST TEST_WINDOWS; do : LINT_MACOS\n",
        refused_as: "aggregate-loop",
    },
    WorkflowEscape {
        name: "MUT-AGGREGATE-ALWAYS-DROPPED",
        escape: "without `always()` a failed dependency skips the aggregate, and a skipped \
                 required check never settles at all",
        job: Some("merge-gate"),
        anchor: "    if: always()\n",
        replacement: "    if: success()\n",
        refused_as: "aggregate-condition",
    },
    WorkflowEscape {
        name: "MUT-AGGREGATE-CONTEXT-RENAMED",
        escape: "branch protection requires one context name; a renamed job publishes a \
                 different one and the required check waits forever",
        job: Some("merge-gate"),
        anchor: "    name: upstroke-ci\n",
        replacement: "    name: upstroke-ci-aggregate\n",
        refused_as: "aggregate-context-name",
    },
    WorkflowEscape {
        name: "MUT-DUPLICATE-KEY",
        escape: "a second `runs-on:` in the same job. Under a last-one-wins parser every \
                 equality in this section reads the winner and the mutation hides in the \
                 loser; this is the property the dependency was chosen for, executed.",
        job: Some("lint-windows"),
        anchor: "    runs-on: windows-latest\n",
        replacement: "    runs-on: windows-latest\n    runs-on: ubuntu-latest\n",
        refused_as: "parse",
    },
    WorkflowEscape {
        name: "MUT-CI-CARGO-TEST-STEP-DELETED",
        escape: "the job that runs these fixtures stops running them. Named in \
                 `test-docs-consistency.sh` as a mutation that gate's withdrawn claim could \
                 not kill.",
        job: Some("test"),
        anchor: "      - run: cargo test --all-targets --all-features\n",
        replacement: "",
        refused_as: "test-job-command",
    },
    WorkflowEscape {
        name: "MUT-CI-CARGO-TEST-STEP-SKIPPED",
        escape: "the same, by disabling the job rather than deleting the step. The other \
                 mutation `test-docs-consistency.sh` kept as history.",
        job: Some("test"),
        anchor: "    runs-on: ${{ matrix.os }}\n",
        replacement: "    runs-on: ${{ matrix.os }}\n    if: false\n",
        refused_as: "unexpected-job-field",
    },
    WorkflowEscape {
        name: "MUT-CI-STOPS-INSTALLING-CLIPPY",
        escape: "`clippy-driver` is a test dependency of the job that runs these fixtures. \
                 The first version of that test looked for the word `clippy` in the job's \
                 text and the nine-line comment above the line spelled it, so deleting the \
                 line left the test green -- `PR4-CENSUS-COMMENT-ORACLE`, in the test whose \
                 purpose is to answer which command runs this.",
        job: Some("test"),
        anchor: "          components: clippy\n",
        replacement: "",
        refused_as: "test-job-toolchain",
    },
    WorkflowEscape {
        name: "MUT-CI-TEST-MATRIX-NARROWED",
        escape: "the hosted fixtures run on one platform and the other goes unexercised, \
                 while every check that names the command still passes",
        job: Some("test"),
        anchor: "        os: [ubuntu-latest, macos-latest]\n",
        replacement: "        os: [ubuntu-latest]\n",
        refused_as: "test-job-matrix",
    },
    WorkflowEscape {
        name: "MUT-TEST-MATRIX-KEEPS-WINDOWS",
        escape: "the hosted matrix quietly re-admits Windows. Two jobs then run the Windows \
                 suite, and the one whose duration this contract retired is back on the \
                 critical path with every other check passing.",
        job: Some("test"),
        anchor: "        os: [ubuntu-latest, macos-latest]\n",
        replacement: "        os: [windows-latest, ubuntu-latest, macos-latest]\n",
        refused_as: "test-job-matrix",
    },
    WorkflowEscape {
        name: "MUT-TEST-WINDOWS-REHOSTED",
        escape: "the self-hosted job moved back to `windows-latest` as a scalar `runs-on:`. \
                 Every step still matches character for character; only the machine, and \
                 with it the twelve minutes, changed.",
        job: Some("test-windows"),
        anchor: "    runs-on: [self-hosted, windows, winguest]\n",
        replacement: "    runs-on: windows-latest\n",
        refused_as: "test-windows-runner",
    },
    WorkflowEscape {
        name: "MUT-TEST-WINDOWS-LABEL-DROPPED",
        escape: "the labels loosened to `[self-hosted, windows]`, which any Windows runner \
                 the account ever registers satisfies; the curated image is named by the \
                 label this drops",
        job: Some("test-windows"),
        anchor: "    runs-on: [self-hosted, windows, winguest]\n",
        replacement: "    runs-on: [self-hosted, windows]\n",
        refused_as: "test-windows-runner",
    },
    WorkflowEscape {
        name: "MUT-TEST-WINDOWS-COMMAND-DELETED",
        escape: "the self-hosted job checks out, configures git, and runs nothing",
        job: Some("test-windows"),
        anchor: "      - run: cargo test --all-targets --all-features\n",
        replacement: "",
        refused_as: "test-windows-command",
    },
    WorkflowEscape {
        name: "MUT-TEST-WINDOWS-DISABLED",
        escape: "a step-level `if: false` on the self-hosted job's test step: the job reports \
                 success having run nothing. A job-level `if:` is not this escape -- the job \
                 reports `skipped` and the aggregate, which accepts only `success`, fails.",
        job: Some("test-windows"),
        anchor: "      - run: cargo test --all-targets --all-features\n",
        replacement: "      - run: cargo test --all-targets --all-features\n\
                      \x20       if: false\n",
        refused_as: "unexpected-step-field",
    },
    WorkflowEscape {
        name: "MUT-TEST-WINDOWS-RENAMED-AWAY",
        escape: "the job renamed: the pinned name has no job. Not a false green -- the \
                 aggregate's derived `needs` refuses the renamed workflow too -- but a lost \
                 handle: every pin above hangs from this name, so the contract must notice the \
                 name going before it notices nothing else",
        job: None,
        anchor: "\n  test-windows:\n",
        replacement: "\n  test-windows-hosted:\n",
        refused_as: "test-windows-missing",
    },
    WorkflowEscape {
        name: "MUT-TEST-WINDOWS-RUN-RETARGETED",
        escape: "the identity step of the self-hosted job fetches `master` and checks it out \
                 detached before the pinned test command runs. Labels, fields, shell, the \
                 input-free checkout and the exact command all still match; the suite tests \
                 `master`.",
        job: Some("test-windows"),
        anchor: "          git config --global user.name \"upstroke CI\"\n",
        replacement: "          git config --global user.name \"upstroke CI\"\n\
                      \x20         git fetch origin master\n\
                      \x20         git checkout --detach FETCH_HEAD\n",
        refused_as: "test-windows-run-script",
    },
    WorkflowEscape {
        name: "MUT-TEST-RUN-RETARGETED",
        escape: "the same retarget on the hosted test matrix: two platforms test `master` \
                 while every pin still matches",
        job: Some("test"),
        anchor: "          git config --global user.name \"upstroke CI\"\n",
        replacement: "          git config --global user.name \"upstroke CI\"\n\
                      \x20         git fetch origin master\n\
                      \x20         git checkout --detach FETCH_HEAD\n",
        refused_as: "test-job-run-script",
    },
    WorkflowEscape {
        name: "MUT-GATE-RUN-RETARGETED",
        escape: "a step ahead of the Windows Clippy gate and the build witness checks out \
                 `master`; both pinned commands then run against a tree the candidate never \
                 touched, and a Windows-only denial or link failure in the candidate goes green",
        job: Some("lint-windows"),
        anchor: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        replacement: "      - run: git fetch origin master && git checkout --detach FETCH_HEAD\n\
                      \x20     - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        refused_as: "gate-run-script",
    },
    WorkflowEscape {
        name: "MUT-MSRV-RUN-RETARGETED",
        escape: "the MSRV leg checks out `master` ahead of its pinned command; the floor is \
                 verified on a tree the candidate never touched",
        job: Some("msrv"),
        anchor: "      - run: cargo check --locked --all-targets --all-features\n",
        replacement: "      - run: git fetch origin master && git checkout --detach FETCH_HEAD\n\
                      \x20     - run: cargo check --locked --all-targets --all-features\n",
        refused_as: "msrv-run-script",
    },
    WorkflowEscape {
        name: "MUT-STEP-USES-UNPINNED",
        escape: "the self-hosted job's checkout floats to `actions/checkout@v4`: the same \
                 action at whatever commit the tag points to, which is code nobody here \
                 reviewed with a checkout of its own",
        job: Some("test-windows"),
        anchor: "      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n",
        replacement: "      - uses: actions/checkout@v4\n",
        refused_as: "unpinned-action",
    },
    WorkflowEscape {
        name: "MUT-WITNESS-CHECKOUT-REF",
        escape: "the witness carrier's checkout points at `master`: the current-stable build \
                 witness links a tree the candidate never touched while the guest, one stable \
                 behind, links and passes the candidate",
        job: Some("lint-windows"),
        anchor: "      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n",
        replacement: "      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n\
                      \x20       with:\n\
                      \x20         ref: master\n",
        refused_as: "gate-checkout",
    },
    WorkflowEscape {
        name: "MUT-TEST-WINDOWS-CHECKOUT-REF",
        escape: "the self-hosted job's checkout points at `master`. Every hosted leg still \
                 reads the candidate, the self-hosted leg tests a tree the candidate never \
                 touched, and a Windows-only regression goes green.",
        job: Some("test-windows"),
        anchor: "      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n",
        replacement: "      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n\
                      \x20       with:\n\
                      \x20         ref: master\n",
        refused_as: "test-windows-checkout",
    },
    WorkflowEscape {
        name: "MUT-TEST-CHECKOUT-REF",
        escape: "the hosted test matrix checks out `master`: two platforms test a tree the \
                 candidate never touched while Clippy and MSRV read the candidate",
        job: Some("test"),
        anchor: "      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n",
        replacement: "      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n\
                      \x20       with:\n\
                      \x20         ref: master\n",
        refused_as: "test-job-checkout",
    },
    WorkflowEscape {
        name: "MUT-WINDOWS-BUILD-WITNESS-DELETED",
        escape: "the hosted `cargo build --all-targets` step removed. Clippy and MSRV still \
                 type-check the Windows tree on GitHub's runner, but nothing on current \
                 stable code-generates or links it: a Windows-only codegen or link failure \
                 passes every hosted leg while the guest, one stable behind, links and passes.",
        job: Some("lint-windows"),
        anchor: "      - run: cargo build --all-targets --all-features\n",
        replacement: "",
        refused_as: "windows-build-witness",
    },
    WorkflowEscape {
        name: "MUT-WINDOWS-BUILD-WITNESS-NARROWED",
        escape: "the witness narrowed to `cargo test --no-run`, which builds only test-profile \
                 artifacts: a `#[cfg(all(windows, not(test)))]` body in a binary is never \
                 linked on current stable, and a link failure there passes every hosted leg",
        job: Some("lint-windows"),
        anchor: "      - run: cargo build --all-targets --all-features\n",
        replacement: "      - run: cargo test --no-run --all-targets --all-features\n",
        refused_as: "windows-build-witness",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-WEAKENED",
        escape: "warnings allowed instead of denied, at workflow scope. Every leg still runs \
                 every command, every job is green, and `unfulfilled_lint_expectations` -- \
                 warn-by-default -- stops retiring the `#[expect]`s that `CODING_STANDARDS.md` \
                 §11 says self-retire, so a suppression that no longer suppresses anything \
                 reads as enforcement forever.",
        job: None,
        anchor: "  RUSTFLAGS: -D warnings\n",
        replacement: "  RUSTFLAGS: -A warnings\n",
        refused_as: "rustflags",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-ALLOW-APPENDED",
        escape: "an allow appended after the deny. The line still *contains* `-D warnings`, so \
                 every substring reading of it passes, while the effect denylist this whole \
                 file exists to enforce is switched off on every leg at once. The exact reason \
                 the pin is an equality.",
        job: None,
        anchor: "  RUSTFLAGS: -D warnings\n",
        replacement: "  RUSTFLAGS: -D warnings -A clippy::disallowed_methods\n",
        refused_as: "rustflags",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-DELETED",
        escape: "the binding is gone and the `env:` block it lived in stays, so the workflow \
                 still declares the field this contract's top-level equality requires",
        job: None,
        anchor: "  RUSTFLAGS: -D warnings\n",
        replacement: "",
        refused_as: "rustflags",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-VALUE-EMPTIED",
        escape: "the key stays and the value goes. YAML reads the empty value as null, so a \
                 reader that asks whether the name is bound gets yes and a reader that asks \
                 what it is bound to gets nothing.",
        job: None,
        anchor: "  RUSTFLAGS: -D warnings\n",
        replacement: "  RUSTFLAGS:\n",
        refused_as: "rustflags",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-ENCODED-AT-WORKFLOW-SCOPE",
        escape: "`CARGO_ENCODED_RUSTFLAGS` beside the pinned line. Cargo reads it in \
                 preference to `RUSTFLAGS` and ignores `RUSTFLAGS` entirely when it is set, so \
                 the pin below it is read past rather than obeyed -- and it is still there, \
                 character for character, for any equality that reads only that line.",
        job: None,
        anchor: "  RUSTFLAGS: -D warnings\n",
        replacement: "  RUSTFLAGS: -D warnings\n  CARGO_ENCODED_RUSTFLAGS: ''\n",
        refused_as: "rustflags",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-JOB-OVERRIDE",
        escape: "the narrowing `CODING_STANDARDS.md` §11 names, one job at a time: a job-level \
                 `env:` shadows the workflow-scope value for every step it covers while the \
                 pinned line stays untouched at the top of the file. The `msrv` leg is the \
                 mutation site because until this change it had no field set to refuse an \
                 `env:` at all.",
        job: Some("msrv"),
        anchor: "    timeout-minutes: 15\n",
        replacement: "    timeout-minutes: 15\n\
                      \x20   env:\n\
                      \x20     RUSTFLAGS: -A warnings\n",
        refused_as: "rustflags-override",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-ENCODED-JOB-OVERRIDE",
        escape: "the same narrowing under the name Cargo prefers, so the job's `RUSTFLAGS` is \
                 not rewritten but ignored. Nothing in this workflow mentions `RUSTFLAGS` in \
                 that job for a reader to notice.",
        job: Some("test"),
        anchor: "    timeout-minutes: 30\n",
        replacement: "    timeout-minutes: 30\n\
                      \x20   env:\n\
                      \x20     CARGO_ENCODED_RUSTFLAGS: ''\n",
        refused_as: "rustflags-override",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-STEP-OVERRIDE",
        escape: "the narrowing one level down, in the one step this contract allows an `env:` \
                 at all -- the aggregate's. A step-level binding is the smallest form of the \
                 same defect and the one a field-set equality cannot see, because the field is \
                 legal there.",
        job: Some("merge-gate"),
        anchor: "          LINT_RESULT: ${{ needs.lint.result }}\n",
        replacement: "          LINT_RESULT: ${{ needs.lint.result }}\n\
                      \x20         RUSTFLAGS: -A warnings\n",
        refused_as: "rustflags-override",
    },
    WorkflowEscape {
        name: "MUT-MSRV-TOOLCHAIN-DRIFTED",
        escape: "the leg named for the floor installs `stable`. It runs, it passes, and it is \
                 evidence about whichever compiler the runner shipped that week rather than \
                 about the version `Cargo.toml` publishes to crates.io.",
        job: Some("msrv"),
        anchor: "          toolchain: 1.85.0\n",
        replacement: "          toolchain: stable\n",
        refused_as: "msrv-job-toolchain",
    },
    WorkflowEscape {
        name: "MUT-MSRV-TOOLCHAIN-AHEAD-OF-MANIFEST",
        escape: "a real version, above the declared floor. `test-docs-consistency.sh`'s C2 \
                 refuses this shape too, by grepping a text block. The difference is \
                 exactness, not coverage: C2 accepts `rust-version` \"or a patch release of \
                 it\", so it reads `toolchain: 1.85` as agreement, and that name resolves to \
                 the newest 1.85.x rather than to the `cargo +1.85.0` the documents publish.",
        job: Some("msrv"),
        anchor: "          toolchain: 1.85.0\n",
        replacement: "          toolchain: 1.90.0\n",
        refused_as: "msrv-job-toolchain",
    },
    WorkflowEscape {
        name: "MUT-MSRV-COMMAND-UNLOCKED",
        escape: "`--locked` dropped. Cargo re-resolves `Cargo.lock` forward past the exact \
                 pins this manifest carries for the floor -- `globset =0.4.19` and \
                 `yaml-rust2 =0.12.0` are both pinned against exactly this -- so the leg \
                 compiles a dependency set no release ships and reports green over a floor it \
                 never tested.",
        job: Some("msrv"),
        anchor: "      - run: cargo check --locked --all-targets --all-features\n",
        replacement: "      - run: cargo check --all-targets --all-features\n",
        refused_as: "msrv-job-command",
    },
    WorkflowEscape {
        name: "MUT-MSRV-COMMAND-ECHOED",
        escape: "the leg echoes its command and succeeds, `MUT-GATE-ECHOED`'s shape one job \
                 over. `test-docs-consistency.sh` withdrew every claim about which commands CI \
                 runs precisely because a text checker cannot tell these apart.",
        job: Some("msrv"),
        anchor: "      - run: cargo check --locked --all-targets --all-features\n",
        replacement: "      - run: echo cargo check --locked --all-targets --all-features\n",
        refused_as: "msrv-job-command",
    },
    WorkflowEscape {
        name: "MUT-MSRV-MATRIX-NARROWED",
        escape: "the floor is checked on Linux only. A dependency that raises its MSRV behind \
                 a `cfg` -- this manifest has a `cfg(windows)` and a `cfg(unix)` dependency \
                 table -- breaks on a platform this leg no longer visits.",
        job: Some("msrv"),
        anchor: "        os: [ubuntu-latest, windows-latest, macos-latest]\n",
        replacement: "        os: [ubuntu-latest]\n",
        refused_as: "msrv-job-matrix",
    },
    WorkflowEscape {
        name: "MUT-MSRV-MATRIX-EXCLUDED",
        escape: "the same narrowing with `os:` left listing all three, so every reading of \
                 `os:` passes while the Windows floor is never compiled. \
                 `MUT-TEST-MATRIX-EXCLUDED`'s shape on the leg that had no strategy contract.",
        job: Some("msrv"),
        anchor: "        os: [ubuntu-latest, windows-latest, macos-latest]\n",
        replacement: "        os: [ubuntu-latest, windows-latest, macos-latest]\n\
                      \x20       exclude:\n\
                      \x20         - os: windows-latest\n",
        refused_as: "msrv-job-matrix",
    },
    WorkflowEscape {
        name: "MUT-MSRV-FAIL-FAST-ENABLED",
        escape: "one platform's floor failure cancels the other two before they report, so a \
                 break on two platforms is indistinguishable from a break on one and the \
                 second is invisible until the first is fixed",
        job: Some("msrv"),
        anchor: "      fail-fast: false\n",
        replacement: "      fail-fast: true\n",
        refused_as: "msrv-job-matrix",
    },
    WorkflowEscape {
        name: "MUT-MSRV-JOB-ABSOLVED",
        escape: "`continue-on-error` on the leg. This is the one the aggregate cannot catch: a \
                 *skipped* job still reports `skipped` and the aggregate's loop demands \
                 `success`, but an absolved job reports `success` after its check failed, so \
                 `upstroke-ci` settles green over an unmet floor on all three platforms.",
        job: Some("msrv"),
        anchor: "    runs-on: ${{ matrix.os }}\n",
        replacement: "    runs-on: ${{ matrix.os }}\n    continue-on-error: true\n",
        refused_as: "msrv-job-field",
    },
    WorkflowEscape {
        name: "MUT-MSRV-STEP-ABSOLVED",
        escape: "the same absolution one level down, on the step that runs the check: the step \
                 fails, the job succeeds, and the leg is green",
        job: Some("msrv"),
        anchor: "      - run: cargo check --locked --all-targets --all-features\n",
        replacement: "      - run: cargo check --locked --all-targets --all-features\n\
                      \x20       continue-on-error: true\n",
        refused_as: "msrv-job-step-field",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-PERSISTED-VIA-GITHUB-ENV",
        escape: "the narrowing with no `env:` mapping anywhere in the document. One `run:` \
                 line writes the job-scoped environment file, and every step after it in the \
                 job -- the `cargo check` on the next line -- compiles under `-A warnings`. \
                 Every field-set equality passes, the pinned workflow line is untouched, and \
                 the whole defect lives inside a scalar this contract used to treat as \
                 opaque.",
        job: Some("msrv"),
        anchor: "      - run: cargo check --locked --all-targets --all-features\n",
        replacement: "      - run: echo \"RUSTFLAGS=-A warnings\" >> \"$GITHUB_ENV\"\n\
                      \x20     - run: cargo check --locked --all-targets --all-features\n",
        refused_as: "rustflags-persisted",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-PERSISTED-VIA-EXPRESSION",
        escape: "the same write through `${{ github.env }}`, the expression form of the same \
                 file, and under the name Cargo prefers. Nothing in the line spells \
                 `GITHUB_ENV`, so a reading that looks for that one token misses it entirely.",
        job: Some("lint"),
        anchor: "      - run: cargo fmt --check\n",
        replacement: "      - run: echo \"CARGO_ENCODED_RUSTFLAGS=\" >> ${{ github.env }}\n\
                      \x20     - run: cargo fmt --check\n",
        refused_as: "rustflags-persisted",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-PERSISTED-VIA-POWERSHELL",
        escape: "the same write in PowerShell, on the leg whose default shell is `pwsh`. \
                 `Add-Content -Path $env:GITHUB_ENV` is how the file is reached there, and it \
                 shares no syntax at all with the bash form -- which is the point of covering \
                 the shells the runners actually resolve to.",
        job: Some("lint-windows"),
        anchor: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        replacement: "      - run: Add-Content -Path $env:GITHUB_ENV -Value \"RUSTFLAGS=-A warnings\"\n\
                      \x20     - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        refused_as: "rustflags-persisted",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-PERSISTED-VIA-HEREDOC",
        escape: "the same write with the name and the redirection on different lines. A \
                 heredoc is the form that defeats any reading anchored to `echo` or to a \
                 single line, and the job it lands in is the one that runs these fixtures.",
        job: Some("test"),
        anchor: "      - run: cargo test --all-targets --all-features\n",
        replacement: "      - run: |\n\
                      \x20         cat >> \"$GITHUB_ENV\" <<'EOF'\n\
                      \x20         RUSTFLAGS=-A warnings\n\
                      \x20         EOF\n\
                      \x20     - run: cargo test --all-targets --all-features\n",
        refused_as: "rustflags-persisted",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-COMMAND-SCOPED",
        escape: "the narrowing without the env file at all: the flags are scoped to one \
                 command on one line. It persists nothing, so it is the arm that proves the \
                 script scan is not merely a `GITHUB_ENV` detector -- and a leg that names the \
                 variable at all is doing something this contract has no model for.",
        job: Some("lint-macos"),
        anchor: "      - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        replacement: "      - run: RUSTFLAGS=-A warnings cargo build --all-targets\n\
                      \x20     - run: cargo clippy --all-targets --all-features -- -D warnings\n",
        refused_as: "rustflags-in-script",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-JOB-OVERRIDE-LOWERCASE",
        escape: "the job-level narrowing spelled in lower case. On the two Unix legs it binds \
                 a different variable and does nothing; on `windows-latest` the process \
                 environment is case-insensitive and it *is* `RUSTFLAGS`. A case-sensitive \
                 reading refuses it on no platform, which is the Linux-only half of a \
                 platform-shaped mutation.",
        job: Some("msrv"),
        anchor: "    timeout-minutes: 15\n",
        replacement: "    timeout-minutes: 15\n\
                      \x20   env:\n\
                      \x20     rustflags: -A warnings\n",
        refused_as: "rustflags-override",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-STEP-OVERRIDE-MIXED-CASE",
        escape: "the same in mixed case, one level down, in the one step this contract allows \
                 an `env:` at all",
        job: Some("merge-gate"),
        anchor: "          LINT_RESULT: ${{ needs.lint.result }}\n",
        replacement: "          LINT_RESULT: ${{ needs.lint.result }}\n\
                      \x20         RustFlags: -A warnings\n",
        refused_as: "rustflags-override",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-WORKFLOW-CASE-VARIANT",
        escape: "a case variant beside the pinned line at workflow scope. The pin still reads \
                 back character for character, and on `windows-latest` the two keys are one \
                 variable whose winner this document does not decide.",
        job: None,
        anchor: "  RUSTFLAGS: -D warnings\n",
        replacement: "  RUSTFLAGS: -D warnings\n  Rustflags: -A warnings\n",
        refused_as: "rustflags",
    },
    WorkflowEscape {
        name: "MUT-RUSTFLAGS-ENCODED-LOWERCASE",
        escape: "the encoded name in lower case. `MUT-RUSTFLAGS-ENCODED-AT-WORKFLOW-SCOPE` \
                 covers the upper-case form; this is the one an exact-key reading would let \
                 through on the platform where it works.",
        job: None,
        anchor: "  RUSTFLAGS: -D warnings\n",
        replacement: "  RUSTFLAGS: -D warnings\n  cargo_encoded_rustflags: ''\n",
        refused_as: "rustflags",
    },
    WorkflowEscape {
        name: "MUT-MSRV-CHECK-BEFORE-TOOLCHAIN",
        escape: "both steps present, both exact, and the check above the install. \
                 `dtolnay/rust-toolchain` selects the toolchain for the steps that follow it, \
                 so the check compiles on whatever the runner image preinstalled -- stable -- \
                 and the leg is green about a version this crate publishes no floor for. \
                 Nothing in the presence-and-equality contract could see it: the step count, \
                 the command, the matrix and the toolchain scalar are all unchanged.",
        job: Some("msrv"),
        anchor: "      - uses: dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c # stable\n\
                 \x20       with:\n\
                 \x20         toolchain: 1.85.0\n\
                 \x20     - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6 # v2.9.2\n\
                 \x20     - run: cargo check --locked --all-targets --all-features\n",
        replacement: "      - run: cargo check --locked --all-targets --all-features\n\
                      \x20     - uses: dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c # stable\n\
                      \x20       with:\n\
                      \x20         toolchain: 1.85.0\n\
                      \x20     - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6 # v2.9.2\n",
        refused_as: "msrv-job-order",
    },
];

/// Replace `anchor` with `replacement` inside one job's block.
///
/// Scoped to the block, and the anchor must occur in it exactly once: a mutation
/// that lands somewhere unintended, or that no longer matches because the
/// workflow moved, is a mutation nobody measured. Both are failures here rather
/// than a quietly weaker test.
pub(super) fn mutate_workflow(
    text: &str,
    job: Option<&str>,
    anchor: &str,
    replacement: &str,
) -> String {
    match job {
        Some(job) => mutate_job(text, job, anchor, replacement),
        None => {
            let hits = text.matches(anchor).count();
            assert_eq!(
                hits, 1,
                "the workflow contains {hits} copies of the mutation anchor, not one. It \
                 moved and this mutation measures nothing:\n{anchor}"
            );
            text.replacen(anchor, replacement, 1)
        }
    }
}

fn mutate_job(text: &str, job: &str, anchor: &str, replacement: &str) -> String {
    let jobs_at = text
        .find("\njobs:\n")
        .unwrap_or_else(|| panic!("{CI_WORKFLOW} has no `jobs:` mapping"));
    let header = format!("\n  {job}:\n");
    let start = jobs_at
        + text[jobs_at..]
            .find(&header)
            .unwrap_or_else(|| panic!("{CI_WORKFLOW} has no `{job}` job"))
        + 1;
    let rest = &text[start + header.len() - 1..];
    let end = rest
        .lines()
        .scan(0_usize, |offset, line| {
            let at = *offset;
            *offset += line.len() + 1;
            Some((at, line))
        })
        .find(|(at, line)| {
            *at > 0
                && line.len() > 2
                && line.starts_with("  ")
                && !line[2..].starts_with(' ')
                && line.ends_with(':')
                && line[2..line.len() - 1]
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        })
        .map_or(rest.len(), |(at, _)| at);
    let block = &rest[..end];
    let hits = block.matches(anchor).count();
    assert_eq!(
        hits, 1,
        "the `{job}` block contains {hits} copies of the mutation anchor, not one. The \
         workflow moved and this mutation measures nothing:\n{anchor}"
    );
    format!(
        "{}{}{}",
        &text[..start + header.len() - 1],
        block.replacen(anchor, replacement, 1),
        &rest[end..]
    )
}
