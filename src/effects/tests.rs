//! The enforcement layer's tests: the allow-placement scan, the frozen legacy
//! section, the wrapper classification, the generated inventories, and the
//! build refusals whose *reason* is pinned.
//!
//! Three rules this project pays for when it forgets them are load-bearing
//! here:
//!
//! * **A function may not be its own oracle.** The denylist is checked against
//!   [`PACKET_PRIMITIVES`], transcribed from
//!   `decisions.effect_site_inventory.mechanism`'s own sentence, never against
//!   itself. The site inventory is checked against the enums.
//! * **Enumerations come from the types and the packet.** The site grid iterates
//!   `EffectSiteId::all()`; the classification domain is derived by parsing the
//!   modules, not by listing what came to mind.
//! * **A refusal is executed, not inferred.** Every "this is refused" claim here
//!   is driven with input that *does* the forbidden thing — a legacy list that
//!   grows, an entry that names a topology module, an allow below module level —
//!   because a refusal only ever measured against compliant input is a refusal
//!   nobody has seen fire.

// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
// carries this module's review clause -- effects only inside site-taking APIs,
// no writable handle returned. `decisions.effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{
    ALLOWLIST_TOML, CLASSIFIED_MODULES, CLIPPY_TOML, DENIAL_CONTROL, DENIAL_FIXTURES,
    EFFECT_SITES_JSON, FROZEN_LEGACY_ALLOWLIST, FUNNEL_MODULES_JSON, REGENERATE,
    RESIDUE_CLASSES_JSON, TOPOLOGY_MODULES, USED_GOVERNED_LINTS, WRAPPERS_TOML, blank_comments,
    blank_comments_and_strings, externally_reachable_fns, governed_allows, legacy_growth,
    normalize_lint, production_code, production_region, topology_modules_among,
};
use crate::topology::effects::{EffectSiteId, effect_sites, effect_sites_json};

// ---------------------------------------------------------------------------
// Reading the tree and the artifacts
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `src/**/*.rs` and `examples/**/*.rs`, as `(repo-relative path, source)`.
///
/// `examples/**` is beyond the mechanism sentence's `src/**/*.rs` and is scanned
/// anyway: `cargo clippy --all-targets` compiles examples, so an ungoverned
/// example is a hole in the same wall. Scanning wider can only find more.
fn scanned_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, into: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.map(|e| e.expect("entry").path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                walk(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                into.push(path);
            }
        }
    }
    let root = repo_root();
    let mut files = Vec::new();
    walk(&root.join("src"), &mut files);
    walk(&root.join("examples"), &mut files);
    assert!(files.len() > 30, "the walk found the tree: {}", files.len());
    files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .expect("under the manifest")
                .to_string_lossy()
                .replace('\\', "/");
            (relative, fs::read_to_string(&path).expect("read source"))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    #[serde(default)]
    funnel: Vec<AllowlistEntry>,
    #[serde(default)]
    legacy: Vec<AllowlistEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowlistEntry {
    path: String,
    #[serde(default)]
    allows: Vec<String>,
    #[serde(default)]
    absent: bool,
    packet: String,
    #[serde(default)]
    review: String,
    #[serde(default)]
    legacy_effect: String,
    #[serde(default)]
    shrinks_when: String,
}

fn allowlist() -> Allowlist {
    let text =
        fs::read_to_string(repo_root().join(ALLOWLIST_TOML)).expect("effects/allowlist.toml");
    toml::from_str(&text).expect("the allowlist parses")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClippyToml {
    #[serde(default, rename = "disallowed-methods")]
    disallowed_methods: Vec<DeniedPath>,
    #[serde(default, rename = "disallowed-types")]
    disallowed_types: Vec<DeniedPath>,
    #[serde(default, rename = "disallowed-macros")]
    disallowed_macros: Vec<DeniedPath>,
    // The §7 panic-policy allowances (CODING_STANDARDS.md §7), which arrived
    // with master's lint mechanization. They configure clippy's own lints
    // rather than naming an effect primitive, so `all()` deliberately excludes
    // them. They are declared because `deny_unknown_fields` above is the
    // mechanism that turns an unclassified clippy.toml key into a failure --
    // the correct response to a new key is to classify it here, never to
    // relax the attribute -- and they are asserted by
    // `clippy_toml_turns_the_allowances_on_and_gives_unwrap_none` so a
    // field this file merely parses cannot drift unobserved.
    #[serde(default, rename = "allow-expect-in-tests")]
    allow_expect_in_tests: bool,
    #[serde(default, rename = "allow-panic-in-tests")]
    allow_panic_in_tests: bool,
    #[serde(default, rename = "allow-print-in-tests")]
    allow_print_in_tests: bool,
}

/// `clippy.toml` turns the three §7 allowances on, and gives `.unwrap()` none.
///
/// **It reads `clippy.toml`, not the standard**, and is named for that. An
/// earlier name claimed the allowances were "exactly what the standard
/// states", which this test cannot know: parsing §7's prose to compare would
/// be a text checker over an open-ended surface, and PR #25 is five review
/// rounds of evidence that those do not converge.
///
/// CODING_STANDARDS.md §7: tests fail their own setup with `.expect(` and a
/// message, use `panic!` in their own assertion helpers, and may print;
/// `.unwrap()` "is denied everywhere, tests included" because it carries no
/// diagnostic. A `false` here would silently re-deny a form 4,100 call sites
/// use, and an `unwrap` allowance appearing would silently permit one the
/// standard refuses -- so both directions are asserted rather than assumed.
#[test]
fn clippy_toml_turns_the_allowances_on_and_gives_unwrap_none() {
    let clippy = denylist();
    assert!(
        clippy.allow_expect_in_tests,
        "§7 allows .expect( with a message in tests"
    );
    assert!(
        clippy.allow_panic_in_tests,
        "§7 allows panic! in a test's own assertion helpers"
    );
    assert!(clippy.allow_print_in_tests, "§7 allows printing from tests");
    let text = fs::read_to_string(repo_root().join(CLIPPY_TOML)).expect("clippy.toml");
    assert!(
        !text.contains("allow-unwrap-in-tests"),
        "§7: .unwrap() has no allowance -- it is denied everywhere, tests included"
    );
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeniedPath {
    path: String,
    reason: String,
    #[serde(default, rename = "allow-invalid")]
    allow_invalid: bool,
}

impl ClippyToml {
    fn all(&self) -> impl Iterator<Item = &DeniedPath> {
        self.disallowed_methods
            .iter()
            .chain(&self.disallowed_types)
            .chain(&self.disallowed_macros)
    }

    fn paths(&self) -> BTreeSet<&str> {
        self.all().map(|entry| entry.path.as_str()).collect()
    }
}

fn denylist() -> ClippyToml {
    let text = fs::read_to_string(repo_root().join(CLIPPY_TOML)).expect("clippy.toml");
    toml::from_str(&text).expect("clippy.toml parses")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wrappers {
    module: Vec<ModuleClassification>,
    libc: LibcClassification,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleClassification {
    path: String,
    /// The path a denied entry would name this module by, or empty when the
    /// module is not reachable from outside its parent (a private `mod`, or the
    /// binary crate root).
    crate_path: String,
    #[serde(default)]
    funnel: Vec<String>,
    #[serde(default)]
    effectful: Vec<String>,
    #[serde(default)]
    effectful_unnameable: Vec<String>,
    #[serde(default)]
    effect_free: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LibcClassification {
    effect: Vec<String>,
    not_an_effect: Vec<String>,
}

fn wrappers() -> Wrappers {
    let text = fs::read_to_string(repo_root().join(WRAPPERS_TOML)).expect("effects/wrappers.toml");
    toml::from_str(&text).expect("the wrapper classification parses")
}

// ---------------------------------------------------------------------------
// (2) The allow-placement scan
// ---------------------------------------------------------------------------

/// `mechanism` (2), executed over the tree.
///
/// Four things, and the fourth is the one a scan usually leaves out: an
/// attribute's lint set must **equal** what the allowlist records, so a widening
/// is a failure rather than a silent extra.
#[test]
fn every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist() {
    let list = allowlist();
    let recorded: BTreeMap<&str, (&AllowlistEntry, &'static str)> = list
        .funnel
        .iter()
        .map(|entry| (entry.path.as_str(), (entry, "funnel")))
        .chain(
            list.legacy
                .iter()
                .map(|entry| (entry.path.as_str(), (entry, "legacy"))),
        )
        .collect();
    assert_eq!(
        recorded.len(),
        list.funnel.len() + list.legacy.len(),
        "a path is listed in both sections, or twice in one"
    );

    let mut carried: BTreeSet<String> = BTreeSet::new();
    let mut attributes = 0;
    for (path, source) in scanned_sources() {
        let found = governed_allows(&source);
        if found.is_empty() {
            continue;
        }
        attributes += found.len();
        let Some((entry, section)) = recorded.get(path.as_str()) else {
            panic!(
                "{path} allows a governed lint and is in no section of {ALLOWLIST_TOML}: {found:#?}"
            );
        };
        carried.insert(path.clone());
        for allow in &found {
            assert!(
                allow.module_level,
                "{path}:{} allows {:?} below module level; `mechanism` (2) permits it \
                 \"only as module-level attributes\"",
                allow.line, allow.lints
            );
            let marker = marker_before(&source, allow.line, allow.inner);
            assert!(
                marker.contains(ALLOWLIST_TOML),
                "{path}:{} carries no pointer to {ALLOWLIST_TOML} above the attribute",
                allow.line
            );
            let expected_marker = if *section == "legacy" {
                "LEGACY-EFFECT"
            } else {
                "funnel section"
            };
            assert!(
                marker.contains(expected_marker),
                "{path}:{} is in the {section} section and its prologue never says \
                 `{expected_marker}`",
                allow.line
            );
        }
        let written: BTreeSet<&str> = found
            .iter()
            .flat_map(|allow| allow.written.iter().map(String::as_str))
            .filter(|entry| normalize_lint(entry).is_some())
            .collect();
        let declared: BTreeSet<&str> = entry.allows.iter().map(String::as_str).collect();
        assert_eq!(
            written, declared,
            "{path}: the attribute allows {written:?} and {ALLOWLIST_TOML} records {declared:?}"
        );
    }

    // A file listed with a non-empty `allows` and no attribute is a stale entry;
    // a scan that found nothing is a scan that proves nothing.
    for (path, (entry, _)) in &recorded {
        if entry.allows.is_empty() || entry.absent {
            continue;
        }
        assert!(
            carried.contains(*path),
            "{path} records allows {:?} and carries no attribute",
            entry.allows
        );
    }
    assert!(
        attributes >= 25,
        "the scan found only {attributes} governed attributes; it is measuring nothing"
    );
}

/// The prologue text on the ten lines above the attribute, from the original
/// source — comments included, because the marker *is* a comment.
fn marker_before(source: &str, line: usize, inner: bool) -> String {
    let lines: Vec<&str> = source.lines().collect();
    // A file-level inner attribute is preceded by the module's whole prologue,
    // and lane A's `# LEGACY-EFFECT` sections are doc-comment headings sixteen
    // lines long. An outer attribute on an inner `mod` gets a window.
    let start = if inner { 0 } else { line.saturating_sub(13) };
    lines[start..line.min(lines.len())].join("\n")
}

/// The scan refuses what it is for — driven with input that breaks each rule.
///
/// A placement scan only ever run against a compliant tree is a scan nobody has
/// seen refuse anything. Every case here is synthetic and every one asserts a
/// *different* discriminator, so a scan that collapsed to "returns true" would
/// fail on the counts rather than pass on the cases.
#[test]
fn the_placement_scan_refuses_an_allow_that_is_not_module_level_and_sees_through_no_disguise() {
    // (1) A function-level allow is found and is not module-level.
    let on_a_function = "#[allow(clippy::disallowed_methods)]\nfn go() {}\n";
    let found = governed_allows(on_a_function);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(!found[0].module_level);

    // (2) A statement-level allow, likewise.
    let on_a_statement = "fn go() {\n    #[allow(clippy::disallowed_methods)]\n    let _ = 1;\n}\n";
    let found = governed_allows(on_a_statement);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(!found[0].module_level);

    // (3) An outer allow on an inner `mod` IS module-level — the rule permits
    //     module-level attributes, not only file-level ones.
    let on_a_module = "#[allow(clippy::disallowed_methods)]\nmod inner { }\n";
    let found = governed_allows(on_a_module);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].module_level);

    // (4) An inner attribute in the prologue is module-level.
    let inner = "//! doc\n#![allow(clippy::disallowed_types)]\nfn go() {}\n";
    let found = governed_allows(inner);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].inner && found[0].module_level);

    // (5) An inner attribute after an item is not in the prologue.
    let late = "fn go() {}\n#![allow(clippy::disallowed_types)]\n";
    let found = governed_allows(late);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(!found[0].module_level);

    // (6) `expect` counts too; the sentence says "allow/expect".
    let expected = "#![expect(clippy::disallowed_macros)]\n";
    assert_eq!(governed_allows(expected).len(), 1);

    // (7) An ungoverned lint is not reported at all.
    assert!(governed_allows("#![allow(clippy::too_many_arguments)]\n").is_empty());
    assert!(governed_allows("#![allow(unused_variables)]\n").is_empty());

    // (8) THE DISGUISES. An attribute inside a comment or a string is not an
    //     attribute. `PR4-CENSUS-COMMENT-ORACLE` is in the ledger because a
    //     census counted a doc comment, and this module's own fixtures are
    //     attributes written inside string literals.
    let disguised = concat!(
        "//! ```\n",
        "//! #![allow(clippy::disallowed_methods)]\n",
        "//! ```\n",
        "// #![allow(clippy::disallowed_types)]\n",
        "/* #![allow(clippy::disallowed_macros)] */\n",
        "const FIXTURE: &str = \"#![allow(clippy::disallowed_methods)]\";\n",
        "const RAW: &str = r#\"#![allow(clippy::disallowed_types)]\"#;\n",
    );
    assert!(
        governed_allows(disguised).is_empty(),
        "{:#?}",
        governed_allows(disguised)
    );
    // ... and the blanking that makes that true actually ran.
    let blanked = blank_comments_and_strings(disguised);
    assert_eq!(blanked.len(), disguised.len(), "offsets are preserved");
    assert_ne!(blanked, disguised, "the blanking is a no-op");
    assert!(!blanked.contains("disallowed_methods"));

    // (9) A real attribute in a file that also carries disguised ones is still
    //     found — the blanking must not be a blunt "delete everything".
    let mixed = format!("{disguised}#![allow(clippy::disallowed_macros)]\n");
    let found = governed_allows(&mixed);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].lints, vec!["disallowed_macros".to_owned()]);

    // The hostility is a count over *mechanisms*, not over strings: nine cases,
    // and the placement answers partition 4 / 3 (module-level / not) with two
    // that report nothing at all.
    let mechanisms = 9;
    assert_eq!(mechanisms, 9);
}

/// `clippy::style`, `clippy::all` and `warnings` are governed and unused.
///
/// Each would suppress far more than an effect denial — `warnings` would
/// suppress the whole gate. The count is asserted at zero rather than left to
/// habit, and the scanner is shown to *see* them so the zero is not a blind
/// spot.
#[test]
fn the_three_blunt_governed_lints_are_used_by_nobody() {
    let mut blunt = Vec::new();
    for (path, source) in scanned_sources() {
        for allow in governed_allows(&source) {
            for lint in &allow.lints {
                if matches!(lint.as_str(), "style" | "all" | "warnings") {
                    blunt.push(format!("{path}:{} {lint}", allow.line));
                }
            }
        }
    }
    assert!(blunt.is_empty(), "{blunt:#?}");

    // The scanner sees them when they are there.
    for probe in [
        "#![allow(warnings)]\n",
        "#![allow(clippy::all)]\n",
        "#![allow(clippy::style)]\n",
    ] {
        assert_eq!(governed_allows(probe).len(), 1, "{probe}");
    }

    // And the three that ARE used are exactly the three recorded.
    let list = allowlist();
    let used: BTreeSet<&str> = list
        .funnel
        .iter()
        .chain(&list.legacy)
        .flat_map(|entry| entry.allows.iter().map(String::as_str))
        .collect();
    let expected: BTreeSet<&str> = USED_GOVERNED_LINTS.iter().copied().collect();
    assert_eq!(used, expected);
}

/// `mechanism` (2) scans `Cargo.toml [lints]` too, so this is that half.
#[test]
fn cargo_toml_declares_no_lint_table_that_could_allow_a_governed_lint() {
    let text = fs::read_to_string(repo_root().join("Cargo.toml")).expect("Cargo.toml");
    let manifest: toml::Value = toml::from_str(&text).expect("Cargo.toml parses");
    let Some(lints) = manifest.get("lints") else {
        return; // No table at all is the strongest form of the answer.
    };
    let rendered = lints.to_string();
    for lint in super::GOVERNED_LINTS {
        assert!(
            !rendered.contains(lint),
            "Cargo.toml [lints] names the governed lint `{lint}`: {rendered}"
        );
    }
}

// ---------------------------------------------------------------------------
// (2) The frozen legacy section
// ---------------------------------------------------------------------------

/// The legacy section may only shrink, and the refusal is executed.
#[test]
fn the_legacy_section_is_frozen_and_may_only_shrink() {
    let list = allowlist();
    let current: Vec<&str> = list.legacy.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        legacy_growth(FROZEN_LEGACY_ALLOWLIST, &current),
        Vec::<&str>::new(),
        "the legacy section grew past the frozen list"
    );
    assert_eq!(
        current.len(),
        FROZEN_LEGACY_ALLOWLIST.len(),
        "PR5 freezes the list at exactly what it ships"
    );

    // Executed, not inferred: a list that DOES grow is refused, and shrinking is
    // allowed. Two directions, because a checker that refused everything would
    // pass the first assertion.
    let grown: Vec<&str> = current.iter().copied().chain(["src/catalog.rs"]).collect();
    assert_eq!(
        legacy_growth(FROZEN_LEGACY_ALLOWLIST, &grown),
        vec!["src/catalog.rs"]
    );
    let shrunk: Vec<&str> = current.iter().copied().skip(1).collect();
    assert!(legacy_growth(FROZEN_LEGACY_ALLOWLIST, &shrunk).is_empty());

    // And the frozen list is the tree's, not a second copy that drifted.
    let frozen: BTreeSet<&str> = FROZEN_LEGACY_ALLOWLIST.iter().copied().collect();
    let listed: BTreeSet<&str> = current.iter().copied().collect();
    assert_eq!(frozen, listed);
}

