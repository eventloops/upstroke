//! Extended notes: `docs/internals/engine/topology/emit.md`

use std::fmt;

use crate::error::UpstrokeError;
use crate::events::log::{BarrierStep, EventLog, TopologyLine, establish_stable_prefix, site_for};
use crate::topology::effects::EventSite;
use crate::topology::events::{TopologyEvent, TopologyEventBody};
use crate::topology::fold::{FoldError, FrozenInputs, TopologyFold};

use super::identity::{InvocationLedger, Reservations};
use super::seams::{TimeSource, TopologyHooks};

#[derive(Debug, Clone)]
pub struct RunIdentity {
    pub run_id: String,
    pub inputs: FrozenInputs,
    pub committed_first_line_sha256: Option<String>,
}

pub struct EmitState<'a> {
    pub fold: &'a mut TopologyFold,
    pub log: &'a mut EventLog,
    pub reservations: &'a mut Reservations,
    pub warnings: &'a mut Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendOutcome {
    Present,
    Absent,
    Undetermined { step: BarrierStep, detail: String },
}

impl AppendOutcome {
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Present => {
                "the proven prefix contains the line: the transition is committed and durable"
                    .to_owned()
            }
            Self::Absent => {
                "the proven prefix does not contain the line: the previous prefix stands and is \
                 durable"
                    .to_owned()
            }
            Self::Undetermined { step, detail } => format!(
                "the outcome is undetermined — the stable-prefix barrier did not hold at {step} \
                 ({detail}), so neither the line's presence nor its absence is asserted"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstAppendDisposition {
    Committed,
    RetainedPossiblyCommitted,
    UndeterminedAndRetained,
}

impl fmt::Display for FirstAppendDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Committed => "committed",
            Self::RetainedPossiblyCommitted => "retained, possibly committed",
            Self::UndeterminedAndRetained => "undetermined and retained",
        })
    }
}

#[derive(Debug)]
pub struct AppendError {
    pub report: UncancelledAppend,
    pub cancelled_invocations: usize,
    _cancelled: Cancelled,
}

