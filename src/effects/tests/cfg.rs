//! The cfg census: every `cfg` occurrence in the tree, and the runners that
//! actually compile it.
//!
//! The predecessor collected `target_os = "..."` names wherever they appeared
//! at a code position and read each name as a platform demanding its own Clippy
//! runner. This module decides predicates instead, against the valuations
//! `super::ci_model`'s [`CI_TARGETS`] say CI sets — completely, so an unmodelled
//! name is a hard failure rather than an optimistic guess, and per invocation,
//! because `--all-targets` compiles the library twice and merging the two would
//! make `all(test, not(test))` look reachable.
//!
//! Three distinctions carry the weight, and each is a claim an earlier version
//! got wrong:
//!
//!   * **Not every cfg gates.** `cfg!(P)` is an expression and
//!     `#[cfg_attr(P, attr)]` conditions an attribute; the code around either is
//!     compiled everywhere. [`CfgForm`] keeps the three apart, and only
//!     [`CfgForm::Gate`] is a platform demand.
//!   * **An item's predicate is not the attribute written on it.** Stacked
//!     `#[cfg]`s conjoin, and so does every enclosing guard — the module block
//!     it sits in, and, for a whole-file module, the `#[cfg(test)] mod name;`
//!     that declares the file. [`CfgSite::written`] and [`CfgSite::rendered`]
//!     are both kept so the difference is visible.
//!   * **Position and text come from different views.** Nesting and brace depth
//!     read the blanked source, where a `cfg(` in prose or in a string literal
//!     is spaces; the predicate text reads the raw span, because blanking erases
//!     the platform name along with the quotes.
//!
//! The `#[test]` wrappers that drive this stay in `super`, together with the
//! join against the workflow contract: this module is the census, not the
//! harness, and every name in it is deliberately not a test name.
//!
//! The three effect denials are **restored** here rather than inherited.
//! `super`'s module-level allowance exists because that file drives
//! `clippy-driver` over fixtures it has to create; this module reads the tree
//! it is handed and writes nothing, so the allowance has no business reaching
//! it.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::LazyLock;

use super::ci_model::CI_TARGETS;
use crate::effects::blank_comments_and_strings;

/// A cfg predicate, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CfgPred {
    All(Vec<CfgPred>),
    Any(Vec<CfgPred>),
    Not(Box<CfgPred>),
    Flag(String),
    Pair(String, String),
}

impl CfgPred {
    /// Canonical text, so two spellings of one predicate are one row.
    pub(super) fn render(&self) -> String {
        match self {
            CfgPred::All(list) => format!("all({})", Self::render_list(list)),
            CfgPred::Any(list) => format!("any({})", Self::render_list(list)),
            CfgPred::Not(inner) => format!("not({})", inner.render()),
            CfgPred::Flag(name) => name.clone(),
            CfgPred::Pair(key, value) => format!("{key} = \"{value}\""),
        }
    }

