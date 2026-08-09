//! Reviewer (DESIGN.md §11.2): an ordinary read-only worker profile judges
//! the engine-captured diff against the task's acceptance criteria and must
//! end its answer with a fenced JSON verdict.
//!
//! Two things make this more than a second opinion from the same model: the
//! reviewer sees the *diff* rather than the implementer's account of it
//! (invariant 3), and its prompt is explicitly anti-sycophantic — its job is
//! to find reasons to fail, not to agree. Unparseable output earns exactly
//! one re-ask (resuming the same session where the adapter supports it);
//! after that the attempt fails, because a reviewer that cannot answer in the
//! required shape has not reviewed anything.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::agent::{AgentAdapter, TaskRun, proc};
use crate::error::TactusError;
use crate::ir::{PermissionMode, Task, Verdict, WorkerProfile};
use crate::util;

/// Diff bytes shown to the reviewer. Large diffs are truncated head-first so
/// the most recent hunks survive; a task that changes more than this is
/// beyond what one review can meaningfully judge anyway.
pub const MAX_DIFF_BYTES: usize = 60 * 1024;

pub struct ReviewCx<'a> {
    pub adapter: &'a dyn AgentAdapter,
    pub profile: WorkerProfile,
    pub task: &'a Task,
    pub diff: &'a str,
    /// Artifacts the reviewer should judge against (conventions brief first).
    pub artifacts: &'a [(String, String)],
    pub workspace: &'a Path,
    pub run_dir: &'a Path,
    /// Unique file stem for this task's review artifacts.
    pub stem: String,
    pub timeout: Duration,
}

#[derive(Debug)]
pub struct ReviewOutcome {
    pub verdict: Verdict,
    pub cost_usd: Option<f64>,
    /// How many agent invocations it took (2 means the re-ask was needed).
    pub invocations: u32,
    pub transcript: PathBuf,
}

/// A read-only profile bound to the same rung the reviewer is configured for.
pub fn profile_for(agent: &str, model: &str, name: &str) -> WorkerProfile {
    WorkerProfile {
        name: name.to_owned(),
        agent: agent.to_owned(),
        model: model.to_owned(),
        pool: String::new(),
        permissions: PermissionMode::ReadOnly,
        max_turns: None,
        extra_args: Vec::new(),
    }
}

pub fn run_review(cx: &ReviewCx<'_>) -> Result<ReviewOutcome, TactusError> {
    // Reviewers run nothing: no gate commands, no edit tools (§20).
    let settings_path = cx.adapter.materialize_permissions(
        &cx.profile,
        &[],
        &cx.run_dir.join("settings"),
        &format!("{}-review", cx.stem),
    )?;
    let reviews_dir = cx.run_dir.join("reviews");
    let transcript = reviews_dir.join(format!("{}-review.json", cx.stem));

    let mut cost = None;
    let mut session = None;
    for invocation in 1..=2u32 {
        let prompt = if invocation == 1 {
            materialize_prompt(cx)
        } else {
            REASK_PROMPT.to_owned()
        };
        let task_run = TaskRun {
            prompt,
            profile: cx.profile.clone(),
            workspace: cx.workspace.to_path_buf(),
            // The re-ask continues the same conversation where possible: the
            // reviewer already read the diff, it just answered in the wrong
            // shape.
            resume_session: (invocation > 1).then(|| session.clone()).flatten(),
            settings_path: settings_path.clone(),
        };
        let command = cx.adapter.build(&task_run)?;
        let output =
            proc::run_with_timeout(command, cx.adapter.stdin_payload(&task_run), cx.timeout)?;

        let path = if invocation == 1 {
            transcript.clone()
        } else {
            reviews_dir.join(format!("{}-review-reask.json", cx.stem))
        };
        util::write_text(&path, &output.stdout)?;

        let outcome = cx.adapter.parse(&output)?;
        cost = add_cost(cost, outcome.cost_usd);
        session = outcome.session_id.clone().or(session);

        // The verdict lives in the agent's own answer text, so a failed
        // invocation still gets read — a reviewer that crashed after emitting
        // a verdict has still told us something.
        let answer = outcome.detail.clone().unwrap_or_default();
        if let Some(verdict) = parse_verdict(&answer) {
            return Ok(ReviewOutcome {
                verdict,
                cost_usd: cost,
                invocations: invocation,
                transcript: path,
            });
        }
    }

    // §11.2: one re-ask, then it counts as a failure.
    Ok(ReviewOutcome {
        verdict: Verdict {
            pass: false,
            reasons: vec![
                "reviewer did not return a parseable JSON verdict after a re-ask".to_owned(),
            ],
            required_changes: Vec::new(),
        },
        cost_usd: cost,
        invocations: 2,
        transcript,
    })
}