/// "never contains a topology module (src/topology/**, src/runner/**,
/// src/workspace_manager.rs, src/engine/topology.rs)".
#[test]
fn the_legacy_section_never_contains_a_topology_module() {
    let list = allowlist();
    let current: Vec<&str> = list.legacy.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        topology_modules_among(&current),
        Vec::<&str>::new(),
        "a topology module is in the frozen legacy section"
    );

    // Executed: each of the banned shapes is refused on its own, so a check
    // that only knew about `src/topology/` would fail here.
    let probes = [
        "src/topology/registry.rs",
        "src/runner/mod.rs",
        "src/workspace_manager.rs",
        "src/engine/topology.rs",
        "src/engine/topology/create.rs",
    ];
    for probe in probes {
        assert_eq!(
            topology_modules_among(&[probe]),
            vec![probe],
            "`{probe}` is a topology module and the check missed it"
        );
    }
    assert_eq!(
        probes.len(),
        TOPOLOGY_MODULES.len(),
        "one probe per banned shape"
    );

    // The gap the fifth shape closes, executed rather than described.
    //
    // `topology_modules_among` matches with `str::starts_with`, and the packet
    // sentence names `src/engine/topology.rs` — a file. PR7 makes the schema-4
    // engine a directory, and `"src/engine/topology/create.rs"` does not start
    // with `"src/engine/topology.rs"`. Run the check with only the four shapes
    // the sentence names and it returns nothing for a submodule: the ban would
    // have stopped covering every file of the module it exists to cover, and
    // nothing would have said so.
    //
    // A test that has never been seen red is not coverage. This is the red.
    let sentence_shapes = [
        "src/topology/",
        "src/runner/",
        "src/workspace_manager.rs",
        "src/engine/topology.rs",
    ];
    let submodule = "src/engine/topology/create.rs";
    assert!(
        !submodule.starts_with("src/engine/topology.rs"),
        "the prefix relation this entry exists for no longer holds"
    );
    assert!(
        !sentence_shapes
            .iter()
            .any(|banned| submodule.starts_with(banned) || *banned == submodule),
        "the four shapes the packet sentence names already cover `{submodule}`, \
         so the fifth entry is dead weight and should be removed"
    );

    // The ban is on the LEGACY section alone: the same sentence puts
    // `src/runner/{host,invocation}.rs` and `src/workspace_manager.rs` in the
    // funnel section, and they are there.
    let funnel: BTreeSet<&str> = list.funnel.iter().map(|e| e.path.as_str()).collect();
    for expected in [
        "src/workspace_manager.rs",
        "src/runner/host.rs",
        "src/runner/invocation.rs",
        "src/topology/effects.rs",
    ] {
        assert!(
            funnel.contains(expected),
            "{expected} left the funnel section"
        );
    }
}

/// Every legacy entry carries the justification the packet asks for, and every
/// funnel entry carries its review clause.
#[test]
fn every_allowlist_entry_carries_its_justification_and_names_a_real_file() {
    let list = allowlist();
    let mut absent = Vec::new();
    for entry in &list.funnel {
        assert!(
            !entry.review.trim().is_empty(),
            "{} has no funnel review clause",
            entry.path
        );
        assert!(!entry.packet.trim().is_empty(), "{}", entry.path);
    }
    for entry in &list.legacy {
        assert!(
            entry.legacy_effect.contains("LEGACY-EFFECT"),
            "{} carries no LEGACY-EFFECT justification",
            entry.path
        );
        assert!(
            !entry.shrinks_when.trim().is_empty(),
            "{} does not say when it shrinks",
            entry.path
        );
    }
    for entry in list.funnel.iter().chain(&list.legacy) {
        let exists = repo_root().join(&entry.path).is_file();
        assert_eq!(
            exists, !entry.absent,
            "{} is marked absent={} and exists={exists}",
            entry.path, entry.absent
        );
        if entry.absent {
            absent.push(entry.path.as_str());
            assert!(
                entry.allows.is_empty(),
                "{} is absent and cannot carry an attribute",
                entry.path
            );
        }
    }
    // **Empty since PR6.** It held exactly one entry — `src/runner/container.rs`,
    // the file `FunnelGroup::Container.module()` names and PR5 did not have —
    // and PR6 adds that file, so the allowlist now describes the tree it is in
    // with nothing left over. A new entry appearing here would mean the
    // allowlist had started describing a tree that does not exist.
    assert_eq!(absent, Vec::<&str>::new(), "the absent set moved");
    assert!(
        repo_root().join("src/runner/container.rs").is_file(),
        "the Container funnel is the entry that used to be absent; if it is gone \
         again, this assertion is the one that says so rather than an empty set \
         reading as agreement"
    );
}

// ---------------------------------------------------------------------------
// (1) The denylist
// ---------------------------------------------------------------------------

/// The primitives `mechanism` (1) enumerates, transcribed from the packet.
///
/// An independent table, which is the whole point: checking `clippy.toml`
/// against itself would pass however much of the sentence it had dropped. The
/// sentence, in order, is
///
/// > "std::fs write/create/remove_file/remove_dir/remove_dir_all/rename/copy/
/// > hard_link/set_permissions/create_dir/create_dir_all/DirBuilder,
/// > File::create/create_new/options/set_len/sync_data/sync_all,
/// > io::Write::write_all/flush on files, OpenOptions, symlink creation on both
/// > platforms, std::process::Command (type) and its spawn/output/status, libc
/// > fork/kill/setpgid/setsid/flock/fcntl/exec*, windows_sys process, job, and
/// > LockFileEx/UnlockFileEx functions, docker invocation helpers, and every
/// > crate-internal effectful wrapper identified by the wrapper classification
/// > (e.g., util::write_json)".
const PACKET_PRIMITIVES: &[&str] = &[
    "std::fs::write",
    "std::fs::remove_file",
    "std::fs::remove_dir",
    "std::fs::remove_dir_all",
    "std::fs::rename",
    "std::fs::copy",
    "std::fs::hard_link",
    "std::fs::set_permissions",
    "std::fs::create_dir",
    "std::fs::create_dir_all",
    "std::fs::File::create",
    "std::fs::File::create_new",
    "std::fs::File::options",
    "std::fs::File::set_len",
    "std::fs::File::sync_data",
    "std::fs::File::sync_all",
    "std::io::Write::write_all",
    "std::io::Write::flush",
    "std::os::unix::fs::symlink",
    "std::os::windows::fs::symlink_file",
    "std::os::windows::fs::symlink_dir",
    "std::process::Command::spawn",
    "std::process::Command::output",
    "std::process::Command::status",
    "libc::fork",
    "libc::kill",
    "libc::setpgid",
    "libc::setsid",
    "libc::flock",
    "libc::fcntl",
    "libc::execv",
    "libc::execve",
    "libc::execvp",
    "windows_sys::Win32::Storage::FileSystem::LockFileEx",
    "windows_sys::Win32::Storage::FileSystem::UnlockFileEx",
    "upstroke::util::write_json",
];

/// The types and the macro list the same sentence names.
const PACKET_TYPES: &[&str] = &[
    "std::fs::DirBuilder",
    "std::fs::OpenOptions",
    "std::process::Command",
];

#[test]
fn the_denylist_names_every_primitive_the_packet_enumerates() {
    let denied = denylist();
    let methods: BTreeSet<&str> = denied
        .disallowed_methods
        .iter()
        .map(|e| e.path.as_str())
        .collect();
    let types: BTreeSet<&str> = denied
        .disallowed_types
        .iter()
        .map(|e| e.path.as_str())
        .collect();

    let missing: Vec<&str> = PACKET_PRIMITIVES
        .iter()
        .copied()
        .filter(|path| !methods.contains(path))
        .collect();
    assert!(missing.is_empty(), "disallowed-methods omits {missing:?}");

    let missing: Vec<&str> = PACKET_TYPES
        .iter()
        .copied()
        .filter(|path| !types.contains(path))
        .collect();
    assert!(missing.is_empty(), "disallowed-types omits {missing:?}");

    // The three lists exist and none is vacuous. An empty `disallowed-macros`
    // would satisfy "clippy.toml has three lists" and enforce nothing.
    assert!(!denied.disallowed_methods.is_empty());
    assert!(!denied.disallowed_types.is_empty());
    assert!(
        !denied.disallowed_macros.is_empty(),
        "the macro list is the one that can be vacuous without looking it"
    );

    // Every entry says why. A denial without a reason is a denial the next
    // author deletes.
    for entry in denied.all() {
        assert!(
            entry.reason.starts_with("UPSTROKE-EFFECT")
                || entry.reason.starts_with("UPSTROKE-WRAPPER"),
            "{} has no classified reason: {}",
            entry.path,
            entry.reason
        );
    }

    // "docker invocation helpers". PR6 adds them, so this is no longer an
    // absence claim: exactly one production file may name a container runtime,
    // and it is the module `FunnelGroup::Container.module()` names.
    //
    // **The predecessor of this block could not fail.** It searched
    // `blank_comments_and_strings(...)` for `"docker` — and that function blanks
    // string literals *including their quotes*, so the needle it looked for was
    // one the haystack could never contain. Measured at PR6, when a real
    // `const DOCKER_PROGRAM: &str = "docker"` landed in production and the
    // census stayed green. The comparison is against the **unblanked**
    // production region now, and the control below proves the needle is
    // findable.
    //
    // The **set** of files is the claim, in the idiom of
    // `runner::tests::every_production_process_start_is_classified`: a new file
    // naming a container runtime is the finding, and every file in the set has
    // a reason.
    const NAMES_A_CONTAINER_RUNTIME: &[(&str, &str)] = &[
        (
            "src/effects/tests.rs",
            "this census's own needle table, which is the one place the strings \
             have to be written down",
        ),
        (
            "src/runner/container.rs",
            "the Container funnel: `FunnelGroup::Container.module()`, the one \
             production file that may reach a container runtime, and the one \
             `Command::new(` row in `every_production_process_start_is_classified`",
        ),
        (
            "src/runner/container/fake.rs",
            "the funnel's `#[cfg(test)]` substrate — the fake runtime and the \
             Docker gate. Excluded from nothing by `production_region`, because \
             the `#[cfg(test)]` marker is at the DECLARATION and not in the file",
        ),
        (
            "src/runner/container/tests.rs",
            "the funnel's `#[cfg(test)]` suite, for the same reason",
        ),
    ];
    let expected: BTreeSet<&str> = NAMES_A_CONTAINER_RUNTIME
        .iter()
        .map(|(path, _)| *path)
        .collect();
    let mut naming: BTreeSet<String> = BTreeSet::new();
    for (path, source) in scanned_sources() {
        // Comments blanked and **strings kept**: the needle lives inside a
        // string literal, so the sibling blanker would remove the very bytes
        // this looks for. Comments are blanked because a doc comment quoting
        // the packet's "docker ps" is prose, and a census that counted it would
        // be the fifth `PR4-CENSUS-COMMENT-ORACLE`.
        let production = blank_comments(&production_region(&source));
        for needle in ["\"docker", "\"podman", "docker::", "bollard", "DockerCli"] {
            if production.contains(needle) {
                naming.insert(path.clone());
            }
        }
    }
    assert_eq!(
        naming,
        expected.iter().map(|p| (*p).to_owned()).collect(),
        "the set of files naming a container runtime moved. A new one is either \
         a helper the denylist does not name, or a row this table needs"
    );

    // And the helpers themselves are denied by name, which is the packet's
    // actual requirement: the six effectful operations of the two seams the
    // Container sites are primitives of.
    for helper in [
        "upstroke::runner::container::runtime::ContainerRuntime::create",
        "upstroke::runner::container::runtime::ContainerRuntime::start",
        "upstroke::runner::container::runtime::ContainerRuntime::stop",
        "upstroke::runner::container::runtime::ContainerRuntime::remove",
        "upstroke::runner::container::GitView::materialize",
        "upstroke::runner::container::GitView::discard",
    ] {
        assert!(
            methods.contains(helper),
            "`{helper}` is a docker invocation helper and disallowed-methods does \
             not name it"
        );
    }
}

/// A denied path that does not resolve enforces nothing, and clippy says so with
/// a bare `warning:` that `-D warnings` does **not** escalate (measured on
/// clippy 0.1.97). This is the check that would otherwise not exist.
#[test]
fn every_denied_path_this_host_can_resolve_does_resolve() {
    let scratch = scratch_dir("resolve");
    // The repo's own denylist, with every `allow-invalid` stripped, so the
    // suppression cannot hide a typo from this test the way it hides the
    // platform-conditional entries from the gate.
    let denied_text = fs::read_to_string(repo_root().join(CLIPPY_TOML)).expect("clippy.toml");
    let stripped = denied_text.replace(", allow-invalid = true", "");
    assert_ne!(stripped, denied_text, "no allow-invalid entry to strip");
    fs::write(scratch.join(CLIPPY_TOML), &stripped).expect("the probe config");

    let unresolved = unresolved_paths(&scratch, "probe");
    let expected: BTreeSet<String> = host_conditional_paths()
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        unresolved, expected,
        "the set of paths this host cannot resolve moved. Anything new here is a \
         denial that enforces nothing."
    );

    // The control: a typo IS detected. Without it, a probe that silently linted
    // nothing would report an empty set and pass.
    let with_typo = format!("{stripped}\n[[extra]]\n",).replace("[[extra]]\n", "");
    let with_typo = with_typo.replace(
        "disallowed-methods = [",
        "disallowed-methods = [\n    { path = \"std::fs::wrrite\", reason = \"UPSTROKE-EFFECT: control\" },",
    );
    fs::write(scratch.join(CLIPPY_TOML), with_typo).expect("the control config");
    let control = unresolved_paths(&scratch, "control");
    assert!(
        control.contains("std::fs::wrrite"),
        "the control typo was not reported: {control:?}"
    );
}

/// The paths this host cannot resolve, and the reason each one is here.
///
/// On a Unix host `std::os::windows::fs::*` is a module that does not exist, so
/// clippy reports it. `windows_sys::*` is a crate that is not linked at all, and
/// clippy reports **nothing** for those — measured — which is why they are
/// cross-checked against the tree's own Windows source instead, by
/// [`every_platform_conditional_denial_names_something_real`].
fn host_conditional_paths() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["std::os::unix::fs::symlink"]
    } else if cfg!(target_os = "macos") {
        // `libc::pipe2` is Linux-only: the `libc` crate does not define it for
        // Darwin, so the denial resolves on Linux and does not here. That is the
        // "a denial that enforces nothing" class `clippy.toml`'s header warns
        // about -- but it is **vacuous** rather than a hole, because a path that
        // does not resolve is also a path no macOS code can call. Recorded here
        // rather than suppressed, so the set stays asserted on every host.
        //
        // Found by CI, not locally: this project has a Windows guest and no
        // macOS host, and `PR5-MACOS-CLIPPY-NEVER-RUN` predicted this exact test
        // would be the one to see it.
        vec![
            "std::os::windows::fs::symlink_dir",
            "std::os::windows::fs::symlink_file",
            "libc::pipe2",
        ]
    } else {
        vec![
            "std::os::windows::fs::symlink_dir",
            "std::os::windows::fs::symlink_file",
        ]
    }
}

/// Run clippy over an empty probe with `dir`'s `clippy.toml` and collect the
/// paths it reports as unreachable.
fn unresolved_paths(dir: &Path, tag: &str) -> BTreeSet<String> {
    let (deps, rlib) = crate_under_test();
    let source = dir.join(format!("{tag}.rs"));
    fs::write(&source, "pub fn nothing() {}\n").expect("the probe source");
    let out = dir.join(format!("{tag}-out"));
    fs::create_dir_all(&out).expect("an output directory");
    let mut command = std::process::Command::new(clippy_driver());
    command
        .env("CLIPPY_CONF_DIR", dir)
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "lib",
            "--emit=metadata",
        ])
        .arg("--out-dir")
        .arg(&out)
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("--extern")
        .arg(format!("upstroke={}", rlib.display()));
    for (name, path) in extern_dependencies(&deps) {
        command
            .arg("--extern")
            .arg(format!("{name}={}", path.display()));
    }
    let output = command
        .arg(&source)
        .output()
        .expect("clippy-driver runs; the lint gate uses the same binary");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    stderr
        .lines()
        .filter(|line| line.contains("does not refer to a reachable"))
        .filter_map(|line| {
            let start = line.find('`')? + 1;
            let end = line[start..].find('`')? + start;
            Some(line[start..end].to_owned())
        })
        .collect()
}

/// Every dependency rlib beside the test executable, so the probe links the
/// crates whose paths the denylist names — `libc` above all, whose entries would
/// otherwise be silently unchecked.
fn extern_dependencies(deps: &Path) -> Vec<(String, PathBuf)> {
    let mut best: BTreeMap<String, (std::time::SystemTime, PathBuf)> = BTreeMap::new();
    let Ok(entries) = fs::read_dir(deps) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name
            .strip_prefix("lib")
            .and_then(|n| n.strip_suffix(".rlib"))
        else {
            continue;
        };
        let Some((crate_name, _)) = stem.rsplit_once('-') else {
            continue;
        };
        if crate_name == "upstroke" {
            continue;
        }
        let stamp = path
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let slot = best
            .entry(crate_name.replace('-', "_"))
            .or_insert((stamp, path.clone()));
        if stamp >= slot.0 {
            *slot = (stamp, path);
        }
    }
    best.into_iter()
        .map(|(name, (_, path))| (name, path))
        .collect()
}

