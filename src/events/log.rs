//! Extended notes: `docs/internals/events/log.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use super::{Event, EventBody};
use crate::error::UpstrokeError;
use crate::topology::effects::{
    EffectSiteId, EventSite, HookHarness, HookPhase, Injection, InjectionMode, SubEffectPoint,
};
use crate::topology::events::{TopologyEvent, TopologyEventBody};
use crate::topology::fold::{FrozenInputs, TopologyFold};
use crate::util::{DurabilityLedger, DurableStep};

pub trait EventHooks {
    fn phase(&mut self, _site: EventSite, _phase: HookPhase) {}

    fn point(
        &mut self,
        _site: EventSite,
        _point: SubEffectPoint,
        _mode: InjectionMode,
    ) -> Injection {
        Injection::Proceed
    }

    fn written_kill_shape(&mut self, _site: EventSite) -> WrittenShape {
        WrittenShape::Complete
    }

    fn durability_ledger(&self) -> DurabilityLedger {
        DurabilityLedger::off()
    }

    fn synced(&mut self, _record: &SyncRecord) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrittenShape {
    Torn,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRecord {
    pub site: EventSite,
    pub point: SubEffectPoint,
    pub target: SyncTarget,
    pub len: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncTarget {
    LogFile,
    LogDirectory,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoEventHooks;

impl EventHooks for NoEventHooks {}

#[derive(Debug, Clone)]
pub struct HarnessEventHooks {
    harness: Arc<Mutex<HookHarness>>,
    ledger: DurabilityLedger,
    syncs: Arc<Mutex<Vec<SyncRecord>>>,
    written: WrittenShape,
}

impl HarnessEventHooks {
    #[must_use]
    pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {
        Self {
            harness,
            ledger: DurabilityLedger::off(),
            syncs: Arc::new(Mutex::new(Vec::new())),
            written: WrittenShape::Complete,
        }
    }

    #[must_use]
    pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {
        &self.harness
    }

    #[must_use]
    pub fn recording_durability(mut self) -> Self {
        self.ledger = DurabilityLedger::recording();
        self
    }

    #[must_use]
    pub fn with_written_kill_shape(mut self, shape: WrittenShape) -> Self {
        self.written = shape;
        self
    }

    #[must_use]
    pub fn ledger(&self) -> DurabilityLedger {
        self.ledger.clone()
    }

    #[must_use]
    pub fn syncs(&self) -> Vec<SyncRecord> {
        self.syncs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl EventHooks for HarnessEventHooks {
    fn phase(&mut self, site: EventSite, phase: HookPhase) {
        let mut harness = self
            .harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        harness.hook(EffectSiteId::Event(site), phase);
    }

    fn point(&mut self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> Injection {
        let mut harness = self
            .harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        harness.hook(EffectSiteId::Event(site), HookPhase::Point { point, mode })
    }

    fn durability_ledger(&self) -> DurabilityLedger {
        self.ledger.clone()
    }

    fn synced(&mut self, record: &SyncRecord) {
        self.syncs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(record.clone());
    }

    fn written_kill_shape(&mut self, _site: EventSite) -> WrittenShape {
        self.written
    }
}

fn apply(
    injection: Injection,
    site: EventSite,
    point: SubEffectPoint,
    path: &Path,
) -> Result<(), UpstrokeError> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(injected(site, point, path)),
    }
}

fn injected(site: EventSite, point: SubEffectPoint, path: &Path) -> UpstrokeError {
    UpstrokeError::EventLog {
        path: path.to_path_buf(),
        message: format!(
            "{INJECTED_PREFIX}`{}` was made to return an error at its `{}` point",
            EffectSiteId::Event(site),
            point.name()
        ),
    }
}

pub const INJECTED_PREFIX: &str = "simulated fault: ";

#[derive(Debug)]
pub struct EventLog {
    path: PathBuf,
    file: File,
    opened_at: EventSite,
    poisoned: Option<(EventSite, SubEffectPoint)>,
}

impl EventLog {
    pub fn open(
        site: EventSite,
        path: &Path,
        warnings: &mut Vec<String>,
    ) -> Result<Self, UpstrokeError> {
        Self::open_hooked(site, path, warnings, &mut NoEventHooks)
    }

    pub fn open_hooked(
        site: EventSite,
        path: &Path,
        warnings: &mut Vec<String>,
        hooks: &mut dyn EventHooks,
    ) -> Result<Self, UpstrokeError> {
        Self::open_with_prefix(site, path, warnings, hooks).map(|(log, _)| log)
    }

    fn open_with_prefix(
        site: EventSite,
        path: &Path,
        warnings: &mut Vec<String>,
        hooks: &mut dyn EventHooks,
    ) -> Result<(Self, Vec<u8>), UpstrokeError> {
        match site {
            EventSite::OpenLog => {
                hooks.phase(site, HookPhase::Before);
                let opened =
                    Self::open_funnel(site, path, warnings, hooks).map_err(|(_, error)| error);
                if opened.is_ok() {
                    hooks.phase(site, HookPhase::After);
                }
                opened
            }
            EventSite::LegacyOpenLog => {
                hooks.phase(site, HookPhase::Before);
                let opened = Self::open_legacy(site, path, warnings);
                if opened.is_ok() {
                    hooks.phase(site, HookPhase::After);
                }
                opened
            }
            other => Err(wrong_site(other, path, "an open site", OPEN_SITES)),
        }
    }

    fn open_legacy(
        site: EventSite,
        path: &Path,
        warnings: &mut Vec<String>,
    ) -> Result<(Self, Vec<u8>), UpstrokeError> {
        let io = |source| UpstrokeError::Io {
            path: path.to_path_buf(),
            source,
        };
        let mut prefix = Vec::new();
        match crate::util::read_file_bounded(path) {
            Ok(existing) if !existing.is_empty() && existing.last() != Some(&b'\n') => {
                let keep = existing
                    .iter()
                    .rposition(|byte| *byte == b'\n')
                    .map_or(0, |index| index + 1);
                OpenOptions::new()
                    .write(true)
                    .open(path)
                    .map_err(io)?
                    .set_len(keep as u64)
                    .map_err(io)?;
                warnings.push(torn_tail_warning(path, existing.len() - keep));
                prefix.extend_from_slice(&existing[..keep]);
            }
            Ok(existing) => prefix = existing,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(io(source)),
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(io)?;
        Ok((
            Self {
                path: path.to_path_buf(),
                file,
                opened_at: site,
                poisoned: None,
            },
            prefix,
        ))
    }

    fn open_funnel(
        site: EventSite,
        path: &Path,
        warnings: &mut Vec<String>,
        hooks: &mut dyn EventHooks,
    ) -> Result<(Self, Vec<u8>), (BarrierStep, UpstrokeError)> {
        let io = |source| {
            (
                BarrierStep::OpenLog,
                UpstrokeError::Io {
                    path: path.to_path_buf(),
                    source,
                },
            )
        };
        let existing = match crate::util::read_file_bounded(path) {
            Ok(bytes) => Some(bytes),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => return Err(io(source)),
        };
        let created = existing.is_none();
        let mut truncated = false;
        let mut prefix = Vec::new();

        if let Some(existing) = existing {
            if !existing.is_empty() && existing.last() != Some(&b'\n') {
                let keep = existing
                    .iter()
                    .rposition(|byte| *byte == b'\n')
                    .map_or(0, |index| index + 1);
                OpenOptions::new()
                    .write(true)
                    .open(path)
                    .map_err(io)?
                    .set_len(keep as u64)
                    .map_err(io)?;
                hooks
                    .durability_ledger()
                    .record(DurableStep::Truncated, path, keep as u64);
                warnings.push(torn_tail_warning(path, existing.len() - keep));
                truncated = true;
                prefix.extend_from_slice(&existing[..keep]);
                for mode in InjectionMode::ALL {
                    apply(
                        hooks.point(site, SubEffectPoint::TruncateTornTail, *mode),
                        site,
                        SubEffectPoint::TruncateTornTail,
                        path,
                    )
                    .map_err(|error| (BarrierStep::OpenLog, error))?;
                }
            } else {
                prefix = existing;
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(io)?;

        if created {
            sync_directory(path, hooks, site, SubEffectPoint::Create)
                .map_err(|error| (BarrierStep::OpenLog, error))?;
            for mode in InjectionMode::ALL {
                apply(
                    hooks.point(site, SubEffectPoint::Create, *mode),
                    site,
                    SubEffectPoint::Create,
                    path,
                )
                .map_err(|error| (BarrierStep::OpenLog, error))?;
            }
        }

        for mode in InjectionMode::ALL {
            apply(
                hooks.point(site, SubEffectPoint::SyncPrefix, *mode),
                site,
                SubEffectPoint::SyncPrefix,
                path,
            )
            .map_err(|error| (BarrierStep::SyncPrefix, error))?;
        }
        sync_log_file(&file, path, hooks, site)
            .map_err(|error| (BarrierStep::SyncPrefix, error))?;
        if truncated {
            sync_directory(path, hooks, site, SubEffectPoint::SyncPrefix)
                .map_err(|error| (BarrierStep::SyncPrefix, error))?;
        }

        Ok((
            Self {
                path: path.to_path_buf(),
                file,
                opened_at: site,
                poisoned: None,
            },
            prefix,
        ))
    }

    pub fn append(&mut self, site: EventSite, body: EventBody) -> Result<Event, UpstrokeError> {
        self.append_hooked(site, body, &mut NoEventHooks)
    }

    pub fn append_hooked(
        &mut self,
        site: EventSite,
        body: EventBody,
        hooks: &mut dyn EventHooks,
    ) -> Result<Event, UpstrokeError> {
        if site != EventSite::LegacyAppend {
            return Err(wrong_site(
                site,
                &self.path,
                "the schema-1..3 append site",
                &[EventSite::LegacyAppend],
            ));
        }
        self.check_scope(site)?;
        self.check_poison()?;
        hooks.phase(site, HookPhase::Before);
        let event = Event::now(body);
        let mut line = serde_json::to_string(&event).map_err(|e| UpstrokeError::EventLog {
            path: self.path.clone(),
            message: format!("serializing {}: {e}", event.body.kind()),
        })?;
        let written = serde_json::from_str(&line).map_err(|e| UpstrokeError::EventLog {
            path: self.path.clone(),
            message: format!(
                "{} does not survive its own wire format ({e}); the log could not be replayed",
                event.body.kind()
            ),
        })?;
        line.push('\n');
        self.write_committed(site, line.as_bytes(), hooks)?;
        hooks.phase(site, HookPhase::After);
        Ok(written)
    }

    pub fn append_topology(
        &mut self,
        site: EventSite,
        line: &TopologyLine,
    ) -> Result<(), UpstrokeError> {
        self.append_topology_hooked(site, line, &mut NoEventHooks)
    }

    pub fn append_topology_hooked(
        &mut self,
        site: EventSite,
        line: &TopologyLine,
        hooks: &mut dyn EventHooks,
    ) -> Result<(), UpstrokeError> {
        if !TOPOLOGY_APPEND_SITES.contains(&site) {
            return Err(wrong_site(
                site,
                &self.path,
                "a schema-4 append site",
                TOPOLOGY_APPEND_SITES,
            ));
        }
        self.check_scope(site)?;
        if line.site() != site {
            return Err(UpstrokeError::EventLog {
                path: self.path.clone(),
                message: format!(
                    "`{}` belongs at `Event.{}`, not `Event.{}`; filing it under the wrong site \
                     would file its faults under the wrong registry coordinate",
                    line.kind(),
                    line.site().name(),
                    site.name()
                ),
            });
        }
        self.check_poison()?;
        hooks.phase(site, HookPhase::Before);
        self.write_committed(site, line.committed_bytes(), hooks)?;
        hooks.phase(site, HookPhase::After);
        Ok(())
    }

    fn write_committed(
        &mut self,
        site: EventSite,
        bytes: &[u8],
        hooks: &mut dyn EventHooks,
    ) -> Result<(), UpstrokeError> {
        let ledger = hooks.durability_ledger();
        if hooks.point(site, SubEffectPoint::Written, InjectionMode::ErrorReturn)
            == Injection::Error
        {
            let cut = torn_cut(bytes);
            let partial =
                self.write_or_poison(site, &bytes[..cut], SubEffectPoint::Written, &ledger);
            self.poisoned = Some((site, SubEffectPoint::Written));
            return Err(partial
                .err()
                .unwrap_or_else(|| injected(site, SubEffectPoint::Written, &self.path)));
        }

        match hooks.written_kill_shape(site) {
            WrittenShape::Torn => {
                let cut = torn_cut(bytes);
                self.write_or_poison(site, &bytes[..cut], SubEffectPoint::Written, &ledger)?;
                self.at_point(hooks, site, SubEffectPoint::Written, InjectionMode::Kill)?;
                self.write_or_poison(site, &bytes[cut..], SubEffectPoint::Written, &ledger)?;
            }
            WrittenShape::Complete => {
                self.write_or_poison(site, bytes, SubEffectPoint::Written, &ledger)?;
                self.at_point(hooks, site, SubEffectPoint::Written, InjectionMode::Kill)?;
            }
        }

        self.at_point(
            hooks,
            site,
            SubEffectPoint::WrittenFull,
            InjectionMode::ErrorReturn,
        )?;
        let flushed = self.file.flush();
        ledger.record(DurableStep::Flushed, &self.path, 0);
        if let Err(error) = flushed {
            self.poisoned = Some((site, SubEffectPoint::WrittenFull));
            return Err(self.io(error));
        }

        let synced = self.file.sync_data();
        let durable = self.file.metadata().map(|meta| meta.len()).unwrap_or(0);
        ledger.record(DurableStep::SyncedData, &self.path, durable);
        if let Err(error) = synced {
            self.poisoned = Some((site, SubEffectPoint::Synced));
            return Err(self.io(error));
        }
        for mode in InjectionMode::ALL {
            self.at_point(hooks, site, SubEffectPoint::Synced, *mode)?;
        }
        Ok(())
    }

    fn at_point(
        &mut self,
        hooks: &mut dyn EventHooks,
        site: EventSite,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) -> Result<(), UpstrokeError> {
        let answer = hooks.point(site, point, mode);
        if answer == Injection::Proceed {
            return Ok(());
        }
        self.poisoned = Some((site, point));
        apply(answer, site, point, &self.path)
    }

    fn io(&self, source: std::io::Error) -> UpstrokeError {
        UpstrokeError::Io {
            path: self.path.clone(),
            source,
        }
    }

    fn write_or_poison(
        &mut self,
        site: EventSite,
        bytes: &[u8],
        point: SubEffectPoint,
        ledger: &DurabilityLedger,
    ) -> Result<(), UpstrokeError> {
        let written = self.file.write_all(bytes);
        ledger.record(DurableStep::Wrote, &self.path, bytes.len() as u64);
        if let Err(error) = written {
            self.poisoned = Some((site, point));
            return Err(self.io(error));
        }
        Ok(())
    }

    fn check_scope(&self, site: EventSite) -> Result<(), UpstrokeError> {
        let legacy_handle = self.opened_at == EventSite::LegacyOpenLog;
        let legacy_append = site == EventSite::LegacyAppend;
        if legacy_handle == legacy_append {
            return Ok(());
        }
        Err(UpstrokeError::EventLog {
            path: self.path.clone(),
            message: format!(
                "a handle opened at `Event.{}` does not accept `Event.{}`: mixing the scopes would \
                 put schema-4 lines in a schema-3 log, and would let a legacy append report \
                 coverage for a Shared site",
                self.opened_at.name(),
                site.name()
            ),
        })
    }

    fn check_poison(&self) -> Result<(), UpstrokeError> {
        match self.poisoned {
            None => Ok(()),
            Some((site, point)) => Err(UpstrokeError::EventLog {
                path: self.path.clone(),
                message: format!(
                    "{POISONED_PREFIX}an append returned an error at `Event.{}`'s `{}` point, so \
                     this handle's outcome is unknown and nothing may be appended through it. \
                     Reopen the log through `Event.OpenLog` and establish the stable-prefix \
                     barrier.",
                    site.name(),
                    point.name()
                ),
            }),
        }
    }

    #[must_use]
    pub fn poisoned_at(&self) -> Option<SubEffectPoint> {
        self.poisoned.map(|(_, point)| point)
    }

    #[must_use]
    pub fn poisoned_site(&self) -> Option<EventSite> {
        self.poisoned.map(|(site, _)| site)
    }

    #[must_use]
    pub fn opened_at(&self) -> EventSite {
        self.opened_at
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub const POISONED_PREFIX: &str = "the event log handle is poisoned: ";

pub const OPEN_SITES: &[EventSite] = &[EventSite::OpenLog, EventSite::LegacyOpenLog];

pub const TOPOLOGY_APPEND_SITES: &[EventSite] = &[
    EventSite::AppendFirst,
    EventSite::Append,
    EventSite::AppendInformational,
];

#[must_use]
pub fn site_for(body: &TopologyEventBody) -> EventSite {
    if matches!(body, TopologyEventBody::RunStarted { .. }) {
        EventSite::AppendFirst
    } else if body.is_transaction() {
        EventSite::Append
    } else {
        EventSite::AppendInformational
    }
}

fn wrong_site(
    site: EventSite,
    path: &Path,
    expected: &str,
    allowed: &[EventSite],
) -> UpstrokeError {
    UpstrokeError::EventLog {
        path: path.to_path_buf(),
        message: format!(
            "`Event.{}` is not {expected} ({})",
            site.name(),
            allowed
                .iter()
                .map(|allowed| format!("Event.{}", allowed.name()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn torn_cut(bytes: &[u8]) -> usize {
    (bytes.len() / 2).max(1).min(bytes.len().saturating_sub(1))
}

fn torn_tail_warning(path: &Path, discarded: usize) -> String {
    format!(
        "{}: discarded {discarded} trailing byte(s) of an event that was never finished being \
         written — the shape an interrupted run leaves behind",
        path.display()
    )
}

fn sync_log_file(
    file: &File,
    path: &Path,
    hooks: &mut dyn EventHooks,
    site: EventSite,
) -> Result<(), UpstrokeError> {
    let io = |source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    };
    crate::util::fsync_file(file).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    hooks
        .durability_ledger()
        .record(DurableStep::SyncedFile, path, len);
    hooks.synced(&SyncRecord {
        site,
        point: SubEffectPoint::SyncPrefix,
        target: SyncTarget::LogFile,
        len,
        path: path.to_path_buf(),
    });
    Ok(())
}

fn sync_directory(
    path: &Path,
    hooks: &mut dyn EventHooks,
    site: EventSite,
    point: SubEffectPoint,
) -> Result<(), UpstrokeError> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    crate::util::fsync_dir(parent).map_err(|source| UpstrokeError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let len = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    hooks
        .durability_ledger()
        .record(DurableStep::SyncedDirectory, path, len);
    hooks.synced(&SyncRecord {
        site,
        point,
        target: SyncTarget::LogDirectory,
        len,
        path: path.to_path_buf(),
    });
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyLine {
    committed: String,
    kind: &'static str,
    site: EventSite,
}

impl TopologyLine {
    pub fn round_trip(event: &TopologyEvent) -> Result<(Self, TopologyEvent), UpstrokeError> {
        let kind = event.body.kind();
        let line = serde_json::to_string(event).map_err(|e| UpstrokeError::EventLog {
            path: PathBuf::new(),
            message: format!("serializing {kind}: {e}"),
        })?;
        let written: TopologyEvent =
            serde_json::from_str(&line).map_err(|e| UpstrokeError::EventLog {
                path: PathBuf::new(),
                message: format!(
                    "{kind} does not survive its own wire format ({e}); the log could not be \
                     replayed"
                ),
            })?;
        Ok((
            Self {
                committed: line + "\n",
                kind,
                site: site_for(&event.body),
            },
            written,
        ))
    }

    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    #[must_use]
    pub fn site(&self) -> EventSite {
        self.site
    }

    #[must_use]
    pub fn committed_bytes(&self) -> &[u8] {
        self.committed.as_bytes()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierStep {
    OpenLog,
    SyncPrefix,
    ProvePrefixStable,
    CheckedReplay,
}

impl BarrierStep {
    pub const ALL: &'static [Self] = &[
        Self::OpenLog,
        Self::SyncPrefix,
        Self::ProvePrefixStable,
        Self::CheckedReplay,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OpenLog => "Event.OpenLog",
            Self::SyncPrefix => "Event.OpenLog.SyncPrefix",
            Self::ProvePrefixStable => "Event.ProvePrefixStable",
            Self::CheckedReplay => "the checked replay",
        }
    }
}

impl fmt::Display for BarrierStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug)]
pub struct BarrierError {
    pub step: BarrierStep,
    pub path: PathBuf,
    pub detail: String,
}

impl fmt::Display for BarrierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the event log's stable-prefix barrier did not hold at {}: {} ({}). No append handle \
             was handed out and nothing derived from this log was acted on; the run is resumable.",
            self.step,
            self.detail,
            self.path.display()
        )
    }
}

impl std::error::Error for BarrierError {}

impl From<BarrierError> for UpstrokeError {
    fn from(error: BarrierError) -> Self {
        Self::EventLog {
            path: error.path.clone(),
            message: error.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct StablePrefix {
    log: EventLog,
    bytes: Vec<u8>,
    events: Vec<TopologyEvent>,
    fold: TopologyFold,
}

impl StablePrefix {
    #[must_use]
    pub fn log(&mut self) -> &mut EventLog {
        &mut self.log
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn events(&self) -> &[TopologyEvent] {
        &self.events
    }

    #[must_use]
    pub fn fold(&self) -> &TopologyFold {
        &self.fold
    }

    #[must_use]
    pub fn into_log_and_fold(self) -> (EventLog, Vec<u8>, Vec<TopologyEvent>, TopologyFold) {
        (self.log, self.bytes, self.events, self.fold)
    }
}

#[must_use]
pub fn first_line_digest(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|byte| *byte == b'\n')?;
    Some(format!("sha256:{:x}", Sha256::digest(&bytes[..end])))
}

pub fn establish_stable_prefix(
    path: &Path,
    inputs: FrozenInputs,
    committed_first_line_sha256: Option<&str>,
    warnings: &mut Vec<String>,
    hooks: &mut dyn EventHooks,
) -> Result<StablePrefix, BarrierError> {
    hooks.phase(EventSite::OpenLog, HookPhase::Before);
    let (log, normalized) = EventLog::open_funnel(EventSite::OpenLog, path, warnings, hooks)
        .map_err(|(step, error)| BarrierError {
            step,
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    hooks.phase(EventSite::OpenLog, HookPhase::After);

    hooks.phase(EventSite::ProvePrefixStable, HookPhase::Before);
    let reread = crate::util::read_file_bounded(path).map_err(|source| BarrierError {
        step: BarrierStep::ProvePrefixStable,
        path: path.to_path_buf(),
        detail: format!("the log could not be reread ({source})"),
    })?;

    if !reread.is_empty() && reread.last() != Some(&b'\n') {
        return Err(BarrierError {
            step: BarrierStep::ProvePrefixStable,
            path: path.to_path_buf(),
            detail: "the reread does not end at a commit marker — a torn tail reappeared after \
                     the truncation"
                .to_owned(),
        });
    }
    if reread.len() != normalized.len() {
        return Err(BarrierError {
            step: BarrierStep::ProvePrefixStable,
            path: path.to_path_buf(),
            detail: format!(
                "the reread is {} byte(s) where the prefix synced at open was {}",
                reread.len(),
                normalized.len()
            ),
        });
    }
    if reread != normalized {
        let first = reread
            .iter()
            .zip(&normalized)
            .position(|(reread, synced)| reread != synced)
            .unwrap_or(0);
        return Err(BarrierError {
            step: BarrierStep::ProvePrefixStable,
            path: path.to_path_buf(),
            detail: format!("the reread differs from the prefix synced at open at byte {first}"),
        });
    }
    if let Some(expected) = committed_first_line_sha256 {
        match first_line_digest(&reread) {
            Some(actual) if actual == expected => {}
            Some(actual) => {
                return Err(BarrierError {
                    step: BarrierStep::ProvePrefixStable,
                    path: path.to_path_buf(),
                    detail: format!(
                        "the committed first line digests {actual}, and the commit record says \
                         {expected}"
                    ),
                });
            }
            None => {
                return Err(BarrierError {
                    step: BarrierStep::ProvePrefixStable,
                    path: path.to_path_buf(),
                    detail: format!(
                        "the commit record says the first line digests {expected}, and the proven \
                         prefix has no committed first line"
                    ),
                });
            }
        }
    }
    hooks.phase(EventSite::ProvePrefixStable, HookPhase::After);

    let events = TopologyFold::parse_log(&reread).map_err(|error| BarrierError {
        step: BarrierStep::CheckedReplay,
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let fold = TopologyFold::replay(inputs, &events).map_err(|error| BarrierError {
        step: BarrierStep::CheckedReplay,
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;

    Ok(StablePrefix {
        log,
        events,
        bytes: reread,
        fold,
    })
}

pub fn read_all(path: &Path, warnings: &mut Vec<String>) -> Result<Vec<Event>, UpstrokeError> {
    let bytes = read_bytes(path)?;
    let parsed = parse_bytes(path, &bytes)?;
    warnings.extend(parsed.torn_tail_warning);
    Ok(parsed.events)
}

pub(crate) fn read_bytes(path: &Path) -> Result<Vec<u8>, UpstrokeError> {
    match crate::util::read_file_bounded(path) {
        Ok(bytes) => Ok(bytes),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Err(UpstrokeError::EventLog {
                path: path.to_path_buf(),
                message: "no event log here — this run never started, or its directory was \
                          removed"
                    .to_owned(),
            })
        }
        Err(source) => Err(UpstrokeError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) struct ParsedLines {
    pub events: Vec<Event>,
    pub torn_tail_warning: Option<String>,
}

pub(crate) fn parse_bytes(path: &Path, bytes: &[u8]) -> Result<ParsedLines, UpstrokeError> {
    let committed_end = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let (committed_bytes, trailing) = bytes.split_at(committed_end);
    let torn_tail_warning = (!trailing.is_empty()).then(|| {
        format!(
            "{}: dropped an incomplete final line ({} trailing byte(s)) — the shape an \
             interrupted write leaves behind",
            path.display(),
            trailing.len()
        )
    });
    let committed = std::str::from_utf8(committed_bytes).map_err(|error| {
        let line = committed_bytes[..error.valid_up_to()]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1;
        UpstrokeError::EventLog {
            path: path.to_path_buf(),
            message: format!(
                "line {line} contains invalid UTF-8 in a committed event ({error}). This is not a \
                 torn tail — the log has been rewritten, and state derived from what is left \
                 would be confidently wrong."
            ),
        }
    })?;

    let mut events = Vec::with_capacity(committed.lines().count());
    for (position, line) in committed.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event =
            serde_json::from_str::<Event>(line).map_err(|error| UpstrokeError::EventLog {
                path: path.to_path_buf(),
                message: format!(
                    "line {} is not a valid event ({error}). This is not a torn tail — the log has \
                 been rewritten, and state derived from what is left would be confidently wrong.",
                    position + 1
                ),
            })?;
        events.push(event);
    }
    Ok(ParsedLines {
        events,
        torn_tail_warning,
    })
}

#[derive(Debug)]
pub struct LogTail {
    path: PathBuf,
    offset: u64,
}

impl LogTail {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path, offset: 0 }
    }

    pub fn skip_existing(&mut self) {
        self.offset = std::fs::metadata(&self.path).map_or(0, |meta| meta.len());
    }

    pub fn poll(&mut self, warnings: &mut Vec<String>) -> Result<Vec<Event>, UpstrokeError> {
        let io = |source| UpstrokeError::Io {
            path: self.path.clone(),
            source,
        };
        let Ok(mut file) = File::open(&self.path) else {
            return Ok(Vec::new());
        };
        let length = file.metadata().map_err(io)?.len();
        if length <= self.offset {
            if length < self.offset {
                self.offset = 0;
            }
            if length == self.offset {
                return Ok(Vec::new());
            }
        }
        file.seek(SeekFrom::Start(self.offset)).map_err(io)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(io)?;
        let Some(end) = buffer.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(Vec::new());
        };
        let complete = &buffer[..=end];
        self.offset += complete.len() as u64;
        let parsed = parse_bytes(&self.path, complete)?;
        warnings.extend(parsed.torn_tail_warning);
        Ok(parsed.events)
    }
}

#[cfg(test)]
mod premove;

#[cfg(test)]
mod tests;
