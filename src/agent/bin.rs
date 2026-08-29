//! Naming and invoking an agent CLI — the parts every adapter needs and
//! none of them should own privately.
//!
//! **This module used to *locate* the CLI, and it deliberately no longer
//! does.** An adapter names its CLI; the boundary that will execute it decides
//! which file that name is. `PR4-ADAPTER-RESOLVES-ON-THE-HOST` in
//! `reviews/FINDINGS.md` is the entry, [`Invocation::named`] is the repair, and
//! DESIGN.md:612 is the sentence it serves: "Probes run through that same
//! runner, **or pre-flight could certify a host CLI/version different from the
//! one the attempt executes**", with the normal container case being "an image
//! with version-pinned CLIs" that a coordinator host need not have at all.
//!
//! Windows is why this module exists. Both agent CLIs ship as npm packages, so
//! the thing on PATH is frequently a `.cmd` shim rather than a native
//! executable, and `CreateProcess` cannot exec a batch script. That used to be
//! handled here by building a `cmd /C` command line by hand and passing it
//! through `raw_arg`.
//!
//! **It is not any more, and the reason is worth recording.** `raw_arg` opts
//! out of everything the standard library does for batch targets, including the
//! argument escaping added in Rust 1.77.2 for CVE-2024-24576; this crate is
//! edition 2024, so that fix is unconditionally present. Measured against a
//! real npm-shape shim, the hand-rolled version expanded `%VAR%` inside
//! arguments — turning `--allow-tool=shell(echo %PATH%)` into the machine's
//! entire PATH — while `Command::args` carried every case through intact:
//! `&`, `|`, `%`, embedded quotes, `^`, spaces, and the empty argument.
//!
//! Copilot is what made that matter. Its permission surface is argv, so gate
//! commands — strings a user writes in `upstroke.toml` — now reach a Windows
//! command line, and a mangled `--allow-tool=shell(<gate>)` is a permission
//! grant that no longer matches the command it is meant to authorize. The
//! module comment used to argue that two copies of the quoting logic would be
//! two chances to get it wrong. The right number was zero.
use std::path::PathBuf;

use crate::error::UpstrokeError;
use crate::runner::CommandSpec;
use crate::util;

/// An agent CLI as a program string, and how to spawn it.
///
/// The field is a [`PathBuf`] rather than a `String` because a program string
/// **may** be a path: [`Invocation::at`] still builds one from an absolute
/// path, and [`Invocation::spec`]'s refusal is the boundary at which a path
/// that a `String` cannot carry is named rather than silently rewritten
/// (`PR4-PROGRAM-PATH-NOT-UNICODE`). Production constructs only
/// [`Invocation::named`], whose input is a `&str` and therefore always
/// representable — so that conflict no longer has a production instance,
/// while the refusal that documents it stays reachable and tested.
#[derive(Debug, Clone)]
pub struct Invocation {
    path: PathBuf,
}

impl Invocation {
    /// The agent CLI as the **boundary that will execute it** names it.
    ///
    /// A bare program name, and that is the whole repair. The adapter knows
    /// "an official CLI" (DESIGN.md:117); it does not know which filesystem
    /// will hold it, and until this it answered that question anyway by
    /// resolving against the coordinator host's `PATH` and serialising an
    /// absolute host path into [`CommandSpec::program`]. With one runner whose
    /// boundary *is* the host that was invisible. With a container runner it is
    /// three separate failures, and `PR4-ADAPTER-RESOLVES-ON-THE-HOST` names
    /// them: a CLI pinned in the image and absent on the host was refused
    /// before the runtime was asked anything; every spec carried a path that
    /// names nothing inside the image; and `Caps.version` certified the host's
    /// CLI while the attempt ran the image's.
    ///
    /// A bare name is not a new shape for this crate. [`crate::gates::ShellKind::spec`]
    /// has always put one in a spec — `sh`, `bash`, `cmd`, `pwsh` — for every
    /// gate and for the `RunnerPreflight` shell probe, and the host runner has
    /// always executed it. The three agent CLIs were the exception; this makes
    /// them the rule.
    ///
    /// **There is no cache and nothing to key.** `probe` and `build` call this,
    /// it is a function of its argument alone, and the two therefore agree by
    /// construction rather than by an ordering between them. That is the
    /// answer to "two runners in one process": a resolution that is correct on
    /// first use and wrong on the second needs a resolution to be *remembered*,
    /// and this remembers nothing.
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self {
            path: PathBuf::from(name),
        }
    }
}