    fn render_list(list: &[CfgPred]) -> String {
        list.iter()
            .map(CfgPred::render)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The conjunction of `parts`, without an `all(...)` around a single one.
    fn conjunction(mut parts: Vec<CfgPred>) -> CfgPred {
        match parts.len() {
            1 => parts.remove(0),
            _ => CfgPred::All(parts),
        }
    }
}

/// The bare cfg flags this census models. Anything else is a hard failure.
///
/// The list is short because the tree is: `test`, `unix` and `windows` are every
/// bare flag any `#[cfg]` in `src/` and `examples/` names. Keeping it exactly
/// that short is the point -- a census that guesses at `debug_assertions` or
/// `miri` would be asserting what CI sets rather than reading it, and the right
/// answer to a new flag is to decide it here, once, in front of a reviewer.
const MODELLED_FLAGS: [&str; 3] = ["test", "unix", "windows"];

/// The `key = "value"` cfg keys this census models. Same rule.
const MODELLED_KEYS: [&str; 7] = [
    "target_arch",
    "target_endian",
    "target_env",
    "target_family",
    "target_os",
    "target_pointer_width",
    "target_vendor",
];

/// One compilation's **complete** cfg valuation.
///
/// Complete is the load-bearing word. A name this valuation does not carry is
/// not "unknown": rustc leaves it unset, so `cfg(name)` is **false**. That is
/// only sound while the set of names is closed, which is what [`MODELLED_FLAGS`]
/// and [`MODELLED_KEYS`] close and what [`holds`] refuses to guess past.
struct Valuation {
    runner: &'static str,
    /// What the invocation is, for a failure message that can be acted on.
    invocation: String,
    flags: BTreeSet<&'static str>,
    keys: &'static [(&'static str, &'static str)],
}

/// Every compilation CI performs, as a valuation.
///
/// Two per runner, because `cargo clippy --all-targets` and `cargo test
/// --all-targets` each compile the library twice -- once as a library, once as a
/// test harness with `test` set. They are kept apart rather than merged: merging
/// would set `test` and `not(test)` in one valuation and make `all(test,
/// not(test))` look reachable.
fn ci_valuations() -> Vec<Valuation> {
    let mut out = Vec::new();
    for target in &CI_TARGETS {
        for extra in target.per_invocation_flags {
            let mut flags: BTreeSet<&'static str> = target.flags.iter().copied().collect();
            flags.extend(extra.iter().copied());
            let invocation = if extra.is_empty() {
                "the library and binary targets".to_owned()
            } else {
                format!("the {} target(s)", extra.join(" + "))
            };
            out.push(Valuation {
                runner: target.runner,
                invocation,
                flags,
                keys: target.keys,
            });
        }
    }
    out
}

/// Whether `pred` holds under `valuation`, or the name that made it undecidable.
///
/// There is no third answer. An unmodelled name returns `Err` and fails the
/// census, which is the difference between this and the version it replaces:
/// that one returned `Unknown` and the caller counted `Unknown` as coverage, so
/// a predicate nobody could decide was reported as compiled by every runner.
fn holds(pred: &CfgPred, valuation: &Valuation) -> Result<bool, String> {
    match pred {
        CfgPred::All(list) => {
            for inner in list {
                if !holds(inner, valuation)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        CfgPred::Any(list) => {
            for inner in list {
                if holds(inner, valuation)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        CfgPred::Not(inner) => Ok(!holds(inner, valuation)?),
        CfgPred::Flag(name) => {
            if !MODELLED_FLAGS.contains(&name.as_str()) {
                return Err(format!(
                    "`{name}` is a bare cfg flag this census does not model. Decide what \
                     each CI invocation sets it to and add it to `MODELLED_FLAGS`; guessing \
                     is how the predecessor reported bodies as covered that nothing compiles."
                ));
            }
            Ok(valuation.flags.contains(name.as_str()))
        }
        CfgPred::Pair(key, value) => {
            if !MODELLED_KEYS.contains(&key.as_str()) {
                return Err(format!(
                    "`{key} = \"{value}\"` names a cfg key this census does not model. Add it \
                     to `MODELLED_KEYS` and give every target tuple a value for it."
                ));
            }
            Ok(valuation
                .keys
                .iter()
                .any(|(name, set)| name == key && set == value))
        }
    }
}

/// The runners that **actually compile** a body guarded by `pred`.
///
/// A runner is in the set when some invocation it performs makes the predicate
/// true. Not "might" -- the predecessor's `might` is what let an undecidable
/// predicate claim three platforms.
pub(super) fn compiled_by(pred: &CfgPred) -> Result<BTreeSet<&'static str>, String> {
    let mut out = BTreeSet::new();
    for valuation in ci_valuations() {
        let decided = holds(pred, &valuation).map_err(|error| {
            format!(
                "{error} (deciding `{}` for {} on `{}`)",
                pred.render(),
                valuation.invocation,
                valuation.runner
            )
        })?;
        if decided {
            out.insert(valuation.runner);
        }
    }
    Ok(out)
}

/// A recursive-descent reader for the cfg predicate grammar.
///
/// Hand-written on purpose. The alternative is a Rust parser crate, and the only
/// dependency this crate was authorised to add is the YAML one; the grammar
/// `cfg` accepts is small enough that reading it exactly costs less than
/// carrying `syn`, and every form it accepts is exercised below.
struct CfgReader<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> CfgReader<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, at: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    /// Whitespace and comments alike.
    ///
    /// The reader skips comments itself rather than being handed a
    /// comment-blanked view, because the repository's comment blanker *deletes*
    /// comment bytes instead of replacing them -- it does not preserve
    /// positions, and every span here is a byte range. `#[cfg(all(\n // why\n
    /// unix))]` is legal Rust and reads correctly through this.
    fn skip_space(&mut self) {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.at += 1;
            }
            let rest = &self.text[self.at..];
            if rest.starts_with("//") {
                self.at += rest.find('\n').unwrap_or(rest.len());
            } else if rest.starts_with("/*") {
                let bytes = rest.as_bytes();
                let mut depth = 0_usize;
                let mut cursor = 0;
                while cursor < bytes.len() {
                    if bytes[cursor..].starts_with(b"/*") {
                        depth += 1;
                        cursor += 2;
                    } else if bytes[cursor..].starts_with(b"*/") {
                        depth -= 1;
                        cursor += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        cursor += 1;
                    }
                }
                self.at += cursor;
            } else {
                return;
            }
        }
    }

    fn ident(&mut self) -> &'a str {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.at += 1;
        }
        &self.text[start..self.at]
    }

    /// A cfg value: any Rust string literal, raw or escaped.
    ///
    /// `#[cfg(target_os = r"linux")]` and `#[cfg(target_os = "li\x6eux")]` are
    /// both valid Rust naming the same platform, and a reader that handles only
    /// `"..."` with a backslash passed through verbatim decodes the second to a
    /// different platform than rustc does. Neither form appears in this tree
    /// today, which is exactly why the control fixture carries both: a lexical
    /// gap that nothing exercises is a gap nobody notices.
    fn value(&mut self) -> Result<String, String> {
        if self.peek() == Some(b'r') {
            return self.raw_value();
        }
        if self.peek() != Some(b'"') {
            return Err(format!("expected a string value at byte {}", self.at));
        }
        self.at += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated value".to_owned()),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    self.escape(&mut out)?;
                }
                Some(_) => {
                    let ch = self.text[self.at..]
                        .chars()
                        .next()
                        .expect("a character at a character boundary");
                    self.at += ch.len_utf8();
                    out.push(ch);
                }
            }
        }
    }

    /// One escape sequence, already past its backslash.
    fn escape(&mut self, out: &mut String) -> Result<(), String> {
        let Some(byte) = self.peek() else {
            return Err("a value ends in a backslash".to_owned());
        };
        self.at += 1;
        let simple = match byte {
            b'n' => Some('\n'),
            b'r' => Some('\r'),
            b't' => Some('\t'),
            b'0' => Some('\0'),
            b'\\' => Some('\\'),
            b'\'' => Some('\''),
            b'"' => Some('"'),
            _ => None,
        };
        if let Some(ch) = simple {
            out.push(ch);
            return Ok(());
        }
        match byte {
            // `\x41`: exactly two hex digits, and only ASCII in a string.
            b'x' => {
                let digits = self.take_hex(2)?;
                let code = u32::from_str_radix(&digits, 16)
                    .map_err(|error| format!("`\\x{digits}`: {error}"))?;
                char::from_u32(code)
                    .filter(char::is_ascii)
                    .map(|ch| out.push(ch))
                    .ok_or_else(|| format!("`\\x{digits}` is not an ASCII escape"))
            }
            // `\u{1F600}`: one to six hex digits in braces.
            b'u' => {
                if self.peek() != Some(b'{') {
                    return Err("`\\u` is not followed by `{`".to_owned());
                }
                self.at += 1;
                let start = self.at;
                while self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                    self.at += 1;
                }
                let digits = self.text[start..self.at].to_owned();
                if self.peek() != Some(b'}') {
                    return Err(format!("`\\u{{{digits}` is not closed"));
                }
                self.at += 1;
                let code = u32::from_str_radix(&digits, 16)
                    .map_err(|error| format!("`\\u{{{digits}}}`: {error}"))?;
                char::from_u32(code)
                    .map(|ch| out.push(ch))
                    .ok_or_else(|| format!("`\\u{{{digits}}}` is not a character"))
            }
            // A backslash before a newline eats the following whitespace.
            b'\n' => {
                while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                    self.at += 1;
                }
                Ok(())
            }
            other => Err(format!("`\\{}` is not an escape", char::from(other))),
        }
    }

    fn take_hex(&mut self, count: usize) -> Result<String, String> {
        let start = self.at;
        for _ in 0..count {
            if !self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("expected {count} hex digits at byte {start}"));
            }
            self.at += 1;
        }
        Ok(self.text[start..self.at].to_owned())
    }

    /// `r"..."`, `r#"..."#`, `r##"..."##` — no escapes inside.
    fn raw_value(&mut self) -> Result<String, String> {
        self.at += 1;
        let hashes_at = self.at;
        while self.peek() == Some(b'#') {
            self.at += 1;
        }
        let hashes = self.at - hashes_at;
        if self.peek() != Some(b'"') {
            return Err(format!("`r` at byte {hashes_at} opens no raw string"));
        }
        self.at += 1;
        let close: String = std::iter::once('"')
            .chain(std::iter::repeat_n('#', hashes))
            .collect();
        let rest = &self.text[self.at..];
        let Some(end) = rest.find(&close) else {
            return Err("unterminated raw value".to_owned());
        };
        let out = rest[..end].to_owned();
        self.at += end + close.len();
        Ok(out)
    }

    fn predicate(&mut self) -> Result<CfgPred, String> {
        self.skip_space();
        let name = self.ident().to_owned();
        if name.is_empty() {
            return Err(format!("expected a cfg identifier at byte {}", self.at));
        }
        self.skip_space();
        match self.peek() {
            Some(b'(') => {
                self.at += 1;
                let inner = self.list()?;
                match name.as_str() {
                    "all" => Ok(CfgPred::All(inner)),
                    "any" => Ok(CfgPred::Any(inner)),
                    "not" if inner.len() == 1 => Ok(CfgPred::Not(Box::new(
                        inner.into_iter().next().expect("one predicate"),
                    ))),
                    "not" => Err(format!("`not` takes one predicate, given {}", inner.len())),
                    other => Err(format!("unknown cfg operator `{other}`")),
                }
            }
            Some(b'=') => {
                self.at += 1;
                self.skip_space();
                Ok(CfgPred::Pair(name, self.value()?))
            }
            _ => Ok(CfgPred::Flag(name)),
        }
    }

    fn list(&mut self) -> Result<Vec<CfgPred>, String> {
        let mut out = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some(b')') => {
                    self.at += 1;
                    return Ok(out);
                }
                None => return Err("unterminated predicate list".to_owned()),
                Some(_) => {}
            }
            out.push(self.predicate()?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b')') => {
                    self.at += 1;
                    return Ok(out);
                }
                found => {
                    return Err(format!(
                        "expected `,` or `)` at byte {}, found {found:?}",
                        self.at
                    ));
                }
            }
        }
    }
}

