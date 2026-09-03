//! The `RunnerPreflight` shell probe: the command it runs, how long it may
//! take, and its execution through a [`Runner`].
//!
//! INV-23's non-slotted half. What the probe *means* is the same for both
//! runners -- PR6's container runner executes the identical request inside the
//! recorded image -- so nothing here is host-specific and nothing here performs
//! an effect: the spawn belongs to whichever `Runner` the caller passes, which
//! for `host-v1` is the parent module's `HostRunner::run`.
//!
//! The request *constructor* stays in the parent and is deliberately not here.
//! `runner::tests::every_production_runner_request_is_built_by_its_roles_builder`
//! and `runner::tests::a_command_is_assembled_in_one_production_place_per_role`
//! pin `RunnerRequest`'s and `ShellKind::spec`'s one production construction
//! site per role by file, and `src/runner/host.rs` is the pinned file -- the
//! same kind of pin that keeps `Contained`'s mint and the reserved-key
//! vocabulary in the parent. Construction is pinned; execution is not.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree and not by the file, so an out-of-line
// child of `src/runner/host.rs` would otherwise inherit that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// -- `PR6-LANEF-004`, and the mistake two W1 pull requests each made
// independently. Nothing here reaches a governed primitive, so all three are
// DENIED rather than allowed, and this module takes no `effects/allowlist.toml`
// row: an allowance is what that file records, and this module takes none.
// `runner::container::tests::every_child_module_of_the_container_funnel_states_\
// its_own_lint_level` already walks `src/runner/host/`, so this file was graded
// against all three from its first commit.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::path::PathBuf;
use std::time::Duration;

use crate::error::UpstrokeError;
use crate::gates::ShellKind;
use crate::runner::Runner;
use crate::runner::invocation::InvocationId;

use super::shell_probe_request;

/// The command the `RunnerPreflight` shell probe runs.
///
/// `decisions.sequential_substrate.runner`: "the RunnerPreflight shell probe
/// (the recorded shell executing `exit 0` through the Runner …)". Not `true`,
/// not `--version`: `exit 0` is a builtin of every shell
/// [`ShellKind`] names, so the probe tests the shell and not a program that
/// happens to be beside it.
pub const SHELL_PROBE_COMMAND: &str = "exit 0";

/// How long the shell probe may take.
///
/// A shell that has not run `exit 0` in ten seconds is unavailable for
/// pre-flight's purposes whatever it is doing.
pub const SHELL_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Execute the shell probe through `runner`.
///
/// The packet's own finding `F-43` / `V14-VERIFY-004` is why this exists:
/// availability cannot be established by inspection, only by a spawn.
/// `gates::shell_available` is a PATH check and stays one — it answers
/// "is there a file with that name", which is a different question from "does
/// this shell run a command".
///
/// # Errors
///
/// [`UpstrokeError::Refused`]. The contract's `expected_failures_refusals[3]`:
/// "a failing shell probe -> returned pre-flight error to the caller
/// (TopologyRun classifies it: creator error before P5b on a fresh run;
/// refusal before any recovery event on resume)". Classification is the
/// caller's; this returns the error.
pub fn run_shell_probe(
    runner: &dyn Runner,
    shell: ShellKind,
    workspace: PathBuf,
    invocation: InvocationId,
) -> Result<(), UpstrokeError> {
    let request = shell_probe_request(shell, workspace, invocation);
    let output = runner
        .run(&request)
        .map_err(|error| UpstrokeError::Refused {
            message: format!(
                "pre-flight: the recorded shell `{}` could not be run through the runner: {error}",
                shell.program()
            ),
        })?;
    if output.timed_out {
        return Err(UpstrokeError::Refused {
            message: format!(
                "pre-flight: the recorded shell `{}` did not finish `{SHELL_PROBE_COMMAND}` \
                 within {:?}",
                shell.program(),
                SHELL_PROBE_TIMEOUT
            ),
        });
    }
    // A probe whose tree was terminated for exceeding the bounded capture
    // allowance did not run `exit 0` to completion, whatever code came back.
    // `output_limited` means the supervisor killed the owned process tree
    // (`proc.rs`: "the child exceeded the bounded stdout or stderr capture
    // allowance and its owned process tree was terminated"), and the limit can
    // be observed during the final drain of a child that has *already exited
    // 0* — so this is checked on its own rather than through the exit code. A
    // shell that printed 16 MiB in answer to `exit 0` is not the shell this
    // pre-flight is certifying either way.
    if output.output_limited {
        return Err(UpstrokeError::Refused {
            message: format!(
                "pre-flight: the recorded shell `{}` exceeded the bounded output allowance \
                 running `{SHELL_PROBE_COMMAND}` and its process tree was terminated",
                shell.program()
            ),
        });
    }
    if output.code != Some(0) {
        return Err(UpstrokeError::Refused {
            message: format!(
                "pre-flight: the recorded shell `{}` ran `{SHELL_PROBE_COMMAND}` and exited {}; \
                 stderr: {}",
                shell.program(),
                match output.code {
                    Some(code) => code.to_string(),
                    None => "by a signal or a kill".to_owned(),
                },
                output.stderr.trim()
            ),
        });
    }
    Ok(())
}
