//! OpenAI Codex CLI adapter (DESIGN.md §16) — a second pool, and a reviewer
//! from a different model family that costs nothing on the first one.
//!
//! §13's capacity engine is built around several subscriptions with independent
//! windows, and until this adapter there was one that tactus could actually
//! drive on its own: Copilot reaches OpenAI models, but through GitHub's
//! harness and GitHub's billing.
//!
//! **Implementing works where the sandbox is real, and only there.** This
//! CLI's sandbox is an external helper, present on Linux and absent on
//! Windows — so on Windows `exec` silently degrades to read-only and
//! [`refuse_edit_profile`] turns an implementer away at build time rather than
//! letting it spend attempts on empty diffs. On Linux the same flags write
//! inside the workspace and are blocked outside it, which is what §20 asks
//! for, so the implementer path is open. The evidence for both lives on that
//! function.
//!
//! The judge's seat works everywhere: `read-only` is enforced on every
//! platform, the family is genuinely different from Anthropic's (§11.3), and a
//! review that spends nothing on the Claude window is worth having on its own —
//! measured end to end on run `01KZRN48A4ZK3AEDST3RJ8HMA4`, where Sonnet
//! implemented and this adapter judged.
//!
//! **Two command shapes, not one with a flag swapped.** `codex exec` and
//! `codex exec resume` accept *different* flag sets: resume takes no `-s`, no
//! `-C`, no `--profile`. That is not a gap to work around. The sandbox is a
//! property of the session, fixed when it is created and inherited by every
//! resumed turn — which is exactly tactus's model, where a same-rung retry has
//! the same profile by definition (§11.4). Observed 2026-08-11 against
//! codex-cli 0.147.0: a resume with no sandbox flag ran under the policy its
//! session recorded.
//!
//! **The prompt goes on stdin, as `-`.** Windows caps a command line at ~8,191
//! characters and a review prompt carries up to
//! [`crate::review::MAX_DIFF_BYTES`] of diff, so argv was never an option. The
//! CLI also *waits* on stdin when it expects input ("Reading additional input
//! from stdin…"), so the payload must always be written and the pipe always
//! closed — [`super::proc`] does both, and an adapter that returned an empty
//! payload here would hang every attempt until the wall-clock timeout.
//!
//! **stdout is JSONL, stderr is tracing.** `--json` emits one event per line —
//! `thread.started`, `turn.started`, `item.started`, `item.completed`,
//! `turn.completed` — while stderr carries `ERROR codex_api::…` log lines.
//! Only stdout is parsed; stderr survives in the transcript for whoever is
//! debugging.
//!
//! **What this route reports, and what it does not.** A session id worth
//! resuming (`thread_id`), the final message, and token usage — but no
//! dollars. Tokens are recorded on the attempt and `cost_reporting` stays
//! false, so the ledger keeps saying `?` for these routes rather than
//! inventing a price. Pricing them here would mean a rate table inside a
//! published binary, going stale silently, to produce a figure that is
//! notional twice over on subscription auth where the marginal dollar is zero.
//! §13 already has the words: an estimate that flatters is worse than none.
//!
//! **Two of this CLI's own features are deliberately unused.**
//!
//! `codex review` runs a code review non-interactively, and adopting it would
//! swap the standard. §11.3's second opinion is *the same standard, a
//! different judge*: tactus's review prompt carries the task's acceptance
//! criteria, the anti-sycophancy framing, the `DATA UNDER REVIEW` fencing and
//! the operator's decisions (§12). A verdict from OpenAI's own rubric applied
//! to a bare diff is not comparable with one from the Claude reviewer, and a
//! cross-family disagreement between them would be uninterpretable — the model
//! disagreeing, or the rubric? Reviews therefore run through plain `exec` with
//! `-s read-only`, like every other reviewer. This adapter cannot even tell it
//! is reviewing; it sees [`PermissionMode::ReadOnly`] and nothing else, and
//! that is the right amount to know.
//!
//! `--output-schema` would force the model's final message into a JSON shape,
//! which is tempting for §7 verdicts — but it would make a third copy of the
//! verdict shape (prompt, parser, schema) that can drift, hold two reviewers to
//! two different contracts, and push the reviewer's prose into escaped strings
//! where humans read it. The existing re-ask-on-unparseable path already covers
//! the failure it would prevent, and nothing has yet measured that failure
//! happening. Revisit if real runs show it firing more than rarely.
//!
//! **Never passed:** `--dangerously-bypass-approvals-and-sandbox`,
//! `--dangerously-bypass-hook-trust`, `-s danger-full-access`. §20 grants the
//! narrowest surface that lets the work happen, and there is no task for which
//! the answer is "turn the sandbox off". `--ephemeral` is also never passed —
//! it would discard the session that §11.4's same-rung retry resumes.
//!
//! Surface captured from `codex --help`, `codex exec --help` and
//! `codex exec resume --help` at 0.147.0, and verified by running it.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};