/// Parse the inside of a `cfg(...)`, or of a `cfg_attr(...)` up to its first
/// comma.
pub(super) fn parse_cfg(inside: &str, attribute_form: bool) -> Result<CfgPred, String> {
    let mut reader = CfgReader::new(inside);
    let pred = reader.predicate()?;
    reader.skip_space();
    match reader.peek() {
        None => Ok(pred),
        Some(b',') if attribute_form => Ok(pred),
        Some(_) => Err(format!(
            "`cfg` takes one predicate; trailing input at byte {}",
            reader.at
        )),
    }
}

/// What a `cfg` occurrence does to the code around it.
///
/// Only one of the three gates, and conflating them is the defect this
/// distinction repairs: a census that counts all three demands a Clippy runner
/// for platforms whose bodies are compiled everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CfgForm {
    /// `#[cfg(P)]` and `#![cfg(P)]`. The item exists only where `P` holds, so
    /// this is the only form whose predicate is a platform demand.
    Gate,
    /// `#[cfg_attr(P, attr)]`. The **attribute** is conditional; the item is
    /// compiled everywhere. `#[cfg_attr(not(windows), allow(dead_code))]` in
    /// this tree does not make its function Windows-only.
    Attribute,
    /// `cfg!(P)`. A compile-time boolean *expression*: both arms of the `if`
    /// around it are compiled and type-checked on every platform.
    Macro,
}

/// One `cfg` occurrence, with the predicate that actually decides it.
pub(super) struct CfgSite {
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) form: CfgForm,
    /// The predicate as written on this occurrence.
    pub(super) written: String,
    /// The predicate that decides whether the item is compiled: `written`
    /// conjoined with every stacked attribute, every enclosing guard, and the
    /// file's own guard when it is a whole-file module. Equal to `written` for
    /// the non-gating forms, which decide nothing.
    pub(super) rendered: String,
    pub(super) pred: CfgPred,
}

