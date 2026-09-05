//! What `connect` renders: the pools file it writes, and the summary the CLI
//! prints once it has.
//!
//! Both are text over decisions the parent has already made. Nothing here
//! probes a CLI, derives a pool, reads the file that is already on disk, or
//! writes anything — `run_with` does all four and hands the results down. That
//! is the whole cut: the parent keeps discovery, pool derivation, the operator
//! keys it carries across a `--force`, the two comparisons that decide whether
//! to rewrite, and the write itself; this module turns what they produced into
//! strings.
//!
//! **One input is not the parent's: the clock.** The header records when
//! `connect` ran, and [`pools_file`] reads the clock to say so. That is the
//! module's only impure line; [`pools_file_at`] is the same rendering with the
//! timestamp supplied, and it is what the tests render.
//!
//! **The pools file is a persisted format, and this module is its writer.** Two
//! readers parse it back: `config::read` whenever `upstroke run`, `validate` or
//! `capacity` loads pools, and the parent's `operator_keys` on the next
//! `connect`. So nothing this module did not write itself is placed in the file
//! raw. A value or a table key goes through [`toml_string`] or [`toml_key`], a
//! number through [`toml_number`], and anything written into a `#` line through
//! [`comment`]. The payloads are a CLI's output (a discovery note), an adapter's
//! wording and the operator's own keys, none of which promises to be one clean
//! line of printable text — and a raw one produced a file `config::read`
//! refused, which stops every command that loads pools, while on the `--force`
//! path it corrupted the very keys the carrying exists to keep.
//!
//! **No name here is a public path.** `render_report` stays in `super` under
//! the name `main` calls and `effects/wrappers.toml` classifies, delegating to
//! [`report`]; the declaration is a plain private `mod`, so nothing nests under
//! `connect::render` and `connect`'s externally reachable surface is the same
//! four functions the wrapper census already records.

// The two effect denials are **restored** here rather than inherited. A lint
// level is scoped by the module tree and not by the file, so `super`'s
// `#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]` — which it
// carries because it creates a directory and writes the operator's pools file —
// would otherwise reach every line below. Nothing here touches a file or a
// process, so that allowance has no business here, and re-denying is what keeps
// this module out of `effects/allowlist.toml`: an allowance is what that file
// records, and this module takes none.
//
// Not in tension with a file written entirely with `writeln!`: these render
// onto a `String`, which is `std::fmt::Write::write_fmt`, and `clippy.toml`
// says in its own words that this is "a different DefId" from the
// `std::io::Write::write_fmt` it denies. The `let _ =` on each of them discards
// a `fmt::Result` that `String`'s implementation never makes `Err`; it is the
// idiom for an infallible write, not a folded error.
#![deny(clippy::disallowed_methods, clippy::disallowed_macros)]

use std::fmt::Write as _;

use crate::agent::Discovery;
use crate::capacity::{Allowance, Pool};
use crate::util;

use super::{AgentReport, ConnectReport, Wrote};

/// How every pools file `connect` writes begins, up to the version.
///
/// The parent's `stable_content` drops exactly one line from the rewrite
/// comparison, by this prefix, because the timestamp on that line moves on its
/// own; the parent spells the prefix as its own literal, and
/// `re_connecting_an_unchanged_machine_reports_unchanged_rather_than_a_conflict`
/// is what fails when the two spellings part.
pub(super) const WRITTEN_BY: &str = "# Written by `upstroke connect`";

/// Render the pools file: §17's shape, plus a header saying who wrote it, when,
/// and where the model roster came from.
///
/// This is the one line of the module that reads the clock. The rendering
/// itself is [`pools_file_at`].
pub(super) fn pools_file(agents: &[AgentReport]) -> String {
    pools_file_at(agents, &util::rfc3339_utc_now())
}