impl Invocation {
    /// The command to run, as data: `args` are carried verbatim.
    ///
    /// Nothing is quoted, escaped, or wrapped here on purpose: `std` knows
    /// whether the resolved path is a batch shim and applies the right rules,
    /// and every attempt to help it has been a way to get this wrong. The
    /// escaping still happens in exactly one place — the runner, when it turns
    /// this spec into a `Command` — and this returns a
    /// [`CommandSpec`] rather than a `Command` because DESIGN.md:117 says an
    /// adapter "does not decide where the process runs".
    ///
    /// [`CommandSpec::program`] is a `String` (DESIGN.md:222), and a resolved
    /// path that is not valid Unicode cannot become one **without becoming a
    /// different path**. So this refuses rather than converting.
    ///
    /// The rejected alternative was `to_string_lossy`, and it is worth
    /// recording why: `String::from_utf8_lossy` replaces each invalid byte
    /// with `U+FFFD`, so a `claude` inside a `PATH` directory whose name
    /// carries a non-UTF-8 byte — legal on Unix, where a path is bytes —
    /// arrives at the runner as a path that names *nothing*, and the run dies
    /// at `CreateProcess`/`execvp` with "failed to spawn", pointing at a path
    /// the operator never wrote. Before this slice the `PathBuf` reached
    /// `Command::new` unchanged and that installation ran.
    ///
    /// Neither behaviour is "legacy engine behavior unchanged"
    /// (`invariants_preserved[1]`), because the frozen `CommandSpec.program:
    /// String` cannot carry the input at all; the choice is between two ways
    /// of failing. This one fails **at the boundary that cannot represent the
    /// value**, names the path and says why, and cannot be mistaken for a
    /// missing installation. Widening `CommandSpec.program` to an `OsString`
    /// is the repair that would restore the old behaviour, and it is a change
    /// to DESIGN.md:222 rather than to this function.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when the resolved path is not valid Unicode.
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

/// Rewrite a runner's refusal to execute `name` into something an operator can
/// act on, saying **where** the CLI is missing.
///
/// This is the operator-facing half of the repair, and it is needed *because*
/// of it. Before, the adapter refused with "claude binary not found on PATH …
/// install Claude Code" — a true sentence, because the only boundary was this
/// machine. Now the boundary may be a container image, and "not found" without
/// "not found *where*" sends the operator to install a CLI on a host that will
/// never execute it.
///
/// It reads this machine's `PATH` **only after the boundary has already
/// refused**, and only to say which of the two situations the operator is in.
/// Nothing it returns decides what runs. `install_hint` is the adapter's own
/// sentence about how its CLI is installed.
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

/// First `digits.digits.digits` token wins; otherwise the trimmed first line
/// verbatim (`--version` formats have churned before, in both CLIs).
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
        // Trailing punctuation is not part of a version. The Copilot CLI ends
        // its line with a full stop — `GitHub Copilot CLI 1.0.78.` — which
        // otherwise rides along into `Caps.version` and out through every
        // message that quotes it (`upstroke capacity`, and the probe refusal that
        // names the version an adapter would not support).
        .map(|t| {
            t.trim_start_matches('v')
                .trim_end_matches(['.', ',', ';'])
                .to_owned()
        })
        .unwrap_or_else(|| first_line.to_owned())
}

/// Test-only constructors.
///
/// Below every production item on purpose: `effects::production_region` cuts a
/// file at its **first** `#[cfg(test)]`, so a test-only item placed among the
/// production ones takes the rest of the file out of the wrapper-classification
/// domain — silently, and `mechanism` (3)'s "every pubfn … is classified" would
/// then be true of a domain nobody drew. That is `PR5D-VISIBILITY-CHECK-
/// DUPLICATED`'s shape one level out, and it was measured here: five of this
/// module's functions left the census the moment a `#[cfg(test)] fn` was added
/// above them.
#[cfg(test)]
impl Invocation {
    /// An invocation naming `path`, for tests that need one without asking
    /// this machine what it has installed.
    ///
    /// Production's only constructor is [`Invocation::named`], whose argument
    /// is a bare CLI name. This exists for the tests that must drive a spec
    /// carrying an **absolute** program — the host runner's own fixture grids
    /// pin the difference between a shell's bare name and an absolute native
    /// executable, and [`Invocation::spec`]'s non-Unicode refusal has no other
    /// input at all.
    pub(crate) fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[cfg(test)]
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

