//! Lane B's suite: container `RunnerPolicy` resolution, the rebuild-from-record
//! path, and the schema-1..3 container refusal.
//!
//! Kept out of `resolve.rs` so `effects::production_region` — which cuts a
//! source at its **first** `#[cfg(test)]` — sees that module whole
//! (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).

// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/runner/container.rs` -- the same shape
// `src/runner/container/tests.rs` and `src/runner/container/census/tests.rs`
// have.
//
// `PR6-LANEF-004`: this file states its level **of its own** rather than
// inheriting the Container funnel's inner `#![allow(...)]` through the module
// tree. `resolve.rs`, the production half, carries `#![deny(...)]` for all
// three and reaches no denied primitive at all.
//
// WHAT IT NEEDS THE ALLOW FOR, and the residual is stated rather than implied:
// it builds real temporary Git repositories (`std::process::Command` running
// `git`, `fs::write`, `fs::create_dir_all`, `fs::remove_dir_all`) and wraps a
// `ContainerRuntime` whose four effectful methods it delegates. It is the one
// child of this directory that allows `clippy::disallowed_types` as well as
// `clippy::disallowed_methods`, so a `std::process::Command` here is NOT a
// build error the way it is in the two sibling test modules -- a real
// difference, recorded here and in `effects/allowlist.toml` instead of being
// left to a reviewer to discover. `src/events/log/tests.rs` and
// `src/engine/tests.rs` are the precedent for a test module needing both.
// `clippy::disallowed_macros` is re-denied, so a `println!` is still an error.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(clippy::disallowed_macros)]

#[cfg(test)]
mod this_file_is_test_only {}

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::agent::{AdapterSource, AgentAdapter};
use crate::config::{EngineLimits, RunnerMount, RunnerSelection};
use crate::engine::{ResumeOptions, RunOptions};
use crate::runner::container::FakeRuntime;
use crate::runner::container::runtime::{ContainerTrace, Liveness};
use crate::runner::policy::{canonical_bytes, runner_policy_sha256};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const REFERENCE: &str = "upstroke/ci:3.2";
const IMAGE_ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const OTHER_ID: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const MANIFEST: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const CLAUDE_VOLUME: &str = "upstroke-creds-claude-code";
const CODEX_VOLUME: &str = "upstroke-creds-codex";

/// The two credential volumes, as an independent table.
///
/// Written out rather than read back from a `RunnerSelection`, so a test that
/// compares a record against this is comparing it against a value nothing under
/// test produced.
fn volumes() -> BTreeMap<String, String> {
    let mut volumes = BTreeMap::new();
    volumes.insert("claude-code".to_owned(), CLAUDE_VOLUME.to_owned());
    volumes.insert("codex".to_owned(), CODEX_VOLUME.to_owned());
    volumes
}

/// The `[runner]` selection every fixture starts from.
fn selection() -> RunnerSelection {
    RunnerSelection {
        kind: RunnerKind::Container,
        image: Some(REFERENCE.to_owned()),
        credential_volumes: volumes(),
        mounts: Vec::new(),
        from_config: true,
    }
}

/// A runtime holding the image at [`IMAGE_ID`] under [`REFERENCE`], with both
/// credential volumes present. Every refusal fixture below is this, minus one
/// thing.
fn ready_runtime() -> (FakeRuntime, ContainerTrace) {
    let trace = ContainerTrace::recording();
    let runtime = FakeRuntime::new(trace.clone());
    runtime.add_image(IMAGE_ID, Some(MANIFEST));
    runtime.tag(REFERENCE, IMAGE_ID);
    runtime.add_volume(CLAUDE_VOLUME);
    runtime.add_volume(CODEX_VOLUME);
    (runtime, trace)
}

/// The record a first incarnation would have written, built by hand from
/// INV-23's field list rather than by calling `resolve_container`.
fn recorded() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Container,
        policy: RunnerContract::ContainerV1,
        image: Some(ImageIdentity {
            reference: REFERENCE.to_owned(),
            id: IMAGE_ID.to_owned(),
            digest: Some(MANIFEST.to_owned()),
        }),
        credential_volumes: Some(volumes()),
    }
}

/// A [`RunnerPreflight`] that records what it was asked and can be armed to
/// refuse.
///
/// It also snapshots the runtime trace **at the moment it is called**, which is
/// what makes "before any spawn" a statement about a sequence rather than about
/// a boolean: the snapshot is the prefix of runtime operations that had already
/// happened when the first spawn was about to occur.
struct RecordingPreflight {
    trace: ContainerTrace,
    calls: Mutex<Vec<(RunnerPolicy, Vec<String>)>>,
    refuse: Option<String>,
}

impl RecordingPreflight {
    fn accepting(trace: ContainerTrace) -> Self {
        Self {
            trace,
            calls: Mutex::new(Vec::new()),
            refuse: None,
        }
    }

    fn refusing(trace: ContainerTrace, message: &str) -> Self {
        Self {
            trace,
            calls: Mutex::new(Vec::new()),
            refuse: Some(message.to_owned()),
        }
    }

    fn calls(&self) -> Vec<(RunnerPolicy, Vec<String>)> {
        self.calls.lock().expect("preflight log").clone()
    }

    fn spawns(&self) -> usize {
        self.calls().len()
    }
}

