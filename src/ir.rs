//! Core data model — the DESIGN.md §7 subset needed by `tactus validate`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable identifier for a task within a plan.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub String);

impl TaskId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for TaskId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Identifier for an artifact flowing between tasks (contracts, briefs).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ArtifactId(pub String);

impl ArtifactId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ArtifactId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Abstract capability tier. Ordering matters: `Small < Mid < Frontier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Small,
    Mid,
    Frontier,
}

impl Tier {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "small" => Some(Self::Small),
            "mid" => Some(Self::Mid),
            "frontier" => Some(Self::Frontier),
            _ => None,
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Small => "small",
            Self::Mid => "mid",
            Self::Frontier => "frontier",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Design,
    Implement,
    Fix,
    Refactor,
    Test,
    Docs,
    Chore,
}

impl TaskKind {
    pub const ALL: [Self; 7] = [
        Self::Design,
        Self::Implement,
        Self::Fix,
        Self::Refactor,
        Self::Test,
        Self::Docs,
        Self::Chore,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "design" => Some(Self::Design),
            "implement" => Some(Self::Implement),
            "fix" => Some(Self::Fix),
            "refactor" => Some(Self::Refactor),
            "test" => Some(Self::Test),
            "docs" => Some(Self::Docs),
            "chore" => Some(Self::Chore),
            _ => None,
        }
    }
}

impl fmt::Display for TaskKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Design => "design",
            Self::Implement => "implement",
            Self::Fix => "fix",
            Self::Refactor => "refactor",
            Self::Test => "test",
            Self::Docs => "docs",
            Self::Chore => "chore",
        })
    }
}

/// Where a plan came from: which adapter parsed it and a content hash of the
/// original text, so a run can detect that its plan file changed underneath it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSource {
    pub adapter: String,
    pub hash: String,
}

/// Artifact stub — full artifact handling (files on disk, injection into
/// prompts) arrives with execution; validate only tracks identity and wiring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub produced_by: Option<TaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    pub title: String,
    pub body: String,
    pub depends_on: Vec<TaskId>,
    pub acceptance: Vec<String>,
    pub path_hints: Vec<String>,
    pub suggested_tier: Option<Tier>,
    pub min_tier: Option<Tier>,
    pub artifacts_in: Vec<ArtifactId>,
    pub artifacts_out: Vec<ArtifactId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub source: PlanSource,
    pub tasks: Vec<Task>,
    pub artifacts: Vec<Artifact>,
}

/// FNV-1a 64-bit content hash. Dependency-free and stable across platforms and
/// releases — identity only, nothing cryptographic.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_ordering_small_to_frontier() {
        assert!(Tier::Small < Tier::Mid);
        assert!(Tier::Mid < Tier::Frontier);
    }

    #[test]
    fn tier_and_kind_parse_case_insensitively() {
        assert_eq!(Tier::parse("Frontier"), Some(Tier::Frontier));
        assert_eq!(Tier::parse("nope"), None);
        assert_eq!(TaskKind::parse("FIX"), Some(TaskKind::Fix));
        assert_eq!(TaskKind::parse(""), None);
    }

    #[test]
    fn tier_serde_uses_lowercase() {
        let json = serde_json::to_string(&Tier::Frontier).expect("serialize");
        assert_eq!(json, "\"frontier\"");
        let back: Tier = serde_json::from_str("\"mid\"").expect("deserialize");
        assert_eq!(back, Tier::Mid);
    }

    #[test]
    fn content_hash_is_stable() {
        assert_eq!(content_hash(b""), "cbf29ce484222325");
        assert_eq!(content_hash(b"tactus"), content_hash(b"tactus"));
        assert_ne!(content_hash(b"tactus"), content_hash(b"tactvs"));
    }
}