use super::bin::{self, Invocation};
use super::proc::{self, ProcessOutput};
use super::{AdapterSource, AgentAdapter, AuthState, Caps, Discovery, TaskRun, looks_rate_limited};
use crate::capacity::PoolKind;
use crate::error::TactusError;
use crate::ir::{Outcome, OutcomeStatus, PermissionMode, Usage, WorkerProfile};
use crate::util;

pub const ADAPTER_ID: &str = "codex";

/// Budget for one probe call. Generous for the same reason Copilot's is: §19
/// makes a probe failure a refusal to START, so a slow machine that times out
/// here loses a whole run rather than one attempt. Paid once per run.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// Flags `exec` must still advertise, checked at pre-flight.
///
/// Every one is load-bearing rather than decorative: without `--json` there is
/// no session id and no usage, and without `--sandbox` a reviewer could edit
/// the code it is judging. A CLI that has dropped one of these must refuse the
/// run up front, not fail attempts once it is already spending (§19).
const REQUIRED_EXEC_FLAGS: [&str; 3] = ["--json", "--sandbox", "--model"];

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }

    fn probe(&self) -> Result<Caps, TactusError> {
        let invocation = locate()?;
        let out = proc::run_with_timeout(
            invocation.command(&["--version".to_owned()]),
            "",
            PROBE_TIMEOUT,
        )?;
        if out.timed_out {
            return Err(TactusError::Agent {
                message: format!("`{}` --version timed out", invocation.display()),
            });
        }
        if out.code != Some(0) {
            return Err(TactusError::Agent {
                message: format!(
                    "`{}` --version exited with {:?}: {}",
                    invocation.display(),
                    out.code,
                    out.stderr.trim()
                ),
            });
        }
        let version = bin::extract_version(&out.stdout);

        // `exec --help`, not `--help`: every flag this adapter passes lives on
        // the subcommand, and the top-level help does not list them.
        let help = proc::run_with_timeout(
            invocation.command(&["exec".to_owned(), "--help".to_owned()]),
            "",
            PROBE_TIMEOUT,
        )?;
        let help_text = format!("{}{}", help.stdout, help.stderr);
        let readable = help.code == Some(0) && !help_text.trim().is_empty();
        if readable {
            let missing: Vec<&str> = REQUIRED_EXEC_FLAGS
                .into_iter()
                .filter(|flag| !help_text.contains(flag))
                .collect();
            if !missing.is_empty() {
                return Err(TactusError::Agent {
                    message: format!(
                        "codex {version} does not advertise required `exec` flag(s): {}. This \
                         adapter pins known-good behavior per version — upgrade tactus or pin an \
                         older codex.",
                        missing.join(", ")
                    ),
                });
            }
        }

        Ok(Caps {
            version,
            // Asked for and parsed, unlike Copilot's route where the flag's
            // existence would promise an envelope no caller reads.
            json_output: true,
            // `codex exec resume <id>` — proven to round-trip: the resumed turn
            // returned the same `thread_id` and recalled the prior exchange.
            session_resume: readable && help_text.contains("resume"),
            // Tokens, not dollars. See the module header — this is a decision
            // about what tactus is willing to claim, not a missing feature.
            cost_reporting: false,
            read_only_mode: readable && help_text.contains("--sandbox"),
            // The CLI has `mcp-server` and `app-server`, neither of which is
            // ACP, and this adapter spawns a process per attempt either way.
            acp: false,
            // No enumeration subcommand, so the roster is the shipped catalog.
            model_list: false,
        })
    }

    fn build(&self, run: &TaskRun) -> Result<Command, TactusError> {
        if let Some(refusal) = edit_refusal(&run.profile) {
            return Err(refusal);
        }
        let invocation = locate()?;
        let mut cmd = invocation.command(&build_args(run));
        // The working root comes from the process, not from `-C`: `exec resume`
        // has no `-C`, and one mechanism that works for both shapes beats two
        // that have to agree.
        cmd.current_dir(&run.workspace);
        Ok(cmd)
    }

    fn parse(&self, out: &ProcessOutput) -> Result<Outcome, TactusError> {
        Ok(parse_output(out))
    }

    /// The one thing this CLI does better than either incumbent: it answers
    /// "am I signed in?" without spending anything.
    ///
    /// `codex login status` is non-interactive, exits 0 either way, and prints
    /// `Logged in using ChatGPT` or `Not logged in` (observed 2026-08-11).
    /// Copilot's adapter has to report [`AuthState::Unknown`] because GitHub
    /// documents no such query; here the honest answer is a real one, so
    /// `tactus connect` writes a pool an operator can trust rather than a
    /// shrug.
    fn discover(&self, _caps: &Caps) -> Result<Discovery, TactusError> {
        let invocation = locate()?;
        let out = proc::run_with_timeout(
            invocation.command(&["login".to_owned(), "status".to_owned()]),
            "",
            PROBE_TIMEOUT,
        )?;
        Ok(parse_login_status(&out).with_note(
            "no model listing is offered, so the roster for this agent is the catalog shipped \
             with tactus, not something confirmed here",
        ))
    }

    /// Nothing to reference — permissions are argv here, as they are for
    /// Copilot — but the audit file is still written, because §15 calls
    /// `settings/<task>-<attempt>.json` the per-attempt permission surface and
    /// a trail that exists for one agent and silently not another is worse than
    /// none.
    fn materialize_permissions(
        &self,
        profile: &WorkerProfile,
        _gate_cmds: &[String],
        dir: &std::path::Path,
        stem: &str,
    ) -> Result<Option<PathBuf>, TactusError> {
        let path = dir.join(format!("{stem}.json"));
        util::write_json(
            &path,
            &json!({
                "agent": ADAPTER_ID,
                "profile": profile.name,
                "permissions": profile.permissions,
                "note": "recorded for audit only; codex takes its sandbox as an argv flag",
                "sandbox": sandbox_mode(profile),
            }),
        )?;
        Ok(None)
    }
}

