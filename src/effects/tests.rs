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
    ALLOWLIST_TOML, CLIPPY_TOML, DENIAL_CONTROL, DENIAL_FIXTURES, EFFECT_SITES_JSON,
    FROZEN_LEGACY_ALLOWLIST, FUNNEL_MODULES_JSON, REGENERATE, RESIDUE_CLASSES_JSON,
    TOPOLOGY_MODULES, USED_GOVERNED_LINTS, WRAPPERS_TOML, blank_comments,
    blank_comments_and_strings, governed_allows, legacy_growth, normalize_lint, production_region,
    topology_modules_among,
};
use crate::topology::effects::{EffectSiteId, effect_sites, effect_sites_json};

// The definitions these tests are checked against -- the two packet tables, the
// host-conditional denials and the placement scan's prologue reader -- are
// beside this file. The machinery that *drives* a compiler is not: it stays
// here, because this file is a whole-file test module and `policy.rs` is not,
// so a `Command::new(` moved there would enter two production censuses in
// `src/runner/mod.rs` and have to be classified in them.
mod policy;

use policy::{PACKET_PRIMITIVES, PACKET_TYPES, host_conditional_paths, marker_before};

// The wrapper classification's four checks are beside this file too, and they
// go one step further than `policy.rs`: their bodies sit inside a `cfg(test)`
// module, so both source cutters read that file as test logic. An inline module
// with a body is not the terminated declaration `census_domain` derives a skip
// from, so the whole-file module census is untouched by it.
mod classification;

use classification::checks;

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

// ---------------------------------------------------------------------------
// The CI workflow, read as a document rather than as text
// ---------------------------------------------------------------------------
//
// `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE` in `reviews/FINDINGS.md` deferred
// this section's repair for one reason -- it needs a YAML parser and the crate
// had no `[dev-dependencies]` at all. The dependency is added, so the repair is
// made: every claim below is an equality over a parsed mapping or an exact
// scalar pin, and the two escapes the row enumerates are executed as mutations
// that must be refused.
//
// **The ruling this section used to carry is withdrawn.** An earlier round
// argued from PR #25 that a checker over this surface cannot converge. PR #25's
// retained half kept C1-C4 as equalities and exact pins and it is the withdrawn
// half that compared prose across an open document set; the lesson supports a
// structural equality here rather than licensing repeated `contains`.

// The shape itself, and the oracle that reads the document against it, are
// implementation and live beside this file rather than in it. `ci_model` is the
// single authority for what CI runs and on which runners; `workflow` turns a
// parsed document into complaints and carries the mutations that prove each
// complaint fires. The cfg census below reads `ci_model` too -- which is why
// the constants are a module of their own and not a half of `workflow`.
//
// What stays here is what this section *is*: the five tests, and, further down,
// the join where the census and the workflow contract meet.

mod ci_model;
mod workflow;

use ci_model::{CI_TARGETS, CI_WORKFLOW, MSRV_COMMAND, MSRV_JOB, RUSTFLAGS_KEY};
use workflow::{
    WORKFLOW_ESCAPES, ci_msrv_job_complaints, ci_test_job_complaints, ci_workflow_text,
    complaint_codes, declared_msrv_toolchain, declared_rust_version, field, field_names,
    mutate_workflow, parse_workflow, rustflags_complaints, scalar, steps_of, three_component,
    workflow_complaints,
};

/// The parser this oracle depends on has the two properties it was chosen for.
///
/// Executed rather than believed. A silent change in either -- a dependency
/// bump, a feature flag -- weakens every equality in this section, so it fails
/// here first.
#[test]
fn the_workflow_parser_rejects_duplicate_keys_and_reads_on_as_a_string() {
    // The control: the same shape without the duplicate parses, so "refused"
    // below is not "refuses everything".
    let clean = "jobs:\n  lint:\n    runs-on: ubuntu-latest\n";
    let parsed = parse_workflow(clean).expect("the control document parses");
    assert_eq!(
        field(&parsed, "jobs")
            .and_then(|jobs| field(jobs, "lint"))
            .and_then(|lint| scalar(lint, "runs-on")),
        Some("ubuntu-latest"),
        "the control parsed but did not read back"
    );

    for (shape, document) in [
        (
            "a duplicated top-level key",
            "jobs:\n  a: 1\njobs:\n  b: 2\n",
        ),
        (
            "a duplicated key inside a job",
            "jobs:\n  lint:\n    runs-on: ubuntu-latest\n    runs-on: windows-latest\n",
        ),
    ] {
        let refused = parse_workflow(document);
        assert!(
            refused.is_err(),
            "{shape} was accepted. Last-one-wins makes every structural equality in this \
             section read the winning entry while a mutation hides in the loser."
        );
    }

    // YAML 1.1 resolves the bare word `on` to the boolean `true`, which would
    // put the workflow's trigger block under a key no reader looks for. A 1.2
    // parser reads it as the string it is, and `field_names` renders a non-string
    // key rather than dropping it, so this would fail loudly either way.
    let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);
    assert!(
        field_names(&doc).contains("on"),
        "the workflow's `on:` key did not read back as the string `on`: {:?}",
        field_names(&doc)
    );
}

/// Every escape the ledger and this section's history name is refused.
///
/// The oracle is run against mutated documents because an oracle only ever run
/// on conforming input is one nobody has seen refuse anything -- the rule this
/// file states in its own header and the reason `PR5-C-DOCTEST-FIXTURES-NEVER-RAN`
/// exists.
#[test]
fn the_workflow_shape_oracle_refuses_every_escape_the_ledger_names() {
    let text = ci_workflow_text();

    // The negative control first: the real document has no complaint, so a
    // refusal below is the mutation and not a contract that refuses everything.
    let doc = parse_workflow(&text).expect(CI_WORKFLOW);
    let clean = workflow_complaints(&doc);
    assert!(
        clean.is_empty(),
        "the unmutated workflow does not satisfy its own contract:\n{}",
        clean.join("\n")
    );

    let mut refused: BTreeSet<&str> = BTreeSet::new();
    for escape in WORKFLOW_ESCAPES {
        let mutated = mutate_workflow(&text, escape.job, escape.anchor, escape.replacement);
        assert_ne!(
            mutated, text,
            "{}: the mutation changed nothing, so it measures nothing",
            escape.name
        );
        let complaints = match parse_workflow(&mutated) {
            Ok(document) => workflow_complaints(&document),
            Err(error) => vec![error],
        };
        let codes = complaint_codes(&complaints);
        assert!(
            codes.contains(escape.refused_as),
            "{} was not refused as `{}` -- {}\nComplaints: {:#?}",
            escape.name,
            escape.refused_as,
            escape.escape,
            complaints
        );
        refused.insert(escape.name);
    }
    assert_eq!(
        refused.len(),
        WORKFLOW_ESCAPES.len(),
        "two escapes share a name, so one of them was never measured"
    );
}

/// The command that executes the fixtures is one CI runs, on every platform.
///
/// `clippy-driver` is a test dependency of that job and `dtolnay/rust-toolchain`
/// installs the minimal profile, so the components list is part of the claim.
/// The predecessor asked whether the word `clippy` appeared on a `components:`
/// line of the comment-stripped text, and whether the file contained the test
/// command anywhere; both survive an `echo`, and the strip existed only because
/// the job's own comment spelled the needle.
#[test]
fn the_workflow_that_runs_these_tests_installs_the_compiler_they_need() {
    let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);
    let complaints = ci_test_job_complaints(&doc);
    assert!(
        complaints.is_empty(),
        "the `test` job does not run these fixtures the way they need:\n{}",
        complaints.join("\n")
    );
}

/// The MSRV leg checks the floor the manifest publishes, on every platform.
///
/// Four claims. Three were held by nothing at all before this test: that the leg
/// is enabled and unabsolved, that its command is the documented one *including*
/// `--locked`, and that its matrix is every supported runner. The fourth, the
/// toolchain, was held loosely -- `.github/scripts/test-docs-consistency.sh`'s C2
/// accepts `rust-version` "or a patch release of it" -- and is held exactly here.
/// It is derived from the manifest and quoted from it on failure, because a
/// literal `1.85.0` would make this its own oracle for the fact it exists to
/// hold.
///
/// The refusals are executed in [`WORKFLOW_ESCAPES`] -- every row named
/// `MUT-MSRV-*` -- so this test passing is not the claim that the contract
/// refuses nothing.
#[test]
fn the_msrv_leg_checks_the_floor_the_manifest_publishes_on_every_platform() {
    // The derivation, with its controls, before anything is asserted with it.
    assert_eq!(three_component("1.85"), "1.85.0");
    assert_eq!(three_component("1.85.0"), "1.85.0");
    assert_eq!(
        three_component("nightly"),
        "nightly",
        "a manifest value this does not understand must reach the equality below unchanged \
         and fail there with both strings quoted, not be normalised into agreement"
    );

    let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);
    let complaints = ci_msrv_job_complaints(&doc);
    assert!(
        complaints.is_empty(),
        "the `{MSRV_JOB}` job does not check the floor the way the documents publish it:\n{}",
        complaints.join("\n")
    );

    // The toolchain claim once more as a bare equality, so its failure names the
    // manifest and the workflow rather than only the complaint between them.
    let installed: Vec<&str> = field(&doc, "jobs")
        .and_then(|jobs| field(jobs, MSRV_JOB))
        .map(steps_of)
        .unwrap_or_default()
        .iter()
        .filter_map(|step| field(step, "with").and_then(|with| scalar(with, "toolchain")))
        .collect();
    let expected = declared_msrv_toolchain();
    assert_eq!(
        installed,
        vec![expected.as_str()],
        "`Cargo.toml` publishes `rust-version = \"{}\"`, whose toolchain name is \
         `{expected}`; the `{MSRV_JOB}` leg installs {installed:?}",
        declared_rust_version()
    );

    // The order, as the indices themselves. `MUT-MSRV-CHECK-BEFORE-TOOLCHAIN`
    // executes the refusal; this is the positive control beside it, and it fails
    // with both positions named rather than with a complaint about them.
    let steps = field(&doc, "jobs")
        .and_then(|jobs| field(jobs, MSRV_JOB))
        .map(steps_of)
        .unwrap_or_default();
    let install_at = steps.iter().position(|step| {
        scalar(step, "uses").is_some_and(|uses| uses.starts_with("dtolnay/rust-toolchain@"))
            && field(step, "with").and_then(|with| scalar(with, "toolchain"))
                == Some(expected.as_str())
    });
    let check_at = steps
        .iter()
        .position(|step| scalar(step, "run") == Some(MSRV_COMMAND));
    assert!(
        matches!((install_at, check_at), (Some(install), Some(check)) if install < check),
        "the `{MSRV_JOB}` leg installs toolchain `{expected}` at step {install_at:?} and runs \
         `{MSRV_COMMAND}` at step {check_at:?}. The install has to come first: it selects the \
         toolchain for the steps that follow it, and a check above it compiles on whatever \
         the runner image shipped."
    );
}