/// [`pools_file`] with the header's timestamp supplied.
///
/// `written_at` is interpolated into the first comment line and nothing parses
/// it back, so it is text here rather than a time; the parent's
/// `stable_content` is what knows that line moves.
pub(super) fn pools_file_at(agents: &[AgentReport], written_at: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{WRITTEN_BY} v{} on {written_at}.",
        env!("CARGO_PKG_VERSION")
    );
    // The roster sentence defers to the per-agent notes on purpose. Whether a
    // CLI lists its models non-interactively is an adapter's finding, stated
    // in its own note (Codex does and its note says so; Claude Code and
    // Copilot do not and theirs say that), and a blanket claim here was false
    // for one of the three.
    comment(
        &mut out,
        "# ",
        &format!(
            "\n\
             Pools are user-level (§17): they describe YOUR subscriptions, not this repo. The file\n\
             is hand-editable and `upstroke connect` will not overwrite your edits without --force.\n\
             \n\
             Model roster provenance: the static capability table shipped with this binary (v{}).\n\
             Whether your installed CLI's own model listing was checked against it is stated per\n\
             agent below; where it was not, nothing here was confirmed against what that CLI accepts.\n\
             \n\
             `profile` selects between several accounts on one vendor (§13). It is parsed, shown by\n\
             `upstroke capacity`, carried across --force, and acted on by nothing in v0.1.",
            env!("CARGO_PKG_VERSION"),
        ),
    );

    for report in agents {
        out.push('\n');
        match usable(report) {
            Ok((discovery, pool)) => {
                comment(
                    &mut out,
                    "# ",
                    &format!("{}: {}", report.agent, discovery.auth),
                );
                for note in &discovery.notes {
                    comment(&mut out, "#   ", note);
                }
                if discovery.shape.is_none() {
                    comment(&mut out, "#   ", KIND_IS_A_DEFAULT);
                }
                pool_section(&mut out, pool);
            }
            Err(_) => {
                comment(
                    &mut out,
                    "# ",
                    &format!(
                        "{}: not usable on this machine, so no pool was written for it.",
                        report.agent
                    ),
                );
            }
        }
    }
    out
}

/// The sentence the file carries under a pool whose `kind` discovery could not
/// determine. The summary marks the same pool inline, so an operator who never
/// opens the file still sees that the kind is a guess.
const KIND_IS_A_DEFAULT: &str =
    "kind below is a default, not something detected — change it if your plan differs";

/// Whether an agent contributed a pool, and if so which, with the reason where
/// it did not.
///
/// `AgentReport` carries a `Result` and an `Option` that the parent always sets
/// together — `Ok` with `Some`, `Err` with `None` — so the two other
/// combinations are unreachable from `run_with`; but the type admits them, and
/// the two renderers used to read the pair separately, the file printing a
/// line for `(Ok, None)` and the summary nothing. One reading, here: an agent
/// is usable when both halves say so, and `Err` means what the field's doc
/// says — no pool — whatever the other half holds.
fn usable(report: &AgentReport) -> Result<(&Discovery, &Pool), &str> {
    match (&report.outcome, &report.pool) {
        (Ok(discovery), Some(pool)) => Ok((discovery, pool)),
        (Err(error), _) => Err(error),
        (Ok(_), None) => Err("discovery answered but derived no pool"),
    }
}

/// One `[pools.<name>]` table.
///
/// Every value goes through the encoder for its type. `safety_margin` and
/// `reserve` are written with two decimals because that is §17's spelling and
/// both are always §13's defaults here (`Pool::discovered` sets them and
/// nothing the parent carries changes them), so the rounding is exact.
fn pool_section(out: &mut String, pool: &Pool) {
    let _ = writeln!(out, "[pools.{}]", toml_key(&pool.name));
    let _ = writeln!(out, "kind = {}", toml_string(&pool.kind.to_string()));
    let _ = writeln!(out, "agent = {}", toml_string(&pool.agent));
    if let Some(window) = pool.window {
        let _ = writeln!(
            out,
            "window = {}",
            toml_string(&crate::capacity::render_duration(window))
        );
    }
    if pool.weekly {
        let _ = writeln!(out, "weekly = true");
    }
    let sources: Vec<String> = pool
        .sources
        .iter()
        .map(|source| toml_string(&source.to_string()))
        .collect();
    let _ = writeln!(out, "sources = [{}]", sources.join(", "));
    let _ = writeln!(out, "safety_margin = {:.2}", pool.safety_margin);
    let _ = writeln!(
        out,
        "reserve = {:.2}                     # headroom kept for your own interactive sessions",
        pool.reserve
    );
    // The operator's own keys, written back out. `connect` never invents any of
    // these — it cannot discover which account, how large an allowance is, or
    // where a local model lives — but once one is in the file it has to survive
    // being rewritten, or `--force` would delete exactly what the refusal it
    // overrides existed to protect. They are the operator's text, parsed by the
    // parent from whatever spelling the operator chose, so they are the values
    // most likely to need an escape: a Windows path in `profile` holds
    // backslashes, and written raw those read as TOML escapes.
    if let Some(profile) = &pool.profile {
        let _ = writeln!(out, "profile = {}", toml_string(profile));
    }
    if let Allowance::Units(units) = pool.monthly_allowance {
        let _ = writeln!(out, "monthly_allowance = {}", toml_number(units));
    }
    if let Some(endpoint) = &pool.endpoint {
        let _ = writeln!(out, "endpoint = {}", toml_string(endpoint));
    }
}