/// The sandbox this profile runs under (§20).
///
/// Two modes and no third: `danger-full-access` exists on this CLI and is
/// never used. A reviewer may read and nothing else, because a reviewer that
/// edits the code it is judging has invalidated its own verdict.
fn sandbox_mode(profile: &WorkerProfile) -> &'static str {
    match profile.permissions {
        PermissionMode::Edit => "workspace-write",
        PermissionMode::ReadOnly => "read-only",
    }
}

/// Why an implementer is refused **on Windows only**, and what was measured.
///
/// This CLI's sandbox is an external helper. `codex doctor` reports it as
/// `linux helper: <path>` where one exists and `none` on Windows — where there
/// is therefore nothing to enforce a boundary with. The consequence is a rule
/// the binary states itself:
///
/// > `approval_policy = "never"` cannot be used because requirements do not
/// > allow `sandbox_mode = "danger-full-access"`; Codex would fall back to
/// > read-only permissions with approvals disabled.
///
/// `exec` is non-interactive, so it forces `never`. With no enforceable
/// sandbox that degrades to read-only, and `--sandbox workspace-write` is
/// *accepted and then ignored*: exit 0, no warning, no diff. The silence is the
/// dangerous part — run `01KZRMHA28M5CM88VAXP613X9P` spent both attempts on
/// empty diffs and parked asking for write access it had been granted.
/// `-c approval_policy="on-request"` and `-c permission_profile="…"` were both
/// tried; `exec` wins.
///
/// The only mode that writes there is `--approve-for-me`, which routes
/// approvals through an automatic reviewer rather than a human — and it is not
/// a sandbox. Asked to write outside the repository it did so, and
/// `sandbox_workspace_write.writable_roots` did not constrain it. §20 grants
/// permission by mechanism, not by asking an LLM nicely, and §14's rollback is
/// `git clean -fd` *inside* the workspace: anything written outside it survives
/// a failed attempt, which is the one thing the design rules out.
///
/// **On Linux the sandbox is real and none of this applies.** Same CLI, same
/// flags, helper present: `--sandbox workspace-write` writes inside the
/// workspace and is *blocked* outside it — both measured. So the refusal is
/// scoped to the platform that cannot enforce it, and the implementer path is
/// open everywhere else.
///
/// One trap worth recording for whoever containerises this: Docker's default
/// seccomp profile blocks the syscalls the sandbox needs to initialise, and the
/// failure is a *different* message ("the workspace sandbox failed to
/// initialize") with the same empty-diff result. Granting
/// `--security-opt seccomp=unconfined --cap-add SYS_ADMIN` let it initialise;
/// which of the two is strictly required was not isolated.
/// The platform gate, kept out of [`AgentAdapter::build`] so it is testable on
/// a machine with no codex installed — the same reason [`build_args`] is its
/// own function.
fn edit_refusal(profile: &WorkerProfile) -> Option<TactusError> {
    (cfg!(windows) && profile.permissions == PermissionMode::Edit)
        .then(|| refuse_edit_profile(profile))
}

