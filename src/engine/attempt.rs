//! Extended notes: `docs/internals/engine/attempt.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::agent::{AgentAdapter, proc};
use crate::error::UpstrokeError;
use crate::events::{self, Feedback};
use crate::gates::{self, ShellGate};
use crate::ir::{Outcome, OutcomeStatus, Task, WorkerProfile};
use crate::ladder::{AttemptFailure, FailureKind};
use crate::review;
use crate::rundir::RunPaths;
use crate::runner::invocation::{AttemptRole, InvocationId};
use crate::runner::{AgentId, Runner};
use crate::topology::events::AttemptNumber;
use crate::topology::registry::TaskKey;
use crate::util;
use crate::workspace::Workspace;

#[cfg(test)]
use super::options::AfterCandidateCapture;

const MAX_FEEDBACK_ENTRIES: usize = 6;

pub(super) const QUESTION_MARKER: &str = "UPSTROKE-QUESTION:";

pub(super) fn pool_option(pool: &str) -> Option<String> {
    (!pool.is_empty()).then(|| pool.to_owned())
}

pub(super) struct AttemptCx<'a> {
    pub(super) task: &'a Task,
    pub(super) profile: WorkerProfile,
    pub(super) adapter: &'a dyn AgentAdapter,
    pub(super) runner: &'a dyn Runner,
    pub(super) task_index: u32,
    pub(super) attempt: u32,
    pub(super) stem: String,
    pub(super) paths: &'a RunPaths,
    pub(super) gates: &'a [ShellGate],
    pub(super) gate_cmds: &'a [String],
    pub(super) reviewers: Vec<Reviewer<'a>>,
    pub(super) timeout: Duration,
    pub(super) review_pass_timeout: Duration,
    pub(super) retry: Option<RetryBrief>,
    pub(super) decisions: Vec<String>,
    #[cfg(test)]
    pub(super) after_candidate_capture: Option<AfterCandidateCapture>,
}

impl AttemptCx<'_> {
    fn invocation(&self, role: AttemptRole) -> InvocationId {
        InvocationId::legacy_attempt(
            TaskKey(self.task_index),
            AttemptNumber(self.attempt),
            role,
            0,
        )
    }
}

pub(super) struct RetryBrief {
    pub(super) resumed: bool,
    pub(super) feedback: Vec<Feedback>,
}

#[derive(Clone)]
pub(super) struct Reviewer<'a> {
    pub(super) adapter: &'a dyn AgentAdapter,
    pub(super) profile: WorkerProfile,
    pub(super) lens: review::Lens,
    pub(super) preflight_cli_version: Option<String>,
}

pub(super) struct AttemptResult {
    pub(super) outcome: Outcome,
    pub(super) failure: Option<AttemptFailure>,
    pub(super) candidate_branch_ref: String,
    pub(super) candidate_parent: String,
    pub(super) candidate_tree: String,
    pub(super) reviews: Vec<events::ReviewRecord>,
}