/// Write `text` as comment lines, one per line of `text`, each beginning with
/// `prefix`.
///
/// TOML forbids control characters other than tab in a comment (U+0000 to
/// U+0008, U+000A to U+001F and U+007F), and a line break is the one that does
/// more than fail the parse: it ends the comment, and the rest of the payload
/// reaches the reader as a setting. So the payload is split on its line breaks
/// and each line gets the prefix, and every other forbidden character becomes
/// a space. An empty payload still occupies one line, so a caller writing one
/// comment always gets one.
fn comment(out: &mut String, prefix: &str, text: &str) {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    // A bare `#` on an empty line rather than `# `: the prefix's trailing
    // spaces are written only ahead of text.
    let marker = prefix.trim_end_matches(' ');
    let spaces = prefix.strip_prefix(marker).unwrap_or("");
    for line in std::iter::once(first).chain(lines) {
        out.push_str(marker);
        if !line.is_empty() {
            out.push_str(spaces);
            out.extend(
                line.chars()
                    .map(|c| if is_toml_control(c) { ' ' } else { c }),
            );
        }
        out.push('\n');
    }
}

/// The characters TOML admits in neither a comment nor an unescaped basic
/// string: the C0 controls other than tab, and DEL.
fn is_toml_control(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{A}'..='\u{1F}' | '\u{7F}')
}

/// `text` as a TOML basic string, quotes included.
///
/// Always the `"…"` form and never a literal `'…'` one: the parent's
/// `strip_comment` tracks double quotes so that a `#` inside a value is not
/// read as a comment, and the basic form is the one that comparator
/// understands. The escapes are TOML's own — `\"`, `\\`, `\b`, `\t`, `\n`,
/// `\f`, `\r`, and `\uXXXX` for every other control character, U+007F
/// included — so the reader gets back exactly the text it was given.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if is_toml_control(c) => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `name` as a TOML table key: bare where TOML allows it, quoted otherwise.
///
/// A bare key is ASCII letters, digits, `-` and `_`. Every registered adapter
/// id is one, but `run_with` is a public seam that takes any ids, and a name
/// with a `.` in it written bare would nest a table rather than name one.
fn toml_key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if bare {
        name.to_owned()
    } else {
        toml_string(name)
    }
}

/// `units` as a TOML number that parses back to the same `f64`.
///
/// A whole number is written as an integer, which is how the operator wrote
/// `300` and what `Display` gives for `300.0`; `Display` never uses an
/// exponent, though, so `1e300` would become three hundred digits and read as
/// an integer too large to hold — a syntax error for the whole file. So
/// anything else takes `Debug`'s shortest round-trip form, which switches to
/// an exponent where the magnitude needs one, and the non-finite values take
/// TOML's own spellings, so that `config::read` refuses them by name instead
/// of the parser refusing the file. Whole numbers are bounded at 2^53, below
/// which every one of them is exact in an `f64` and inside an `i64`.
fn toml_number(units: f64) -> String {
    const EXACT_INTEGERS: f64 = 9_007_199_254_740_992.0; // 2^53
    if units.is_nan() {
        "nan".to_owned()
    } else if units.is_infinite() {
        if units.is_sign_positive() {
            "inf"
        } else {
            "-inf"
        }
        .to_owned()
    } else if units.fract() == 0.0 && units.abs() < EXACT_INTEGERS {
        format!("{units}")
    } else {
        format!("{units:?}")
    }
}

