//! Extended notes: `docs/internals/runner/container/runtime.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::topology::effects::ContainerSite;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeOp {
    Probe,
    InspectImageByReference,
    InspectImageById,
    InspectVolume,
    ListByLabel,
    Observe,
    Collect,
    Create,
    Start,
    Stop,
    Remove,
}

impl RuntimeOp {
    pub const ALL: &'static [Self] = &[
        Self::Probe,
        Self::InspectImageByReference,
        Self::InspectImageById,
        Self::InspectVolume,
        Self::ListByLabel,
        Self::Observe,
        Self::Collect,
        Self::Create,
        Self::Start,
        Self::Stop,
        Self::Remove,
    ];

    #[must_use]
    pub const fn is_effect(self) -> bool {
        match self {
            Self::Create | Self::Start | Self::Stop | Self::Remove => true,
            Self::Probe
            | Self::InspectImageByReference
            | Self::InspectImageById
            | Self::InspectVolume
            | Self::ListByLabel
            | Self::Observe
            | Self::Collect => false,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Probe => "probe",
            Self::InspectImageByReference => "inspect-image-by-reference",
            Self::InspectImageById => "inspect-image-by-id",
            Self::InspectVolume => "inspect-volume",
            Self::ListByLabel => "list-by-label",
            Self::Observe => "observe",
            Self::Collect => "collect",
            Self::Create => "create",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Remove => "remove",
        }
    }
}

impl fmt::Display for RuntimeOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    Unreachable {
        operation: RuntimeOp,
        detail: String,
    },
    Failed {
        operation: RuntimeOp,
        detail: String,
    },
}

impl RuntimeError {
    #[must_use]
    pub const fn operation(&self) -> RuntimeOp {
        match self {
            Self::Unreachable { operation, .. } | Self::Failed { operation, .. } => *operation,
        }
    }

