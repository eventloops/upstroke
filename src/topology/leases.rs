//! Extended notes: `docs/internals/topology/leases.md`

use std::collections::BTreeMap;

use crate::topology::events::{GenerationId, LeaseDisposition};
use crate::topology::paths::{GitPath, PathPolicy, PathSet};
use crate::topology::registry::TaskKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LeaseOwner {
    Generation {
        key: TaskKey,
        generation: GenerationId,
    },
    Candidate {
        key: TaskKey,
        generation: GenerationId,
    },
    Lineage {
        root: TaskKey,
    },
}

impl LeaseOwner {
    pub fn key(self) -> TaskKey {
        match self {
            Self::Generation { key, .. } | Self::Candidate { key, .. } => key,
            Self::Lineage { root } => root,
        }
    }

    pub fn is_lineage(self) -> bool {
        matches!(self, Self::Lineage { .. })
    }
}

pub fn regions_overlap(left: &PathSet, right: &PathSet, policy: &PathPolicy) -> bool {
    match (left.prefixes(), right.prefixes()) {
        (None, _) | (_, None) => true,
        (Some(left), Some(right)) => left
            .iter()
            .any(|one| right.iter().any(|other| paths_overlap(one, other, policy))),
    }
}

pub fn paths_overlap(left: &GitPath, right: &GitPath, policy: &PathPolicy) -> bool {
    let mut left = components(left);
    let mut right = components(right);
    loop {
        match (left.next(), right.next()) {
            (None, _) | (_, None) => return true,
            (Some(one), Some(other)) => {
                if !components_equal(one, other, policy.case_fold) {
                    return false;
                }
            }
        }
    }
}

fn components(path: &GitPath) -> impl Iterator<Item = &str> {
    path.as_str().split('/').filter(|part| !part.is_empty())
}

fn components_equal(left: &str, right: &str, case_fold: bool) -> bool {
    if !case_fold {
        return left == right;
    }
    let mut left = left.chars().flat_map(char::to_lowercase);
    let mut right = right.chars().flat_map(char::to_lowercase);
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (one, other) if one == other => {}
            _ => return false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageLease {
    pub root: TaskKey,
    pub paths: PathSet,
    pub age: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeaseTable {
    held: BTreeMap<LeaseOwner, PathSet>,
    lineages: Vec<LineageLease>,
}

impl LeaseTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(&mut self, owner: LeaseOwner, paths: PathSet) {
        if let LeaseOwner::Lineage { root } = owner {
            let age = u32::try_from(self.lineages.len()).unwrap_or(u32::MAX);
            match self.lineages.iter_mut().find(|lease| lease.root == root) {
                Some(existing) => existing.paths = paths,
                None => self.lineages.push(LineageLease { root, paths, age }),
            }
            return;
        }
        self.held.insert(owner, paths);
    }

    pub fn widen_lineage(&mut self, root: TaskKey, paths: &PathSet) {
        let widened = match self.lineages.iter().find(|lease| lease.root == root) {
            Some(existing) => union(&existing.paths, paths),
            None => paths.clone(),
        };
        self.grant(LeaseOwner::Lineage { root }, widened);
    }

    pub fn release(&mut self, owner: LeaseOwner) {
        if let LeaseOwner::Lineage { root } = owner {
            self.lineages.retain(|lease| lease.root != root);
            return;
        }
        self.held.remove(&owner);
    }

    pub fn holds(&self, owner: LeaseOwner) -> bool {
        match owner {
            LeaseOwner::Lineage { root } => self.lineage(root).is_some(),
            _ => self.held.contains_key(&owner),
        }
    }

    pub fn lineage(&self, root: TaskKey) -> Option<&LineageLease> {
        self.lineages.iter().find(|lease| lease.root == root)
    }

    pub fn lineages(&self) -> &[LineageLease] {
        &self.lineages
    }

    pub fn any_candidate_or_lineage(&self) -> bool {
        !self.lineages.is_empty()
            || self
                .held
                .keys()
                .any(|owner| matches!(owner, LeaseOwner::Candidate { .. }))
    }

    pub fn overlaps_another(
        &self,
        owner: LeaseOwner,
        paths: &PathSet,
        policy: &PathPolicy,
    ) -> bool {
        self.held
            .iter()
            .any(|(held, region)| *held != owner && regions_overlap(region, paths, policy))
            || self.lineages.iter().any(|lease| {
                !matches!(owner, LeaseOwner::Lineage { root } if root == lease.root)
                    && regions_overlap(&lease.paths, paths, policy)
            })
    }

    pub fn overlapping_lineages<'a>(
        &'a self,
        paths: &'a PathSet,
        policy: &'a PathPolicy,
    ) -> impl Iterator<Item = &'a LineageLease> {
        self.lineages
            .iter()
            .filter(move |lease| regions_overlap(&lease.paths, paths, policy))
    }
}

fn union(left: &PathSet, right: &PathSet) -> PathSet {
    let (Some(left), Some(right)) = (left.prefixes(), right.prefixes()) else {
        return PathSet::RepoWide;
    };
    let mut paths: Vec<GitPath> = left.to_vec();
    for path in right {
        if !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    PathSet::Prefixes { paths }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationLease {
    Own,
    InheritedLineage { root: TaskKey },
}

impl GenerationLease {
    pub fn expected(self, survives: bool) -> LeaseDisposition {
        match self {
            Self::InheritedLineage { .. } => LeaseDisposition::LineageHeld,
            Self::Own if survives => LeaseDisposition::PredictedRetained,
            Self::Own => LeaseDisposition::PredictedReleased,
        }
    }
}