impl RunnerPreflight for RecordingPreflight {
    fn certify(&self, policy: &RunnerPolicy) -> Result<(), UpstrokeError> {
        self.calls
            .lock()
            .expect("preflight log")
            .push((policy.clone(), self.trace.rendered()));
        match &self.refuse {
            None => Ok(()),
            Some(message) => Err(UpstrokeError::Refused {
                message: message.clone(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// 1. Resolution by read-only inspection
// ---------------------------------------------------------------------------

/// The five obligations `pr_sequence[7].scope` packs into one sentence, each
/// read off the resolved record and compared against an independent value.
///
/// Second field held constant: the runtime's whole state — one image, one tag,
/// both volumes — is identical for every assertion. What varies is which field
/// of the record is being asked about.
#[test]
fn the_resolved_container_policy_is_the_record_inv23_describes() {
    let (runtime, _trace) = ready_runtime();
    let policy = resolve_container(&runtime, &selection()).expect("a ready runtime resolves");

    assert_eq!(policy.kind, RunnerKind::Container);
    // "policy container-v1" — the mount, environment, Git-view and supervision
    // contract this binary implements for that kind.
    assert_eq!(policy.policy, RunnerContract::ContainerV1);
    let image = policy.image.as_ref().expect("a container records an image");
    assert_eq!(
        image.reference, REFERENCE,
        "the reference is the operator's, from `[runner] image`"
    );
    assert_eq!(
        image.id, IMAGE_ID,
        "the id is the runtime's immutable answer"
    );
    assert_eq!(
        image.digest.as_deref(),
        Some(MANIFEST),
        "the manifest digest, because this runtime reported one"
    );
    assert_eq!(
        policy.credential_volumes.as_ref(),
        Some(&volumes()),
        "the per-agent credential volume names, verbatim"
    );
    // A run must not start with a record its own resume would reject.
    policy
        .completeness()
        .expect("PR3 accepts the record PR6 resolves");
}

/// The recorded reference is the one an operator wrote, never one the runtime
/// volunteered.
///
/// `ImageInspection` carries **every** reference the runtime says resolves to
/// the id. A resolver that took the record's `reference` from there would make
/// the record its own oracle and "the recorded reference now names another
/// image" unconstructible — `runtime.rs` says so in as many words, and this is
/// the assertion behind it.
///
/// Second field held constant: the id and digest, which are the runtime's in
/// both cells.
#[test]
fn the_recorded_reference_is_the_operators_and_never_the_runtimes() {
    let (runtime, _trace) = ready_runtime();
    // The same image, additionally tagged twice under names nobody configured.
    runtime.tag("mirror.example/upstroke:latest", IMAGE_ID);
    runtime.tag("aaa-sorts-first/upstroke:1", IMAGE_ID);

    let inspection = runtime
        .image_by_reference(REFERENCE)
        .expect("inspects")
        .expect("present");
    assert_eq!(
        inspection.references.len(),
        3,
        "the fixture does not offer the resolver a choice: {:?}",
        inspection.references
    );
    assert_ne!(
        inspection.references.first().map(String::as_str),
        Some(REFERENCE),
        "the configured reference sorts first, so `references[0]` would accidentally be right"
    );

    let policy = resolve_container(&runtime, &selection()).expect("resolves");
    assert_eq!(
        policy.image.expect("image").reference,
        REFERENCE,
        "the record took a reference from the runtime's answer"
    );
}

/// Resolution issues read-only operations only, in the order the scope names.
///
/// "Before any lock or effect" has two halves and this is the effect half:
/// every operation the resolution performs is one [`RuntimeOp::is_effect`]
/// calls false. Asserted as the **sequence**, not as a set, because
/// `probe → reference → volumes` is what "the runtime must already hold the
/// image and the volumes must exist" means in order.
///
/// Second field held constant: the runtime is ready in every cell, so the trace
/// is the full happy-path sequence rather than a truncated one.
#[test]
fn resolution_issues_only_read_only_operations_in_the_scopes_order() {
    let (runtime, trace) = ready_runtime();
    resolve_container(&runtime, &selection()).expect("resolves");

    assert_eq!(
        trace.ops(),
        vec![
            RuntimeOp::Probe,
            RuntimeOp::InspectImageByReference,
            // One per credential volume, in the map's sorted order.
            RuntimeOp::InspectVolume,
            RuntimeOp::InspectVolume,
        ],
        "the inspection sequence moved: {:?}",
        trace.rendered()
    );
    assert!(
        trace.ops().iter().all(|op| !op.is_effect()),
        "resolution reached an effectful operation: {:?}",
        trace.rendered()
    );
    // The volumes are asked about by name, and both of them are.
    let asked: Vec<String> = trace
        .rendered()
        .into_iter()
        .filter(|entry| entry.starts_with("rt:inspect-volume:"))
        .collect();
    assert_eq!(
        asked,
        vec![
            format!("rt:inspect-volume:{CLAUDE_VOLUME}"),
            format!("rt:inspect-volume:{CODEX_VOLUME}"),
        ]
    );
}

/// A digest the runtime does not report, and one it reports as an empty string,
/// both resolve to `None` — and the *record* still separates them.
///
/// Two halves, because collapsing them at the inspection seam is only safe if
/// the encoding underneath has not collapsed them too. INV-23 compares four
/// copies of this record exactly; a canonicalisation in which `None` and
/// `Some("")` agree would let a marker attest a record the fold calls different.
///
/// Second field held constant: the image id and the reference, identical in all
/// three cells, so what varies is only what the runtime said about the manifest.
#[test]
fn a_runtime_reporting_no_digest_and_one_reporting_an_empty_string_both_resolve_to_none() {
    for (label, reported) in [("absent", None), ("empty string", Some(""))] {
        let trace = ContainerTrace::recording();
        let runtime = FakeRuntime::new(trace);
        runtime.add_image(IMAGE_ID, reported);
        runtime.tag(REFERENCE, IMAGE_ID);
        runtime.add_volume(CLAUDE_VOLUME);
        runtime.add_volume(CODEX_VOLUME);

        let policy = resolve_container(&runtime, &selection()).expect("resolves");
        assert_eq!(
            policy.image.as_ref().expect("image").digest,
            None,
            "`{label}`: a runtime that reported no usable manifest digest still put one in the \
             record"
        );
        policy
            .completeness()
            .expect("a container without a digest is a complete record");
    }

    // And the encoding underneath has not collapsed them, so a record that
    // acquired an empty digest by some other route is still a different record.
    let mut absent = recorded();
    let mut empty = recorded();
    absent.image.as_mut().expect("image").digest = None;
    empty.image.as_mut().expect("image").digest = Some(String::new());
    assert_ne!(absent, empty, "the two fixtures are the same value");
    assert_ne!(
        canonical_bytes(&absent),
        canonical_bytes(&empty),
        "`digest: None` and `digest: Some(\"\")` canonicalize alike"
    );
    assert_ne!(
        runner_policy_sha256(&absent),
        runner_policy_sha256(&empty),
        "and the digest INV-23 compares exactly does not separate them either"
    );
}

/// The three resolution refusals, each with a control that differs in exactly
/// one thing, and each proved to have reached no lock and no effect.
///
/// The grid is **{fault} × {phase = resolve}** with the selection held constant;
/// `the_rebuild_refuses_each_of_its_faults_before_any_spawn` is the same faults
/// at the other phase, and
/// `resolution_and_rebuild_ask_different_questions_of_the_runtime` is the cross.
///
/// Each cell asserts the **typed** refusal rather than `is_err()`: a test that
/// only proves an error came back is green when the fixture is misspelt, which
/// is the failure this grid is built against. The `none` cell is the control
/// that says the fixture would otherwise resolve.
#[test]
fn resolution_refuses_each_of_its_faults_before_any_lock_or_effect() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fault {
        None,
        RuntimeUnreachable,
        RuntimeFails,
        ReferenceAbsent,
        ImageUnidentified,
        VolumeAbsent,
    }

    // Written out so a fault that stops being driven is a compile-time hole
    // rather than a silently shorter grid.
    const FAULTS: &[Fault] = &[
        Fault::None,
        Fault::RuntimeUnreachable,
        Fault::RuntimeFails,
        Fault::ReferenceAbsent,
        Fault::ImageUnidentified,
        Fault::VolumeAbsent,
    ];

    let mut refusals = BTreeSet::new();
    for fault in FAULTS {
        let (runtime, trace) = ready_runtime();
        match fault {
            Fault::None => {}
            Fault::RuntimeUnreachable => runtime.set_unreachable(RuntimeOp::Probe),
            Fault::RuntimeFails => runtime.set_failing(RuntimeOp::InspectImageByReference),
            // The image is still in the table; only the tag is gone, so this
            // cell is about the *reference* and nothing else.
            Fault::ReferenceAbsent => runtime.tag(REFERENCE, "sha256:not-a-real-id"),
            Fault::ImageUnidentified => {
                runtime.add_image("", None);
                runtime.tag(REFERENCE, "");
            }
            Fault::VolumeAbsent => runtime.remove_volume(CODEX_VOLUME),
        }

        let outcome = resolve_container(&runtime, &selection());

        // No effect, ever, on any path.
        assert!(
            trace.ops().iter().all(|op| !op.is_effect()),
            "{fault:?}: resolution reached an effectful operation: {:?}",
            trace.rendered()
        );

        match (fault, outcome) {
            (Fault::None, Ok(policy)) => {
                assert_eq!(policy.image.expect("image").id, IMAGE_ID);
            }
            (Fault::RuntimeUnreachable, Err(refusal)) => {
                assert_eq!(
                    refusal,
                    InspectionRefusal::RuntimeUnavailable {
                        operation: RuntimeOp::Probe,
                        detail: "the fake runtime is armed unreachable for this operation"
                            .to_owned(),
                    }
                );
                assert!(refusal.is_runtime_unavailable());
                // Nothing was asked after the runtime failed to answer.
                assert_eq!(trace.ops(), vec![RuntimeOp::Probe]);
                refusals.insert("runtime unreachable");
            }
            (Fault::RuntimeFails, Err(refusal)) => {
                assert_eq!(
                    refusal.to_string(),
                    "the container runtime refused `inspect-image-by-reference` (the fake \
                     runtime is armed failing for this operation)"
                );
                assert!(
                    refusal.is_runtime_unavailable(),
                    "a runtime that answered with a failure is still a runtime that could not \
                     establish the runner"
                );
                refusals.insert("runtime failed");
            }
            (Fault::ReferenceAbsent, Err(refusal)) => {
                assert_eq!(
                    refusal,
                    InspectionRefusal::ImageReferenceAbsent {
                        reference: REFERENCE.to_owned(),
                    }
                );
                assert!(
                    refusal.to_string().contains("nothing is pulled implicitly"),
                    "the refusal does not say why it is not a fetch: {refusal}"
                );
                // The volumes were never asked about: the refusal is the end of
                // the command, not a step in it.
                assert!(!trace.ops().contains(&RuntimeOp::InspectVolume));
                refusals.insert("reference absent");
            }
            (Fault::ImageUnidentified, Err(refusal)) => {
                assert_eq!(
                    refusal,
                    InspectionRefusal::ImageNotIdentified {
                        reference: REFERENCE.to_owned(),
                    }
                );
                refusals.insert("image unidentified");
            }
            (Fault::VolumeAbsent, Err(refusal)) => {
                assert_eq!(
                    refusal,
                    InspectionRefusal::CredentialVolumeAbsent {
                        agent: "codex".to_owned(),
                        volume: CODEX_VOLUME.to_owned(),
                    },
                    "the refusal must name the agent, not just the volume"
                );
                // The *other* volume was asked about and answered yes, so this
                // cell is about one absent volume and not about volumes at all.
                assert_eq!(
                    trace
                        .rendered()
                        .iter()
                        .filter(|entry| entry.starts_with("rt:inspect-volume:"))
                        .count(),
                    2
                );
                refusals.insert("volume absent");
            }
            (fault, outcome) => panic!("{fault:?} produced {outcome:?}"),
        }
    }
    assert_eq!(
        refusals.len(),
        5,
        "five distinct refusals were driven: {refusals:?}"
    );
}

/// "Before any lock or effect", as one ordered sequence.
///
/// The effect half is asserted above. This is the **lock** half, and it needs a
/// caller: `resolve_container` cannot take a lock — it is handed a runtime and
/// values and has no path, no run directory and no runner — but that is an
/// argument from a signature, and INV-23's clause is about an order.
///
/// So the documented pre-lock sequence is driven against one log that both the
/// runtime and the driver write into: resolution's operations and the caller's
/// worktree lock, public directory, marker and first probe, interleaved in the
/// order they actually happened. A refusal must leave the last four absent.
///
/// Second field held constant: the driver is identical in both cells: only the
/// runtime's readiness varies.
#[test]
fn the_pre_lock_sequence_reaches_no_lock_no_marker_and_no_probe_when_resolution_refuses() {
    for (label, ready) in [("ready", true), ("no image", false)] {
        let log = SharedLog::default();
        let (fake, _trace) = ready_runtime();
        if !ready {
            fake.tag(REFERENCE, "sha256:not-a-real-id");
        }
        let runtime = LoggingRuntime {
            inner: fake,
            log: log.clone(),
        };

        // INV-23's pre-lock order, as a caller performs it: "resolved once by
        // read-only inspection before the worktree lock (before the public
        // directory, the marker, and any probe)".
        let resolved = resolve_container(&runtime, &selection());
        if resolved.is_ok() {
            log.push("lock:worktree");
            log.push("effect:public-dir");
            log.push("effect:marker");
            log.push("spawn:probe");
        }

        let entries = log.entries();
        let inspections = entries
            .iter()
            .filter(|entry| entry.starts_with("rt:"))
            .count();
        assert_eq!(
            inspections,
            if ready { 4 } else { 2 },
            "{label}: {entries:?}"
        );

        let after: Vec<&String> = entries
            .iter()
            .filter(|entry| !entry.starts_with("rt:"))
            .collect();
        if ready {
            assert_eq!(
                after,
                vec![
                    "lock:worktree",
                    "effect:public-dir",
                    "effect:marker",
                    "spawn:probe"
                ],
                "{label}: {entries:?}"
            );
            // And every one of them is after every inspection.
            let first_after = entries
                .iter()
                .position(|entry| !entry.starts_with("rt:"))
                .expect("the driver ran");
            assert_eq!(
                first_after, inspections,
                "{label}: an inspection happened after the lock: {entries:?}"
            );
        } else {
            assert!(
                after.is_empty(),
                "{label}: a refused resolution still reached {after:?}"
            );
        }
    }
}

/// Resolution and rebuild ask **different** questions, and the four cells of
/// {reference present} × {recorded id present} prove it.
///
/// `expected_failures_refusals[1]`'s two sets of three are not the same three:
/// resolution looks up the **reference**, the rebuild looks up the **recorded
/// id**. A seam with one image question would make the two indistinguishable,
/// and a suite that never crossed them would not notice.
///
/// Second field held constant: the credential volumes, present in every cell,
/// so no cell can pass or fail for the volume reason.
#[test]
fn resolution_and_rebuild_ask_different_questions_of_the_runtime() {
    for reference_present in [true, false] {
        for id_present in [true, false] {
            let trace = ContainerTrace::recording();
            let runtime = FakeRuntime::new(trace);
            runtime.add_volume(CLAUDE_VOLUME);
            runtime.add_volume(CODEX_VOLUME);
            if id_present {
                runtime.add_image(IMAGE_ID, Some(MANIFEST));
            }
            if reference_present {
                // A reference that resolves — to *another* image when the
                // recorded id is gone, which is the only way to have one
                // without the other.
                let target = if id_present { IMAGE_ID } else { OTHER_ID };
                runtime.add_image(target, Some(MANIFEST));
                runtime.tag(REFERENCE, target);
            }

            let cell = format!("reference={reference_present} id={id_present}");
            let resolved = resolve_container(&runtime, &selection());
            let mut warnings = Vec::new();
            let rebuilt = rebuild_by_inspection(&runtime, &recorded(), &selection(), &mut warnings);

            assert_eq!(
                resolved.is_ok(),
                reference_present,
                "{cell}: resolution's answer does not track the reference"
            );
            assert_eq!(
                rebuilt.is_ok(),
                id_present,
                "{cell}: the rebuild's answer does not track the recorded id"
            );
            if !reference_present {
                assert_eq!(
                    resolved.expect_err("an absent image reference refuses inspection"),
                    InspectionRefusal::ImageReferenceAbsent {
                        reference: REFERENCE.to_owned()
                    }
                );
            }
            if !id_present {
                assert_eq!(
                    rebuilt.expect_err("an absent image id refuses inspection"),
                    InspectionRefusal::ImageIdAbsent {
                        id: IMAGE_ID.to_owned()
                    }
                );
            }
        }
    }
}

/// Two runtimes holding the same reference at different ids resolve to two
/// execution identities.
///
/// The digest is what the marker, the owner record and every container intent
/// carry, so "the runtime's immutable image id" being in the record is only
/// worth something if moving it moves the digest. Pinned against
/// `canonical_bytes` written by hand in `crate::runner::policy`, not round-
/// tripped here.
///
/// Second field held constant: the reference and the volume set, identical in
/// both cells.
#[test]
fn the_resolved_records_digest_moves_with_the_id_the_runtime_reported() {
    let mut digests = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for id in [IMAGE_ID, OTHER_ID] {
        let trace = ContainerTrace::recording();
        let runtime = FakeRuntime::new(trace);
        runtime.add_image(id, Some(MANIFEST));
        runtime.tag(REFERENCE, id);
        runtime.add_volume(CLAUDE_VOLUME);
        runtime.add_volume(CODEX_VOLUME);
        let policy = resolve_container(&runtime, &selection()).expect("resolves");
        ids.insert(policy.image.as_ref().expect("image").id.clone());
        digests.insert(runner_policy_sha256(&policy));
    }
    assert_eq!(ids.len(), 2, "the fixture did not vary the id at all");
    assert_eq!(
        digests.len(),
        2,
        "two runners executing different images carry one runner_policy_sha256"
    );
}

/// R20 is operator-owned, and the seam makes that structural rather than
/// merely observed.
///
/// The row says `persistent_output` in all five `at_run_end` outcomes and
/// "never created or pruned by a run" — and a run that tidied a volume it
/// mounted would destroy operator credentials, which CLIs rotate on use, so a
/// discarded rotation forces re-login. `ContainerRuntime` has exactly **one**
/// volume operation, it is read-only, and there is therefore no create or prune
/// for this module to reach: the enumeration is derived from `RuntimeOp::ALL`
/// rather than from a list somebody remembered to write.
///
/// The runtime half of the same claim is lane C's
/// `r20_is_persistent_output_in_every_at_run_end_outcome_and_no_census_path_touches_it`;
/// this is the resolution half.
#[test]
fn the_only_volume_operation_the_seam_has_is_a_read_only_presence_question() {
    let volume_ops: Vec<RuntimeOp> = RuntimeOp::ALL
        .iter()
        .copied()
        .filter(|op| op.name().contains("volume"))
        .collect();
    assert_eq!(
        volume_ops,
        vec![RuntimeOp::InspectVolume],
        "the seam grew a volume operation, so a run can now create or prune R20"
    );
    assert!(!RuntimeOp::InspectVolume.is_effect());

    // And this module reaches nothing else: resolution and rebuild both.
    let (runtime, trace) = ready_runtime();
    resolve_container(&runtime, &selection()).expect("resolves");
    let mut warnings = Vec::new();
    rebuild_by_inspection(&runtime, &recorded(), &selection(), &mut warnings).expect("rebuilds");
    assert_eq!(
        trace
            .ops()
            .iter()
            .filter(|op| op.name().contains("volume"))
            .count(),
        4,
        "two volumes, twice"
    );
    assert!(trace.ops().iter().all(|op| !op.is_effect()));
    // The volume is still there afterwards, which is the thing the row is
    // actually about.
    assert!(runtime.volume_present(CLAUDE_VOLUME).expect("inspects"));
    assert!(runtime.volume_present(CODEX_VOLUME).expect("inspects"));
}

/// A `[runner] mounts` entry changes the boundary and **does not** move the
/// recorded execution identity.
///
/// Stated as an assertion rather than left implicit, because it is a real gap
/// and a reviewer should find it named rather than derive it. INV-23's
/// `RunnerPolicy` has four fields — kind, policy, image, credential volumes —
/// and none of them is a mount list, so two runs whose `[runner]` sections
/// differ only in `mounts` record the same runner and carry the same
/// `runner_policy_sha256`. Filed as `PR6B-MOUNTS-ARE-NOT-EXECUTION-IDENTITY`.
///
/// Second field held constant: everything but `mounts`, so the equality below
/// is about that field alone.
#[test]
fn a_configured_mount_does_not_reach_the_recorded_execution_identity() {
    let bare = selection();
    let mounted = RunnerSelection {
        mounts: vec![RunnerMount {
            source: PathBuf::from("/opt/toolchain"),
            target: "/opt/toolchain".to_owned(),
            read_only: false,
        }],
        ..selection()
    };
    assert_ne!(bare, mounted, "the two selections are the same value");

    let (runtime, _trace) = ready_runtime();
    let without = resolve_container(&runtime, &bare).expect("resolves");
    let with = resolve_container(&runtime, &mounted).expect("resolves");
    assert_eq!(
        without, with,
        "a mount reached the record; if that is intended, INV-23's four fields moved"
    );
    assert_eq!(
        runner_policy_sha256(&without),
        runner_policy_sha256(&with),
        "the digest the marker and every container intent carry does not separate them"
    );
    // And a rebuild therefore cannot warn about a mount that changed: the
    // comparison has no field for it.
    let mut warnings = Vec::new();
    rebuild_by_inspection(&runtime, &recorded(), &mounted, &mut warnings).expect("rebuilds");
    assert!(
        warnings.is_empty(),
        "a mount difference produced a warning the record cannot carry: {warnings:?}"
    );
}

/// A selection this module is not for.
#[test]
fn resolution_refuses_a_selection_that_does_not_ask_for_a_container() {
    let (runtime, trace) = ready_runtime();
    let host = RunnerSelection::host_default();
    assert_eq!(
        resolve_container(&runtime, &host)
            .expect_err("a host selection is not a container selection"),
        InspectionRefusal::NotAContainerSelection {
            kind: RunnerKind::Host
        }
    );
    let mut imageless = selection();
    imageless.image = None;
    assert_eq!(
        resolve_container(&runtime, &imageless).expect_err("a selection without an image refuses"),
        InspectionRefusal::SelectionWithoutImage
    );
    assert!(
        trace.ops().is_empty(),
        "a selection guard asked the runtime something: {:?}",
        trace.rendered()
    );
}

// ---------------------------------------------------------------------------
// 2. The rebuild-from-record path
// ---------------------------------------------------------------------------

/// However today's config differs, the rebuilt runner is the recorded one,
/// field for field.
///
/// "warns naming the difference and **is ignored**" — the second half is the
/// one a plausible implementation drops, by merging the config in "where it does
/// not conflict". `run_resumed(4).runner` must equal `run_started(4).runner`
/// exactly, so anything today's config reaches is a `FoldError` later.
///
/// Second field held constant: the runtime, ready in every cell, so no cell can
/// succeed or fail for an inspection reason.
#[test]
fn the_rebuild_returns_the_recorded_runner_exactly_however_the_config_differs() {
    let mut moved_reference = selection();
    moved_reference.image = Some("someone/else:9".to_owned());
    let mut fewer_volumes = selection();
    fewer_volumes.credential_volumes.remove("codex");
    let mut host_now = RunnerSelection::host_default();
    host_now.from_config = true;

    let configs = [
        ("identical", selection()),
        ("moved reference", moved_reference),
        ("fewer volumes", fewer_volumes),
        ("host now", host_now),
        ("absent section", RunnerSelection::host_default()),
    ];
    for (label, today) in configs {
        let (runtime, _trace) = ready_runtime();
        let mut warnings = Vec::new();
        let rebuilt = rebuild_by_inspection(&runtime, &recorded(), &today, &mut warnings)
            .expect("a ready runtime rebuilds");
        assert_eq!(
            rebuilt,
            recorded(),
            "`{label}`: today's config reached the rebuilt runner"
        );
        assert_eq!(
            runner_policy_sha256(&rebuilt),
            runner_policy_sha256(&recorded()),
            "`{label}`: the rebuilt runner carries a different runner_policy_sha256, so \
             run_resumed(4).runner would be a FoldError"
        );
    }
}

/// A config that differs warns **naming the field that moved**, and the fields
/// it can name are ST-20's three.
///
/// "warns naming the difference" is a real assertion and "config differs" fails
/// it: PR3 built `RunnerPolicy::difference()` to name *which* field moved
/// precisely so a warning could. The grid drives one config edit per field and
/// asserts the named field, and then asserts the **set** of reachable fields —
/// so a comparison that started reporting `image id` (which no operator can
/// edit and no operator can fix) fails here.
///
/// Second field held constant: the record, identical in every cell.
#[test]
fn a_config_that_differs_warns_naming_the_field_that_moved() {
    let mut host_now = RunnerSelection::host_default();
    host_now.from_config = true;
    let mut moved_reference = selection();
    moved_reference.image = Some("someone/else:9".to_owned());
    let mut renamed_volume = selection();
    renamed_volume
        .credential_volumes
        .insert("codex".to_owned(), "another-volume".to_owned());
    let mut extra_volume = selection();
    extra_volume
        .credential_volumes
        .insert("copilot".to_owned(), "creds-copilot".to_owned());
    // **Two fields at once.** Every other cell moves exactly one, and an
    // implementation that answered `None` whenever more than one had moved
    // would pass all of them while a two-field edit warned about nothing
    // (`PR6-CORRECTNESS-015`). `RunnerPolicy::difference` reports the first
    // field in its own order, which for these two is the reference.
    let mut reference_and_volumes = selection();
    reference_and_volumes.image = Some("someone/else:9".to_owned());
    reference_and_volumes.credential_volumes.remove("codex");
    // The control: each half of that edit is independently a difference, so the
    // cell below is genuinely the *intersection* and not one edit with a
    // no-op beside it.
    let mut only_reference = selection();
    only_reference.image = Some("someone/else:9".to_owned());
    let mut only_volumes = selection();
    only_volumes.credential_volumes.remove("codex");
    assert_eq!(
        configured_difference(&recorded(), &only_reference),
        Some(RunnerField::ImageReference)
    );
    assert_eq!(
        configured_difference(&recorded(), &only_volumes),
        Some(RunnerField::CredentialVolumes)
    );

    let cases: Vec<(&str, RunnerSelection, Option<RunnerField>)> = vec![
        ("identical", selection(), None),
        ("kind", host_now, Some(RunnerField::Kind)),
        (
            "image reference",
            moved_reference,
            Some(RunnerField::ImageReference),
        ),
        (
            "renamed volume",
            renamed_volume,
            Some(RunnerField::CredentialVolumes),
        ),
        (
            "extra volume",
            extra_volume,
            Some(RunnerField::CredentialVolumes),
        ),
        (
            "reference and volumes together",
            reference_and_volumes,
            Some(RunnerField::ImageReference),
        ),
    ];

    let mut named = BTreeSet::new();
    for (label, today, expected) in cases {
        assert_eq!(
            configured_difference(&recorded(), &today),
            expected,
            "`{label}`: the wrong field was named"
        );
        let (runtime, _trace) = ready_runtime();
        let mut warnings = Vec::new();
        rebuild_by_inspection(&runtime, &recorded(), &today, &mut warnings).expect("rebuilds");
        match expected {
            None => assert!(
                warnings.is_empty(),
                "`{label}`: an identical config warned: {warnings:?}"
            ),
            Some(field) => {
                let warning = warnings
                    .iter()
                    .find(|warning| warning.contains("differs from the runner this run recorded"))
                    .unwrap_or_else(|| panic!("`{label}`: no difference warning in {warnings:?}"));
                assert!(
                    warning.contains(&field.to_string()),
                    "`{label}`: the warning does not name `{field}`: {warning}"
                );
                assert!(
                    warning.contains("is ignored"),
                    "`{label}`: the warning does not say the config was ignored: {warning}"
                );
                named.insert(field.to_string());
            }
        }
    }

    // ST-20: "a `[runner]` config that differs (kind, image reference, or
    // credential volumes)". Three, and only three, are reachable.
    assert_eq!(
        named,
        [
            RunnerField::Kind,
            RunnerField::ImageReference,
            RunnerField::CredentialVolumes,
        ]
        .into_iter()
        .map(|field| field.to_string())
        .collect::<BTreeSet<_>>(),
        "the set of fields a config difference can name has moved"
    );
    for unreachable in [
        RunnerField::Policy,
        RunnerField::ImagePresence,
        RunnerField::ImageId,
        RunnerField::ImageDigest,
    ] {
        assert!(
            !named.contains(&unreachable.to_string()),
            "`{unreachable}` is not a field `upstroke.toml` can move"
        );
    }
}

/// An absent `[runner]` section is a **selection**, and whether it differs
/// depends on what the run recorded.
///
/// The intersection **{section present or absent} × {recorded kind}**, which is
/// the cell `PR6-CORRECTNESS-015` found missing. This test previously asserted
/// only the first axis — "absent never warns" — against a **container** record,
/// and so pinned the defect: a run that recorded a container runner and whose
/// `[runner]` section was subsequently **deleted** is running under an
/// effective selection of host/default, which is as real an edit as changing
/// `kind` in place, and it warned about nothing.
///
/// The claim the original test was protecting is still here and is still true,
/// and it is the **host-record** row: a repository that never configured a
/// runner is not told its runner kind moved. It holds because
/// `RunnerSelection::host_default()` renders to exactly what a host run
/// records, not because a flag suppresses the comparison — which is the
/// difference between a guarantee and a silence.
///
/// Second field held constant: the runtime, ready in every cell, so no cell can
/// warn or not warn for an inspection reason.
#[test]
fn an_absent_runner_section_warns_only_when_the_record_is_not_the_default() {
    let mut present_host = RunnerSelection::host_default();
    present_host.from_config = true;
    let absent = RunnerSelection::host_default();
    assert_eq!(
        RunnerSelection {
            from_config: absent.from_config,
            ..present_host.clone()
        },
        absent,
        "the two selections differ by more than `from_config`"
    );

    let host_record = RunnerPolicy {
        kind: RunnerKind::Host,
        policy: RunnerContract::HostV1,
        image: None,
        credential_volumes: None,
    };

    // {section} x {record}: four cells, and only the three that are a real
    // difference warn.
    let cells: Vec<(&str, RunnerPolicy, RunnerSelection, Option<RunnerField>)> = vec![
        (
            "absent section, host record",
            host_record.clone(),
            absent.clone(),
            None,
        ),
        (
            "present host section, host record",
            host_record.clone(),
            present_host.clone(),
            None,
        ),
        (
            "absent section, container record",
            recorded(),
            absent,
            Some(RunnerField::Kind),
        ),
        (
            "present host section, container record",
            recorded(),
            present_host,
            Some(RunnerField::Kind),
        ),
        (
            "present container section, container record",
            recorded(),
            selection(),
            None,
        ),
    ];

    for (label, record, today, expected) in cells {
        assert_eq!(
            configured_difference(&record, &today),
            expected,
            "`{label}`: the wrong field was named"
        );
        let (runtime, _trace) = ready_runtime();
        let mut warnings = Vec::new();
        if record.kind == RunnerKind::Container {
            rebuild_by_inspection(&runtime, &record, &today, &mut warnings).expect("rebuilds");
        }
        let differed = warnings
            .iter()
            .any(|warning| warning.contains("differs from the runner this run recorded"));
        assert_eq!(
            differed,
            expected.is_some() && record.kind == RunnerKind::Container,
            "`{label}`: {warnings:?}"
        );
    }
}

/// A reference that now names another image warns and the recorded id is used;
/// a reference that no longer resolves at all warns too, and neither refuses.
///
/// `expected_failures_refusals[1]` names the **id** and not the reference, and
/// INV-23 says "so a moved reference cannot change what executes". The grid is
/// {reference resolves to the recorded id, to another id, to nothing} with the
/// recorded id present in all three — that is the second field, held constant,
/// and it is what makes every cell a rebuild that succeeds.
#[test]
fn a_moved_or_vanished_reference_warns_and_the_rebuild_keeps_the_recorded_id() {
    for (label, retag, expect) in [
        ("unchanged", Some(IMAGE_ID), None),
        ("moved", Some(OTHER_ID), Some("now names image")),
        ("vanished", None, Some("no longer resolves")),
    ] {
        let (runtime, trace) = ready_runtime();
        runtime.add_image(OTHER_ID, Some(MANIFEST));
        match retag {
            Some(target) => runtime.move_tag(REFERENCE, target),
            // Point the tag at an id the table does not hold, which is how a
            // reference stops resolving while the recorded id stays present.
            None => runtime.move_tag(REFERENCE, "sha256:nothing-here"),
        }

        let mut warnings = Vec::new();
        let rebuilt = rebuild_by_inspection(&runtime, &recorded(), &selection(), &mut warnings)
            .expect("the recorded id is still present, so the rebuild succeeds");
        assert_eq!(
            rebuilt.image.as_ref().expect("image").id,
            IMAGE_ID,
            "`{label}`: the rebuild took the runtime's current answer instead of the record"
        );
        match expect {
            None => assert!(warnings.is_empty(), "`{label}`: {warnings:?}"),
            Some(needle) => {
                assert_eq!(warnings.len(), 1, "`{label}`: {warnings:?}");
                assert!(
                    warnings[0].contains(needle) && warnings[0].contains(IMAGE_ID),
                    "`{label}`: the warning does not name the recorded id: {}",
                    warnings[0]
                );
            }
        }
        assert!(
            trace.ops().iter().all(|op| !op.is_effect()),
            "`{label}`: the rebuild reached an effectful operation"
        );
    }
}

/// The three rebuild refusals, each **before any spawn**, each with a control.
///
/// Two independent witnesses of the same ordering predicate, because one of them
/// could be a lie: [`RebuildRefusal::before_any_spawn`] is what the code says,
/// and the preflight's own call count is what actually happened. A refusal that
/// classified itself correctly while having already spawned fails the second.
///
/// The grid is **{fault} × {phase = rebuild}**, the record held constant across
/// every cell.
///
/// **Today's config differs in every cell, deliberately.** A refused rebuild
/// must emit no warnings — the refusals come first and a warning about a config
/// difference describes a run that is about to continue — and with an identical
/// config there is nothing for the warning block to say, so the assertion holds
/// vacuously and a mutation that hoisted the warnings above the refusals
/// survives. Measured: it did (M15). The control at the end of the test proves
/// the same `today` *does* warn when the rebuild succeeds, so the emptiness
/// above is about the ordering rather than about the fixture.
#[test]
fn the_rebuild_refuses_each_of_its_faults_before_any_spawn() {
    // Differs from the record in its image reference, so `configured_difference`
    // has something to name in every cell.
    let today = RunnerSelection {
        image: Some("someone/else:9".to_owned()),
        ..selection()
    };
    assert_eq!(
        configured_difference(&recorded(), &today),
        Some(RunnerField::ImageReference),
        "the fixture config does not differ, so `warnings.is_empty()` below is vacuous"
    );
    #[derive(Debug, Clone, Copy)]
    enum Fault {
        None,
        RuntimeUnavailable,
        RecordedIdAbsent,
        VolumeAbsent,
    }
    const FAULTS: &[Fault] = &[
        Fault::None,
        Fault::RuntimeUnavailable,
        Fault::RecordedIdAbsent,
        Fault::VolumeAbsent,
    ];

    let mut refusals = BTreeSet::new();
    for fault in FAULTS {
        let (runtime, trace) = ready_runtime();
        match fault {
            Fault::None => {}
            Fault::RuntimeUnavailable => runtime.set_all_unreachable(),
            // The reference still resolves; only the recorded id is gone, so
            // this cell is about the id.
            Fault::RecordedIdAbsent => {
                runtime.add_image(OTHER_ID, Some(MANIFEST));
                runtime.move_tag(REFERENCE, OTHER_ID);
                let trace2 = ContainerTrace::recording();
                let replacement = FakeRuntime::new(trace2);
                replacement.add_image(OTHER_ID, Some(MANIFEST));
                replacement.tag(REFERENCE, OTHER_ID);
                replacement.add_volume(CLAUDE_VOLUME);
                replacement.add_volume(CODEX_VOLUME);
                // Drive the replacement rather than the ready fixture: it holds
                // the reference and not the recorded id.
                let preflight = RecordingPreflight::accepting(ContainerTrace::off());
                let mut warnings = Vec::new();
                let outcome = rebuild_from_record(
                    &replacement,
                    &recorded(),
                    &today,
                    &preflight,
                    &mut warnings,
                );
                let refusal = outcome.expect_err("the recorded id is absent");
                assert!(refusal.before_any_spawn());
                assert_eq!(preflight.spawns(), 0);
                assert!(
                    warnings.is_empty(),
                    "a refused rebuild warned: {warnings:?}"
                );
                assert!(matches!(
                    refusal,
                    RebuildRefusal::Inspection(InspectionRefusal::ImageIdAbsent { .. })
                ));
                refusals.insert("recorded id absent");
                continue;
            }
            Fault::VolumeAbsent => runtime.remove_volume(CLAUDE_VOLUME),
        }

        let preflight = RecordingPreflight::accepting(trace.clone());
        let mut warnings = Vec::new();
        let outcome = rebuild_from_record(&runtime, &recorded(), &today, &preflight, &mut warnings);

        match (fault, outcome) {
            (Fault::None, Ok(rebuilt)) => {
                assert_eq!(rebuilt, recorded());
                assert_eq!(preflight.spawns(), 1, "the control never spawned");
                // The control for every `warnings.is_empty()` below: this same
                // `today` warns when the rebuild gets that far.
                assert!(
                    warnings.iter().any(
                        |warning| warning.contains("differs from the runner this run recorded")
                    ),
                    "the differing config did not warn on the successful cell, so the empty \
                     warning lists on the refused cells prove nothing: {warnings:?}"
                );
            }
            (Fault::None, Err(error)) => panic!("the control refused: {error}"),
            (fault, Ok(_)) => panic!("{fault:?} did not refuse"),
            (fault, Err(refusal)) => {
                assert!(
                    refusal.before_any_spawn(),
                    "{fault:?} classified itself as a probe-observed refusal"
                );
                assert_eq!(
                    preflight.spawns(),
                    0,
                    "{fault:?} spawned before refusing: {:?}",
                    trace.rendered()
                );
                assert!(
                    warnings.is_empty(),
                    "{fault:?}: a refused rebuild warned: {warnings:?}"
                );
                match (fault, &refusal) {
                    (
                        Fault::RuntimeUnavailable,
                        RebuildRefusal::Inspection(InspectionRefusal::RuntimeUnavailable {
                            operation,
                            ..
                        }),
                    ) => {
                        assert_eq!(*operation, RuntimeOp::Probe);
                        refusals.insert("runtime unavailable");
                    }
                    (
                        Fault::VolumeAbsent,
                        RebuildRefusal::Inspection(InspectionRefusal::CredentialVolumeAbsent {
                            agent,
                            volume,
                        }),
                    ) => {
                        assert_eq!(agent, "claude-code");
                        assert_eq!(volume, CLAUDE_VOLUME);
                        refusals.insert("volume absent");
                    }
                    (fault, refusal) => panic!("{fault:?} produced {refusal}"),
                }
            }
        }
    }
    assert_eq!(refusals.len(), 3, "three distinct refusals: {refusals:?}");
}

/// The fourth behaviour: a shell or CLI that fails inside the recorded image is
/// observed **only** by a spawn, and refuses on the other side of the split.
///
/// The two arms of [`RebuildRefusal`] are the contract's own refusal split, and
/// this is the arm that is not `before_any_spawn`. The preflight's snapshot of
/// the trace at call time is the ordering evidence: every inspection had already
/// happened when the first spawn was about to.
///
/// Second field held constant: the runtime, which is ready in both cells — so
/// what varies is only what the *process* did, exactly as
/// `non_goals[2]` ("non-spawn shell/CLI presence inspection") requires.
#[test]
fn a_failing_preflight_probe_refuses_after_every_inspection_and_only_a_spawn_observes_it() {
    for (label, message) in [
        (
            "shell",
            "pre-flight: the recorded shell `sh` exited 127 inside the recorded image",
        ),
        (
            "agent CLI",
            "pre-flight: `claude` is not on PATH inside the recorded image",
        ),
    ] {
        let (runtime, trace) = ready_runtime();
        let preflight = RecordingPreflight::refusing(trace.clone(), message);
        let mut warnings = Vec::new();
        let refusal = rebuild_from_record(
            &runtime,
            &recorded(),
            &selection(),
            &preflight,
            &mut warnings,
        )
        .expect_err("a probe that fails inside the image refuses");

        assert!(
            !refusal.before_any_spawn(),
            "`{label}`: a probe-observed refusal claimed it happened before any spawn"
        );
        assert!(matches!(refusal, RebuildRefusal::Preflight(_)));
        assert!(refusal.to_string().contains(message), "{refusal}");

        // The observation is a spawn, and it came after every inspection.
        let calls = preflight.calls();
        assert_eq!(calls.len(), 1, "`{label}`: the probe did not run");
        let (certified, prefix) = &calls[0];
        assert_eq!(
            certified,
            &recorded(),
            "`{label}`: the probe certified something other than the rebuilt record"
        );
        let ops: Vec<&String> = prefix
            .iter()
            .filter(|entry| entry.starts_with("rt:"))
            .collect();
        assert_eq!(
            ops.len(),
            5,
            "`{label}`: the probe ran before the inspections finished: {prefix:?}"
        );
        assert!(
            ops[0].starts_with("rt:probe:")
                && ops[1].starts_with("rt:inspect-image-by-id:")
                && ops[2].starts_with("rt:inspect-volume:")
                && ops[3].starts_with("rt:inspect-volume:")
                && ops[4].starts_with("rt:inspect-image-by-reference:"),
            "`{label}`: the inspection prefix is not the rebuild's: {ops:?}"
        );
        // Nothing was created, started, stopped or removed on the way.
        assert!(trace.ops().iter().all(|op| !op.is_effect()));
    }
}

/// A record the fold would refuse never reaches an inspection.
#[test]
fn the_rebuild_refuses_an_incomplete_record_before_asking_the_runtime_anything() {
    let mut without_volumes = recorded();
    without_volumes.credential_volumes = None;
    let (runtime, trace) = ready_runtime();
    let mut warnings = Vec::new();
    assert_eq!(
        rebuild_by_inspection(&runtime, &without_volumes, &selection(), &mut warnings)
            .expect_err("a container record without credential volumes is incomplete"),
        InspectionRefusal::RecordIncomplete(RunnerRecordDefect::ContainerWithoutCredentialVolumes)
    );
    let host = crate::runner::policy::host_policy();
    assert_eq!(
        rebuild_by_inspection(&runtime, &host, &selection(), &mut warnings)
            .expect_err("a host policy is not a container selection"),
        InspectionRefusal::NotAContainerSelection {
            kind: RunnerKind::Host
        }
    );
    assert!(
        trace.ops().is_empty(),
        "a record guard asked the runtime something: {:?}",
        trace.rendered()
    );
}

// ---------------------------------------------------------------------------
// 3. `[runner]` config and the schema-1..3 refusal
// ---------------------------------------------------------------------------

/// **T-CONTAINER (13).** `[runner] kind = "container"` under a schema-1..3
/// fresh run **or** resume is a config error before any effect.
///
/// Both write commands, because `expected_failures_refusals[0]` names both and a
/// suite that covers one covers half. The grid is
/// **{command = run, resume} × {`[runner]` = host, container}**, in two phases
/// with two different witnesses, because "before any effect" is an ordering and
/// an ordering needs something on the far side of it to be visible.
///
/// **Phase A — a competing worktree lease is held for the whole grid.**
/// `WorktreeLock::acquire_in` is the first effect either command performs
/// (`coordinator.rs`: "every read-only refusal precedes every lock";
/// `resume.rs` marks the line after `validate_inputs` "the first effect of the
/// command"). So with the lease held by this test:
///
/// * `kind = "host"` fails with the **lease** refusal — it reached the lock;
/// * `kind = "container"` fails with the **config** error — it did not.
///
/// One fixture, one held lease, two configs, two *different* failures. A
/// refusal moved after the lock fails this by turning the container cell into a
/// lease refusal, and a test that only asserted "an error came back" would not
/// notice.
///
/// **Phase B — the lease is released, and the tree is inspected.** No run
/// directory under either half of the §15 split, no `run.lock`, no branch, no
/// container intent namespace, and — for the fresh run — no adapter ever
/// resolved, so no pre-flight probe could have been spawned. The `host` control
/// of phase B is what proves the run command reaches pre-flight at all.
///
/// The whole-tree half of ST-16 (i)'s second clause is
/// [`no_module_outside_the_container_runner_writes_a_container_intent`].
#[test]
fn legacy_container_selection_refused_before_effects() {
    const CONTAINER_TOML: &str = "container";
    const HOST_TOML: &str = "host";

    for kind in [HOST_TOML, CONTAINER_TOML] {
        let repo = temp_repo(&format!("legacy-{kind}"));
        let private = repo.join("private");
        fs::create_dir_all(&private).expect("private root");
        let config = if kind == HOST_TOML {
            "[runner]\nkind = \"host\"\n".to_owned()
        } else {
            format!(
                "[runner]\nkind = \"container\"\nimage = \"{REFERENCE}\"\n\
                 credential_volumes = {{ claude-code = \"{CLAUDE_VOLUME}\" }}\n"
            )
        };
        fs::write(repo.join("upstroke.toml"), &config).expect("config");
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "config"]);
        // One seeded run per legacy schema: `EngineLimits::for_resume` reads the
        // header's schema, so a suite driving only one of the three has driven
        // only one reading.
        let seeded: Vec<(u32, String)> = [1_u32, 2, 3]
            .into_iter()
            .map(|schema| {
                let run_id = format!("01ARZ3NDEKTSV4RRFFQ69G5FA{schema}");
                seed_legacy_run(&repo, &run_id, &private, schema);
                (schema, run_id)
            })
            .collect();

        let run_opts = || {
            let mut opts = RunOptions::new(repo.join("plan.md"), repo.clone());
            opts.pools_path = Some(empty_pools(&repo));
            opts.private_root = Some(private.clone());
            opts.wait_on_block = Some(Duration::ZERO);
            opts.defer_backoff = Duration::ZERO;
            opts
        };
        let resume_opts = |run_id: &str| {
            let mut opts = ResumeOptions::new(run_id.to_owned(), repo.clone());
            opts.pools_path = Some(empty_pools(&repo));
            opts.private_root = Some(private.clone());
            opts.wait_on_block = Some(Duration::ZERO);
            opts.defer_backoff = Duration::ZERO;
            opts
        };

        // -- phase A: the lease is the far side of the ordering ---------------
        {
            let _lease = crate::rundir::WorktreeLock::acquire(&repo).expect("the test's lease");
            let adapters = RecordingAdapters::default();
            let mut outcomes = vec![(
                "run".to_owned(),
                crate::engine::run_with(&run_opts(), &adapters),
            )];
            for (schema, run_id) in &seeded {
                outcomes.push((
                    format!("resume of a schema-{schema} run"),
                    crate::engine::resume_with(&resume_opts(run_id), &adapters),
                ));
            }
            for (command, outcome) in outcomes {
                let error = outcome.expect_err("the lease is held, so nothing can proceed");
                if kind == HOST_TOML {
                    assert!(
                        matches!(&error, UpstrokeError::Refused { message }
                            if message.contains("already driving worktree")),
                        "{command}: the control did not reach the worktree lock, so the \
                         container cell's failure to reach it proves nothing: {error}"
                    );
                } else {
                    let UpstrokeError::Config { message, .. } = &error else {
                        panic!(
                            "{command}: refused as {error:?} — a container selection reached the \
                             worktree lock before it was refused"
                        );
                    };
                    assert!(
                        message.contains("[runner] `kind = \"container\"` is refused"),
                        "{command}: {message}"
                    );
                    assert!(
                        message.contains("no owner run") && message.contains("run.lock"),
                        "{command}: the refusal does not say why a late refusal would be \
                         broken: {message}"
                    );
                }
            }
        }

        // -- phase B: nothing was created ------------------------------------
        let adapters = RecordingAdapters::default();
        let run = crate::engine::run_with(&run_opts(), &adapters);
        assert!(run.is_err(), "the fixture has no agents");
        for (_, run_id) in &seeded {
            assert!(
                crate::engine::resume_with(&resume_opts(run_id), &adapters).is_err(),
                "the fixture has no agents"
            );
        }

        if kind == HOST_TOML {
            assert!(
                !adapters.asked().is_empty(),
                "the control never reached pre-flight, so the container cell's empty adapter \
                 log proves nothing"
            );
            continue;
        }

        assert!(
            adapters.asked().is_empty(),
            "an adapter was resolved before the refusal ({:?}), so a pre-flight probe could \
             have been spawned",
            adapters.asked()
        );
        let runs = repo.join(".upstroke").join("runs");
        let mut ids: Vec<String> = fs::read_dir(&runs)
            .expect("runs root")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
            .collect();
        ids.sort();
        let mut seeded_ids: Vec<String> = seeded.iter().map(|(_, run_id)| run_id.clone()).collect();
        seeded_ids.sort();
        assert_eq!(
            ids, seeded_ids,
            "the refused fresh run created a run directory"
        );
        for (schema, run_id) in &seeded {
            assert!(
                !runs.join(run_id).join("run.lock").exists(),
                "the refused schema-{schema} resume left a run.lock behind"
            );
        }
        assert!(
            !private.join("runs").exists(),
            "the refused commands created a private run directory"
        );
        assert!(
            !private.join("containers").exists(),
            "a legacy command wrote something into the container intent namespace"
        );
        let branches = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["branch", "--list", "upstroke/*"])
                .output()
                .expect("git branch")
                .stdout,
        )
        .expect("utf-8");
        assert!(
            branches.trim().is_empty(),
            "the refused run created a branch: {branches}"
        );
    }
}

/// Every reading of `[engine]`'s limits refuses a container selection.
///
/// `EngineLimits` is what distinguishes "a run being created now" from "a
/// sequential run's resume", and `expected_failures_refusals[0]` names both. The
/// grid is written out with an exhaustive `match` beside it, so a third variant
/// is a compile error here rather than a reading that quietly escapes.
#[test]
fn every_engine_limits_reading_refuses_a_container_selection() {
    let all = [EngineLimits::Fresh, EngineLimits::SequentialResume];
    for limits in all {
        match limits {
            // Exhaustive on purpose: a new variant must be classified here.
            EngineLimits::Fresh | EngineLimits::SequentialResume => {}
        }
    }

    let dir = scratch("engine-limits");
    let mut refused = 0;
    for limits in all {
        fs::write(
            dir.join("upstroke.toml"),
            format!("[runner]\nkind = \"container\"\nimage = \"{REFERENCE}\"\n"),
        )
        .expect("config");
        let mut warnings = Vec::new();
        let error = crate::config::load_limits(
            Some(&dir.join("upstroke.toml")),
            &dir,
            Some(&empty_pools(&dir)),
            limits,
            &mut warnings,
        )
        .expect_err("a container selection is refused");
        assert!(
            matches!(&error, UpstrokeError::Config { message, .. }
                if message.contains("[runner] `kind = \"container\"` is refused")),
            "{limits:?}: {error}"
        );
        refused += 1;

        // The control, byte-identical apart from the kind: the same reading
        // accepts a host selection, so the refusal is about the value.
        fs::write(dir.join("upstroke.toml"), "[runner]\nkind = \"host\"\n").expect("config");
        let config = crate::config::load_limits(
            Some(&dir.join("upstroke.toml")),
            &dir,
            Some(&empty_pools(&dir)),
            limits,
            &mut warnings,
        )
        .expect("a host selection loads");
        assert_eq!(config.runner.kind, RunnerKind::Host);
        assert!(config.runner.from_config);
    }
    assert_eq!(refused, 2, "both readings were driven");
}

/// ST-16 (i)'s second clause: **no legacy process ever writes a container
/// intent** — a claim about the whole tree, not about the parser.
///
/// A census rather than a behavioural test, because the clause is a universal:
/// it is not satisfied by showing that one legacy path does not write one. The
/// set of files whose production region names the intent record or the funnel
/// that writes it is written out here; a legacy module that acquired one fails
/// this by name.
///
/// The control at the bottom is what stops this from becoming
/// `PR6F-DOCKER-CENSUS-CANNOT-FAIL`: the census must still be finding the files
/// it is supposed to find, or "no offenders" means "the needle is unfindable".
#[test]
fn no_module_outside_the_container_runner_writes_a_container_intent() {
    /// Everything that could put a record in `<R>/containers`.
    ///
    /// Container-specific by construction: `write_intent` alone would also match
    /// `crate::workspace_manager`'s **worktree** intent (DESIGN.md:234, a
    /// different R-row and a different namespace), and a census that reported
    /// that would be a census nobody could keep green. Measured — it did.
    const WRITERS: &[&str] = &[
        "ContainerIntent",
        "ContainerName",
        "containers_dir",
        "CONTAINERS_DIR",
        "container::write_intent",
    ];
    /// The files allowed to name one, each with the reason.
    const ALLOWED: &[(&str, &str)] = &[
        ("src/runner/container.rs", "the funnel that writes them"),
        ("src/runner/container/intent.rs", "the record itself"),
        (
            "src/runner/container/census.rs",
            "the census that reclaims them",
        ),
        (
            "src/runner/container/exec.rs",
            "the ContainerRunner that owns an invocation",
        ),
    ];
    /// The files left out of the scan, each an **exact** repo-relative path.
    ///
    /// Every one of them is asserted to exist below: an exclusion that names no
    /// file excludes nothing today and silently excludes whatever is created at
    /// that path tomorrow.
    const EXCLUDED: &[&str] = &[
        "src/effects/tests.rs",
        "src/engine/topology/create/tests.rs",
        "src/engine/topology/recover/tests.rs",
        "src/runner/container/census/tests.rs",
        "src/runner/container/fake.rs",
        "src/runner/container/resolve/tests.rs",
        "src/runner/container/tests.rs",
    ];

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let allowed: BTreeSet<&str> = ALLOWED.iter().map(|(path, _)| *path).collect();
    for excluded in EXCLUDED {
        assert!(
            root.join(excluded).is_file(),
            "`{excluded}` is excluded from this census and names no file; an exclusion that \
             matches nothing today is one that matches whatever is created at that path \
             tomorrow"
        );
    }
    let mut sorted = EXCLUDED.to_vec();
    sorted.sort_unstable();
    assert_eq!(
        EXCLUDED,
        &sorted[..],
        "`EXCLUDED` is read by eye, so it is written sorted"
    );
    let mut offenders = Vec::new();
    let mut found = BTreeSet::new();
    let mut matched_needles: BTreeSet<&str> = BTreeSet::new();
    let mut scanned = 0;
    for path in rust_sources(&root.join("src")) {
        let relative = path
            .strip_prefix(&root)
            .expect("under the manifest")
            .to_string_lossy()
            .replace('\\', "/");
        // Test modules of the container subtree drive the funnel and name its
        // types; they are excluded by name, so a new one is a change here.
        //
        // **Every exclusion is an exact path.** Three of them were
        // `starts_with` — `src/runner/container/tests`,
        // `.../census/tests`, `.../resolve/tests` — under a comment claiming
        // the opposite, and a prefix widens to every sibling whose name begins
        // the same way. Measured: a `pub fn write_one(intent: &ContainerIntent)`
        // module failed this census as `src/runner/container/rogue.rs` and
        // passed it as `src/runner/container/tests_of_the_funnel.rs`. The list
        // is `EXCLUDED` above, every entry of it is asserted to name a file that
        // exists, and the match is `==`.
        //
        // `src/effects/tests.rs` is the fourth, added by PR6 lane E. It is the
        // `#[cfg(test)] mod tests;` of `src/effects.rs` — a test module, never
        // reachable from production — and it names `ContainerName` for one
        // reason: `the_view_directory_has_one_definition_in_the_tree` calls
        // `exec::view_dir` and `census::view_path` with the same name and
        // asserts they answer the same path (`PR6E-005`, a divergence that
        // survived all 1324 tests). It writes no intent and constructs no
        // container. The exclusion is by exact path rather than by prefix, so
        // it cannot widen to a sibling.
        //
        // `src/engine/topology/recover/tests.rs` is the fifth, added by PR7
        // lane E. It is the `mod tests;` of `src/engine/topology/recover.rs`,
        // declared under a test configuration and never reachable from
        // production, and it names these types for one reason: recovery step
        // (a)'s row is "containers **incl. every earlier incarnation of this
        // run** under `<R>/containers`", so
        // `resume_of_nondefault_root_run_reclaims_earlier_incarnation_intents_in_recorded_root`
        // has to *plant* a dead incarnation's intent for the census to find,
        // and it plants it through this very funnel rather than with `fs`. A
        // fixture that writes an intent for the census to reclaim is the same
        // category as `src/runner/container/census/tests.rs` above. The
        // exclusion is by exact path, so it cannot widen to a sibling.
        // `src/engine/topology/create/tests.rs` is the seventh, added by PR7
        // lane B, on the same terms. It is the `#[cfg(test)] mod tests;` of the
        // schema-4 creator. It names `ContainerIntent`, `ContainerName` and
        // `containers_dir` to **read back** the intent a containerized probe
        // left after a kill —
        // `probe_intent_carries_runner_policy_digest_matching_owner_record` and
        // `kill_during_containerized_probe_...` — and writes none: the one that
        // exists was written by `ContainerRunner` through the funnel. Exact, so
        // it cannot widen to `src/engine/topology/create.rs`, which is
        // production and is scanned.
        if EXCLUDED.contains(&relative.as_str()) {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read source");
        // The WHOLE file, comments and strings blanked — deliberately **not**
        // `effects::production_region`. That helper cuts a source at its first
        // `#[cfg(test)]`, and `src/engine/coordinator.rs` has a `#[cfg(test)]
        // use` on **line 36 of 1599**: 97% of the schema-1..3 coordinator, and
        // 96% of `attempt.rs` and `resume.rs`, are outside it. A prohibition
        // about the legacy engine that could not see the legacy engine would be
        // the vacuous census this project has already paid for twice
        // (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`, `PR6F-DOCKER-CENSUS-CANNOT-FAIL`).
        // Measured: with `production_region` a planted `ContainerIntent` in
        // `run_harness_inner_on` SURVIVED. Filed as
        // `PR6B-PRODUCTION-REGION-CUT-AT-A-CFG-TEST-USE`.
        //
        // Scanning the whole file is strictly stronger for a prohibition, and
        // its only cost is that a *test* naming these types is an offender —
        // which is why the container subtree's test files are excluded above,
        // by name.
        let scanned_text = crate::effects::blank_comments_and_strings(&source);
        scanned += 1;
        for writer in WRITERS {
            if scanned_text.contains(writer) {
                if allowed.contains(relative.as_str()) {
                    found.insert(relative.clone());
                    matched_needles.insert(writer);
                } else {
                    offenders.push(format!("{relative} names `{writer}`"));
                }
            }
        }
        // The domain control, and the reason this census is not the one above:
        // the body of the legacy coordinator must be inside what was scanned.
        if relative == "src/engine/coordinator.rs" {
            assert!(
                scanned_text.contains("fn run_harness_inner_on"),
                "the census does not reach the legacy coordinator's body, so it cannot hold a \
                 claim about what a legacy process writes"
            );
        }
    }
    assert!(scanned > 20, "the walk found the tree: {scanned}");
    assert!(
        offenders.is_empty(),
        "a module outside the container runner can write a container intent, so a legacy \
         process could own a container with no run identity behind it: {offenders:#?}"
    );
    // The needle control, in two halves, because the file half alone does not
    // hold. Without either, a census whose needles stopped matching would be
    // silently green — `PR6F-DOCKER-CENSUS-CANNOT-FAIL`, measured this slice, in
    // this repository, on this clause.
    //
    // (a) Every allowed file was reached.
    assert_eq!(
        found.len(),
        ALLOWED.len(),
        "the census found {found:?} of {ALLOWED:?}, so it is not looking at what it claims to"
    );
    // (b) Every needle matched something. This half was missing, and (a) does
    // not imply it: `ContainerName` alone appears in all four allowed files, so
    // the other four `WRITERS` could each have stopped matching anywhere in the
    // tree and (a) would still have counted four. Measured: rewriting the two
    // `crate::runner::container::write_intent(` call sites as
    // `super::super::write_intent(` — a legal, meaning-preserving refactor —
    // left the needle `container::write_intent` matching nothing in the scanned
    // set, and this census stayed green.
    assert_eq!(
        matched_needles,
        WRITERS.iter().copied().collect::<BTreeSet<_>>(),
        "a `WRITERS` needle matches nothing in the allowed files, so the prohibition it \
         encodes is not being enforced on anything"
    );
}

/// This module reaches no lock, no filesystem and no spawn.
///
/// The structural half of "before any lock or effect": the argument that
/// `resolve_container` cannot take a worktree lock is an argument about what it
/// is given, and this is that argument executed. A call planted in the
/// production region fails it.
#[test]
fn the_resolution_module_names_no_lock_no_write_and_no_spawn() {
    const FORBIDDEN: &[&str] = &[
        "WorktreeLock",
        "RunLock",
        "fs::",
        "File::",
        "Command::",
        ".spawn(",
        "runtime.create(",
        "runtime.start(",
        "runtime.stop(",
        "runtime.remove(",
        "create_dir",
        "write(",
    ];
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runner/container/resolve.rs"),
    )
    .expect("this module");
    let production =
        crate::effects::blank_comments_and_strings(&crate::effects::production_region(&source));
    let named: Vec<&str> = FORBIDDEN
        .iter()
        .copied()
        .filter(|needle| production.contains(needle))
        .collect();
    assert!(
        named.is_empty(),
        "resolution reaches {named:?}, so it is no longer read-only by construction"
    );
    // The control: the census is reading the module and not an empty string.
    assert!(
        production.contains("fn resolve_container"),
        "the production region is empty, so the census above proves nothing"
    );
}

// ---------------------------------------------------------------------------
// Test-only substrate
// ---------------------------------------------------------------------------

/// One ordered log both a runtime and its caller write into.
#[derive(Debug, Clone, Default)]
struct SharedLog(std::sync::Arc<Mutex<Vec<String>>>);

impl SharedLog {
    fn push(&self, entry: &str) {
        self.0.lock().expect("log").push(entry.to_owned());
    }

