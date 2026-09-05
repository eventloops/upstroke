//! Extended notes: `docs/internals/effects/tests/cfg.md`

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CfgPred {
    All(Vec<CfgPred>),
    Any(Vec<CfgPred>),
    Not(Box<CfgPred>),
    Flag(String),
    Pair(String, String),
}

impl CfgPred {
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

    fn conjunction(mut parts: Vec<CfgPred>) -> CfgPred {
        match parts.len() {
            1 => parts.remove(0),
            _ => CfgPred::All(parts),
        }
    }
}

const MODELLED_FLAGS: [&str; 3] = ["test", "unix", "windows"];

const MODELLED_KEYS: [&str; 7] = [
    "target_arch",
    "target_endian",
    "target_env",
    "target_family",
    "target_os",
    "target_pointer_width",
    "target_vendor",
];

struct Valuation {
    runner: &'static str,
    invocation: String,
    flags: BTreeSet<&'static str>,
    keys: &'static [(&'static str, &'static str)],
}

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
            b'x' => {
                let digits = self.take_hex(2)?;
                let code = u32::from_str_radix(&digits, 16)
                    .map_err(|error| format!("`\\x{digits}`: {error}"))?;
                char::from_u32(code)
                    .filter(char::is_ascii)
                    .map(|ch| out.push(ch))
                    .ok_or_else(|| format!("`\\x{digits}` is not an ASCII escape"))
            }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CfgForm {
    Gate,
    Attribute,
    Macro,
}

pub(super) struct CfgSite {
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) form: CfgForm,
    pub(super) written: String,
    pub(super) rendered: String,
    pub(super) pred: CfgPred,
}

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

pub(super) fn cfg_regions(sources: &[(String, String)]) -> (Vec<CfgSite>, Vec<String>) {
    let mut unreadable = Vec::new();
    let known: BTreeSet<&str> = sources.iter().map(|(path, _)| path.as_str()).collect();

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
    let mut item_scopes: Vec<(usize, CfgPred)> = Vec::new();
    let mut inner_scopes: Vec<(usize, CfgPred)> = Vec::new();
    let mut pending: Vec<(usize, CfgPred)> = Vec::new();

    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte.is_ascii_whitespace() {
            i += 1;
            continue;
        }

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

enum ItemShape {
    Module { name: String, body: Option<usize> },
    Block,
    Flat,
}

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

pub(super) const CONTROL_GATES: [&str; 7] = [
    "not(any(target_os = \"linux\", target_os = \"macos\", target_os = \"windows\"))",
    "not(target_os = \"freebsd\")",
    "all(unix, target_os = \"macos\")",
    "windows",
    "all(windows, test)",
    "target_os = \"linux\"",
    "target_os = \"macos\"",
];

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

pub(super) const CFG_GATE_FLOOR: usize = 350;

pub(crate) static WHOLE_FILE_TEST_MODULES: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    let written = [
        "agent/proc/test_support/readiness.rs",
        "agent/proc/tests.rs",
        "effects/tests.rs",
        "engine/tests.rs",
        "engine/topology/attempt/tests.rs",
        "engine/topology/candidate/tests.rs",
        "engine/topology/create/tests.rs",
        "engine/topology/dispatch/tests.rs",
        "engine/topology/emit/tests.rs",
        "engine/topology/preflight/tests.rs",
        "engine/topology/prelock/tests.rs",
        "engine/topology/recover/tests.rs",
        "engine/topology/run/tests.rs",
        "engine/topology/scaffold.rs",
        "engine/topology/select/tests.rs",
        "engine/topology/settle/tests.rs",
        "engine/topology/startup/tests.rs",
        "events/log/premove.rs",
        "events/log/tests.rs",
        "rundir/scratch_tree.rs",
        "rundir/tests.rs",
        "runner/container/census/tests.rs",
        "runner/container/exec/tests.rs",
        "runner/container/fake.rs",
        "runner/container/resolve/tests.rs",
        "runner/container/tests.rs",
        "runner/host/tests.rs",
        "topology/effects/tests.rs",
        "topology/fold/tests.rs",
        "workspace_manager/fixture.rs",
        "workspace_manager/tests.rs",
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
