//! Extended notes: `docs/internals/topology/queue.md`

use crate::topology::events::{CandidateRef, GenerationId, SequenceId};
use crate::topology::leases::LeaseTable;
use crate::topology::paths::{PathPolicy, PathSet};
use crate::topology::registry::TaskKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEntry {
    pub candidate: CandidateRef,
    pub paths: PathSet,
    pub lineage_root: Option<TaskKey>,
    pub verification_deferred: bool,
    pub defers: u32,
    pub sequence: Option<SequenceId>,
}

impl QueueEntry {
    pub fn key(&self) -> TaskKey {
        self.candidate.key
    }

    pub fn generation(&self) -> GenerationId {
        self.candidate.generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ineligible {
    AwaitingInput,
    VerificationDeferred,
    InsideLineage { root: TaskKey },
    BehindOlderLineage { root: TaskKey },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateQueue {
    entries: Vec<QueueEntry>,
}

impl CandidateQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, entry: QueueEntry) {
        self.entries.push(entry);
    }

    pub fn remove(&mut self, key: TaskKey, generation: GenerationId) {
        self.entries
            .retain(|entry| entry.key() != key || entry.generation() != generation);
    }

    pub fn entries(&self) -> &[QueueEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, key: TaskKey, generation: GenerationId) -> Option<&QueueEntry> {
        self.entries
            .iter()
            .find(|entry| entry.key() == key && entry.generation() == generation)
    }

    pub fn get_mut(&mut self, key: TaskKey, generation: GenerationId) -> Option<&mut QueueEntry> {
        self.entries
            .iter_mut()
            .find(|entry| entry.key() == key && entry.generation() == generation)
    }

    pub fn holds_task(&self, key: TaskKey) -> bool {
        self.entries.iter().any(|entry| entry.key() == key)
    }

    pub fn wake_deferred(&mut self) {
        for entry in &mut self.entries {
            entry.verification_deferred = false;
        }
    }

    pub fn ineligible<F>(
        entry: &QueueEntry,
        awaiting_input: &F,
        leases: &LeaseTable,
        policy: &PathPolicy,
    ) -> Option<Ineligible>
    where
        F: Fn(TaskKey) -> bool,
    {
        if awaiting_input(entry.key()) {
            return Some(Ineligible::AwaitingInput);
        }
        if entry.verification_deferred {
            return Some(Ineligible::VerificationDeferred);
        }
        let mut overlapping = leases.overlapping_lineages(&entry.paths, policy);
        match entry.lineage_root {
            None => overlapping
                .next()
                .map(|lease| Ineligible::InsideLineage { root: lease.root }),
            Some(mine) => {
                let own_age = leases.lineage(mine).map_or(u32::MAX, |lease| lease.age);
                overlapping
                    .find(|lease| lease.root != mine && lease.age < own_age)
                    .map(|lease| Ineligible::BehindOlderLineage { root: lease.root })
            }
        }
    }

    pub fn first_eligible<F>(
        &self,
        awaiting_input: F,
        leases: &LeaseTable,
        policy: &PathPolicy,
    ) -> Option<&QueueEntry>
    where
        F: Fn(TaskKey) -> bool,
    {
        self.entries
            .iter()
            .find(|entry| Self::ineligible(entry, &awaiting_input, leases, policy).is_none())
    }
}
