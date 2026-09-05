//! Extended notes: `docs/internals/ir.md`

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuestionId(pub String);

impl QuestionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for QuestionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for QuestionId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    pub const ALL: [Self; 5] = [Self::Low, Self::Medium, Self::High, Self::XHigh, Self::Max];

    pub const KNOWN: &'static str = "low, medium, high, xhigh, max";

    pub fn for_tier(tier: Tier) -> Self {
        match tier {
            Tier::Small => Self::Low,
            Tier::Mid => Self::Medium,
            Tier::Frontier => Self::High,
        }
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedEffortPolicy {
    pub small: Effort,
    pub mid: Effort,
    pub frontier: Effort,
    pub review: Effort,
}

impl ResolvedEffortPolicy {
    pub fn implementation_for(self, tier: Tier) -> Effort {
        match tier {
            Tier::Small => self.small,
            Tier::Mid => self.mid,
            Tier::Frontier => self.frontier,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSource {
    pub adapter: String,
    pub hash: String,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Edit,
    ReadOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerProfile {
    pub name: String,
    pub agent: String,
    pub model: String,
    pub pool: String,
    pub permissions: PermissionMode,
    #[serde(default)]
    pub effort: Option<Effort>,
    pub max_turns: Option<u32>,
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Completed,
    AgentError,
    Timeout,
    RateLimited,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub num_turns: Option<u32>,
    #[serde(default)]
    pub reasoning_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub status: OutcomeStatus,
    pub diff: String,
    pub detail: Option<String>,
    pub session_id: Option<String>,
    pub usage: Option<Usage>,
    pub cost_usd: Option<f64>,
    pub transcript_path: PathBuf,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub pass: bool,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub required_changes: Vec<String>,
    #[serde(default)]
    pub needs_human: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    Unblock,
    ApproveSpend,
    Continue,
    Clarify,
}

impl fmt::Display for QuestionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unblock => "unblock",
            Self::ApproveSpend => "approve-spend",
            Self::Continue => "continue",
            Self::Clarify => "clarify",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub id: QuestionId,
    pub kind: QuestionKind,
    pub affected_tasks: Vec<TaskId>,
    pub context: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    Answered { text: String },
    Declined,
    Unanswered,
}

pub fn content_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes.iter().filter(|b| **b != b'\r') {
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
    fn a_verdict_without_needs_human_is_a_judgement_not_an_escalation() {
        let verdict: Verdict =
            serde_json::from_str(r#"{"pass": false, "reasons": ["no tests"]}"#).expect("parse");
        assert!(!verdict.needs_human);
        assert!(!verdict.pass);
    }

    #[test]
    fn answers_round_trip_and_keep_declined_apart_from_unanswered() {
        for answer in [
            Answer::Answered {
                text: "use the cursor format".to_owned(),
            },
            Answer::Declined,
            Answer::Unanswered,
        ] {
            let json = serde_json::to_string(&answer).expect("serialize");
            let back: Answer = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, answer, "{json}");
        }
        assert_ne!(Answer::Declined, Answer::Unanswered);
    }

    #[test]
    fn content_hash_is_stable() {
        assert_eq!(content_hash(b""), "cbf29ce484222325");
        assert_eq!(content_hash(b"upstroke"), content_hash(b"upstroke"));
        assert_ne!(content_hash(b"upstroke"), content_hash(b"tactvs"));
    }

    #[test]
    fn content_hash_ignores_line_ending_style() {
        assert_eq!(
            content_hash(b"# Plan\r\n\r\n## Task\r\n"),
            content_hash(b"# Plan\n\n## Task\n"),
            "a CRLF checkout must not look like a changed plan"
        );
    }

    #[test]
    fn task_kind_all_lists_every_variant_exactly_once_in_order() {
        fn successor(kind: TaskKind) -> Option<TaskKind> {
            match kind {
                TaskKind::Design => Some(TaskKind::Implement),
                TaskKind::Implement => Some(TaskKind::Fix),
                TaskKind::Fix => Some(TaskKind::Refactor),
                TaskKind::Refactor => Some(TaskKind::Test),
                TaskKind::Test => Some(TaskKind::Docs),
                TaskKind::Docs => Some(TaskKind::Chore),
                TaskKind::Chore => None,
            }
        }

        let mut expected = vec![TaskKind::Design];
        while let Some(next) = successor(*expected.last().expect("seeded non-empty")) {
            expected.push(next);
            if expected.len() > TaskKind::ALL.len() {
                break;
            }
        }
        assert_eq!(
            expected,
            TaskKind::ALL,
            "TaskKind::ALL must list every variant, in declaration order"
        );

        for kind in TaskKind::ALL {
            assert_eq!(TaskKind::parse(&kind.to_string()), Some(kind));
            let json = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(json, format!("\"{kind}\""));
        }
    }
}