impl fmt::Display for AppendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "run `{}`: the `{}` append at `Event.{}` was entered and returned an error ({}), so \
             its outcome is unknown. {}. Nothing was retried, no state was derived from this \
             process's fold, and the run is resumable.",
            self.report.run_id,
            self.report.kind,
            self.report.site.name(),
            self.report.cause,
            self.report.outcome.describe()
        )?;
        if let Some(disposition) = self.report.creator_disposition() {
            write!(
                f,
                " The run directory is reported as {disposition}; neither half is deleted."
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for UncancelledAppend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "run `{}`: the `{}` append at `Event.{}` was entered and returned an error ({}), so \
             its outcome is unknown. {}. Nothing was retried, no state was derived from this \
             process's fold, and the run is resumable.",
            self.run_id,
            self.kind,
            self.site.name(),
            self.cause,
            self.outcome.describe()
        )?;
        if let Some(disposition) = self.creator_disposition() {
            write!(
                f,
                " The run directory is reported as {disposition}; neither half is deleted."
            )?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Cancelled(());

#[derive(Debug)]
#[must_use = "obligation (3) of the append-error protocol is outstanding: pass this to \
              `cancelling` with the run's ledger"]
pub struct UncancelledAppend {
    pub run_id: String,
    pub kind: &'static str,
    pub site: EventSite,
    pub cause: UpstrokeError,
    pub outcome: AppendOutcome,
    pub cancelled_reservation: bool,
}

impl UncancelledAppend {
    #[must_use]
    pub fn creator_disposition(&self) -> Option<FirstAppendDisposition> {
        if self.site != EventSite::AppendFirst {
            return None;
        }
        Some(match self.outcome {
            AppendOutcome::Present => FirstAppendDisposition::Committed,
            AppendOutcome::Absent => FirstAppendDisposition::RetainedPossiblyCommitted,
            AppendOutcome::Undetermined { .. } => FirstAppendDisposition::UndeterminedAndRetained,
        })
    }

    #[must_use]
    pub const fn resumable(&self) -> bool {
        true
    }
    pub fn cancelling(self, invocations: &mut InvocationLedger) -> AppendError {
        AppendError {
            report: self,
            cancelled_invocations: invocations.cancel_all_running(),
            _cancelled: Cancelled(()),
        }
    }
}

#[derive(Debug)]
pub enum EmitError {
    Unserializable(UpstrokeError),
    Refused(FoldError),
    NotEntered(UpstrokeError),
    AppendFailed(Box<UncancelledAppend>),
}

impl EmitError {
    #[must_use]
    pub const fn wrote_nothing(&self) -> bool {
        !matches!(self, Self::AppendFailed(_))
    }

    #[must_use]
    pub fn append_error(&self) -> Option<&UncancelledAppend> {
        match self {
            Self::AppendFailed(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for EmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unserializable(error) | Self::NotEntered(error) => write!(f, "{error}"),
            Self::Refused(error) => write!(f, "{error}"),
            Self::AppendFailed(error) => write!(
                f,
                "the `{}` append at `Event.{}` was entered and returned an error, and its \
                 in-flight invocations have not been cancelled yet",
                error.kind,
                error.site.name()
            ),
        }
    }
}

impl std::error::Error for EmitError {}

impl EmitError {
    #[allow(dead_code)]
    pub fn discharging(self, invocations: &mut InvocationLedger) -> UpstrokeError {
        match self {
            Self::Unserializable(error) | Self::NotEntered(error) => error,
            Self::Refused(refusal) => UpstrokeError::Refused {
                message: refusal.to_string(),
            },
            Self::AppendFailed(append) => UpstrokeError::Refused {
                message: append.cancelling(invocations).to_string(),
            },
        }
    }
}

pub fn emit(
    identity: &RunIdentity,
    state: &mut EmitState<'_>,
    time: &dyn TimeSource,
    body: TopologyEventBody,
    hooks: &mut dyn TopologyHooks,
) -> Result<TopologyEvent, EmitError> {
    let event = TopologyEvent {
        ts: time.now_rfc3339(),
        body,
    };
    let (line, checked) = TopologyLine::round_trip(&event).map_err(EmitError::Unserializable)?;

    let delta = state
        .fold
        .plan_transition(&checked)
        .map_err(EmitError::Refused)?;

    let site = site_for(&checked.body);
    let poisoned_before = state.log.poisoned_at().is_some();
    match state
        .log
        .append_topology_hooked(site, &line, hooks.events())
    {
        Ok(()) => {
            state.fold.apply_delta(delta);
            Ok(checked)
        }
        Err(cause) if poisoned_before || state.log.poisoned_at().is_none() => {
            Err(EmitError::NotEntered(cause))
        }
        Err(cause) => Err(EmitError::AppendFailed(Box::new(protocol(
            identity, state, &line, site, cause, hooks,
        )))),
    }
}

fn protocol(
    identity: &RunIdentity,
    state: &mut EmitState<'_>,
    line: &TopologyLine,
    site: EventSite,
    cause: UpstrokeError,
    hooks: &mut dyn TopologyHooks,
) -> UncancelledAppend {
    state.fold.poison();

    let cancelled_reservation = state.reservations.cancel_any();

    let path = state.log.path().to_path_buf();
    let outcome = match establish_stable_prefix(
        &path,
        identity.inputs.clone(),
        identity.committed_first_line_sha256.as_deref(),
        state.warnings,
        hooks.events(),
    ) {
        Ok(prefix) => {
            if prefix.bytes().ends_with(line.committed_bytes()) {
                AppendOutcome::Present
            } else {
                AppendOutcome::Absent
            }
        }
        Err(error) => AppendOutcome::Undetermined {
            step: error.step,
            detail: error.detail,
        },
    };

    UncancelledAppend {
        run_id: identity.run_id.clone(),
        kind: line.kind(),
        site,
        cause,
        outcome,
        cancelled_reservation,
    }
}

#[derive(Debug)]
#[must_use]
pub enum EmitFailure {
    Clean(UpstrokeError),
    Undischarged(Box<UncancelledAppend>),
}

impl EmitFailure {
    pub fn discharging(self, invocations: &mut InvocationLedger) -> UpstrokeError {
        match self {
            Self::Clean(error) => error,
            Self::Undischarged(append) => UpstrokeError::Refused {
                message: append.cancelling(invocations).to_string(),
            },
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub const fn wrote_nothing(&self) -> bool {
        matches!(self, Self::Clean(_))
    }
}

impl From<UpstrokeError> for EmitFailure {
    fn from(error: UpstrokeError) -> Self {
        Self::Clean(error)
    }
}

impl From<EmitError> for EmitFailure {
    fn from(error: EmitError) -> Self {
        match error {
            EmitError::AppendFailed(append) => Self::Undischarged(append),
            EmitError::Unserializable(error) | EmitError::NotEntered(error) => Self::Clean(error),
            EmitError::Refused(refusal) => Self::Clean(UpstrokeError::Refused {
                message: refusal.to_string(),
            }),
        }
    }
}

impl fmt::Display for EmitFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clean(error) => write!(f, "{error}"),
            Self::Undischarged(append) => write!(
                f,
                "the `{}` append at `Event.{}` was entered and returned an error, and its \
                 in-flight invocations have not been cancelled yet",
                append.kind,
                append.site.name()
            ),
        }
    }
}

#[cfg(test)]
mod tests;