fn refuse_edit_profile(profile: &WorkerProfile) -> TactusError {
    TactusError::Refused {
        message: format!(
            "codex cannot run `{}` as an implementer on Windows: this CLI's sandbox is an \
             external helper that does not exist here (`codex doctor` reports `linux helper: \
             none`), so `codex exec` degrades to read-only — it accepts `--sandbox \
             workspace-write` and then writes nothing, with no error. Its only writing mode \
             (`--approve-for-me`) auto-approves writes anywhere on the filesystem, including \
             outside the repository, which §14's rollback cannot undo. Run codex under Linux \
             where its sandbox is enforced, or route implementation to another agent and keep \
             codex as a reviewer — its read-only sandbox works everywhere, and its different \
             model family is the point (§11.3).",
            profile.name
        ),
    }
}

/// Argument list, kept separate from binary resolution so it is testable on a
/// machine with no CLI installed.
///
/// Two shapes, because the CLI has two. A fresh attempt sets the sandbox that
/// the session will carry; a resumed one inherits it and would be rejected for
/// passing `-s` at all (observed: exit 2, "unexpected argument '-s' found").
pub fn build_args(run: &TaskRun) -> Vec<String> {
    let mut args = vec!["exec".to_owned()];
    if let Some(session) = &run.resume_session {
        args.push("resume".to_owned());
        args.push(session.clone());
    }
    args.push("--json".to_owned());
    // Passed on both shapes even though a resumed session already knows its
    // model: the recorded command should say what it ran on without a reader
    // having to open the session file, and a future change to the CLI's
    // default must not silently move a resumed retry to another model.
    args.push("--model".to_owned());
    args.push(run.profile.model.clone());
    if run.resume_session.is_none() {
        args.push("--sandbox".to_owned());
        args.push(sandbox_mode(&run.profile).to_owned());
    }
    args.extend(run.profile.extra_args.iter().cloned());
    // `-` is "read the prompt from stdin" and must be last: everything after it
    // would be taken as the prompt's own arguments.
    args.push("-".to_owned());
    args
}