/// The platform-conditional denials name something this tree really calls.
///
/// `windows_sys::*` cannot be resolved from a Unix host at all — clippy ignores
/// a path whose crate is not linked, without even the unreachable-path notice —
/// so a typo there would be invisible on the only platform where the lint gate
/// runs. What *is* checkable from here is that every such path's item name
/// appears in this tree's own Windows source. A misspelling diverges from the
/// call site and fails.
///
/// **The residual, stated:** this proves the name is spelled the way the tree
/// spells it, not that `windows_sys` exports it at that module path. The
/// msvc-target clippy run is what proves the second half, and it is a gate
/// rather than a test.
#[test]
fn every_platform_conditional_denial_names_something_real() {
    let denied = denylist();
    let sources: String = scanned_sources()
        .into_iter()
        .map(|(_, source)| source)
        .collect();
    let mut checked = 0;
    for entry in denied.all() {
        let conditional = entry.path.starts_with("windows_sys::")
            || entry.path.starts_with("libc::")
            || entry.path.starts_with("std::os::");
        if !conditional {
            continue;
        }
        let item = entry.path.rsplit("::").next().expect("a path has an item");
        // `exec*` is the packet's own wildcard: the tree calls none of them
        // today and the sentence still requires them denied.
        const PACKET_ONLY: &[&str] = &[
            "setsid",
            "execv",
            "execve",
            "execvp",
            "execl",
            "execle",
            "execlp",
            "soft_link",
            "symlink_file",
            "symlink_dir",
            "OpenProcess",
            "TerminateProcess",
            "ResumeThread",
            "OpenJobObjectW",
            "TerminateJobObject",
            "UnlockFileEx",
        ];
        checked += 1;
        assert!(
            sources.contains(item) || PACKET_ONLY.contains(&item),
            "`{}` names `{item}`, which appears nowhere in this tree and is not one \
             of the primitives the packet's sentence requires regardless",
            entry.path
        );
    }
    assert!(
        checked >= 30,
        "only {checked} platform-conditional denials were checked"
    );

    // `allow-invalid` suppresses the unreachable-path notice, so it is also the
    // one way to hide a typo from `every_denied_path_this_host_can_resolve_does_
    // resolve`. It is therefore spent on exactly the paths that are a real
    // module on one supported platform and no module on the other, and the set
    // is written out rather than counted.
    let suppressed: BTreeSet<&str> = denied
        .all()
        .filter(|entry| entry.allow_invalid)
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(
        suppressed,
        BTreeSet::from([
            // Real on Linux, no module on Darwin: `libc` does not define `pipe2`
            // for macOS. Added after CI's macOS job found it -- this project has
            // a Windows guest and no macOS host, which is `PR5-MACOS-CLIPPY-NEVER-
            // RUN`. The suppression is what keeps the `lint (macos)` job green;
            // `host_conditional_paths` still asserts the path is unresolved there,
            // because that test strips `allow-invalid` before it probes.
            "libc::pipe2",
            "std::os::unix::fs::symlink",
            "std::os::windows::fs::symlink_dir",
            "std::os::windows::fs::symlink_file",
        ]),
        "an entry bought silence about whether it resolves"
    );
}

// ---------------------------------------------------------------------------
// `proof_tests[4]` — the fixtures whose failure reason is pinned
// ---------------------------------------------------------------------------

/// `proof_tests[4]`: "injected renamed-import / re-export / function-value /
/// legacy-wrapper call fixtures fail the build".
///
/// A fixture asserting "this does not build" is green whether it failed for the
/// intended reason or a typo. Four things are asserted that a bare refusal
/// cannot give:
///
/// * a **positive control** compiles clean first, so a mis-wired `--extern` or a
///   missing `clippy.toml` cannot make every fixture "refuse";
/// * each fixture emits **exactly** its declared lint and no other governed one;
/// * clippy's message names the **resolved** path — `std::fs::write`, not the
///   alias the fixture wrote — which is the whole of `mechanism` (1)'s claim
///   that resolution defeats renaming;
/// * the shapes are counted, so a deleted fixture is loud.
#[test]
fn every_declared_effect_denial_refuses_for_the_reason_it_declares() {
    let scratch = scratch_dir("denial");

    // The control first. If this does not compile clean, nothing below means
    // anything -- `PR5-C-DOCTEST-FIXTURES-NEVER-RAN` is the ledger entry for
    // fixtures that were green having never executed.
    let (ok, diagnostics) = lint_fixture(&scratch, "control", DENIAL_CONTROL);
    assert!(
        ok && diagnostics.is_empty(),
        "the positive control did not compile clean, so no refusal below is \
         evidence of anything:\n{diagnostics:#?}"
    );

    let mut shapes = BTreeSet::new();
    let mut lints = BTreeSet::new();
    for fixture in DENIAL_FIXTURES {
        let tag = fixture.shape.replace([' ', '-'], "_");
        let (_, diagnostics) = lint_fixture(&scratch, &tag, fixture.source);
        let emitted: BTreeSet<&str> = diagnostics.iter().map(|(lint, _)| lint.as_str()).collect();
        assert_eq!(
            emitted,
            BTreeSet::from([fixture.lint]),
            "the `{}` fixture emitted {emitted:?}, not exactly {{{}}}",
            fixture.shape,
            fixture.lint
        );
        let named = diagnostics
            .iter()
            .any(|(_, message)| message.contains(fixture.resolves_to));
        assert!(
            named,
            "the `{}` fixture was denied, but clippy's message never names `{}` -- \
             so this proves a refusal, not that the alias resolved: {diagnostics:#?}",
            fixture.shape, fixture.resolves_to
        );
        shapes.insert(fixture.shape);
        lints.insert(fixture.lint);
    }

    // `mechanism` (1) names five resolution shapes -- "aliases, re-exports,
    // function values, method calls, and macro-expanded code" -- and
    // `proof_tests[4]` names four fixtures. The grid covers the union plus the
    // type list, which is seven, and all three lints fire.
    assert_eq!(shapes.len(), 7, "{shapes:?}");
    assert_eq!(lints.len(), 3, "{lints:?}");
    for required in [
        "renamed-import",
        "re-export",
        "function-value",
        "legacy-wrapper call",
    ] {
        assert!(
            shapes.contains(required),
            "proof_tests[4] names `{required}`"
        );
    }
}

/// Compile `body` as its own crate under the repo's `clippy.toml`, and return
/// whether it compiled plus every clippy diagnostic it emitted.
fn lint_fixture(dir: &Path, tag: &str, body: &str) -> (bool, Vec<(String, String)>) {
    let (deps, rlib) = crate_under_test();
    let source = dir.join(format!("{tag}.rs"));
    fs::write(&source, body).expect("the fixture");
    let out = dir.join(format!("{tag}-out"));
    fs::create_dir_all(&out).expect("an output directory");
    let mut command = std::process::Command::new(clippy_driver());
    command
        .env("CLIPPY_CONF_DIR", repo_root())
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "lib",
            "--emit=metadata",
            "--error-format=json",
        ])
        .arg("--out-dir")
        .arg(&out)
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("--extern")
        .arg(format!("upstroke={}", rlib.display()));
    for (name, path) in extern_dependencies(&deps) {
        command
            .arg("--extern")
            .arg(format!("{name}={}", path.display()));
    }
    let output = command
        .arg(&source)
        .output()
        .expect("clippy-driver runs; the lint gate uses the same binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut diagnostics = Vec::new();
    for line in stderr.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(code) = value
            .get("code")
            .and_then(|code| code.get("code"))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if !code.starts_with("clippy::disallowed") {
            continue;
        }
        let message = value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        diagnostics.push((code.to_owned(), message));
    }
    (output.status.success(), diagnostics)
}

/// `clippy-driver`, from `PATH` or from the active toolchain's sysroot.
///
/// **Not** optional, and not skipped when missing: a build refusal whose only
/// evidence is a fixture nothing executes is `PR5-C-DOCTEST-FIXTURES-NEVER-RAN`,
/// and the rule adopted from it is to name the command that runs the fixture and
/// check that the command is one CI runs. `.github/workflows/ci.yml` installs
/// the clippy component in both the `test` and the `lint` job, and
/// [`the_workflow_that_runs_these_tests_installs_the_compiler_they_need`]
/// asserts it.
fn clippy_driver() -> PathBuf {
    let sysroot = std::process::Command::new("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()
        .expect("rustc runs; it built this test");
    let sysroot = PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim().to_owned());
    let name = if cfg!(windows) {
        "clippy-driver.exe"
    } else {
        "clippy-driver"
    };
    let in_sysroot = sysroot.join("bin").join(name);
    if in_sysroot.is_file() {
        return in_sysroot;
    }
    PathBuf::from(name)
}

/// The command that executes the fixtures is one CI runs.
///
/// **The comments are stripped first, and the strip is asserted to have done
/// something.** The first version of this test looked for the substring
/// `clippy` in the job's YAML, and the `components: clippy` line above it
/// carries a nine-line comment saying why — so deleting the line left the word
/// in place and the test green. That is `PR4-CENSUS-COMMENT-ORACLE` verbatim, in
/// the test whose whole purpose is to answer "which command runs this?".
/// Measured, mutation `ci-stops-installing-clippy`.
#[test]
fn the_workflow_that_runs_these_tests_installs_the_compiler_they_need() {
    let workflow = fs::read_to_string(repo_root().join(".github/workflows/ci.yml"))
        .expect(".github/workflows/ci.yml");
    let jobs: Vec<&str> = workflow.split("\n  test:").collect();
    assert_eq!(jobs.len(), 2, "the `test` job moved");
    let test_job = jobs[1].split("\n  msrv:").next().expect("the msrv job");

    // YAML comments run from an unquoted `#` to end of line.
    let code: String = test_job
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.len() < test_job.len(),
        "the comment strip removed nothing, so this census is reading prose"
    );
    assert!(
        !code.contains("PR5-C-DOCTEST"),
        "the control: the job's comments name that ledger entry and the strip \
         must have removed it"
    );

    let installs = code
        .lines()
        .any(|line| line.trim_start().starts_with("components:") && line.contains("clippy"));
    assert!(
        installs,
        "the `test` job does not install the clippy component, so \
         `every_declared_effect_denial_refuses_for_the_reason_it_declares` cannot run there. \
         `dtolnay/rust-toolchain` installs the minimal profile and clippy is not in it.\n{code}"
    );
    assert!(
        code.contains("cargo test --all-targets --all-features"),
        "the `test` job no longer runs the command these fixtures live in"
    );
}

/// `ci.yml` lexically names a Clippy job for each of three platforms, and
/// `merge-gate` lists each one.
///
/// **This is a substring check over one text file, not a proof of coverage.**
/// The predicates it holds are enumerated below; the parse that would close its
/// escapes is specified as `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE` in
/// `reviews/FINDINGS.md` and needs a YAML dependency this crate does not have.
///
/// It pins three separable things per platform, because a gate that can be
/// dropped from the merge aggregate is a gate that can fail without blocking
/// anything: the job exists and runs on that runner, `merge-gate` lists it in
/// `needs`, and `merge-gate`'s own loop names it.
///
/// **What this test does NOT establish, stated without hedging.** It does not
/// establish that the job runs Clippy: `- run: echo cargo clippy ...` satisfies
/// the command check while the job merely echoes. It does not establish that
/// the env entry binds *this* job's result, because a decoy elsewhere in the
/// mapping satisfies the search. It does not establish that every platform
/// whose code CI compiles has a leg, because the census collects `target_os`
/// names without evaluating `all`/`any`/`not` — so
/// `#[cfg(not(any(target_os = "linux", target_os = "macos", target_os =
/// "windows")))]` reports all three covered while no runner compiles the body.
///
/// **These are not unbounded-surface problems, and an earlier version of this
/// doc wrongly claimed they were.** It cited PR #25 to argue that a text
/// checker cannot converge. That misread the lesson: PR #25's narrowed pass
/// kept its C1–C4 contracts as *equalities and exact pins*, and its withdrawn
/// half compared prose across an open document set — not one machine-readable
/// file. The bounded repair is named in `reviews/FINDINGS.md`: parse the
/// workflow structurally and evaluate cfg predicates against the CI target
/// tuples. It needs a YAML dependency this crate does not have, which is an
/// owner decision rather than a patch.
///
/// **What it does hold, stated as the substring predicates it actually is.**
/// For each of three hard-coded runners: some job block contains both the
/// literal gate command and the literal runner string; `merge-gate`'s `needs:`
/// line contains that job's name as a substring; `merge-gate`'s `env:` mapping
/// contains the literal `<JOB>_RESULT: ${{ needs.<job>.result }}`; the
/// whitespace-normalised job block does not contain `if: false`; and the
/// `for gate in` line contains the upper-cased job name as a
/// whitespace-delimited word. Every one of those is `contains` over text.
/// A `for gate in LINT LINT_WINDOWS MSRV TEST; do : LINT_MACOS` satisfies the
/// last while omitting the gate from the loop, and that is the shape of every
/// escape still open.
///
/// The name says `lexically` for that reason: it is what this test proves, and
/// a name that promised platform coverage would be promising the parse it does
/// not perform.
///
/// **Why this is a loop and not three tests.** `PR5D-MSVC-CLIPPY-NEVER-RUN`
/// and `PR5-MACOS-CLIPPY-NEVER-RUN` are the same defect on two platforms, found
/// apart, because the Windows repair was written as
/// an instance rather than a class. A per-platform table makes the next
/// platform's omission a failure here rather than a third finding.
#[test]
fn ci_yml_lexically_names_a_clippy_job_per_platform_and_the_aggregate_lists_it() {
    const GATE: &str = "cargo clippy --all-targets --all-features -- -D warnings";
    // The runner, never the job name: what discharges the clause is the platform
    // that compiles the `#[cfg(...)]` bodies, and a name is a label.
    //
    // The DOMAIN is derived from production source, not written down here. A
    // hand-written platform list is `OFFERS_WORK` — a list nothing forces an
    // author to extend — which is exactly what the previous repair of this test
    // shipped. `platform_cfgs_in_production` reads the cfgs the crate actually
    // uses, and the assertion below refuses any it cannot map to a runner, so
    // adding `#[cfg(target_os = "freebsd")]` to `src/` fails HERE until a job
    // covers it.
    const RUNNERS: [(&str, &str); 3] = [
        ("windows-latest", "windows"),
        ("macos-latest", "macos"),
        ("ubuntu-latest", "linux"),
    ];

    let domain = platform_cfgs_in_production();
    assert!(
        domain.len() >= 3,
        "only {} platform cfg(s) found in src/; the census is reading the wrong shape: {domain:?}",
        domain.len()
    );
    let covered: BTreeSet<&str> = RUNNERS.iter().map(|(_, platform)| *platform).collect();
    let uncovered: Vec<&String> = domain
        .iter()
        .filter(|p| !covered.contains(p.as_str()))
        .collect();
    assert!(
        uncovered.is_empty(),
        "production code carries platform cfg(s) {uncovered:?} that no runner in this \
         test covers, so their bodies are outside the denylist's reach on every job CI \
         runs. Add the platform's Clippy job and its entry here."
    );

    let workflow = fs::read_to_string(repo_root().join(".github/workflows/ci.yml"))
        .expect(".github/workflows/ci.yml");

    // YAML comments run from an unquoted `#` to end of line, and the jobs this
    // test is about carry comments that name the denial command and the findings
    // in prose. A census that reads prose is `PR4-CENSUS-COMMENT-ORACLE`, so the
    // strip comes first and is itself asserted to have bitten.
    let code: String = workflow
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.len() < workflow.len(),
        "the comment strip removed nothing, so this census is reading prose"
    );
    assert!(
        !code.contains("PR5-CONF-014"),
        "the control: the workflow's comments name that finding and the strip must \
         have removed it"
    );

    // Job blocks, by their own two-space top-level keys under `jobs:`.
    let body = code.split_once("\njobs:\n").expect("a `jobs:` mapping").1;
    let mut jobs: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        let named = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' '))
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '-')
            });
        match named {
            Some(name) => jobs.push((name.to_owned(), String::new())),
            None => {
                if let Some(current) = jobs.last_mut() {
                    current.1.push_str(line);
                    current.1.push('\n');
                }
            }
        }
    }
    assert!(
        jobs.len() >= 5,
        "only {} job(s) parsed out of ci.yml; the splitter is reading the wrong shape",
        jobs.len()
    );

    let merge = jobs
        .iter()
        .find(|(name, _)| name == "merge-gate")
        .map(|(_, block)| block.as_str())
        .expect("the merge-gate job");
    let needs = merge
        .lines()
        .find(|line| line.trim_start().starts_with("needs:"))
        .expect("merge-gate declares its dependencies");
    let requires = merge
        .lines()
        .find(|line| line.contains("for gate in "))
        .expect("merge-gate's required-gate loop");

    for (runner, regions) in RUNNERS {
        let gates: Vec<&str> = jobs
            .iter()
            .filter(|(_, block)| block.contains(GATE) && block.contains(runner))
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            gates.len(),
            1,
            "expected exactly one job running `{GATE}` on `{runner}`, found {gates:?}. \
             Without it every {regions} body in the crate is outside the denylist's \
             reach on every job CI runs."
        );
        let gate_job = gates[0];

        // A step conditional turns the job green without running the gate.
        // `.github/scripts/test-docs-consistency.sh` records this escape class.
        let block = &jobs
            .iter()
            .find(|(n, _)| n == gate_job)
            .expect("the gate job")
            .1;
        // Whitespace-normalised, because `if:  false` is the same YAML. This
        // is a text check and cannot refuse every disabling form -- `if:
        // ${{ false }}` and an `env`-driven expression both evade it. Stated
        // here rather than claimed away; see the doc comment.
        let normalised = block.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            !normalised.contains("if: false"),
            "`{gate_job}` carries `if: false`, so it reports success without running \
             the denial gate on {runner}"
        );

        assert!(
            needs.contains(gate_job),
            "`merge-gate` does not depend on `{gate_job}`, so branch protection would \
             settle green while the {runner} denial gate failed: {needs}"
        );

        // `needs` alone is not enough: the aggregate's own loop decides which
        // results are *required*, and a job listed but not looped over may fail
        // freely. The loop names gates in the shape its env vars use.
        let looped = gate_job.to_uppercase().replace('-', "_");
        // The BINDING, not its existence. `LINT_MACOS_RESULT: needs.lint-windows.result`
        // is a copy-paste that satisfies an existence check, reads a passing
        // sibling, and — with `if: always()` on the aggregate — reports the
        // required check green over a red leaf. Measured: the previous version
        // of this assertion accepted exactly that.
        let binding = format!("{looped}_RESULT: ${{{{ needs.{gate_job}.result }}}}");
        // Scoped to the `env:` mapping, not the whole job: searching the block
        // let an inert `echo` step carrying the expected text satisfy this while
        // the real binding read a sibling's result.
        let env_block = merge
            .split_once("        env:\n")
            .map_or(merge, |(_, rest)| {
                rest.split_once("\n        run:")
                    .map_or(rest, |(env, _)| env)
            });
        assert!(
            env_block.contains(&binding),
            "`merge-gate` does not bind `{looped}_RESULT` to `needs.{gate_job}.result`. \
             A binding that reads another job's result passes an existence check and \
             reports green over this platform's failure."
        );
        assert!(
            requires.split_whitespace().any(|word| word == looped),
            "`merge-gate`'s required-gate loop does not name `{looped}`, so `{gate_job}` \
             can fail without failing the aggregate: {requires}"
        );
    }
}