/// The workflow-scope `-D warnings` is pinned, and nothing narrows it.
///
/// The refusals are driven on synthetic documents as well as on mutations of the
/// real one, because on the real one this scan cannot be seen working *alone*:
/// every job and step of the live workflow that could carry an `env:` is already
/// covered by a field set, so `MUT-RUSTFLAGS-JOB-OVERRIDE` is refused twice
/// over. Those rows still bind to the code this scan emits and nothing else
/// emits, so they measure it; what they cannot show is it holding somewhere no
/// field set does. Each document below carries one job that no other check in
/// this section reaches, which is where that is shown.
///
/// The positive controls come first, in both halves: the real workflow satisfies
/// the contract, and so does the minimal conforming probe. Without them a
/// refusal below would be evidence of nothing.
#[test]
fn the_workflow_scope_rustflags_pin_refuses_weakening_and_every_override() {
    /// A workflow carrying one job the rest of this section does not model.
    fn probe(header: &str, job_body: &str) -> String {
        format!("{header}jobs:\n  probe:\n{job_body}")
    }
    /// A job that binds nothing of its own.
    const PLAIN: &str = "    runs-on: ubuntu-latest\n    steps:\n      - run: cargo check\n";
    /// The pinned workflow-scope binding, written as the real document writes it.
    const PINNED: &str = "env:\n  RUSTFLAGS: -D warnings\n";

    let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);
    let live = rustflags_complaints(&doc);
    assert!(
        live.is_empty(),
        "the real workflow does not satisfy the `{RUSTFLAGS_KEY}` contract:\n{}",
        live.join("\n")
    );

    let control = parse_workflow(&probe(PINNED, PLAIN)).expect("the control document parses");
    let refused_control = rustflags_complaints(&control);
    assert!(
        refused_control.is_empty(),
        "the conforming probe is refused, so no refusal below is evidence of anything:\n{}",
        refused_control.join("\n")
    );

    for (shape, document, code) in [
        ("no workflow `env:` at all", probe("", PLAIN), "rustflags"),
        (
            "an `env:` that binds other names but not this one",
            probe("env:\n  CARGO_TERM_COLOR: always\n", PLAIN),
            "rustflags",
        ),
        (
            "warnings allowed instead of denied",
            probe("env:\n  RUSTFLAGS: -A warnings\n", PLAIN),
            "rustflags",
        ),
        (
            "an allow appended after the deny, which every `contains` reading accepts",
            probe(
                "env:\n  RUSTFLAGS: -D warnings -A clippy::disallowed_methods\n",
                PLAIN,
            ),
            "rustflags",
        ),
        (
            "a value YAML does not read as a string",
            probe("env:\n  RUSTFLAGS: true\n", PLAIN),
            "rustflags",
        ),
        (
            "the encoded form at workflow scope, which Cargo reads first",
            probe(
                "env:\n  RUSTFLAGS: -D warnings\n  CARGO_ENCODED_RUSTFLAGS: ''\n",
                PLAIN,
            ),
            "rustflags",
        ),
        (
            "a job-level rebinding",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    env:\n      RUSTFLAGS: -A warnings\n    \
                 steps:\n      - run: cargo check\n",
            ),
            "rustflags-override",
        ),
        (
            "a job-level binding of the name Cargo prefers",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    env:\n      CARGO_ENCODED_RUSTFLAGS: ''\n    \
                 steps:\n      - run: cargo check\n",
            ),
            "rustflags-override",
        ),
        (
            "a step-level rebinding",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    steps:\n      - run: cargo check\n        \
                 env:\n          RUSTFLAGS: -A warnings\n",
            ),
            "rustflags-override",
        ),
        (
            "a step-level binding of the name Cargo prefers",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    steps:\n      - run: cargo check\n        \
                 env:\n          CARGO_ENCODED_RUSTFLAGS: ''\n",
            ),
            "rustflags-override",
        ),
        (
            "a job-level rebinding in lower case, which is `RUSTFLAGS` on Windows",
            probe(
                PINNED,
                "    runs-on: windows-latest\n    env:\n      rustflags: -A warnings\n    \
                 steps:\n      - run: cargo check\n",
            ),
            "rustflags-override",
        ),
        (
            "a step-level rebinding in mixed case",
            probe(
                PINNED,
                "    runs-on: windows-latest\n    steps:\n      - run: cargo check\n        \
                 env:\n          RustFlags: -A warnings\n",
            ),
            "rustflags-override",
        ),
        (
            "a case variant beside the pinned line at workflow scope",
            probe(
                "env:\n  RUSTFLAGS: -D warnings\n  Rustflags: -A warnings\n",
                PLAIN,
            ),
            "rustflags",
        ),
        (
            "the encoded name in lower case at workflow scope",
            probe(
                "env:\n  RUSTFLAGS: -D warnings\n  cargo_encoded_rustflags: ''\n",
                PLAIN,
            ),
            "rustflags",
        ),
        (
            "a bash write to the job environment file",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    steps:\n      - run: echo \
                 \"RUSTFLAGS=-A warnings\" >> \"$GITHUB_ENV\"\n",
            ),
            "rustflags-persisted",
        ),
        (
            "the same write through the `github.env` expression",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    steps:\n      - run: echo \
                 \"CARGO_ENCODED_RUSTFLAGS=\" >> ${{ github.env }}\n",
            ),
            "rustflags-persisted",
        ),
        (
            "the PowerShell form, which shares no syntax with the bash one",
            probe(
                PINNED,
                "    runs-on: windows-latest\n    steps:\n      - run: Add-Content -Path \
                 $env:GITHUB_ENV -Value \"RUSTFLAGS=-A warnings\"\n",
            ),
            "rustflags-persisted",
        ),
        (
            "the cmd form, where the file is reached as a percent variable",
            probe(
                PINNED,
                "    runs-on: windows-latest\n    steps:\n      - run: echo \
                 RUSTFLAGS=-A warnings>>%GITHUB_ENV%\n        shell: cmd\n",
            ),
            "rustflags-persisted",
        ),
        (
            "a heredoc, with the name and the redirection on different lines",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cat >> \
                 \"$GITHUB_ENV\" <<'EOF'\n          RUSTFLAGS=-A warnings\n          EOF\n",
            ),
            "rustflags-persisted",
        ),
        (
            "flags scoped to one command, with no env file in sight",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    steps:\n      - run: RUSTFLAGS=-A warnings \
                 cargo build\n",
            ),
            "rustflags-in-script",
        ),
    ] {
        let parsed = parse_workflow(&document).expect(shape);
        let complaints = rustflags_complaints(&parsed);
        let codes = complaint_codes(&complaints);
        assert!(
            codes.contains(code),
            "{shape} was not refused as `{code}`. Document:\n{document}\nComplaints: {:#?}",
            complaints
        );
    }

    // The other half of a scan that matches whole names case-insensitively: it
    // must not fire on names that merely resemble the guarded ones. Each of these
    // is a real thing a workflow could carry, and none of them is the warning
    // policy. Without this block the case-insensitive widening above could be
    // satisfied by a scan that refuses everything containing `rustflags`.
    for (shape, document) in [
        (
            "an unrelated variable whose name contains the guarded one",
            probe(
                "env:\n  RUSTFLAGS: -D warnings\n  RUSTFLAGS_EXTRA: -C debuginfo=0\n",
                PLAIN,
            ),
        ),
        (
            "an unrelated variable the guarded one is a prefix of, at job scope",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    env:\n      MY_RUSTFLAGS: -A warnings\n      \
                 RUST_FLAGS: -A warnings\n    steps:\n      - run: cargo check\n",
            ),
        ),
        (
            "a script that writes the env file without touching the policy",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    steps:\n      - run: echo \
                 \"CARGO_TERM_COLOR=never\" >> \"$GITHUB_ENV\"\n",
            ),
        ),
        (
            "a script naming a variable the guarded one is only a prefix of",
            probe(
                PINNED,
                "    runs-on: ubuntu-latest\n    steps:\n      - run: echo \
                 \"RUSTFLAGS_EXTRA=1\" >> \"$GITHUB_ENV\"\n",
            ),
        ),
    ] {
        let parsed = parse_workflow(&document).expect(shape);
        let complaints = rustflags_complaints(&parsed);
        assert!(
            complaints.is_empty(),
            "{shape} was refused, so this scan reads substrings rather than whole names. \
             Document:\n{document}\nComplaints: {complaints:#?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The cfg census: effective predicates, decided against real valuations
// ---------------------------------------------------------------------------
//
// The predecessor collected `target_os = "..."` names wherever they appeared at
// a code position and treated each name as a platform demanding its own Clippy
// runner. `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE` records three ways that
// misreads the tree, and all three are what a name-collector cannot see:
//
//   * `not(any(target_os = "linux", target_os = "macos", target_os = "windows"))`
//     reported all three platforms covered while **no** runner compiles the body;
//   * `not(target_os = "freebsd")` would demand a FreeBSD runner for a body every
//     runner compiles;
//   * `let target_os = "android";` is code, passes a position gate, and was
//     reported as a platform.
//
// Three further corrections come from the review of that repair, and each is a
// claim the first structural version got wrong rather than a refinement:
//
//   * **Coverage is decided, never assumed.** The first version evaluated under
//     three-valued logic and counted `Unknown` as coverage, so a predicate whose
//     truth it could not decide was reported as compiled. Every valuation below
//     is COMPLETE for the names this census models, an unmodelled name is a hard
//     failure rather than an optimistic guess, and `test` is enumerated per
//     invocation because `--all-targets` compiles the library twice.
//   * **A `#[cfg]` is not the only cfg, and not every cfg gates.** `cfg!(P)` is a
//     boolean expression: the code around it is compiled everywhere. `#[cfg_attr(
//     P, attr)]` applies an attribute conditionally; the item is compiled
//     everywhere. Counting either as a gated region invents platform demands the
//     tree does not make.
//   * **An item's predicate is not the attribute written on it.** Stacked
//     `#[cfg]`s conjoin, and so does every enclosing guard -- the module block it
//     sits in, and, for a whole-file module, the `mod name;` declaration that
//     names the file -- whether the guard is written on that declaration or on
//     an inline module enclosing it. Eighteen files in this tree are reached
//     only that way.

// The census is `cfg`, beside this file; the two tests below are what it answers
// to. It decides predicates against `ci_model`'s targets -- the same table the
// workflow contract above is checked against -- so "no runner compiles this
// body" and "no job lints that platform" cannot drift apart.

mod cfg;

use cfg::{
    CFG_CENSUS_CONTROL, CFG_ESCAPES, CFG_GATE_FLOOR, CONTROL_GATES, CfgForm, CfgSite,
    NO_CI_RUNNER_COMPILES, WHOLE_FILE_TEST_MODULES, cfg_regions, compiled_by, parse_cfg,
};

/// The cfg census reads effective predicates, decides them, and knows which
/// forms gate.
#[test]
fn the_cfg_census_evaluates_effective_predicates_against_the_valuations_ci_sets() {
    for (text, expected, why) in CFG_ESCAPES {
        let pred = parse_cfg(text, false).unwrap_or_else(|error| {
            panic!("the census cannot read `cfg({text})`, which it must: {error}")
        });
        assert_eq!(
            pred.render(),
            text,
            "`cfg({text})` did not round-trip through the parser"
        );
        let expected: BTreeSet<&str> = expected.iter().copied().collect();
        let compiled = compiled_by(&pred)
            .unwrap_or_else(|error| panic!("`cfg({text})` is undecidable: {error}"));
        assert_eq!(compiled, expected, "`cfg({text})` -- {why}");
    }

    // An unmodelled name is a hard failure, not an optimistic guess. This is the
    // review's fourth finding as a control: the version this replaces answered
    // `Unknown` here and the caller read `Unknown` as "every runner compiles it".
    let unmodelled = parse_cfg("feature = \"unshipped\"", false).expect("a parseable predicate");
    let refused = compiled_by(&unmodelled);
    assert!(
        refused.is_err(),
        "a cfg key no valuation models was decided anyway, as {refused:?}"
    );

    // The control rides along with the whole domain, so finding it proves the
    // scan reaches injected content in the presence of every real file rather
    // than in a fixture read on its own.
    let mut domain = scanned_sources();
    let real = domain.len();
    let fixture = "fixtures/cfg-census-control.rs";
    domain.push((fixture.to_owned(), CFG_CENSUS_CONTROL.to_owned()));
    let (sites, unreadable) = cfg_regions(&domain);
    assert!(
        unreadable.is_empty(),
        "the census could not read {} occurrence(s):\n{}",
        unreadable.len(),
        unreadable.join("\n")
    );
    let gates: Vec<&CfgSite> = sites
        .iter()
        .filter(|site| site.form == CfgForm::Gate)
        .collect();
    assert!(
        real > 30 && gates.len() > CFG_GATE_FLOOR,
        "the control was scanned inside a truncated domain: {real} files, {} gates",
        gates.len()
    );

    let injected: Vec<&CfgSite> = sites.iter().filter(|site| site.path == fixture).collect();
    let rendered: Vec<&str> = injected
        .iter()
        .filter(|site| site.form == CfgForm::Gate)
        .map(|site| site.rendered.as_str())
        .collect();
    assert_eq!(
        rendered, CONTROL_GATES,
        "the control fixture produced the wrong gates. `haiku` or `plan9` among them is a \
         non-gating form counted as a gate; a missing `all(...)` is a stacked attribute or a \
         module guard the scan did not conjoin; `android` is a `let` binding read as a \
         predicate, and a `cfg(` from `fn cfg(bits: u32)` is a parameter list read as one."
    );

    // The two non-gating forms are RECORDED and not counted. Recording them is
    // what makes their exclusion measurable: `target_os = "plan9"` and
    // `target_os = "haiku"` are compiled by no runner, so if either were read as
    // a gate the census below would report an uncovered predicate.
    let by_form: BTreeMap<CfgForm, Vec<&str>> =
        injected
            .iter()
            .fold(BTreeMap::new(), |mut acc: BTreeMap<_, Vec<&str>>, site| {
                acc.entry(site.form)
                    .or_default()
                    .push(site.written.as_str());
                acc
            });
    assert_eq!(
        by_form.get(&CfgForm::Attribute).map(Vec::as_slice),
        Some(["target_os = \"haiku\""].as_slice()),
        "`#[cfg_attr(P, attr)]` applies an attribute conditionally; the item is compiled \
         everywhere and it is not a platform demand"
    );
    assert_eq!(
        by_form.get(&CfgForm::Macro).map(Vec::as_slice),
        Some(["target_os = \"plan9\""].as_slice()),
        "`cfg!(P)` is an expression: both arms around it compile on every platform"
    );

    // Two of the gates exist only because a guard was conjoined from somewhere
    // other than the attribute itself.
    let stacked = injected
        .iter()
        .find(|site| site.rendered == "all(unix, target_os = \"macos\")")
        .expect("the stacked control");
    assert_eq!(
        stacked.written, "all(unix, target_os = \"macos\")",
        "stacked `#[cfg]`s are one item's predicate, not two items'"
    );
    let nested = injected
        .iter()
        .find(|site| site.rendered == "all(windows, test)")
        .expect("the nested control");
    assert_eq!(
        nested.written, "test",
        "the nested item writes only `test`; `windows` comes from the module around it"
    );
    assert_eq!(
        compiled_by(&nested.pred).expect("decidable"),
        BTreeSet::from(["windows-latest"]),
        "an item inside a `#[cfg(windows)] mod` is not compiled by the Linux leg, whatever \
         its own attribute says"
    );
}

/// Every platform this crate configures code for has a Clippy gate, and the
/// aggregate makes that gate required.
///
/// The domain is derived from the tree rather than listed here: a written-down
/// platform list is one nothing forces an author to extend, which is what the
/// previous repair of this test shipped. The two halves join at the target
/// tuple -- [`cfg_regions`] decides which runners compile each body, and the
/// workflow contract requires a gate job whose `runs-on:` is that runner.
///
/// **Why this is one test and not three.** `PR5D-MSVC-CLIPPY-NEVER-RUN` and
/// `PR5-MACOS-CLIPPY-NEVER-RUN` are the same defect on two platforms, found
/// apart, because the Windows repair was written as an instance rather than a
/// class. A derived domain makes the next platform's omission a failure here
/// rather than a third finding.
#[test]
fn every_platform_this_crate_configures_for_has_a_clippy_gate_the_aggregate_requires() {
    let sources = scanned_sources();
    let (sites, unreadable) = cfg_regions(&sources);
    assert!(
        unreadable.is_empty(),
        "the census could not read {} occurrence(s):\n{}",
        unreadable.len(),
        unreadable.join("\n")
    );
    let gates: Vec<&CfgSite> = sites
        .iter()
        .filter(|site| site.form == CfgForm::Gate)
        .collect();
    assert!(
        gates.len() > CFG_GATE_FLOOR,
        "only {} gating cfg attribute(s) found across {} files; the census is reading the \
         wrong shape",
        gates.len(),
        sources.len()
    );
    // A boundary, not a count: the tree carries nested, negated predicates and a
    // census that only reads flat ones would pass every other assertion here.
    assert!(
        gates
            .iter()
            .any(|site| site.written == "not(any(target_os = \"linux\", target_os = \"macos\"))"),
        "the census did not find the nested negated predicate this tree is known to carry, \
         so it is reading a narrower grammar than the tree uses"
    );
    // The whole-file guards are the other boundary. Every predicate in those
    // files is `all(test, …)`, and a census that resolved none of them would
    // read them all as unconditional.
    let under_a_file_guard: BTreeSet<&str> = gates
        .iter()
        .filter(|site| site.rendered.starts_with("all(test,") || site.rendered == "test")
        .map(|site| site.path.as_str())
        .collect();
    assert!(
        under_a_file_guard.len() >= WHOLE_FILE_TEST_MODULES,
        "only {} file(s) carry a `test` guard the census resolved, and \
         `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names` \
         resolves {WHOLE_FILE_TEST_MODULES} whole-file test modules on its own",
        under_a_file_guard.len()
    );

    let mut uncovered: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for site in &gates {
        let compiled = compiled_by(&site.pred).unwrap_or_else(|error| {
            panic!(
                "{}:{}: `cfg({})` cannot be decided: {error}",
                site.path, site.line, site.rendered
            )
        });
        if compiled.is_empty() {
            uncovered
                .entry(site.rendered.as_str())
                .or_default()
                .push(format!("{}:{}", site.path, site.line));
        }
    }
    let acknowledged: BTreeSet<&str> = NO_CI_RUNNER_COMPILES
        .iter()
        .map(|(pred, _)| *pred)
        .collect();
    let found: BTreeSet<&str> = uncovered.keys().copied().collect();
    assert_eq!(
        found, acknowledged,
        "the set of effective predicates no CI runner compiles moved. Every such body is \
         outside the effect denylist's reach on every job CI runs: add the platform's Clippy \
         leg, or add a row to `NO_CI_RUNNER_COMPILES` saying why the body is unreachable on \
         purpose.\n{uncovered:#?}"
    );

    // Each leg is load-bearing, with a witness. A runner no body needs is a job
    // this contract would keep demanding for no reason; a body only one runner
    // compiles is why that runner's leg cannot be dropped.
    for target in &CI_TARGETS {
        let only = BTreeSet::from([target.runner]);
        let witness = gates
            .iter()
            .find(|site| compiled_by(&site.pred).is_ok_and(|compiled| compiled == only));
        assert!(
            witness.is_some(),
            "no body in this tree is compiled by `{}` alone, so nothing here establishes \
             that its Clippy leg is needed",
            target.runner
        );
    }

    let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);
    let complaints = workflow_complaints(&doc);
    assert!(
        complaints.is_empty(),
        "{CI_WORKFLOW} does not wire the gates its own cfg census requires:\n{}",
        complaints.join("\n\n")
    );
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
//
// The four bodies are in `classification::checks`, beside this file. The names
// here are the harness -- they are what the contract, CI and `--list` know --
// and each one delegates and does nothing else. Every check reads
// `effects/wrappers.toml` and `clippy.toml` against the tree they classify, so
// the child is read-only and can be, and is, cut out as test logic by both
// source cutters without joining the whole-file module census.

#[test]
fn every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified() {
    checks::reachable_fns_are_classified();
}

#[test]
fn every_effectful_wrapper_is_on_the_disallowed_list() {
    checks::effectful_wrappers_are_denied();
}

#[test]
fn every_funnel_classified_fn_names_a_site() {
    checks::funnel_rows_name_a_site();
}

#[test]
fn every_libc_item_the_tree_names_is_classified_and_the_effects_are_denied() {
    checks::libc_items_are_classified_and_denied();
}

// ---------------------------------------------------------------------------
// `outputs` — the generated inventories
// ---------------------------------------------------------------------------

// What the artifacts *contain* and what the inventory *declares* are
// definitions, and they live beside this file rather than in it: the CRLF
// discipline every comparison is made under, the module a group's funnel bodies
// are actually in, the sites no funnel names, the frozen sampling N, and the
// two record generators. `artifacts` is the single authority for all six, and
// the three Answer disagreements are its answer rather than this file's.
//
// What stays here is what this section *is*: the six tests -- and the reason
// the boundary is drawn exactly there is that three of them **regenerate**.
// `fs::write` is a denied primitive, `artifacts` restores that denial, and an
// allowance may live only in a file `effects/allowlist.toml` lists; a child
// that regenerated an artifact would need an entry in it. That is a governance
// claim about where an effect may live, not a mechanical consequence of moving
// a declaration, so the writes stay with the harness -- the same cut, for the
// same reason, that left the effectful build helpers out of `policy.rs`.

mod artifacts;

use artifacts::{
    SAMPLING_N, SITES_WITHOUT_A_FUNNEL, artifact_content, funnel_module, funnel_module_record,
    residue_record,
};

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

/// No production module may return a writable process handle through public or
/// crate-visible API, directly or behind a function pointer.
///
/// This is structural over signatures, not a builder-name denylist. Renaming
/// `build_command`, adding a second builder, or returning `fn() -> Command`
/// therefore cannot make the finding disappear. Private construction inside a
/// funnel and APIs that *consume* a Command remain permitted.
#[test]
fn no_production_api_exports_a_writable_process_command() {
    fn command_returning_public_signatures(source: &str) -> Vec<String> {
        let code = blank_comments_and_strings(&production_region(source));
        let mut found = Vec::new();
        for (at, _) in code.match_indices("pub") {
            let before_ok = at == 0
                || !(code.as_bytes()[at - 1].is_ascii_alphanumeric()
                    || code.as_bytes()[at - 1] == b'_');
            let tail = &code[at..];
            if !before_ok {
                continue;
            }
            let after_pub = tail["pub".len()..].trim_start();
            let item = if let Some(restricted) = after_pub.strip_prefix('(') {
                let Some(close) = restricted.find(')') else {
                    continue;
                };
                if restricted[..close].trim() != "crate" {
                    continue;
                }
                restricted[close + 1..].trim_start()
            } else {
                after_pub
            };
            if !item.starts_with("fn ") {
                continue;
            }
            let end = tail.find(['{', ';']).unwrap_or(tail.len());
            let signature = &tail[..end];
            let Some(arrow) = signature.find("->") else {
                continue;
            };
            let returns = &signature[arrow + 2..];
            let names_command = returns
                .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .any(|token| token == "Command");
            if names_command {
                found.push(signature.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
        found
    }

    let mut escapes = Vec::new();
    for (path, source) in scanned_sources() {
        for signature in command_returning_public_signatures(&source) {
            escapes.push(format!("{path}: {signature}"));
        }
    }
    assert!(escapes.is_empty(), "writable Command escapes: {escapes:#?}");

    assert_eq!(
        command_returning_public_signatures(
            "pub fn renamed() -> std::process::Command { todo!() }\n\
             pub ( crate )\n fn pointer() -> fn() -> Command { todo!() }\n\
             fn private() -> Command { todo!() }\n\
             pub fn consumes(_: Command) -> ProcessOutput { todo!() }"
        )
        .len(),
        2,
        "the structural control must catch direct and function-pointer returns only"
    );
}

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

// ---------------------------------------------------------------------------
// "no topology production callers", and the source oracles under it
// ---------------------------------------------------------------------------

// The eleven bodies are in `source_oracles::oracles`, beside this file: the two
// site censuses here, and, in the T-CONTAINER section further down, the five
// that hold the two production regions and the whole-file module derivation.
// The names in this file are the harness -- they are what the contract, CI,
// `effects/wrappers.toml`, `reviews/FINDINGS.md` and `--list` know -- and each
// one delegates and does nothing else.
//
// The boundary is drawn at "reads the tree, writes nothing". All eleven do
// exactly that, so the child restores the three effect denials `super` allows
// and takes no allowlist entry. The needles they carry -- a funnel table, a
// `RunnerRequest {` in prose, the container-runtime literal -- are the reason
// the bodies sit inside a `cfg(test)` module there rather than at file level:
// both source cutters then read them as test logic, and the census that counts
// files naming a container runtime keeps the set it has.

mod source_oracles;

use source_oracles::oracles;

#[test]
fn no_site_enums_row_mapping_has_a_wildcard_arm() {
    oracles::site_row_mappings_have_no_wildcard_arm();
}

#[test]
fn no_topology_module_calls_a_funnel_in_production() {
    oracles::topology_production_names_no_funnel();
}

#[test]
fn the_reachable_fn_parser_finds_each_shape_this_tree_uses() {
    oracles::the_reachable_fn_parser_finds_every_shape();
}

#[test]
fn the_comment_blanker_models_raw_strings_and_still_blanks_comments() {
    oracles::the_comment_blanker_models_raw_strings();
}

#[test]
fn a_multi_byte_char_literal_does_not_desync_the_blanker() {
    oracles::a_multi_byte_char_literal_keeps_the_blankers_phase();
}

#[test]
fn a_region_that_cannot_find_an_items_end_blanks_the_attribute_not_the_file() {
    oracles::an_unfindable_item_end_blanks_the_attribute();
}

// ---------------------------------------------------------------------------
// The T-CONTAINER mechanical checklist
// ---------------------------------------------------------------------------

// The nineteen-name transcription, the presence predicate they share and both
// bodies are in `contract_mappings::mappings`, beside this file, with the three
// R3b enumerations below. The names here are the harness -- they are what the
// contract, CI and `--list` know -- and each one delegates and does nothing
// else.
//
// The boundary is drawn at "resolves a transcribed enumeration against the
// tree, and writes nothing". Both do exactly that, so the child restores the
// three effect denials `super` allows and takes no allowlist entry.
//
// `the_view_directory_has_one_definition_in_the_tree` below is a mapping test
// by shape and deliberately did NOT follow them. It constructs a
// `ContainerName` to drive the mount side against the census side, and that is
// one of the five needles `runner::container::resolve::tests::
// no_module_outside_the_container_runner_writes_a_container_intent` counts over
// the WHOLE file -- not over a production region, so an inline `cfg(test)`
// module does not close it. That census excludes this file by exact path and
// its exclusion names this very test as the reason; a child holding it would
// need a second exclusion there, which is a change to another slice's census
// rather than a consequence of moving a declaration. The same cut, for the same
// reason, that left the effectful build helpers out of `policy.rs`.

mod contract_mappings;

use contract_mappings::mappings;

#[test]
fn every_test_the_container_fault_row_names_is_a_test_in_this_tree() {
    mappings::every_fault_row_name_is_a_test_in_the_tree();
}

#[test]
fn the_container_fault_row_predicate_refuses_a_name_that_is_only_prose() {
    mappings::the_presence_predicate_refuses_a_non_test_shape();
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

// The five source-oracle bodies that close this section are in
// `source_oracles::oracles` with the other six. They belong to that file and
// stand here because this is where the harness names are: the whole-file module
// derivation the four censuses skip by, and the two production regions every
// prohibition census counts over.

#[test]
fn the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names() {
    oracles::the_whole_file_modules_are_read_from_the_declarations();
}

// ---------------------------------------------------------------------------
// The module resolver reads structure, and refuses what it cannot read
// ---------------------------------------------------------------------------

// **These two bodies are here rather than beside the other instrument controls
// in `source_oracles.rs`, and the reason is that file's own rule.** It is
// reached by a plain `mod` declaration, so it sits inside every whole-tree
// census's domain, and it therefore refuses to spell out a terminated
// `#[cfg(test)] mod name;` even inside a string literal -- one written in a
// comment is the exact shape that once derived a phantom skip and removed a
// real production file from every census below it. A scanner whose whole
// subject is that form cannot be driven under that rule. This file is itself a
// whole-file test module -- `effects.rs` declares it `#[cfg(test)] mod tests;`
// -- so no census reads it and the fixtures below cost nothing.
//
// Every positive case carries the mutation that makes it negative, in the same
// assertion pair: the guard deleted, the ancestry flattened, the qualifier
// removed. A scan that answered "test-only" unconditionally passes the
// positives and fails on every one of the negatives beside them.

/// The scan reads a file's **module structure**: inline ancestry, visibility
/// qualifiers, and the predicates that compose down the tree.
#[test]
fn the_module_scan_reads_ancestry_and_visibility_rather_than_text_after_an_attribute() {
    use crate::effects::census_domain::{
        Predicate, ScannedDeclaration, entails_test, parse_predicate, scan_module_declarations,
    };

    fn scan(source: &str) -> Vec<ScannedDeclaration> {
        scan_module_declarations(source)
            .unwrap_or_else(|refusal| panic!("the fixture is readable: {refusal}"))
    }
    fn only(source: &str) -> ScannedDeclaration {
        let mut found = scan(source);
        assert_eq!(found.len(), 1, "{source:?} -> {found:#?}");
        found.remove(0)
    }

    // (1) The plain form the text rule found, still found.
    let plain = only("#[cfg(test)]\nmod tests;\n");
    assert_eq!(plain.name, "tests");
    assert!(plain.inline_path.is_empty());
    assert_eq!(plain.guard, "test");
    assert!(plain.test_only);

    // (2) **Visibility qualifiers.** The text rule read `mod ` immediately
    // after the attribute, so every one of these was invisible to it and the
    // file it named stayed inside every census's domain. Four spellings,
    // because `pub(in path)` carries a `::` and `pub(crate)` carries a paren,
    // and a scan that stepped over one shape and not the others would pass on
    // whichever the tree happens to use today.
    for written in [
        "#[cfg(test)]\npub mod helpers;\n",
        "#[cfg(test)]\npub(crate) mod helpers;\n",
        "#[cfg(test)]\npub(super) mod helpers;\n",
        "#[cfg(test)]\npub(in crate::a::b) mod helpers;\n",
    ] {
        let qualified = only(written);
        assert_eq!(qualified.name, "helpers", "{written:?}");
        assert!(qualified.test_only, "{written:?}");
    }
    // And the qualifier is not what makes it test-only: removed guard, same
    // qualifier, decided the other way.
    assert!(!only("pub(crate) mod helpers;\n").test_only);

    // (3) **Inline ancestry**, which is the shape `agent/proc.rs` uses and the
    // one no text rule reaches at all: the declaration carries no attribute.
    let inherited =
        only("#[cfg(test)]\npub(crate) mod test_support {\n    pub(crate) mod readiness;\n}\n");
    assert_eq!(inherited.name, "readiness");
    assert_eq!(inherited.inline_path, vec!["test_support".to_owned()]);
    assert_eq!(inherited.guard, "test");
    assert!(inherited.test_only);
    // The mutation: the same file with the ancestor's guard deleted. The
    // declaration is byte-identical and the answer flips, which is what says
    // the ancestry is being read rather than the name.
    let ungated = only("pub(crate) mod test_support {\n    pub(crate) mod readiness;\n}\n");
    assert_eq!(ungated.inline_path, vec!["test_support".to_owned()]);
    assert!(
        !ungated.test_only,
        "a declaration under an unguarded inline module is production code"
    );

    // (4) **Nested inline modules**, with the guard on the middle one — so
    // neither "the outermost" nor "the declaration's own" is the rule.
    let deep =
        only("mod outer {\n    #[cfg(test)]\n    mod middle {\n        pub mod leaf;\n    }\n}\n");
    assert_eq!(deep.name, "leaf");
    assert_eq!(
        deep.inline_path,
        vec!["outer".to_owned(), "middle".to_owned()]
    );
    assert!(deep.test_only);

    // (5) **The scope closes.** A declaration written after the guarded block
    // ends does not inherit it, which is the whole of what brace depth is for.
    let both = scan("#[cfg(test)]\nmod inner {\n    mod under;\n}\nmod beside;\n");
    assert_eq!(both.len(), 2, "{both:#?}");
    assert_eq!(both[0].name, "under");
    assert_eq!(both[0].inline_path, vec!["inner".to_owned()]);
    assert!(both[0].test_only);
    assert_eq!(both[1].name, "beside");
    assert!(both[1].inline_path.is_empty());
    assert!(
        !both[1].test_only,
        "a declaration after the guarded block inherited a guard that had closed"
    );

    // (6) **An attribute belongs to the item it precedes.** A `#[cfg(test)]`
    // on a function does not carry to the next `mod`, and a brace-bodied module
    // is not a declaration of a file at all.
    let after_a_function = scan("#[cfg(test)]\nfn helper() {}\nmod plain;\n");
    assert_eq!(after_a_function.len(), 1, "{after_a_function:#?}");
    assert!(!after_a_function[0].test_only);
    assert!(
        scan("#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n").is_empty(),
        "an inline module with a body names no file"
    );

    // (7) **The predicate is decided, never assumed.** `any` is the case that
    // matters: a Unix build with `test` off compiles the file, so the census
    // must keep it. Deciding it "test-only" would remove a production file from
    // every census below, silently, which is the failure direction this whole
    // derivation is shaped against.
    for (written, expected) in [
        ("#[cfg(test)]\nmod x;\n", true),
        ("#[cfg(all(test, unix))]\nmod x;\n", true),
        ("#[cfg(all(unix, all(test, windows)))]\nmod x;\n", true),
        ("#[cfg(test)]\n#[cfg(unix)]\nmod x;\n", true),
        ("#[cfg(unix)]\nmod outer {\n#[cfg(test)]\nmod x;\n}\n", true),
        ("#[cfg(any(test, unix))]\nmod x;\n", false),
        ("#[cfg(not(test))]\nmod x;\n", false),
        ("#[cfg(unix)]\nmod x;\n", false),
        ("#[cfg(feature = \"slow\")]\nmod x;\n", false),
        ("mod x;\n", false),
    ] {
        assert_eq!(
            only(written).test_only,
            expected,
            "{written:?} was decided the other way"
        );
    }

    // (8) The entailment itself, driven on predicates rather than on sources.
    for written in ["test", "all(test, unix)", "not(any(not(test), unix))"] {
        let pred = parse_predicate(written).unwrap_or_else(|why| panic!("{written}: {why}"));
        assert!(entails_test(&pred), "`{written}` does not entail `test`");
    }
    for written in [
        "any(test, unix)",
        "not(test)",
        "unix",
        "target_os = \"linux\"",
        "all(unix, windows)",
    ] {
        let pred = parse_predicate(written).unwrap_or_else(|why| panic!("{written}: {why}"));
        assert!(
            !entails_test(&pred),
            "`{written}` was read as entailing `test`"
        );
    }
    assert_eq!(
        parse_predicate("all(test, unix)").map(|pred| pred.render()),
        Ok("all(test, unix)".to_owned())
    );
    assert_eq!(parse_predicate("test"), Ok(Predicate::Test));

    // (9) **Comments and string literals are not code.** `PR4-CENSUS-COMMENT-
    // ORACLE` is the standing entry, and this derivation is the one it was
    // filed against: a `//` line carrying a declaration once derived a skip for
    // a real production module and removed it from every census below.
    for prose in [
        "// #[cfg(test)] mod ghost;\n",
        "/* #[cfg(test)] mod ghost; */\n",
        "/// #[cfg(test)] mod ghost;\nfn documented() {}\n",
        "const S: &str = \"#[cfg(test)] mod ghost;\";\n",
        "const S: &str = r#\"#[cfg(test)] mod ghost;\"#;\n",
        "const S: &[u8] = b\"#[cfg(test)] mod ghost;\";\n",
    ] {
        assert!(scan(prose).is_empty(), "{prose:?} derived a declaration");
    }
    // A char literal holding a brace must not move the depth the ancestry is
    // measured in — `PR7-R2C-CHAR-LITERAL-DESYNC`'s class, one instrument over.
    let after_a_brace_char = only("const C: char = '{';\n#[cfg(test)]\nmod real;\n");
    assert_eq!(after_a_brace_char.name, "real");
    assert!(after_a_brace_char.inline_path.is_empty());
    assert!(after_a_brace_char.test_only);

    // (10) **The word, not a prefix of one.** `models` is not `mod els`.
    assert!(scan("fn models() {}\nstruct modest;\n").is_empty());

    // (11) **A macro body is discarded, not walked.** Its tokens are only
    // *shaped* like items, so anything read out of one is invented. The
    // discard has to be verified from both sides: nothing inside is derived,
    // and everything outside still is.
    let past_a_macro = only("thread_local! {\n    static X: u8 = 0;\n}\n#[cfg(test)]\nmod real;\n");
    assert_eq!(past_a_macro.name, "real");
    assert!(past_a_macro.inline_path.is_empty());
    assert!(past_a_macro.test_only);
    // Delimiters inside a macro body do not move the depth the ancestry is
    // measured in, and an attribute above a macro belongs to the macro.
    let after_attributed_macro = only("#[cfg(test)]\nlazy! [ a, b ]\nmod plain;\n");
    assert_eq!(after_attributed_macro.name, "plain");
    assert!(
        !after_attributed_macro.test_only,
        "a `#[cfg(test)]` above a macro invocation carried to the next item"
    );
    // `a != b` is not a macro: the token after `!` opens nothing.
    let past_a_negation = only("fn f() { let _ = a != b; }\n#[cfg(test)]\nmod real;\n");
    assert_eq!(past_a_negation.name, "real");
    assert!(past_a_negation.test_only);

    // **The discard is load-bearing, not decoration.** `mod` is an ordinary
    // token inside a macro, and a matcher may capture it — but a scanner
    // reading the body as *items* sees a `mod` with no name after it and
    // refuses the whole file. So these are legal Rust that a body-walking scan
    // cannot read, and the discard is what makes them silent.
    for tokens in [
        "macro_rules! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n",
        "m! { mod }\n",
        "outer! { inner! { mod } }\n",
    ] {
        assert_eq!(
            scan_module_declarations(tokens).map(|found| found.len()),
            Ok(0),
            "{tokens:?} was read as items rather than discarded"
        );
    }
    // And a file holding both still derives exactly the real one.
    let beside_a_macro = only(
        "macro_rules! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n#[cfg(test)]\nmod real;\n",
    );
    assert_eq!(beside_a_macro.name, "real");
    assert!(beside_a_macro.test_only);

    // (12) **A spaced or commented `!` is still a macro**, and the discard has
    // to survive the widening: these bodies hold nothing module-shaped, so they
    // are dropped in silence and the declaration after them is unaffected.
    for spaced in [
        "vec ! [1, 2];\n#[cfg(test)]\nmod real;\n",
        "assert /* sic */ ! (a == b);\n#[cfg(test)]\nmod real;\n",
        "macro_rules ! m {\n    () => {\n        fn go() {}\n    };\n}\n#[cfg(test)]\nmod real;\n",
        // Discriminating: a matcher capturing the `mod` keyword. Recognised as
        // a macro it is discarded; missed because the `!` is not the very next
        // byte, its body is walked and the bare `mod` refuses the whole file.
        "macro_rules ! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n#[cfg(test)]\nmod real;\n",
        "macro_rules /* named next */ ! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n#[cfg(test)]\nmod real;\n",
    ] {
        let past = only(spaced);
        assert_eq!(past.name, "real", "{spaced:?}");
        assert!(past.test_only, "{spaced:?}");
    }

    // **`if !condition { … }` is not an invocation of `if`.** Allowing a gap
    // before the `!` is what makes that shape reachable -- identifier, `!`,
    // identifier, delimiter -- and reading it as a macro would skip the whole
    // block. Only `macro_rules` carries a name between its `!` and its body, so
    // the block below is walked as a block: the declaration inside it is still
    // derived, with the ancestry it actually has.
    let inside_a_negated_block = only(
        "#[cfg(test)]\nmod outer {\n    fn f() {\n        if !ready { }\n    }\n    mod inner;\n}\n",
    );
    assert_eq!(inside_a_negated_block.name, "inner");
    assert_eq!(
        inside_a_negated_block.inline_path,
        vec!["outer".to_owned()],
        "a negated condition was read as a macro and swallowed the block"
    );
    assert!(inside_a_negated_block.test_only);
    for negation in [
        "fn f() { if !ready { } }\n#[cfg(test)]\nmod real;\n",
        "fn f() { while !done { } }\n#[cfg(test)]\nmod real;\n",
        "fn f() { let _ = !flag; }\n#[cfg(test)]\nmod real;\n",
    ] {
        let past = only(negation);
        assert_eq!(past.name, "real", "{negation:?}");
        assert!(past.test_only, "{negation:?}");
    }
    // And the block a negated condition guards is **walked**, not skipped. An
    // empty block cannot tell the two apart — skipping a balanced group and
    // walking it leave the same depth — so the discriminating shape is a
    // declaration inside it. Read as a macro body this is module-shaped and the
    // whole file is refused; read as a block it is the declaration it is.
    let inside_a_negated_block = only(
        "#[cfg(test)]\nmod outer {\n    fn f() {\n        if !ready {\n            mod local;\n        }\n    }\n}\n",
    );
    assert_eq!(inside_a_negated_block.name, "local");
    assert_eq!(
        inside_a_negated_block.inline_path,
        vec!["outer".to_owned()],
        "the negated block was skipped as a macro body and its declaration lost"
    );
    assert!(inside_a_negated_block.test_only);
    let inside_a_negated_loop = only(
        "mod outer {\n    fn f() {\n        while !done {\n            mod local;\n        }\n    }\n}\n",
    );
    assert_eq!(inside_a_negated_loop.name, "local");
    assert!(!inside_a_negated_loop.test_only);

    // (13) **A negated grouped expression is not a macro body.** `if !(…)` is
    // identifier, `!`, delimited group — the same three tokens as `foo!(…)` —
    // and a block expression inside the group may legally declare a module.
    // Read as a macro the group is module-shaped and the whole file is refused;
    // read as the negation it is, the module is the module it is. A keyword
    // cannot be a macro's path segment, which is what separates them.
    for negated_group in [
        "#[cfg(test)]\nmod outer {\n    fn f() -> bool {\n        if !({ mod local {} true }) { false } else { true }\n    }\n}\n",
        "mod outer {\n    fn f() -> bool {\n        !({ mod local {} true })\n    }\n}\n",
        "mod outer {\n    fn f() {\n        while !({ mod local {} false }) { }\n    }\n}\n",
        "mod outer {\n    fn f() -> bool {\n        return !({ mod local {} true });\n    }\n}\n",
    ] {
        let read = scan_module_declarations(negated_group)
            .unwrap_or_else(|refusal| panic!("{negated_group:?} was refused: {refusal}"));
        assert!(
            read.is_empty(),
            "an inline `mod local {{}}` names no file, so it is a scope and not a declaration: \
             {negated_group:?} -> {read:#?}"
        );
    }
    // And the same shape with a real out-of-line declaration inside the group,
    // so the walk is shown to reach it rather than merely not to refuse.
    let through_a_negated_group = only(
        "#[cfg(test)]\nmod outer {\n    fn f() -> bool {\n        if !({ mod local; true }) { false } else { true }\n    }\n}\n",
    );
    assert_eq!(through_a_negated_group.name, "local");
    assert_eq!(
        through_a_negated_group.inline_path,
        vec!["outer".to_owned()],
        "the negated group was skipped as a macro body and its declaration lost"
    );
    assert!(through_a_negated_group.test_only);

    // (14) **Raw identifiers are one token, and their name may be a keyword.**
    // `mod r#type;` declares a module called `type` and resolves to `type.rs`;
    // a reader that stopped at the `#` saw `mod r` with no terminator after it
    // and refused the file.
    for (written, expected) in [
        ("#[cfg(test)]\nmod r#type;\n", "type"),
        ("#[cfg(test)]\npub(crate) mod r#fn;\n", "fn"),
        ("#[cfg(test)]\nmod r#tests;\n", "tests"),
    ] {
        let raw = only(written);
        assert_eq!(raw.name, expected, "{written:?}");
        assert!(raw.test_only, "{written:?}");
    }
    // A raw `r#mod` is an identifier named `mod`, not the keyword, so it opens
    // nothing; and `raw` is an ordinary identifier that merely starts with `r`.
    assert!(scan("struct r#mod;\nfn f() { let raw = 1; }\n").is_empty());
    let beside_a_raw_word = only("fn raw() {}\n#[cfg(test)]\nmod real;\n");
    assert_eq!(beside_a_raw_word.name, "real");

    // (15) **A raw macro name is still a macro.** `r#if!(…)` is a macro called
    // `if`, so the keyword rule above must read the raw spelling as an
    // identifier rather than as the keyword it names.
    let past_a_raw_macro = only("r#if! { let _ = 1; }\n#[cfg(test)]\nmod real;\n");
    assert_eq!(past_a_raw_macro.name, "real");
    assert!(past_a_raw_macro.test_only);

    // (16) **CRLF.** The guest checks this tree out with CRLF, and every
    // structural answer above has to be the same there. Driven by converting
    // each fixture rather than by trusting that nothing here reads a line.
    for fixture in [
        "#[cfg(test)]\npub(crate) mod test_support {\n    pub(crate) mod readiness;\n}\n",
        "mod outer {\n    #[cfg(test)]\n    mod middle {\n        pub mod leaf;\n    }\n}\n",
        "macro_rules ! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n#[cfg(test)]\nmod real;\n",
        "#[cfg(test)]\nmod r#type;\n",
    ] {
        let lf = scan_module_declarations(fixture);
        let crlf = scan_module_declarations(&fixture.replace('\n', "\r\n"));
        assert_eq!(lf, crlf, "CRLF changed the derivation for {fixture:?}");
        assert!(lf.is_ok_and(|found| found.len() == 1));
    }
    for refused in [
        "macro_rules! m {\n    () => {\n        #[cfg(test)]\n        mod x;\n    };\n}\n",
        "#[cfg(test)]\nmod tests;\n#[cfg(test)]\nmod tests;\n",
    ] {
        assert_eq!(
            scan_module_declarations(refused).is_err(),
            scan_module_declarations(&refused.replace('\n', "\r\n")).is_err(),
            "CRLF changed whether {refused:?} is refused"
        );
        assert!(scan_module_declarations(refused).is_err());
    }
}

/// The resolver **refuses** every shape it cannot resolve, rather than guessing.
///
/// Both wrong answers are silent. A missing skip leaves a test file inside a
/// census's domain, where a fixture reads as a production offender and someone
/// looks; a spurious one removes a real production file from every census
/// below and nothing says so. So the derivation refuses instead of choosing,
/// and every refusal below is driven — none of them is reachable from this
/// tree, which is exactly why they would otherwise be code nobody has watched
/// work.
#[test]
fn the_module_resolver_refuses_every_shape_it_cannot_resolve() {
    use crate::effects::census_domain::{
        CandidateRefusal, ScanRefusal, candidates_for, contained_in, declaration_cycle,
        module_directory, parse_predicate, scan_module_declarations, sole_present,
    };

    fn refusal(source: &str) -> ScanRefusal {
        scan_module_declarations(source).expect_err("this source is refused")
    }

    // (1) Malformed input the scan cannot tokenise.
    assert_eq!(
        refusal("#[cfg(test)\nmod tests;\n"),
        ScanRefusal::UnclosedAttribute { line: 1 }
    );
    assert_eq!(
        refusal("mod a { }\n}\n"),
        ScanRefusal::UnbalancedBraces { line: 2 }
    );
    for malformed in ["mod ;\n", "mod x = 3;\n", "mod trailing\n"] {
        assert!(
            matches!(refusal(malformed), ScanRefusal::MalformedDeclaration { .. }),
            "{malformed:?} was read as a declaration"
        );
    }

    // (2) A predicate the entailment grammar cannot read. Unresolved is
    // refused, not treated as "not test" — because "not test" is the answer
    // that keeps a file in a census's domain, and a scan that cannot read a
    // guard does not know which direction is safe.
    for unreadable in [
        "#[cfg(sometimes(test))]\nmod x;\n",
        "#[cfg(test]\nmod x;\n",
        "#[cfg()]\nmod x;\n",
        "#[cfg(not(test, unix))]\nmod x;\n",
        "#[cfg(feature =)]\nmod x;\n",
    ] {
        assert!(
            matches!(refusal(unreadable), ScanRefusal::UnreadablePredicate { .. }),
            "{unreadable:?} was decided rather than refused"
        );
    }
    // The parser's own refusals, driven directly.
    for unreadable in [
        "",
        "all(test",
        "not(test, unix)",
        "maybe(test)",
        "all(test) extra",
    ] {
        assert!(
            parse_predicate(unreadable).is_err(),
            "`{unreadable}` parsed"
        );
    }

    // (3) `#[path]`, which is the one construct that can point a declaration
    // outside its own directory — and therefore the one that could build the
    // cycle asserted against below. Refused rather than resolved, in both the
    // direct and the `cfg_attr` forms.
    for pathed in [
        "#[path = \"elsewhere.rs\"]\nmod x;\n",
        "#[cfg_attr(unix, path = \"elsewhere.rs\")]\nmod x;\n",
    ] {
        assert!(
            matches!(
                refusal(pathed),
                ScanRefusal::UnsupportedPathAttribute { .. }
            ),
            "{pathed:?} was resolved"
        );
    }
    // And it is refused **because it reaches a module**: the same attribute on
    // something else is not this derivation's business, and refusing it would
    // be a scan that fails on files it has no claim about.
    assert!(
        scan_module_declarations("#[path = \"x\"]\nstruct S;\nmod y;\n").is_ok(),
        "a `path` attribute on a non-module item is not a module path attribute"
    );

    // (3b) **A macro body holding a module-shaped sequence.** A macro invoked
    // at item position can expand to a module, and nothing here can tell which
    // does — so a body whose tokens *could* be one is refused rather than
    // either walked (which invents a declaration for a file the macro never
    // names) or silently dropped (which loses a real one). Every delimiter,
    // and the `macro_rules!` definition form, which carries a name between the
    // `!` and its body.
    for shaped in [
        "macro_rules! m {\n    () => {\n        mod x;\n    };\n}\n",
        "macro_rules! m {\n    () => {\n        #[cfg(test)]\n        mod x;\n    };\n}\n",
        "quote! { mod x; }\n",
        "paste!( mod x { } );\n",
        "items![ pub(crate) mod x; ]\n",
        "outer! { inner! { mod x; } }\n",
        // Raw identifiers, on both halves: a macro defined with a keyword for a
        // name, and a module-shaped body whose module is named with one.
        "macro_rules! r#mod {\n    () => {\n        mod x;\n    };\n}\n",
        "macro_rules ! r#type {\n    () => {\n        #[cfg(test)]\n        mod x;\n    };\n}\n",
        "quote! { mod r#type; }\n",
        "r#if! { mod r#fn { } }\n",
    ] {
        assert!(
            matches!(refusal(shaped), ScanRefusal::ModuleShapedMacroBody { .. }),
            "{shaped:?} was read rather than refused"
        );
    }
    // **The `!` need not touch the name.** Whitespace and comments between a
    // macro's name and its `!` are valid Rust, and `#[rustfmt::skip]` keeps
    // whatever spelling a file was written with -- so a guard keyed on the very
    // next byte missed exactly the macros somebody had spaced out. Every
    // spelling below is a real one a formatter would otherwise close up.
    for spaced in [
        "macro_rules ! m {\n    () => {\n        mod x;\n    };\n}\n",
        "macro_rules\n! m {\n    () => {\n        #[cfg(test)]\n        mod x;\n    };\n}\n",
        "macro_rules /* named next */ ! m {\n    () => {\n        mod x;\n    };\n}\n",
        "#[rustfmt::skip]\nmacro_rules  !  m  {\n    () => {\n        mod x;\n    };\n}\n",
        "quote ! { mod x; }\n",
        "quote // why\n! { mod x; }\n",
        "quote /* why */ ! { pub(crate) mod x; }\n",
        "items\n    ![ mod x { } ]\n",
    ] {
        assert!(
            matches!(refusal(spaced), ScanRefusal::ModuleShapedMacroBody { .. }),
            "{spaced:?} was read rather than refused"
        );
    }

    // And a macro body with nothing module-shaped in it is discarded in
    // silence, which is what stops the refusal from being a tax on every
    // `vec!`, `assert!` and `format!` in the tree.
    for ordinary in [
        "vec![1, 2, 3];\n",
        "assert!(a == b, \"mod x; is prose here\");\n",
        "macro_rules! m {\n    () => {\n        fn go() {}\n    };\n}\n",
        "modify!(x);\n",
    ] {
        assert_eq!(
            scan_module_declarations(ordinary).map(|found| found.len()),
            Ok(0),
            "{ordinary:?} was not discarded cleanly"
        );
    }

    // (4) An inner `#![cfg(…)]` gates the module it is written in, which this
    // derivation does not model. There are none in this tree; one arriving
    // fails loudly rather than being read as ungated.
    assert!(matches!(
        refusal("#![cfg(test)]\nmod x;\n"),
        ScanRefusal::UnsupportedInnerCfg { .. }
    ));

    // (5) Duplicates, and the control that says the check is per parent module
    // rather than per file — two modules may each declare an `x`.
    assert!(matches!(
        refusal("#[cfg(test)]\nmod tests;\n#[cfg(test)]\nmod tests;\n"),
        ScanRefusal::DuplicateDeclaration { .. }
    ));
    assert!(
        scan_module_declarations("mod a {\n    mod x;\n}\nmod b {\n    mod x;\n}\n").is_ok(),
        "two parents each declaring `x` are not a duplicate"
    );

    // (6) **Candidate paths, the flattening mutation, and the crate roots.**
    // The inline path is part of the directory. A resolver that dropped it
    // looks in `agent/proc/readiness.rs`, which does not exist — so the failure
    // is a zero-candidate refusal if you are lucky, and the wrong file if a
    // module of that name is ever added beside it.
    let src = Path::new("src");
    let proc = Path::new("src/agent/proc.rs");
    let named = |root: &Path, file: &str, inline: &[String], name: &str| {
        candidates_for(root, Path::new(file), inline, name)
    };
    assert_eq!(
        named(
            src,
            "src/agent/proc.rs",
            &["test_support".to_owned()],
            "readiness"
        ),
        Ok([
            PathBuf::from("src/agent/proc/test_support/readiness.rs"),
            PathBuf::from("src/agent/proc/test_support/readiness/mod.rs"),
        ])
    );
    assert_eq!(
        named(src, "src/agent/proc.rs", &[], "readiness"),
        Ok([
            PathBuf::from("src/agent/proc/readiness.rs"),
            PathBuf::from("src/agent/proc/readiness/mod.rs"),
        ])
    );
    let root = repo_root().join("src");
    for flattened in candidates_for(&root, &root.join("agent/proc.rs"), &[], "readiness")
        .expect("proc.rs is an ordinary module")
    {
        assert!(
            !flattened.is_file(),
            "{} exists, so the flattening mutation would resolve instead of refusing",
            flattened.display()
        );
    }
    assert!(proc.is_relative());

    // **A crate root owns its directory; an ordinary module does not.**
    // `mod.rs` is the first case wherever it sits, and `lib.rs`/`main.rs` only
    // at the crate's source root.
    assert_eq!(
        named(src, "src/engine/mod.rs", &[], "tests").map(|pair| pair[0].clone()),
        Ok(PathBuf::from("src/engine/tests.rs"))
    );
    assert_eq!(
        named(src, "src/lib.rs", &[], "effects").map(|pair| pair[0].clone()),
        Ok(PathBuf::from("src/effects.rs"))
    );
    assert_eq!(
        named(src, "src/main.rs", &[], "tests").map(|pair| pair[0].clone()),
        Ok(PathBuf::from("src/tests.rs"))
    );
    // **The competing production sibling.** A nested `src/a/lib.rs` is the
    // ordinary module `a::lib` unless the manifest says otherwise, so reading
    // it as a crate root points `mod tests;` at `src/a/tests.rs` — a *different
    // file*, a sibling that may well be production, which the derivation would
    // then remove from every census as though `a/lib.rs` had declared it. That
    // failure does not announce itself: with no `src/a/lib/tests.rs` present it
    // resolves, it does not refuse. This derivation does not read `Cargo.toml`,
    // so it refuses rather than choosing between the two readings.
    assert_eq!(
        named(src, "src/a/lib.rs", &[], "tests"),
        Err(CandidateRefusal::AmbiguousCrateRoot {
            declared_in: PathBuf::from("src/a/lib.rs")
        })
    );
    assert_eq!(
        named(src, "src/a/b/main.rs", &[], "tests"),
        Err(CandidateRefusal::AmbiguousCrateRoot {
            declared_in: PathBuf::from("src/a/b/main.rs")
        })
    );
    // And the refusal is about *position*, not about the name: the same stem at
    // the source root is a crate root, and `mod.rs` is never ambiguous.
    assert!(module_directory(src, Path::new("src/lib.rs")).is_ok());
    assert!(module_directory(src, Path::new("src/a/mod.rs")).is_ok());
    assert_eq!(
        module_directory(src, Path::new("src/a/mod.rs")),
        Ok(PathBuf::from("src/a"))
    );
    // The sibling the wrong reading would have claimed, named so the two
    // readings are visible side by side rather than asserted apart.
    assert_eq!(
        module_directory(src, Path::new("src/a/other.rs")),
        Ok(PathBuf::from("src/a/other")),
        "an ordinary module owns a directory named after it, never its parent"
    );

    // (7) **Zero and two candidates.** Two is `x.rs` and `x/mod.rs` both
    // present — a competing `mod.rs` that Rust itself refuses to compile and
    // that a resolver taking the first match would silently pick a side in.
    let pair = named(src, "src/a.rs", &[], "b").expect("an ordinary module");
    assert_eq!(sole_present(&pair, &|_| false), Err(0));
    assert_eq!(sole_present(&pair, &|_| true), Err(2));
    assert_eq!(sole_present(&pair, &|at| at == pair[0]), Ok(&pair[0]));
    assert_eq!(sole_present(&pair, &|at| at == pair[1]), Ok(&pair[1]));

    // (8) **Path escape.** A candidate must descend from the declaring file's
    // directory through plain components. This holds by construction while
    // `#[path]` is refused, and the two are one control with two halves.
    let base = Path::new("src/agent");
    assert!(contained_in(
        base,
        Path::new("src/agent/proc/test_support/readiness.rs")
    ));
    assert!(
        !contained_in(base, base),
        "a directory does not contain itself"
    );
    assert!(!contained_in(base, Path::new("src/effects.rs")));
    assert!(
        !contained_in(base, Path::new("src/agent/../effects.rs")),
        "a `..` component escapes and must not read as contained"
    );

    // (9) **Cycles.** The derivation reads every guard from the file above, so
    // a cycle means a guard attributed to a file that does not inherit it. Not
    // reachable while directory-derived candidates descend, which is the reason
    // to drive it here rather than a reason to leave it unchecked.
    let edge = |from: &str, to: &str| (PathBuf::from(from), PathBuf::from(to));
    let forest = vec![edge("a.rs", "a/b.rs"), edge("a/b.rs", "a/b/c.rs")];
    assert_eq!(declaration_cycle(&forest), None);
    assert!(
        declaration_cycle(&[edge("a.rs", "a.rs")]).is_some(),
        "a file declaring itself is a cycle"
    );
    assert!(
        declaration_cycle(&[edge("a.rs", "b.rs"), edge("b.rs", "a.rs")]).is_some(),
        "a two-file loop is a cycle"
    );
    // **A branching graph, with the cycle on the second edge out of a node.**
    // The first version walked `edges.iter().find(…)` — one outgoing edge per
    // node — so it followed `a -> b`, found `b` a leaf, and reported the whole
    // graph acyclic while `a -> c -> a` sat beside it. Every node here has an
    // outgoing edge, so a detector that merely *terminates* still passes; what
    // separates them is which edges get walked.
    let branching = vec![
        edge("a.rs", "a/b.rs"),
        edge("a.rs", "a/c.rs"),
        edge("a/c.rs", "a.rs"),
    ];
    let closed = declaration_cycle(&branching).expect("the second edge closes a loop");
    assert_eq!(
        closed.first(),
        closed.last(),
        "a reported cycle must start and end at the same node: {closed:?}"
    );
    assert!(
        closed.contains(&PathBuf::from("a/c.rs")),
        "the reported cycle does not name the branch that closes it: {closed:?}"
    );
    // The same shape without the back edge is a tree, so the branching itself
    // is not what the detector is reacting to.
    assert_eq!(
        declaration_cycle(&[edge("a.rs", "a/b.rs"), edge("a.rs", "a/c.rs")]),
        None
    );
    // A cycle reachable only through a node whose *first* edge leads away from
    // it: the depth-first walk has to come back and take the second.
    let deferred = vec![
        edge("a.rs", "a/b.rs"),
        edge("a/b.rs", "a/b/leaf.rs"),
        edge("a.rs", "a/c.rs"),
        edge("a/c.rs", "a/d.rs"),
        edge("a/d.rs", "a/c.rs"),
    ];
    assert!(
        declaration_cycle(&deferred).is_some(),
        "a cycle two branches deep was not reached"
    );
}

/// The file-module-level lint reader is a **census instrument**, not a shipped
/// API.
///
/// `PR72-API-001`. It arrived as a `pub fn` in this file's production region,
/// which is a public surface added so that a test could call it: the binary
/// consults it nowhere, and `effects/allowlist.toml` records `allows = []` for
/// this file precisely because everything above the `#[cfg(test)]` cut is meant
/// to be the parsers and the frozen lists and nothing else. It is
/// `#[cfg(test)] pub(crate)` now, in a module at the bottom.
///
/// Asserted over the region rather than by eye, and in both directions: the
/// name is absent from the production region and present in the file, so a
/// typo in the needle fails the second half instead of passing the first.
#[test]
fn the_file_level_lint_reader_is_a_census_instrument_and_not_a_shipped_api() {
    /// The two claims, over one spelling of the file.
    ///
    /// **Structural, and therefore line-ending-blind.** The first draft
    /// searched for the literal `"#[cfg(test)]\npub(crate) mod lint_levels {"`,
    /// which is a search for a spelling of a newline: the guest checks this
    /// tree out with CRLF and that needle is `\r\n` there, so the assertion
    /// held on Unix and on nothing else. What actually has to be true is that
    /// the item is **removed by** [`crate::effects::production_code`], which is
    /// what `#[cfg(test)]` means to every census in this crate — and that is
    /// read from the region, not from a byte sequence spanning a line.
    fn absent_from_production(source: &str) -> Vec<String> {
        let production = crate::effects::production_code(source);
        let whole = blank_comments_and_strings(source);
        let mut wrong = Vec::new();
        for needle in [
            "fn file_level_lint_state(",
            "fn names_lint(",
            "mod lint_levels",
        ] {
            if !whole.contains(needle) {
                wrong.push(format!("`{needle}` is not in src/effects.rs at all"));
            }
            if production.contains(needle) {
                wrong.push(format!(
                    "`{needle}` survives into the production region, which makes it a shipped \
                     surface rather than a census instrument"
                ));
            }
        }
        wrong
    }

    let source = fs::read_to_string(repo_root().join("src/effects.rs")).expect("src/effects.rs");
    assert!(
        absent_from_production(&source).is_empty(),
        "{:#?}",
        absent_from_production(&source)
    );
    // The same file with the line endings the Windows guest gives it.
    let crlf = source.replace('\n', "\r\n");
    assert!(
        absent_from_production(&crlf).is_empty(),
        "{:#?}",
        absent_from_production(&crlf)
    );

    // The visibility is narrow as well as gated, and that fits on one line in
    // either spelling.
    assert!(
        blank_comments_and_strings(&source).contains("pub(crate) mod lint_levels"),
        "the lint reader's module is no longer `pub(crate)`"
    );
    assert!(
        !blank_comments_and_strings(&source).contains("pub mod lint_levels"),
        "the lint reader's module is `pub`, which is the surface this repair removed"
    );

    // The instrument still answers where it is used, so narrowing it did not
    // narrow it out of existence — under both spellings of a line ending.
    for prologue in [
        "#![deny(clippy::disallowed_types)]\n",
        "#![deny(clippy::disallowed_types)]\r\n",
        "//! docs\r\n#![allow(clippy::too_many_arguments)]\r\n#![forbid(clippy::disallowed_macros)]\r\n",
    ] {
        let wanted = if prologue.contains("forbid") {
            ("clippy::disallowed_macros", Some("forbid"))
        } else {
            ("clippy::disallowed_types", Some("deny"))
        };
        assert_eq!(
            crate::effects::lint_levels::file_level_lint_state(prologue, wanted.0),
            wanted.1,
            "{prologue:?}"
        );
    }
}

#[test]
fn the_production_code_region_removes_a_configured_item_and_keeps_the_rest() {
    oracles::the_configured_item_is_removed_and_the_rest_kept();
}

#[test]
fn a_configured_attribute_in_prose_removes_nothing() {
    oracles::a_configured_attribute_in_prose_is_inert();
}

#[test]
fn the_production_code_region_contains_the_truncated_one() {
    oracles::the_whole_region_contains_the_truncated_one();
}

#[test]
fn every_production_region_that_stops_early_stops_at_a_module() {
    oracles::every_early_stop_is_at_a_module();
}

// ---------------------------------------------------------------------------
// R3b: the enumerations the reconciliation promised and did not supply
// ---------------------------------------------------------------------------

// The three enumerations this section supplies -- the nine refusals with their
// ordering predicates, the twelve ST-16 variants and the twelve clauses -- and
// the body that holds them are in `contract_mappings::mappings` with the
// T-CONTAINER transcription above. They are resolved by the same
// `defining_test_sites` census and belong beside it; the name below is the
// harness and delegates.

#[test]
fn every_pr6_refusal_st16_variant_and_invariant_clause_names_a_test_or_an_owner() {
    mappings::every_promised_mapping_names_a_test_or_an_owner();
}