/// What the CLI prints.
///
/// Every agent gets exactly one line, usable or not, so that "no change" and
/// "could not tell" read differently: the first is the `unchanged:` line at the
/// end, the second is an agent's auth state on its own line, and a pool whose
/// kind is a default says so on the same line the file does under the pool.
pub(super) fn report(report: &ConnectReport) -> String {
    let mut out = String::new();
    for agent in &report.agents {
        match usable(agent) {
            Ok((discovery, pool)) => {
                let default = if discovery.shape.is_none() {
                    ", a default — not detected"
                } else {
                    ""
                };
                let _ = writeln!(
                    out,
                    "{}: {} — pool `{}` [{}{default}]",
                    agent.agent, discovery.auth, pool.name, pool.kind
                );
                for note in &discovery.notes {
                    let _ = writeln!(out, "  {note}");
                }
            }
            Err(reason) => {
                let _ = writeln!(out, "{}: skipped — {reason}", agent.agent);
            }
        }
    }
    for warning in &report.warnings {
        let _ = writeln!(out, "warning: {warning}");
    }
    match report.outcome {
        Wrote::Written => {
            let _ = writeln!(out, "wrote {}", report.path.display());
        }
        Wrote::Unchanged => {
            let _ = writeln!(out, "unchanged: {}", report.path.display());
        }
        Wrote::Refused => {
            // The proposed text is the whole answer to "what would --force
            // do": it is what the parent would write, the operator's
            // `profile`, `monthly_allowance` and `endpoint` already carried
            // into it from the pools of the same name. Saying so sends the
            // operator to the one place they can check what survives, rather
            // than to a promise this module cannot see the truth of — the
            // parent reads the existing file leniently, and keys in a file it
            // could not parse are not in the text below.
            let _ = writeln!(
                out,
                "{} already exists and differs from what connect would write. That file is \
                 hand-editable (§17), so it is not overwritten silently.\n\nWhat connect would \
                 write:\n{}\nRe-run with --force to replace it with the text above. Your \
                 `profile`, `monthly_allowance` and `endpoint` are carried into that text from \
                 the pools of the same name; anything of yours that is not in it is lost.",
                report.path.display(),
                indent(&report.content)
            );
        }
    }
    out
}

