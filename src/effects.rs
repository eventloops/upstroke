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
    /// Which attribute keywords the governed lints were found under: `allow`,
    /// `expect`, or both if one attribute writes both.
    ///
    /// The two are not the same permission and the placement rule now
    /// distinguishes them. `allow` is unconditional and says nothing when the
    /// thing it permits stops happening; `expect` is refused by the compiler
    /// when it goes unfulfilled, which is what makes a per-site one a count the
    /// build owns rather than a claim a reviewer has to re-check.
    pub keywords: Vec<&'static str>,
    /// Whether it carries a `reason = "…"`.
    pub reasoned: bool,
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
/// * A file that declares its tests as `#[cfg(test)] mod tests;` — fourteen of
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
        let mut keywords: Vec<&'static str> = Vec::new();
        let mut reasoned = false;
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
                        let before = lints.len();
                        for entry in attribute[after + 1..end].split(',') {
                            let entry = entry.trim();
                            if entry.is_empty() {
                                continue;
                            }
                            if entry.starts_with("reason") {
                                reasoned = true;
                                continue;
                            }
                            written.push(entry.to_owned());
                            if let Some(name) = normalize_lint(entry) {
                                lints.push(name.to_owned());
                            }
                        }
                        if lints.len() > before && !keywords.contains(&keyword) {
                            keywords.push(keyword);
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
                keywords,
                reasoned,
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
    "src/agent/claude.rs",
    "src/agent/codex.rs",
    "src/agent/copilot.rs",
    "src/agent/bin.rs",
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
    /// fourteen files named `tests.rs` and **not** the four that are not:
    /// `scaffold`, `premove`, `fake` and `readiness`. Eighteen, not fourteen,
    /// and the four it misses are the ones a census is most likely to trip
    /// over, because a scaffold, a fake and a readiness protocol exist to
    /// *name* the things production names. Found by S5 round 5's `seams`,
    /// `attempt` and `settle` lenses independently; the consolidation had been
    /// filed one commit earlier in `reviews/FINDINGS.md` §20 as tidiness.
    ///
    /// # Panics
    ///
    /// When a declaration resolves to no file or to both candidates — a skip
    /// path naming no file is a skip that has stopped meaning anything — when
    /// two declarations resolve to one file, when the declaration graph is
    /// cyclic, or when fewer than `floor` declarations are derived, which is
    /// the control against a derivation that has silently stopped finding
    /// anything.
    pub(crate) fn whole_file_test_modules(
        source_root: &std::path::Path,
        files: &[PathBuf],
        floor: usize,
    ) -> std::collections::BTreeSet<PathBuf> {
        let declarations = declared_whole_file_test_modules(source_root, files);
        assert!(
            declarations.len() >= floor,
            "only {} test-only `mod …;` declarations were derived and the floor is {floor}; \
             the derivation is finding nothing",
            declarations.len()
        );
        let mut modules = std::collections::BTreeSet::new();
        let mut edges: Vec<(PathBuf, PathBuf)> = Vec::new();
        for declaration in &declarations {
            let resolved = sole_present(&declaration.candidates, &|path| path.is_file())
                .unwrap_or_else(|present| {
                    panic!(
                        "`{}` declares `mod {};` under {} and {present} of {:?} exist. A skip \
                         path naming no file is a skip that has stopped meaning anything",
                        declaration.declared_in.display(),
                        declaration.name,
                        declaration.render_guard(),
                        declaration.candidates
                    )
                })
                .clone();
            assert!(
                modules.insert(resolved.clone()),
                "two declarations resolve to `{}`; one of them is deriving a skip for a file it \
                 does not declare",
                resolved.display()
            );
            edges.push((declaration.declared_in.clone(), resolved));
        }
        // **The declaration graph is a forest.** Directory-derived candidates
        // descend, so a cycle is not reachable from this tree — which is the
        // reason to check rather than a reason not to: an unreachable path is
        // one nobody would notice becoming reachable. A `#[path]` attribute is
        // the one construct that could build one, and the scanner refuses those
        // rather than resolving them, so this assertion and that refusal are
        // one control with two halves.
        assert!(
            declaration_cycle(&edges).is_none(),
            "the module declarations are cyclic, so no file's guard can be trusted: {:?}",
            declaration_cycle(&edges)
        );
        // **The control that binds every caller**, and it belongs here rather
        // than at each of them.
        // `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
        // asserts what this *returns*; it says nothing about whether a census
        // calls it, which is the defect `3a91626` repaired for two censuses and
        // this witness then reproduced one commit later (`R6-SETTLE-003`). A
        // caller cannot reach the set without passing through this line.
        assert!(
            modules
                .iter()
                .any(|path| path.file_stem().is_none_or(|stem| stem != "tests")),
            "every module derived here is called `tests.rs`, which is exactly what the file-name \
             rule this replaces also finds. The derivation has degraded to the rule it exists to \
             be better than: {modules:?}"
        );
        modules
    }

    /// A cycle in `edges`, as the path that closes it, or `None`.
    ///
    /// `edges` is (declaring file, declared file). The derivation treats that
    /// relation as a forest — every guard is read from the file *above* — so a
    /// cycle means the traversal would either not terminate or attribute a
    /// guard to a file that does not inherit it.
    ///
    /// Pure and separately driven, because the real tree cannot produce one: a
    /// census control that is only ever exercised on input that satisfies it is
    /// a control nobody has seen refuse anything.
    pub(crate) fn declaration_cycle(edges: &[(PathBuf, PathBuf)]) -> Option<Vec<PathBuf>> {
        use std::collections::{BTreeMap, BTreeSet};

        /// Depth-first search state. `Grey` is "on the path being walked", and
        /// reaching a `Grey` node is what a back edge *is*.
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Colour {
            White,
            Grey,
            Black,
        }

        // **The full adjacency, not the first edge out of each node.** The
        // first version followed `edges.iter().find(…)`, which walks one
        // outgoing edge per node — so a node with two children whose *second*
        // child closes the loop reported no cycle. `a -> b`, `a -> c`,
        // `c -> a` was the shape, and it read as acyclic.
        let mut adjacency: BTreeMap<&PathBuf, Vec<&PathBuf>> = BTreeMap::new();
        let mut nodes: BTreeSet<&PathBuf> = BTreeSet::new();
        for (from, to) in edges {
            adjacency.entry(from).or_default().push(to);
            nodes.insert(from);
            nodes.insert(to);
        }

        let mut colour: BTreeMap<&PathBuf, Colour> =
            nodes.iter().map(|node| (*node, Colour::White)).collect();
        for start in &nodes {
            if colour.get(start) != Some(&Colour::White) {
                continue;
            }
            colour.insert(start, Colour::Grey);
            // (node, how many of its outgoing edges have been taken). The stack
            // IS the current path, which is what makes the cycle reportable.
            let mut stack: Vec<(&PathBuf, usize)> = vec![(start, 0)];
            while let Some((node, taken)) = stack.pop() {
                let outgoing: &[&PathBuf] = adjacency
                    .get(node)
                    .map_or(&[][..], |edges| edges.as_slice());
                let Some(next) = outgoing.get(taken).copied() else {
                    colour.insert(node, Colour::Black);
                    continue;
                };
                stack.push((node, taken + 1));
                match colour.get(next) {
                    Some(Colour::Grey) => {
                        let path: Vec<&PathBuf> = stack.iter().map(|(at, _)| *at).collect();
                        let from = path.iter().position(|at| *at == next).unwrap_or(0);
                        let mut cycle: Vec<PathBuf> =
                            path[from..].iter().map(|at| (*at).clone()).collect();
                        cycle.push(next.clone());
                        return Some(cycle);
                    }
                    Some(Colour::Black) => {}
                    _ => {
                        colour.insert(next, Colour::Grey);
                        stack.push((next, 0));
                    }
                }
            }
        }
        None
    }

    /// The one of `candidates` that `exists` accepts, or how many it accepted.
    ///
    /// Zero is a declaration naming no file, which is a skip that has stopped
    /// meaning anything. Two is `name.rs` and `name/mod.rs` both present, which
    /// Rust itself refuses to compile and which a resolver that took the first
    /// match would silently pick a side in. Both are refusals.
    ///
    /// `exists` is a parameter rather than a `Path::is_file` call so the two
    /// refusals can be driven: neither is reachable from this tree, and a
    /// control that has only ever seen compliant input is a control nobody has
    /// watched refuse anything. It is also what keeps this body free of an
    /// effect — the funnel section of the allowlist records `allows = []` for
    /// this file and that claim is stronger than any other entry there.
    pub(crate) fn sole_present<'a>(
        candidates: &'a [PathBuf; 2],
        exists: &dyn Fn(&std::path::Path) -> bool,
    ) -> Result<&'a PathBuf, usize> {
        let present: Vec<&PathBuf> = candidates
            .iter()
            .filter(|candidate| exists(candidate))
            .collect();
        match present.as_slice() {
            [only] => Ok(only),
            other => Err(other.len()),
        }
    }

    /// Why a declaration's candidate files cannot be named.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum CandidateRefusal {
        /// The declaring file is not inside the package the inventory was read
        /// for, so that inventory does not say whether it is a crate root.
        OutsideThePackage {
            declared_in: PathBuf,
            package_dir: PathBuf,
        },
    }

    impl std::fmt::Display for CandidateRefusal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::OutsideThePackage {
                    declared_in,
                    package_dir,
                } => write!(
                    f,
                    "`{}` is not inside `{}`, so the target inventory read for that package \
                     does not say whether it is a crate root",
                    declared_in.display(),
                    package_dir.display()
                ),
            }
        }
    }

    /// Why a package's target inventory could not be established.
    ///
    /// Every variant is a **refusal to guess**. The resolution below turns on
    /// which files Cargo compiles as crate roots, that is a fact only the
    /// manifest holds, and the previous derivation held it as a rule about file
    /// stems instead. A rule cannot be wrong quietly the way a stem test can:
    /// when the authority is unavailable the census stops.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum InventoryRefusal {
        /// `cargo metadata` could not be started at all.
        NotRun { manifest: PathBuf, why: String },
        /// It ran and exited non-zero.
        Failed {
            manifest: PathBuf,
            status: String,
            stderr: String,
        },
        /// Its output is not the JSON document this reads.
        Unreadable { manifest: PathBuf, why: String },
        /// No package in the document has that manifest path.
        NoPackage { manifest: PathBuf },
        /// The package has no targets, so nothing is a crate root.
        NoTargets { manifest: PathBuf },
    }

    impl std::fmt::Display for InventoryRefusal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::NotRun { manifest, why } => write!(
                    f,
                    "`cargo metadata` for `{}` could not be started ({why}), so the crate roots \
                     are unknown and this census will not guess them from file names",
                    manifest.display()
                ),
                Self::Failed {
                    manifest,
                    status,
                    stderr,
                } => write!(
                    f,
                    "`cargo metadata` for `{}` exited {status}: {stderr}",
                    manifest.display()
                ),
                Self::Unreadable { manifest, why } => write!(
                    f,
                    "`cargo metadata` for `{}` did not answer with the document this reads \
                     ({why})",
                    manifest.display()
                ),
                Self::NoPackage { manifest } => write!(
                    f,
                    "`cargo metadata` named no package whose manifest is `{}`",
                    manifest.display()
                ),
                Self::NoTargets { manifest } => write!(
                    f,
                    "the package at `{}` declares no target, so it has no crate root",
                    manifest.display()
                ),
            }
        }
    }

    /// **The files Cargo compiles as crate roots**, read from the manifest via
    /// `cargo metadata` rather than inferred from their names.
    ///
    /// A crate root owns its own directory; every other file owns a directory
    /// named after it. Which files are roots is a property of the *manifest*,
    /// and the previous derivation decided it from the file's stem: `lib.rs` or
    /// `main.rs` at the source root was a root, the same stem anywhere else was
    /// refused, and anything else was an ordinary module. Both halves are wrong
    /// against a manifest that says otherwise, and the second half is wrong
    /// **silently**:
    ///
    /// * `[[bin]] path = "src/tools/odd.rs"` is a crate root with an arbitrary
    ///   name. The stem rule reads it as the ordinary module `tools::odd`, so a
    ///   `mod helper;` inside it resolves to `src/tools/odd/helper.rs` when
    ///   Cargo compiles `src/tools/helper.rs`. That is a **different file** —
    ///   the same competing-sibling hazard the nested-`lib.rs` refusal was
    ///   written for, arriving through the door that refusal left open, and it
    ///   does not announce itself: with no `src/tools/odd/helper.rs` present the
    ///   wrong reading resolves rather than refusing.
    /// * `examples/probe.rs` is this tree's live instance. It is an `example`
    ///   target — a crate root — and `effects::tests::scanned_sources` walks
    ///   `examples/**`, so the stem rule already answers `examples/probe` for a
    ///   directory Cargo calls `examples`.
    /// * A nested `src/a/lib.rs` the manifest never names is the ordinary module
    ///   `a::lib`, which is decidable rather than ambiguous once the manifest is
    ///   read. The old refusal was the honest answer to not knowing; this is the
    ///   answer.
    ///
    /// Kinds are **not** filtered. `lib`, `bin`, `example`, `test`, `bench` and
    /// `custom-build` are each a crate root of their own, and a census that
    /// looked only at `lib`/`bin` would re-introduce the same class one kind at
    /// a time.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct CrateRoots {
        package_dir: PathBuf,
        roots: std::collections::BTreeSet<PathBuf>,
    }

    impl CrateRoots {
        /// The inventory in one `cargo metadata --format-version 1` document,
        /// for the package whose manifest is `manifest`.
        ///
        /// Pure over the document, which is what makes every refusal below
        /// drivable: the acquisition is a process start and lives in
        /// [`crate::effects::tests`], where this crate's governance puts one.
        pub(crate) fn from_metadata_json(
            json: &str,
            manifest: &std::path::Path,
        ) -> Result<Self, InventoryRefusal> {
            let refuse = |why: &str| InventoryRefusal::Unreadable {
                manifest: manifest.to_path_buf(),
                why: why.to_owned(),
            };
            let document: serde_json::Value =
                serde_json::from_str(json).map_err(|error| refuse(&error.to_string()))?;
            let packages = document
                .get("packages")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| refuse("no `packages` array"))?;
            let package = packages
                .iter()
                .find(|package| {
                    package
                        .get("manifest_path")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|path| std::path::Path::new(path) == manifest)
                })
                .ok_or_else(|| InventoryRefusal::NoPackage {
                    manifest: manifest.to_path_buf(),
                })?;
            let targets = package
                .get("targets")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| refuse("the package has no `targets` array"))?;
            let mut roots = std::collections::BTreeSet::new();
            for target in targets {
                let source = target
                    .get("src_path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| refuse("a target carries no `src_path`"))?;
                roots.insert(PathBuf::from(source));
            }
            if roots.is_empty() {
                return Err(InventoryRefusal::NoTargets {
                    manifest: manifest.to_path_buf(),
                });
            }
            Ok(Self {
                package_dir: manifest
                    .parent()
                    .ok_or_else(|| refuse("the manifest path has no directory"))?
                    .to_path_buf(),
                roots,
            })
        }

        /// The directory the package's manifest sits in.
        pub(crate) fn package_dir(&self) -> &std::path::Path {
            &self.package_dir
        }

        /// Every crate root, absolute, in sorted order.
        pub(crate) fn roots(&self) -> impl Iterator<Item = &std::path::Path> {
            self.roots.iter().map(PathBuf::as_path)
        }

        /// Whether `path` is one of them.
        pub(crate) fn is_root(&self, path: &std::path::Path) -> bool {
            self.roots.contains(path)
        }

        /// Whether the package-relative, `/`-separated `relative` is one.
        ///
        /// The second caller reads the tree as repo-relative slash strings
        /// rather than as paths, and one authority answering both is the point:
        /// `PR5D-VISIBILITY-CHECK-DUPLICATED` is the standing entry for the rule
        /// that got written twice, and the stem test *was* written twice — here
        /// and in `effects::tests::cfg::module_dir`.
        pub(crate) fn is_root_relative(&self, relative: &str) -> bool {
            let mut candidate = self.package_dir.clone();
            for part in relative.split('/') {
                candidate.push(part);
            }
            self.is_root(&candidate)
        }
    }

    /// The directory an out-of-line child of `declared_in` lives in.
    ///
    /// **A crate root owns its directory; an ordinary module owns a directory
    /// named after it.** `mod.rs` is the first case wherever it sits — that is
    /// what `mod.rs` means. Everything else is the first case exactly when the
    /// manifest names it as a target's path, which is what [`CrateRoots`] reads
    /// and what no rule about file names can answer.
    ///
    /// Refused rather than decided when `declared_in` is not inside the package
    /// the inventory was read for: an inventory is a statement about one
    /// package, and a file outside it is one the inventory is silent on.
    pub(crate) fn module_directory(
        roots: &CrateRoots,
        declared_in: &std::path::Path,
    ) -> Result<PathBuf, CandidateRefusal> {
        let parent = declared_in
            .parent()
            .expect("a source file has a directory")
            .to_path_buf();
        let stem = declared_in.file_stem().expect("a source file has a name");
        if roots.is_root(declared_in) {
            return Ok(parent);
        }
        if !declared_in.starts_with(roots.package_dir()) {
            return Err(CandidateRefusal::OutsideThePackage {
                declared_in: declared_in.to_path_buf(),
                package_dir: roots.package_dir().to_path_buf(),
            });
        }
        if stem == "mod" {
            return Ok(parent);
        }
        Ok(parent.join(stem))
    }

    /// The two files `mod <name>;` can name, given where it was written.
    ///
    /// `declared_in` is the declaring file; `inline_path` is the inline modules
    /// enclosing the declaration, outermost first. **The inline path is part of
    /// the directory**, which is the half a resolver reading only the file name
    /// gets wrong: `mod readiness;` inside `proc.rs`'s inline `test_support`
    /// names `proc/test_support/readiness.rs`, and flattened to
    /// `proc/readiness.rs` it names nothing — a zero-candidate refusal if you
    /// are lucky and the wrong file if you are not.
    ///
    /// `roots` is the package's target inventory, and [`module_directory`] is
    /// why that is a parameter rather than a test on the file's stem.
    pub(crate) fn candidates_for(
        roots: &CrateRoots,
        declared_in: &std::path::Path,
        inline_path: &[String],
        name: &str,
    ) -> Result<[PathBuf; 2], CandidateRefusal> {
        let mut dir = module_directory(roots, declared_in)?;
        for enclosing in inline_path {
            dir.push(enclosing);
        }
        Ok([
            dir.join(format!("{name}.rs")),
            dir.join(name).join("mod.rs"),
        ])
    }

    /// Whether `candidate` stays inside `base` through plain path components.
    ///
    /// A module name is an identifier and a candidate is `base` joined with
    /// identifiers, so this holds by construction — and is asserted anyway,
    /// because the construction is what a `#[path = "../.."]` attribute would
    /// change, and the failure it would cause is a census reading a file
    /// outside the tree as declared inside it.
    pub(crate) fn contained_in(base: &std::path::Path, candidate: &std::path::Path) -> bool {
        let Ok(rest) = candidate.strip_prefix(base) else {
            return false;
        };
        rest.components().count() > 0
            && rest
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
    }

    /// One out-of-line `mod <name>;` the crate declares as test-only.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct TestModuleDeclaration {
        /// The file the declaration is written in.
        pub(crate) declared_in: PathBuf,
        /// The declared module's name.
        pub(crate) name: String,
        /// The **inline** modules enclosing the declaration, outermost first.
        /// Empty when the declaration sits at the file's top level.
        pub(crate) inline_path: Vec<String>,
        /// The effective `cfg` predicate, rendered — the conjunction of every
        /// enclosing inline module's predicate and the declaration's own.
        pub(crate) guard: String,
        /// `[<dir>/<name>.rs, <dir>/<name>/mod.rs]`, where `<dir>` is the
        /// declaring file's module directory joined with [`Self::inline_path`].
        pub(crate) candidates: [PathBuf; 2],
    }

    impl TestModuleDeclaration {
        /// The guard and the inline path it was read through, for a diagnostic.
        fn render_guard(&self) -> String {
            if self.inline_path.is_empty() {
                format!("`cfg({})`", self.guard)
            } else {
                format!(
                    "`cfg({})` through `{}`",
                    self.guard,
                    self.inline_path.join("::")
                )
            }
        }
    }

    /// Every out-of-line module declaration the crate compiles **only** under
    /// `cfg(test)`, structurally resolved.
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
    /// that has stopped meaning anything, so [`whole_file_test_modules`] asserts
    /// that exactly one of the two candidates exists.
    ///
    /// # Structure, not a literal `#[cfg(test)] mod name;`
    ///
    /// The predicate used to be exactly that string, and it had two holes a
    /// **text** rule cannot close and a structural one closes together:
    ///
    /// * **A visibility qualifier hid the declaration.** `#[cfg(test)]
    ///   pub(crate) mod helpers;` was not matched, because the rule read `mod `
    ///   immediately after the attribute. That direction was chosen as the safe
    ///   one — failing to derive a skip leaves a test file in a census's domain,
    ///   where a fixture reads as an offender and someone looks — and it is
    ///   still the safe direction. It stopped being *necessary*: the scan below
    ///   reads the item, so a qualifier is transparent rather than fatal.
    /// * **An inline ancestor carried the guard.** `#[cfg(test)] mod
    ///   test_support { … mod readiness; }` compiles `readiness.rs` only under
    ///   `cfg(test)`, and the declaration inside carries no attribute at all.
    ///   `src/agent/proc/test_support/readiness.rs` is that file; without the
    ///   ancestry it is a whole test file with no `#[cfg(test)]` anywhere in it,
    ///   which is precisely the shape every census here exists to skip.
    ///
    /// So the scan walks each file's **module structure**: brace depth, the
    /// inline modules open at each point, and the `cfg` predicates on each of
    /// them. A declaration is test-only when the conjunction of its own
    /// predicate and every enclosing inline module's predicate is false
    /// wherever `test` is false — [`entails_test`].
    ///
    /// # What it deliberately does not do
    ///
    /// **No transitive closure over files.** `src/effects/tests.rs` is itself a
    /// whole-file test module and declares `mod policy;`, so Rust compiles
    /// `src/effects/tests/policy.rs` only under `cfg(test)` too — and this
    /// derivation does not say so. Every census in this crate reads
    /// [`super::production_code`], which removes `#[cfg(test)]` items from the
    /// files it keeps, and those second-level files carry their own inline
    /// `cfg(test)` modules and their own `#![deny]` prologues for exactly that
    /// reason (`effects/tests/classification.rs` and its siblings say so at
    /// length). Closing over the file graph would widen the skip set by a dozen
    /// files whose contents no census has been measured against, which is a
    /// change to what every census can see and not a bug fix. The measured
    /// domain is the eighteen
    /// `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
    /// names and counts — fourteen literal `#[cfg(test)] mod tests;`, plus
    /// `scaffold`, `premove`, `fake` and `readiness`. A nineteenth arrives with
    /// the slice that measures it.
    ///
    /// **No `#[path]`.** A `#[path]` attribute on a module is refused rather
    /// than resolved: it is the one construct that can point a declaration
    /// outside its own directory, and there are none in this tree.
    ///
    /// # Panics
    ///
    /// When a file cannot be read structurally at all — an attribute that never
    /// closes, a brace that closes one too many, a `mod` with no name or no
    /// terminator, a `cfg` predicate the entailment grammar cannot read, a
    /// `#[path]`, or one name declared twice in one module. Every one of those
    /// means the scan does not know what the file declares, and a scan that
    /// does not know must not answer.
    pub(crate) fn declared_whole_file_test_modules(
        source_root: &std::path::Path,
        files: &[PathBuf],
    ) -> Vec<TestModuleDeclaration> {
        let roots = crate::effects::tests::crate_roots();
        // **The inventory has to describe the tree being walked.** `source_root`
        // is the caller's claim about where the crate's sources live, and the
        // manifest's is the target paths; a `source_root` no target sits under
        // means the two are about different trees, and every answer below would
        // be resolved against an inventory that says nothing about the files in
        // hand. Fail closed, in the same breath as the acquisition itself.
        assert!(
            roots.roots().any(|root| root.starts_with(source_root)),
            "no target of the package at `{}` lives under `{}`, so its inventory does not \
             describe the tree this census was handed: {:?}",
            roots.package_dir().display(),
            source_root.display(),
            roots.roots().collect::<Vec<_>>()
        );
        let mut found = Vec::new();
        for path in files {
            let source = std::fs::read_to_string(path).expect("read source");
            let declarations = scan_module_declarations(&source)
                .unwrap_or_else(|refusal| panic!("{}: {refusal}", path.display()));
            let parent = path.parent().expect("a source file has a directory");
            for declaration in declarations {
                if !declaration.test_only {
                    continue;
                }
                let candidates =
                    candidates_for(roots, path, &declaration.inline_path, &declaration.name)
                        .unwrap_or_else(|refusal| panic!("{refusal}"));
                for candidate in &candidates {
                    assert!(
                        contained_in(parent, candidate),
                        "`{}` declares `mod {};` and the candidate `{}` leaves `{}`",
                        path.display(),
                        declaration.name,
                        candidate.display(),
                        parent.display()
                    );
                }
                found.push(TestModuleDeclaration {
                    declared_in: path.clone(),
                    name: declaration.name,
                    inline_path: declaration.inline_path,
                    guard: declaration.guard,
                    candidates,
                });
            }
        }
        found
    }

    /// One `mod` declaration as the scan read it out of a file's structure.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct ScannedDeclaration {
        /// The declared module's name.
        pub(crate) name: String,
        /// The inline modules enclosing it, outermost first.
        pub(crate) inline_path: Vec<String>,
        /// The effective predicate, rendered.
        pub(crate) guard: String,
        /// Whether that predicate is false wherever `test` is false.
        pub(crate) test_only: bool,
    }

    /// Why a file's structure could not be read, and where.
    ///
    /// Every variant is a refusal rather than a guess. The direction is the one
    /// [`declared_whole_file_test_modules`] argues for: a scan that cannot tell
    /// what a file declares must not answer, because both wrong answers are
    /// silent — a missing skip reports a fixture as an offender, and a spurious
    /// one removes a production file from every census below.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum ScanRefusal {
        /// `#[…` with no `]`.
        UnclosedAttribute { line: usize },
        /// A `}` with no `{`.
        UnbalancedBraces { line: usize },
        /// `mod` with no name, or a name followed by neither `;` nor `{`.
        MalformedDeclaration { line: usize },
        /// A `cfg` predicate the entailment grammar cannot read.
        UnreadablePredicate {
            line: usize,
            written: String,
            why: String,
        },
        /// `#[path = "…"]`, or a `cfg_attr` that could apply one.
        UnsupportedPathAttribute { line: usize, name: String },
        /// An inner `#![cfg(…)]`, which gates the module it is written in.
        UnsupportedInnerCfg { line: usize },
        /// One module name declared twice in one module.
        DuplicateDeclaration { line: usize, name: String },
        /// A macro body holding a module-shaped token sequence.
        ModuleShapedMacroBody { line: usize, macro_name: String },
    }

    impl std::fmt::Display for ScanRefusal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::UnclosedAttribute { line } => {
                    write!(f, "line {line}: an attribute is never closed")
                }
                Self::UnbalancedBraces { line } => {
                    write!(
                        f,
                        "line {line}: a `}}` closes a block that was never opened"
                    )
                }
                Self::MalformedDeclaration { line } => write!(
                    f,
                    "line {line}: a `mod` declaration has no name, or no `;` or `{{` after it"
                ),
                Self::UnreadablePredicate { line, written, why } => write!(
                    f,
                    "line {line}: `cfg({written})` cannot be decided against `test`: {why}"
                ),
                Self::UnsupportedPathAttribute { line, name } => write!(
                    f,
                    "line {line}: `mod {name}` carries a `path` attribute, which this \
                     derivation refuses rather than resolves"
                ),
                Self::UnsupportedInnerCfg { line } => write!(
                    f,
                    "line {line}: an inner `#![cfg(…)]` gates the module it is written in, \
                     which this derivation does not model"
                ),
                Self::DuplicateDeclaration { line, name } => {
                    write!(
                        f,
                        "line {line}: `mod {name};` is declared twice in one module"
                    )
                }
                Self::ModuleShapedMacroBody { line, macro_name } => write!(
                    f,
                    "line {line}: the body of `{macro_name}!` holds a module-shaped token \
                     sequence. A macro body is token trees, not items, and whether the \
                     expansion declares a module is not readable from here"
                ),
            }
        }
    }

    /// Every `mod` declaration in `source`, with the inline modules enclosing it
    /// and the effective `cfg` predicate it inherits.
    ///
    /// Pure over `&str`, which is what makes the refusals above drivable: the
    /// tree satisfies every one of them, so the only way to see one is to hand
    /// this a source that does not.
    ///
    /// Comments and string literals are blanked first —
    /// [`super::blank_comments_and_strings`], which also handles raw strings,
    /// byte strings and char literals — so a `mod` written in prose is spaces.
    /// The predicate text is read from the **raw** span at the same offsets,
    /// because blanking erases what is inside a string and `feature = "x"` would
    /// otherwise arrive as `feature = "   "`.
    pub(crate) fn scan_module_declarations(
        source: &str,
    ) -> Result<Vec<ScannedDeclaration>, ScanRefusal> {
        /// An inline `mod name { … }` that is open at the current position.
        struct Scope {
            /// The brace depth *outside* the module's body.
            open_depth: usize,
            name: String,
            preds: Vec<Predicate>,
            declared: std::collections::BTreeSet<String>,
        }

        let blanked = super::blank_comments_and_strings(source);
        debug_assert_eq!(blanked.len(), source.len());
        let bytes = blanked.as_bytes();
        let line_of = |at: usize| blanked[..at].matches('\n').count() + 1;

        let mut found = Vec::new();
        let mut scopes: Vec<Scope> = Vec::new();
        let mut top_level: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut pending: Vec<Predicate> = Vec::new();
        let mut pending_path = false;
        let mut depth = 0_usize;
        let mut i = 0;

        while i < bytes.len() {
            let byte = bytes[i];
            if byte.is_ascii_whitespace() {
                i += 1;
                continue;
            }

            // -- an attribute, which belongs to whatever item comes next -----
            if byte == b'#' {
                let inner = bytes.get(i + 1) == Some(&b'!');
                let open = if inner { i + 2 } else { i + 1 };
                if bytes.get(open) != Some(&b'[') {
                    i += 1;
                    continue;
                }
                let Some(close) = super::matching(bytes, open, b'[', b']') else {
                    return Err(ScanRefusal::UnclosedAttribute { line: line_of(i) });
                };
                let raw = &source[open + 1..close];
                let name = raw
                    .trim_start()
                    .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                    .next()
                    .unwrap_or_default();
                match name {
                    "cfg" => {
                        let written = raw
                            .trim()
                            .strip_prefix("cfg")
                            .map(str::trim_start)
                            .and_then(|rest| rest.strip_prefix('('))
                            .and_then(|rest| rest.strip_suffix(')'))
                            .unwrap_or_default()
                            .trim();
                        let pred = parse_predicate(written).map_err(|why| {
                            ScanRefusal::UnreadablePredicate {
                                line: line_of(i),
                                written: written.to_owned(),
                                why,
                            }
                        })?;
                        if inner {
                            return Err(ScanRefusal::UnsupportedInnerCfg { line: line_of(i) });
                        }
                        pending.push(pred);
                    }
                    // `path` names the file directly; `cfg_attr` can apply one
                    // conditionally. Both are refused where they could reach a
                    // module, which is decided when the item is read.
                    "path" => pending_path = true,
                    "cfg_attr" if raw.contains("path") => pending_path = true,
                    _ => {}
                }
                i = close + 1;
                continue;
            }

            // -- a macro, whose body is token trees and not items ------------
            //
            // `mod x;` inside `macro_rules! m { () => { mod x; } }` is not a
            // declaration, and `#[cfg(test)] mod x;` inside one is not a
            // test-only declaration: the tokens are only *shaped* like an item
            // until something expands them. Walking into a macro body therefore
            // invents declarations, which is the direction that removes a real
            // production file from every census below.
            //
            // A macro invoked at item position **can** expand to a module,
            // though, and this scan cannot tell which does. So the body is
            // discarded when it holds nothing module-shaped and refused when it
            // does: the discard is what stops the false positives, and the
            // refusal is what stops the discard from becoming a blind spot.
            // Measured on this tree: zero macro bodies hold one.
            if let Some(invocation) = macro_at(bytes, i) {
                let MacroInvocation { name, open, close } = invocation;
                if let Some(shaped) = module_shaped_between(bytes, open + 1, close) {
                    return Err(ScanRefusal::ModuleShapedMacroBody {
                        line: line_of(shaped),
                        macro_name: name,
                    });
                }
                // Attributes stacked above a macro invocation belong to it.
                pending.clear();
                pending_path = false;
                i = close + 1;
                continue;
            }

            // -- a `mod` item, with any visibility qualifier in front of it ---
            if let Some(shape) = module_at(bytes, i) {
                let ModuleShape {
                    name_at,
                    name,
                    body,
                } = shape;
                if name.is_empty() {
                    return Err(ScanRefusal::MalformedDeclaration { line: line_of(i) });
                }
                if pending_path {
                    return Err(ScanRefusal::UnsupportedPathAttribute {
                        line: line_of(i),
                        name,
                    });
                }
                let mut preds: Vec<Predicate> = scopes
                    .iter()
                    .flat_map(|scope| scope.preds.iter().cloned())
                    .collect();
                preds.extend(pending.iter().cloned());
                match body {
                    Some(brace) => {
                        scopes.push(Scope {
                            open_depth: depth,
                            name,
                            preds: std::mem::take(&mut pending),
                            declared: std::collections::BTreeSet::new(),
                        });
                        depth += 1;
                        i = brace + 1;
                    }
                    None => {
                        let declared = match scopes.last_mut() {
                            Some(scope) => &mut scope.declared,
                            None => &mut top_level,
                        };
                        if !declared.insert(name.clone()) {
                            return Err(ScanRefusal::DuplicateDeclaration {
                                line: line_of(name_at),
                                name,
                            });
                        }
                        let effective = Predicate::all(preds);
                        found.push(ScannedDeclaration {
                            name,
                            inline_path: scopes.iter().map(|scope| scope.name.clone()).collect(),
                            guard: effective.render(),
                            test_only: entails_test(&effective),
                        });
                        pending.clear();
                        // Past the `;`.
                        i = bytes[name_at..]
                            .iter()
                            .position(|byte| *byte == b';')
                            .map_or(bytes.len(), |at| name_at + at + 1);
                    }
                }
                pending_path = false;
                continue;
            }

            // -- anything else: the attributes above it are not a module's ---
            pending.clear();
            pending_path = false;
            if byte == b'{' {
                depth += 1;
                i += 1;
                continue;
            }
            if byte == b'}' {
                if depth == 0 {
                    return Err(ScanRefusal::UnbalancedBraces { line: line_of(i) });
                }
                depth -= 1;
                scopes.retain(|scope| scope.open_depth < depth);
                i += 1;
                continue;
            }
            if super::is_ident_byte(byte) {
                // **Past the whole token, `r#` included.** This advanced by
                // `is_ident_byte`, and a raw identifier is not a run of
                // identifier bytes: `r#mod` is `r`, a `#`, and `mod`. So the
                // scan consumed the `r`, met the `#`, stepped over it as a
                // non-attribute byte, and then read the *inside* of the token
                // as though it stood at item position. `let r#mod = 1;` — valid
                // Rust — became `mod = 1;`, a `mod` item with no name, and the
                // whole file was refused; `use std::r#mod as tests;` inside a
                // `#[cfg(test)]` module became `mod as;`, a test-only
                // declaration the crate never wrote, whose skip names a file
                // that does not exist. [`word`] is the token this scan reads
                // everywhere else, and the fallback reads it too now.
                i = word(bytes, i).end;
                continue;
            }
            i += 1;
        }
        Ok(found)
    }

    /// A macro invocation or `macro_rules!` definition and its delimited body.
    struct MacroInvocation {
        /// The macro's name, for the diagnostic.
        name: String,
        /// The index of the body's opening delimiter.
        open: usize,
        /// The index of the matching closing delimiter.
        close: usize,
    }

    /// [`MacroInvocation`] beginning at `at`, or `None`.
    ///
    /// The shape is an identifier, `!`, an optional second identifier — that is
    /// `macro_rules! name { … }`, the one form that has one — and a delimited
    /// group. Requiring the group is what keeps `a != b` out: after that `!`
    /// comes `=`, which opens nothing. Requiring the second identifier only
    /// after `macro_rules` is what keeps `if !condition { … }` out, which
    /// otherwise reads as an invocation of `if` whose body is the block.
    fn macro_at(bytes: &[u8], at: usize) -> Option<MacroInvocation> {
        let name = word(bytes, at);
        let after_name = name.end;
        if name.text.is_empty() {
            return None;
        }
        // **A keyword before a `!` is unary negation, not a macro name.**
        // `if !(cond)`, `while !(cond)`, `return !(x)` are identifier, `!`,
        // delimited group -- the same three tokens as `foo!(…)` -- so reading
        // them as macros skips the grouped expression, and a `mod` written
        // inside it (`if !({ mod local {} true })` is valid Rust) then reads as
        // a module-shaped macro body and refuses the whole file. A macro's path
        // segment cannot be a keyword unless it is written raw, and `r#if!(…)`
        // is a macro called `if`, so the test is on the plain spelling only.
        if !name.raw && is_keyword(name.text) {
            return None;
        }
        // **Whitespace and comments may sit between the name and its `!`.**
        // `macro_rules ! m { … }` and `quote /* why */ ! { … }` are both valid
        // Rust, and `#[rustfmt::skip]` keeps either spelling in a real file —
        // so requiring the `!` to be the very next byte made the guard miss
        // exactly the macros somebody had gone out of their way to space out.
        // Comments are already spaces in the view this reads, so one skip
        // covers both.
        let bang = whitespace(bytes, after_name);
        if bytes.get(bang) != Some(&b'!') {
            return None;
        }
        let mut cursor = whitespace(bytes, bang + 1);
        // `macro_rules! name { … }` is the **only** form carrying a name
        // between the `!` and the body, and reading one for every macro is
        // what would make `if !condition { … }` an invocation of `if` once the
        // gap above is allowed: identifier, `!`, identifier, delimiter — and
        // the whole block would be skipped. Keyed on the one name that has it.
        if !name.raw && name.text == b"macro_rules" {
            // The defined name may itself be raw -- `macro_rules! r#mod { … }`
            // is how a macro takes a keyword for a name.
            let defined = word(bytes, cursor);
            if !defined.text.is_empty() {
                cursor = whitespace(bytes, defined.end);
            }
        }
        let (opener, closer) = match bytes.get(cursor) {
            Some(b'(') => (b'(', b')'),
            Some(b'[') => (b'[', b']'),
            Some(b'{') => (b'{', b'}'),
            _ => return None,
        };
        let close = super::matching(bytes, cursor, opener, closer)?;
        Some(MacroInvocation {
            name: String::from_utf8_lossy(name.text).into_owned(),
            open: cursor,
            close,
        })
    }

    /// Where a module-shaped token sequence starts inside `from..to`, if any.
    ///
    /// "Module-shaped" is the word `mod`, a name, and a `;` or `{` — the same
    /// three tokens [`module_at`] reads, minus the visibility prefix, because
    /// what matters here is only whether the body *could* expand to a module.
    fn module_shaped_between(bytes: &[u8], from: usize, to: usize) -> Option<usize> {
        let mut at = from;
        while at < to {
            if !super::is_ident_byte(bytes[at]) {
                at += 1;
                continue;
            }
            let keyword = word(bytes, at);
            if !keyword.raw && keyword.text == b"mod" {
                let name_at = whitespace(bytes, keyword.end);
                if name_at > keyword.end {
                    // The name may be raw: `mod r#type;` inside a macro body is
                    // as module-shaped as `mod tests;` is.
                    let declared = word(bytes, name_at);
                    if !declared.text.is_empty()
                        && matches!(
                            bytes.get(whitespace(bytes, declared.end)),
                            Some(b';' | b'{')
                        )
                    {
                        return Some(at);
                    }
                }
            }
            at = keyword.end;
        }
        None
    }

    /// The identifier at `from`, and where it ends. Empty when there is none.
    fn identifier(bytes: &[u8], from: usize) -> (usize, &[u8]) {
        let mut end = from;
        while end < bytes.len() && super::is_ident_byte(bytes[end]) {
            end += 1;
        }
        (end, &bytes[from..end])
    }

    /// One identifier token, raw or plain.
    struct Word<'a> {
        /// Where the token ends, `r#` included.
        end: usize,
        /// Whether it was written `r#name`.
        raw: bool,
        /// The name, without any `r#`.
        text: &'a [u8],
    }

    /// The identifier token at `from`, reading `r#name` as one token.
    ///
    /// **A raw identifier is one token and its name may be a keyword.** That is
    /// the whole reason this exists: `mod r#type;` declares a module called
    /// `type`, and a reader that stopped at the `#` saw `mod r` followed by
    /// something that is not a terminator and refused the file. `raw` is an
    /// ordinary identifier that merely begins with the same letter, so the
    /// prefix counts only when a `#` and an identifier byte follow it.
    fn word(bytes: &[u8], from: usize) -> Word<'_> {
        if bytes.get(from) == Some(&b'r')
            && bytes.get(from + 1) == Some(&b'#')
            && bytes
                .get(from + 2)
                .is_some_and(|byte| super::is_ident_byte(*byte))
        {
            let (end, text) = identifier(bytes, from + 2);
            return Word {
                end,
                raw: true,
                text,
            };
        }
        let (end, text) = identifier(bytes, from);
        Word {
            end,
            raw: false,
            text,
        }
    }

    /// Rust's keywords, strict and reserved.
    ///
    /// **A keyword cannot be a macro's path segment**, and that is the only
    /// structural thing separating `if !(…)` from `foo!(…)`: both are an
    /// identifier, a `!` and a delimited group. Written raw it can --
    /// `r#if!(…)` is a macro named `if` -- which is why [`Word`] carries that
    /// bit rather than only the text.
    const KEYWORDS: &[&[u8]] = &[
        b"as",
        b"break",
        b"const",
        b"continue",
        b"crate",
        b"dyn",
        b"else",
        b"enum",
        b"extern",
        b"false",
        b"fn",
        b"for",
        b"if",
        b"impl",
        b"in",
        b"let",
        b"loop",
        b"match",
        b"mod",
        b"move",
        b"mut",
        b"pub",
        b"ref",
        b"return",
        b"self",
        b"Self",
        b"static",
        b"struct",
        b"super",
        b"trait",
        b"true",
        b"type",
        b"unsafe",
        b"use",
        b"where",
        b"while",
        b"async",
        b"await",
        b"dyn",
        b"abstract",
        b"become",
        b"box",
        b"do",
        b"final",
        b"macro",
        b"override",
        b"priv",
        b"typeof",
        b"unsized",
        b"virtual",
        b"yield",
        b"try",
        b"gen",
    ];

    /// Whether `text` is a Rust keyword written plainly.
    fn is_keyword(text: &[u8]) -> bool {
        KEYWORDS.contains(&text)
    }

    /// The first non-whitespace index at or after `from`.
    fn whitespace(bytes: &[u8], from: usize) -> usize {
        let mut at = from;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        at
    }

    /// A `mod` item beginning at `at`, past any visibility qualifier.
    struct ModuleShape {
        /// Where the module's name starts.
        name_at: usize,
        name: String,
        /// The index of the body's `{`, or `None` for `mod name;`.
        body: Option<usize>,
    }

    /// [`ModuleShape`] at `at`, or `None` when this is not a `mod` item.
    ///
    /// `pub`, `pub(crate)`, `pub(super)` and `pub(in a::b)` are transparent:
    /// they are read and stepped over rather than treated as the start of some
    /// other item, which is the whole of what "visibility-qualified declaration"
    /// costs a structural scan. A text rule keyed on `mod ` immediately after
    /// the attribute could not do it, and that is the hole this closes.
    fn module_at(bytes: &[u8], at: usize) -> Option<ModuleShape> {
        // Raw-aware throughout: `r#pub` and `r#mod` are identifiers named for
        // keywords, not the keywords, and neither opens a module item.
        let mut token = word(bytes, at);
        if !token.raw && token.text == b"pub" {
            let after = whitespace(bytes, token.end);
            let cursor = if bytes.get(after) == Some(&b'(') {
                super::matching(bytes, after, b'(', b')')? + 1
            } else {
                after
            };
            token = word(bytes, whitespace(bytes, cursor));
        }
        if token.raw || token.text != b"mod" {
            return None;
        }
        // `mod` and the name must be separated: `models` is not `mod els`.
        let after_keyword = whitespace(bytes, token.end);
        if after_keyword == token.end {
            return None;
        }
        // The declared name is the identifier without its `r#`: `mod r#type;`
        // names `type.rs`, the way rustc resolves it.
        let declared = word(bytes, after_keyword);
        let name_end = declared.end;
        let name = String::from_utf8_lossy(declared.text).into_owned();
        let terminator = whitespace(bytes, name_end);
        match bytes.get(terminator) {
            Some(b'{') => Some(ModuleShape {
                name_at: after_keyword,
                name,
                body: Some(terminator),
            }),
            Some(b';') => Some(ModuleShape {
                name_at: after_keyword,
                name,
                body: None,
            }),
            // A name with neither terminator is malformed, and the caller
            // refuses it. Reported through an empty-bodied shape so the caller
            // sees the position rather than silently skipping the item.
            _ => Some(ModuleShape {
                name_at: after_keyword,
                name: String::new(),
                body: None,
            }),
        }
    }

    /// A `cfg` predicate, reduced to the one question this module asks of it.
    ///
    /// `effects::tests::cfg` models predicates *properly* — every `target_os`,
    /// every CI valuation, which platform compiles which body — and answers a
    /// different question with them. This decides one: is the predicate false
    /// wherever `test` is false. So every atom that is not `test` collapses to
    /// [`Predicate::Other`], and the grammar below is the whole of what the
    /// derivation reads. A predicate it cannot parse is a refusal, not a guess.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Predicate {
        /// The `test` atom itself.
        Test,
        /// Any other atom: a bare name, or `key = "value"`.
        Other(String),
        /// `all(…)`, and the conjunction an inline ancestry composes.
        All(Vec<Predicate>),
        /// `any(…)`.
        Any(Vec<Predicate>),
        /// `not(…)`.
        Not(Box<Predicate>),
    }

    impl Predicate {
        /// The conjunction of `parts`, flattened; the empty one is `All([])`,
        /// which is true and entails nothing.
        fn all(parts: Vec<Predicate>) -> Self {
            if parts.len() == 1 {
                parts.into_iter().next().unwrap_or(Self::All(Vec::new()))
            } else {
                Self::All(parts)
            }
        }

        /// The predicate as it reads, for a diagnostic.
        pub(crate) fn render(&self) -> String {
            fn join(parts: &[Predicate]) -> String {
                parts
                    .iter()
                    .map(Predicate::render)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            match self {
                Self::Test => "test".to_owned(),
                Self::Other(written) => written.clone(),
                Self::All(parts) if parts.is_empty() => "true".to_owned(),
                Self::All(parts) => format!("all({})", join(parts)),
                Self::Any(parts) => format!("any({})", join(parts)),
                Self::Not(inner) => format!("not({})", inner.render()),
            }
        }
    }

    /// Whether `predicate` is false wherever `test` is false.
    ///
    /// Three-valued, with `test` bound to false and every other atom left
    /// *unknown* — which is the only sound reading, because this module knows
    /// nothing about platforms or features and must not pretend to. `all(test,
    /// unix)` entails; `any(test, unix)` does not, because a Unix build without
    /// `test` compiles it; `not(test)` does not.
    pub(crate) fn entails_test(predicate: &Predicate) -> bool {
        matches!(decide_without_test(predicate), Some(false))
    }

    /// `predicate` with `test = false` and every other atom unknown.
    fn decide_without_test(predicate: &Predicate) -> Option<bool> {
        match predicate {
            Predicate::Test => Some(false),
            Predicate::Other(_) => None,
            Predicate::Not(inner) => decide_without_test(inner).map(|value| !value),
            // Short-circuiting, and the `None` arms are the point: one
            // undecidable conjunct does not make a conjunction undecidable if
            // another is already false, and one undecidable disjunct does not
            // make a disjunction undecidable if another is already true. The
            // empty forms answer as `cfg` does -- `all()` is true, `any()` is
            // false.
            Predicate::All(parts) => {
                let mut every_part_is_true = true;
                for part in parts {
                    match decide_without_test(part) {
                        Some(false) => return Some(false),
                        Some(true) => {}
                        None => every_part_is_true = false,
                    }
                }
                every_part_is_true.then_some(true)
            }
            Predicate::Any(parts) => {
                let mut every_part_is_false = true;
                for part in parts {
                    match decide_without_test(part) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => every_part_is_false = false,
                    }
                }
                every_part_is_false.then_some(false)
            }
        }
    }

    /// `written` as a [`Predicate`], or why it cannot be read.
    ///
    /// The grammar is `all(…)`, `any(…)`, `not(P)`, and an atom — a bare name
    /// or `name = "value"`. Anything else is refused: an unknown combinator, an
    /// unbalanced paren, `not` with other than one argument, an empty atom.
    pub(crate) fn parse_predicate(written: &str) -> Result<Predicate, String> {
        let text = written.trim();
        if text.is_empty() {
            return Err("the predicate is empty".to_owned());
        }
        let name_end = text
            .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
            .unwrap_or(text.len());
        let (name, rest) = text.split_at(name_end);
        let rest = rest.trim_start();
        if !rest.starts_with('(') {
            // An atom: `test`, `unix`, or `key = "value"`.
            if name.is_empty() {
                return Err(format!("`{text}` does not begin with a name"));
            }
            if rest.is_empty() {
                return Ok(if name == "test" {
                    Predicate::Test
                } else {
                    Predicate::Other(name.to_owned())
                });
            }
            let Some(value) = rest.strip_prefix('=') else {
                return Err(format!("`{text}` is neither an atom nor a combinator"));
            };
            if value.trim().is_empty() {
                return Err(format!("`{name}` is compared with nothing"));
            }
            return Ok(Predicate::Other(text.to_owned()));
        }
        let inner = split_arguments(rest)?;
        let parts = inner
            .into_iter()
            .map(parse_predicate)
            .collect::<Result<Vec<_>, _>>()?;
        match name {
            "all" => Ok(Predicate::All(parts)),
            "any" => Ok(Predicate::Any(parts)),
            "not" => match <[Predicate; 1]>::try_from(parts) {
                Ok([only]) => Ok(Predicate::Not(Box::new(only))),
                Err(parts) => Err(format!("`not` takes one predicate, not {}", parts.len())),
            },
            other => Err(format!("`{other}(…)` is not a predicate combinator")),
        }
    }

    /// The comma-separated arguments of a parenthesised group starting at `(`.
    fn split_arguments(text: &str) -> Result<Vec<&str>, String> {
        let bytes = text.as_bytes();
        let mut depth = 0_usize;
        let mut close = None;
        let mut quoted = false;
        for (at, byte) in bytes.iter().enumerate() {
            match byte {
                b'"' => quoted = !quoted,
                b'(' if !quoted => depth += 1,
                b')' if !quoted => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(at);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            return Err(format!("`{text}` has an unbalanced parenthesis"));
        };
        if !text[close + 1..].trim().is_empty() {
            return Err(format!("`{text}` has text after its closing parenthesis"));
        }
        let body = &text[1..close];
        if body.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut parts = Vec::new();
        let mut depth = 0_usize;
        let mut quoted = false;
        let mut from = 0;
        for (at, byte) in body.bytes().enumerate() {
            match byte {
                b'"' => quoted = !quoted,
                b'(' if !quoted => depth += 1,
                b')' if !quoted => depth -= 1,
                b',' if !quoted && depth == 0 => {
                    parts.push(&body[from..at]);
                    from = at + 1;
                }
                _ => {}
            }
        }
        let last = &body[from..];
        if !last.trim().is_empty() {
            parts.push(last);
        }
        Ok(parts)
    }
}