/// Outcome parsing over the JSONL event stream.
///
/// Defensive throughout, like every other adapter here: a line that is not JSON
/// is skipped rather than failing the attempt, and a missing field degrades the
/// status instead of panicking. The engine owns `diff` and `transcript_path`.
fn parse_output(out: &ProcessOutput) -> Outcome {
    let mut outcome = Outcome {
        status: OutcomeStatus::AgentError,
        diff: String::new(),
        detail: None,
        session_id: None,
        usage: None,
        cost_usd: None,
        transcript_path: PathBuf::new(),
        duration: out.duration,
    };

    let mut message: Option<String> = None;
    let mut errors: Vec<String> = Vec::new();
    let mut usage: Option<Usage> = None;

    for line in out.stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(event): Result<Value, _> = serde_json::from_str(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("thread.started") => {
                outcome.session_id = event
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            Some("item.completed") => {
                let item = event.get("item");
                let is_message = item
                    .and_then(|i| i.get("type"))
                    .and_then(Value::as_str)
                    .is_some_and(|t| t == "agent_message");
                if is_message {
                    // Last one wins: the final message is the agent's answer,
                    // and it is the field a reviewer's verdict travels in.
                    message = item
                        .and_then(|i| i.get("text"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
            }
            Some("turn.completed") => {
                // Summed rather than replaced. One invocation emitted exactly
                // one of these, tool call and all (measured), so this is
                // defence against a future version that reports per step —
                // where taking the last would quietly under-count.
                usage = Some(add_usage(usage, event.get("usage")));
            }
            Some("error") => {
                if let Some(text) = event.get("message").and_then(Value::as_str) {
                    errors.push(text.to_owned());
                }
            }
            _ => {}
        }
    }
    outcome.usage = usage;

    if out.timed_out {
        outcome.status = OutcomeStatus::Timeout;
        outcome.detail = Some("attempt exceeded its wall-clock timeout".to_owned());
        return outcome;
    }

    if out.code == Some(0) {
        outcome.status = OutcomeStatus::Completed;
        outcome.detail = message;
        return outcome;
    }

    // Failures only — a successful task *about* rate limiting must never read
    // as the pool being exhausted (see `looks_rate_limited`).
    let joined = errors.join("\n");
    outcome.status = if looks_rate_limited(&joined) || looks_rate_limited(&out.stderr) {
        OutcomeStatus::RateLimited
    } else {
        OutcomeStatus::AgentError
    };
    // The `error` events first: on this route stderr is a tracing log, so the
    // event stream carries the diagnostic a human actually wants. An
    // unauthenticated run exits 101 with 401s here, which is an agent error
    // and not a rate limit — a distinction the ladder acts on.
    outcome.detail = [
        (!joined.is_empty()).then(|| util::tail(&joined, 2000)),
        message,
        (!out.stderr.trim().is_empty()).then(|| util::tail(out.stderr.trim(), 2000)),
    ]
    .into_iter()
    .flatten()
    .next();
    outcome
}

/// Fold one `turn.completed`'s usage into the running total.
///
/// `reasoning_output_tokens` is a *subset* of `output_tokens` on this CLI, not
/// an addition to it, so it is carried across rather than added in — summing
/// both would double-count the thinking.
fn add_usage(total: Option<Usage>, reported: Option<&Value>) -> Usage {
    let mut total = total.unwrap_or_default();
    let Some(reported) = reported else {
        return total;
    };
    let field = |name: &str| reported.get(name).and_then(Value::as_u64);
    let add = |slot: &mut Option<u64>, value: Option<u64>| {
        if let Some(value) = value {
            *slot = Some(slot.unwrap_or(0) + value);
        }
    };
    add(&mut total.input_tokens, field("input_tokens"));
    add(&mut total.output_tokens, field("output_tokens"));
    // Vendor names differ; the concepts line up. `cached_input_tokens` is a
    // read from the cache, `cache_write_input_tokens` is a write into it.
    add(
        &mut total.cache_read_input_tokens,
        field("cached_input_tokens"),
    );
    add(
        &mut total.cache_creation_input_tokens,
        field("cache_write_input_tokens"),
    );
    add(
        &mut total.reasoning_output_tokens,
        field("reasoning_output_tokens"),
    );
    // One `turn.completed` is one turn, so this counts them for free.
    total.num_turns = Some(total.num_turns.unwrap_or(0) + 1);
    total
}

/// Read `codex login status`, as defensively as everything else here.
///
/// Observed forms (0.147.0): `Not logged in`, and `Logged in using ChatGPT`.
/// The negative is checked first because it contains the positive as a
/// substring — matching "logged in" first would call a signed-out account
/// signed in, which is the one error `AuthState` exists to prevent.
fn parse_login_status(out: &ProcessOutput) -> Discovery {
    let mut discovery = Discovery::unknown();
    if out.timed_out {
        return discovery.with_note("`codex login status` timed out; auth state unknown");
    }
    let text = format!("{}{}", out.stdout, out.stderr).to_ascii_lowercase();
    if text.contains("not logged in") || text.contains("not authenticated") {
        discovery.auth = AuthState::NotAuthenticated;
        return discovery.with_note("`codex login status` reports no stored credentials");
    }
    if !text.contains("logged in") {
        return discovery.with_note(format!(
            "`codex login status` said something this adapter does not recognise: {}",
            util::head(text.trim(), 120)
        ));
    }
    discovery.auth = AuthState::Authenticated;
    // §13's two billing shapes. A ChatGPT plan is a rate-limit window; an API
    // key is metered dollars. Anything else is left for the caller's documented
    // default rather than guessed at.
    if text.contains("chatgpt") {
        discovery.shape = Some(PoolKind::SubscriptionWindow);
        discovery = discovery.with_note(
            "signed in through a ChatGPT plan, so this pool is a rate-limit window rather than \
             metered dollars",
        );
    } else if text.contains("api key") {
        discovery = discovery.with_note(
            "signed in with an API key; the pool kind below is a default rather than something \
             detected",
        );
    }
    discovery
}

// ---------------------------------------------------------------------------
// Binary discovery — npm ships this as codex.cmd on Windows, which
// CreateProcess cannot exec directly; `super::bin` owns the mechanics.
// ---------------------------------------------------------------------------

fn candidate_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["codex.exe", "codex.cmd", "codex.bat"]
    } else {
        &["codex"]
    }
}

static RESOLVED: OnceLock<Option<Invocation>> = OnceLock::new();

fn locate() -> Result<Invocation, TactusError> {
    bin::locate(candidate_names(), &RESOLVED, |tried| {
        format!(
            "codex binary not found on PATH (looked for {}); install the OpenAI Codex CLI \
             (`npm install -g @openai/codex`) or adjust PATH",
            tried.join(", ")
        )
    })
}

/// Registry entry, so `by_id("codex")` resolves without this module being
/// reached through the concrete type.
impl AdapterSource for CodexAdapter {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        (id == ADAPTER_ID).then_some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::WorkerProfile;

    /// Flags that would hand the agent the machine. §20 says none is ever
    /// passed, so the list exists to be asserted against.
    const FORBIDDEN: [&str; 4] = [
        "--dangerously-bypass-approvals-and-sandbox",
        "--dangerously-bypass-hook-trust",
        "danger-full-access",
        "--ephemeral",
    ];

    fn profile(permissions: PermissionMode) -> WorkerProfile {
        WorkerProfile {
            name: "small-gpt-5.6-sol".to_owned(),
            agent: ADAPTER_ID.to_owned(),
            model: "gpt-5.6-sol".to_owned(),
            pool: String::new(),
            permissions,
            max_turns: None,
            extra_args: Vec::new(),
        }
    }

    fn run(permissions: PermissionMode, resume: Option<&str>) -> TaskRun {
        TaskRun {
            prompt: "do the thing".to_owned(),
            profile: profile(permissions),
            workspace: PathBuf::from("/repo"),
            gate_cmds: vec!["cargo test".to_owned()],
            resume_session: resume.map(str::to_owned),
            settings_path: None,
        }
    }

    fn output(code: i32, stdout: &str, stderr: &str) -> ProcessOutput {
        ProcessOutput {
            code: Some(code),
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            timed_out: false,
            duration: Duration::from_secs(1),
        }
    }

    #[test]
    fn a_fresh_attempt_sets_its_sandbox_and_a_resumed_one_must_not() {
        // The CLI's two shapes, which are not one shape with a flag swapped.
        // `exec resume` rejects `-s` outright — observed as exit 2, "unexpected
        // argument '-s' found" — because the sandbox belongs to the session and
        // is inherited. Passing it anyway would fail every same-rung retry for
        // a reason that has nothing to do with the code.
        let fresh = build_args(&run(PermissionMode::Edit, None));
        assert!(fresh.starts_with(&["exec".to_owned()]), "{fresh:?}");
        assert!(!fresh.contains(&"resume".to_owned()), "{fresh:?}");
        assert!(fresh.contains(&"--sandbox".to_owned()), "{fresh:?}");
        assert!(fresh.contains(&"workspace-write".to_owned()), "{fresh:?}");

        let resumed = build_args(&run(PermissionMode::Edit, Some("019ff122-4d61")));
        assert_eq!(
            resumed[..3],
            [
                "exec".to_owned(),
                "resume".to_owned(),
                "019ff122-4d61".to_owned()
            ],
            "{resumed:?}"
        );
        assert!(
            !resumed.contains(&"--sandbox".to_owned()),
            "a resumed attempt must not re-specify the sandbox: {resumed:?}"
        );
    }

    #[test]
    fn the_prompt_is_the_last_argument_and_it_is_stdin() {
        // Windows caps argv at ~8,191 bytes and a review prompt carries the
        // diff, so the prompt has never been passable as an argument. `-` says
        // "read it from stdin", and anything after it would be swallowed as the
        // prompt's own arguments.
        for resume in [None, Some("sess")] {
            let args = build_args(&run(PermissionMode::ReadOnly, resume));
            assert_eq!(args.last().map(String::as_str), Some("-"), "{args:?}");
        }
        // And the payload is actually written, or the CLI sits waiting on a
        // pipe nobody closed.
        let run = run(PermissionMode::Edit, None);
        assert_eq!(CodexAdapter.stdin_payload(&run), "do the thing");
    }

    #[cfg(windows)]
    #[test]
    fn an_implementer_is_refused_where_no_sandbox_can_enforce_it() {
        // Windows has no sandbox helper (`codex doctor`: `linux helper: none`),
        // so `exec` degrades to read-only and writes nothing while returning 0.
        // Measured on run 01KZRMHA28M5CM88VAXP613X9P, which spent both attempts
        // on empty diffs and then parked asking for write access it had been
        // granted. A capability this platform cannot deliver is a refusal to
        // start (§19), not a task that fails after spending.
        let err = edit_refusal(&profile(PermissionMode::Edit))
            .expect("an implementer profile must be refused on Windows");
        let text = err.to_string();
        assert!(text.contains("cannot run"), "{text}");
        assert!(
            text.contains("--approve-for-me"),
            "the refusal has to say which door was tried and why it is shut: {text}"
        );
        // And where to go instead: Linux, or another agent.
        assert!(text.contains("Linux"), "{text}");
        assert!(text.contains("reviewer"), "{text}");
    }

    #[cfg(not(windows))]
    #[test]
    fn an_implementer_is_allowed_where_the_sandbox_is_real() {
        // Same CLI, same flags, helper present: `--sandbox workspace-write`
        // wrote inside the workspace and was blocked outside it, both measured
        // in a container. The refusal above is scoped to the platform that
        // cannot enforce a boundary, not to the CLI.
        assert!(
            edit_refusal(&profile(PermissionMode::Edit)).is_none(),
            "an implementer is fine where the sandbox is enforced"
        );
    }

    #[test]
    fn a_reviewer_is_read_only_and_nothing_is_ever_given_the_machine() {
        // Never refused anywhere: read-only is enforced on every platform, and
        // it is the seat this adapter is most useful in.
        assert!(edit_refusal(&profile(PermissionMode::ReadOnly)).is_none());
        let args = build_args(&run(PermissionMode::ReadOnly, None));
        assert!(args.contains(&"read-only".to_owned()), "{args:?}");
        for permissions in [PermissionMode::Edit, PermissionMode::ReadOnly] {
            for resume in [None, Some("sess")] {
                let args = build_args(&run(permissions, resume)).join(" ");
                for flag in FORBIDDEN {
                    assert!(
                        !args.contains(flag),
                        "`{flag}` must never be passed: {args}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_successful_run_yields_its_session_message_and_tokens() {
        // The real event stream, from a tool-using run against codex-cli
        // 0.147.0 on 2026-08-11.
        let stdout = r#"{"type":"thread.started","thread_id":"019ff122-4d61-7323-a217-843ddfe5932c"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_0","type":"command_execution"}}
{"type":"item.completed","item":{"id":"item_0","type":"command_execution"}}
{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"hi"}}
{"type":"turn.completed","usage":{"input_tokens":27707,"cached_input_tokens":22016,"cache_write_input_tokens":0,"output_tokens":102,"reasoning_output_tokens":0}}"#;
        let outcome = parse_output(&output(0, stdout, "some tracing noise"));

        assert_eq!(outcome.status, OutcomeStatus::Completed);
        assert_eq!(
            outcome.session_id.as_deref(),
            Some("019ff122-4d61-7323-a217-843ddfe5932c"),
            "the thread id is what `exec resume` takes"
        );
        // The agent's final message, not the command_execution item before it.
        // A reviewer's verdict travels in exactly this field.
        assert_eq!(outcome.detail.as_deref(), Some("hi"));

        let usage = outcome.usage.expect("usage");
        assert_eq!(usage.input_tokens, Some(27707));
        assert_eq!(usage.output_tokens, Some(102));
        assert_eq!(usage.cache_read_input_tokens, Some(22016));
        assert_eq!(usage.num_turns, Some(1));
        // Tokens, never a price: this route reports no dollars and tactus does
        // not own a rate table.
        assert_eq!(outcome.cost_usd, None);
    }

    #[test]
    fn several_turns_are_summed_rather_than_last_wins() {
        // One invocation emits one `turn.completed` today, tool call and all.
        // This is the guard for a version that reports per step, where taking
        // the last would silently under-count the run.
        let stdout = r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":2,"reasoning_output_tokens":1}}
{"type":"turn.completed","usage":{"input_tokens":30,"output_tokens":5,"reasoning_output_tokens":4}}"#;
        let usage = parse_output(&output(0, stdout, "")).usage.expect("usage");
        assert_eq!(usage.input_tokens, Some(40));
        assert_eq!(usage.output_tokens, Some(7));
        // Carried, not double-counted: reasoning tokens are a subset of output.
        assert_eq!(usage.reasoning_output_tokens, Some(5));
        assert_eq!(usage.num_turns, Some(2));
    }

    #[test]
    fn an_unauthenticated_run_is_an_agent_error_not_an_exhausted_pool() {
        // Observed: five 401 retries then exit 101. The ladder acts on this
        // distinction — a rate limit defers and waits for a window, an agent
        // error spends an attempt — so calling a signed-out account "rate
        // limited" would park a run forever on a problem that never resolves.
        let stdout = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"error","message":"Reconnecting... 2/5 (unexpected status 401 Unauthorized: Missing bearer or basic authentication in header)"}"#;
        let outcome = parse_output(&output(101, stdout, "ERROR codex_api::endpoint: 401"));
        assert_eq!(outcome.status, OutcomeStatus::AgentError);
        assert!(
            outcome.detail.as_deref().is_some_and(|d| d.contains("401")),
            "{:?}",
            outcome.detail
        );
    }

    #[test]
    fn a_rate_limited_failure_is_told_apart_from_an_ordinary_one() {
        let stdout =
            r#"{"type":"error","message":"You have hit your usage limit for this window"}"#;
        assert_eq!(
            parse_output(&output(1, stdout, "")).status,
            OutcomeStatus::RateLimited
        );
        let stdout = r#"{"type":"error","message":"the file could not be written"}"#;
        assert_eq!(
            parse_output(&output(1, stdout, "")).status,
            OutcomeStatus::AgentError
        );
    }

    #[test]
    fn junk_on_stdout_never_fails_an_attempt() {
        // Warnings, progress chatter, a half-written line at a kill — none of
        // it is JSON and none of it should turn a finished attempt into a
        // failure.
        let stdout = "Reading additional input from stdin...\n\
                      {\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"ok\"}}\n\
                      {not json at all";
        let outcome = parse_output(&output(0, stdout, ""));
        assert_eq!(outcome.status, OutcomeStatus::Completed);
        assert_eq!(outcome.detail.as_deref(), Some("ok"));
    }

    #[test]
    fn signed_out_is_never_read_as_signed_in() {
        // "Not logged in" contains "logged in", so order of checks is the whole
        // test: a confident wrong "you are signed in" writes a pool the
        // operator trusts and a run then fails against.
        let signed_out = parse_login_status(&output(0, "Not logged in\n", ""));
        assert_eq!(signed_out.auth, AuthState::NotAuthenticated);
        assert_eq!(signed_out.shape, None);

        let signed_in = parse_login_status(&output(0, "Logged in using ChatGPT\n", ""));
        assert_eq!(signed_in.auth, AuthState::Authenticated);
        assert_eq!(
            signed_in.shape,
            Some(PoolKind::SubscriptionWindow),
            "a ChatGPT plan is a window, not metered dollars"
        );

        // Anything unrecognised stays Unknown and says so, rather than being
        // forced into one of the two answers.
        let odd = parse_login_status(&output(0, "something new entirely\n", ""));
        assert_eq!(odd.auth, AuthState::Unknown);
        assert!(!odd.notes.is_empty());
    }
}