pub(super) fn run_attempt(
    cx: &AttemptCx<'_>,
    workspace: &Workspace,
    resume_session: Option<String>,
) -> Result<AttemptResult, UpstrokeError> {
    let worker_workspace = workspace.root().to_path_buf();
    let command = super::assembly::WorkerAssembly {
        adapter: cx.adapter,
        profile: &cx.profile,
        task: super::assembly::WorkerSubject::of(cx.task),
        gate_cmds: cx.gate_cmds,
        paths: cx.paths,
        stem: &cx.stem,
        attempt: cx.attempt,
        retry: cx.retry.as_ref(),
        workspace: &worker_workspace,
        resume_session,
    }
    .command()?;
    let output = cx.runner.run(&crate::runner::worker_request(
        command,
        worker_workspace.clone(),
        AgentId::new(cx.adapter.id()),
        cx.timeout,
        cx.invocation(AttemptRole::Worker),
    ))?;

    let transcripts = cx.paths.transcripts();
    let transcript_path = transcripts.join(format!("{}-{}.json", cx.stem, cx.attempt));
    util::write_text(&transcript_path, &output.stdout)?;
    if !output.stderr.trim().is_empty() {
        util::write_text(
            &transcripts.join(format!("{}-{}.stderr.log", cx.stem, cx.attempt)),
            &output.stderr,
        )?;
    }

    let mut outcome: Outcome = cx.adapter.parse(&output)?;
    let candidate = workspace.capture_candidate()?;
    #[cfg(test)]
    if let Some(after_capture) = cx.after_candidate_capture {
        after_capture(workspace, &candidate)?;
    }
    outcome.diff = candidate.diff;
    outcome.transcript_path = transcript_path;

    let mut failure = evaluate_outcome(&outcome, &output);
    if failure.is_none() {
        failure =
            super::classify::diff_failure(&outcome.diff, cx.task.kind, !cx.reviewers.is_empty());
    }
    if failure.is_none() {
        if let Some(problem) =
            <LegacyReviewInputPolicy as super::topology::attempt::ReviewInputPolicy>::problem(
                &LegacyReviewInputPolicy,
                workspace.root(),
                &candidate.tree_oid,
            )?
        {
            failure = Some(super::classify::review_input_failure(problem));
        }
    }
    if failure.is_none() && !cx.gates.is_empty() {
        let gate_workspace = workspace.gate_snapshot_for_candidate_in_store(
            &candidate.parent_oid,
            &candidate.tree_oid,
            &cx.paths.gate_worktrees(),
        )?;
        if let Some(gate_failure) = gates::run_all(
            cx.gates,
            cx.runner,
            &|index| cx.invocation(AttemptRole::Gate(index)),
            gate_workspace.workspace(),
            &cx.paths.gates(),
            &cx.stem,
            cx.attempt,
        )? {
            failure = Some(super::classify::gate_failure(&gate_failure));
        }
    }

    let mut reviews = Vec::new();
    if failure.is_none() && !cx.reviewers.is_empty() {
        let artifacts = load_artifacts(
            &cx.paths.artifacts(),
            super::assembly::WorkerSubject::of(cx.task),
        );
        let review_workspace = workspace.gate_snapshot_for_candidate_in_store(
            &candidate.parent_oid,
            &candidate.tree_oid,
            &cx.paths.gate_worktrees(),
        )?;
        for (pass, reviewer) in cx.reviewers.iter().enumerate() {
            let pass = u32::try_from(pass).unwrap_or(u32::MAX);
            let review = super::topology::attempt::ReviewPasses::run(
                &LegacyReviewPasses,
                &review::ReviewCx {
                    adapter: reviewer.adapter,
                    profile: reviewer.profile.clone(),
                    lens: reviewer.lens,
                    task: review::ReviewSubject::of(cx.task),
                    diff: &outcome.diff,
                    artifacts: &artifacts,
                    decisions: &cx.decisions,
                    workspace: review_workspace.workspace().root(),
                    settings_dir: &cx.paths.settings(),
                    reviews_dir: &cx.paths.reviews(),
                    stem: format!("{}-{}", cx.stem, cx.attempt),
                    timeout: cx.review_pass_timeout,
                },
                cx.runner,
                &review::ReviewInvocations {
                    pass: cx.invocation(AttemptRole::ReviewPass(pass)),
                    reask: cx.invocation(AttemptRole::ReviewReask(pass)),
                },
            )?;
            let cost_usd = review.cost_usd;
            let unavailable = matches!(review.result, review::ReviewResult::Unavailable { .. });
            failure = review_failure(review.result);
            reviews.push(
                super::classify::ReviewPassFacts {
                    pass: reviewer.lens.name(),
                    agent: &reviewer.profile.agent,
                    model: &reviewer.profile.model,
                    adapter: reviewer.adapter.id(),
                    preflight_cli_version: reviewer.preflight_cli_version.clone(),
                    effort: reviewer.profile.effort,
                    pool: pool_option(&reviewer.profile.pool),
                    cost_usd,
                    unavailable,
                    failed: failure.is_some(),
                }
                .record(),
            );
            if failure.is_some() {
                break;
            }
        }
    }

    Ok(AttemptResult {
        outcome,
        failure,
        candidate_branch_ref: candidate.branch_ref,
        candidate_parent: candidate.parent_oid,
        candidate_tree: candidate.tree_oid,
        reviews,
    })
}

pub(super) struct LegacyReviewPasses;

pub(super) struct LegacyReviewInputPolicy;

impl super::topology::attempt::ReviewInputPolicy for LegacyReviewInputPolicy {
    fn problem(
        &self,
        worktree: &std::path::Path,
        tree: &str,
    ) -> Result<Option<String>, UpstrokeError> {
        crate::workspace::Workspace::open(worktree)?.review_input_problem_for_tree(tree)
    }
}

impl super::topology::attempt::ReviewPasses for LegacyReviewPasses {
    fn run(
        &self,
        cx: &review::ReviewCx<'_>,
        runner: &dyn crate::runner::Runner,
        invocations: &review::ReviewInvocations,
    ) -> Result<review::ReviewOutcome, UpstrokeError> {
        review::run_review(cx, runner, invocations)
    }
}

