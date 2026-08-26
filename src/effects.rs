//! The compile-time enforcement layer: the effect denylist, the allowlist, the
//! wrapper classification, and the generated inventories.
//!
//! `decisions.effect_site_inventory.mechanism` is the whole of this module's
//! specification, in four numbered parts:
//!
//! 1. **The denylist is rustc-resolved, not lexical.** `clippy.toml`'s
//!    `disallowed-methods` / `disallowed-types` / `disallowed-macros` name every
//!    effect primitive the crate can reach, and "aliases, re-exports, function
//!    values, method calls, and macro-expanded code in this crate resolve to the
//!    same DefId". [`tests::every_declared_effect_denial_refuses_for_the_reason_it_declares`]
//!    compiles one fixture per shape and asserts the lint each emits, because
//!    that sentence is a claim about a toolchain and not a law of nature.
//! 2. **An allow of a governed lint lives only where the allowlist says.**
//!    Module-level, in a file listed in `effects/allowlist.toml`, whose legacy
//!    section is frozen, may only shrink, and never contains a topology module.
//! 3. **Wrapper classification.** Every externally reachable `fn` of a legacy or
//!    shared module is classified; the effectful ones join the denylist, "so a
//!    topology module cannot reach an effect through a legacy wrapper".
//! 4. **Dependency review** — a new dependency performing filesystem, process,
//!    lock or container effects has its API added to the denylist or is confined
//!    to a funnel module.
//!
//! # This module performs no effect
//!
//! Everything above the `#[cfg(test)]` line is a pure function over `&str`: the
//! parsers, the classifiers, the frozen lists. Reading `clippy.toml`, writing
//! `effect_sites.json` and compiling fixtures all happen in the test region,
//! which is where `outputs` puts them anyway ("effect_sites.json (from the
//! enums) … generated from the enums **by a test**"). That is why this file is
//! in the funnel section of the allowlist while claiming something stronger than
//! any other entry there.
//!
//! # The reading trap
//!
//! Every sentence quoted here is from `decisions.*`. `*_verification_dispositions`,
//! `finding_dispositions[].rationale` and the `v4_`..`v15_` keys are the packet's
//! disposition history and are quoted nowhere.

// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, and
// the entry there records `allows = []`. This module carries no attribute at
// all, which is the strongest form of the claim above: it reaches no denied
// primitive, and the one `std::process::Command` its text contains is inside
// `DENIAL_FIXTURES`, a string constant compiled elsewhere in order to be
// refused. `decisions.effect_site_inventory.mechanism` (2).

use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// The artifacts, by the names `outputs` gives them
// ---------------------------------------------------------------------------

/// `effect_site_inventory.outputs`: "clippy.toml".
pub const CLIPPY_TOML: &str = "clippy.toml";

/// `effect_site_inventory.outputs`: "effects/allowlist.toml".
pub const ALLOWLIST_TOML: &str = "effects/allowlist.toml";

/// `effect_site_inventory.outputs`: "the wrapper classification".
pub const WRAPPERS_TOML: &str = "effects/wrappers.toml";

/// `effect_site_inventory.outputs`: "effect_sites.json (from the enums)".
pub const EFFECT_SITES_JSON: &str = "effect_sites.json";

/// `effect_site_inventory.outputs`: "the residue-class evidence record (per
/// element: constructed, classified, recovered; per site: sampling N and
/// observed-class histogram)".
///
/// The *declarations* half. The histogram half is [`RESIDUE_HISTOGRAM_JSON`],
/// and the split is forced rather than chosen — see there.
pub const RESIDUE_CLASSES_JSON: &str = "effects/residue-classes.json";

/// The **observed-class histogram** half of the same record (`PR5-CONF-004`).
///
/// `outputs` requires, per site, "sampling N **and observed-class histogram**".
/// [`RESIDUE_CLASSES_JSON`] is generated from the frozen enums and compared
/// byte-for-byte, which it must be — and a histogram is machine-varying by
/// construction, since which class a kill sample lands in is a race between the
/// kill and Git. A count cannot be byte-pinned and a byte-pinned file cannot
/// carry one, so the histogram is emitted to this path on every run of
/// `workspace_manager::tests::sampled_git_child_kills_every_residue_classified_
/// and_recovered`, which then reads it back. Not checked in: its contents are a
/// property of the machine that produced them, and a stale copy of somebody
/// else's numbers would be worse than no copy.
pub const RESIDUE_HISTOGRAM_JSON: &str = "effects/residue-histogram.json";

/// Where each site's funnel **bodies** actually are, where that is not what
/// [`EFFECT_SITES_JSON`]'s `module` column says (`PR5-CONF-018`).
///
/// `effect_sites.json` is generated from the frozen enums, so its `module`
/// column is `EffectSiteId::module()` — PR3's answer, and the packet's:
/// `mechanism` (2) places "the answer funnels in `src/interaction.rs`". PR5's
/// lane B put the three Answer funnel bodies in `src/rundir.rs` and left
/// `interaction::{write_question, write_answer, read_answer}` as delegations,
/// so for `Answer.Ingest`, `Answer.PublishRename` and `Answer.StageWrite` the
/// checked-in artifact states something that is not true of this tree — and the
/// artifact is attached to gate reports, where a reader has no way to know.
///
/// The generator is `src/topology/effects.rs`, frozen under the owner ruling of
/// 2026-08-20, so the column cannot be corrected in place and the bodies are not
/// moved: `AnswerSite`'s three funnels close over `rundir`'s private `funnel`
/// and `RunDirHooks`, and relocating them to satisfy a column would be a slice
/// redesigning what it implements. What ships instead is this companion, which
/// carries the tree's own answer beside the inventory's for **every** site, so
/// the pair is true where either alone is not. Derived, compared byte-for-byte,
/// and regenerated by the same `REGENERATE` switch, so it cannot drift.
pub const FUNNEL_MODULES_JSON: &str = "effects/funnel-modules.json";

/// The environment variable that turns the generating tests into writers.
///
/// A generated artifact that is only ever *compared* rots into a chore nobody
/// can discharge; one that is only ever *written* proves nothing. Both, keyed on
/// this, is the ordinary resolution.
pub const REGENERATE: &str = "UPSTROKE_REGENERATE_EFFECT_ARTIFACTS";

// ---------------------------------------------------------------------------
// (2) The governed lints and where an allow of one may live
// ---------------------------------------------------------------------------

