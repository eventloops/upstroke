//! Extended notes: `docs/internals/engine/topology/seams.md`

use std::sync::{Arc, Mutex};

use crate::agent::proc::SpawnHooks;
use crate::events::log::EventHooks;
use crate::ir::QuestionId;
use crate::rundir::RunDirHooks;
use crate::runner::container::ContainerHooks;
use crate::topology::effects::HookHarness;
use crate::topology::events::IncarnationId;
use crate::workspace_manager::EffectHooks;

pub trait TopologyHooks {
    fn effects(&mut self) -> &mut dyn EffectHooks;

    fn rundir(&mut self) -> &mut dyn RunDirHooks;

    fn events(&mut self) -> &mut dyn EventHooks;

    fn container(&mut self) -> &mut dyn ContainerHooks;

    fn spawn(&mut self) -> &mut dyn SpawnHooks;
}

#[derive(Debug, Default)]
pub struct NoTopologyHooks {
    effects: crate::workspace_manager::NoHooks,
    rundir: crate::rundir::NoHooks,
    events: crate::events::log::NoEventHooks,
    container: crate::runner::container::NoHooks,
    spawn: crate::agent::proc::NoHooks,
}

impl NoTopologyHooks {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl TopologyHooks for NoTopologyHooks {
    fn effects(&mut self) -> &mut dyn EffectHooks {
        &mut self.effects
    }

    fn rundir(&mut self) -> &mut dyn RunDirHooks {
        &mut self.rundir
    }

    fn events(&mut self) -> &mut dyn EventHooks {
        &mut self.events
    }

    fn container(&mut self) -> &mut dyn ContainerHooks {
        &mut self.container
    }