pub(super) fn review_failure(result: review::ReviewResult) -> Option<AttemptFailure> {
    let verdict = match result {
        review::ReviewResult::Unavailable { status, detail } => {
            let kind = match status {
                OutcomeStatus::RateLimited => FailureKind::RateLimited,
                OutcomeStatus::Timeout => FailureKind::Timeout,
                _ => FailureKind::ReviewUnavailable,
            };
            return Some(
                AttemptFailure::new(
                    kind,
                    format!("reviewer unavailable: {}", util::head(&detail, 400)),
                )
                .from_reviewer(),
            );
        }
        review::ReviewResult::Judged(verdict) => verdict,
    };

    if verdict.needs_human {
        let reasons = if verdict.reasons.is_empty() {
            "the reviewer asked for a human decision but gave no reason".to_owned()
        } else {
            verdict.reasons.join("; ")
        };
        return Some(
            AttemptFailure::new(
                FailureKind::NeedsHuman,
                format!("reviewer asked for a human decision: {reasons}"),
            )
            .from_reviewer(),
        );
    }

    let contradictory = verdict.pass && !verdict.required_changes.is_empty();
    if verdict.pass && !contradictory {
        return None;
    }
    let summary = if contradictory {
        format!(
            "reviewer passed the change but still required: {}",
            verdict.required_changes.join("; ")
        )
    } else if verdict.reasons.is_empty() {
        "no reasons given".to_owned()
    } else {
        verdict.reasons.join("; ")
    };
    let feedback = if verdict.required_changes.is_empty() {
        summary.clone()
    } else {
        verdict
            .required_changes
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some(
        AttemptFailure::new(
            FailureKind::ReviewFailed,
            format!("review failed: {}", util::head(&summary, 400)),
        )
        .with_feedback(feedback),
    )
}

pub(super) fn load_artifacts(
    artifacts_dir: &Path,
    task: super::assembly::WorkerSubject<'_>,
) -> Vec<(String, String)> {
    let mut wanted: Vec<String> = vec![CONVENTIONS_BRIEF.to_owned()];
    wanted.extend(task.artifacts_in.iter().map(|id| id.as_str().to_owned()));
    let produced: Vec<&str> = task.artifacts_out.iter().map(|id| id.as_str()).collect();
    let mut seen: Vec<String> = Vec::new();
    wanted
        .into_iter()
        .filter(|id| !produced.contains(&id.as_str()))
        .filter(|id| {
            let fresh = !seen.contains(id);
            if fresh {
                seen.push(id.clone());
            }
            fresh
        })
        .filter_map(|id| {
            let content = fs::read_to_string(artifact_path(artifacts_dir, &id)).ok()?;
            (!content.trim().is_empty()).then_some((id, content))
        })
        .collect()
}

const CONVENTIONS_BRIEF: &str = "conventions-brief";

pub(super) fn evaluate_outcome(
    outcome: &Outcome,
    output: &proc::ProcessOutput,
) -> Option<AttemptFailure> {
    let detail = || {
        outcome
            .detail
            .clone()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| {
                let stderr = util::tail(&output.stderr, 400);
                if stderr.is_empty() {
                    "no diagnostic output; see the transcript".to_owned()
                } else {
                    stderr
                }
            })
    };
    match outcome.status {
        OutcomeStatus::Completed => {
            if let Some(question) = worker_question(outcome.detail.as_deref()) {
                return Some(AttemptFailure::new(FailureKind::NeedsHuman, question));
            }
            if !outcome.diff.trim().is_empty() {
                return None;
            }
            Some(
                AttemptFailure::new(
                    FailureKind::EmptyDiff,
                    "agent reported success but the diff is empty — \"done\" claims require \
                     changed code",
                )
                .with_feedback(
                    "You reported the task complete, but the repository is unchanged. Either make \
                     the change the task asks for, or explain what blocks it using the \
                     UPSTROKE-QUESTION marker."
                        .to_owned(),
                ),
            )
        }
        OutcomeStatus::AgentError => Some(
            AttemptFailure::new(
                FailureKind::AgentError,
                format!("agent error (exit {:?}): {}", output.code, detail()),
            )
            .with_feedback(detail()),
        ),
        OutcomeStatus::Timeout => Some(
            AttemptFailure::new(
                FailureKind::Timeout,
                "attempt hit the wall-clock timeout",
            )
            .with_feedback(format!(
                "Your previous attempt was cut off at its time limit. Work in smaller steps and \
                 finish the highest-value change first. Its last output was:\n{}",
                util::tail(&outcome.detail.clone().unwrap_or_default(), 2000)
            )),
        ),
        OutcomeStatus::RateLimited => Some(AttemptFailure::new(
            FailureKind::RateLimited,
            format!("pool rate-limited: {}", detail()),
        )),
    }
}

pub(super) fn worker_question(detail: Option<&str>) -> Option<String> {
    let detail = detail?;
    let start = detail.rfind(QUESTION_MARKER)?;
    let text = detail[start + QUESTION_MARKER.len()..].trim();
    (!text.is_empty()).then(|| util::head(text, 2000))
}