    fn entries(&self) -> Vec<String> {
        self.0.lock().expect("log").clone()
    }
}

/// A [`ContainerRuntime`] that records every call into a log the caller also
/// writes into, so "before the worktree lock" is one sequence.
struct LoggingRuntime {
    inner: FakeRuntime,
    log: SharedLog,
}

macro_rules! logged {
    ($self:ident, $op:expr) => {{
        $self.log.push(&format!("rt:{}", $op.name()));
    }};
}

impl ContainerRuntime for LoggingRuntime {
    fn probe(&self) -> Result<(), RuntimeError> {
        logged!(self, RuntimeOp::Probe);
        self.inner.probe()
    }

    fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        logged!(self, RuntimeOp::InspectImageByReference);
        self.inner.image_by_reference(reference)
    }

    fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        logged!(self, RuntimeOp::InspectImageById);
        self.inner.image_by_id(id)
    }

    fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
        logged!(self, RuntimeOp::InspectVolume);
        self.inner.volume_present(name)
    }

    fn containers_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<crate::runner::container::runtime::DiscoveredContainer>, RuntimeError> {
        logged!(self, RuntimeOp::ListByLabel);
        self.inner.containers_with_label(key, value)
    }

    fn observe(&self, name: &str) -> Result<Liveness, RuntimeError> {
        logged!(self, RuntimeOp::Observe);
        self.inner.observe(name)
    }

    fn collect(
        &self,
        name: &str,
    ) -> Result<crate::runner::container::runtime::ContainerExecution, RuntimeError> {
        logged!(self, RuntimeOp::Collect);
        self.inner.collect(name)
    }

    fn create(
        &self,
        spec: &crate::runner::container::runtime::CreateSpec,
    ) -> Result<crate::runner::container::runtime::CreatedContainer, RuntimeError> {
        logged!(self, RuntimeOp::Create);
        self.inner.create(spec)
    }

    fn start(&self, name: &str) -> Result<(), RuntimeError> {
        logged!(self, RuntimeOp::Start);
        self.inner.start(name)
    }

    fn stop(
        &self,
        name: &str,
        mode: crate::runner::container::runtime::StopMode,
    ) -> Result<(), RuntimeError> {
        logged!(self, RuntimeOp::Stop);
        self.inner.stop(name, mode)
    }

    fn remove(&self, name: &str) -> Result<(), RuntimeError> {
        logged!(self, RuntimeOp::Remove);
        self.inner.remove(name)
    }
}