    fn spawn(&mut self) -> &mut dyn SpawnHooks {
        &mut self.spawn
    }
}

#[derive(Debug, Clone)]
pub struct HarnessTopologyHooks {
    effects: crate::workspace_manager::HarnessEffects,
    rundir: crate::rundir::HarnessHooks,
    events: crate::events::log::HarnessEventHooks,
    container: crate::runner::container::HarnessHooks,
    spawn: crate::runner::HarnessHooks,
    #[allow(dead_code)]
    harness: Arc<Mutex<HookHarness>>,
}

impl HarnessTopologyHooks {
    #[must_use]
    pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {
        Self {
            effects: crate::workspace_manager::HarnessEffects::new(Arc::clone(&harness)),
            rundir: crate::rundir::HarnessHooks::new(Arc::clone(&harness)),
            events: crate::events::log::HarnessEventHooks::new(Arc::clone(&harness)),
            container: crate::runner::container::HarnessHooks::new(Arc::clone(&harness)),
            spawn: crate::runner::HarnessHooks::new(Arc::clone(&harness)),
            harness,
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {
        &self.harness
    }

    #[must_use]
    pub fn recording_durability(mut self) -> Self {
        self.effects = self.effects.recording_durability();
        self.rundir = self.rundir.clone().recording_durability();
        self.events = self.events.clone().recording_durability();
        self
    }

    #[must_use]
    pub fn with_written_kill_shape(mut self, shape: crate::events::log::WrittenShape) -> Self {
        self.events = self.events.clone().with_written_kill_shape(shape);
        self
    }

    #[must_use]
    pub fn event_observer(&self) -> &crate::events::log::HarnessEventHooks {
        &self.events
    }
}

impl TopologyHooks for HarnessTopologyHooks {
    fn effects(&mut self) -> &mut dyn EffectHooks {
        &mut self.effects
    }

    fn rundir(&mut self) -> &mut dyn RunDirHooks {
        &mut self.rundir
    }

    fn events(&mut self) -> &mut dyn EventHooks {
        &mut self.events
    }

    fn container(&mut self) -> &mut dyn ContainerHooks {
        &mut self.container
    }

    fn spawn(&mut self) -> &mut dyn SpawnHooks {
        &mut self.spawn
    }
}

pub trait TimeSource {
    fn now_rfc3339(&self) -> String;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl TimeSource for SystemClock {
    fn now_rfc3339(&self) -> String {
        crate::util::rfc3339_utc_now()
    }
}

pub trait IdSource {
    fn run_id(&self) -> String;

    fn incarnation(&self) -> IncarnationId;

    fn pid(&self) -> u32;

    fn question_id(&self) -> QuestionId;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RealIds;

impl IdSource for RealIds {
    fn question_id(&self) -> QuestionId {
        crate::interaction::new_question_id()
    }

    fn run_id(&self) -> String {
        crate::ulid::ulid()
    }

    fn incarnation(&self) -> IncarnationId {
        IncarnationId(crate::ulid::ulid())
    }

    fn pid(&self) -> u32 {
        std::process::id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::SPAWN_SITE;
    use crate::topology::effects::{
        EffectSiteId, EventSite, HookPhase, Injection, InjectionMode, RunDirSite, SubEffectPoint,
    };

    #[derive(Debug, Clone)]
    pub(crate) struct Fixed {
        pub(crate) ts: String,
        pub(crate) run_id: String,
        pub(crate) incarnation: String,
        pub(crate) pid: u32,
    }

    impl Default for Fixed {
        fn default() -> Self {
            Self {
                ts: "2026-08-23T09:41:02Z".to_owned(),
                run_id: "01KZTPR7000000000000000001".to_owned(),
                incarnation: "01KZTAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                pid: 4242,
            }
        }
    }

    impl TimeSource for Fixed {
        fn now_rfc3339(&self) -> String {
            self.ts.clone()
        }
    }

    impl IdSource for Fixed {
        fn run_id(&self) -> String {
            self.run_id.clone()
        }

        fn incarnation(&self) -> IncarnationId {
            IncarnationId(self.incarnation.clone())
        }

        fn pid(&self) -> u32 {
            self.pid
        }

        fn question_id(&self) -> QuestionId {
            QuestionId("q-fixed".to_owned())
        }
    }

    #[test]
    fn the_production_bundle_proceeds_from_every_family() {
        let mut hooks = NoTopologyHooks::new();
        let site = EffectSiteId::RunDir(RunDirSite::PublishMarker);

        assert_eq!(
            hooks.effects().phase(site, HookPhase::Before),
            Injection::Proceed
        );
        assert_eq!(
            hooks.rundir().hook(site, HookPhase::After),
            Injection::Proceed
        );
        assert_eq!(
            hooks.container().phase(site, HookPhase::Before),
            Injection::Proceed
        );
        hooks.events().phase(
            crate::topology::effects::EventSite::OpenLog,
            HookPhase::Before,
        );
        assert_eq!(
            hooks
                .spawn()
                .point(crate::topology::effects::SubEffectPoint::Exec),
            Injection::Proceed
        );
    }

    #[test]
    fn every_family_of_the_harness_bundle_records_into_the_same_harness() {
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .arm(
                SPAWN_SITE,
                SubEffectPoint::AmbientJobJoined,
                InjectionMode::ErrorReturn,
            )
            .expect("`Process.Spawn` exposes `AmbientJobJoined` with an error contract");
        let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));

        let marker = EffectSiteId::RunDir(RunDirSite::PublishMarker);
        let public = EffectSiteId::RunDir(RunDirSite::CreatePublicDir);
        let commit = EffectSiteId::RunDir(RunDirSite::PublishCommitRecord);
        let container = EffectSiteId::Container(crate::topology::effects::ContainerSite::Create);
        let worktree = EffectSiteId::Worktree(crate::topology::effects::WorktreeSite::Add);
        let append = EffectSiteId::Event(EventSite::AppendFirst);

        hooks.rundir().hook(marker, HookPhase::Before);
        hooks.effects().phase(worktree, HookPhase::Before);
        hooks.container().phase(container, HookPhase::After);
        hooks
            .events()
            .phase(EventSite::AppendFirst, HookPhase::Before);
        hooks.spawn().point(SubEffectPoint::Exec);
        assert_eq!(
            hooks.spawn().point(SubEffectPoint::AmbientJobJoined),
            Injection::Error,
            "an injection armed on the shared harness did not reach the spawn family"
        );

        let seen = harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (site, phase) in [
            (marker, HookPhase::Before),
            (worktree, HookPhase::Before),
            (container, HookPhase::After),
            (append, HookPhase::Before),
            (
                SPAWN_SITE,
                HookPhase::Point {
                    point: SubEffectPoint::AmbientJobJoined,
                    mode: InjectionMode::ErrorReturn,
                },
            ),
        ] {
            assert!(
                seen.observed(site, phase),
                "`{site}`'s `{phase}` did not reach the shared harness"
            );
        }
        assert!(
            seen.reached_point(SPAWN_SITE, SubEffectPoint::Exec, InjectionMode::Kill),
            "the spawn family's reachability did not reach the shared harness"
        );
        assert!(
            !seen.observed(public, HookPhase::Before),
            "a site nothing drove was recorded as observed"
        );
        assert!(
            !seen.observed(commit, HookPhase::Before),
            "a site nothing drove was recorded as observed"
        );
        assert!(
            !seen.observed(EffectSiteId::Event(EventSite::OpenLog), HookPhase::Before),
            "an event site nothing drove was recorded as observed"
        );
        assert!(
            !seen.reached_point(SPAWN_SITE, SubEffectPoint::Registered, InjectionMode::Kill),
            "a containment point nothing drove was recorded as reached"
        );
    }

    #[test]
    fn a_time_source_produces_the_timestamp_a_durable_event_records() {
        let live = SystemClock.now_rfc3339();
        assert_eq!(live.len(), 20, "RFC 3339 UTC to the second: {live}");
        assert!(live.ends_with('Z'), "{live}");
        assert_eq!(&live[4..5], "-");
        assert_eq!(&live[10..11], "T");

        let fixed = Fixed::default();
        assert_eq!(fixed.now_rfc3339(), "2026-08-23T09:41:02Z");
        assert_eq!(
            fixed.now_rfc3339(),
            fixed.now_rfc3339(),
            "a fixed clock that moved would defeat every byte-exact assertion"
        );
    }

    #[test]
    fn an_id_source_mints_fresh_identities_and_a_fixed_one_does_not() {
        assert_ne!(
            RealIds.run_id(),
            RealIds.run_id(),
            "two runs sharing an id would share a public directory"
        );
        assert_ne!(RealIds.incarnation(), RealIds.incarnation());
        assert_eq!(RealIds.pid(), std::process::id());

        let fixed = Fixed::default();
        assert_eq!(fixed.run_id(), fixed.run_id());
        assert_eq!(fixed.incarnation(), fixed.incarnation());
        assert_eq!(fixed.pid(), 4242);
    }
}