const REASK_PROMPT: &str = "Your previous answer did not end with a parseable verdict. Reply \
    with NOTHING except a single fenced JSON block in exactly this shape:\n\n\
    ```json\n\
    {\"pass\": false, \"reasons\": [\"...\"], \"required_changes\": [\"...\"]}\n\
    ```\n";

fn materialize_prompt(cx: &ReviewCx<'_>) -> String {
    let task = cx.task;
    let mut prompt = String::new();
    prompt.push_str(
        "You are reviewing one task's changes for the tactus engine. You have READ-ONLY access: \
         do not edit files, do not run commands.\n\n\
         Your job is to find reasons this change should NOT be accepted. A reviewer who agrees \
         by default is worthless. Be specific and cite the diff. If the change genuinely meets \
         every acceptance criterion and introduces no defect, say so plainly — but look hard \
         first.\n\n",
    );
    let _ = writeln!(prompt, "# Task under review: {}\n", task.title);
    if !task.body.is_empty() {
        let _ = writeln!(prompt, "{}\n", task.body);
    }
    if task.acceptance.is_empty() {
        prompt.push_str(
            "This task declares no explicit acceptance criteria; judge it against the task \
             description and the repository's existing conventions.\n\n",
        );
    } else {
        prompt.push_str("Acceptance criteria (every one must hold):\n");
        for item in &task.acceptance {
            let _ = writeln!(prompt, "- {item}");
        }
        prompt.push('\n');
    }
    for (name, content) in cx.artifacts {
        let _ = writeln!(
            prompt,
            "Reference `{name}`:\n---\n{}\n---\n",
            content.trim()
        );
    }

    let (diff, truncated) = clamp_diff(cx.diff);
    prompt.push_str(
        "The change, exactly as captured by the engine (this is the ground truth — the \
         implementer's own summary is not shown to you on purpose):\n\n```diff\n",
    );
    prompt.push_str(diff);
    prompt.push_str("\n```\n");
    if truncated {
        let _ = writeln!(
            prompt,
            "\n(The diff was truncated to the last {} KB. Judge what you can see, and say so if \
             the truncation prevents a confident verdict.)",
            MAX_DIFF_BYTES / 1024
        );
    }
    prompt.push_str(
        "\nCheck at least: does it satisfy every acceptance criterion; does it do anything the \
         task did not ask for; does it break existing behavior; are there obvious defects, \
         missing error handling, or untested edge cases.\n\n\
         End your reply with a single fenced JSON block, and nothing after it:\n\n\
         ```json\n\
         {\"pass\": true, \"reasons\": [\"why you reached this verdict\"], \
         \"required_changes\": [\"what must change before this can pass\"]}\n\
         ```\n\n\
         `required_changes` must be empty when you pass, and must be actionable when you fail — \
         it is sent verbatim to the agent that will fix the code.\n",
    );
    prompt
}

fn clamp_diff(diff: &str) -> (&str, bool) {
    if diff.len() <= MAX_DIFF_BYTES {
        return (diff, false);
    }
    let start = diff.len() - MAX_DIFF_BYTES;
    let start = (start..diff.len())
        .find(|i| diff.is_char_boundary(*i))
        .unwrap_or(diff.len());
    (&diff[start..], true)
}

fn add_cost(current: Option<f64>, extra: Option<f64>) -> Option<f64> {
    match (current, extra) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    }
}

/// The verdict is the LAST fenced block that parses as a verdict object:
/// models routinely show an example or a draft before their real answer, and
/// §11.2 specifies the last block.
pub fn parse_verdict(text: &str) -> Option<Verdict> {
    fenced_blocks(text)
        .into_iter()
        .rev()
        .find_map(|block| verdict_from_json(&block))
        // A reviewer that answered with bare JSON and no fence is answering
        // in substance; accept it rather than burning a re-ask on formatting.
        .or_else(|| bare_json_object(text).and_then(|b| verdict_from_json(&b)))
}