pub(super) fn materialize_prompt(
    task: super::assembly::WorkerSubject<'_>,
    gate_cmds: &[String],
    artifacts_dir: &Path,
    retry: Option<&RetryBrief>,
) -> String {
    if let Some(retry) = retry {
        if retry.resumed {
            let mut prompt = String::new();
            prompt.push_str(
                "Your previous attempt did not pass verification. Fix it in this same session — the \
                 task and its rules have not changed.\n\n",
            );
            prompt.push_str(&feedback_section(&retry.feedback, false));
            prompt.push_str(
                "\nMake the smallest change that resolves the above, then stop and summarize.\n",
            );
            return prompt;
        }
    }

    let mut prompt = String::new();
    prompt.push_str(
        "You are executing one task from a frozen plan, conducted by the upstroke engine.\n\n",
    );
    let _ = writeln!(prompt, "# Task: {}\n", task.title);
    if !task.body.is_empty() {
        prompt.push_str(task.body);
        prompt.push_str("\n\n");
    }
    if !task.acceptance.is_empty() {
        prompt.push_str("Acceptance criteria (all must hold when you finish):\n");
        for item in task.acceptance {
            let _ = writeln!(prompt, "- {item}");
        }
        prompt.push('\n');
    }
    for id in task.artifacts_in {
        let path = artifact_path(artifacts_dir, id.as_str());
        match fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                let _ = writeln!(
                    prompt,
                    "Input artifact `{id}` (produced by an earlier task):\n---\n{}\n---\n",
                    content.trim()
                );
            }
            _ => {
                let _ = writeln!(
                    prompt,
                    "Note: this task expected input artifact `{id}`, but the earlier task did \
                     not leave one. Work from the repository as it stands.\n"
                );
            }
        }
    }
    for id in task.artifacts_out {
        let _ = writeln!(
            prompt,
            "Before you finish, write artifact `{id}` — the notes later tasks depend on — to:\n\
             {}\n",
            artifact_path(artifacts_dir, id.as_str()).display()
        );
    }
    if !gate_cmds.is_empty() {
        prompt.push_str(
            "Verification gates run after you finish. You may run EXACTLY these commands \
             yourself to check your work (any other shell command is denied):\n",
        );
        for cmd in gate_cmds {
            let _ = writeln!(prompt, "- {cmd}");
        }
        prompt.push('\n');
    }
    if let Some(retry) = retry {
        prompt.push_str(&feedback_section(&retry.feedback, true));
    }
    prompt.push_str(
        "Rules:\n\
         - Complete ONLY this task; leave work that belongs to other tasks alone.\n\
         - Edit files inside this repository only.\n\
         - NEVER run git commit, branch, merge, push, or reset — the engine owns git.\n\
         - When the acceptance criteria hold, stop and summarize what changed.\n\
         - If a decision genuinely is not yours to make — the task is ambiguous in a way that \
           changes what \"correct\" means, or it turns on a product or policy call you cannot \
           settle from this repository — stop and end your message with a line beginning \
           `UPSTROKE-QUESTION:` followed by the decision a person has to make. That pauses this \
           task and asks them. Do not use it for uncertainty you could resolve by reading the \
           code.\n",
    );
    prompt
}

fn feedback_section(feedback: &[Feedback], all: bool) -> String {
    let entries: Vec<&Feedback> = if all {
        feedback
            .iter()
            .skip(feedback.len().saturating_sub(MAX_FEEDBACK_ENTRIES))
            .collect()
    } else {
        feedback.last().into_iter().collect()
    };
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if all && entries.len() > 1 {
        out.push_str(
            "Earlier attempts at this task failed. You are a fresh, stronger worker on the same \
             task — do not repeat these:\n\n",
        );
    }
    let last = entries.len() - 1;
    for (position, entry) in entries.iter().enumerate() {
        if entry.human {
            let fence = util::fence_for(entry.detail.as_deref().unwrap_or_default());
            let _ = writeln!(
                out,
                "The operator answered the question that paused this task. This is an \
                 instruction from a person, and it takes precedence over your earlier \
                 assumptions:\n{fence}\n{}\n{fence}\n",
                entry.detail.as_deref().unwrap_or_default().trim()
            );
            continue;
        }
        let where_ = if entry.tier.is_empty() {
            String::new()
        } else {
            format!(" on the {} rung", entry.tier)
        };
        let _ = writeln!(
            out,
            "Attempt {}{where_} failed: {}",
            entry.attempt, entry.summary
        );
        if position == last {
            if let Some(detail) = &entry.detail {
                if !detail.trim().is_empty() {
                    let fence = util::fence_for(detail);
                    let _ = writeln!(out, "{fence}\n{}\n{fence}", detail.trim());
                }
            }
        }
        out.push('\n');
    }
    out
}

pub(super) fn artifact_path(artifacts_dir: &Path, id: &str) -> PathBuf {
    artifacts_dir.join(format!("{}.md", util::filename_component(id)))
}