/// The **file-module-level lint state** reader, for the governance censuses.
///
/// `#[cfg(test)]` and `pub(crate)`, and both halves are the point. This is a
/// census instrument, not a product API: nothing the binary does consults it,
/// and a `pub fn` here would have been a shipped surface added for a test to
/// call. It sits at the BOTTOM beside [`census_domain`] for the same reason
/// that module does — `production_region` cuts a file at its first
/// `#[cfg(test)]` and
/// `effects::tests::every_production_region_that_stops_early_stops_at_a_module`
/// pins the ten files whose cut lands on something that is not a module. This
/// file is not one of them and must not become one.
#[cfg(test)]
pub(crate) mod lint_levels {
    /// How a file's prologue resolves for one lint: the level **in force**, and
    /// whether rustc refuses the prologue outright.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Resolution {
        /// The level governing the file module, or `None` when its prologue
        /// states none and the lint is left at whatever it inherits.
        pub(crate) level: Option<&'static str>,
        /// A later attribute tried to weaken a `forbid`. rustc answers `E0453`
        /// and the crate does not compile, so this is not a level at all — it is
        /// the file failing to build, and a reader that folded it into a level
        /// would report a governance state for a file that has none.
        pub(crate) refused_downgrade: bool,
        /// The prologue says something about this lint that the reader cannot
        /// resolve to one answer for every supported target — a `cfg_attr`
        /// whose condition names a target, a feature or `test`, or an
        /// attribute shape the reader does not understand.
        ///
        /// It is a **refusal**, not a level: `level` is `None` whenever it is
        /// set, so every census that asks "has this module closed the hole"
        /// is told no. The field exists so a test can tell a prologue that
        /// says nothing apart from one that says something unprovable; a
        /// census does not need to, because both must be loud.
        pub(crate) ambiguous: bool,
    }

    /// [`Resolution`] for `lint` over `source`'s file-module prologue.
    ///
    /// "File-module level" is the whole of the claim, and it is narrower than
    /// "somewhere in the file". A lint level is scoped by the module tree, so
    /// `#![deny(clippy::disallowed_types)]` in a file's prologue governs the file
    /// and everything nested in it — while `#[deny(clippy::disallowed_types)]`
    /// written on a single `fn` governs that function and says nothing whatever
    /// about the file, which goes on inheriting whatever its ancestors allow.
    /// A scan that accepts the second in place of the first reports a module as
    /// having stated its own level when it has not, which is `PR6-LANEF-004`
    /// answered by the wrong evidence.
    ///
    /// So the walk is: from the first byte, over whitespace and **inner**
    /// attributes only, stopping at the first token that is neither. That is
    /// exactly the region an `#![…]` may govern the file module from, and it is the
    /// same rule [`super::is_module_level`] applies to the inner half of its answer.
    ///
    /// # Ordered, because rustc is ordered
    ///
    /// `PR72-LEVELS-001`. This used to return at the **first** attribute naming
    /// the lint, which is not what a prologue means. Lint levels at one scope
    /// are applied in source order and the last one wins, so
    /// `#![deny(L)] #![allow(L)]` is a file where `L` is **allowed** — and the
    /// first-match reader called it a denial. That is the failure direction that
    /// matters: a census asking "has this module closed the hole" was told yes
    /// by a prologue whose second line reopens it, and the reopening line is
    /// exactly what an author adding an exception writes.
    ///
    /// `forbid` is not symmetrical with the rest and is not modelled as if it
    /// were. Once a lint is forbidden at a scope, a later `allow`, `warn` or
    /// `expect` of it is `E0453` — the crate does not compile — while a later
    /// `deny` or `forbid` is accepted and leaves the forbid in force. Both halves
    /// are **measured** rather than reasoned: every row of
    /// `effects::tests::the_file_level_lint_reader_answers_what_rustc_does` is
    /// compiled by `clippy-driver` and this reader's answer is checked against
    /// the diagnostics that come back, so no sentence here is the authority for
    /// what the compiler does.
    ///
    /// # What it deliberately does not do
    ///
    /// **Lint groups are not expanded.** `#![deny(clippy::all)]` denies this
    /// lint to rustc and reads as `None` here. The direction is the safe one — a
    /// census is told a module states nothing when it states something, which is
    /// loud — and the tree is measured rather than trusted:
    /// [`tests::the_three_blunt_governed_lints_are_used_by_nobody`] asserts that
    /// `clippy::all`, `clippy::style` and `warnings` are used by no file at all.
    ///
    /// Comments and string literals are blanked first, so a level quoted in a doc
    /// comment or inside a `&str` is invisible — `PR4-CENSUS-COMMENT-ORACLE`, and
    /// this crate's effect fixtures are written as exactly those two shapes.
    ///
    /// `clippy::disallowed_methods` and `disallowed_methods` are the same lint;
    /// [`super::normalize_lint`] is the bridge, as it is everywhere else here.
    ///
    /// # Structural, because a prefix match is not a parser
    ///
    /// `PR57-FINAL-001`. This used to read an attribute by stripping a level
    /// keyword off the front of it, so it understood one shape — `deny(L)`
    /// written literally — and was blind to every attribute that wraps one.
    /// `#![cfg_attr(P, deny(L))]` read as stating nothing, which is loud and
    /// merely wrong; but a prologue that DENIES the lint and then takes it back
    ///
    /// ```text
    /// #![deny(clippy::disallowed_methods)]
    /// #![cfg_attr(windows, allow(clippy::disallowed_methods))]
    /// ```
    ///
    /// read as `deny`, and on Windows that file allows the lint. Both censuses
    /// that consult this reader act on `deny`:
    /// `effects::tests::every_allow_of_a_governed_lint_is_module_level_and_in_    /// the_allowlist` admits a per-site `#[expect]` only where the lint is
    /// denied at module level, and
    /// `runner::container::tests::every_child_module_of_the_container_funnel_    /// states_its_own_lint_level` reads a denial as `PR6-LANEF-004` closed.
    ///
    /// So attributes are now **parsed**: `cfg_attr` is unwrapped to any depth,
    /// every attribute after its predicate is applied and not just the first,
    /// and each level carries the condition it was written under.
    ///
    /// # What counts, and what refuses
    ///
    /// A level counts only when it is unconditional at module top level, or
    /// when its condition is proven true on every supported target. [`Truth`]
    /// proves exactly two — `all()` is true everywhere and `any()` is false
    /// everywhere — and composes `all`/`any`/`not` over them, modelling no
    /// target list of its own.
    ///
    /// Everything else **refuses**: a condition naming a target, a feature or
    /// `test`; an attribute shape that is not understood and names the lint;
    /// brackets that do not close. A refusal sets [`Resolution::ambiguous`] and
    /// carries **no level at all**, so a census asking whether the module
    /// closed the hole is told no. Wrongly red is allowed here and wrongly
    /// green is not: the reverse hands a reviewer a guard that is not on every
    /// target, which is the one failure this reader exists to prevent.
    ///
    /// Measured, not argued —
    /// `effects::tests::the_file_level_lint_reader_refuses_a_condition_it_    /// cannot_prove` drives the provable conditions through `clippy-driver`
    /// against a body that reaches `std::fs::write`, and compiles the slip
    /// attempt itself on whichever host runs it.
    #[must_use]
    pub(crate) fn file_level_lint_resolution(source: &str, lint: &str) -> Resolution {
        let blanked = super::blank_comments_and_strings(source);
        let bytes = blanked.as_bytes();
        let mut applied: Vec<Applied> = Vec::new();
        let mut readable = true;
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at].is_ascii_whitespace() {
                at += 1;
                continue;
            }
            // The prologue ends at the first token that is not an inner attribute.
            if bytes[at] != b'#' || bytes.get(at + 1) != Some(&b'!') {
                break;
            }
            let open = at + 2;
            if bytes.get(open) != Some(&b'[') {
                break;
            }
            let Some(close) = super::matching(bytes, open, b'[', b']') else {
                // An inner attribute whose brackets do not close. Everything
                // after it is unread, so the answer is a refusal rather than
                // whatever the attributes before it happened to say.
                readable = false;
                break;
            };
            readable &=
                read_attribute(&blanked[open + 1..close], lint, Truth::Always, &mut applied);
            at = close + 1;
        }
        resolve(&applied, readable)
    }

    /// One level-setting attribute the prologue applies to the lint, with the
    /// condition the reader proved it is under.
    #[derive(Debug, Clone, Copy)]
    struct Applied {
        level: &'static str,
        truth: Truth,
    }

    /// A `cfg` condition's truth over the targets this repository supports.
    ///
    /// The reader proves exactly two things — `all()` is the empty conjunction
    /// and is true everywhere, `any()` is the empty disjunction and is false
    /// everywhere — and composes `all`/`any`/`not` over them. **It models no
    /// target list at all.** A list is a place to be wrong, and the way to be
    /// wrong here is to report a module as guarded on a target where it is not,
    /// so `windows`, `target_os = "…"`, `feature = "…"` and `test` are each
    /// [`Truth::Unknown`] and stay that way.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Truth {
        Always,
        Never,
        Unknown,
    }

    impl Truth {
        /// Both conditions, which is what one `cfg_attr` inside another means.
        fn both(self, other: Self) -> Self {
            match (self, other) {
                (Self::Never, _) | (_, Self::Never) => Self::Never,
                (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
                _ => Self::Always,
            }
        }

        /// Either condition.
        fn either(self, other: Self) -> Self {
            match (self, other) {
                (Self::Always, _) | (_, Self::Always) => Self::Always,
                (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
                _ => Self::Never,
            }
        }

        /// The complement. An unprovable condition has an unprovable one.
        fn negate(self) -> Self {
            match self {
                Self::Always => Self::Never,
                Self::Never => Self::Always,
                Self::Unknown => Self::Unknown,
            }
        }
    }

    /// [`Truth`] of one `cfg` predicate.
    fn evaluate(predicate: &str) -> Truth {
        let Some((head, body)) = split_call(predicate) else {
            // A leaf: `test`, `windows`, `feature = "strict"`. Not modelled.
            return Truth::Unknown;
        };
        match head {
            "all" => split_top_level(body)
                .into_iter()
                .fold(Truth::Always, |truth, term| truth.both(evaluate(term))),
            "any" => split_top_level(body)
                .into_iter()
                .fold(Truth::Never, |truth, term| truth.either(evaluate(term))),
            "not" => match split_top_level(body).as_slice() {
                [one] => evaluate(one).negate(),
                // `not` takes exactly one predicate. Anything else is a shape
                // this reader has not understood.
                _ => Truth::Unknown,
            },
            _ => Truth::Unknown,
        }
    }

    /// Read one attribute's contents — already blanked — and append every level
    /// it applies to `lint`, each under the condition it is written beneath.
    ///
    /// `truth` is the condition inherited from the `cfg_attr` chain above it,
    /// `Truth::Always` at the top. Returns **false** when the attribute names
    /// the lint somewhere this reader could not resolve: an absence and a
    /// refusal are not the same answer and the caller must not confuse them.
    fn read_attribute(
        attribute: &str,
        lint: &str,
        truth: Truth,
        applied: &mut Vec<Applied>,
    ) -> bool {
        const LEVELS: [&str; 5] = ["allow", "expect", "warn", "deny", "forbid"];
        let attribute = attribute.trim();
        let Some((head, body)) = split_call(attribute) else {
            // Not a call: `#![no_std]`, `#![doc = "…"]`. It sets no level, and
            // the string a `doc =` carries is already blanked.
            return !mentions(attribute, lint);
        };
        if head == "cfg_attr" {
            // `#![cfg_attr(P, a, b)]` applies EVERY attribute after the
            // predicate, not just the first.
            let mut terms = split_top_level(body);
            if terms.is_empty() {
                return !mentions(body, lint);
            }
            let condition = truth.both(evaluate(terms.remove(0)));
            let mut readable = true;
            for term in terms {
                readable &= read_attribute(term, lint, condition, applied);
            }
            return readable;
        }
        if let Some(level) = LEVELS.iter().copied().find(|level| *level == head) {
            if split_top_level(body)
                .iter()
                .any(|entry| names_lint(entry, lint))
            {
                applied.push(Applied { level, truth });
            }
            return true;
        }
        // Some other attribute. It governs no lint level, so it matters only if
        // it names this lint somewhere the reader has not understood — and then
        // it is a refusal, because the alternative is a reader guessing that
        // nothing was stated.
        !mentions(attribute, lint)
    }

    /// The level the applied attributes leave in force, in source order.
    ///
    /// Only [`Truth::Always`] attributes decide it: a [`Truth::Never`] one is
    /// not in the file on any target and a [`Truth::Unknown`] one refuses the
    /// whole answer. **A refusal carries no level**, so the two censuses that
    /// consult this reader are told the module has stated nothing — which is
    /// the direction that is loud.
    fn resolve(applied: &[Applied], readable: bool) -> Resolution {
        let mut level: Option<&'static str> = None;
        let mut refused_downgrade = false;
        let mut ambiguous = !readable;
        for entry in applied {
            match entry.truth {
                Truth::Never => {}
                Truth::Unknown => ambiguous = true,
                // Ordered, and `forbid` is sticky. A weaker level after a
                // `forbid` is `E0453`, which is the file not compiling rather
                // than a level; anything else replaces what came before it.
                Truth::Always => {
                    if level == Some("forbid") {
                        if matches!(entry.level, "allow" | "warn" | "expect") {
                            refused_downgrade = true;
                        }
                    } else {
                        level = Some(entry.level);
                    }
                }
            }
        }
        if ambiguous {
            return Resolution {
                level: None,
                refused_downgrade: false,
                ambiguous: true,
            };
        }
        Resolution {
            level,
            refused_downgrade,
            ambiguous: false,
        }
    }

    /// `head` and the contents of `head(…)`, when `text` is exactly that call.
    ///
    /// `head` must be a bare identifier and the closing parenthesis must be the
    /// last thing in `text`, so `allowance(…)` is a call named `allowance` and
    /// `deny(a) something` is not a call at all. Both refusals are deliberate:
    /// this is the structural half of the reader, and a prefix match is what it
    /// replaced.
    fn split_call(text: &str) -> Option<(&str, &str)> {
        let text = text.trim();
        let open = text.find('(')?;
        let head = text[..open].trim_end();
        if head.is_empty()
            || !head
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return None;
        }
        let close = super::matching(text.as_bytes(), open, b'(', b')')?;
        if !text[close + 1..].trim().is_empty() {
            return None;
        }
        Some((head, &text[open + 1..close]))
    }

    /// `list` split on the commas that are not inside a nested group.
    ///
    /// `all(unix, windows), deny(L)` is two terms, not three. Empty terms are
    /// dropped, so `all()` yields none — which is what makes the empty
    /// conjunction true and the empty disjunction false.
    fn split_top_level(list: &str) -> Vec<&str> {
        let mut terms = Vec::new();
        let mut depth = 0usize;
        let mut start = 0usize;
        for (index, byte) in list.bytes().enumerate() {
            match byte {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    terms.push(list[start..index].trim());
                    start = index + 1;
                }
                _ => {}
            }
        }
        terms.push(list[start..].trim());
        terms.retain(|term| !term.is_empty());
        terms
    }

    /// Whether `text` names `lint` as a word rather than as part of a longer one.
    ///
    /// The bare name, because an attribute may write either spelling, and
    /// bounded on both sides so `disallowed_methods_extra` is not this lint.
    fn mentions(text: &str, lint: &str) -> bool {
        let bare = match super::normalize_lint(lint) {
            Some(name) => name,
            None => lint.rsplit("::").next().unwrap_or(lint),
        };
        let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
        text.match_indices(bare).any(|(at, _)| {
            !text.as_bytes()[..at].last().copied().is_some_and(word)
                && !text
                    .as_bytes()
                    .get(at + bare.len())
                    .copied()
                    .is_some_and(word)
        })
    }

    /// The level in force for `lint` at `source`'s file-module scope, or none.
    ///
    /// [`file_level_lint_resolution`] without the `E0453` bit, for the censuses
    /// that ask only which level governs a module.
    #[must_use]
    pub(crate) fn file_level_lint_state(source: &str, lint: &str) -> Option<&'static str> {
        file_level_lint_resolution(source, lint).level
    }

    /// Whether an attribute entry names `lint`, qualified either way.
    fn names_lint(entry: &str, lint: &str) -> bool {
        if entry == lint {
            return true;
        }
        match (super::normalize_lint(entry), super::normalize_lint(lint)) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