fn indent(text: &str) -> String {
    text.lines().map(|line| format!("  {line}\n")).collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::agent::AuthState;
    use crate::capacity::{PoolKind, Source};

    /// A usable agent: discovery answered and a pool was derived, as `run_with`
    /// builds one.
    fn usable_agent(id: &str, discovery: Discovery, pool: Pool) -> AgentReport {
        AgentReport {
            agent: id.to_owned(),
            outcome: Ok(discovery),
            pool: Some(pool),
        }
    }

    fn signed_in(shape: Option<PoolKind>, notes: &[&str]) -> Discovery {
        Discovery {
            auth: AuthState::Authenticated,
            models: Vec::new(),
            shape,
            notes: notes.iter().map(|note| (*note).to_owned()).collect(),
        }
    }

    /// The pools file parsed by the same library `config::read` parses it
    /// with, then one pool's table.
    fn parsed_pool(file: &str, name: &str) -> toml::Table {
        let table: toml::Table = toml::from_str(file)
            .unwrap_or_else(|error| panic!("the rendered file must parse: {error}\n{file}"));
        table
            .get("pools")
            .and_then(toml::Value::as_table)
            .and_then(|pools| pools.get(name))
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| panic!("no [pools.{name}] in:\n{file}"))
            .clone()
    }

    fn string_setting<'a>(pool: &'a toml::Table, key: &str) -> &'a str {
        pool.get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("`{key}` is not a string in {pool:?}"))
    }

    #[test]
    fn every_string_the_file_carries_parses_back_to_the_same_value() {
        // The operator's keys are parsed from their spelling and written back
        // in this module's; a Windows path in `profile` is the documented use
        // (§13 calls it a config-directory path) and it holds backslashes,
        // which are TOML escapes when written raw. `#` inside a value is the
        // parent's `strip_comment` case, and the rest are the characters TOML
        // requires escaped in a basic string.
        let nasty = [
            r"C:\Users\me\.claude-work",
            r#"say "hi" # not a comment"#,
            "it's",
            "tab\there",
            "line\nbreak",
            "carriage\rreturn",
            "bell\u{7}del\u{7f}",
            "ünïcödé — ok",
            "",
        ];
        for text in nasty {
            let mut pool = Pool::discovered(
                "claude-code",
                PoolKind::SubscriptionWindow,
                "claude-code",
                vec![Source::Signals, Source::SelfMetered],
            );
            pool.profile = Some(text.to_owned());
            pool.endpoint = Some(text.to_owned());
            let file = pools_file_at(
                &[usable_agent(
                    "claude-code",
                    signed_in(Some(PoolKind::SubscriptionWindow), &[]),
                    pool,
                )],
                "2026-09-05T00:00:00Z",
            );
            let parsed = parsed_pool(&file, "claude-code");
            assert_eq!(
                string_setting(&parsed, "profile"),
                text,
                "profile in:\n{file}"
            );
            assert_eq!(
                string_setting(&parsed, "endpoint"),
                text,
                "endpoint in:\n{file}"
            );
            assert_eq!(string_setting(&parsed, "kind"), "subscription-window");
            assert_eq!(string_setting(&parsed, "agent"), "claude-code");
            assert_eq!(string_setting(&parsed, "window"), "5h");
            assert_eq!(
                parsed
                    .get("sources")
                    .and_then(toml::Value::as_array)
                    .map(Vec::len),
                Some(2),
                "sources in:\n{file}"
            );
        }
    }

    #[test]
    fn a_pool_name_that_is_not_a_bare_key_is_quoted_rather_than_nested() {
        // `run_with` takes any ids; `default_pool_name` makes the pool name the
        // id. Written bare, `a.b` names table `b` under table `a`, and a space
        // is a parse error.
        for name in ["with.dot", "with space", "quote\"d"] {
            let pool = Pool::discovered(name, PoolKind::Credits, name, vec![Source::Signals]);
            let file = pools_file_at(
                &[usable_agent(
                    name,
                    signed_in(Some(PoolKind::Credits), &[]),
                    pool,
                )],
                "2026-09-05T00:00:00Z",
            );
            let parsed = parsed_pool(&file, name);
            assert_eq!(string_setting(&parsed, "agent"), name, "{file}");
        }
    }

    #[test]
    fn a_discovery_note_with_a_line_break_stays_inside_the_comment() {
        // A note is a CLI's output: the Codex adapter quotes up to 120
        // characters of an unrecognised `login status` answer, line breaks
        // included. Written raw, the second line reaches the parser as a
        // setting — here one that would flip `weekly`. The test's oracle is
        // the parse and the value, not the comment's spelling.
        let discovery = signed_in(
            Some(PoolKind::SubscriptionWindow),
            &["first line\nweekly = false\r\nthird\rline with \u{1} control"],
        );
        let pool = Pool::discovered(
            "codex",
            PoolKind::SubscriptionWindow,
            "codex",
            vec![Source::Signals],
        );
        let file = pools_file_at(
            &[usable_agent("codex", discovery, pool)],
            "2026-09-05T00:00:00Z",
        );
        let parsed = parsed_pool(&file, "codex");
        assert_eq!(
            parsed.get("weekly").and_then(toml::Value::as_bool),
            Some(true),
            "the note's second line must not become a setting:\n{file}"
        );
        for line in file.lines() {
            assert!(
                line.is_empty()
                    || line.starts_with('#')
                    || line.starts_with('[')
                    || line.contains(" = "),
                "a line that is neither comment, header nor setting: {line:?}\n{file}"
            );
            assert!(
                !line.chars().any(is_toml_control),
                "a control character survived into the file: {line:?}"
            );
        }
        assert!(
            file.contains("#   first line\n#   weekly = false\n#   third line with   control"),
            "each line of the note is its own comment line:\n{file}"
        );
    }

    #[test]
    fn an_allowance_is_written_as_a_number_the_reader_accepts() {
        // `Display` for `f64` never uses an exponent: `1e300` is three hundred
        // digits, which the reader takes for an integer and refuses as too
        // large — a syntax error for the file, over a value `config::read`
        // accepts (finite, positive). The whole numbers keep the operator's
        // integer spelling, which the parent's test reads back by text.
        let cases: [(f64, &str, toml::Value); 5] = [
            (300.0, "300", toml::Value::Integer(300)),
            (300.5, "300.5", toml::Value::Float(300.5)),
            (1e300, "1e300", toml::Value::Float(1e300)),
            (1e16, "1e16", toml::Value::Float(1e16)),
            (f64::INFINITY, "inf", toml::Value::Float(f64::INFINITY)),
        ];
        for (units, spelled, value) in cases {
            assert_eq!(toml_number(units), spelled);
            let mut pool = Pool::discovered("copilot", PoolKind::Credits, "copilot", Vec::new());
            pool.monthly_allowance = Allowance::Units(units);
            let file = pools_file_at(
                &[usable_agent(
                    "copilot",
                    signed_in(Some(PoolKind::Credits), &[]),
                    pool,
                )],
                "2026-09-05T00:00:00Z",
            );
            let parsed = parsed_pool(&file, "copilot");
            assert_eq!(parsed.get("monthly_allowance"), Some(&value), "{file}");
        }
        // `nan` parses as a float TOML admits, so the reader refuses it by
        // name (`is_finite`) rather than the parser refusing the file.
        assert_eq!(toml_number(f64::NAN), "nan");
        let auto = Pool::discovered("copilot", PoolKind::Credits, "copilot", Vec::new());
        let file = pools_file_at(
            &[usable_agent(
                "copilot",
                signed_in(Some(PoolKind::Credits), &[]),
                auto,
            )],
            "2026-09-05T00:00:00Z",
        );
        assert!(
            parsed_pool(&file, "copilot")
                .get("monthly_allowance")
                .is_none(),
            "`auto` is the reader's default and is not written:\n{file}"
        );
    }

    #[test]
    fn only_the_written_by_line_moves_between_two_timestamps() {
        // The parent's `stable_content` filters exactly one line by prefix; if
        // the timestamp ever appeared on a second line, or the first line
        // stopped starting with the prefix, every re-connect would rewrite.
        let agents = [usable_agent(
            "claude-code",
            signed_in(Some(PoolKind::SubscriptionWindow), &["a note"]),
            Pool::discovered(
                "claude-code",
                PoolKind::SubscriptionWindow,
                "claude-code",
                vec![Source::Signals],
            ),
        )];
        let first = pools_file_at(&agents, "2026-09-05T00:00:00Z");
        let second = pools_file_at(&agents, "2026-09-06T12:34:56Z");
        let differing: Vec<(&str, &str)> = first
            .lines()
            .zip(second.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(first.lines().count(), second.lines().count());
        assert_eq!(differing.len(), 1, "{differing:?}");
        let (moved, _) = differing.first().copied().expect("one differing line");
        assert!(moved.starts_with(WRITTEN_BY), "{moved}");
        assert!(first.starts_with(WRITTEN_BY), "{first}");
        assert!(moved.contains("2026-09-05T00:00:00Z"), "{moved}");
    }

    #[test]
    fn the_header_prefix_is_the_literal_the_parent_filters_by() {
        // `stable_content` in `src/connect.rs` spells this prefix as its own
        // literal, and the parent's test of the `Unchanged` answer runs two
        // connects within one second, so the same timestamp makes them equal
        // whatever the prefix says — measured at the base: renaming the
        // prefix failed no test in the suite. Until the parent reads
        // `WRITTEN_BY` (SWEEP-CONNECT-RENDER-011), this literal is the guard.
        assert_eq!(WRITTEN_BY, "# Written by `upstroke connect`");
        assert!(
            pools_file_at(&[], "2026-09-05T00:00:00Z")
                .starts_with("# Written by `upstroke connect` v"),
            "the prefix is the first thing in the file"
        );
    }

    #[test]
    fn the_summary_and_the_file_agree_on_which_agents_are_usable() {
        // The type admits four combinations of outcome and pool; the parent
        // produces two. Both renderers now decide usability once, so an agent
        // has a pool in the file exactly when the summary says it has one,
        // and every agent is named in the summary exactly once — the two
        // unreachable combinations included, where the summary used to say
        // nothing for one of them.
        let pool = || Pool::discovered("x", PoolKind::Credits, "x", vec![Source::Signals]);
        let could_not_tell = || Discovery::unknown().with_note("no auth query exists");
        let agents = vec![
            AgentReport {
                agent: "ok-some".to_owned(),
                outcome: Ok(could_not_tell()),
                pool: Some(pool()),
            },
            AgentReport {
                agent: "err-none".to_owned(),
                outcome: Err("binary not found on PATH".to_owned()),
                pool: None,
            },
            AgentReport {
                agent: "ok-none".to_owned(),
                outcome: Ok(could_not_tell()),
                pool: None,
            },
            AgentReport {
                agent: "err-some".to_owned(),
                outcome: Err("probe failed".to_owned()),
                pool: Some(pool()),
            },
        ];
        let file = pools_file_at(&agents, "2026-09-05T00:00:00Z");
        let summary = report(&ConnectReport {
            path: PathBuf::from("pools.toml"),
            outcome: Wrote::Unchanged,
            content: file.clone(),
            agents,
            warnings: Vec::new(),
        });
        let table: toml::Table = toml::from_str(&file).expect("parses");
        let pools = table
            .get("pools")
            .and_then(toml::Value::as_table)
            .expect("[pools]");
        assert_eq!(pools.len(), 1, "one pool from four agents:\n{file}");
        assert_eq!(pools.keys().next().map(String::as_str), Some("x"));
        let summary_lines: Vec<&str> = summary.lines().collect();
        for (id, expected) in [
            (
                "ok-some",
                "ok-some: auth state could not be determined — pool `x` [credits, a default — not detected]",
            ),
            ("err-none", "err-none: skipped — binary not found on PATH"),
            (
                "ok-none",
                "ok-none: skipped — discovery answered but derived no pool",
            ),
            ("err-some", "err-some: skipped — probe failed"),
        ] {
            let named: Vec<&&str> = summary_lines
                .iter()
                .filter(|line| line.starts_with(&format!("{id}:")))
                .collect();
            assert_eq!(named, vec![&expected], "{summary}");
        }
        // "No change" and "could not tell" are two different lines.
        assert!(summary.contains("\nunchanged: pools.toml\n"), "{summary}");
        assert!(
            file.contains("# ok-some: auth state could not be determined\n"),
            "{file}"
        );
        assert!(
            file.contains(
                "# err-some: not usable on this machine, so no pool was written for it.\n"
            ),
            "{file}"
        );
    }

    #[test]
    fn a_defaulted_kind_is_marked_in_the_summary_as_it_is_in_the_file() {
        // The file already says so under the pool; the summary is what the
        // operator sees when connect writes nothing, and it showed `[credits]`
        // as though detected.
        for (shape, marked) in [(None, true), (Some(PoolKind::Credits), false)] {
            let agents = vec![usable_agent(
                "copilot",
                signed_in(shape, &[]),
                Pool::discovered("copilot", PoolKind::Credits, "copilot", Vec::new()),
            )];
            let file = pools_file_at(&agents, "2026-09-05T00:00:00Z");
            let summary = report(&ConnectReport {
                path: PathBuf::from("pools.toml"),
                outcome: Wrote::Written,
                content: file.clone(),
                agents,
                warnings: Vec::new(),
            });
            assert_eq!(
                file.contains(&format!("#   {KIND_IS_A_DEFAULT}\n")),
                marked,
                "{file}"
            );
            assert_eq!(
                summary.contains("[credits, a default — not detected]"),
                marked,
                "{summary}"
            );
            assert_eq!(summary.contains("[credits]"), !marked, "{summary}");
        }
    }

    #[test]
    fn a_refusal_shows_the_text_force_would_write_and_says_what_it_keeps() {
        let agents = vec![usable_agent(
            "claude-code",
            signed_in(Some(PoolKind::SubscriptionWindow), &[]),
            Pool::discovered(
                "claude-code",
                PoolKind::SubscriptionWindow,
                "claude-code",
                vec![Source::Signals],
            ),
        )];
        let content = pools_file_at(&agents, "2026-09-05T00:00:00Z");
        let summary = report(&ConnectReport {
            path: PathBuf::from("pools.toml"),
            outcome: Wrote::Refused,
            content: content.clone(),
            agents,
            warnings: vec!["a warning".to_owned()],
        });
        assert!(
            summary.contains("pools.toml already exists and differs"),
            "{summary}"
        );
        assert!(summary.contains("\nwarning: a warning\n"), "{summary}");
        assert!(summary.contains("Re-run with --force"), "{summary}");
        for key in ["`profile`", "`monthly_allowance`", "`endpoint`"] {
            assert!(summary.contains(key), "{key} named: {summary}");
        }
        for line in content.lines() {
            assert!(
                summary.contains(&format!("\n  {line}\n")),
                "every proposed line is shown, indented: {line:?}\n{summary}"
            );
        }
        assert!(!summary.contains("\nwrote "), "{summary}");
        assert!(!summary.contains("\nunchanged:"), "{summary}");
    }
}