        // The property the deleted quoting code kept breaking. These are the
        // exact shapes Copilot's permission surface produces: a gate command
        // with spaces and parentheses, a cmd metacharacter, a percent sign, an
        // embedded quote, and an empty argument.
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

        // And the same through the runner's own translation, which is what
        // actually spawns: the spec surviving intact would be worth nothing if
        // the step that turns it into a `Command` re-quoted it. The `cmd.exe`
        // raw-tail rule applies to `cmd`, and this program is not it.
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
        // Verbatim from the Copilot CLI: the sentence's full stop is not part
        // of the version, and rode into `Caps.version` when it was not trimmed.
        assert_eq!(
            extract_version(
                "GitHub Copilot CLI 1.0.78.\nRun 'copilot update' to check for updates.\n"
            ),
            "1.0.78"
        );
    }

    /// A named CLI is the name and nothing else — no directory, no extension,
    /// nothing this machine contributed.
    ///
    /// The expected values are written here, not read from the adapters: a
    /// constructor compared only against the code that produced it proves
    /// nothing. What is asserted is the *shape* — one path component, not
    /// absolute — because that is the property a coordinator-host resolution
    /// cannot have, on either platform, whatever this machine happens to have
    /// installed.
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

    /// The same name, twice, is the same spec — which is what makes `probe`
    /// and `build` agree without an ordering between them.
    ///
    /// The old constructor memoised into a process-wide `OnceLock`, so the
    /// *first* caller in the process decided the answer for every later one.
    /// This asserts the property that replaced it: the constructor is a
    /// function of its argument, so no call can be poisoned by an earlier one.
    /// Both call orders, because "the first caller wins" is a property of
    /// order.
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

    /// A refusal from the boundary says which boundary, and whether this host
    /// has the CLI — the two are different situations with different fixes.
    ///
    /// Both branches, and both asserted rather than whichever this machine
    /// happens to take: `upstroke-definitely-not-a-real-binary` is absent
    /// everywhere by construction, and the present branch is driven with a
    /// program every machine of each family has.
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

    /// A `.cmd` shim really does execute, and an argument really does arrive.
    ///
    /// Asserting on the constructed `Command` proves we hand `std` the right
    /// thing; only spawning proves `std` then does the right thing with a batch
    /// target, which is the half the old hand-rolled code got wrong.
    #[cfg(windows)]
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "Windows-only test fixture creates and executes a local batch shim; production carries only CommandSpec data into the Process funnel"
    )]
    fn a_batch_shim_runs_and_receives_its_argument() {
        let dir = std::env::temp_dir().join(format!("upstroke-bin-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let shim = dir.join("upstroke-test-shim.cmd");
        // `%~1` strips the quotes the child got; a benign argument keeps this
        // about plumbing rather than about batch re-parsing.
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

    /// A resolved path that a `String` cannot carry is refused by name, not
    /// converted into a path that names nothing.
    ///
    /// Both platforms have such a path and neither can be spelled in source as
    /// a `&str`: on Unix a path is bytes, so `0xff` is legal and not UTF-8; on
    /// Windows it is UTF-16, so an unpaired surrogate is legal and not UTF-8.
    /// Every other fixture in this module is valid Unicode, which is why the
    /// lossy conversion this replaced survived the suite while changing what a
    /// supported installation did.
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
        // The operator has to be able to find the entry. `display()` stays
        // lossy on purpose — it is a diagnostic, not a program.
        assert!(message.contains(rendered), "{message}");
        assert!(message.contains("not valid Unicode"), "{message}");
        assert_eq!(unusable.display(), rendered);

        // And the ordinary case is unaffected: same call, a Unicode path.
        let fine = invocation("/usr/local/bin/claude")
            .spec(&["--version".to_owned()])
            .expect("a Unicode path is carried unchanged");
        assert_eq!(fine.program, "/usr/local/bin/claude");
        assert_eq!(fine.args, vec!["--version".to_owned()]);

        // A path that legitimately *contains* `U+FFFD` is carried as itself.
        //
        // `U+FFFD` is an ordinary character in a filename. It is only special
        // as `to_string_lossy`'s substitution marker, so every conversion that
        // treats it as one — `to_string_lossy()` followed by a `replace`, the
        // shape `PR4-SEAMS-004` names — silently renames a directory that
        // really is called that, and spawns something else or nothing.
        //
        // Neither fixture above can see it: the refusal fixture's path is not
        // valid Unicode at all, and the ordinary fixture's path carries no
        // marker. This is the one input on which "refuse" and "substitute"
        // still disagree after the refusal is in place.
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