/// The six lints `mechanism` (2) governs, as bare names.
///
/// > "permits allow/expect of disallowed_methods, disallowed_types,
/// > disallowed_macros, clippy::style, clippy::all, or warnings only as
/// > module-level attributes in files listed in effects/allowlist.toml"
///
/// Bare, because an attribute may write either `disallowed_methods` or
/// `clippy::disallowed_methods` and the sentence names them both ways in one
/// breath. [`normalize_lint`] is the bridge.
pub const GOVERNED_LINTS: &[&str] = &[
    "disallowed_methods",
    "disallowed_types",
    "disallowed_macros",
    "style",
    "all",
    "warnings",
];

/// The three governed lints this slice actually uses, fully qualified.
///
/// `clippy::style`, `clippy::all` and `warnings` are governed and **unused**:
/// each would suppress far more than an effect denial, and
/// [`tests::the_three_blunt_governed_lints_are_used_by_nobody`] asserts the
/// count is zero rather than leaving it to habit.
pub const USED_GOVERNED_LINTS: &[&str] = &[
    "clippy::disallowed_methods",
    "clippy::disallowed_types",
    "clippy::disallowed_macros",
];

/// The bare lint name an attribute entry refers to, if it is governed.
///
/// `clippy::disallowed_methods` and `disallowed_methods` are the same lint;
/// `clippy::too_many_arguments` is not governed and answers `None`.
#[must_use]
pub fn normalize_lint(entry: &str) -> Option<&'static str> {
    let bare = entry.trim().rsplit("::").next()?.trim();
    GOVERNED_LINTS.iter().copied().find(|name| *name == bare)
}

/// One `allow`/`expect` of a governed lint, as the scan found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernedAllow {
    /// 1-based line of the attribute's `#`.
    pub line: usize,
    /// Whether it is an inner attribute (`#![…]`).
    pub inner: bool,
    /// Whether it is module-level: an inner attribute in the file's prologue, or
    /// an outer attribute on a `mod` item.
    pub module_level: bool,
    /// The governed lints it names, normalized, in source order.
    pub lints: Vec<String>,
    /// Every lint it names, as written — so a widening is visible.
    pub written: Vec<String>,
}