/// The index of the byte closing the group `open` at `at`.
fn balanced(bytes: &[u8], at: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0_usize;
    let mut cursor = at;
    while cursor < bytes.len() {
        if bytes[cursor] == open {
            depth += 1;
        } else if bytes[cursor] == close {
            depth -= 1;
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }
    None
}

/// The directory a file's `mod name;` declarations resolve inside.
///
/// **The crate roots come from the manifest**, through the one inventory
/// `census_domain` resolves against. This used to read `matches!(stem, "mod" |
/// "lib" | "main")` — a second, lexical copy of the rule that census had
/// already stopped trusting, and the copy that was still wrong in this tree:
/// `examples/probe.rs` is an `example` target, so it is a crate root whose
/// children live in `examples/`, and the stem rule answered `examples/probe`.
/// An arbitrary `[[bin]] path` is the same error with more room in it.
/// `PR5D-VISIBILITY-CHECK-DUPLICATED` is the standing entry for a rule written
/// twice; this is the second copy retired rather than re-synchronised.
pub(super) fn module_dir(path: &str) -> String {
    let (parent, file) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = file.strip_suffix(".rs").unwrap_or(file);
    if stem == "mod" || crate::effects::tests::crate_roots().is_root_relative(path) {
        parent.to_owned()
    } else if parent.is_empty() {
        stem.to_owned()
    } else {
        format!("{parent}/{stem}")
    }
}

/// Every `cfg` occurrence in `sources`, and every one that could not be read.
///
/// Two passes, because a file's guard is written in another file. Pass one reads
/// the `#[cfg(P)] mod name;` declarations and resolves each to the file it
/// governs; pass two scans every file with the guard it inherited. The files
/// [`WHOLE_FILE_TEST_MODULES`] lists exist only under a `cfg(test)` module
/// declaration -- the population
/// `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
/// resolves independently -- and a census that missed it would read every
/// predicate in them as unconditional.
///
/// **All but one of them, and the difference is deliberate.** This pass
/// reads `#[cfg(P)] mod name;` -- the attribute is on the declaration -- and
/// `agent/proc/test_support/readiness.rs` is declared with no attribute at all,
/// inside an inline `#[cfg(test)]` module. `census_domain` resolves that
/// ancestry and this pass does not, because the two answer different questions:
/// that one decides which files are test code, this one decides what predicate
/// each `cfg` occurrence is under. The floor below is `>=`, and readiness.rs
/// carries no `cfg` occurrence for this census to misattribute -- measured, and
/// asserted by the census test above staying green with the file in its domain.
///
/// Two views of each file, and which one answers which question is the part that
/// has cost this repository time before:
///
///   * **position, nesting and brace depth** come from
///     `blank_comments_and_strings`, where a `cfg(` inside prose or inside a
///     string literal is spaces, and so is a brace. That is what keeps this
///     census off its own explanatory comments -- an earlier version reported
///     `freebsd` quoted from the paragraph beside it.
///   * **the predicate text** is the raw span, because
///     `blank_comments_and_strings` erases the platform name along with the
///     quotes: reading the name from the blanked view is why the first version
///     found only `windows`. [`CfgReader`] skips comments on its own, which is
///     the part the comment blanker cannot do here -- it deletes comment bytes
///     rather than blanking them, so it does not preserve the positions this
///     scan is built on.
pub(super) fn cfg_regions(sources: &[(String, String)]) -> (Vec<CfgSite>, Vec<String>) {
    let mut unreadable = Vec::new();
    let known: BTreeSet<&str> = sources.iter().map(|(path, _)| path.as_str()).collect();

    // Pass one: which file each `#[cfg(P)] mod name;` governs.
    let mut declared: Vec<(String, String, CfgPred)> = Vec::new();
    for (path, source) in sources {
        let mut declarations = Vec::new();
        scan_file(
            path,
            source,
            None,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut declarations,
        );
        for (name, pred) in declarations {
            let dir = module_dir(path);
            let candidates = [format!("{dir}/{name}.rs"), format!("{dir}/{name}/mod.rs")];
            let present: Vec<&String> = candidates
                .iter()
                .filter(|candidate| known.contains(candidate.as_str()))
                .collect();
            match present.as_slice() {
                [child] => declared.push((path.clone(), (*child).clone(), pred)),
                [] => unreadable.push(format!(
                    "{path} declares `#[cfg({})] mod {name};` and neither {candidates:?} is \
                     in the scanned domain, so the guard on that file is unaccounted for",
                    pred.render()
                )),
                _ => unreadable.push(format!(
                    "{path} declares `mod {name};` and {} candidate files exist",
                    present.len()
                )),
            }
        }
    }

    // A guarded file may itself declare a guarded module, so the guards compose.
    // Bounded rather than recursive, and the bound is checked: a cycle here
    // would otherwise be an infinite loop inside a test.
    let mut guards: BTreeMap<String, CfgPred> = BTreeMap::new();
    let mut settled = false;
    for _ in 0..8 {
        let mut next: BTreeMap<String, CfgPred> = BTreeMap::new();
        for (parent, child, pred) in &declared {
            let mut parts: Vec<CfgPred> = Vec::new();
            if let Some(inherited) = guards.get(parent) {
                parts.push(inherited.clone());
            }
            parts.push(pred.clone());
            next.insert(child.clone(), CfgPred::conjunction(parts));
        }
        if next == guards {
            settled = true;
            break;
        }
        guards = next;
    }
    if !settled {
        unreadable.push(
            "the module guards did not settle in eight rounds; `mod` declarations may be \
             cyclic and no file's guard can be trusted"
                .to_owned(),
        );
    }

    // Pass two: every occurrence, under the guard its file inherited.
    let mut sites = Vec::new();
    for (path, source) in sources {
        scan_file(
            path,
            source,
            guards.get(path),
            &mut sites,
            &mut unreadable,
            &mut Vec::new(),
        );
    }
    (sites, unreadable)
}

/// One file's occurrences.
///
/// `declarations` collects `#[cfg(P)] mod name;` for the caller's first pass;
/// `sites` and `unreadable` are the second pass's output. A pass wanting only
/// one of the two hands the other an empty vector it then discards.
fn scan_file(
    path: &str,
    source: &str,
    file_guard: Option<&CfgPred>,
    sites: &mut Vec<CfgSite>,
    unreadable: &mut Vec<String>,
    declarations: &mut Vec<(String, CfgPred)>,
) {
    let blanked = blank_comments_and_strings(source);
    assert_eq!(
        blanked.len(),
        source.len(),
        "{path}: the blanker moved positions, so every span below is wrong"
    );
    let bytes = blanked.as_bytes();
    let line_of = |at: usize| source[..at].matches('\n').count() + 1;

    let mut depth = 0_usize;
    // Active while the current depth is INSIDE the body the item opened.
    let mut item_scopes: Vec<(usize, CfgPred)> = Vec::new();
    // `#![cfg(P)]`: active from the depth it was written at, downward.
    let mut inner_scopes: Vec<(usize, CfgPred)> = Vec::new();
    // The `#[cfg]`s stacked on the item being read, in source order.
    let mut pending: Vec<(usize, CfgPred)> = Vec::new();

    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // -- an attribute ---------------------------------------------------
        if byte == b'#' {
            let mut open = i + 1;
            let inner = bytes.get(open) == Some(&b'!');
            if inner {
                open += 1;
            }
            if bytes.get(open) != Some(&b'[') {
                i += 1;
                continue;
            }
            let Some(close) = balanced(bytes, open, b'[', b']') else {
                unreadable.push(format!(
                    "{path}:{}: an attribute is never closed",
                    line_of(i)
                ));
                i += 1;
                continue;
            };
            let span = &source[open + 1..close];
            let mut reader = CfgReader::new(span);
            reader.skip_space();
            let name = reader.ident().to_owned();
            let rest = &span[reader.at..];
            let inside = rest
                .trim_start()
                .strip_prefix('(')
                .and_then(|body| body.strip_suffix(')'));
            match (name.as_str(), inside) {
                ("cfg", Some(inside)) => match parse_cfg(inside, false) {
                    Ok(pred) => {
                        if inner {
                            inner_scopes.push((depth, pred));
                        } else {
                            pending.push((i, pred));
                        }
                    }
                    Err(error) => {
                        unreadable.push(unreadable_cfg(path, line_of(i), "cfg", inside, &error))
                    }
                },
                ("cfg_attr", Some(inside)) => match parse_cfg(inside, true) {
                    Ok(pred) => sites.push(CfgSite {
                        path: path.to_owned(),
                        line: line_of(i),
                        form: CfgForm::Attribute,
                        written: pred.render(),
                        rendered: pred.render(),
                        pred,
                    }),
                    Err(error) => {
                        unreadable.push(unreadable_cfg(
                            path,
                            line_of(i),
                            "cfg_attr",
                            inside,
                            &error,
                        ));
                    }
                },
                _ => {}
            }
            i = close + 1;
            continue;
        }

        // -- the item those attributes belong to -----------------------------
        if !pending.is_empty() {
            let mut parts: Vec<CfgPred> = Vec::new();
            if let Some(guard) = file_guard {
                parts.push(guard.clone());
            }
            parts.extend(inner_scopes.iter().map(|(_, pred)| pred.clone()));
            parts.extend(item_scopes.iter().map(|(_, pred)| pred.clone()));
            parts.extend(pending.iter().map(|(_, pred)| pred.clone()));
            let own = CfgPred::conjunction(pending.iter().map(|(_, p)| p.clone()).collect());
            let effective = CfgPred::conjunction(parts);
            let at = pending[0].0;
            sites.push(CfgSite {
                path: path.to_owned(),
                line: line_of(at),
                form: CfgForm::Gate,
                written: own.render(),
                rendered: effective.render(),
                pred: effective.clone(),
            });
            match item_shape(bytes, i) {
                ItemShape::Module { name, body: None } => {
                    declarations.push((name, effective.clone()));
                }
                ItemShape::Module { body: Some(_), .. } | ItemShape::Block => {
                    item_scopes.push((depth, effective));
                }
                ItemShape::Flat => {}
            }
            pending.clear();
        }

        // -- ordinary tokens --------------------------------------------------
        if byte == b'{' {
            depth += 1;
            i += 1;
            continue;
        }
        if byte == b'}' {
            depth = depth.saturating_sub(1);
            item_scopes.retain(|(open, _)| depth > *open);
            inner_scopes.retain(|(at, _)| depth >= *at);
            i += 1;
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = i;
            let mut end = i;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            // `cfg!(P)`, and ONLY with the `!`. A bare `cfg(` is an ordinary
            // call or a function named `cfg`, which is not an attribute and not
            // a macro; treating it as one is how a census invents a predicate
            // out of `fn cfg(bits: u32)`.
            if &blanked[start..end] == "cfg" {
                let mut cursor = end;
                while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                    cursor += 1;
                }
                if bytes.get(cursor) == Some(&b'!') {
                    cursor += 1;
                    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                        cursor += 1;
                    }
                    if bytes.get(cursor) == Some(&b'(') {
                        if let Some(close) = balanced(bytes, cursor, b'(', b')') {
                            let inside = &source[cursor + 1..close];
                            match parse_cfg(inside, false) {
                                Ok(pred) => sites.push(CfgSite {
                                    path: path.to_owned(),
                                    line: line_of(start),
                                    form: CfgForm::Macro,
                                    written: pred.render(),
                                    rendered: pred.render(),
                                    pred,
                                }),
                                Err(error) => unreadable.push(unreadable_cfg(
                                    path,
                                    line_of(start),
                                    "cfg!",
                                    inside,
                                    &error,
                                )),
                            }
                            i = close + 1;
                            continue;
                        }
                    }
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
}

fn unreadable_cfg(path: &str, line: usize, form: &str, inside: &str, error: &str) -> String {
    format!(
        "{path}:{line}: `{form}({})` did not parse: {error}. A predicate this census cannot \
         read is a body whose platform coverage it cannot decide, so extend the grammar \
         rather than skipping it.",
        inside.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

/// What the item starting at `at` is, as far as scoping cares.
enum ItemShape {
    /// `mod name { … }` or `mod name;`.
    Module { name: String, body: Option<usize> },
    /// Anything else with a braced body: a function, an `impl`, a `struct`, a
    /// bare block. Its guard reaches everything inside it.
    Block,
    /// Anything that ends before a brace: a `use`, a `const`, a struct field, a
    /// match arm. Nothing is nested under it.
    Flat,
}

/// Read far enough to tell those three apart.
///
/// The scan stops at the first `;` or `,` outside any bracket, which is what
/// ends a flat item, and at the first `{` outside any bracket, which opens a
/// body. Brackets are tracked because `const X: [u8; 2]` puts a `;` inside one
/// and `fn f(a: u8, b: u8)` puts a `,` inside one.
fn item_shape(bytes: &[u8], at: usize) -> ItemShape {
    let mut cursor = at;
    let mut brackets = 0_usize;
    let mut words: Vec<String> = Vec::new();
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'(' | b'[' => brackets += 1,
            b')' | b']' => brackets = brackets.saturating_sub(1),
            b';' | b',' if brackets == 0 => break,
            b'{' if brackets == 0 => {
                let module = words
                    .iter()
                    .position(|word| word == "mod")
                    .and_then(|index| words.get(index + 1).cloned());
                return match module {
                    Some(name) => ItemShape::Module {
                        name,
                        body: Some(cursor),
                    },
                    None => ItemShape::Block,
                };
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = cursor;
                while cursor < bytes.len()
                    && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
                {
                    cursor += 1;
                }
                words.push(String::from_utf8_lossy(&bytes[start..cursor]).into_owned());
                continue;
            }
            _ => {}
        }
        cursor += 1;
    }
    match words.iter().position(|word| word == "mod") {
        Some(index) => match words.get(index + 1) {
            Some(name) => ItemShape::Module {
                name: name.clone(),
                body: None,
            },
            None => ItemShape::Flat,
        },
        None => ItemShape::Flat,
    }
}

/// The effective predicates in this tree that no CI runner compiles, and why
/// each is deliberate.
///
/// An equality, not a filter. A new predicate that no runner compiles fails this
/// census until someone adds the platform's Clippy leg or writes the reason down
/// here, which is the check the predecessor could not make at all: it collected
/// `target_os` names, `not(any(unix, windows))` carries none, and five
/// production regions the denylist has never examined were invisible to it.
pub(super) const NO_CI_RUNNER_COMPILES: [(&str, &str); 2] = [
    (
        "all(unix, not(any(target_os = \"linux\", target_os = \"macos\")))",
        "the else-arm of `agent::proc`'s `last_errno()`, which reads `errno` through \
         `__errno_location` on Linux, `__error` on macOS, and falls back to \
         `std::io::Error::last_os_error()` on any other Unix. **Found by conjoining the \
         guards, and invisible without it**: the attribute on the arm is \
         `not(any(target_os = \"linux\", target_os = \"macos\"))`, which is true on Windows, \
         so an oracle reading the attribute alone reports the Windows leg as compiling it. \
         The `#[cfg(unix)]` module around it is what makes the conjunction unreachable -- a \
         Unix that is neither Linux nor macOS, which is a target this project does not ship \
         and no runner provides.",
    ),
    (
        "not(any(unix, windows))",
        "the else-arm of a `unix` / `windows` / otherwise split. `DESIGN.md` §3 makes the \
         crate first-class on Windows, macOS and Linux and claims no fourth family, so the \
         arm exists to keep the crate compiling on a target the project does not ship and \
         nothing CI runs can reach it. Clippy never examines these bodies -- which is the \
         fact this row records rather than repairs.",
    ),
];

/// The census's permanent positive control.
///
/// Injected into the **whole** scanned domain rather than parsed on its own:
/// `CODING_STANDARDS.md` §12 says a control inside a truncated domain does not
/// prove the domain was scanned, so the control rides along with every real file
/// and must still be found.
///
/// Every row of it is a thing a version of this census got wrong. The first four
/// are the standing ledger's: a predicate nothing compiles, one everything
/// compiles, a `target_os` binding that is not a predicate, and the same token
/// in prose and in a string literal. The rest are the review's: stacked
/// attributes, a guard on the module rather than the item, the two non-gating
/// forms, and the two literal shapes a `"..."`-only reader decodes wrongly.
/// `fn cfg` is there because an ordinary function may be called `cfg`, and a
/// scanner that reads any `cfg(` as an attribute invents a predicate from its
/// parameter list.
pub(super) const CFG_CENSUS_CONTROL: &str = r##"//! A control fixture. It is not compiled; it is scanned.

// Prose that spells #[cfg(target_os = "haiku")] and must not become a site.
const NAMED_IN_A_STRING: &str = "#[cfg(target_os = \"plan9\")]";

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn no_runner_compiles_this() {}

#[cfg(not(target_os = "freebsd"))]
fn every_runner_compiles_this() {}

#[cfg(unix)]
#[cfg(target_os = "macos")]
fn stacked_attributes_conjoin() {}

#[cfg(windows)]
mod only_on_windows {
    #[cfg(test)]
    fn nested_under_the_module_guard() {}
}

#[cfg(target_os = r"linux")]
fn a_raw_string_value() {}

#[cfg(target_os = "ma\x63os")]
fn an_escaped_value() {}

#[cfg_attr(target_os = "haiku", allow(dead_code))]
fn the_attribute_is_conditional_not_the_item() {}

fn the_macro_is_an_expression() -> bool {
    cfg!(target_os = "plan9")
}

fn cfg(bits: u32) -> u32 {
    bits
}

fn a_binding_is_not_a_predicate() {
    let target_os = "android";
    let _ = target_os;
    let _ = cfg(1);
}
"##;

/// The gate predicates the control fixture must produce, in source order.
pub(super) const CONTROL_GATES: [&str; 7] = [
    "not(any(target_os = \"linux\", target_os = \"macos\", target_os = \"windows\"))",
    "not(target_os = \"freebsd\")",
    "all(unix, target_os = \"macos\")",
    "windows",
    "all(windows, test)",
    "target_os = \"linux\"",
    "target_os = \"macos\"",
];

/// The predicate rows the standing ledger and the review name, with the runners
/// that actually compile each.
///
/// `(predicate, the runners that compile it, why the row is here)`.
pub(super) const CFG_ESCAPES: [(&str, &[&str], &str); 12] = [
    (
        "not(any(target_os = \"linux\", target_os = \"macos\", target_os = \"windows\"))",
        &[],
        "the standing row's second escape, verbatim: a name-collector reports all three \
         platforms covered while no runner compiles the body",
    ),
    (
        "not(target_os = \"freebsd\")",
        &["macos-latest", "ubuntu-latest", "windows-latest"],
        "the same escape inverted, which the row also names: a name-collector demands a \
         FreeBSD runner for a body every runner already compiles",
    ),
    (
        "not(any(unix, windows))",
        &[],
        "the shape this tree actually carries, and the one a `target_os` collector cannot \
         see at all because it names no `target_os`",
    ),
    (
        "target_os = \"macos\"",
        &["macos-latest"],
        "`PR5-MACOS-CLIPPY-NEVER-RUN`: the macOS leg is the only one that compiles it",
    ),
    (
        "windows",
        &["windows-latest"],
        "`PR5D-MSVC-CLIPPY-NEVER-RUN`: a bare flag with no key, which is why a `target_os \
         = ` scan needed a second special case for it and this one does not",
    ),
    (
        "unix",
        &["macos-latest", "ubuntu-latest"],
        "two runners, so it adds no platform requirement of its own",
    ),
    (
        "all(windows, test)",
        &["windows-latest"],
        "`test` is set by the test-harness invocation and not by the library one, so this \
         is decided rather than unknown -- the answer the three-valued version could not \
         give",
    ),
    (
        "all(test, not(test))",
        &[],
        "the reason the invocations are enumerated instead of merged. A single valuation \
         carrying every flag any invocation sets would make this reachable",
    ),
    (
        "all(unix, not(target_os = \"macos\"))",
        &["ubuntu-latest"],
        "nesting under a negation, which is where a collector loses the sign",
    ),
    (
        "any(unix, windows)",
        &["macos-latest", "ubuntu-latest", "windows-latest"],
        "every runner, so it demands nothing",
    ),
    (
        "all(unix, windows)",
        &[],
        "no target is both, so an effective predicate that conjoins a module guard with an \
         item guard can be uncompilable even though each half is compiled somewhere",
    ),
    (
        "not(test)",
        &["macos-latest", "ubuntu-latest", "windows-latest"],
        "the library invocation does not set `test`, so the negation is true there",
    ),
];

/// The floor on the census's gate domain.
///
/// A count, because a scan that silently stops reading is a scan that reports
/// nothing uncovered. The tree carries several hundred gating attributes and the
/// number moves with ordinary edits, so this is a floor rather than a pin; the
/// boundary assertions beside it are what pin the shape.
pub(super) const CFG_GATE_FLOOR: usize = 350;

/// The census domain: every file a test-only `mod …;` declaration names,
/// relative to `src/`, sorted.
///
/// Derived by
/// `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`,
/// and pinned here because every predicate in those files is `all(test, …)`
/// rather than what it says: a census that resolved none of them would read
/// several hundred predicates as unconditional and never notice.
///
/// **A collection of paths, and the type says so.** Every reader compares these
/// against a `Path` the tree produced, and `CODING_STANDARDS.md` §8 keeps a path
/// out of `String`: a lossy rendering maps two distinct non-UTF-8 paths onto
/// one, and rewriting `\` to `/` turns a backslash -- a legal character in a
/// Unix file name -- into what reads as a separator. A `&[&str]` named as the
/// source of truth for path identities says "string" in its type however
/// carefully each use site converts, so what everything reads is a
/// `Vec<PathBuf>` and the conversion happens once, here. The `&'static str`
/// literals inside are the *written* form, because a path literal in source is
/// how a human writes one; nothing reads them without going through `PathBuf`.
///
/// **Not every file this crate compiles only under `cfg(test)`, and the gap is
/// deliberate.** `census_domain::declared_whole_file_test_modules` does not
/// close over the file graph, so a declaration inside a file that is itself in
/// this list derives nothing: `effects/tests.rs` is listed and declares `mod
/// policy;`, which makes rustc compile `effects/tests/policy.rs` under
/// `cfg(test)` too, and `policy.rs` is deliberately absent here. Adding it
/// would widen what every census scans, which is a change to the measurement
/// and not a correction to this list. That derivation's own doc comment states
/// the closure it declines and the declaration form its scan cannot see.
///
/// **A list rather than a count, because a count does not say *which* files.**
/// A derivation that swapped one module for another -- same cardinality,
/// different set -- satisfies every assertion a number can carry, and fails
/// here naming the file it gained and the file it lost. The four modules not
/// called `tests.rs` were already named individually for exactly that reason;
/// this is that argument applied to the whole population rather than to the
/// part of it a file-name rule misses.
///
/// **Both populations are read off this list, so neither number is written
/// anywhere.** The whole of it is the domain above. The subset a literal
/// `#[cfg(test)] mod tests;` declares is the entries whose file stem is
/// `tests`: declared under that exact name at their parent's own top level,
/// with the attribute on the declaration and its effective predicate the bare
/// `test` atom, so each is a `tests.rs` and the file-name rule `file_stem ==
/// "tests"` finds it. The rest
/// -- `scaffold`, `premove`, `fake` and `readiness` -- differ by **how each
/// file is reached**, which is the distinction a census gets wrong, and they
/// are the ones it is most likely to trip over, since a scaffold, a fake and a
/// readiness protocol exist to name what production names.
///
/// **A narrowed guard would stay in this list and leave that subset, and the
/// disagreement is the signal.** `#[cfg(all(test, unix))] mod tests;` compiles
/// a whole test file on Unix and no file at all on Windows. It is still
/// test-only, so the derivation keeps it and it belongs in this domain; it is
/// not the form `#[cfg(test)] mod tests;`, so it is not in the subset above --
/// while its file stem is still `tests`, which is the half of this list that
/// subset is compared against. The two disagree, and the oracle fails naming
/// the file. That is the decision rather than an oversight: a census skipping
/// by file name treats such a module as present everywhere, so Windows would
/// lose it in silence, and the slice that writes one has to say what every
/// census should do about a module that exists on only some platforms. There
/// is no such declaration in this tree; PR #101's reviewer supplied the
/// reproduction and
/// `a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal_mod_tests_form`
/// drives it over synthetic input, so no later change can lose the distinction.
///
/// **A slice that adds a whole-file test module adds its path here, in sorted
/// position, in the same commit.** That is the whole edit: both counts follow,
/// and so does the named-individually set. The entries cluster by directory, so
/// slices landing in different directories insert far apart in this list. That
/// argument depends on the written order actually being sorted, and the
/// initializer asserts it **as written**, before anything normalises it --
/// every comparison against this list sorts what it reads, so an entry appended
/// at the end would otherwise satisfy all of them while the claim quietly
/// stopped being true.
///
/// Where it is compared with `>=`, the length is a floor. One entry --
/// `readiness.rs`, reached through an inline ancestor rather than through an
/// attribute on its own declaration -- is outside [`cfg_regions`]' grammar and
/// carries no `cfg` occurrence, so the two derivations agree on the number
/// without that census having to resolve the ancestry that produces it.
///
/// **This is the only place either population is written, and every assertion
/// about them reads it.** The two counts were stated as English words 37 times
/// across ten files, and written as an integer literal in five more places, so
/// one slice adding one whole-file test module falsified every one of them at
/// once while the `>=` floor stayed green -- and a passing floor is not the
/// same as a true document. PR #97's review found that, and
/// the prose now names this constant or describes the population without
/// counting it.
///
/// `pub(crate)` rather than `pub(super)` for one reader outside this directory:
/// `engine::topology::recover::tests` floors its skip count at `.len()`, which
/// is the only form of that floor that is not satisfied by the derivation
/// having gone inert.
pub(crate) static WHOLE_FILE_TEST_MODULES: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    // The written order, which is the sorted one the paragraph above argues
    // for. Sortedness is checked here rather than in a test so that no reader
    // of the list can bypass it, and on the literals rather than on the
    // `PathBuf`s so that what is checked is the text a human reads and diffs.
    // `>=` rather than `>`, so a path written twice fails here as well: the
    // oracle compares a `Vec` and not a set for the same reason.
    let written = [
        "agent/proc/test_support/readiness.rs",
        "effects/tests.rs",
        "engine/tests.rs",
        "engine/topology/attempt/tests.rs",
        "engine/topology/create/tests.rs",
        "engine/topology/dispatch/tests.rs",
        "engine/topology/emit/tests.rs",
        "engine/topology/preflight/tests.rs",
        "engine/topology/recover/tests.rs",
        "engine/topology/run/tests.rs",
        "engine/topology/scaffold.rs",
        "engine/topology/startup/tests.rs",
        "events/log/premove.rs",
        "events/log/tests.rs",
        "runner/container/census/tests.rs",
        "runner/container/fake.rs",
        "runner/container/resolve/tests.rs",
        "runner/container/tests.rs",
        "topology/effects/tests.rs",
        "topology/fold/tests.rs",
    ];
    let out_of_order = written.windows(2).find(|pair| pair[0] >= pair[1]);
    assert!(
        out_of_order.is_none(),
        "`WHOLE_FILE_TEST_MODULES` is not sorted as written, at {out_of_order:?}. Every \
         comparison against this list sorts what it reads, so an entry appended at the end -- or \
         written twice -- passes all of them, and the argument that slices in different \
         directories insert far apart stops being true without anything failing"
    );
    written.into_iter().map(PathBuf::from).collect()
});