fn verdict_from_json(candidate: &str) -> Option<Verdict> {
    let value: Value = serde_json::from_str(candidate.trim()).ok()?;
    // `pass` is mandatory: without it there is no verdict, only prose.
    let pass = value.get("pass")?.as_bool()?;
    Some(Verdict {
        pass,
        reasons: string_list(value.get("reasons")),
        required_changes: string_list(value.get("required_changes")),
    })
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| match i {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .filter(|s| !s.trim().is_empty())
            .collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Contents of every ``` fenced block, language tag stripped.
fn fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            match current.take() {
                Some(block) => blocks.push(block),
                None => current = Some(String::new()),
            }
            continue;
        }
        if let Some(block) = current.as_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    // An unterminated final fence still carries an answer.
    if let Some(block) = current {
        blocks.push(block);
    }
    blocks
}

/// Outermost `{...}` span, for replies that skipped the fence entirely.
fn bare_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| text[start..=end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{TaskId, TaskKind};

    fn task() -> Task {
        Task {
            id: TaskId::from("t1"),
            kind: TaskKind::Implement,
            title: "Implement cursor encoding".to_owned(),
            body: "Implement opaque cursor encode/decode.".to_owned(),
            depends_on: Vec::new(),
            acceptance: vec!["Cursors round-trip".to_owned()],
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::new(),
        }
    }

    #[test]
    fn parses_a_fenced_verdict() {
        let text = "Looks reasonable.\n\n```json\n{\"pass\": true, \"reasons\": [\"meets the \
                    criteria\"], \"required_changes\": []}\n```\n";
        let verdict = parse_verdict(text).expect("verdict");
        assert!(verdict.pass);
        assert_eq!(verdict.reasons, ["meets the criteria"]);
        assert!(verdict.required_changes.is_empty());
    }

    #[test]
    fn the_last_block_wins_over_an_example() {
        let text = "I will answer in this shape:\n```json\n{\"pass\": true, \"reasons\": \
                    [\"example\"]}\n```\nAfter reading the diff:\n```json\n{\"pass\": false, \
                    \"reasons\": [\"no tests\"], \"required_changes\": [\"add a round-trip \
                    test\"]}\n```\n";
        let verdict = parse_verdict(text).expect("verdict");
        assert!(!verdict.pass, "the real answer is the last block");
        assert_eq!(verdict.required_changes, ["add a round-trip test"]);
    }

    #[test]
    fn tolerates_plain_fences_bare_json_and_missing_lists() {
        let plain = "```\n{\"pass\": false}\n```";
        let verdict = parse_verdict(plain).expect("plain fence");
        assert!(!verdict.pass);
        assert!(verdict.reasons.is_empty());

        let bare = "Verdict: {\"pass\": true, \"reasons\": \"single string\"}";
        let verdict = parse_verdict(bare).expect("bare json");
        assert!(verdict.pass);
        assert_eq!(verdict.reasons, ["single string"]);
    }

    #[test]
    fn rejects_prose_and_shapeless_json() {
        assert!(parse_verdict("This looks good to me, ship it.").is_none());
        assert!(parse_verdict("```json\n{\"verdict\": \"good\"}\n```").is_none());
        assert!(parse_verdict("```json\n{\"pass\": \"yes\"}\n```").is_none());
        assert!(parse_verdict("").is_none());
    }

    #[test]
    fn prompt_shows_the_diff_and_demands_the_shape() {
        let task = task();
        let artifacts = [("conventions-brief".to_owned(), "Use snake_case.".to_owned())];
        let cx = ReviewCx {
            adapter: &crate::agent::claude::ClaudeCodeAdapter,
            profile: profile_for("claude-code", "claude-opus-5", "review"),
            task: &task,
            diff: "+++ b/src/api.rs\n+fn encode() {}\n",
            artifacts: &artifacts,
            workspace: Path::new("."),
            run_dir: Path::new("."),
            stem: "00-t1".to_owned(),
            timeout: Duration::from_secs(60),
        };
        let prompt = materialize_prompt(&cx);
        assert!(prompt.contains("READ-ONLY"));
        assert!(prompt.contains("find reasons this change should NOT be accepted"));
        assert!(prompt.contains("Cursors round-trip"), "acceptance included");
        assert!(prompt.contains("+fn encode() {}"), "diff included");
        assert!(prompt.contains("Use snake_case."), "artifacts included");
        assert!(prompt.contains("```json"), "verdict shape demanded");
        assert!(
            !prompt.contains("Implement opaque cursor encode/decode.\n\nThe change"),
            "task body present but not conflated with the diff section"
        );
    }

    #[test]
    fn oversized_diffs_are_clamped_with_a_notice() {
        let task = task();
        let huge = format!("+++ b/x\n{}", "+line of change\n".repeat(8000));
        assert!(huge.len() > MAX_DIFF_BYTES);
        let cx = ReviewCx {
            adapter: &crate::agent::claude::ClaudeCodeAdapter,
            profile: profile_for("claude-code", "claude-opus-5", "review"),
            task: &task,
            diff: &huge,
            artifacts: &[],
            workspace: Path::new("."),
            run_dir: Path::new("."),
            stem: "00-t1".to_owned(),
            timeout: Duration::from_secs(60),
        };
        let prompt = materialize_prompt(&cx);
        assert!(prompt.contains("truncated"), "truncation disclosed");
        assert!(prompt.len() < huge.len(), "prompt actually smaller");
    }

    #[test]
    fn reviewer_profiles_are_read_only() {
        let profile = profile_for("claude-code", "claude-opus-5", "review-frontier");
        assert_eq!(profile.permissions, PermissionMode::ReadOnly);
        let settings = crate::agent::claude::permission_settings(&profile, &["cargo test".into()]);
        let allow = settings["permissions"]["allow"].to_string();
        assert!(!allow.contains("Edit"), "reviewers never edit: {allow}");
        assert!(
            !allow.contains("Bash"),
            "reviewers run nothing, not even gates: {allow}"
        );
    }
}
