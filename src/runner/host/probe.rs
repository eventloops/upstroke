//! Extended notes: `docs/internals/runner/host/probe.md`

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

pub const SHELL_PROBE_COMMAND: &str = "exit 0";

pub const SHELL_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

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