/// The platform `cfg`s production code actually uses.
///
/// The domain for the CI-gate test above. Derived rather than listed, because a
/// listed domain is a list nothing forces an author to extend — `OFFERS_WORK`,
/// which this repository has already paid for once. `cfg(unix)` is deliberately
/// absent: it is compiled by both the macOS and Linux legs, so it adds no
/// platform requirement of its own.
fn platform_cfgs_in_production() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (_, source) in scanned_sources() {
        // RAW source, deliberately, and not `production_code`. Two reasons, and
        // the second is the load-bearing one. First, `production_code` blanks
        // string literals, so `cfg(target_os = "macos")` would arrive here as
        // `cfg(target_os = "     ")` with the platform name erased -- measured,
        // and the reason the first version of this census found only `windows`.
        // Second, the gate is `cargo clippy --all-targets`, which compiles test
        // code too: a platform cfg inside a test module needs that platform's
        // lint leg exactly as much as one in production does.
        // Raw for the NAME, blanked for the POSITION.
        //
        // `blank_comments_and_strings` erases the platform name -- a raw read is
        // the only way to see `macos` at all. But a raw read also sees this very
        // census's own explanatory comments, and did: an earlier version
        // reported `freebsd` and a blanked string, both quoted from the prose
        // beside it. That is the repository's recorded "a comment that spells
        // the token a census greps for" class, and this file is its fourth
        // occurrence.
        //
        // So the name comes from raw text and the POSITION is gated on the
        // blanked text still carrying `target_os =` at the same offset -- the
        // KEY, not `cfg(`. That gate is LEXICAL and nothing more: **any**
        // code-position `target_os =` passes it, including `let target_os =
        // "android";`, which this census would then report as a platform
        // demanding its own Clippy runner. Confirming the occurrence sits in
        // cfg syntax needs the parse that `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-
        // ORACLE` specifies. A comment blanks to spaces and fails the gate;
        // code of any kind keeps its structure and passes.
        let text = &source;
        let blanked = blank_comments_and_strings(&source);

        // Every `target_os = "..."` at a CODE position, at any nesting depth,
        // rather than parsing cfg structure. The tree really does carry
        // `#[cfg(not(any(target_os = "linux", target_os = "macos")))]`
        // (`src/agent/proc.rs`), and a parser that required the predicate to sit
        // directly inside `cfg(` contributed nothing for it. Scanning for the
        // key is both simpler and strictly wider: nested, negated and
        // `cfg_attr` forms all carry it.
        for (at, _) in text.match_indices("target_os = \"") {
            // The KEY survives blanking; the quotes do not. `blank_comments_and_strings`
            // copies code up to the opening quote and resumes after the closing
            // one, so `target_os = "macos"` blanks to `target_os = ` plus
            // spaces -- measured, and the reason an earlier gate here matched
            // nothing at all.
            if !blanked.as_bytes()[at..].starts_with(b"target_os =") {
                continue;
            }
            let rest = &text[at + "target_os = \"".len()..];
            let Some(end) = rest.find('"') else { continue };
            found.insert(rest[..end].to_owned());
        }
        // `windows` is a bare predicate with no key, so it is matched on the
        // cfg form. `unix` is deliberately absent: both the macOS and Linux legs
        // compile it, so it adds no platform requirement of its own.
        for (at, _) in text.match_indices("cfg(windows)") {
            if blanked.as_bytes()[at..].starts_with(b"cfg(windows)") {
                found.insert("windows".to_owned());
            }
        }
    }
    found
}

/// The crate's own rlib and the directory its dependencies are in.
///
/// The test binary lives beside them, so both are found from `current_exe`
/// rather than from a guessed target directory — `CARGO_TARGET_DIR` here is the
/// build wrapper's slot, not `target/`. The idiom is lane C's, from
/// `src/events/log/tests.rs`.
fn crate_under_test() -> (PathBuf, PathBuf) {
    let exe = std::env::current_exe().expect("the test executable");
    let deps = exe
        .parent()
        .expect("the test executable is in a directory")
        .to_path_buf();
    let mut rlibs: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(&deps)
        .expect("the deps directory")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            (name.starts_with("libupstroke-") && name.ends_with(".rlib")).then(|| {
                let stamp = path
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                (stamp, path)
            })
        })
        .collect();
    rlibs.sort();
    let rlib = rlibs
        .pop()
        .unwrap_or_else(|| {
            panic!(
                "no libupstroke-*.rlib beside the test executable in {}",
                deps.display()
            )
        })
        .1;
    (deps, rlib)
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("upstroke-effects-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

// ---------------------------------------------------------------------------
// (3) Wrapper classification
// ---------------------------------------------------------------------------

/// Every externally reachable `fn` of a legacy or shared module is classified.
///
/// The domain is **derived from the modules**, not listed: a `pub fn` added to
/// one of them fails this test until somebody decides what it is. That is the
/// only half of `mechanism` (3) a test can hold — the classification itself is
/// a review — and it is the half that omission attacks.
#[test]
fn every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified() {
    let record = wrappers();
    let recorded: BTreeMap<&str, &ModuleClassification> = record
        .module
        .iter()
        .map(|module| (module.path.as_str(), module))
        .collect();
    assert_eq!(
        recorded.len(),
        record.module.len(),
        "a module is recorded twice"
    );
    assert_eq!(
        recorded.keys().copied().collect::<BTreeSet<_>>(),
        CLASSIFIED_MODULES.iter().copied().collect::<BTreeSet<_>>(),
        "the record and CLASSIFIED_MODULES disagree about the domain"
    );

    let mut total = 0;
    let mut disagreements: Vec<String> = Vec::new();
    for path in CLASSIFIED_MODULES {
        let source = fs::read_to_string(repo_root().join(path))
            .unwrap_or_else(|_| panic!("{path} is in CLASSIFIED_MODULES and not in the tree"));
        let derived: BTreeSet<String> = externally_reachable_fns(&source).into_iter().collect();
        let module = recorded[path];
        // A row may carry its receiver (`Workspace::branch_exists`) so the
        // denied path can name it; the domain is over bare fn names.
        let classified: Vec<&str> = module
            .funnel
            .iter()
            .chain(&module.effectful)
            .chain(&module.effectful_unnameable)
            .chain(&module.effect_free)
            .map(|name| name.rsplit("::").next().expect("a name"))
            .collect();
        let unique: BTreeSet<&str> = classified.iter().copied().collect();
        assert_eq!(
            unique.len(),
            classified.len(),
            "{path}: a name is in two classes"
        );
        let derived_refs: BTreeSet<&str> = derived.iter().map(String::as_str).collect();
        if unique != derived_refs {
            disagreements.push(format!(
                "{path}\n    unclassified: {:?}\n    invented:     {:?}",
                derived_refs.difference(&unique).collect::<Vec<_>>(),
                unique.difference(&derived_refs).collect::<Vec<_>>()
            ));
        }
        total += derived.len();
    }
    assert!(
        disagreements.is_empty(),
        "the classification and the modules disagree:\n{}",
        disagreements.join("\n")
    );
    assert!(
        total > 300,
        "only {total} functions were classified; the derivation is finding nothing"
    );
}

/// "effectful wrappers are added to the disallowed list themselves".
#[test]
fn every_effectful_wrapper_is_on_the_disallowed_list() {
    let record = wrappers();
    let denied = denylist()
        .paths()
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<BTreeSet<String>>();
    let mut named = 0;
    for module in &record.module {
        if module.effectful.is_empty() {
            continue;
        }
        assert!(
            !module.crate_path.is_empty(),
            "{} records effectful wrappers and no crate path to name them by; an \
             unreachable module's wrappers belong in `effectful_unnameable`",
            module.path
        );
        for name in &module.effectful {
            // `Type::method` is recorded as written, so an inherent method keeps
            // its receiver in the path clippy has to resolve.
            let path = format!("{}::{name}", module.crate_path);
            assert!(
                denied.contains(&path),
                "{} classifies `{name}` effectful and `{path}` is not in {CLIPPY_TOML}",
                module.path
            );
            named += 1;
        }
    }
    assert!(named >= 10, "only {named} wrappers were checked");

    // The other direction: every crate-internal denial is a row somebody
    // classified. A `upstroke::…` entry nobody classified is a denial with no
    // review behind it.
    let classified: BTreeSet<String> = record
        .module
        .iter()
        .flat_map(|module| {
            module
                .effectful
                .iter()
                .map(move |name| format!("{}::{name}", module.crate_path))
        })
        .collect();
    for entry in denylist().all() {
        if !entry.path.starts_with("upstroke::") {
            continue;
        }
        assert!(
            classified.contains(&entry.path),
            "{CLIPPY_TOML} denies `{}` and no module classifies it effectful",
            entry.path
        );
    }
}

/// A row classified `funnel` really does name a site.
#[test]
fn every_funnel_classified_fn_names_a_site() {
    let record = wrappers();
    let mut checked = 0;
    for module in &record.module {
        if module.funnel.is_empty() {
            continue;
        }
        let source = fs::read_to_string(repo_root().join(&module.path)).expect("read module");
        let production = blank_comments_and_strings(&production_region(&source));
        assert!(
            production.contains("EffectSiteId") || production.contains("Site"),
            "{} classifies funnels and never names a site",
            module.path
        );
        for name in &module.funnel {
            let bare = name.rsplit("::").next().expect("a name");
            assert!(
                production.contains(&format!("fn {bare}")),
                "{} classifies `{name}` a funnel and declares no such fn",
                module.path
            );
            // A funnel is not a wrapper: it must not also be denied.
            let path = format!("{}::{name}", module.crate_path);
            assert!(
                !denylist().paths().contains(path.as_str()),
                "`{path}` is classified a funnel and is also denied"
            );
            checked += 1;
        }
    }
    assert!(checked >= 15, "only {checked} funnels were checked");
}

/// Every `libc::` item the tree names is classified effect or not-an-effect, and
/// every one classified an effect is denied.
///
/// `claim_scope` makes exhaustiveness "the disallowed list is complete for the
/// **primitives the crate uses**", so the list is derived from the tree rather
/// than transcribed from the sentence's `fork/kill/setpgid/setsid/flock/fcntl/
/// exec*` — which is six names out of the twenty-four this crate actually calls.
#[test]
fn every_libc_item_the_tree_names_is_classified_and_the_effects_are_denied() {
    let record = wrappers();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for (_, source) in scanned_sources() {
        let text = blank_comments_and_strings(&source);
        let mut at = 0;
        while let Some(hit) = text[at..].find("libc::") {
            let start = at + hit + "libc::".len();
            let item: String = text[start..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            at = start.max(at + 1);
            if !item.is_empty() {
                used.insert(item);
            }
        }
    }
    assert!(used.len() > 60, "only {} libc items found", used.len());

    let classified: BTreeSet<&str> = record
        .libc
        .effect
        .iter()
        .chain(&record.libc.not_an_effect)
        .map(String::as_str)
        .collect();
    let unclassified: Vec<&String> = used
        .iter()
        .filter(|item| !classified.contains(item.as_str()))
        .collect();
    assert!(
        unclassified.is_empty(),
        "these `libc::` items are used and unclassified: {unclassified:?}"
    );
    let overlap: Vec<&String> = record
        .libc
        .effect
        .iter()
        .filter(|item| record.libc.not_an_effect.contains(item))
        .collect();
    assert!(overlap.is_empty(), "classified both ways: {overlap:?}");

    let denied_toml = denylist();
    let denied = denied_toml.paths();
    for item in &record.libc.effect {
        let path = format!("libc::{item}");
        assert!(
            denied.contains(path.as_str()),
            "`{path}` is classified an effect and is not denied"
        );
    }
    // The other direction, or a reclassification would be free: moving an item
    // from `effect` to `not_an_effect` would leave its denial in place with
    // nothing behind it, and the first assertion could not tell.
    let effects: BTreeSet<&str> = record.libc.effect.iter().map(String::as_str).collect();
    for path in &denied {
        let Some(item) = path.strip_prefix("libc::") else {
            continue;
        };
        assert!(
            effects.contains(item),
            "{CLIPPY_TOML} denies `{path}` and {WRAPPERS_TOML} does not classify \
             `{item}` an effect"
        );
    }
}

// ---------------------------------------------------------------------------
// `outputs` — the generated inventories
// ---------------------------------------------------------------------------

/// A generated artifact's content, with the line discipline the checkout gave it
/// taken out of the comparison.
///
/// **Measured, not anticipated.** The first three Windows guest runs failed both
/// artifact tests and nothing else: the guest's `core.autocrlf` checks these
/// files out with `\r\n`, and `serde_json::to_string_pretty` emits `\n`, so the
/// byte comparison was asserting the checkout's line endings rather than the
/// document's content. `test (windows-latest)` in CI would have failed the same
/// way. The claim these tests make is that the *inventory* is what the enums
/// generate; the separator between its lines is the filesystem's business.
fn artifact_content(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// `outputs`: "effect_sites.json (from the enums) … generated from the enums by
/// a test and attached to gate reports".
#[test]
fn the_checked_in_effect_sites_json_is_what_the_enums_generate() {
    let generated = format!(
        "{}\n",
        effect_sites_json().expect("the inventory serializes")
    );
    let path = repo_root().join(EFFECT_SITES_JSON);
    if std::env::var_os(REGENERATE).is_some() {
        fs::write(&path, &generated).expect("write the inventory");
    }
    let on_disk = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{EFFECT_SITES_JSON} is missing; run with {REGENERATE}=1"));
    let on_disk = artifact_content(&on_disk);
    assert_eq!(
        on_disk, generated,
        "{EFFECT_SITES_JSON} is stale; regenerate with {REGENERATE}=1"
    );
    // It really is the whole inventory, not a corner of it.
    assert_eq!(effect_sites().len(), EffectSiteId::all().len());
    assert!(on_disk.contains("\"site\": \"Event.OpenLog\""));
    assert!(on_disk.contains("\"site\": \"Object.CandidateCommitTree\""));
}

/// The companion artifact states where the funnel bodies actually are
/// (`PR5-CONF-018`).
///
/// `effect_sites.json` ships `"module": "src/interaction.rs"` for
/// `Answer.Ingest`, `Answer.PublishRename` and `Answer.StageWrite`, and the
/// `AnswerSite::` literals are at `src/rundir.rs:899`, `:912` and `:934` and
/// nowhere else. Until this round the only thing reconciling the artifact with
/// the tree was a **test-side override** — [`funnel_module`] — so the artifact a
/// gate report carries said something false about this tree and nothing checked
/// in said otherwise. Measured: deleting that override makes the three Answer
/// sites join the "no funnel names them" set, which is the finding.
///
/// The two axes are the *inventory's claim* and the *tree's answer*. Every
/// existing test holds one constant and reads the other — the census searches
/// the file the override names, the artifact test compares the file the enums
/// name — so the pair was never written down together. Here they are written
/// down together, for every site rather than for the three that disagree, so a
/// fourth disagreement appearing later is a change to this file rather than a
/// silence.
#[test]
fn the_checked_in_funnel_module_record_states_where_the_bodies_are() {
    let generated = funnel_module_record();
    let path = repo_root().join(FUNNEL_MODULES_JSON);
    if std::env::var_os(REGENERATE).is_some() {
        fs::write(&path, &generated).expect("write the funnel-module record");
    }
    let on_disk =
        artifact_content(&fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!("{FUNNEL_MODULES_JSON} is missing; run with {REGENERATE}=1")
        }));
    assert_eq!(
        on_disk, generated,
        "{FUNNEL_MODULES_JSON} is stale; regenerate with {REGENERATE}=1"
    );

    let parsed: serde_json::Value = serde_json::from_str(&on_disk).expect("the record parses");
    assert_eq!(
        parsed["sites_checked"].as_u64().expect("a count"),
        EffectSiteId::all().len() as u64,
        "the record must cover the whole inventory; a record over a corner of it          would report agreement it never looked for"
    );
    let disagreements: Vec<&str> = parsed["disagreements"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|entry| entry["site"].as_str().expect("a site name"))
        .collect();
    assert_eq!(
        disagreements,
        // In `EffectSiteId::all()` order, which is the frozen enum's, so a site
        // moving within the inventory is a change here too.
        ["Answer.StageWrite", "Answer.PublishRename", "Answer.Ingest"],
        "the set of sites whose funnel bodies are not where the inventory says          moved. Each one is a claim a gate report carries about this tree."
    );
    for entry in parsed["disagreements"].as_array().expect("an array") {
        assert_eq!(entry["inventory_module"], "src/interaction.rs");
        assert_eq!(entry["funnel_module"], "src/rundir.rs");
    }
}

/// The companion record, from the enums and from [`funnel_module`].
fn funnel_module_record() -> String {
    let mut disagreements = Vec::new();
    for site in EffectSiteId::all() {
        let inventory = site.module();
        let actual = funnel_module(site);
        if inventory != actual {
            disagreements.push(serde_json::json!({
                "site": site.name(),
                "group": site.group().name(),
                "inventory_module": inventory,
                "funnel_module": actual,
            }));
        }
    }
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "note": "PR5-CONF-018. effect_sites.json's `module` column is \
                     EffectSiteId::module(), which is PR3's frozen answer and the \
                     packet's -- mechanism (2) places the answer funnels in \
                     src/interaction.rs. PR5 lane B put those three funnel BODIES in \
                     src/rundir.rs and left interaction::{write_question, write_answer, \
                     read_answer} as delegations. Both files are allowlisted funnel \
                     modules and enforcement is unchanged either way, so Fable ruled \
                     this a preference and Sol a low defect; what is not a matter of \
                     taste is that a gate-attached artifact stated something untrue of \
                     this tree with nothing checked in saying otherwise. The generator \
                     is src/topology/effects.rs, frozen, so the column is corrected \
                     here rather than in place, and the funnel bodies are NOT moved.",
            "sites_checked": EffectSiteId::all().len(),
            "disagreements": disagreements,
        }))
        .expect("the funnel-module record serializes")
    )
}

