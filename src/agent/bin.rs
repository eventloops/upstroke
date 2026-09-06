//! Extended notes: `docs/internals/agent/bin.md`

use std::path::PathBuf;

use crate::error::UpstrokeError;
use crate::runner::CommandSpec;
use crate::util;

#[derive(Debug, Clone)]
pub struct Invocation {
    path: PathBuf,
}

impl Invocation {
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self {
            path: PathBuf::from(name),
        }
    }
}

impl Invocation {
    pub fn spec(&self, args: &[String]) -> Result<CommandSpec, UpstrokeError> {
        let Some(program) = self.path.to_str() else {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the agent binary resolved to `{}`, a path that is not valid Unicode. \
                     A CommandSpec carries its program as a String (DESIGN.md:222), and \
                     converting this path would spawn a different one, so it is refused here \
                     rather than at the spawn. Install the CLI under a Unicode path, or remove \
                     that PATH entry",
                    self.path.to_string_lossy()
                ),
            });
        };
        Ok(CommandSpec {
            program: program.to_owned(),
            args: args.to_vec(),
            env: Vec::new(),
            stdin: Vec::new(),
        })
    }

    pub fn display(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

pub fn boundary_refused(name: &str, install_hint: &str, cause: &UpstrokeError) -> UpstrokeError {
    let on_this_host = match util::find_program(name) {
        Some(path) => format!(
            "this coordinator host has `{name}` at `{}`, so the boundary this run executes in is \
             not this host",
            path.display()
        ),
        None => format!("this coordinator host has no `{name}` on PATH either"),
    };
    UpstrokeError::Agent {
        message: format!(
            "`{name}` could not be executed by the runner this run uses: {cause}. It must be \
             installed inside the boundary that executes it — on PATH for the host runner, in the \
             image for a container runner. {install_hint} ({on_this_host})"
        ),
    }
}

pub fn extract_version(stdout: &str) -> String {
    let first_line = stdout.lines().next().unwrap_or_default().trim();
    first_line
        .split_whitespace()
        .find(|token| {
            let mut parts = token.trim_start_matches('v').split('.');
            let numeric = |s: Option<&str>| {
                s.is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
            };
            numeric(parts.next()) && numeric(parts.next()) && parts.next().is_some()
        })
        .map(|t| {
            t.trim_start_matches('v')
                .trim_end_matches(['.', ',', ';'])
                .to_owned()
        })
        .unwrap_or_else(|| first_line.to_owned())
}

#[cfg(test)]
impl Invocation {
    pub(crate) fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::Path;

    fn invocation(path: &str) -> Invocation {
        Invocation {
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn arguments_reach_the_command_untouched() {
        use crate::runner::host::test_support::build_command;

        let args: Vec<String> = [
            "-s",
            "--allow-tool=shell(cargo test)",
            "--allow-tool=shell(echo hi & whoami)",
            "--allow-tool=shell(echo %PATH%)",
            r#"--allow-tool=shell(cargo test -- --exact "my test")"#,
            "--setting-sources",
            "",
        ]
        .map(str::to_owned)
        .to_vec();

        let spec = invocation(r"C:\Users\John Smith\npm\copilot.cmd")
            .spec(&args)
            .expect("a Unicode path");
        assert_eq!(
            spec.program, r"C:\Users\John Smith\npm\copilot.cmd",
            "the shim is the program; nothing wraps it in a shell"
        );
        assert_eq!(spec.args, args, "every argument survives verbatim");

        let cmd = build_command(&spec);
        assert_eq!(
            cmd.get_program(),
            OsStr::new(r"C:\Users\John Smith\npm\copilot.cmd")
        );
        let seen: Vec<&OsStr> = cmd.get_args().collect();
        let expected: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        assert_eq!(seen, expected, "every argument survives verbatim");
    }

    #[test]
    fn version_extraction_handles_known_formats() {
        assert_eq!(extract_version("2.1.35 (Claude Code)\n"), "2.1.35");
        assert_eq!(extract_version("claude v1.0.128\n"), "1.0.128");
        assert_eq!(extract_version("weird output\n"), "weird output");
        assert_eq!(
            extract_version(
                "GitHub Copilot CLI 1.0.78.\nRun 'copilot update' to check for updates.\n"
            ),
            "1.0.78"
        );
    }

    #[test]
    fn a_named_cli_carries_no_location() {
        for name in ["claude", "codex", "copilot"] {
            let spec = Invocation::named(name)
                .spec(&["--version".to_owned()])
                .expect("a bare name is always representable as a String");
            assert_eq!(spec.program, name);
            let program = Path::new(&spec.program);
            assert!(
                !program.is_absolute(),
                "{name}: a named CLI became an absolute path"
            );
            assert_eq!(
                program.components().count(),
                1,
                "{name}: a named CLI grew a directory component"
            );
            assert_eq!(
                program.extension(),
                None,
                "{name}: a named CLI grew an extension"
            );
            assert_eq!(Invocation::named(name).display(), name);
        }
    }

    #[test]
    fn naming_a_cli_is_a_function_of_its_argument_alone() {
        let claude_first = [
            Invocation::named("claude").display(),
            Invocation::named("codex").display(),
            Invocation::named("claude").display(),
        ];
        let codex_first = [
            Invocation::named("codex").display(),
            Invocation::named("claude").display(),
            Invocation::named("codex").display(),
        ];
        assert_eq!(claude_first, ["claude", "codex", "claude"]);
        assert_eq!(codex_first, ["codex", "claude", "codex"]);
    }

    #[test]
    fn a_boundary_refusal_says_where_the_cli_is_missing() {
        let cause = UpstrokeError::Agent {
            message: "no such file or directory".to_owned(),
        };

        let absent = boundary_refused(
            "upstroke-definitely-not-a-real-binary",
            "install it.",
            &cause,
        )
        .to_string();
        assert!(
            absent.contains("upstroke-definitely-not-a-real-binary"),
            "{absent}"
        );
        assert!(absent.contains("no such file or directory"), "{absent}");
        assert!(
            absent.contains("in the image for a container runner"),
            "{absent}"
        );
        assert!(
            absent.contains("no `upstroke-definitely-not-a-real-binary` on PATH either"),
            "{absent}"
        );

        let present_name = if cfg!(windows) { "cmd" } else { "sh" };
        let present_path = util::find_program(present_name)
            .expect("every machine of this family has a shell on PATH");
        let present = boundary_refused(present_name, "install it.", &cause).to_string();
        assert!(
            present.contains(&present_path.display().to_string()),
            "the message must name where this host has it: {present}"
        );
        assert!(
            present.contains("not this host"),
            "the message must say the boundary is not this host: {present}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_batch_shim_runs_and_receives_its_argument() {
        let dir = std::env::temp_dir().join(format!("upstroke-bin-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let shim = dir.join("upstroke-test-shim.cmd");
        std::fs::write(&shim, "@echo off\r\necho GOT:%~1\r\n").expect("write shim");

        let out = crate::runner::host::test_support::build_command(
            &invocation(&shim.to_string_lossy())
                .spec(&["hello world".to_owned()])
                .expect("a Unicode path"),
        )
        .output()
        .expect("the shim spawns");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("GOT:hello world"),
            "the shim ran and saw its argument: {stdout:?}"
        );
    }

    #[test]
    fn a_program_path_a_string_cannot_carry_is_refused_by_name() {
        #[cfg(unix)]
        let (path, rendered) = {
            use std::os::unix::ffi::OsStringExt;
            let mut bytes = b"/opt/upstroke-".to_vec();
            bytes.push(0xff);
            bytes.extend_from_slice(b"/claude");
            (
                PathBuf::from(std::ffi::OsString::from_vec(bytes)),
                "/opt/upstroke-\u{fffd}/claude",
            )
        };
        #[cfg(windows)]
        let (path, rendered) = {
            use std::os::windows::ffi::OsStringExt;
            let mut units: Vec<u16> = r"C:\upstroke-".encode_utf16().collect();
            units.push(0xd800);
            units.extend(r"\claude.cmd".encode_utf16());
            (
                PathBuf::from(std::ffi::OsString::from_wide(&units)),
                "C:\\upstroke-\u{fffd}\\claude.cmd",
            )
        };

        assert!(
            path.to_str().is_none(),
            "the fixture path is valid Unicode, so it witnesses nothing"
        );
        let unusable = Invocation { path };
        let error = unusable
            .spec(&["--version".to_owned()])
            .expect_err("a path a String cannot carry must be refused");
        let message = error.to_string();
        assert!(message.contains(rendered), "{message}");
        assert!(message.contains("not valid Unicode"), "{message}");
        assert_eq!(unusable.display(), rendered);

        let fine = invocation("/usr/local/bin/claude")
            .spec(&["--version".to_owned()])
            .expect("a Unicode path is carried unchanged");
        assert_eq!(fine.program, "/usr/local/bin/claude");
        assert_eq!(fine.args, vec!["--version".to_owned()]);

        let literal = "/opt/upstroke-\u{fffd}/claude";
        assert!(
            literal.contains(char::REPLACEMENT_CHARACTER),
            "the fixture lost its marker, so it witnesses nothing"
        );
        let carried = invocation(literal)
            .spec(&["--version".to_owned()])
            .expect("U+FFFD is a legal character in a path, not a conversion failure");
        assert_eq!(
            carried.program, literal,
            "a path containing U+FFFD was rewritten rather than carried"
        );
    }

    #[test]
    fn display_is_the_resolved_path() {
        assert_eq!(
            invocation("/usr/local/bin/claude").display(),
            "/usr/local/bin/claude"
        );
        assert!(Path::new("/usr/local/bin/claude").is_absolute() || cfg!(windows));
    }
}