/// An [`AdapterSource`] that records every id it was asked for and hands back
/// nothing.
///
/// The recording is the point: an empty log proves the command refused before
/// pre-flight ever tried to resolve an agent, which is what "before any effect"
/// buys and what a refusal returning the right message would not.
#[derive(Default)]
struct RecordingAdapters {
    asked: Mutex<Vec<String>>,
}

impl RecordingAdapters {
    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("adapter log").clone()
    }
}

impl AdapterSource for RecordingAdapters {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        self.asked.lock().expect("adapter log").push(id.to_owned());
        None
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "upstroke-resolve-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A clean repository with a two-task plan, seeded and committed.
fn temp_repo(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "test@upstroke.local"]);
    git(&dir, &["config", "user.name", "upstroke tests"]);
    fs::write(dir.join("README.md"), "seed\n").expect("seed");
    fs::write(
        dir.join("plan.md"),
        "## Implement the widget\n<!-- upstroke: id=t1 depends= -->\nMake it.\n",
    )
    .expect("plan");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "seed"]);
    dir
}

/// An explicit pools file with no pools in it — never the operator's real one.
fn empty_pools(dir: &Path) -> PathBuf {
    let path = dir.join("pools.toml");
    if !path.exists() {
        fs::write(&path, "# no pools\n").expect("pools");
    }
    path
}

