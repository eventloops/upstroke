//! Extended notes: `docs/internals/connect/render.md`

#![deny(clippy::disallowed_methods, clippy::disallowed_macros)]

use std::fmt::Write as _;

use crate::agent::Discovery;
use crate::capacity::{Allowance, Pool};
use crate::util;

use super::{AgentReport, ConnectReport, Wrote};

pub(super) const WRITTEN_BY: &str = "# Written by `upstroke connect`";

pub(super) fn pools_file(agents: &[AgentReport]) -> String {
    pools_file_at(agents, &util::rfc3339_utc_now())
}

pub(super) fn pools_file_at(agents: &[AgentReport], written_at: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{WRITTEN_BY} v{} on {written_at}.",
        env!("CARGO_PKG_VERSION")
    );

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

const KIND_IS_A_DEFAULT: &str =
    "kind below is a default, not something detected — change it if your plan differs";

fn usable(report: &AgentReport) -> Result<(&Discovery, &Pool), &str> {
    match (&report.outcome, &report.pool) {
        (Ok(discovery), Some(pool)) => Ok((discovery, pool)),
        (Err(error), _) => Err(error),
        (Ok(_), None) => Err("discovery answered but derived no pool"),
    }
}

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

fn comment(out: &mut String, prefix: &str, text: &str) {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");

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

fn is_toml_control(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{A}'..='\u{1F}' | '\u{7F}')
}

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

fn toml_number(units: f64) -> String {
    const EXACT_INTEGERS: f64 = 9_007_199_254_740_992.0;
    if units.fract() == 0.0 && units < EXACT_INTEGERS {
        format!("{units}")
    } else {
        format!("{units:?}")
    }
}

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
                    let mut lines = note.lines();
                    let first = lines.next().unwrap_or("");
                    for line in std::iter::once(first).chain(lines) {
                        out.push_str("  ");
                        out.extend(line.chars().map(|c| if c.is_control() { ' ' } else { c }));
                        out.push('\n');
                    }
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
            let _ = writeln!(
                out,
                "{} already exists and differs from what connect would write. That file is \
                 hand-editable (§17), so it is not overwritten silently.\n\nWhat connect would \
                 write:\n{}\nRe-run with --force to replace it with the text above. Check that \
                 text for your own keys first — `profile`, `monthly_allowance` and `endpoint` — \
                 because connect carries them from pools of the same name only when it can parse \
                 the existing file, and anything of yours that is not in the text above is lost.",
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
        let cases: [(f64, &str, toml::Value); 4] = [
            (300.0, "300", toml::Value::Integer(300)),
            (300.5, "300.5", toml::Value::Float(300.5)),
            (1e300, "1e300", toml::Value::Float(1e300)),
            (1e16, "1e16", toml::Value::Float(1e16)),
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
    fn the_header_starts_with_the_shared_timestamp_prefix() {
        assert!(
            pools_file_at(&[], "2026-09-05T00:00:00Z").starts_with(&format!("{WRITTEN_BY} v")),
            "the prefix is the first thing in the file"
        );
    }

    #[test]
    fn the_summary_and_the_file_agree_on_which_agents_are_usable() {
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
        assert!(
            summary.contains("only when it can parse the existing file"),
            "carrying is stated as a condition, not a promise: {summary}"
        );
        assert!(!summary.contains("are carried into"), "{summary}");
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