/// Every module the inventory names is in the funnel section, and every site has
/// a funnel that names it — or is recorded absent with the reason.
///
/// This is where an omission would live. `effect_sites.json` is generated from
/// the enums so it cannot omit a *site*; what it can do is name a module that
/// implements none of them, which reads identically to a module that implements
/// all of them.
#[test]
fn every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent() {
    let list = allowlist();
    let funnel: BTreeMap<&str, &AllowlistEntry> = list
        .funnel
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();

    let mut modules: BTreeSet<String> = BTreeSet::new();
    for site in EffectSiteId::all() {
        modules.insert(site.module().to_owned());
    }
    assert_eq!(modules.len(), 7, "{modules:?}");
    for module in &modules {
        assert!(
            funnel.contains_key(module.as_str()),
            "`{module}` is a funnel module the inventory names and the allowlist's \
             funnel section does not list it"
        );
    }

    // Per site: does a funnel name it?
    //
    // Two mechanisms, because the three lanes built two and a grid that knew
    // one would report the other's whole group as unimplemented:
    //
    //   * the variant literal — `RunDirSite::PublishMarker` inside the funnel
    //     body, which is lane B's shape (one `pub fn` per site, site fixed);
    //   * the site as a **parameter** — `fn create_ref_zero_old(site: RefSite,
    //     …)`, which is lane A's and lane C's, and is the shape `identity`
    //     literally describes ("every effectful funnel API takes its group's
    //     site by value").
    //
    // Recorded per group by `funnel_mechanism` so a group that stopped doing
    // either is loud rather than silently "still covered by the other".
    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    let mut unimplemented = Vec::new();
    let mut mechanisms: BTreeMap<&str, &str> = BTreeMap::new();
    for site in EffectSiteId::all() {
        let group = site.group().name();
        let module = funnel_module(site);
        let entry = funnel[module];
        if entry.absent {
            unimplemented.push(site.name());
            continue;
        }
        let source = sources.entry(module.to_owned()).or_insert_with(|| {
            let text = fs::read_to_string(repo_root().join(module)).expect("read funnel module");
            blank_comments_and_strings(&production_region(&text))
        });
        let variant = format!("{group}Site::{}", site.variant());
        let parameter = format!(": {group}Site");
        if source.contains(&variant) {
            mechanisms.insert(group, "variant");
        } else if source.contains(&parameter) {
            mechanisms.insert(group, "parameter");
        } else {
            unimplemented.push(site.name());
        }
    }
    // Both mechanisms are in use. If one disappeared, every group would have to
    // be re-measured against the other rather than inheriting a pass.
    let distinct: BTreeSet<&str> = mechanisms.values().copied().collect();
    assert_eq!(distinct.len(), 2, "{mechanisms:?}");

    // The expected set, written out rather than counted, because *which* sites
    // have no funnel is the finding and a count would hide a swap.
    let expected: BTreeSet<String> = SITES_WITHOUT_A_FUNNEL
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let actual: BTreeSet<String> = unimplemented.into_iter().collect();
    assert_eq!(
        actual, expected,
        "the set of sites no funnel names moved. Each one is a row of the site \
         inventory in reconciliation-D.md and needs a reason."
    );
}

/// The module a group's funnel bodies are actually in.
///
/// `FunnelGroup::module()` is PR3's answer and is frozen. For one group it is
/// not where PR5 put the code: `mechanism` (2) places "the answer funnels in
/// src/interaction.rs", and lane B put the bodies in `src/rundir.rs`, leaving
/// `interaction::{write_question, write_answer, read_answer}` as thin
/// delegations. Both files are in the allowlist's funnel section and the
/// disagreement is section J of `reconciliation-D.md`; it is recorded here
/// rather than resolved by silence, because silently searching the right file
/// would make the inventory's `module` column read as correct.
fn funnel_module(site: EffectSiteId) -> &'static str {
    match site.group().name() {
        "Answer" => "src/rundir.rs",
        _ => site.module(),
    }
}

/// The sites the frozen inventory declares that no funnel in this tree names.
///
/// Every one is a row in `reconciliation-D.md`'s site inventory with the packet
/// key that defers it. They are written out rather than counted because *which*
/// site is missing is the finding: a count would survive a swap.
const SITES_WITHOUT_A_FUNNEL: &[&str] = &[
    // The **Container group is no longer here.** PR5 recorded all eight as
    // unimplemented because `FunnelGroup::Container.module()` names
    // `src/runner/container.rs` and that file was not in the tree; PR6 adds it,
    // and every one of the eight is taken by value by an API in it. The group
    // leaving this list is the finding that PR6 landed, and a variant coming
    // back would mean a funnel stopped naming its site.
    //
    // `ReportSite::Write` maps to `src/util.rs`, and the report write this slice
    // ships is `RunDir.WriteReport` in `src/rundir.rs` (`rundir::write_report`,
    // which calls `util::write_json`). `PR3-REPORT-DOUBLE-NAME` in
    // `reviews/FINDINGS.md` is the standing entry for the two names on one file
    // and is the owner's, not this slice's.
    "Report.Write",
    // The Process group. `identity` says "every effectful funnel API takes its
    // group's site by value", and PR4's process funnel does not: `HostRunner`
    // threads a `SpawnHooks` observer and consults the containment sub-effect
    // points by name, while `ProcessSite` is named in production nowhere. The
    // hooks fire and PR4's grids drive them, so this is a *shape* gap and not a
    // coverage one — filed as `PR5D-PROCESS-FUNNEL-TAKES-NO-SITE` in
    // `reviews/FINDINGS.md` with `src/runner/**` frozen under the owner ruling.
    "Process.Spawn",
    "Process.Terminate",
];

/// `outputs`: "the residue-class evidence record (per element: constructed,
/// classified, recovered; per site: sampling N and observed-class histogram)".
#[test]
fn the_checked_in_residue_class_record_is_what_the_enums_generate() {
    let generated = residue_record();
    let path = repo_root().join(RESIDUE_CLASSES_JSON);
    if std::env::var_os(REGENERATE).is_some() {
        fs::write(&path, &generated).expect("write the residue record");
    }
    let on_disk = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{RESIDUE_CLASSES_JSON} is missing; run with {REGENERATE}=1"));
    let on_disk = artifact_content(&on_disk);
    assert_eq!(
        on_disk, generated,
        "{RESIDUE_CLASSES_JSON} is stale; regenerate with {REGENERATE}=1"
    );

    // The sampling N the record freezes is the N the harness runs.
    // `command_internal_sub_effects` says "N frozen per site in the registry";
    // `src/topology/registry.rs` is PR3's and frozen, and carries no N, so the
    // record carries it and this is the cross-check that keeps the two equal.
    let harness = fs::read_to_string(repo_root().join("src/workspace_manager.rs"))
        .expect("src/workspace_manager.rs");
    assert!(
        harness.contains(&format!("const SAMPLING_N: u32 = {SAMPLING_N};")),
        "the sampling harness no longer runs N = {SAMPLING_N}"
    );
    assert!(on_disk.contains(&format!("\"sampling_n\": {SAMPLING_N}")));
}

/// The durability barrier is reached through **one** call each, and the syscall
/// is inside it (`PR5-CONF-012`).
///
/// `proof_tests[9]` makes the durability ledger a *named proof*: "the sync
/// ledger shows the synced length equal to the file length after open". The
/// ledger entry is written beside the syscall by the same function, so it
/// certifies itself: `let outcome = file.sync_all();` → `let outcome:
/// io::Result<()> = Ok(());` survived the whole suite, with the fsync gone and
/// every trace assertion still green. `sync_file_recorded`'s own doc conceded
/// the residual in as many words, and the same shape held in
/// `src/workspace_manager.rs` and for the Event sync records.
///
/// Nothing on a machine that does not lose power can see *inside* `fsync`. What
/// can be seen is two things either side of it, and the repair is to make both
/// checkable rather than one:
///
/// * **the syscall is there** — this census, which reads the source and fails if
///   the call leaves the one function that is allowed to make it;
/// * **the seam was reached as often as the ledger claims** —
///   `rundir::tests::the_durability_ledger_counts_barriers_that_were_actually_
///   performed`, which crosses the ledger's entries against
///   `util::barriers_performed()`.
///
/// Neither alone is enough, and that is the point: a census cannot tell whether
/// the line ran, and a counter cannot tell whether the line still contains the
/// syscall.
///
/// `src/events/log/premove.rs` is excluded by name. It is `git show
/// ff0490a:src/events.rs` kept verbatim as the independent oracle for
/// byte-identical legacy behaviour, and its whole value is that it is unchanged.
#[test]
fn every_file_durability_barrier_in_a_funnel_module_goes_through_one_call() {
    // The two functions that may name the primitive, and how many times each.
    const BARRIERS: &[(&str, &str, usize)] = &[
        ("src/util.rs", "fsync_file", 1),
        ("src/util.rs", "fsync_dir", 1),
    ];
    // Line endings normalized before any structural search: the guest checks this
    // tree out with CRLF, and `find("\n}\n")` does not match `\r\n}\r\n`. Measured —
    // this census passed on Linux and panicked "the function ends" on Windows
    // Server 2025, which is the platform half of the same lesson the rest of this
    // round is about. `artifact_content` exists for exactly this reason.
    let util = artifact_content(
        &fs::read_to_string(repo_root().join("src/util.rs")).expect("src/util.rs"),
    );
    for (file, function, expected) in BARRIERS {
        let body = util
            .split_once(&format!("fn {function}("))
            .unwrap_or_else(|| panic!("{file} no longer defines `{function}`"))
            .1;
        let body = &body[..body.find("\n}\n").expect("the function ends")];
        let calls = body.matches(".sync_all()").count();
        assert_eq!(
            calls, *expected,
            "`{function}` makes {calls} durability syscall(s), not {expected}; deleting \
             the barrier from inside it is exactly PR5-CONF-012's surviving mutation"
        );
    }

    // And nowhere else in the funnel modules, so a caller cannot quietly grow a
    // second barrier the counter and this census both miss.
    const FUNNELS: &[&str] = &[
        "src/rundir.rs",
        "src/workspace_manager.rs",
        "src/events/log.rs",
        // PR6's Container funnel writes the intent record durably and reaches
        // the barrier through `util::fsync_file`/`util::fsync_dir` like every
        // other funnel, so it belongs in the "and nowhere else" half.
        "src/runner/container.rs",
    ];
    for path in FUNNELS {
        let source =
            fs::read_to_string(repo_root().join(path)).unwrap_or_else(|_| panic!("{path}"));
        let production = blank_comments_and_strings(&production_region(&source));
        assert_eq!(
            production.matches(".sync_all()").count(),
            0,
            "{path} calls `sync_all` directly; the file barrier is `util::fsync_file` \
             and the directory barrier is `util::fsync_dir`"
        );
    }

    // The Event funnel's own primitive is `sync_data`, a different call with its
    // own census next door, and it is named here so this test's silence about it
    // is a decision rather than an oversight.
    let log = fs::read_to_string(repo_root().join("src/events/log.rs")).expect("src/events/log.rs");
    assert_eq!(
        blank_comments_and_strings(&production_region(&log))
            .matches(".sync_data()")
            .count(),
        1,
        "the log's own barrier is one `sync_data`; \
         `events::log::tests::the_event_log_is_written_in_exactly_one_module` \
         is the census that owns it"
    );
}

/// The sampling N `effects/residue-classes.json` freezes.
const SAMPLING_N: u32 = 8;

fn residue_record() -> String {
    let mut sites = Vec::new();
    for site in EffectSiteId::all() {
        if site.residue_classes().is_empty() {
            continue;
        }
        sites.push(serde_json::json!({
            "site": site.name(),
            "group": site.group().name(),
            "row": site.row(),
            "module": site.module(),
            "sampling_n": SAMPLING_N,
            "classes": site
                .residue_classes()
                .iter()
                .map(|class| serde_json::json!({
                    "class": class,
                    "label": class.label(),
                    "classified_as": class.classified_as(),
                }))
                .collect::<Vec<_>>(),
            "elements": site.residue_elements(),
        }));
    }
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "note": "decisions.effect_site_inventory.outputs: the residue-class \
                     evidence record, DECLARATIONS half. Per element: constructed, \
                     classified, recovered -- proven by workspace_manager::tests. Per \
                     site: the frozen sampling N. The observed-class histogram is \
                     machine-varying and cannot be pinned in a file compared \
                     byte-for-byte, so it is emitted to effects/residue-histogram.json \
                     on every run by \
                     sampled_git_child_kills_every_residue_classified_and_recovered, \
                     which reads it back and checks it accounts for every sample \
                     (PR5-CONF-004).",
            "sites": sites,
        }))
        .expect("the residue record serializes")
    )
}

// ---------------------------------------------------------------------------
// "no topology production callers"
// ---------------------------------------------------------------------------

/// Every site enum's `row()` is **exhaustive by construction**: no wildcard
/// arm (`PR5-EVENTS-063`, and the other half of `PR5-WORKSPACE-049`).
///
/// `expected_failures_refusals[7]` is "a site without a row mapping fails to
/// compile", and today that holds only as a *side effect* of `row()` happening
/// to be written out arm by arm. Nothing asserted the absence of a wildcard,
/// and the control measured what one costs: with `EventSite::row`'s single
/// explicit arm replaced by `_ => ResourceRow::R21`, adding an unmapped variant
/// produced ten `E0004` non-exhaustive errors and `row()` was **not** among
/// them — the wildcard had silenced exactly the diagnostic the sentence refers
/// to, and the whole suite stayed green.
///
/// A source census rather than a compile fixture because it is the *absence* of
/// a construct that has to be checked, and a fixture can only demonstrate that
/// something fails to compile today. `src/topology/effects.rs` is frozen, so
/// this scan is a guard on a file this slice does not edit rather than a
/// requirement on one it does.
#[test]
fn no_site_enums_row_mapping_has_a_wildcard_arm() {
    let source = std::fs::read_to_string("src/topology/effects.rs").expect("the frozen inventory");
    let production = blank_comments_and_strings(&production_region(&source));
    let mut scanned = 0_usize;
    let mut offenders = Vec::new();
    let mut rest = production.as_str();
    while let Some(at) = rest.find("fn row(") {
        rest = &rest[at + "fn row(".len()..];
        // The body runs to the closing brace of the `match`, which is the first
        // line at the function's own indentation that is exactly `    }`.
        let body_end = rest.find("\n    }").unwrap_or(rest.len());
        let body = &rest[..body_end];
        scanned += 1;
        for wildcard in ["_ =>", "_=>"] {
            if body.contains(wildcard) {
                offenders.push(format!(
                    "a `row()` mapping falls back through `{wildcard}`, so a site added later \
                     compiles with no declared row: …{}",
                    &body[..body.len().min(160)]
                ));
            }
        }
    }
    assert!(
        scanned >= 8,
        "only {scanned} `row()` mappings scanned, so this census is looking at the wrong file"
    );
    assert!(offenders.is_empty(), "{offenders:#?}");
}

/// `decisions.pr_sequence[6].scope` ends "no topology production callers", and
/// `non_goals[0]` is "production topology callers".
///
/// The census is over the **production region** of every topology module, and it
/// carries its own control: the test region of `src/topology/registry.rs` DOES
/// name a funnel, so a census whose region split had collapsed to the empty
/// string would fail here rather than report "nobody calls anything".
#[test]
fn no_topology_module_calls_a_funnel_in_production() {
    const FUNNELS: &[&str] = &[
        "workspace_manager::",
        "rundir::",
        "EventLog::",
        "establish_stable_prefix",
        "util::write_json",
        "util::write_text",
    ];
    let mut topology = 0;
    let mut callers = Vec::new();
    for (path, source) in scanned_sources() {
        let is_topology = TOPOLOGY_MODULES
            .iter()
            .any(|banned| path.starts_with(banned) || path == *banned);
        // `src/workspace_manager.rs` and `src/runner/**` are in
        // `TOPOLOGY_MODULES` because the legacy section may not contain them;
        // they are the funnels themselves and naturally name funnels.
        if !is_topology || !path.starts_with("src/topology/") {
            continue;
        }
        topology += 1;
        let production = blank_comments_and_strings(&production_region(&source));
        for funnel in FUNNELS {
            if production.contains(funnel) {
                callers.push(format!("{path} names `{funnel}` in production"));
            }
        }
    }
    assert!(topology >= 8, "only {topology} topology modules scanned");
    assert!(callers.is_empty(), "{callers:#?}");

    // The control.
    let registry = fs::read_to_string(repo_root().join("src/topology/registry.rs"))
        .expect("src/topology/registry.rs");
    let production = production_region(&registry);
    assert!(
        !production.contains("rundir::"),
        "the production region names a funnel"
    );
    assert!(
        registry.contains("rundir::create_public_dir"),
        "the control: the registry's TEST region builds its fixture through the \
         run-directory funnel, so a production/test split that had collapsed \
         would fail here instead of reporting silence"
    );
    assert!(
        production.len() < registry.len(),
        "the production region is the whole file, so the split did nothing"
    );
}

/// The scan's own parser, on this tree's real shapes.
///
/// `externally_reachable_fns` decides the classification domain, so a parser
/// that quietly saw half the tree would make [`every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified`]
/// pass against a domain nobody drew — the omission failure this project's
/// reconciliation table exists for, one level down.
#[test]
fn the_reachable_fn_parser_finds_each_shape_this_tree_uses() {
    let source = concat!(
        "pub fn free() {}\n",
        "pub(crate) fn crate_visible() {}\n",
        "pub(super) fn super_visible() {}\n",
        "fn private() {}\n",
        "pub const fn constant() -> u8 { 1 }\n",
        "pub unsafe fn unsafely() {}\n",
        "impl Thing { pub fn inherent(&self) {} fn hidden(&self) {} }\n",
        "impl Trait for Thing { fn through_the_trait(&self) {} }\n",
        "pub trait Public { fn declared(&self) -> u8; fn defaulted(&self) -> u8 { 1 } }\n",
        "trait Private { fn private_default(&self) -> u8 { 1 } }\n",
        "#[cfg(test)]\nmod tests { pub fn in_the_test_region() {} }\n",
    );
    let found = externally_reachable_fns(source);
    assert_eq!(
        found,
        vec![
            "constant".to_owned(),
            "crate_visible".to_owned(),
            "defaulted".to_owned(),
            "free".to_owned(),
            "inherent".to_owned(),
            "super_visible".to_owned(),
            "through_the_trait".to_owned(),
            "unsafely".to_owned(),
        ],
        "the parser's answer moved"
    );
    // Eight shapes accepted, five refused, and the five are refused for five
    // different reasons: private, private-in-an-inherent-impl, test region, a
    // trait method DECLARATION (no body to classify — its implementations are
    // reached by the `impl … for …` shape), and a default body in a trait that
    // is not itself visible.
    assert!(!found.contains(&"private".to_owned()));
    assert!(!found.contains(&"hidden".to_owned()));
    assert!(!found.contains(&"in_the_test_region".to_owned()));
    assert!(!found.contains(&"declared".to_owned()));
    assert!(!found.contains(&"private_default".to_owned()));

    // `PR6-LANEF-007`, stated as the reviewer's own exploit: a default body on a
    // public trait that reaches an effect. The parser used to answer
    // `visible || in_trait_impl`, and a default body is neither — so the body
    // below was outside the classification domain of a CLASSIFIED module, and
    // clippy, all 79 effects tests and all 38 container tests passed with it in
    // the tree. It is in the domain now, which means somebody has to classify it.
    let exploit = concat!(
        "pub trait ContainerHooks {\n",
        "    fn phase(&mut self) -> u8;\n",
        "    fn remove_without_a_site(&self, path: &Path) { let _ = fs::remove_file(path); }\n",
        "}\n",
    );
    assert!(
        externally_reachable_fns(exploit).contains(&"remove_without_a_site".to_owned()),
        "the effect a default trait body performs is invisible to the domain again"
    );
}