/// A legacy run directory with a `run_started` header and nothing else.
///
/// Enough for `resume` to reach `validate_inputs`, which is the statement under
/// test: `resume.rs` marks the line after it "the first effect of the command".
/// `schema` is 1, 2 or 3 — `expected_failures_refusals[0]` says "a schema-1..3
/// fresh run **or** resume", and the reading a resume gets is chosen by
/// `EngineLimits::for_resume(header_schema)`, so all three are driven.
fn seed_legacy_run(repo: &Path, run_id: &str, private: &Path, schema: u32) {
    let public = crate::rundir::public_dir(repo, run_id);
    fs::create_dir_all(&public).expect("public dir");
    let head = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("utf-8");
    let started = crate::events::RunStarted {
        schema,
        upstroke_version: env!("CARGO_PKG_VERSION").to_owned(),
        run_id: run_id.to_owned(),
        branch: format!("upstroke/run-{run_id}"),
        base_sha: head.trim().to_owned(),
        plan_path: "plan.md".to_owned(),
        config_path: Some("upstroke.toml".to_owned()),
        plan_hash: "unused-by-the-refusal".to_owned(),
        // Both required by `ensure_supported_schema` for a schema-3 header,
        // which runs *before* `validate_inputs` — so without them a schema-3
        // resume never reaches the refusal under test. Schemas 1 and 2 accept
        // them and do not require them, so they are set unconditionally.
        normalized_plan_digest: Some(format!("sha256:{}", "0".repeat(64))),
        private_dir: private.join("runs").join(run_id).display().to_string(),
        gates: Vec::new(),
        gates_from_config: false,
        interaction_mode: "off".to_owned(),
        chains: Vec::new(),
        effort_policy: None,
        gate_cmds: None,
        reviews: Some(crate::review::ReviewPlan::default()),
    };
    let event = crate::events::Event::now(crate::events::EventBody::RunStarted {
        data: Box::new(started),
    });
    let line = serde_json::to_string(&event).expect("serialize run_started");
    fs::write(public.join("events.jsonl"), format!("{line}\n")).expect("events.jsonl");
}

/// Every `src/**/*.rs`, sorted.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    entries.sort();
    let mut out = Vec::new();
    for entry in entries {
        if entry.is_dir() {
            out.extend(rust_sources(&entry));
        } else if entry.extension().is_some_and(|ext| ext == "rs") {
            out.push(entry);
        }
    }
    out
}
