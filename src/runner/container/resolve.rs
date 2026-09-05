//! Extended notes: `docs/internals/runner/container/resolve.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]
use std::collections::BTreeMap;

use thiserror::Error;

use super::runtime::{ContainerRuntime, ImageInspection, RuntimeError, RuntimeOp};
use crate::config::RunnerSelection;
use crate::error::UpstrokeError;
use crate::topology::events::{
    ImageIdentity, RunnerContract, RunnerField, RunnerKind, RunnerPolicy, RunnerRecordDefect,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InspectionRefusal {
    #[error(
        "the container runtime cannot be reached for `{operation}` ({detail}); the runner this \
         run needs cannot be established without it"
    )]
    RuntimeUnavailable {
        operation: RuntimeOp,
        detail: String,
    },
    #[error("the container runtime refused `{operation}` ({detail})")]
    RuntimeFailed {
        operation: RuntimeOp,
        detail: String,
    },
    #[error(
        "the container runtime does not hold the image reference `{reference}`; nothing is \
         pulled implicitly, so pull or build it before the run"
    )]
    ImageReferenceAbsent { reference: String },
    #[error(
        "the container runtime no longer holds the recorded image id `{id}`; this run records \
         that id as its execution identity and creates every container from it, so it cannot \
         continue until the runtime holds it again"
    )]
    ImageIdAbsent { id: String },
    #[error("the container runtime resolved `{reference}` to an image it reported no id for")]
    ImageNotIdentified { reference: String },
    #[error(
        "the per-agent credential volume `{volume}` for agent `{agent}` does not exist; \
         credential volumes are operator-owned and a run never creates one"
    )]
    CredentialVolumeAbsent { agent: String, volume: String },
    #[error("[runner] selects the `{kind:?}` runner, so there is no container policy to resolve")]
    NotAContainerSelection { kind: RunnerKind },
    #[error("[runner] selects the container runner without an image reference to resolve")]
    SelectionWithoutImage,
    #[error("the resolved container runner is not a usable RunnerPolicy: {0}")]
    RecordIncomplete(RunnerRecordDefect),
}

impl InspectionRefusal {
    #[must_use]
    pub const fn is_runtime_unavailable(&self) -> bool {
        matches!(
            self,
            Self::RuntimeUnavailable { .. } | Self::RuntimeFailed { .. }
        )
    }

    fn from_runtime(error: RuntimeError) -> Self {
        match error {
            RuntimeError::Unreachable { operation, detail } => {
                Self::RuntimeUnavailable { operation, detail }
            }
            RuntimeError::Failed { operation, detail } => Self::RuntimeFailed { operation, detail },
        }
    }
}

impl From<InspectionRefusal> for UpstrokeError {
    fn from(refusal: InspectionRefusal) -> Self {
        Self::Refused {
            message: refusal.to_string(),
        }
    }
}