    #[must_use]
    pub const fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable { .. })
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable { operation, detail } => write!(
                f,
                "the container runtime cannot be reached for `{operation}`: {detail}"
            ),
            Self::Failed { operation, detail } => {
                write!(f, "the container runtime refused `{operation}`: {detail}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInspection {
    pub id: String,
    pub digest: Option<String>,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mount {
    Path {
        source: PathBuf,
        target: String,
        read_only: bool,
    },
    Volume {
        name: String,
        target: String,
        read_only: bool,
    },
    Tmpfs {
        target: String,
    },
}

impl Mount {
    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            Self::Path { target, .. } | Self::Volume { target, .. } | Self::Tmpfs { target } => {
                target
            }
        }
    }

    #[must_use]
    pub const fn read_only(&self) -> bool {
        match self {
            Self::Path { read_only, .. } | Self::Volume { read_only, .. } => *read_only,
            Self::Tmpfs { .. } => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSpec {
    pub name: String,
    pub image_id: String,
    pub labels: BTreeMap<String, String>,
    pub mounts: Vec<Mount>,
    pub env: Vec<(String, String)>,
    pub command: Vec<String>,
    pub workdir: Option<String>,
    pub read_only_root: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedContainer {
    pub name: String,
    pub reported_image_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredContainer {
    pub name: String,
    pub labels: BTreeMap<String, String>,
}

impl DiscoveredContainer {
    #[must_use]
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(String::as_str)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Running,
    Exited,
    Gone,
}

impl Liveness {
    #[must_use]
    pub const fn is_terminated(self) -> bool {
        match self {
            Self::Exited | Self::Gone => true,
            Self::Running => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopMode {
    Graceful,
    Kill,
}

impl StopMode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Graceful => "graceful",
            Self::Kill => "kill",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerExecution {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait ContainerRuntime: Send + Sync {
    fn probe(&self) -> Result<(), RuntimeError>;

    fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError>;

    fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError>;

    fn volume_present(&self, name: &str) -> Result<bool, RuntimeError>;

    fn containers_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<DiscoveredContainer>, RuntimeError>;

    fn observe(&self, name: &str) -> Result<Liveness, RuntimeError>;

    fn collect(&self, name: &str) -> Result<ContainerExecution, RuntimeError>;

    fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError>;

    fn start(&self, name: &str) -> Result<(), RuntimeError>;

    fn stop(&self, name: &str, mode: StopMode) -> Result<(), RuntimeError>;

    fn remove(&self, name: &str) -> Result<(), RuntimeError>;
}

pub trait OwnerLiveness: Send + Sync {
    fn is_running(&self, public_run_dir: &Path) -> bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LockProbe;

impl OwnerLiveness for LockProbe {
    fn is_running(&self, public_run_dir: &Path) -> bool {
        crate::rundir::is_running(public_run_dir)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TracePhase {
    Before,
    After,
}

impl TracePhase {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableStep {
    Synced,
    Renamed,
    DirSynced,
    Removed,
}

impl DurableStep {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::Renamed => "renamed",
            Self::DirSynced => "dir-synced",
            Self::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEntry {
    Site {
        site: ContainerSite,
        phase: TracePhase,
    },
    Runtime {
        op: RuntimeOp,
        target: String,
    },
    Durable {
        step: DurableStep,
        path: PathBuf,
    },
    View {
        action: ViewAction,
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewAction {
    Materialized,
    Discarded,
}

impl ViewAction {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Materialized => "materialized",
            Self::Discarded => "discarded",
        }
    }
}

impl TraceEntry {
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Site { site, phase } => format!("site:{}:{}", site.name(), phase.name()),
            Self::Runtime { op, target } => format!("rt:{}:{target}", op.name()),
            Self::Durable { step, path } => format!(
                "durable:{}:{}",
                step.name(),
                path.file_name().map_or_else(
                    || path.to_string_lossy().into_owned(),
                    |name| name.to_string_lossy().into_owned()
                )
            ),
            Self::View { action, path } => format!(
                "view:{}:{}",
                action.name(),
                path.file_name().map_or_else(
                    || path.to_string_lossy().into_owned(),
                    |name| name.to_string_lossy().into_owned()
                )
            ),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContainerTrace(Option<Arc<Mutex<Vec<TraceEntry>>>>);

impl ContainerTrace {
    #[must_use]
    pub fn off() -> Self {
        Self(None)
    }

    #[must_use]
    pub fn recording() -> Self {
        Self(Some(Arc::new(Mutex::new(Vec::new()))))
    }

    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.0.is_some()
    }

    pub fn push(&self, entry: TraceEntry) {
        if let Some(log) = &self.0 {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(entry);
        }
    }

    pub fn site(&self, site: ContainerSite, phase: TracePhase) {
        self.push(TraceEntry::Site { site, phase });
    }

    pub fn runtime(&self, op: RuntimeOp, target: &str) {
        self.push(TraceEntry::Runtime {
            op,
            target: target.to_owned(),
        });
    }

    pub fn durable(&self, step: DurableStep, path: &Path) {
        self.push(TraceEntry::Durable {
            step,
            path: path.to_path_buf(),
        });
    }

    pub fn view(&self, action: ViewAction, path: &Path) {
        self.push(TraceEntry::View {
            action,
            path: path.to_path_buf(),
        });
    }

    #[must_use]
    pub fn entries(&self) -> Vec<TraceEntry> {
        self.0.as_ref().map_or_else(Vec::new, |log| {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }

    #[must_use]
    pub fn rendered(&self) -> Vec<String> {
        self.entries().iter().map(TraceEntry::render).collect()
    }

    #[must_use]
    pub fn sites(&self) -> Vec<(ContainerSite, TracePhase)> {
        self.entries()
            .into_iter()
            .filter_map(|entry| match entry {
                TraceEntry::Site { site, phase } => Some((site, phase)),
                _ => None,
            })
            .collect()
    }

    #[must_use]
    pub fn ops(&self) -> Vec<RuntimeOp> {
        self.entries()
            .into_iter()
            .filter_map(|entry| match entry {
                TraceEntry::Runtime { op, .. } => Some(op),
                _ => None,
            })
            .collect()
    }

    #[must_use]
    pub fn position(&self, needle: &str) -> Option<usize> {
        self.rendered().iter().position(|entry| entry == needle)
    }

    #[must_use]
    pub fn position_starting(&self, prefix: &str) -> Option<usize> {
        self.rendered()
            .iter()
            .position(|entry| entry.starts_with(prefix))
    }

    pub fn clear(&self) {
        if let Some(log) = &self.0 {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        }
    }
}