/// The comment blanker models raw strings, so an unparsed literal cannot erase
/// a later one.
///
/// `PR6-LANEF-005`. [`blank_comments`] used to track only `"`, and documented
/// the omission as safe because "the failure mode is a needle this function does
/// not find … loud rather than accept something extra". **For a census over an
/// expected set that is backwards**: a missed needle is a false negative, the
/// computed set stays equal to the expected one, and the census is green with a
/// file it should have caught. `every_declared_effect_denial_names_a_real_path`'s
/// "docker invocation helpers" block is exactly such a census, and the reviewer
/// measured it staying green with an extra Docker-naming file present.
///
/// The two axes: {construct} × {is a later literal on the same line still
/// visible}. Every row keeps a real comment invisible, so this cannot pass by
/// the blanker having stopped blanking.
#[test]
fn the_comment_blanker_models_raw_strings_and_still_blanks_comments() {
    // The reviewer's shape: a raw string whose body contains a quote and a `//`,
    // with a real literal after it on the same line.
    let exploit = r####"const A: &str = r#"x" //"#; const B: &str = "docker";"####;
    let blanked = blank_comments(exploit);
    assert!(
        blanked.contains("\"docker\""),
        "a raw string erased the literal after it: {blanked}"
    );

    // Every other literal shape, each with a live needle after it.
    for (label, source) in [
        ("raw, no hashes", r###"let a = r"//"; let b = "docker";"###),
        ("byte raw", r###"let a = br#""//"#; let b = "docker";"###),
        ("byte string", r#"let a = b"\"//"; let b = "docker";"#),
        ("char literal", "let a = '\"'; let b = \"docker\";"),
        ("escaped quote", "let a = \"\\\" //\"; let b = \"docker\";"),
        ("block comment", "/* // */ let b = \"docker\";"),
        ("nested block", "/* /* // */ */ let b = \"docker\";"),
    ] {
        assert!(
            blank_comments(source).contains("\"docker\""),
            "{label}: the needle after it was erased: {}",
            blank_comments(source)
        );
    }

    // And a real comment is still removed — in both flavours, and a doc comment
    // quoting a needle is still invisible, which is `PR4-CENSUS-COMMENT-ORACLE`.
    for source in [
        "// let b = \"docker\";\nlet c = 1;",
        "/* let b = \"docker\"; */ let c = 1;",
        "//! names \"docker\" in prose\nlet c = 1;",
        "/// names \"docker\" in prose\nlet c = 1;",
    ] {
        assert!(
            !blank_comments(source).contains("\"docker\""),
            "a comment naming the needle survived: {}",
            blank_comments(source)
        );
    }

    // Line breaks survive, because callers report line numbers.
    let counted = "// one\n/* two\nthree */\nlet b = 1;\n";
    assert_eq!(
        blank_comments(counted).lines().count(),
        counted.lines().count(),
        "the blanker lost a line"
    );
}

/// A char literal whose scalar is more than one byte does not desync the
/// tokeniser.
///
/// `PR7-R2C-CHAR-LITERAL-DESYNC`. Both blankers decided "is this a char
/// literal?" with a fixed two-byte lookahead, so `'é'` — whose closing quote is
/// at `+3` — was classified as **not** one, scanning resumed *on that closing
/// quote*, and the quote was read as an opening one. From there the pairing is
/// shifted by one and a `{` that is inside a char literal survives into the
/// blanked text as visible code.
///
/// One unbalanced brace was enough to take a whole file out of every census:
/// `matching` counts it, `configured_item_end`'s brace arm walks past the item's
/// real `}`, and giving up used to mean "blank to end of file". The last block
/// below is that attack, in miniature. Full size, on `src/agent/claude.rs`, the
/// region measured **8525** non-whitespace bytes with the attack and 8525
/// without it — a zero-byte delta, invisible to every byte floor in this crate,
/// which is why the repair is in the tokeniser and in the give-up direction
/// rather than in a floor. Gate-clean, with the probe written as
/// `stringify! { ('é','{') }` (rustfmt leaves brace-delimited macro bodies
/// alone; it rewrites the bare tuple to `('é', '{')`, and the space defuses it)
/// inside `src/runner/container/view.rs`'s `#[cfg(test)] pub(crate) mod
/// fixtures`, a forged `RunnerRequest {` builder above that file's real test
/// module passed `every_production_runner_request_is_built_by_its_roles_builder`
/// with `cargo fmt --check` and `clippy -D warnings` both at exit 0 — and failed
/// it by name with the probe removed.
///
/// The preconditions are already here: `src/status.rs`, `src/util.rs` (twice on
/// one line) and `src/engine/tests.rs` hold non-ASCII char literals today. Only
/// the adjacency was missing.
///
/// Two axes: {scalar width} × {what follows the literal}. The controls are the
/// lifetime rows — a blanker that treated every `'` as a literal would pass the
/// leak rows and fail those.
#[test]
fn a_multi_byte_char_literal_does_not_desync_the_blanker() {
    // 1. The tokeniser. Nothing inside a char literal reaches the blanked text.
    for (label, source, leaked) in [
        (
            "the reviewer's pair",
            "const P: (char, char) = ('é','{');\n",
            "{",
        ),
        (
            "a closing brace",
            "const P: (char, char) = ('é','}');\n",
            "}",
        ),
        ("a cascade", "const P: [char; 3] = ['é','{','{'];\n", "{"),
        (
            "four-byte scalar",
            "const P: (char, char) = ('😀','{');\n",
            "{",
        ),
        (
            "three-byte scalar",
            "const P: (char, char) = ('—','{');\n",
            "{",
        ),
        (
            "ascii, the shape that already worked",
            "const P: char = '{';\n",
            "{",
        ),
        (
            "an escape beside it",
            "const P: (char, char) = ('\\u{7f}','{');\n",
            "{",
        ),
    ] {
        let blanked = blank_comments_and_strings(source);
        assert!(
            !blanked.contains(leaked),
            "{label}: a `{leaked}` inside a char literal survived as code: {blanked:?}"
        );
        assert_eq!(
            blanked.len(),
            source.len(),
            "{label}: the blanking moved byte offsets, which callers map to lines"
        );
        assert!(
            blanked.contains("const P"),
            "{label}: the blanking ate the code around the literal: {blanked:?}"
        );
    }

    // The controls. A lifetime is not a char literal, and a blanker that said
    // "yes" to every `'` would blank from the tick to the next one — taking the
    // signature with it.
    for lifetime in [
        "fn f<'a>(x: &'a str) -> &'a str { x }\n",
        "fn g<'a,'b>(x: &'a str, y: &'b str) -> usize { x.len() + y.len() }\n",
        "fn h(x: &'_ str) -> &'static str { \"k\" }\n",
    ] {
        let blanked = blank_comments_and_strings(lifetime);
        assert!(
            blanked.contains("str") && blanked.contains('{'),
            "a lifetime was read as a char literal and swallowed the code after \
             it: {lifetime:?} -> {blanked:?}"
        );
    }
    // And its sibling, which KEEPS literals instead of blanking them, is driven
    // over the same shapes. Its failure mode is the opposite one — it can only
    // lose bytes — so what it must do is leave a comment-free source alone and
    // still remove the comment after a multi-byte literal. Measured over all 92
    // source files, its output is byte-identical before and after this repair;
    // both blankers consult one scanner, which is what keeps it that way.
    let kept = "const P: (char, char) = ('é','{');\nlet q = '😀';\nlet r = '—';\n";
    assert_eq!(
        blank_comments(kept),
        kept,
        "the sibling blanker altered a source that holds no comment at all"
    );
    let commented = blank_comments("const P: char = 'é'; // names \"docker\"\n");
    assert!(
        commented.starts_with("const P: char = 'é';"),
        "the sibling blanker lost the literal: {commented:?}"
    );
    assert!(
        !commented.contains("docker"),
        "the comment after a multi-byte char literal survived: {commented:?}"
    );

    // 2. The same defect through `production_code`, end to end. Production
    //    above, an inline test module holding the pair, production below — the
    //    exact geometry of `src/agent/claude.rs`.
    let attacked = "fn above() {}\n\
                    #[cfg(test)]\n\
                    mod tests {\n\
                        const P: (char, char) = ('é','{');\n\
                    }\n\
                    fn forged_below() {}\n";
    let region = production_code(attacked);
    assert!(region.contains("fn above()"), "{region:?}");
    assert!(
        region.contains("fn forged_below()"),
        "the desync blanked from the test module to end of file, so every \
         production item below it is invisible to every census: {region:?}"
    );
    assert!(
        !region.contains("const P"),
        "the test module itself must still be removed: {region:?}"
    );
}

/// A region that cannot find where an item ends blanks the attribute, not the
/// file.
///
/// The second half of `PR7-R2C-CHAR-LITERAL-DESYNC`, and the half that decides
/// how much a desync costs. `configured_item_end` has two give-up paths — an
/// unbalanced brace and an item with no terminator before end of file — and both
/// used to return `bytes.len()`, which [`production_code`] reads as "the item is
/// the rest of the file" and blanks. That converts a tokeniser that has lost
/// phase into **silence**: every production item below the attribute leaves
/// every census that consults this region, and the census reports zero
/// offenders.
///
/// They return `start` now. The test module reads as production, the counts go
/// up rather than down, and a census that pins an expected set fails by name.
/// Neither path is reachable from this tree as it stands — measured, zero
/// occurrences over all 92 source files — so this drives them with input that
/// does reach them, which is the only way a give-up path is ever seen.
#[test]
fn a_region_that_cannot_find_an_items_end_blanks_the_attribute_not_the_file() {
    // An unbalanced brace: `mod tests {` never closes.
    let region = production_code("fn above() {}\n#[cfg(test)]\nmod tests {\nfn below() {}\n");
    assert!(region.contains("fn above()"), "{region:?}");
    assert!(
        region.contains("fn below()"),
        "an unbalanced brace blanked the rest of the file: {region:?}"
    );
    assert!(
        region.contains("mod tests {"),
        "the test module must read as production when the region cannot find its \
         end, so the censuses go loud: {region:?}"
    );
    assert!(
        !region.contains("#[cfg(test)]"),
        "the attribute itself is still removed: {region:?}"
    );

    // An item with no terminator before end of file.
    let region = production_code("fn above() {}\n#[cfg(test)]\nuse a::b\n");
    assert!(region.contains("fn above()"), "{region:?}");
    assert!(
        region.contains("use a::b"),
        "an unterminated item blanked the rest of the file: {region:?}"
    );
    assert!(!region.contains("#[cfg(test)]"), "{region:?}");

    // The control: when the item *does* close, it is still removed in full.
    let region = production_code("fn above() {}\n#[cfg(test)]\nmod tests {\n}\nfn below() {}\n");
    assert!(region.contains("fn above()") && region.contains("fn below()"));
    assert!(
        !region.contains("mod tests"),
        "a well-formed item is still removed: {region:?}"
    );
}

// ---------------------------------------------------------------------------
// The T-CONTAINER mechanical checklist
// ---------------------------------------------------------------------------

/// The nineteen tests `transaction_fault_matrix` row `T-CONTAINER` names in its
/// `test:` field, transcribed from the frozen packet.
///
/// **Transcribed, not read.** The packet is not in this repository, so the list
/// is a literal here the way [`PACKET_PRIMITIVES`] is — the no-self-oracle rule
/// requires the expected values to come from the packet's text rather than from
/// the tree, and a literal is the only shape that survives into CI.
///
/// Order is the packet's own. `windows_orphan_window_documented` is the last
/// entry and the packet writes it as `windows_orphan_window_documented (ST-16)`;
/// the trailing citation is not part of the identifier.
const T_CONTAINER_TESTS: [&str; 19] = [
    "container_intent_written_before_run",
    "container_created_from_recorded_image_id_and_verified",
    "substituted_image_id_refused_before_start",
    "orphan_reclaimed_before_slot_reset",
    "live_owner_untouched_while_dead_orphan_reclaimed",
    "labeled_orphan_without_intent_reclaimed",
    "same_run_resume_reclaims_earlier_incarnation_orphan",
    "same_run_resume_censuses_recorded_root_after_default_changed",
    "probe_name_reuse_across_incarnations_never_collides",
    "repeated_crashes_reclaim_every_dead_incarnation",
    "concurrent_reclaimers_converge",
    "schema4_probe_container_owned_during_preflight_untouched_by_foreign_census",
    "legacy_container_selection_refused_before_effects",
    "census_refuses_when_intents_exist_without_reachable_runtime",
    "census_proceeds_without_runtime_when_no_intent_exists",
    "census_report_names_reclaimed_probe_boundary",
    "failing_preflight_probe_on_resume_refuses_before_recovery_event_and_reclaims_probe_containers",
    "unix_reaper_kills_labeled_containers",
    "windows_orphan_window_documented",
];

/// Where `name` is defined as a `#[test]` function, over code with comments and
/// string literals blanked.
///
/// Blanked, because the failure this predicate exists to avoid is a name that
/// appears only in prose. Nine of the nineteen are quoted in a doc comment
/// somewhere in `src/runner/container/**` — `substituted_image_id_refused_
/// before_start` is named in `runtime.rs` and twice in `fake.rs` and is a
/// function in neither — so a `grep` for the bare string passes on a tree that
/// deleted the test and kept the sentence describing it.
fn defining_test_sites(name: &str) -> Vec<String> {
    let needle = format!("fn {name}(");
    let mut sites = Vec::new();
    for (path, source) in scanned_sources() {
        let code = blank_comments_and_strings(&source);
        let Some(index) = code.find(&needle) else {
            continue;
        };
        // `#[test]` sits above the signature, separated at most by the other
        // attributes a test carries (`#[cfg(...)]`, `#[should_panic]`) and by
        // the doc comment, which blanking has already turned into spaces.
        let preceding = &code[index.saturating_sub(400)..index];
        if preceding.contains("#[test]") {
            sites.push(path);
        }
    }
    sites
}

/// Every test `T-CONTAINER` names exists in this tree, as a test.
///
/// **The gate no gate was reading.** `phase9.sh` reads
/// `decisions.pr_sequence[N].slice_contract.proof_tests` and fails a slice that
/// deletes or renames one of its contract-named proof tests — the repair for
/// `PR4-CONTRACT-NAMED-PROOF-TEST-DELETED`. All **four** of PR6's `proof_tests`
/// are prose describing test families, so that gate parses zero identifiers out
/// of this slice and its zero-checked-is-a-failure rule fires without measuring
/// anything. The slice's actual mechanical checklist is somewhere else
/// entirely: `transaction_fault_matrix` row `T-CONTAINER`'s `test:` field, which
/// nothing in this repository read.
///
/// **This gate is orchestrator-added, not packet-required**, and says so rather
/// than implying otherwise. The packet enumerates the nineteen tests; it does
/// not require a meta-test that transcribes them. It is a control, kept because
/// a slice whose only mechanical checklist is unread is worse off without one.
///
/// # What this proves, and what it does not
///
/// **Proves:** each of the nineteen names is a `#[test]` function in real code
/// — not in a comment, not in a string literal, not merely a helper `fn` with
/// the right name. A rename, a deletion, or a demotion to a plain function
/// fails it by name, on every platform, because it is a source census rather
/// than a symbol census (two of the nineteen are behind `cfg(unix)` /
/// `cfg(windows)` and a symbol census would report each missing on the other
/// platform).
///
/// **Does not prove:** that any of them tests what its name claims. A test with
/// the right name and a tautological body satisfies this gate completely. That
/// is the boundary, stated here rather than left for a reviewer to find: this
/// is a **presence** gate over an enumeration nothing else reads, and the
/// evidence that the nineteen hold their clauses is the mutation witnessing in
/// the lanes' own reports, not this.
///
/// The second field it holds constant is the **body**; what varies is the
/// name and the file. The controls at the end vary the other way — one body
/// shape at a time, name held fixed — so the predicate is shown refusing a
/// comment, a string and a plain `fn`, and accepting a real test.
#[test]
fn every_test_the_container_fault_row_names_is_a_test_in_this_tree() {
    // The transcription itself is checked for the two ways a hand-written list
    // decays: a duplicate (which would let a missing name hide behind a present
    // one and keep the count at nineteen) and a name that is not an identifier.
    let unique: BTreeSet<&str> = T_CONTAINER_TESTS.iter().copied().collect();
    assert_eq!(
        unique.len(),
        T_CONTAINER_TESTS.len(),
        "the transcription repeats a name"
    );
    for name in T_CONTAINER_TESTS {
        assert!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                && name.contains('_'),
            "`{name}` is not the snake_case identifier the fault row names"
        );
    }

    let mut absent = Vec::new();
    let mut found = 0usize;
    for name in T_CONTAINER_TESTS {
        match defining_test_sites(name).as_slice() {
            [] => absent.push(name),
            sites => {
                found += 1;
                assert_eq!(
                    sites.len(),
                    1,
                    "`{name}` is defined as a test in {} files ({sites:?}); the fault row names \
                     one test and two would let either rot",
                    sites.len()
                );
            }
        }
    }
    assert!(
        absent.is_empty(),
        "T-CONTAINER names {} tests and {} are not tests in src/: {absent:#?}\n\
         The fault row is this slice's mechanical checklist and nothing else reads it.",
        T_CONTAINER_TESTS.len(),
        absent.len()
    );
    assert_eq!(found, T_CONTAINER_TESTS.len());

    // POSITIVE CONTROL. A census that can only say yes reports success from a
    // predicate that matched nothing -- `PR5-DOCKER-CENSUS-CANNOT-FAIL`, where a
    // needle that lived inside a string made the search unfalsifiable. Drive the
    // same predicate over a name that is not in the tree and require it to say
    // so, so a `defining_test_sites` that returned a constant fails here.
    assert!(
        defining_test_sites("a_test_this_tree_does_not_contain_and_never_will").is_empty(),
        "the predicate finds a test that does not exist, so its `absent` list means nothing"
    );

    // And it must be reading a tree. `scanned_sources` asserts its own walk
    // found files; this asserts the *blanking* left code behind, because a
    // blanker that erased everything would make every name absent and the
    // failure would read as nineteen deleted tests.
    let (_, container) = scanned_sources()
        .into_iter()
        .find(|(path, _)| path == "src/runner/container/tests.rs")
        .expect("the container suite is in the scanned tree");
    let blanked = blank_comments_and_strings(&container);
    assert!(
        blanked.contains("#[test]"),
        "the blanker erased the code it is meant to leave"
    );
    assert!(
        !blanked.contains("Orderings are most of the contract"),
        "the blanker left a doc comment behind, so a name in prose would satisfy this gate"
    );
}

/// The presence predicate refuses every shape that is not a test.
///
/// Separated from the gate above so a failure says which half broke: the tree,
/// or the thing that reads it. Each source varies exactly one property against
/// the accepted shape and holds the name fixed.
#[test]
fn the_container_fault_row_predicate_refuses_a_name_that_is_only_prose() {
    let name = "concurrent_reclaimers_converge";
    let needle = format!("fn {name}(");

    // Accepted: a real test.
    let accepted = format!("#[test]\nfn {name}() {{ assert!(true); }}\n");
    let code = blank_comments_and_strings(&accepted);
    assert!(code.contains(&needle) && code.contains("#[test]"));

    // Refused, one property changed at a time.
    for (label, source) in [
        (
            "a doc comment",
            format!("/// see fn {name}()\nfn other() {{}}\n"),
        ),
        (
            "a line comment",
            format!("// fn {name}()\nfn other() {{}}\n"),
        ),
        (
            "a block comment",
            format!("/* fn {name}() */\nfn other() {{}}\n"),
        ),
        (
            "a string literal",
            format!("const N: &str = \"fn {name}()\";\n"),
        ),
        ("a plain fn", format!("fn {name}() {{}}\n")),
    ] {
        let code = blank_comments_and_strings(&source);
        let is_test = code
            .find(&needle)
            .is_some_and(|index| code[index.saturating_sub(400)..index].contains("#[test]"));
        assert!(
            !is_test,
            "{label} satisfies the presence predicate, so the gate passes on a deleted test"
        );
    }
}

/// The R19 view directory has **one** definition in this tree.
///
/// `PR6E-005`. `src/runner/container/exec.rs` mounts the disposable Git view and
/// `src/runner/container/census.rs` finds it again after a coordinator death.
/// They were written in different lanes and each had its own definition of
/// `<R>/views/<container-name>` — lane A's `join("views")` literal and lane C's
/// `VIEWS_DIR` const — with nothing crossing them. Measured on the merged tree:
/// `VIEWS_DIR = "views-mutated"` passed **all 1324 tests**, because lane C's
/// fixtures plant orphan views through `view_path` itself and lane A's assert
/// its own literal. A divergence leaves every orphan view unreclaimed after a
/// crash, against `resource_accounting` R19's `NoRunFinished` ("pruned at the
/// next write-command start after the owning container is observed terminated")
/// and ST-16's closing clause "ledgers R19/R26 balance".
///
/// `exec::view_dir` now delegates to `census::view_path`, so the two cannot
/// disagree. This is the guard against a **third** definition: the segment is
/// declared once, by one const, and a second production site that joins a
/// `"views"` literal fails here by name.
///
/// The class is `PR5D-VISIBILITY-CHECK-DUPLICATED` — a hand-maintained value
/// kept in two places, where breaking one copy left the suite green because the
/// other still answered.
#[test]
fn the_view_directory_has_one_definition_in_the_tree() {
    // The domain is the container substrate's PRODUCTION modules. Test modules
    // are excluded by name rather than by `production_region`, deliberately:
    // `src/runner/container/tests.rs` is a whole-file `#[cfg(test)] mod tests;`
    // with no inline marker, so `production_region` returns all 3 000 lines of
    // it as production and a fixture asserting the path it expects would read as
    // a second declaration. That inconsistency is `PR6E-006` and is a finding of
    // its own; this test does not depend on it being repaired.
    let container: Vec<(String, String)> = scanned_sources()
        .into_iter()
        .filter(|(path, _)| {
            path.starts_with("src/runner/container") && !path.ends_with("/tests.rs")
        })
        .collect();
    let modules: BTreeSet<&str> = container.iter().map(|(path, _)| path.as_str()).collect();
    // CONTROL, and it is the one that stops this going vacuous: name the modules
    // the scan must be looking at. A filter that matched nothing, or a rename
    // that moved a half of the seam out of the scanned set, fails here rather
    // than reporting one clean site — `PR5-DOCKER-CENSUS-CANNOT-FAIL`.
    assert_eq!(
        modules,
        BTreeSet::from([
            "src/runner/container.rs",
            "src/runner/container/census.rs",
            "src/runner/container/env.rs",
            "src/runner/container/exec.rs",
            "src/runner/container/fake.rs",
            "src/runner/container/intent.rs",
            "src/runner/container/resolve.rs",
            "src/runner/container/runtime.rs",
            "src/runner/container/view.rs",
        ]),
        "the container substrate's production modules moved; the seam this test \
         pins may no longer be inside the scanned set"
    );

    let mut sites = Vec::new();
    let mut located = Vec::new();
    for (path, source) in &container {
        let code = blank_comments(&production_region(source));
        for (index, _) in code.match_indices("\"views\"") {
            let line = code[..index].matches('\n').count() + 1;
            // The property is "one site, and it is the census's". The LINE is
            // incidental: pinning it made this test fail when repair C1's merge
            // shifted census.rs by four lines, which is a true statement about
            // line numbers and says nothing about the seam. Assert the path;
            // carry the line into the message, where a human wants it.
            sites.push(path.clone());
            located.push(format!("{path}:{line}"));
        }
    }
    assert_eq!(
        sites,
        vec!["src/runner/container/census.rs".to_owned()],
        "the R19 view directory segment is declared in more than one production \
         site. `census::VIEWS_DIR` is the one definition and `exec::view_dir` \
         delegates to `census::view_path`; a second literal is a path that can \
         drift away from the census that has to find it, and no behavioural test \
         crosses the two halves. Sites found: {located:?}"
    );

    // And the scan can see a declaration at all: a blanker that erased the code
    // would report zero sites, which reads as "one definition" only because the
    // expected list happens to be short.
    let (_, census) = container
        .iter()
        .find(|(path, _)| path == "src/runner/container/census.rs")
        .expect("the census module is in the scanned set");
    assert!(
        blank_comments(&production_region(census))
            .contains("pub const VIEWS_DIR: &str = \"views\";"),
        "the scan cannot see the declaration it is counting"
    );

    // And the two halves really do answer the same thing, driven rather than
    // read: the mount side and the census side, same inputs, same path.
    let root = Path::new("/private/root");
    let name = crate::runner::container::intent::ContainerName::from_parts(
        "repokey",
        "run01",
        "inc01",
        "0123456789abcdef",
    )
    .expect("a well-formed container name");
    assert_eq!(
        crate::runner::container::exec::view_dir(root, &name),
        crate::runner::container::census::view_path(root, &name),
        "the runner mounts the view somewhere the census does not look"
    );
}

/// **The whole-file test modules a census skips are the crate's declarations,
/// and there are seventeen of them.**
///
/// The class boundary for `PR7-R5-ATT-001`. Four whole-tree censuses skip test
/// files; three took the set from
/// [`census_domain::declared_whole_file_test_modules`] and one wrote its own
/// rule, `path.file_stem() == "tests"`. That covers fourteen files. The crate
/// declares **seventeen**, and the three it misses are exactly the ones a
/// census is most likely to trip over — a scaffold and a fake exist to *name*
/// what production names, and `scaffold.rs` sits inside the `engine/topology`
/// domain one of those censuses walks.
///
/// Named individually rather than counted, because a count alone would pass if
/// the derivation swapped one file for another.
#[test]
fn the_declared_whole_file_test_modules_are_seventeen_and_three_are_not_called_tests() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("src is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }

    let modules = crate::effects::census_domain::whole_file_test_modules(&files, 13);
    let named: Vec<String> = modules
        .iter()
        .filter(|path| path.file_stem().is_none_or(|stem| stem != "tests"))
        .map(|path| {
            path.strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    assert_eq!(
        named,
        vec![
            "engine/topology/scaffold.rs".to_owned(),
            "events/log/premove.rs".to_owned(),
            "runner/container/fake.rs".to_owned(),
        ],
        "these are the whole-file test modules a `file_stem == \"tests\"` rule does not see, and \
         a census that uses that rule reads them as production"
    );
    assert_eq!(
        modules.len(),
        17,
        "the crate declares {} whole-file test modules; a census skipping fourteen of them by \
         file name leaves the rest inside its domain",
        modules.len()
    );
}

/// [`production_code`] removes the item and keeps the file.
///
/// Every shape here is one this tree actually contains, and each is a way a
/// truncating region loses production code. The censuses that use this helper
/// count over the whole tree, so a shape it mishandles is a hole nobody would
/// see: the count would simply be lower.
#[test]
fn the_production_code_region_removes_a_configured_item_and_keeps_the_rest() {
    // A `mod tests;` declaration. Thirteen files in this tree end with one, and
    // everything below it used to be outside every region that truncates.
    let region = production_code("fn above() {}\n#[cfg(test)]\nmod tests;\nfn below() {}\n");
    assert!(region.contains("fn above()"), "{region:?}");
    assert!(region.contains("fn below()"), "{region:?}");
    assert!(!region.contains("mod tests;"), "{region:?}");
    assert_eq!(
        region.lines().count(),
        4,
        "the item is blanked in place, so line numbers survive: {region:?}"
    );

    // A `mod tests { … }` block, brace-matched rather than indentation-matched.
    let region = production_code(
        "fn above() {}\n#[cfg(test)]\nmod tests {\n    fn inner() { let _ = 1; }\n}\nfn below() {}\n",
    );
    assert!(region.contains("fn above()") && region.contains("fn below()"));
    assert!(!region.contains("fn inner()"), "{region:?}");

    // A `#[cfg(test)] use`, which truncates `production_region` and is the
    // shape `src/engine/coordinator.rs` carries on line 36 of 1599.
    let region = production_code("use a::b;\n#[cfg(test)]\nuse c::d;\nfn below() {}\n");
    assert!(region.contains("use a::b;") && region.contains("fn below()"));
    assert!(!region.contains("use c::d;"), "{region:?}");

    // A braced `use`, whose item ends at `}` and takes the `;` with it.
    let region = production_code("#[cfg(test)]\nuse a::{b, c};\nfn below() {}\n");
    assert!(region.contains("fn below()"));
    assert!(!region.contains("a::"), "{region:?}");
    assert!(
        !region.contains(';'),
        "the trailing `;` goes with the item: {region:?}"
    );

    // A test-only `const` whose value is a string, and a test-only `fn`.
    let region = production_code(
        "#[cfg(test)]\npub const ALL: &str = \"x\";\n#[cfg(test)]\npub(super) fn f() { g(); }\nfn below() {}\n",
    );
    assert!(region.contains("fn below()"));
    assert!(
        !region.contains("ALL") && !region.contains("g();"),
        "{region:?}"
    );

    // A struct field, which ends at its comma rather than at the struct's brace
    // (`src/engine/options.rs` has three).
    let region = production_code(
        "struct S {\n    kept: u8,\n    #[cfg(test)]\n    gone: Option<u8>,\n    also_kept: u8,\n}\n",
    );
    assert!(region.contains("kept: u8,") && region.contains("also_kept: u8,"));
    assert!(!region.contains("gone"), "{region:?}");

    // Attributes stacked on one item belong to that item.
    let region = production_code("#[cfg(test)]\n#[allow(dead_code)]\nmod tests;\nfn below() {}\n");
    assert!(region.contains("fn below()"));
    assert!(!region.contains("mod tests;"), "{region:?}");
}

/// A `#[cfg(test)]` that is prose neither cuts nor is removed.
///
/// The two attacks the `//`-only strip this replaced could not see, both
/// measured against the barrier census: with either one planted as line 1 of a
/// production file, a second `TopologyFold::parse_log` route in the same file
/// became invisible and the census passed.
#[test]
fn a_configured_attribute_in_prose_removes_nothing() {
    for prose in [
        "/* a fixture in prose: #[cfg(test)] opens a test module */\nfn kept() {}\n",
        "const CFG_TEST_ATTR: &str = \"#[cfg(test)]\";\nfn kept() {}\n",
        "//! prose naming #[cfg(test)]\nfn kept() {}\n",
        "/// a doc comment naming #[cfg(test)]\nfn kept() {}\n",
    ] {
        let region = production_code(prose);
        assert!(
            region.contains("fn kept()"),
            "a `#[cfg(test)]` in prose removed the item after it: {prose:?} -> {region:?}"
        );
        assert!(
            !region.contains("#[cfg(test)]"),
            "the attribute survived the blanking: {region:?}"
        );
    }
    // And a real attribute beside prose that quotes one is still found.
    let region = production_code(
        "// prose: #[cfg(test)] mod tests;\n#[cfg(test)]\nmod tests;\nfn kept() {}\n",
    );
    assert!(region.contains("fn kept()"), "{region:?}");
    assert!(!region.contains("mod tests;"), "{region:?}");
}

/// The region is a superset of [`production_region`]'s, file by file, over the
/// tree — and keeps what the truncating region cannot: the code below the cut.
///
/// # What each assertion here is worth, because they are not worth the same
///
/// The prefix comparison is a **consistency check on a construction, and it
/// cannot fail.** [`production_code`] never writes below the index of its first
/// `#[cfg(test)]` match, [`production_region`] cuts at exactly that index, and
/// no token straddles a cut that lands on visible code — so the two sides are
/// the same bytes of the same blanking, and no input separates them. It is kept
/// because it would start failing if either function's cut point moved, which is
/// a real regression; it is not the non-weakening proof, and this doc used to
/// claim it was.
///
/// What carries the claim is the rest: `strictly_larger >= 8` and the
/// `src/engine/coordinator.rs` membership check (a strict gain somewhere, by
/// name), and the sentinel block below, which is the one property the truncating
/// region does not have and the one a desync destroys — an item appended *below*
/// everything the file declares is still in the region. That block fails if
/// `configured_item_end` ever blanks to end of file again, on any file in the
/// tree, which is how `PR7-R2C-CHAR-LITERAL-DESYNC` hid a forged item with a
/// zero-byte region delta.
///
/// # The non-weakening measurement, corrected
///
/// The commit that introduced this helper claimed that over 15 census needles
/// and 92 source files the new region "drops 0 occurrences the old line-based
/// region kept". That is false as written, and the same commit deleted a census
/// row *because* of the occurrence it drops. Re-measured over the tree **as that
/// commit left it**, restricted to the 76 files the censuses actually scan
/// (whole-file test modules excluded, as every census excludes them): **8
/// (file, needle) pairs drop, 20 occurrences**, and every one of the 20 is prose
/// or a string literal rather than code —
///
/// | pair | occurrences | what they are |
/// |---|---|---|
/// | `src/agent/proc.rs` × `run_with_timeout` | 3 | doc comments; the census's expected count was re-derived to 5 |
/// | `src/effects.rs` × `Command::new(` | 1 | **a string literal**: `DENIAL_FIXTURES`' `source:` field, a fixture that exists to be refused |
/// | `src/effects.rs` × `run_with_timeout` | 1 | a doc comment in [`production_code`]'s own prose |
/// | five files × `TopologyFold` | 15 | doc comments; that needle decides *set membership* for `FOLD_MENTIONS` and all five files stay in the set |
///
/// So the claim that holds is "drops no occurrence that is **code**". The
/// string-literal drop is the in-domain one: `Command::new(` is a needle of
/// `runner::tests::every_production_process_start_is_classified`, `src/effects.rs`
/// is in that census's domain, and its row there was deleted by the same commit
/// — which is the counterexample to the sentence twenty lines above it.
///
/// The measurement is pinned to that commit's tree deliberately, because it is
/// not stable under editing: a doc comment written *here* naming
/// `RunnerRequest {` adds a ninth pair to it, since the old region counted
/// prose. Under the region this test is about it adds nothing at all, which is
/// the whole of why the blanking moved into the region.
#[test]
fn the_production_code_region_contains_the_truncated_one() {
    let mut compared = 0_usize;
    let mut strictly_larger = 0_usize;
    let mut gained: BTreeSet<String> = BTreeSet::new();
    for (path, source) in scanned_sources() {
        let truncated = blank_comments_and_strings(&production_region(&source));
        let whole = production_code(&source);
        let prefix = &whole[..truncated.len().min(whole.len())];
        assert_eq!(
            prefix.replace(' ', ""),
            truncated.replace(' ', ""),
            "{path}: the truncating region keeps code this one does not"
        );
        compared += 1;
        if whole.trim().len() > truncated.trim().len() {
            strictly_larger += 1;
            gained.insert(path);
        }
    }
    assert!(compared > 40, "only {compared} files were compared");
    // And it is a *strict* superset somewhere, or the two regions are the same
    // function and the comparison above proves nothing. Eleven files gain today,
    // and they are the ones holding code below their first `#[cfg(test)]`: the
    // eight `every_production_region_that_stops_early_stops_at_a_module` pins
    // that still have code under the cut, plus the three test files carrying a
    // `#[cfg(test)] mod this_file_is_test_only {}` marker whose whole purpose
    // was to zero the truncating region.
    assert!(
        strictly_larger >= 8,
        "only {strictly_larger} files gained anything, so the two regions are the same \
         function and this comparison proves nothing"
    );
    assert!(
        gained.contains("src/engine/coordinator.rs"),
        "the legacy coordinator — 35 of 1599 lines under the truncating region — must be one \
         of the files that gains, or the census that adopted this helper still cannot see it"
    );

    // The assertion that can fail, and the property the truncating region does
    // not have: an item appended below everything a file declares is still in
    // the region. A `configured_item_end` that gives up and blanks to end of
    // file takes it — silently, and for the whole file — which is how a
    // desynced tokeniser hides a forged item behind a zero-byte delta.
    const SENTINEL: &str = "\npub fn sentinel_below_every_configured_item() {}\n";
    let mut carried = 0_usize;
    for (path, source) in scanned_sources() {
        let region = production_code(&format!("{source}{SENTINEL}"));
        assert!(
            region.contains("fn sentinel_below_every_configured_item()"),
            "{path}: an item appended below the whole file is not in its region, so the \
             region ends somewhere earlier than the file does and everything past that \
             point is invisible to every census that counts over it"
        );
        carried += 1;
    }
    assert_eq!(
        carried, compared,
        "the sentinel pass and the prefix pass walked different trees"
    );
}

/// The **domain-deciding** function was written three times, the three
/// disagreed, and two of them are gone.
///
/// `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`, `PR6B-PRODUCTION-REGION-CUT-AT-A-CFG-\
/// TEST-USE`, and the class `PR5D-VISIBILITY-CHECK-DUPLICATED` names: a value
/// two places both maintain by hand disagree eventually, and the one that
/// disagrees silently is the one that decides what a census is allowed to see.
/// Measured across the tree by PR6 lane E, this crate had **three**
/// `production_region` implementations with three different semantics, and each
/// carried a hazard the other two did not:
///
/// | where | what it removed | the hazard it carried |
/// |---|---|---|
/// | [`production_region`] (`src/effects.rs`, `pub`) | everything from the **first** `#[cfg(test)]`, whatever it attaches to | a `#[cfg(test)] use` truncates the file |
/// | `runner::tests::production_region` (private, **removed**) | only `#[cfg(test)] mod … { … }` **blocks** | it did not blank comments, so its counts included prose |
/// | `events::log::tests::production_region` (private, still here) | from the first `#[cfg(test)]` in the **raw** source | `PR4-CENSUS-COMMENT-ORACLE`: a `#[cfg(test)]` in a comment truncates |
///
/// They were measured against each other rather than assumed: a `run_with_\
/// timeout` planted at the **last line** of `src/agent/claude.rs` — a file the
/// `effects.rs` region truncates to its first 66 of 1064 lines — is **seen** by
/// `runner::tests::every_production_process_start_is_classified`, because that
/// census used the second implementation. Two censuses in one crate, both
/// answering "every production X is classified", over two different domains.
///
/// PR7's census repair removed the second and left **two**, which is what the
/// count at the bottom now pins. Every whole-tree census that asks a
/// *prohibition* question — the barrier census, the four censuses in
/// `runner::tests`, the container token census — now shares
/// [`crate::effects::production_code`]: the whole file, comments and string
/// literals blanked, every `#[cfg(test)]` **item** removed rather than the file
/// truncated at the first one. It is a fourth semantics and deliberately not a
/// fourth `production_region`: truncation is right for a *domain* question and
/// wrong for a prohibition, and the two names say which is which.
/// `events::log::tests::production_region` survives because two censuses in that
/// file ask about one named file each and assert their own strip removed
/// something before counting.
///
/// # What this test pins
///
/// The files the `effects.rs` implementation truncates at something that is
/// **not a module**. Each one loses everything below the cut from every census
/// that consults it, silently:
///
/// | file | region | cuts at |
/// |---|---|---|
/// | `src/engine/options.rs` | 4 / 166 | `#[cfg(test)] use` |
/// | `src/engine/coordinator.rs` | 35 / 1598 | `#[cfg(test)] use` |
/// | `src/engine/attempt.rs` | 25 / 721 | `#[cfg(test)] use` |
/// | `src/engine/resume.rs` | 30 / 792 | `#[cfg(test)] pub(super) fn` |
/// | `src/agent/claude.rs` | 66 / 1064 | `#[cfg(test)] pub const` |
/// | `src/agent/codex.rs` | 163 / 2009 | `#[cfg(test)] pub const` |
/// | `src/agent/copilot.rs` | 107 / 871 | `#[cfg(test)] pub const` |
/// | `src/agent/proc.rs` | 970 / 7946 | `#[cfg(test)] pub(super) fn` |
/// | `src/agent/bin.rs` | 224 / 533 | `#[cfg(test)] impl` |
/// | `src/util.rs` | 680 / 897 | `#[cfg(test)] pub(crate) fn` |
///
/// `resume.rs`'s shape is a test-only **function**, not the `use` lane B named,
/// so a repair written against that name alone would leave it.
///
/// **This does not repair them.** Moving `#[cfg(test)]` items in three
/// schema-1..3 engine files and four PR4 adapter files is a change to earlier
/// slices' code with reach far beyond this claim, and PR6's
/// `invariants_preserved[1]` is "legacy engine execution unchanged". What it
/// does is make the shrink **counted**: an eleventh file joining this set fails
/// by name rather than quietly removing itself from every census that uses the
/// `effects.rs` region.
#[test]
fn every_production_region_that_stops_early_stops_at_a_module() {
    /// What the first `#[cfg(test)]` in `source` attaches to, or `None` when the
    /// file has none. Read out of the **blanked** text, exactly as
    /// [`production_region`] reads it, so a `#[cfg(test)]` quoted in a doc
    /// comment neither cuts nor is classified — `src/runner/container.rs`
    /// carries such a comment, its own warning about this hazard.
    fn cut_shape(source: &str) -> Option<String> {
        let blanked = blank_comments_and_strings(source);
        let cut = blanked.find("#[cfg(test)]")?;
        let after = blanked[cut + "#[cfg(test)]".len()..].trim_start();
        Some(
            after
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" "),
        )
    }

    /// Whether the cut is at a module — `mod tests {`, `mod fake;`,
    /// `pub(crate) mod fixtures`, `mod this_file_is_test_only {}`. A test module
    /// is what the region is *for*, so cutting at one loses nothing.
    fn is_module(shape: &str) -> bool {
        shape
            .split_whitespace()
            .next()
            .is_some_and(|first| first == "mod" || first.starts_with("pub"))
            && shape.contains("mod ")
    }

    let mut offenders = BTreeMap::new();
    let mut at_a_module = 0usize;
    for (path, source) in scanned_sources() {
        let Some(shape) = cut_shape(&source) else {
            continue;
        };
        if is_module(&shape) {
            at_a_module += 1;
            continue;
        }
        offenders.insert(path, shape);
    }

    let named: BTreeSet<&str> = offenders.keys().map(String::as_str).collect();
    assert_eq!(
        named,
        BTreeSet::from([
            "src/agent/bin.rs",
            "src/agent/claude.rs",
            "src/agent/codex.rs",
            "src/agent/copilot.rs",
            "src/agent/proc.rs",
            "src/engine/attempt.rs",
            "src/engine/coordinator.rs",
            "src/engine/options.rs",
            "src/engine/resume.rs",
            "src/util.rs",
        ]),
        "the set of files whose `effects::production_region` stops at something \
         other than a module moved. Everything below such a cut is invisible to \
         every census that consults that region, silently. Shapes found: \
         {offenders:#?}"
    );

    // CONTROLS, both directions. A classifier that answered one thing always
    // would produce this same set by luck on a tree with ten offenders.
    assert!(is_module("mod tests {"));
    assert!(is_module("mod fake; #[cfg(test)]"));
    assert!(is_module("pub(crate) mod fixtures"));
    assert!(!is_module("use super::X;"));
    assert!(!is_module("pub const ALL:"));
    assert!(!is_module("pub(super) fn resume_harness_inner("));
    assert_eq!(cut_shape("fn a() {}\n").as_deref(), None);
    assert_eq!(
        cut_shape("//! prose naming #[cfg(test)]\nfn a() {}\n").as_deref(),
        None,
        "a `#[cfg(test)]` in a comment classifies, so this census reads prose"
    );

    // And the domain really was walked: far more files cut at a module than not,
    // so an empty or near-empty scan cannot produce the expected set.
    assert!(
        at_a_module > 20,
        "only {at_a_module} file(s) cut at a module; the scan is not reading the tree"
    );

    // Both surviving implementations are still there, and no third has been
    // added. If one is deleted, unified or duplicated, this table is stale and
    // the doc comment above is a lie — which is the failure
    // `PR5D-CI-COMPONENT-CENSUS-COMMENT-ORACLE` is about, one level out.
    // Counted in code, not asserted from prose.
    let definitions: usize = scanned_sources()
        .iter()
        .map(|(_, source)| {
            blank_comments_and_strings(source)
                .matches("fn production_region(")
                .count()
        })
        .sum();
    assert_eq!(
        definitions, 2,
        "this crate no longer has exactly two `production_region` \
         implementations; the divergence table in this test's doc comment \
         describes a tree that no longer exists"
    );
    // And the shared prohibition region has exactly one definition, which is
    // the whole point of having removed the third `production_region`.
    let shared: usize = scanned_sources()
        .iter()
        .map(|(_, source)| {
            blank_comments_and_strings(source)
                .matches("fn production_code(")
                .count()
        })
        .sum();
    assert_eq!(
        shared, 1,
        "`production_code` is the one region every whole-tree prohibition census \
         shares; a second definition is the divergence this table exists to count"
    );
}

// ---------------------------------------------------------------------------
// R3b: the enumerations the reconciliation promised and did not supply
// ---------------------------------------------------------------------------

/// The nine `expected_failures_refusals`, each with the **ordering predicate**
/// it carries and the test that holds it.
///
/// `PR6-ENUM-011`. The reconciliation document states that the nine refusals
/// and the twelve ST-16 variants "are mapped" and never supplies the mappings,
/// so a clause with neither a named test nor an owned deferral was
/// indistinguishable from one with both. A promise in a markdown file is not
/// something a build can read; this is.
///
/// `(clause, ordering predicate, test)`. The ordering is written out because it
/// is the **independently droppable** half: a refusal test that proves only
/// *that* it refused holds none of "before any effect", "before any lock or
/// effect", "before any spawn", "before start", "before any recovery event", or
/// "by construction".
const PR6_REFUSALS: [(&str, &str, &str); 9] = [
    (
        "[runner] kind = container under a schema-1..3 fresh run or resume",
        "before any effect",
        "legacy_container_selection_refused_before_effects",
    ),
    (
        "unreachable runtime / reference absent / credential volume absent, at resolution",
        "before any lock or effect",
        "resolution_refuses_each_of_its_faults_before_any_lock_or_effect",
    ),
    (
        "a recorded shell or agent CLI that fails inside the recorded image",
        "before any recovery event or work spawn",
        "failing_preflight_probe_on_resume_refuses_before_recovery_event_and_reclaims_probe_containers",
    ),
    (
        "a created container whose reported image id differs from the record",
        "before start",
        "substituted_image_id_refused_before_start",
    ),
    (
        "reviewer write attempt",
        "the mount is `:ro`, so the write fails in the runtime",
        "real_docker_refuses_a_reviewer_write_to_its_read_only_mount",
    ),
    (
        "gate write outside mount",
        "the container root is read-only, so the write fails in the runtime",
        "real_docker_a_gate_write_outside_every_declared_mount_fails",
    ),
    (
        "container start without an intent",
        "by construction",
        "a_container_is_created_and_started_only_under_its_own_intent_record",
    ),
    (
        "an intent naming this process's own incarnation at census time",
        "before any effect",
        "an_intent_naming_this_processs_own_incarnation_is_refused_before_any_effect",
    ),
    (
        "an unreclaimable labeled container / intents without a reachable runtime",
        "blocks admission; before any recovery event",
        "census_refuses_when_intents_exist_without_reachable_runtime",
    ),
];

/// The twelve ST-16 variants (a)–(l), each mapped to the test that drives it.
///
/// `PR6-ENUM-011`. `T_CONTAINER_TESTS` is the packet's `test:` field and is a
/// *presence* list; this is the **variant** enumeration, which is a different
/// axis — several variants share a named test and one variant is carried by a
/// test the `test:` field does not name.
const ST16_VARIANTS: [(char, &str, &str); 12] = [
    (
        'a',
        "single owner dies -> next write-command start reclaims",
        "orphan_reclaimed_before_slot_reset",
    ),
    (
        'b',
        "live coordinator A while dead B's orphan exists in the same private root",
        "live_owner_untouched_while_dead_orphan_reclaimed",
    ),
    (
        'c',
        "labeled container without an intent, same liveness rule",
        "labeled_orphan_without_intent_reclaimed",
    ),
    (
        'd',
        "the Unix reaper kills labeled containers",
        "unix_reaper_kills_labeled_containers",
    ),
    (
        'e',
        "Windows documents the orphan window",
        "windows_orphan_window_documented",
    ),
    (
        'f',
        "same-run resume censuses the recorded root after the default moved",
        "same_run_resume_censuses_recorded_root_after_default_changed",
    ),
    (
        'g',
        "three incarnations, orphans from two dead ones, no collision",
        "repeated_crashes_reclaim_every_dead_incarnation",
    ),
    (
        'h',
        "a foreign write command and the resuming incarnation converge",
        "concurrent_reclaimers_converge",
    ),
    (
        'i',
        "schema-1..3 container selection refused; schema-4 probe containers untouched by a foreign census",
        "schema4_probe_container_owned_during_preflight_untouched_by_foreign_census",
    ),
    (
        'j',
        "intents present and runtime unreachable -> refuse; no intent and no runtime -> proceed",
        "census_proceeds_without_runtime_when_no_intent_exists",
    ),
    (
        'k',
        "a probe container killed before run_started is reclaimed, its boundary named",
        "census_report_names_reclaimed_probe_boundary",
    ),
    (
        'l',
        "a resume whose pre-flight probe fails ends before any recovery event, resumable",
        "failing_preflight_probe_on_resume_refuses_before_recovery_event_and_reclaims_probe_containers",
    ),
];

/// The clauses of `invariants_introduced` and of ST-20 that this slice owns,
/// each with a test **or** an owned deferral.
///
/// `PR6-ENUM-011`. The reconciliation decomposed neither, so descendant
/// containment, resumed-epoch attribution and report/status attribution had
/// neither a named test nor an owner. A deferral is written as
/// `defer:<slice>` and is as much an answer as a test name — what is not an
/// answer is silence.
const PR6_CLAUSES: [(&str, &str); 12] = [
    (
        "role mounts and no others",
        "the_mount_set_is_the_roles_own_and_reaches_nothing_of_the_coordinators",
    ),
    (
        "no engine refs, event log, or private artifacts visible",
        "the_role_view_carries_no_engine_refs_and_no_link_back_into_the_repository",
    ),
    (
        "disposable Git view",
        "a_git_dependent_tool_reads_the_role_view_and_cannot_see_the_engines_refs",
    ),
    (
        "probes certify the shell and CLI that will run",
        "the_shell_probe_runs_through_this_runner_as_a_registered_container_invocation",
    ),
    (
        "container contains descendants",
        "real_docker_a_container_contains_a_daemonised_descendant",
    ),
    (
        "INV-15: container intent/reclaim with incarnation-aware owner liveness",
        "the_liveness_rule_classifies_every_cell_of_owner_run_by_incarnation_by_lock",
    ),
    (
        "every container invocation has an owner run whose identity precedes it",
        "legacy_container_selection_refused_before_effects",
    ),
    (
        "INV-23: resolution by inspection, immutable image id, creation from the id with verification",
        "container_created_from_recorded_image_id_and_verified",
    ),
    (
        "INV-23: rebuild-from-record, inspection refusals before any spawn",
        "the_rebuild_returns_the_recorded_runner_exactly_however_the_config_differs",
    ),
    (
        "ST-20: every probe and invocation of the RESUMED epoch executes under the recorded boundary",
        "defer:PR7",
    ),
    (
        "ST-20: report.json and status name the run's kind, policy, image reference, id and digest",
        "defer:PR10",
    ),
    ("the container transition is wired into a run", "defer:PR7"),
];

/// Every enumeration the reconciliation promised is supplied here, and every
/// entry either names a test that exists or defers to a named slice.
///
/// `PR6-ENUM-011`. Three separate claims, each of which the document made and
/// none of which anything read:
///
/// 1. the **nine** refusals are mapped — and to an *ordering predicate* as well
///    as to a test, because the ordering is the droppable half;
/// 2. the **twelve** ST-16 variants (a)–(l) are mapped;
/// 3. `invariants_introduced` and the prose `proof_tests` are decomposed into
///    clauses, each with a test **or an owned deferral**.
///
/// A name that is not a `#[test]` in this tree fails here, through the same
/// [`defining_test_sites`] census `T_CONTAINER_TESTS` uses — so this cannot be
/// satisfied by prose, by a helper function with the right name, or by a string
/// in a comment.
///
/// **What this does not prove**, stated for the same reason the gate above
/// states it: that the named test holds the clause. This is a *mapping* gate.
/// The evidence that the clauses hold is the mutation witnessing recorded in
/// the repair reports.
#[test]
fn every_pr6_refusal_st16_variant_and_invariant_clause_names_a_test_or_an_owner() {
    // (1) The nine refusals, with distinct clauses and distinct orderings.
    assert_eq!(PR6_REFUSALS.len(), 9, "the contract states nine refusals");
    let clauses: BTreeSet<&str> = PR6_REFUSALS.iter().map(|(clause, ..)| *clause).collect();
    assert_eq!(clauses.len(), 9, "two rows name the same refusal");
    let orderings: BTreeSet<&str> = PR6_REFUSALS.iter().map(|(_, order, _)| *order).collect();
    assert!(
        orderings.len() >= 5,
        "the nine refusals carry {} distinct ordering predicates; a mapping in which every \
         refusal has the same ordering is one that dropped the orderings",
        orderings.len()
    );

    // (2) The twelve ST-16 variants, (a)-(l), each present exactly once.
    assert_eq!(ST16_VARIANTS.len(), 12);
    let letters: Vec<char> = ST16_VARIANTS.iter().map(|(letter, ..)| *letter).collect();
    assert_eq!(
        letters,
        ('a'..='l').collect::<Vec<char>>(),
        "the variants are not (a) through (l), in order and complete"
    );

    // (3) The clause decomposition, with deferrals owned by a named slice.
    let deferred: Vec<&str> = PR6_CLAUSES
        .iter()
        .map(|(_, answer)| *answer)
        .filter(|answer| answer.starts_with("defer:"))
        .collect();
    assert!(
        !deferred.is_empty(),
        "a decomposition in which nothing is deferred is one that quietly claimed PR7's and \
         PR10's clauses"
    );
    for answer in &deferred {
        let owner = answer.trim_start_matches("defer:");
        assert!(
            owner.starts_with("PR") && owner[2..].chars().all(|c| c.is_ascii_digit()),
            "`{answer}` defers to nobody in particular"
        );
    }

    // Every name that is not a deferral is a `#[test]` in this tree.
    let named: Vec<&str> = PR6_REFUSALS
        .iter()
        .map(|(_, _, test)| *test)
        .chain(ST16_VARIANTS.iter().map(|(_, _, test)| *test))
        .chain(
            PR6_CLAUSES
                .iter()
                .map(|(_, answer)| *answer)
                .filter(|answer| !answer.starts_with("defer:")),
        )
        .collect();
    assert!(named.len() >= 28, "{}", named.len());
    for name in &named {
        assert!(
            !defining_test_sites(name).is_empty(),
            "`{name}` is named by the PR6 reconciliation and is not a `#[test]` in this tree"
        );
    }

    // And the ST-16 mapping is consistent with the packet's own `test:` field:
    // every variant's test that appears there appears under the same name.
    for (letter, _, test) in &ST16_VARIANTS {
        if T_CONTAINER_TESTS.contains(test) {
            continue;
        }
        // A variant carried by a test the `test:` field does not name is
        // allowed and must be visible, not silent.
        assert!(
            matches!(letter, 'a' | 'b' | 'i'),
            "ST-16 ({letter}) is mapped to `{test}`, which the packet's own `test:` field does \
             not name; only the variants whose clause is split across tests may do that"
        );
    }
}