#[derive(Debug, Error)]
pub enum RebuildRefusal {
    #[error("the recorded container runner cannot be re-established: {0}")]
    Inspection(#[from] InspectionRefusal),
    #[error("the recorded container runner's RunnerPreflight refused: {0}")]
    Preflight(#[source] UpstrokeError),
}

impl RebuildRefusal {
    #[must_use]
    pub const fn before_any_spawn(&self) -> bool {
        matches!(self, Self::Inspection(_))
    }
}

impl From<RebuildRefusal> for UpstrokeError {
    fn from(refusal: RebuildRefusal) -> Self {
        Self::Refused {
            message: refusal.to_string(),
        }
    }
}

pub trait RunnerPreflight {
    fn certify(&self, policy: &RunnerPolicy) -> Result<(), UpstrokeError>;
}

pub fn resolve_container(
    runtime: &dyn ContainerRuntime,
    selection: &RunnerSelection,
) -> Result<RunnerPolicy, InspectionRefusal> {
    if selection.kind != RunnerKind::Container {
        return Err(InspectionRefusal::NotAContainerSelection {
            kind: selection.kind,
        });
    }
    let Some(reference) = selection.image.as_deref() else {
        return Err(InspectionRefusal::SelectionWithoutImage);
    };
    runtime.probe().map_err(InspectionRefusal::from_runtime)?;
    let inspection = runtime
        .image_by_reference(reference)
        .map_err(InspectionRefusal::from_runtime)?
        .ok_or_else(|| InspectionRefusal::ImageReferenceAbsent {
            reference: reference.to_owned(),
        })?;
    if inspection.id.is_empty() {
        return Err(InspectionRefusal::ImageNotIdentified {
            reference: reference.to_owned(),
        });
    }
    inspect_volumes(runtime, &selection.credential_volumes)?;
    let policy = RunnerPolicy {
        kind: RunnerKind::Container,
        policy: RunnerContract::ContainerV1,
        image: Some(ImageIdentity {
            reference: reference.to_owned(),
            id: inspection.id.clone(),
            digest: reported_digest(&inspection),
        }),
        credential_volumes: Some(selection.credential_volumes.clone()),
    };
    policy
        .completeness()
        .map_err(InspectionRefusal::RecordIncomplete)?;
    Ok(policy)
}

fn reported_digest(inspection: &ImageInspection) -> Option<String> {
    inspection
        .digest
        .as_ref()
        .filter(|digest| !digest.is_empty())
        .cloned()
}

fn inspect_volumes(
    runtime: &dyn ContainerRuntime,
    volumes: &BTreeMap<String, String>,
) -> Result<(), InspectionRefusal> {
    for (agent, volume) in volumes {
        let present = runtime
            .volume_present(volume)
            .map_err(InspectionRefusal::from_runtime)?;
        if !present {
            return Err(InspectionRefusal::CredentialVolumeAbsent {
                agent: agent.clone(),
                volume: volume.clone(),
            });
        }
    }
    Ok(())
}

pub fn rebuild_by_inspection(
    runtime: &dyn ContainerRuntime,
    record: &RunnerPolicy,
    today: &RunnerSelection,
    warnings: &mut Vec<String>,
) -> Result<RunnerPolicy, InspectionRefusal> {
    if record.kind != RunnerKind::Container {
        return Err(InspectionRefusal::NotAContainerSelection { kind: record.kind });
    }
    record
        .completeness()
        .map_err(InspectionRefusal::RecordIncomplete)?;
    let image = record
        .image
        .as_ref()
        .ok_or(InspectionRefusal::RecordIncomplete(
            RunnerRecordDefect::ContainerWithoutImage,
        ))?;

    runtime.probe().map_err(InspectionRefusal::from_runtime)?;
    if runtime
        .image_by_id(&image.id)
        .map_err(InspectionRefusal::from_runtime)?
        .is_none()
    {
        return Err(InspectionRefusal::ImageIdAbsent {
            id: image.id.clone(),
        });
    }
    inspect_volumes(
        runtime,
        record.credential_volumes.as_ref().unwrap_or(&EMPTY),
    )?;

    match runtime
        .image_by_reference(&image.reference)
        .map_err(InspectionRefusal::from_runtime)?
    {
        Some(now) if now.id != image.id => warnings.push(format!(
            "[runner] the recorded image reference `{}` now names image `{}` in the container \
             runtime; this run continues from its recorded image id `{}`, so a moved reference \
             cannot change what executes",
            image.reference, now.id, image.id
        )),
        None => warnings.push(format!(
            "[runner] the recorded image reference `{}` no longer resolves in the container \
             runtime; this run continues from its recorded image id `{}`, which the runtime \
             still holds",
            image.reference, image.id
        )),
        Some(_) => {}
    }
    if let Some(field) = configured_difference(record, today) {
        warnings.push(format!(
            "[runner] in the config differs from the runner this run recorded: {field}. A run \
             keeps the boundary and image it started with, so the recorded runner is rebuilt and \
             the configured one is ignored.",
        ));
    }
    Ok(record.clone())
}

pub fn rebuild_from_record(
    runtime: &dyn ContainerRuntime,
    record: &RunnerPolicy,
    today: &RunnerSelection,
    preflight: &dyn RunnerPreflight,
    warnings: &mut Vec<String>,
) -> Result<RunnerPolicy, RebuildRefusal> {
    let rebuilt = rebuild_by_inspection(runtime, record, today, warnings)?;
    preflight
        .certify(&rebuilt)
        .map_err(RebuildRefusal::Preflight)?;
    Ok(rebuilt)
}

static EMPTY: BTreeMap<String, String> = BTreeMap::new();

fn configured_difference(record: &RunnerPolicy, today: &RunnerSelection) -> Option<RunnerField> {
    record.difference(&today_in_record_shape(record, today))
}

fn today_in_record_shape(record: &RunnerPolicy, today: &RunnerSelection) -> RunnerPolicy {
    match today.kind {
        RunnerKind::Host => RunnerPolicy {
            kind: RunnerKind::Host,
            policy: RunnerContract::HostV1,
            image: None,
            credential_volumes: None,
        },
        RunnerKind::Container => RunnerPolicy {
            kind: RunnerKind::Container,
            policy: RunnerContract::ContainerV1,
            image: Some(ImageIdentity {
                reference: today.image.clone().unwrap_or_default(),
                id: record
                    .image
                    .as_ref()
                    .map(|image| image.id.clone())
                    .unwrap_or_default(),
                digest: record.image.as_ref().and_then(|image| image.digest.clone()),
            }),
            credential_volumes: Some(today.credential_volumes.clone()),
        },
    }
}

#[cfg(test)]
mod tests;