/// `source` with every comment and string literal replaced by spaces of the same
/// length, newlines preserved.
///
/// The scan has to be blind to text that only *looks* like an attribute.
/// `PR4-CENSUS-COMMENT-ORACLE` is in the standing ledger because a source census
/// counted a doc comment; this module is worse placed than most, since its own
/// build-refusal fixtures are `#[allow(clippy::disallowed_methods)]` written
/// inside doc comments and string literals. Blanking rather than deleting keeps
/// every byte offset — and therefore every line number — exact.
///
/// Raw strings (`r"…"`, `r#"…"#`), byte strings, char literals and escapes are
/// handled; a `'a` lifetime is not a char literal and is left alone.
/// Comments blanked, **string literals kept**.
///
/// The other half of [`blank_comments_and_strings`], and a separate function
/// because a census whose needle lives *inside* a string cannot use that one:
/// it blanks a literal including its quotes, so a search for `"docker` in its
/// output looks for a byte sequence the haystack can no longer contain. That is
/// not hypothetical — it is what the `mechanism` (1) "docker invocation
/// helpers" census did until PR6, which is why it stayed green when a real
/// `const DOCKER_PROGRAM: &str = "docker"` landed in production.
///
/// **One implementation, one caller shape.** `PR5D-VISIBILITY-CHECK-DUPLICATED`
/// is the standing entry for a parser written twice in this tree, so this lives
/// here beside its sibling rather than in each census that wants it.
///
/// Line comments, block comments (nested), char literals, escapes and **raw
/// strings** (`r"…"`, `r#"…"#`, `b"…"`, `br#"…"#`) are all handled: this
/// function tokenises exactly as [`blank_comments_and_strings`] does and differs
/// only in keeping a literal's bytes instead of blanking them. Byte offsets are
/// not preserved; line breaks are.
///
/// ## Why raw strings are modelled, and the direction the old limit had wrong
///
/// This used to track only `"` and document the omission as safe: "the failure
/// mode is a needle this function does *not* find, which makes a census that
/// uses it report something missing — **loud** — rather than accept something
/// extra." **That is backwards for a census over an expected set, which is what
/// every caller here is** (`PR6-LANEF-005`).
///
/// `r#"x" //"#` closed the literal at the second `"`, so the `//` that followed
/// began a line comment and **the rest of that line was deleted** — including a
/// real `"docker"` literal after it. `every_declared_effect_denial_names_a_real_path`'s
/// "docker invocation helpers" block asserts that the set of files naming a
/// container runtime is exactly a table of four; a fifth file whose literal was
/// erased is *absent from the computed set*, the sets compare equal, and the
/// census is **green with an extra Docker-naming file present**. A missed needle
/// is a false negative, and a false negative in a set comparison is fail-open,
/// not loud. The reviewer built that mutation and measured it.
///
/// So the residual is now the same as its sibling's: an unterminated literal
/// runs to end of input, which is a file that does not compile.
#[must_use]
pub fn blank_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                // The newline itself is left for the outer loop, so line numbers
                // survive.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut depth = 1usize;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        if bytes[i] == b'\n' {
                            out.push(b'\n');
                        }
                        i += 1;
                    }
                }
            }
            b'r' | b'b' if i == 0 || !is_ident_byte(bytes[i - 1]) => {
                // `r"…"`, `r#"…"#`, `b"…"`, `br#"…"#` — and an identifier that
                // merely begins with one of these letters, which is why the
                // preceding byte is checked and why a non-literal falls through
                // to a single push.
                match literal_end(bytes, i) {
                    Some(end) => {
                        out.extend_from_slice(&bytes[i..end]);
                        i = end;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'"' => {
                let end = literal_end(bytes, i).unwrap_or(bytes.len());
                out.extend_from_slice(&bytes[i..end]);
                i = end;
            }
            b'\'' => {
                // `'"'` is the one that matters here: without this arm it opens
                // a string. [`char_literal_end`] decides, so this and its
                // sibling cannot drift apart.
                match char_literal_end(bytes, i) {
                    Some(end) => {
                        out.extend_from_slice(&bytes[i..end]);
                        i = end;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where the char literal starting at `from` ends, exclusive, or `None` when
/// `from` does not start one.
///
/// A char literal is `'`, then either an escape (`\n`, `\\`, `\'`, `\u{1F600}`)
/// or **one UTF-8 scalar**, then `'`. The scalar is one to four bytes, and that
/// is the whole reason this is a scan rather than a lookahead.
///
/// ## The desync a fixed lookahead produces, and how far it reaches
///
/// Both blankers used to answer the question with two bytes: `'` is a char
/// literal when the byte at `+2` is a quote. `'é'` closes at `+3`, so it was
/// classified as **not** a literal, scanning resumed on its closing quote, and
/// that quote was then read as an *opening* one. From there the tokeniser is out
/// of phase: in `('é','{')` the pairing shifts by one and the `{` that is inside
/// a char literal survives into the blanked text as visible **code**.
///
/// One unbalanced brace is enough to take a file out of every census that
/// consults [`production_code`]. [`matching`] counts it, so
/// [`configured_item_end`]'s brace arm walks past the item's real `}`, finds no
/// balancing brace and gives up — and giving up used to mean "blank to end of
/// file".
///
/// Measured end to end, twice. On `src/agent/claude.rs`, with the pair inside
/// that file's `#[cfg(test)] mod tests` and a forged item appended below it, the
/// region measured **8525** non-whitespace bytes with the attack and 8525
/// without — a zero-byte delta no floor can see — and every source census was
/// green. Then gate-clean, because the first form is not: `cargo fmt` rewrites
/// `('é','{')` to `('é', '{')` and the space defuses it, and
/// `clippy::items_after_test_module` refuses an item placed below a file's own
/// `mod tests`. Both are avoidable. `stringify! { ('é','{') }` is left alone by
/// rustfmt (macro bodies in braces are), and a `#[cfg(test)]` module not named
/// `tests` is not what that lint looks for. With the probe inside
/// `src/runner/container/view.rs`'s `#[cfg(test)] pub(crate) mod fixtures` and a
/// forged `RunnerRequest {` builder above the file's real test module,
/// `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` both exit
/// 0 and `runner::tests::every_production_runner_request_is_built_by_its_roles_\
/// builder` passes — while the identical forged builder **without** the probe
/// fails it by name.
///
/// The preconditions are already in this tree: `src/status.rs`, `src/util.rs`
/// (twice on one line) and `src/engine/tests.rs` all hold non-ASCII char
/// literals. Only the adjacency was missing.
///
/// `'a` is a lifetime and is not a char literal; nor is `'_`, nor the `'static`
/// in `&'static str`. All three are refused by one rule — the byte after the
/// scalar is not a quote — rather than by a list.
fn char_literal_end(bytes: &[u8], from: usize) -> Option<usize> {
    if bytes.get(from) != Some(&b'\'') {
        return None;
    }
    let mut at = from + 1;
    if bytes.get(at) == Some(&b'\\') {
        // An escape. The longest Rust spells is `\u{10FFFF}`, which closes at
        // `from + 11`, so the window is bounded and a runaway scan over the rest
        // of the file cannot happen.
        at += 2;
        let limit = (from + 13).min(bytes.len());
        while at < limit && bytes[at] != b'\'' {
            at += 1;
        }
        return (bytes.get(at) == Some(&b'\'')).then_some(at + 1);
    }
    // One UTF-8 scalar, whose width its lead byte states. A continuation or an
    // otherwise invalid lead cannot begin one, and `source` is a `&str`, so the
    // remaining ranges are unreachable rather than merely unhandled.
    let width = match *bytes.get(at)? {
        0x00..=0x7F => 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return None,
    };
    at += width;
    (bytes.get(at) == Some(&b'\'')).then_some(at + 1)
}

/// Where the string literal starting at `from` ends, or `None` when `from` does
/// not start one.
///
/// Accepts `"…"`, `b"…"`, `r"…"`, `r#"…"#` and `br##"…"##`. An unterminated
/// literal ends at end of input — a file that does not compile.
fn literal_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    if bytes.get(j) == Some(&b'b') {
        j += 1;
    }
    let raw = bytes.get(j) == Some(&b'r');
    if raw {
        j += 1;
    }
    let hash_start = j;
    while bytes.get(j) == Some(&b'#') {
        j += 1;
    }
    let hashes = j - hash_start;
    if bytes.get(j) != Some(&b'"') || (!raw && hashes > 0) {
        return None;
    }
    j += 1;
    if raw {
        let close: Vec<u8> = std::iter::once(b'"')
            .chain(std::iter::repeat_n(b'#', hashes))
            .collect();
        while j < bytes.len() && !bytes[j..].starts_with(&close) {
            j += 1;
        }
        return Some((j + close.len()).min(bytes.len()));
    }
    while j < bytes.len() && bytes[j] != b'"' {
        j += if bytes[j] == b'\\' { 2 } else { 1 };
    }
    Some((j + 1).min(bytes.len()))
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn blank_comments_and_strings(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = vec![b' '; bytes.len()];
    // Newlines survive so line numbers do.
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            out[index] = b'\n';
        }
    }
    let keep = |out: &mut Vec<u8>, from: usize, to: usize| {
        out[from..to].copy_from_slice(&bytes[from..to]);
    };

    let mut i = 0;
    let mut code_start = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                keep(&mut out, code_start, i);
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                code_start = i;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                keep(&mut out, code_start, i);
                let mut depth = 1;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                code_start = i;
            }
            b'r' | b'b' => {
                // `r"…"`, `r#"…"#`, `b"…"`, `br#"…"#`
                let mut j = i;
                if bytes[j] == b'b' {
                    j += 1;
                }
                let raw = j < bytes.len() && bytes[j] == b'r';
                if raw {
                    j += 1;
                }
                let hash_start = j;
                while j < bytes.len() && bytes[j] == b'#' {
                    j += 1;
                }
                let hashes = j - hash_start;
                if j < bytes.len() && bytes[j] == b'"' && (raw || hashes == 0) {
                    keep(&mut out, code_start, i);
                    j += 1;
                    if raw {
                        let close: Vec<u8> = std::iter::once(b'"')
                            .chain(std::iter::repeat_n(b'#', hashes))
                            .collect();
                        while j < bytes.len() && !bytes[j..].starts_with(&close) {
                            j += 1;
                        }
                        j = (j + close.len()).min(bytes.len());
                    } else {
                        while j < bytes.len() && bytes[j] != b'"' {
                            j += if bytes[j] == b'\\' { 2 } else { 1 };
                        }
                        j = (j + 1).min(bytes.len());
                    }
                    i = j;
                    code_start = i;
                } else {
                    i += 1;
                }
            }
            b'"' => {
                keep(&mut out, code_start, i);
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(bytes.len());
                code_start = i;
            }
            b'\'' => {
                // [`char_literal_end`] decides, so this and its sibling in
                // [`blank_comments`] cannot drift apart.
                match char_literal_end(bytes, i) {
                    Some(end) => {
                        keep(&mut out, code_start, i);
                        i = end;
                        code_start = i;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    keep(&mut out, code_start, bytes.len());
    String::from_utf8_lossy(&out).into_owned()
}

/// The production region: everything before the first `#[cfg(test)]` that is not
/// inside a comment or a string.
#[must_use]
pub fn production_region(source: &str) -> String {
    let blanked = blank_comments_and_strings(source);
    match blanked.find("#[cfg(test)]") {
        Some(cut) => source[..cut].to_owned(),
        None => source.to_owned(),
    }
}

/// The production **code** of `source`: comments and string literals blanked,
/// and every `#[cfg(test)]`-configured item removed.
///
/// [`production_region`] answers a different question and keeps its answer: it
/// *truncates* at the first `#[cfg(test)]`, which is what a **domain** question
/// wants (everything above the cut is certainly production) and what a
/// **prohibition** question must not have. Three failures a prohibition census
/// pays for with a truncating region, all three measured on this tree:
///
/// * A file that declares its tests as `#[cfg(test)] mod tests;` — thirteen of
///   them here — puts every line **below** that declaration outside the region.
///   The declaration is usually the last item, so the hole is normally empty;
///   appending to the file fills it. Legal Rust, no comment trick, and it
///   defeated the barrier census, the process-start census and the container
///   token census at once.
/// * A `#[cfg(test)]` inside a block comment or a string literal truncates a
///   region that a `//`-only strip cannot see. `PR4-CENSUS-COMMENT-ORACLE`,
///   in the shape a `//`-only strip does not close.
/// * Counting over unblanked text counts prose. `src/agent/proc.rs` names
///   `run_with_timeout` eight times, five in code and three in doc comments, so
///   a real ninth entry point could be paid for by deleting two sentences.
///
/// So this returns the **whole file**, blanked, with each `#[cfg(test)]` item
/// blanked out in place. Newlines survive, so a byte offset still maps to the
/// line it came from.
///
/// The item's extent is found by delimiter matching over the blanked text — a
/// brace body ends at its matching `}` (and takes a trailing `;` with it, for
/// `use a::{b, c};`), anything else ends at the first `;` or `,` outside a
/// nested delimiter, and a closing delimiter that would leave the enclosing
/// block ends it too. Angle brackets are deliberately not matched: a
/// `#[cfg(test)] field: BTreeMap<K, V>,` ends at the comma inside the generics
/// and leaves `V>,` behind. That is the safe direction — a region that is too
/// **large** can only make a census match more, never less.
#[must_use]
pub fn production_code(source: &str) -> String {
    const ATTR: &[u8] = b"#[cfg(test)]";
    let blanked = blank_comments_and_strings(source);
    let bytes = blanked.as_bytes();
    let mut out = bytes.to_vec();
    let mut from = 0;
    // Searched over bytes rather than `str::find`, because a cut offset is not
    // guaranteed to be a char boundary and slicing one panics.
    while let Some(at) = bytes
        .get(from..)
        .and_then(|rest| rest.windows(ATTR.len()).position(|at| at == ATTR))
        .map(|found| from + found)
    {
        // Any further attributes stacked on the same item belong to it.
        let mut start = at + ATTR.len();
        loop {
            while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
                start += 1;
            }
            if bytes.get(start) == Some(&b'#') {
                let open = if bytes.get(start + 1) == Some(&b'!') {
                    start + 2
                } else {
                    start + 1
                };
                if bytes.get(open) == Some(&b'[') {
                    if let Some(close) = matching(bytes, open, b'[', b']') {
                        start = close + 1;
                        continue;
                    }
                }
            }
            break;
        }
        let end = configured_item_end(bytes, start);
        for byte in &mut out[at..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        from = end.max(at + ATTR.len());
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where the item beginning at `start` ends, exclusive. See [`production_code`].
///
/// **The two give-up paths return `start`, not `bytes.len()`.** Both are reached
/// only when the blanked text does not parse — an unbalanced brace, or an item
/// with no terminator before end of file — and neither is reachable from this
/// tree today (measured: zero occurrences over all 92 source files). What
/// decides the value is the *direction* they fail in. `bytes.len()` reads "the
/// item is the rest of the file" and blanks it, so a tokeniser that has lost
/// phase silently removes every production item below the attribute from every
/// census that consults this region — which is exactly what
/// [`char_literal_end`]'s desync used to buy. Returning `start` blanks the
/// attribute and nothing else, so the test module below it reads as production
/// and the censuses go **loud** instead. The larger region is always the safe
/// one here, for the same reason the doc above gives for not matching angle
/// brackets: it can only make a census match more, never less.
fn configured_item_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'{' if depth == 0 => {
                let Some(close) = matching(bytes, index, b'{', b'}') else {
                    return start;
                };
                let mut after = close + 1;
                while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
                    after += 1;
                }
                return if bytes.get(after) == Some(&b';') {
                    after + 1
                } else {
                    close + 1
                };
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => return index,
            b')' | b']' | b'}' => depth -= 1,
            b';' | b',' if depth == 0 => return index + 1,
            _ => {}
        }
        index += 1;
    }
    start
}

/// Every `allow`/`expect` of a governed lint in `source`, with where it sits.
///
/// Attributes are found in the blanked text and read out of the original, so a
/// fixture quoted in a doc comment is invisible and a real attribute is not.
#[must_use]
pub fn governed_allows(source: &str) -> Vec<GovernedAllow> {
    let blanked = blank_comments_and_strings(source);
    let bytes = blanked.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'#' {
            i += 1;
            continue;
        }
        let inner = bytes.get(i + 1) == Some(&b'!');
        let open = if inner { i + 2 } else { i + 1 };
        if bytes.get(open) != Some(&b'[') {
            i += 1;
            continue;
        }
        let Some(close) = matching(bytes, open, b'[', b']') else {
            i += 1;
            continue;
        };
        let attribute = &blanked[open + 1..close];
        let mut lints = Vec::new();
        let mut written = Vec::new();
        for keyword in ["allow", "expect"] {
            let mut at = 0;
            while let Some(hit) = attribute[at..].find(keyword) {
                let start = at + hit;
                let after = start + keyword.len();
                let is_word_start = start == 0
                    || !attribute.as_bytes()[start - 1].is_ascii_alphanumeric()
                        && attribute.as_bytes()[start - 1] != b'_';
                if is_word_start && attribute.as_bytes().get(after) == Some(&b'(') {
                    if let Some(end) = matching(attribute.as_bytes(), after, b'(', b')') {
                        for entry in attribute[after + 1..end].split(',') {
                            let entry = entry.trim();
                            if entry.is_empty() || entry.starts_with("reason") {
                                continue;
                            }
                            written.push(entry.to_owned());
                            if let Some(name) = normalize_lint(entry) {
                                lints.push(name.to_owned());
                            }
                        }
                    }
                }
                at = after;
            }
        }
        if !lints.is_empty() {
            found.push(GovernedAllow {
                line: blanked[..i].matches('\n').count() + 1,
                inner,
                module_level: is_module_level(&blanked, i, close, inner),
                lints,
                written,
            });
        }
        i = close + 1;
    }
    found
}

/// The index of the bracket closing the one at `open`, or `None`.
fn matching(bytes: &[u8], open: usize, opener: u8, closer: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        if *byte == opener {
            depth += 1;
        } else if *byte == closer {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// An inner attribute in the file's prologue, or an outer attribute on a `mod`.
///
/// "Module-level" is the whole of the placement rule, so it is decided here
/// rather than by eye: an `#![allow(…)]` before the first item governs the file
/// module; a `#[allow(…)] mod inner { … }` governs that module; an attribute on
/// a function, a statement or an expression governs neither and is what the rule
/// exists to refuse.
fn is_module_level(blanked: &str, hash: usize, close: usize, inner: bool) -> bool {
    if inner {
        // Nothing but whitespace and other attributes may precede it.
        let mut prefix = &blanked[..hash];
        loop {
            let trimmed = prefix.trim_end();
            if trimmed.ends_with(']') {
                let Some(open) = trimmed.rfind("#![").or_else(|| trimmed.rfind("#[")) else {
                    return false;
                };
                prefix = &trimmed[..open];
                continue;
            }
            return trimmed.is_empty();
        }
    }
    // Outer: skip further attributes and whitespace, then require `mod`.
    let mut rest = &blanked[close + 1..];
    loop {
        rest = rest.trim_start();
        if rest.starts_with('#') {
            let Some(open) = rest.find('[') else {
                return false;
            };
            let Some(end) = matching(rest.as_bytes(), open, b'[', b']') else {
                return false;
            };
            rest = &rest[end + 1..];
            continue;
        }
        for visibility in ["pub(crate)", "pub(super)", "pub", ""] {
            let candidate = rest.strip_prefix(visibility).unwrap_or(rest).trim_start();
            if candidate.starts_with("mod ") {
                return true;
            }
            if visibility.is_empty() {
                return false;
            }
        }
        return false;
    }
}

// ---------------------------------------------------------------------------
// (2) The frozen legacy section
// ---------------------------------------------------------------------------

/// The legacy section of `effects/allowlist.toml` as PR5 freezes it.
///
/// > "the legacy section may only shrink after PR5 (the test compares against
/// > the frozen list) and never contains a topology module"
///
/// Held here rather than only in the TOML because the TOML is the thing under
/// test: a frozen list that lived in the file it freezes would agree with any
/// edit to that file.
pub const FROZEN_LEGACY_ALLOWLIST: &[&str] = &[
    "src/engine/coordinator.rs",
    "src/engine/resume.rs",
    "src/engine/attempt.rs",
    "src/engine/preflight.rs",
    "src/workspace.rs",
    "src/gates.rs",
    "src/review.rs",
    "src/agent/proc.rs",
    "src/agent/bin.rs",
    "src/agent/claude.rs",
    "src/agent/codex.rs",
    "src/agent/copilot.rs",
    "src/capacity.rs",
    "src/export.rs",
    "src/main.rs",
    "src/answer.rs",
    "src/config.rs",
    "src/connect.rs",
    "src/route.rs",
    "src/status.rs",
    "src/validate.rs",
    "src/events/mod.rs",
    "src/events/log/premove.rs",
    "src/engine/tests.rs",
    "examples/probe.rs",
];

/// The modules the legacy section may never contain, verbatim from `mechanism`.
///
/// > "never contains a topology module (src/topology/**, src/runner/**,
/// > src/workspace_manager.rs, src/engine/topology.rs)"
///
/// The ban is on the **legacy** section alone, which is why
/// `src/runner/{host,container,invocation}.rs` and `src/workspace_manager.rs`
/// are in the funnel section without contradiction — the same sentence lists
/// them there.
///
/// `src/engine/topology/` is the fifth entry and is **not** in the packet
/// sentence, which names `src/engine/topology.rs` only. It is here because
/// [`topology_modules_among`] matches with `str::starts_with`, and
/// `"src/engine/topology/create.rs"` does not start with
/// `"src/engine/topology.rs"` — the sentence's four shapes were written when
/// the schema-4 engine was one file, and PR7 makes it a directory. Without
/// this entry the ban silently stops covering every submodule of the module it
/// exists to cover. Widening a ban is not a relaxation of the packet, and
/// `the_legacy_section_never_contains_a_topology_module` executes the gap: it
/// asserts the four-entry list misses a submodule that the five-entry list
/// catches.
pub const TOPOLOGY_MODULES: &[&str] = &[
    "src/topology/",
    "src/runner/",
    "src/workspace_manager.rs",
    "src/engine/topology.rs",
    "src/engine/topology/",
];

/// Entries of `current` that the frozen list does not contain — i.e. growth.
///
/// A pure function over its inputs precisely so the refusal can be *executed*
/// against a list that does grow, rather than inferred from one that does not.
#[must_use]
pub fn legacy_growth<'a>(frozen: &[&str], current: &[&'a str]) -> Vec<&'a str> {
    let frozen: BTreeSet<&str> = frozen.iter().copied().collect();
    current
        .iter()
        .copied()
        .filter(|path| !frozen.contains(path))
        .collect()
}

/// Entries of `paths` that name a topology module.
#[must_use]
pub fn topology_modules_among<'a>(paths: &[&'a str]) -> Vec<&'a str> {
    paths
        .iter()
        .copied()
        .filter(|path| {
            TOPOLOGY_MODULES
                .iter()
                .any(|banned| path.starts_with(banned) || *path == *banned)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// (3) Wrapper classification
// ---------------------------------------------------------------------------

/// The modules whose externally reachable `fn`s `mechanism` (3) classifies.
///
/// > "at PR5 every pubfn of a legacy or shared module is classified effectful or
/// > effect-free by review"
///
/// **Legacy** is the frozen legacy section. **Shared** is the modules the slice
/// `scope` names — "shared primitives (locks, run-dir creation and marker,
/// answer staging/ingestion, util JSON write, the exact-snapshot primitive incl.
/// its ephemeral commit, the event-log writer) moved behind funnels with Shared
/// sites" — plus the process funnel, whose `Shared` sites PR4 landed.
///
/// `src/topology/effects.rs` and `src/effects.rs` are deliberately outside the
/// domain: both are in the allowlist's funnel section, and neither is legacy nor
/// shared. Between them they declare 208 + n functions that touch nothing, and
/// classifying them would bury the rows that matter.
pub const CLASSIFIED_MODULES: &[&str] = &[
    // shared
    "src/workspace_manager.rs",
    "src/rundir.rs",
    "src/interaction.rs",
    "src/util.rs",
    "src/events/log.rs",
    "src/runner/host.rs",
    "src/runner/invocation.rs",
    // The third of `mechanism` (2)'s `src/runner/{host,container,invocation}.rs`,
    // added by PR6. It is here rather than only in the allowlist because it
    // denies six of its own paths — the "docker invocation helpers" the same
    // sentence enumerates — and `every_effectful_wrapper_is_on_the_disallowed_list`
    // requires a `upstroke::` denial to be a row somebody classified.
    "src/runner/container.rs",
    // The body of the Container funnel's R19 view, added by PR7's census
    // repair. It carries `#![allow(clippy::disallowed_methods)]` over its
    // production region and was the **only** non-test production module in the
    // tree in that position and absent from this list. The consequence was not
    // theoretical: with no row here, none of its `pub fn` needed classifying,
    // so `every_effectful_wrapper_is_on_the_disallowed_list` could never force
    // one onto the denylist — a module that may reach `fs` under its own allow,
    // and whose reachable surface nobody had to account for.
    "src/runner/container/view.rs",
    // legacy
    "src/engine/coordinator.rs",
    "src/engine/resume.rs",
    "src/engine/attempt.rs",
    "src/engine/preflight.rs",
    "src/workspace.rs",
    "src/gates.rs",
    "src/review.rs",
    "src/agent/proc.rs",
    "src/agent/bin.rs",
    "src/agent/claude.rs",
    "src/agent/codex.rs",
    "src/agent/copilot.rs",
    "src/capacity.rs",
    "src/export.rs",
    "src/main.rs",
    "src/answer.rs",
    "src/config.rs",
    "src/connect.rs",
    "src/route.rs",
    "src/status.rs",
    "src/validate.rs",
    "src/events/mod.rs",
    "src/events/log/premove.rs",
];

/// Every `fn` of `source`'s production region that is reachable from outside its
/// module.
///
/// Three shapes, because "pubfn" in the packet's sentence has three of them in
/// this tree and a classification that saw one would be complete against a
/// domain nobody drew:
///
/// * `pub fn` / `pub(crate) fn` / `pub(super) fn` items, free or in an inherent
///   `impl`;
/// * every `fn` inside an `impl <Trait> for <Type>` block, which is reachable
///   through the trait whatever its own visibility says;
/// * associated `fn`s of a public trait's default bodies, which are the same
///   case.
///
/// Names are returned once each, sorted. Two `impl` blocks with a `new` apiece
/// are one row: the classification is of a *name in a module*, and a name that
/// is effectful in one impl is a name the denylist has to carry anyway.
///
/// **The third shape was documented and not implemented until repair round F1**
/// (`PR6-REACHABLE-FN-PARSER-MISSES-TRAIT-DEFAULTS`, refiled as
/// `PR6-LANEF-007`). The predicate was `visible || in_trait_impl`, and a default
/// body inside a `pub trait` declaration is neither: it carries no visibility of
/// its own and it is not in an `impl … for …` block. Lane F filed it as narrow
/// because no such body reached an effect; the reviewer **built one** —
/// `fn remove_without_a_site(&self, path: &Path) { let _ = fs::remove_file(path); }`
/// as a default method on the public `ContainerHooks` — and clippy, all 79
/// effects tests and all 38 container tests passed. A default body is the one
/// place in this tree where an effect could be added to a *classified* module
/// without appearing in its classification.
///
/// A trait method **declaration** (no body) is deliberately still excluded: it
/// performs nothing, and every implementation of it is reached by the
/// `impl … for …` shape above.
#[must_use]
pub fn externally_reachable_fns(source: &str) -> Vec<String> {
    let region = blank_comments_and_strings(&production_region(source));
    let bytes = region.as_bytes();
    let mut names = BTreeSet::new();
    let mut trait_impl_spans = Vec::new();
    let mut public_trait_spans = Vec::new();

    // `pub trait X: Y { … }` — the bodies inside are reachable through the
    // trait, exactly as a trait impl's are.
    let mut t = 0;
    while let Some(hit) = region[t..].find("trait ") {
        let start = t + hit;
        t = start + "trait ".len();
        if start > 0 && is_ident_byte(bytes[start - 1]) {
            continue;
        }
        if !declares_visibility(&region[..start]) {
            continue;
        }
        let Some(brace) = find_header_brace(&region, t) else {
            continue;
        };
        if let Some(end) = matching(bytes, brace, b'{', b'}') {
            public_trait_spans.push((brace, end));
        }
    }

    // `impl <something> for <something> {` — the `for` is what makes it a trait
    // impl; an inherent `impl Type {` has none before the brace.
    let mut i = 0;
    while let Some(hit) = region[i..].find("impl") {
        let start = i + hit;
        i = start + 4;
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        if !before_ok || !region[i..].starts_with([' ', '<']) {
            continue;
        }
        let Some(brace) = find_header_brace(&region, i) else {
            continue;
        };
        let header = &region[i..brace];
        if !header.contains(" for ") {
            continue;
        }
        if let Some(end) = matching(bytes, brace, b'{', b'}') {
            trait_impl_spans.push((brace, end));
        }
    }

    for (index, _) in region.match_indices("fn ") {
        if index > 0 && is_ident_byte(bytes[index - 1]) {
            continue;
        }
        let Some(name) = region[index + 3..]
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let visible = declares_visibility(&region[..index]);
        let in_trait_impl = trait_impl_spans
            .iter()
            .any(|(open, close)| index > *open && index < *close);
        // A default body in a public trait, and only a default *body*:
        // `find_header_brace` answers `None` at the `;` of a declaration.
        let is_default_body = public_trait_spans
            .iter()
            .any(|(open, close)| index > *open && index < *close)
            && find_header_brace(&region, index).is_some();
        if visible || in_trait_impl || is_default_body {
            names.insert(name.to_owned());
        }
    }
    names.into_iter().collect()
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether the text immediately before a `fn` declares it visible outside its
/// module — with the `pub const fn` / `pub unsafe fn` / `pub async fn`
/// modifiers stripped first.
///
/// **One copy, deliberately.** This was written twice — once for the bare case
/// and once inside the modifier-stripping fallback — and a mutation that broke
/// the `pub(crate)` arm of the first copy left the whole suite green, because
/// the second copy still caught it. Two hand-maintained lists of three strings
/// disagree eventually, and the one that disagreed silently would be this one.
/// Measured, mutation `the-parser-misses-pub-crate`.
fn declares_visibility(prefix: &str) -> bool {
    let mut rest = prefix.trim_end();
    for modifier in ["unsafe", "const", "async"] {
        for _ in 0..3 {
            rest = rest.strip_suffix(modifier).unwrap_or(rest).trim_end();
        }
    }
    rest.ends_with("pub") || rest.ends_with("pub(crate)") || rest.ends_with("pub(super)")
}

/// The `{` that opens an `impl` block's body, skipping generics and where-clauses.
fn find_header_brace(region: &str, from: usize) -> Option<usize> {
    let bytes = region.as_bytes();
    let mut angle = 0i32;
    let mut paren = 0i32;
    for (index, byte) in bytes.iter().enumerate().skip(from) {
        match byte {
            b'<' => angle += 1,
            b'>' => angle -= 1,
            b'(' => paren += 1,
            b')' => paren -= 1,
            b';' if angle <= 0 && paren <= 0 => return None,
            b'{' if angle <= 0 && paren <= 0 => return Some(index),
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The four build-failure refusals whose reason must be pinned
// ---------------------------------------------------------------------------

/// One shape `mechanism` (1) claims rustc resolution defeats, as a fixture.
///
/// `proof_tests[4]`: "injected renamed-import / re-export / function-value /
/// legacy-wrapper call fixtures fail the build". A fixture asserting "this does
/// not build" is green whether it failed for the intended reason or a typo, so
/// each row carries the lint it must emit **and** the resolved path clippy must
/// name — and the harness runs a control that must compile first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenialFixture {
    /// What the shape is called in `proof_tests[4]`.
    pub shape: &'static str,
    /// The fixture body, compiled as its own crate against this crate's rlib.
    pub source: &'static str,
    /// The lint the fixture must emit, and nothing else.
    pub lint: &'static str,
    /// The path clippy's message must name — the *resolved* one, which is the
    /// whole claim: a renamed import reports as `std::fs::write`, not as `w`.
    pub resolves_to: &'static str,
}

/// The fixture set. One row per shape `proof_tests[4]` names, plus the two the
/// mechanism sentence names that the proof test does not (a method call and a
/// macro), because "aliases, re-exports, function values, method calls, and
/// macro-expanded code" is five shapes and a grid short of its domain is the
/// class this project has recorded four times.
pub const DENIAL_FIXTURES: &[DenialFixture] = &[
    DenialFixture {
        shape: "renamed-import",
        source: "use std::fs::write as scribble;\n\
                 pub fn go(p: &str) { let _ = scribble(p, \"x\"); }\n",
        lint: "clippy::disallowed_methods",
        resolves_to: "std::fs::write",
    },
    DenialFixture {
        shape: "re-export",
        source: "pub mod hatch { pub use std::fs::write; }\n\
                 pub fn go(p: &str) { let _ = hatch::write(p, \"x\"); }\n",
        lint: "clippy::disallowed_methods",
        resolves_to: "std::fs::write",
    },
    DenialFixture {
        shape: "function-value",
        source: "pub fn go(p: &str) {\n\
                 \x20   let f = std::fs::write::<&str, &str>;\n\
                 \x20   let _ = f(p, \"x\");\n\
                 }\n",
        lint: "clippy::disallowed_methods",
        resolves_to: "std::fs::write",
    },
    DenialFixture {
        shape: "legacy-wrapper call",
        source: "pub fn go(p: &std::path::Path) {\n\
                 \x20   let _ = upstroke::util::write_text(p, \"x\");\n\
                 }\n",
        lint: "clippy::disallowed_methods",
        resolves_to: "upstroke::util::write_text",
    },
    DenialFixture {
        shape: "method call",
        source: "pub fn go(p: &std::path::Path) -> std::io::Result<()> {\n\
                 \x20   let f = std::fs::File::open(p)?;\n\
                 \x20   f.sync_all()\n\
                 }\n",
        lint: "clippy::disallowed_methods",
        resolves_to: "std::fs::File::sync_all",
    },
    DenialFixture {
        shape: "macro-expanded code",
        source: "pub fn go() { println!(\"escaped\"); }\n",
        lint: "clippy::disallowed_macros",
        resolves_to: "std::println",
    },
    DenialFixture {
        shape: "type",
        source: "pub fn go() { let _ = std::process::Command::new(\"git\"); }\n",
        lint: "clippy::disallowed_types",
        resolves_to: "std::process::Command",
    },
];

/// A fixture that must compile clean, so a mis-wired invocation cannot make
/// every refusal above "pass".
///
/// `PR5-C-DOCTEST-FIXTURES-NEVER-RAN` is in the standing ledger because three
/// build-refusal fixtures were green having never executed. The control is the
/// difference between "the compiler refused this" and "the compiler could not
/// find a crate to refuse it against".
pub const DENIAL_CONTROL: &str = "pub fn go(p: &std::path::Path) -> bool {\n\
                                  \x20   let _ = upstroke::util::tail(\"x\", 1);\n\
                                  \x20   p.exists()\n\
                                  }\n";

// -- test-only declarations ----------------------------------------------
// At the BOTTOM, and a `mod` rather than a bare `fn`: `production_region` cuts a
// file at its first `#[cfg(test)]`, and
// `effects::tests::every_production_region_that_stops_early_stops_at_a_module`
// pins by name the ten files whose cut lands on something that is not a module.
// This file is not one of them and must not become one.

/// The **domain** every whole-tree census draws, derived once.
///
/// `PR5D-VISIBILITY-CHECK-DUPLICATED`: a value two places both maintain by hand
/// disagree eventually, and the one that disagrees silently is the one that
/// decides what a census is allowed to see. This derivation was written twice —
/// `runner::tests::whole_file_test_module_declarations` and
/// `events::log::tests::declared_whole_file_test_modules`, identical by hand,
/// each deciding which files four whole-tree censuses skip. It lives beside
/// [`production_code`] now, which is the region those same censuses count over.
#[cfg(test)]
pub(crate) mod census_domain {
    use std::path::PathBuf;

    /// Every `#[cfg(test)] mod <name>;` the crate declares, as
    /// `(declaring file, name, [flat candidate, nested candidate])`.
    ///
    /// Such a file is test code end to end. A region function has nothing to
    /// remove in one, so it would count the whole of it as production — a
    /// fixture that names a census's needle would then read as a production
    /// offender. The set is read out of the declarations rather than listed by
    /// hand: it was `src/engine/tests.rs` alone until PR5 moved the Event funnel
    /// into `src/events/log.rs` with two test modules of its own, and the census
    /// failed on the first file the hand-maintained list did not know about.
    ///
    /// **Read out of the blanked source, and every candidate returned rather
    /// than assumed.** The split used to be over the raw text, so a `//` line
    /// containing `#[cfg(test)] mod policy;` derived a skip for
    /// `src/runner/policy.rs` and removed that file from every census below —
    /// measured, with a `git push` planted in it that the census then did not
    /// see. Over the whole tree the raw split derived 50 skip paths of which
    /// **34 named no file at all**, and a skip path naming no file is a skip
    /// that has stopped meaning anything, so each caller asserts that exactly
    /// one of the two candidates exists.
    ///
    /// A declaration carrying a visibility qualifier — `#[cfg(test)]
    /// pub(crate) mod helpers;` — is deliberately **not** matched. Failing to
    /// derive a skip leaves a test file inside a census's domain, where a
    /// fixture is reported as an offender and someone looks; deriving one it
    /// should not removes a real production file, silently. Only the first
    /// direction is safe, so the predicate stays the narrow one.
    /// Calls to `name` in `code`: neither its definition, nor a longer identifier
    /// that merely ends in it.
    ///
    /// The second half is the one that was missing. A needle built as
    /// `format!("{name}(")` is a plain substring search, so `expected_refs(` is
    /// satisfied by every `refuse_unexpected_refs(` in the tree — and a census whose
    /// entry is proved by a different function's call sites proves nothing about its
    /// own. Measured on this tree: `workspace_manager.rs` carries four occurrences of
    /// the substring `expected_refs(` and **zero** calls to `expected_refs` — one of
    /// the four survives into `production_code`'s region, and it is the *definition
    /// line* of `refuse_unexpected_refs`, which the "calls, not definitions" filter
    /// does not see because the text before the match is `pub fn refuse_un`.
    ///
    /// The boundary is "the byte before the match is not an identifier byte", which
    /// keeps `crate::a::b::expected_refs(` — `:` is not one — and rejects
    /// `unexpected_refs(`. Not a rename, which is how
    /// `the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle`
    /// closed the same class: that census's needle is a constant it could choose,
    /// this one's is eleven names the packet chose.
    pub(crate) fn production_calls(code: &str, name: &str, form: Call) -> usize {
        let needle = format!("{name}(");
        code.match_indices(&needle)
            // Not the tail of a longer identifier.
            .filter(|(at, _)| {
                code[..*at]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !before.is_alphanumeric() && before != '_')
            })
            // Calls, not definitions.
            .filter(|(at, _)| !code[..*at].trim_end().ends_with("fn"))
            // And the form the clause is written in, which is what tells three
            // items of one name apart.
            .filter(|(at, _)| {
                let dotted = code[..*at].trim_end().ends_with('.');
                match form {
                    Call::Free => !dotted,
                    Call::Method => dotted,
                }
            })
            .count()
    }

    /// How a clause's function is **called** in production.
    ///
    /// Not decoration. `settle_interrupted` names three unrelated production items
    /// in this tree — `recover::settle_interrupted` (the `T-ATTEMPT` clause, a free
    /// function), `AttemptContext::settle_interrupted` and
    /// `events::RunState::settle_interrupted` (both methods, both called) — and a
    /// census counting the bare name is satisfied by either of the other two.
    /// Measured by S5 round 4: deleting step (d)'s only production call left the
    /// census **and the entire suite** green, with `attempt_interrupted` appended
    /// by no run. `reviews/FINDINGS.md` §4's "a refutation must name which item it
    /// inspected" is the same rule; this is it applied to the instrument.
    #[derive(Clone, Copy)]
    pub(crate) enum Call {
        /// `name(…)` or `path::name(…)` — never `receiver.name(…)`.
        Free,
        /// `receiver.name(…)`.
        Method,
    }

    /// The **files** [`declared_whole_file_test_modules`] resolves to, as a set a
    /// census can test membership in.
    ///
    /// The resolution loop — assert exactly one of the two candidates exists,
    /// collect it — was written out at each caller, and a third caller wrote a
    /// different rule instead: `path.file_stem() == "tests"`. That covers the
    /// fourteen files named `tests.rs` and **not** the three that are not:
    /// `#[cfg(test)] mod scaffold;`, `mod premove;` and `mod fake;`. Seventeen,
    /// not fourteen, and the three it missed are the ones a census is most
    /// likely to trip over, because a scaffold and a fake exist to *name* the
    /// things production names. Found by S5 round 5's `seams`, `attempt` and
    /// `settle` lenses independently; the consolidation had been filed one
    /// commit earlier in `reviews/FINDINGS.md` §20 as tidiness.
    ///
    /// # Panics
    ///
    /// When a declaration resolves to no file or to both candidates — a skip
    /// path naming no file is a skip that has stopped meaning anything — or
    /// when fewer than `floor` declarations are derived, which is the control
    /// against a derivation that has silently stopped finding anything.
    pub(crate) fn whole_file_test_modules(
        files: &[PathBuf],
        floor: usize,
    ) -> std::collections::BTreeSet<PathBuf> {
        let declarations = declared_whole_file_test_modules(files);
        assert!(
            declarations.len() >= floor,
            "only {} `#[cfg(test)] mod …;` declarations were derived and the floor is {floor}; \
             the derivation is finding nothing",
            declarations.len()
        );
        let mut modules = std::collections::BTreeSet::new();
        for (declared_in, name, candidates) in &declarations {
            let present: Vec<&PathBuf> = candidates
                .iter()
                .filter(|candidate| candidate.is_file())
                .collect();
            assert_eq!(
                present.len(),
                1,
                "`{}` declares `#[cfg(test)] mod {name};` and {} of {candidates:?} exist. A skip \
                 path naming no file is a skip that has stopped meaning anything",
                declared_in.display(),
                present.len()
            );
            modules.insert(present[0].clone());
        }
        modules
    }

    pub(crate) fn declared_whole_file_test_modules(
        files: &[PathBuf],
    ) -> Vec<(PathBuf, String, [PathBuf; 2])> {
        let mut found = Vec::new();
        for path in files {
            let blanked = super::blank_comments_and_strings(
                &std::fs::read_to_string(path).expect("read source"),
            );
            let parent = path.parent().expect("a source file has a directory");
            let stem = path.file_stem().expect("a source file has a name");
            let dir = if stem == "mod" || stem == "lib" || stem == "main" {
                parent.to_path_buf()
            } else {
                parent.join(stem)
            };
            for rest in blanked.split("#[cfg(test)]").skip(1) {
                let Some(name) = rest.trim_start().strip_prefix("mod ") else {
                    continue;
                };
                let Some(name) = name.split(';').next().map(str::trim) else {
                    continue;
                };
                if name.is_empty() || name.contains('{') {
                    continue;
                }
                found.push((
                    path.clone(),
                    name.to_owned(),
                    [
                        dir.join(format!("{name}.rs")),
                        dir.join(name).join("mod.rs"),
                    ],
                ));
            }
        }
        found
    }
}

#[cfg(test)]
mod tests;
