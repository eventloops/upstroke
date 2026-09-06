//! Extended notes: `docs/internals/effects/tests.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::census_domain::{CrateRoots, InventoryRefusal};
use super::{
    ALLOWLIST_TOML, CLIPPY_TOML, DENIAL_CONTROL, DENIAL_FIXTURES, EFFECT_SITES_JSON,
    FROZEN_LEGACY_ALLOWLIST, FUNNEL_MODULES_JSON, REGENERATE, RESIDUE_CLASSES_JSON,
    TOPOLOGY_MODULES, USED_GOVERNED_LINTS, WRAPPERS_TOML, blank_comments,
    blank_comments_and_strings, governed_allows, legacy_growth, normalize_lint, production_region,
    topology_modules_among,
};
use crate::topology::effects::{EffectSiteId, effect_sites, effect_sites_json};

mod policy;

use policy::{PACKET_PRIMITIVES, PACKET_TYPES, host_conditional_paths, marker_before};

mod classification;

use classification::checks;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub(in crate::effects) fn crate_roots() -> &'static CrateRoots {
    static ROOTS: std::sync::OnceLock<CrateRoots> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| crate_roots_of(&repo_root()).unwrap_or_else(|refusal| panic!("{refusal}")))
}

pub(in crate::effects) fn crate_roots_of(
    manifest_dir: &Path,
) -> Result<CrateRoots, InventoryRefusal> {
    let manifest = manifest_dir.join("Cargo.toml");
    CrateRoots::from_metadata_json(&cargo_metadata_json(&manifest)?, &manifest)
}

fn cargo_metadata_json(manifest: &Path) -> Result<String, InventoryRefusal> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .map_err(|error| InventoryRefusal::NotRun {
            manifest: manifest.to_path_buf(),
            why: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(InventoryRefusal::Failed {
            manifest: manifest.to_path_buf(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8(output.stdout).map_err(|error| InventoryRefusal::Unreadable {
        manifest: manifest.to_path_buf(),
        why: error.to_string(),
    })
}

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
    expect_sites: usize,
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
    #[serde(default, rename = "allow-expect-in-tests")]
    allow_expect_in_tests: bool,
    #[serde(default, rename = "allow-panic-in-tests")]
    allow_panic_in_tests: bool,
    #[serde(default, rename = "allow-print-in-tests")]
    allow_print_in_tests: bool,
}

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

#[test]
fn the_readiness_expectations_are_per_site_and_both_records_say_so() {
    const READINESS: &str = "src/agent/proc/test_support/readiness.rs";
    const NOTES: &str = "docs/internals/agent/proc/test_support/readiness.md";
    const LINT: &str = "clippy::disallowed_methods";
    const SITES: usize = 6;
    const DECISION: &str = "standards/02_standards_automated_baseline.md";
    const SPELLED: [&str; 8] = [
        "one", "two", "three", "four", "five", "six", "seven", "eight",
    ];
    let sites_in_words = SPELLED[SITES - 1];

    let source = fs::read_to_string(repo_root().join(READINESS)).expect("the readiness module");

    for lint in USED_GOVERNED_LINTS {
        assert_eq!(
            crate::effects::lint_levels::file_level_lint_state(&source, lint),
            Some("deny"),
            "{READINESS} must deny `{lint}` at file-module level"
        );
    }

    let found = governed_allows(&source);
    let per_site: Vec<&crate::effects::GovernedAllow> =
        found.iter().filter(|allow| !allow.module_level).collect();
    assert_eq!(
        per_site.len(),
        SITES,
        "{READINESS} carries {} per-site governed attributes: {per_site:#?}",
        per_site.len()
    );
    assert!(
        found.len() == SITES,
        "a governed attribute at module level is a file-scope allowance and this file has \
         none: {found:#?}"
    );
    for allow in &per_site {
        assert_eq!(allow.keywords, ["expect"], "{READINESS}:{}", allow.line);
        assert_eq!(allow.written, [LINT], "{READINESS}:{}", allow.line);
        assert!(allow.reasoned, "{READINESS}:{} has no reason", allow.line);
    }
    let indices: BTreeSet<usize> = (1..=SITES)
        .filter(|index| source.contains(&format!("site {index} of {SITES}")))
        .collect();
    assert_eq!(
        indices,
        (1..=SITES).collect::<BTreeSet<usize>>(),
        "each expectation's reason names which of the {SITES} sites it is"
    );

    let list = allowlist();
    let row = list
        .funnel
        .iter()
        .find(|entry| entry.path == READINESS)
        .expect("the readiness row is in the funnel section");
    assert_eq!(row.allows, vec![LINT.to_owned()]);
    assert_eq!(row.expect_sites, SITES);

    let phrase = format!("five distinct denied paths across {sites_in_words} sites");
    let shouted = phrase.to_uppercase();
    let allowlist_text =
        fs::read_to_string(repo_root().join(ALLOWLIST_TOML)).expect("the allowlist");
    let notes = fs::read_to_string(repo_root().join(NOTES)).expect("the readiness notes");
    for (record, text, needle) in [
        (NOTES, notes.as_str(), phrase.as_str()),
        (ALLOWLIST_TOML, allowlist_text.as_str(), shouted.as_str()),
    ] {
        for spelling in [text.to_owned(), text.replace('\n', "\r\n")] {
            assert!(
                spelling.lines().any(|line| line.contains(needle)),
                "{record} no longer states `{needle}` on a line of its own"
            );
        }
        assert!(
            text.contains(DECISION),
            "{record} does not cite `{DECISION}`, which is what admits the placement"
        );
    }

    assert!(
        repo_root().join(DECISION).is_file(),
        "`{DECISION}` is cited by both records and is not in the tree"
    );
}

#[test]
fn the_internals_readme_names_the_records_that_carry_the_readiness_statement() {
    const README: &str = "docs/internals/README.md";
    const NOTES: &str = "docs/internals/agent/proc/test_support/readiness.md";
    const READINESS: &str = "src/agent/proc/test_support/readiness.rs";
    const SECTION: &str = "\n## What moves\n";

    let readme = fs::read_to_string(repo_root().join(README))
        .expect("the internals README")
        .replace("\r\n", "\n");
    let (_, below) = readme
        .split_once(SECTION)
        .unwrap_or_else(|| panic!("{README} no longer has a `What moves` section"));
    let what_moves = below
        .split_once("\n## ")
        .map_or(below, |(section, _)| section);

    for record in [NOTES, ALLOWLIST_TOML] {
        assert!(
            what_moves.contains(record),
            "{README}'s `What moves` section does not name `{record}`, which is one of the two \
             records `the_readiness_expectations_are_per_site_and_both_records_say_so` reads the \
             per-site allowance statement from"
        );
    }
    assert!(
        !what_moves.contains(READINESS),
        "{README}'s `What moves` section names `{READINESS}` as prose a census reads. The \
         statement moved to `{NOTES}` and `{ALLOWLIST_TOML}`; the module keeps its marker and \
         nothing else, so a maintainer sent to the source finds no such sentence"
    );
}

fn file_level_denies(source: &str, lint: &str) -> bool {
    matches!(
        crate::effects::lint_levels::file_level_lint_state(source, lint),
        Some("deny" | "forbid")
    )
}

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
        let mut per_site = 0;
        for allow in &found {
            if !allow.module_level
                && allow.keywords == ["expect"]
                && entry.expect_sites > 0
                && allow.reasoned
                && allow
                    .lints
                    .iter()
                    .all(|lint| file_level_denies(&source, lint))
            {
                per_site += 1;
                continue;
            }
            assert!(
                allow.module_level,
                "{path}:{} allows {:?} below module level; `mechanism` (2) permits it \
                 \"only as module-level attributes\", and the per-site `#[expect]` the \
                 2026-08-30 amendment admits needs a reason, a file-level deny of the same \
                 lint, and an `expect_sites` count in {ALLOWLIST_TOML}",
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
        assert_eq!(
            per_site, entry.expect_sites,
            "{path} carries {per_site} per-site `#[expect]` attributes and {ALLOWLIST_TOML} \
             records {}",
            entry.expect_sites
        );
    }

    for (path, (entry, _)) in &recorded {
        assert!(
            entry.expect_sites == 0 || carried.contains(*path),
            "{path} records {} per-site expectations and carries no governed attribute",
            entry.expect_sites
        );
    }

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

#[test]
fn the_placement_scan_refuses_an_allow_that_is_not_module_level_and_sees_through_no_disguise() {
    let on_a_function = "#[allow(clippy::disallowed_methods)]\nfn go() {}\n";
    let found = governed_allows(on_a_function);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(!found[0].module_level);

    let on_a_statement = "fn go() {\n    #[allow(clippy::disallowed_methods)]\n    let _ = 1;\n}\n";
    let found = governed_allows(on_a_statement);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(!found[0].module_level);

    let on_a_module = "#[allow(clippy::disallowed_methods)]\nmod inner { }\n";
    let found = governed_allows(on_a_module);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].module_level);

    let inner = "//! doc\n#![allow(clippy::disallowed_types)]\nfn go() {}\n";
    let found = governed_allows(inner);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].inner && found[0].module_level);

    let late = "fn go() {}\n#![allow(clippy::disallowed_types)]\n";
    let found = governed_allows(late);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(!found[0].module_level);

    let expected = "#![expect(clippy::disallowed_macros)]\n";
    assert_eq!(governed_allows(expected).len(), 1);

    assert!(governed_allows("#![allow(clippy::too_many_arguments)]\n").is_empty());
    assert!(governed_allows("#![allow(unused_variables)]\n").is_empty());

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
    let blanked = blank_comments_and_strings(disguised);
    assert_eq!(blanked.len(), disguised.len(), "offsets are preserved");
    assert_ne!(blanked, disguised, "the blanking is a no-op");
    assert!(!blanked.contains("disallowed_methods"));

    let mixed = format!("{disguised}#![allow(clippy::disallowed_macros)]\n");
    let found = governed_allows(&mixed);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].lints, vec!["disallowed_macros".to_owned()]);

    let mechanisms = 9;
    assert_eq!(mechanisms, 9);
}

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

    for probe in [
        "#![allow(warnings)]\n",
        "#![allow(clippy::all)]\n",
        "#![allow(clippy::style)]\n",
    ] {
        assert_eq!(governed_allows(probe).len(), 1, "{probe}");
    }

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

#[test]
fn cargo_toml_declares_no_lint_table_that_could_allow_a_governed_lint() {
    let text = fs::read_to_string(repo_root().join("Cargo.toml")).expect("Cargo.toml");
    let manifest: toml::Value = toml::from_str(&text).expect("Cargo.toml parses");
    let Some(lints) = manifest.get("lints") else {
        return;
    };
    let rendered = lints.to_string();
    for lint in super::GOVERNED_LINTS {
        assert!(
            !rendered.contains(lint),
            "Cargo.toml [lints] names the governed lint `{lint}`: {rendered}"
        );
    }
}

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

    let grown: Vec<&str> = current.iter().copied().chain(["src/catalog.rs"]).collect();
    assert_eq!(
        legacy_growth(FROZEN_LEGACY_ALLOWLIST, &grown),
        vec!["src/catalog.rs"]
    );
    let shrunk: Vec<&str> = current.iter().copied().skip(1).collect();
    assert!(legacy_growth(FROZEN_LEGACY_ALLOWLIST, &shrunk).is_empty());

    let frozen: BTreeSet<&str> = FROZEN_LEGACY_ALLOWLIST.iter().copied().collect();
    let listed: BTreeSet<&str> = current.iter().copied().collect();
    assert_eq!(frozen, listed);
}

#[test]
fn the_legacy_section_never_contains_a_topology_module() {
    let list = allowlist();
    let current: Vec<&str> = list.legacy.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        topology_modules_among(&current),
        Vec::<&str>::new(),
        "a topology module is in the frozen legacy section"
    );

    let probes = [
        "src/topology/registry.rs",
        "src/runner/mod.rs",
        "src/workspace_manager.rs",
        "src/workspace_manager/residue.rs",
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

    let before_the_split = [
        "src/topology/",
        "src/runner/",
        "src/workspace_manager.rs",
        "src/engine/topology.rs",
        "src/engine/topology/",
    ];
    let child = "src/workspace_manager/residue.rs";
    assert!(
        !child.starts_with("src/workspace_manager.rs"),
        "the prefix relation this entry exists for no longer holds"
    );
    assert!(
        !before_the_split
            .iter()
            .any(|banned| child.starts_with(banned) || *banned == child),
        "the shapes that predate the `m4-workspace` split already cover \
         `{child}`, so the `src/workspace_manager/` entry is dead weight and \
         should be removed"
    );

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
    assert_eq!(absent, Vec::<&str>::new(), "the absent set moved");
    assert!(
        repo_root().join("src/runner/container.rs").is_file(),
        "the Container funnel is the entry that used to be absent; if it is gone \
         again, this assertion is the one that says so rather than an empty set \
         reading as agreement"
    );
}

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

    assert!(!denied.disallowed_methods.is_empty());
    assert!(!denied.disallowed_types.is_empty());
    assert!(
        !denied.disallowed_macros.is_empty(),
        "the macro list is the one that can be vacuous without looking it"
    );

    for entry in denied.all() {
        assert!(
            entry.reason.starts_with("UPSTROKE-EFFECT")
                || entry.reason.starts_with("UPSTROKE-WRAPPER"),
            "{} has no classified reason: {}",
            entry.path,
            entry.reason
        );
    }

    const NAMES_A_CONTAINER_RUNTIME: &[(&str, &str)] = &[
        (
            "src/effects/tests.rs",
            "this census's own needle table, which is the one place the strings \
             have to be written down",
        ),
        (
            "src/agent/proc/tests.rs",
            "the Process funnel's `#[cfg(test)]` suite, out of line since M6. \
             The reaper-reclaim tests name the runtime the cleanup reaper is \
             armed with -- the same text was inside `src/agent/proc.rs` below \
             its `#[cfg(test)]` cut and so was never in this domain; it is \
             named for the same reason `fake.rs` is, the marker being at the \
             DECLARATION and not in the file",
        ),
        (
            "src/runner/container.rs",
            "the Container funnel: `FunnelGroup::Container.module()`, the one \
             production file that may reach a container runtime, and the one \
             `Command::new(` row in `every_production_process_start_is_classified`",
        ),
        (
            "src/runner/container/exec/tests.rs",
            "the `ContainerRunner`'s `#[cfg(test)]` suite, out of line since W1. \
             The same text was inside `exec.rs` below its `#[cfg(test)]` cut and \
             so was never in this domain; it is named for the same reason \
             `fake.rs` is, the marker being at the DECLARATION and not in the file",
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

#[test]
fn every_denied_path_this_host_can_resolve_does_resolve() {
    let scratch = scratch_dir("resolve");
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

    let suppressed: BTreeSet<&str> = denied
        .all()
        .filter(|entry| entry.allow_invalid)
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(
        suppressed,
        BTreeSet::from([
            "libc::pipe2",
            "std::os::unix::fs::symlink",
            "std::os::windows::fs::symlink_dir",
            "std::os::windows::fs::symlink_file",
        ]),
        "an entry bought silence about whether it resolves"
    );
}

#[test]
fn every_declared_effect_denial_refuses_for_the_reason_it_declares() {
    let scratch = scratch_dir("denial");

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

mod ci_model;
mod workflow;

use ci_model::{
    CI_TARGETS, CI_WORKFLOW, MSRV_COMMAND, MSRV_JOB, OVERRIDING_REPO_FILES, RUSTFLAGS_KEY,
    TEST_COMMAND, WINDOWS_TEST_FLOOR, WINDOWS_TEST_WITNESS,
};
use workflow::{
    WORKFLOW_ESCAPES, ci_msrv_job_complaints, ci_test_job_complaints,
    ci_test_windows_job_complaints, ci_windows_build_witness_complaints, ci_workflow_text,
    complaint_codes, declared_msrv_toolchain, declared_rust_version, field, field_names,
    mutate_workflow, parse_workflow, rustflags_complaints, scalar, steps_of, three_component,
    workflow_complaints,
};

#[test]
fn the_workflow_parser_rejects_duplicate_keys_and_reads_on_as_a_string() {
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

    let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);
    assert!(
        field_names(&doc).contains("on"),
        "the workflow's `on:` key did not read back as the string `on`: {:?}",
        field_names(&doc)
    );
}

#[test]
fn the_workflow_shape_oracle_refuses_every_escape_the_ledger_names() {
    let text = ci_workflow_text();

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

#[test]
fn the_self_hosted_windows_leg_runs_these_fixtures_on_the_pinned_labels() {
    let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);
    let complaints = ci_test_windows_job_complaints(&doc);
    assert!(
        complaints.is_empty(),
        "the self-hosted Windows leg does not run these fixtures the way the contract pins:\n{}",
        complaints.join("\n")
    );
}

#[test]
fn the_hosted_windows_leg_still_links_every_test_binary() {
    let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);
    let complaints = ci_windows_build_witness_complaints(&doc);
    assert!(
        complaints.is_empty(),
        "no hosted leg code-generates and links the Windows tree the way the contract pins:\n{}",
        complaints.join("\n")
    );
}

#[test]
fn no_repository_file_overrides_what_ci_compiles_or_runs() {
    let root = repo_root();
    let present: Vec<&str> = OVERRIDING_REPO_FILES
        .iter()
        .copied()
        .filter(|name| root.join(name).exists())
        .collect();
    assert!(
        present.is_empty(),
        "these files outrank `{CI_WORKFLOW}` and this contract reads only the workflow: \
         {present:?}. A toolchain file replaces the compiler every leg runs; a Cargo config \
         can bind a target runner that reports success without executing a test binary. \
         Adding one is a deliberate act: extend this contract in the same change."
    );
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml"))
            .expect("Cargo.toml parses");
    assert!(
        manifest.get("workspace").is_none(),
        "Cargo.toml declares a workspace, so `--all-targets --all-features` no longer selects \
         this crate: `default-members` decides, and a member with no tests makes every CI \
         command succeed without running this suite."
    );
}

#[test]
fn the_self_hosted_leg_counts_the_tests_it_ran() {
    assert!(
        WINDOWS_TEST_WITNESS.starts_with(TEST_COMMAND),
        "the self-hosted leg's step does not open with `{TEST_COMMAND}`, so the suite it \
         witnesses is not the suite the other legs run"
    );
    assert!(
        WINDOWS_TEST_WITNESS.contains(&format!("-lt {WINDOWS_TEST_FLOOR}")),
        "the self-hosted leg's step does not test the count against \
         {WINDOWS_TEST_FLOOR}, so the floor this contract documents is not the floor it runs"
    );
}

#[test]
fn the_msrv_leg_checks_the_floor_the_manifest_publishes_on_every_platform() {
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

#[test]
fn the_workflow_scope_rustflags_pin_refuses_weakening_and_every_override() {
    fn probe(header: &str, job_body: &str) -> String {
        format!("{header}jobs:\n  probe:\n{job_body}")
    }
    const PLAIN: &str = "    runs-on: ubuntu-latest\n    steps:\n      - run: cargo check\n";
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

pub(crate) mod cfg;

use cfg::{
    CFG_CENSUS_CONTROL, CFG_ESCAPES, CFG_GATE_FLOOR, CONTROL_GATES, CfgForm, CfgSite,
    NO_CI_RUNNER_COMPILES, WHOLE_FILE_TEST_MODULES, cfg_regions, compiled_by, parse_cfg,
};

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

    let unmodelled = parse_cfg("feature = \"unshipped\"", false).expect("a parseable predicate");
    let refused = compiled_by(&unmodelled);
    assert!(
        refused.is_err(),
        "a cfg key no valuation models was decided anyway, as {refused:?}"
    );

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
    assert!(
        gates
            .iter()
            .any(|site| site.written == "not(any(target_os = \"linux\", target_os = \"macos\"))"),
        "the census did not find the nested negated predicate this tree is known to carry, \
         so it is reading a narrower grammar than the tree uses"
    );
    let under_a_file_guard: BTreeSet<&str> = gates
        .iter()
        .filter(|site| site.rendered.starts_with("all(test,") || site.rendered == "test")
        .map(|site| site.path.as_str())
        .collect();
    assert!(
        under_a_file_guard.len() >= WHOLE_FILE_TEST_MODULES.len(),
        "only {} file(s) carry a `test` guard the census resolved, and \
         `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names` \
         resolves {} whole-file test modules on its own",
        under_a_file_guard.len(),
        WHOLE_FILE_TEST_MODULES.len()
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

mod artifacts;

use artifacts::{
    SAMPLING_N, SITES_WITHOUT_A_FUNNEL, artifact_content, funnel_module, funnel_module_record,
    residue_record,
};

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
    assert_eq!(effect_sites().len(), EffectSiteId::all().len());
    assert!(on_disk.contains("\"site\": \"Event.OpenLog\""));
    assert!(on_disk.contains("\"site\": \"Object.CandidateCommitTree\""));
}

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
        ["Answer.StageWrite", "Answer.PublishRename", "Answer.Ingest"],
        "the set of sites whose funnel bodies are not where the inventory says          moved. Each one is a claim a gate report carries about this tree."
    );
    for entry in parsed["disagreements"].as_array().expect("an array") {
        assert_eq!(entry["inventory_module"], "src/interaction.rs");
        assert_eq!(entry["funnel_module"], "src/rundir.rs");
    }
}

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
    let distinct: BTreeSet<&str> = mechanisms.values().copied().collect();
    assert_eq!(distinct.len(), 2, "{mechanisms:?}");

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

    let harness = fs::read_to_string(repo_root().join("src/workspace_manager/tests.rs"))
        .expect("src/workspace_manager/tests.rs");
    assert!(
        harness.contains(&format!("const SAMPLING_N: u32 = {SAMPLING_N};")),
        "the sampling harness no longer runs N = {SAMPLING_N}"
    );
    assert!(on_disk.contains(&format!("\"sampling_n\": {SAMPLING_N}")));
}

#[test]
fn every_file_durability_barrier_in_a_funnel_module_goes_through_one_call() {
    const BARRIERS: &[(&str, &str, usize)] = &[
        ("src/util.rs", "fsync_file", 1),
        ("src/util.rs", "fsync_dir", 1),
    ];
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

    const FUNNELS: &[&str] = &[
        "src/rundir.rs",
        "src/workspace_manager.rs",
        "src/events/log.rs",
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

mod source_oracles;

use source_oracles::oracles;

#[test]
fn no_site_enums_row_mapping_has_a_wildcard_arm() {
    oracles::site_row_mappings_have_no_wildcard_arm();
}

#[test]
fn the_row_mapping_census_reads_the_declared_production_module() {
    oracles::the_row_mapping_census_domain_is_the_declared_module();
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
fn the_two_blankers_each_carry_their_own_contract_in_the_notes() {
    oracles::the_notes_give_each_blanker_its_own_contract();
}

#[test]
fn a_multi_byte_char_literal_does_not_desync_the_blanker() {
    oracles::a_multi_byte_char_literal_keeps_the_blankers_phase();
}

#[test]
fn a_region_that_cannot_find_an_items_end_blanks_the_attribute_not_the_file() {
    oracles::an_unfindable_item_end_blanks_the_attribute();
}

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

#[test]
fn the_view_directory_has_one_definition_in_the_tree() {
    let container: Vec<(String, String)> = scanned_sources()
        .into_iter()
        .filter(|(path, _)| {
            path.starts_with("src/runner/container") && !path.ends_with("/tests.rs")
        })
        .collect();
    let modules: BTreeSet<&str> = container.iter().map(|(path, _)| path.as_str()).collect();
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

    let (_, census) = container
        .iter()
        .find(|(path, _)| path == "src/runner/container/census.rs")
        .expect("the census module is in the scanned set");
    assert!(
        blank_comments(&production_region(census))
            .contains("pub const VIEWS_DIR: &str = \"views\";"),
        "the scan cannot see the declaration it is counting"
    );

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

#[test]
fn the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names() {
    oracles::the_whole_file_modules_are_read_from_the_declarations();
}

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

    let plain = only("#[cfg(test)]\nmod tests;\n");
    assert_eq!(plain.name, "tests");
    assert!(plain.inline_path.is_empty());
    assert_eq!(plain.guard, "test");
    assert!(plain.test_only);

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
    assert!(!only("pub(crate) mod helpers;\n").test_only);

    let inherited =
        only("#[cfg(test)]\npub(crate) mod test_support {\n    pub(crate) mod readiness;\n}\n");
    assert_eq!(inherited.name, "readiness");
    assert_eq!(inherited.inline_path, vec!["test_support".to_owned()]);
    assert_eq!(inherited.guard, "test");
    assert!(inherited.test_only);
    let ungated = only("pub(crate) mod test_support {\n    pub(crate) mod readiness;\n}\n");
    assert_eq!(ungated.inline_path, vec!["test_support".to_owned()]);
    assert!(
        !ungated.test_only,
        "a declaration under an unguarded inline module is production code"
    );

    let deep =
        only("mod outer {\n    #[cfg(test)]\n    mod middle {\n        pub mod leaf;\n    }\n}\n");
    assert_eq!(deep.name, "leaf");
    assert_eq!(
        deep.inline_path,
        vec!["outer".to_owned(), "middle".to_owned()]
    );
    assert!(deep.test_only);

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

    let after_a_function = scan("#[cfg(test)]\nfn helper() {}\nmod plain;\n");
    assert_eq!(after_a_function.len(), 1, "{after_a_function:#?}");
    assert!(!after_a_function[0].test_only);
    assert!(
        scan("#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n").is_empty(),
        "an inline module with a body names no file"
    );

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
    let after_a_brace_char = only("const C: char = '{';\n#[cfg(test)]\nmod real;\n");
    assert_eq!(after_a_brace_char.name, "real");
    assert!(after_a_brace_char.inline_path.is_empty());
    assert!(after_a_brace_char.test_only);

    assert!(scan("fn models() {}\nstruct modest;\n").is_empty());

    let past_a_macro = only("thread_local! {\n    static X: u8 = 0;\n}\n#[cfg(test)]\nmod real;\n");
    assert_eq!(past_a_macro.name, "real");
    assert!(past_a_macro.inline_path.is_empty());
    assert!(past_a_macro.test_only);
    let after_attributed_macro = only("#[cfg(test)]\nlazy! [ a, b ]\nmod plain;\n");
    assert_eq!(after_attributed_macro.name, "plain");
    assert!(
        !after_attributed_macro.test_only,
        "a `#[cfg(test)]` above a macro invocation carried to the next item"
    );
    let past_a_negation = only("fn f() { let _ = a != b; }\n#[cfg(test)]\nmod real;\n");
    assert_eq!(past_a_negation.name, "real");
    assert!(past_a_negation.test_only);

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
    let beside_a_macro = only(
        "macro_rules! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n#[cfg(test)]\nmod real;\n",
    );
    assert_eq!(beside_a_macro.name, "real");
    assert!(beside_a_macro.test_only);

    for spaced in [
        "vec ! [1, 2];\n#[cfg(test)]\nmod real;\n",
        "assert /* sic */ ! (a == b);\n#[cfg(test)]\nmod real;\n",
        "macro_rules ! m {\n    () => {\n        fn go() {}\n    };\n}\n#[cfg(test)]\nmod real;\n",
        "macro_rules ! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n#[cfg(test)]\nmod real;\n",
        "macro_rules /* named next */ ! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n#[cfg(test)]\nmod real;\n",
    ] {
        let past = only(spaced);
        assert_eq!(past.name, "real", "{spaced:?}");
        assert!(past.test_only, "{spaced:?}");
    }

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

    for (written, expected) in [
        ("#[cfg(test)]\nmod r#type;\n", "type"),
        ("#[cfg(test)]\npub(crate) mod r#fn;\n", "fn"),
        ("#[cfg(test)]\nmod r#tests;\n", "tests"),
    ] {
        let raw = only(written);
        assert_eq!(raw.name, expected, "{written:?}");
        assert!(raw.test_only, "{written:?}");
    }
    assert!(scan("struct r#mod;\nfn f() { let raw = 1; }\n").is_empty());
    let beside_a_raw_word = only("fn raw() {}\n#[cfg(test)]\nmod real;\n");
    assert_eq!(beside_a_raw_word.name, "real");

    let raw_binding = "fn f() { let r#mod = 1; }\n#[cfg(test)]\nmod tests;\n";
    let read = scan_module_declarations(raw_binding).unwrap_or_else(|refusal| {
        panic!("`let r#mod = 1;` is valid Rust and was refused: {refusal}")
    });
    assert_eq!(read.len(), 1, "{read:#?}");
    assert_eq!(read[0].name, "tests");
    assert!(read[0].test_only);

    let raw_in_a_use = "#[cfg(test)]\nmod harness {\n    use std::r#mod as tests;\n}\n";
    assert_eq!(
        scan_module_declarations(raw_in_a_use),
        Ok(Vec::new()),
        "`use std::r#mod as tests;` declares no module, and the text inside `r#mod` is not an \
         item"
    );

    for source in [
        "fn f() { let r#mod = 1; }\n#[cfg(test)]\nmod real;\n",
        "fn f() { let r#type = 1; }\n#[cfg(test)]\nmod real;\n",
        "fn f() { let r = 1; }\n#[cfg(test)]\nmod real;\n",
        "fn f() { let raw = 1; }\n#[cfg(test)]\nmod real;\n",
    ] {
        assert_eq!(only(source).name, "real", "{source:?}");
        assert_eq!(
            scan_module_declarations(&source.replace('\n', "\r\n")),
            scan_module_declarations(source),
            "CRLF: {source:?}"
        );
    }

    let past_a_raw_macro = only("r#if! { let _ = 1; }\n#[cfg(test)]\nmod real;\n");
    assert_eq!(past_a_raw_macro.name, "real");
    assert!(past_a_raw_macro.test_only);

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

fn is_the_literal_mod_tests_form(name: &str, inline_path: &[String], guard: &str) -> bool {
    name == "tests" && inline_path.is_empty() && guard == "test"
}

#[test]
fn a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal_mod_tests_form() {
    use crate::effects::census_domain::{ScannedDeclaration, scan_module_declarations};

    fn only(source: &str) -> ScannedDeclaration {
        let mut found = scan_module_declarations(source)
            .unwrap_or_else(|refusal| panic!("the fixture is readable: {refusal}"));
        assert_eq!(found.len(), 1, "{source:?} -> {found:#?}");
        found.remove(0)
    }
    fn literal(declaration: &ScannedDeclaration) -> bool {
        is_the_literal_mod_tests_form(
            &declaration.name,
            &declaration.inline_path,
            &declaration.guard,
        )
    }

    let plain = only("#[cfg(test)]\nmod tests;\n");
    assert_eq!(plain.guard, "test");
    assert!(plain.test_only);
    assert!(literal(&plain), "{plain:#?}");

    for narrowed in [
        "#[cfg(all(test, unix))]\nmod tests;\n",
        "#[cfg(test)]\n#[cfg(unix)]\nmod tests;\n",
    ] {
        let declaration = only(narrowed);
        assert_eq!(declaration.name, plain.name);
        assert_eq!(declaration.inline_path, plain.inline_path);
        assert!(
            declaration.test_only,
            "a narrowed guard still entails `test`, so the file is still a whole-file test \
             module and still belongs in the census domain: {declaration:#?}"
        );
        assert_ne!(
            declaration.guard, plain.guard,
            "the guard is the only field that differs, so it is the only field that can \
             distinguish them"
        );
        assert!(
            !literal(&declaration),
            "{narrowed:?} is not the literal `#[cfg(test)] mod tests;` form: rustc compiles no \
             such module where the narrowing is false, and a census that counted it as the plain \
             form would skip a file that is not there and lose the module on that platform in \
             silence: {declaration:#?}"
        );
    }

    let inherited = only("#[cfg(test)]\nmod test_support {\n    pub(crate) mod readiness;\n}\n");
    assert_eq!(
        (inherited.guard.as_str(), inherited.name.as_str()),
        ("test", "readiness")
    );
    assert!(
        inherited.test_only && !literal(&inherited),
        "{inherited:#?}"
    );
    let other_name = only("#[cfg(test)]\nmod scaffold;\n");
    assert_eq!(other_name.guard, "test");
    assert!(other_name.inline_path.is_empty());
    assert!(
        other_name.test_only && !literal(&other_name),
        "{other_name:#?}"
    );
}

#[test]
fn the_module_resolver_refuses_every_shape_it_cannot_resolve() {
    use crate::effects::census_domain::{
        CandidateRefusal, ScanRefusal, candidates_for, contained_in, declaration_cycle,
        module_directory, parse_predicate, scan_module_declarations, sole_present,
    };

    fn refusal(source: &str) -> ScanRefusal {
        scan_module_declarations(source).expect_err("this source is refused")
    }

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
    assert!(
        scan_module_declarations("#[path = \"x\"]\nstruct S;\nmod y;\n").is_ok(),
        "a `path` attribute on a non-module item is not a module path attribute"
    );

    for shaped in [
        "macro_rules! m {\n    () => {\n        mod x;\n    };\n}\n",
        "macro_rules! m {\n    () => {\n        #[cfg(test)]\n        mod x;\n    };\n}\n",
        "quote! { mod x; }\n",
        "paste!( mod x { } );\n",
        "items![ pub(crate) mod x; ]\n",
        "outer! { inner! { mod x; } }\n",
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

    assert!(matches!(
        refusal("#![cfg(test)]\nmod x;\n"),
        ScanRefusal::UnsupportedInnerCfg { .. }
    ));

    assert!(matches!(
        refusal("#[cfg(test)]\nmod tests;\n#[cfg(test)]\nmod tests;\n"),
        ScanRefusal::DuplicateDeclaration { .. }
    ));
    assert!(
        scan_module_declarations("mod a {\n    mod x;\n}\nmod b {\n    mod x;\n}\n").is_ok(),
        "two parents each declaring `x` are not a duplicate"
    );

    let roots = crate::effects::tests::crate_roots();
    let root = repo_root();
    let named = |file: &str, inline: &[String], name: &str| {
        candidates_for(roots, &root.join(file), inline, name)
    };
    assert_eq!(
        named(
            "src/agent/proc.rs",
            &["test_support".to_owned()],
            "readiness"
        ),
        Ok([
            root.join("src/agent/proc/test_support/readiness.rs"),
            root.join("src/agent/proc/test_support/readiness/mod.rs"),
        ])
    );
    assert_eq!(
        named("src/agent/proc.rs", &[], "readiness"),
        Ok([
            root.join("src/agent/proc/readiness.rs"),
            root.join("src/agent/proc/readiness/mod.rs"),
        ])
    );
    for flattened in named("src/agent/proc.rs", &[], "readiness").expect("inside the package") {
        assert!(
            !flattened.is_file(),
            "{} exists, so the flattening mutation would resolve instead of refusing",
            flattened.display()
        );
    }

    assert_eq!(
        named("src/engine/mod.rs", &[], "tests").map(|pair| pair[0].clone()),
        Ok(root.join("src/engine/tests.rs"))
    );
    assert_eq!(
        named("src/lib.rs", &[], "effects").map(|pair| pair[0].clone()),
        Ok(root.join("src/effects.rs"))
    );
    assert_eq!(
        named("src/main.rs", &[], "tests").map(|pair| pair[0].clone()),
        Ok(root.join("src/tests.rs"))
    );
    assert!(
        roots.is_root(&root.join("examples/probe.rs")),
        "`examples/probe.rs` is a target of this package: {:?}",
        roots.roots().collect::<Vec<_>>()
    );
    assert_eq!(
        named("examples/probe.rs", &[], "helper").map(|pair| pair[0].clone()),
        Ok(root.join("examples/helper.rs"))
    );
    assert_eq!(
        named("src/a/lib.rs", &[], "tests").map(|pair| pair[0].clone()),
        Ok(root.join("src/a/lib/tests.rs"))
    );
    assert_eq!(
        named("src/a/b/main.rs", &[], "tests").map(|pair| pair[0].clone()),
        Ok(root.join("src/a/b/main/tests.rs"))
    );
    assert_eq!(
        module_directory(roots, &root.join("src/a/mod.rs")),
        Ok(root.join("src/a"))
    );
    assert_eq!(
        module_directory(roots, &root.join("src/a/other.rs")),
        Ok(root.join("src/a/other")),
        "an ordinary module owns a directory named after it, never its parent"
    );
    let elsewhere = std::env::temp_dir().join("upstroke-not-this-package/src/lib.rs");
    assert_eq!(
        module_directory(roots, &elsewhere),
        Err(CandidateRefusal::OutsideThePackage {
            declared_in: elsewhere.clone(),
            package_dir: root.clone(),
        })
    );
    assert!(
        CandidateRefusal::OutsideThePackage {
            declared_in: elsewhere,
            package_dir: root.clone(),
        }
        .to_string()
        .contains("does not say whether it is a crate root"),
        "the refusal says what it could not decide"
    );

    let pair = named("src/a.rs", &[], "b").expect("an ordinary module");
    assert_eq!(sole_present(&pair, &|_| false), Err(0));
    assert_eq!(sole_present(&pair, &|_| true), Err(2));
    assert_eq!(sole_present(&pair, &|at| at == pair[0]), Ok(&pair[0]));
    assert_eq!(sole_present(&pair, &|at| at == pair[1]), Ok(&pair[1]));

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
    assert_eq!(
        declaration_cycle(&[edge("a.rs", "a/b.rs"), edge("a.rs", "a/c.rs")]),
        None
    );
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

#[test]
#[should_panic(expected = "does not describe the tree this census was handed")]
fn a_census_handed_a_source_root_the_manifest_does_not_describe_is_refused() {
    let elsewhere = std::env::temp_dir().join("upstroke-not-this-package");
    let _ = crate::effects::census_domain::declared_whole_file_test_modules(&elsewhere, &[]);
}

#[test]
fn the_cfg_census_resolves_module_directories_through_the_target_inventory() {
    for (file, directory) in [
        ("src/lib.rs", "src"),
        ("src/main.rs", "src"),
        ("examples/probe.rs", "examples"),
        ("src/engine/mod.rs", "src/engine"),
        ("src/effects.rs", "src/effects"),
        ("src/a/lib.rs", "src/a/lib"),
        ("src/a/main.rs", "src/a/main"),
    ] {
        assert_eq!(cfg::module_dir(file), directory, "`{file}`");
    }
}

#[test]
fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_bin_path_is_one() {
    use crate::effects::census_domain::{CrateRoots, InventoryRefusal, module_directory};

    fn by_stem(file: &Path) -> PathBuf {
        let parent = file.parent().expect("a directory").to_path_buf();
        let stem = file.file_stem().expect("a name");
        if stem == "mod" || stem == "lib" || stem == "main" {
            parent
        } else {
            parent.join(stem)
        }
    }

    let scratch = scratch_dir("inventory");
    fs::write(
        scratch.join("Cargo.toml"),
        "[package]\n\
         name = \"upstroke-inventory-fixture\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         \n\
         [lib]\n\
         path = \"src/lib.rs\"\n\
         \n\
         [[bin]]\n\
         name = \"odd\"\n\
         path = \"src/tools/odd.rs\"\n\
         \n\
         [[bin]]\n\
         name = \"nested\"\n\
         path = \"src/deep/nest/main.rs\"\n\
         \n\
         [workspace]\n",
    )
    .expect("the fixture manifest");

    let inventory = crate_roots_of(&scratch).expect("cargo reads the fixture manifest");
    assert_eq!(inventory.package_dir(), scratch.as_path());
    assert_eq!(
        inventory.roots().collect::<Vec<_>>(),
        vec![
            scratch.join("src/deep/nest/main.rs").as_path(),
            scratch.join("src/lib.rs").as_path(),
            scratch.join("src/tools/odd.rs").as_path(),
        ],
        "the inventory is exactly the manifest's three targets"
    );

    for (file, owns, stem_says) in [
        ("src/tools/odd.rs", "src/tools", "src/tools/odd"),
        ("src/deep/nest/main.rs", "src/deep/nest", "src/deep/nest"),
        ("src/a/lib.rs", "src/a/lib", "src/a"),
    ] {
        let declared_in = scratch.join(file);
        assert_eq!(
            module_directory(&inventory, &declared_in),
            Ok(scratch.join(owns)),
            "`{file}` owns `{owns}`"
        );
        assert_eq!(
            by_stem(&declared_in),
            scratch.join(stem_says),
            "the stem rule's answer for `{file}` is recorded, not guessed"
        );
    }
    let disagreements = ["src/tools/odd.rs", "src/a/lib.rs"]
        .into_iter()
        .filter(|file| {
            let declared_in = scratch.join(file);
            module_directory(&inventory, &declared_in) != Ok(by_stem(&declared_in))
        })
        .count();
    assert_eq!(
        disagreements, 2,
        "the manifest and the stem rule must disagree on the arbitrary bin path and on the \
         nested `lib.rs`, or this control measures nothing"
    );

    let missing = scratch.join("no-such-package");
    assert!(
        matches!(
            crate_roots_of(&missing),
            Err(InventoryRefusal::Failed { .. })
        ),
        "a manifest that does not exist is a refusal, not an empty inventory"
    );
    let manifest = scratch.join("Cargo.toml");
    let refusals: Vec<InventoryRefusal> = [
        "this is not json",
        "{}",
        "{\"packages\":[]}",
        "{\"packages\":[{\"manifest_path\":\"/somewhere/else/Cargo.toml\",\"targets\":[{\"src_path\":\"/somewhere/else/src/lib.rs\"}]}]}",
        "{\"packages\":[{\"manifest_path\":\"PLACEHOLDER\",\"targets\":[]}]}",
        "{\"packages\":[{\"manifest_path\":\"PLACEHOLDER\",\"targets\":[{\"name\":\"x\"}]}]}",
    ]
    .into_iter()
    .map(|document| {
        let document = document.replace(
            "PLACEHOLDER",
            &manifest.display().to_string().replace('\\', "\\\\"),
        );
        CrateRoots::from_metadata_json(&document, &manifest).expect_err("this document is refused")
    })
    .collect();
    assert!(
        matches!(refusals[0], InventoryRefusal::Unreadable { .. }),
        "{:?}",
        refusals[0]
    );
    assert!(
        matches!(refusals[1], InventoryRefusal::Unreadable { .. }),
        "{:?}",
        refusals[1]
    );
    assert!(
        matches!(refusals[2], InventoryRefusal::NoPackage { .. }),
        "{:?}",
        refusals[2]
    );
    assert!(
        matches!(refusals[3], InventoryRefusal::NoPackage { .. }),
        "a document describing a different package is refused rather than adopted: {:?}",
        refusals[3]
    );
    assert!(
        matches!(refusals[4], InventoryRefusal::NoTargets { .. }),
        "{:?}",
        refusals[4]
    );
    assert!(
        matches!(refusals[5], InventoryRefusal::Unreadable { .. }),
        "a target with no `src_path` is unreadable rather than skipped: {:?}",
        refusals[5]
    );
    for refusal in &refusals {
        assert!(
            refusal.to_string().contains("cargo metadata")
                || refusal.to_string().contains("declares no target"),
            "the refusal names the authority it could not reach: {refusal}"
        );
    }

    let live = crate::effects::tests::crate_roots();
    assert_eq!(live.package_dir(), repo_root().as_path());
    assert_eq!(
        live.roots().collect::<Vec<_>>(),
        vec![
            repo_root().join("examples/probe.rs").as_path(),
            repo_root().join("src/lib.rs").as_path(),
            repo_root().join("src/main.rs").as_path(),
        ],
        "this package's exact target inventory"
    );

    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn the_file_level_lint_reader_is_a_census_instrument_and_not_a_shipped_api() {
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
    let crlf = source.replace('\n', "\r\n");
    assert!(
        absent_from_production(&crlf).is_empty(),
        "{:#?}",
        absent_from_production(&crlf)
    );

    assert!(
        blank_comments_and_strings(&source).contains("pub(crate) mod lint_levels"),
        "the lint reader's module is no longer `pub(crate)`"
    );
    assert!(
        !blank_comments_and_strings(&source).contains("pub mod lint_levels"),
        "the lint reader's module is `pub`, which is the surface this repair removed"
    );

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
fn the_file_level_lint_reader_answers_what_rustc_does() {
    use crate::effects::lint_levels::{Resolution, file_level_lint_resolution};

    const BODY: &str = "pub fn go(p: &std::path::Path) { let _ = std::fs::write(p, \"x\"); }\n";
    const LINT: &str = "clippy::disallowed_methods";

    fn compile(dir: &Path, tag: &str, source: &str) -> (bool, Vec<(String, String)>) {
        let file = dir.join(format!("{tag}.rs"));
        fs::write(&file, source).expect("the fixture");
        let out = dir.join("out");
        fs::create_dir_all(&out).expect("an output directory");
        let output = std::process::Command::new(clippy_driver())
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
            .arg(&file)
            .output()
            .expect("clippy-driver runs; the lint gate uses the same binary");
        let mut diagnostics = Vec::new();
        for line in String::from_utf8_lossy(&output.stderr).lines() {
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
            let level = value
                .get("level")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            diagnostics.push((level.to_owned(), code.to_owned()));
        }
        (output.status.success(), diagnostics)
    }

    fn predict(resolution: Resolution) -> (bool, Vec<&'static str>, bool) {
        if resolution.refused_downgrade {
            return (false, Vec::new(), true);
        }
        match resolution.level {
            Some("allow" | "expect") => (true, Vec::new(), false),
            None | Some("warn") => (true, vec!["warning"], false),
            Some("deny" | "forbid") => (false, vec!["error"], false),
            other => panic!("the reader answered `{other:?}`, which nothing predicts"),
        }
    }

    let scratch = scratch_dir("levels");
    let table: &[(&str, &str)] = &[
        ("bare", ""),
        ("allow", "#![allow(clippy::disallowed_methods)]\n"),
        ("warn", "#![warn(clippy::disallowed_methods)]\n"),
        ("deny", "#![deny(clippy::disallowed_methods)]\n"),
        ("forbid", "#![forbid(clippy::disallowed_methods)]\n"),
        ("expect", "#![expect(clippy::disallowed_methods)]\n"),
        (
            "deny_then_allow",
            "#![deny(clippy::disallowed_methods)]\n#![allow(clippy::disallowed_methods)]\n",
        ),
        (
            "allow_then_deny",
            "#![allow(clippy::disallowed_methods)]\n#![deny(clippy::disallowed_methods)]\n",
        ),
        (
            "deny_then_warn",
            "#![deny(clippy::disallowed_methods)]\n#![warn(clippy::disallowed_methods)]\n",
        ),
        (
            "deny_then_expect",
            "#![deny(clippy::disallowed_methods)]\n#![expect(clippy::disallowed_methods)]\n",
        ),
        (
            "allow_warn_deny",
            "#![allow(clippy::disallowed_methods)]\n#![warn(clippy::disallowed_methods)]\n\
             #![deny(clippy::disallowed_methods)]\n",
        ),
        (
            "allow_then_forbid",
            "#![allow(clippy::disallowed_methods)]\n#![forbid(clippy::disallowed_methods)]\n",
        ),
        (
            "forbid_then_allow",
            "#![forbid(clippy::disallowed_methods)]\n#![allow(clippy::disallowed_methods)]\n",
        ),
        (
            "forbid_then_warn",
            "#![forbid(clippy::disallowed_methods)]\n#![warn(clippy::disallowed_methods)]\n",
        ),
        (
            "forbid_then_deny",
            "#![forbid(clippy::disallowed_methods)]\n#![deny(clippy::disallowed_methods)]\n",
        ),
        (
            "deny_then_allow_bare",
            "#![deny(clippy::disallowed_methods)]\n#![allow(disallowed_methods)]\n",
        ),
        (
            "prose_decoy",
            "//! `#![allow(clippy::disallowed_methods)]` is written here in prose.\n\
             #![deny(clippy::disallowed_methods)]\n",
        ),
        (
            "attribute_after_the_prologue",
            "#![deny(clippy::disallowed_methods)]\npub const S: &str = \
             \"#![allow(clippy::disallowed_methods)]\";\n",
        ),
    ];

    let mut observed_shapes: BTreeSet<(bool, Vec<String>, bool)> = BTreeSet::new();
    for (tag, prologue) in table {
        let source = format!("{prologue}{BODY}");
        let resolution = file_level_lint_resolution(&source, LINT);
        let (built, diagnostics) = compile(&scratch, tag, &source);
        let fired: Vec<String> = diagnostics
            .iter()
            .filter(|(_, code)| code == LINT)
            .map(|(level, _)| level.clone())
            .collect();
        let rejected = diagnostics.iter().any(|(_, code)| code == "E0453");
        let (wants_build, wants_fired, wants_rejected) = predict(resolution);
        assert_eq!(
            (built, fired.clone(), rejected),
            (
                wants_build,
                wants_fired
                    .iter()
                    .map(|level| (*level).to_owned())
                    .collect(),
                wants_rejected
            ),
            "`{tag}` — the reader answered {resolution:?} and clippy-driver did something else: \
             built={built} fired={fired:?} E0453={rejected}; all diagnostics {diagnostics:?}"
        );
        observed_shapes.insert((built, fired, rejected));

        assert_eq!(
            file_level_lint_resolution(&source.replace('\n', "\r\n"), LINT),
            resolution,
            "`{tag}` reads differently under CRLF"
        );
    }

    assert!(
        observed_shapes.len() >= 4,
        "the fixtures produced only {} distinct compiler outcomes: {observed_shapes:?}",
        observed_shapes.len()
    );

    let deny_then_allow = format!(
        "#![deny(clippy::disallowed_methods)]\n#![allow(clippy::disallowed_methods)]\n{BODY}"
    );
    assert_eq!(
        file_level_lint_resolution(&deny_then_allow, LINT),
        Resolution {
            level: Some("allow"),
            refused_downgrade: false,
        },
        "deny then allow is effectively allow"
    );
    let forbid_then_allow = format!(
        "#![forbid(clippy::disallowed_methods)]\n#![allow(clippy::disallowed_methods)]\n{BODY}"
    );
    assert_eq!(
        file_level_lint_resolution(&forbid_then_allow, LINT),
        Resolution {
            level: Some("forbid"),
            refused_downgrade: true,
        },
        "a forbid cannot be weakened; the attempt is E0453 and not a level"
    );

    let mut restated = Vec::new();
    for (path, source) in scanned_sources() {
        let blanked = blank_comments_and_strings(&source);
        for lint in USED_GOVERNED_LINTS {
            let bare = normalize_lint(lint).expect("a governed lint");
            let stated = blanked
                .split("#![")
                .skip(1)
                .filter(|attribute| {
                    attribute
                        .split(']')
                        .next()
                        .is_some_and(|body| body.contains(bare))
                })
                .count();
            if stated > 1 {
                restated.push(format!(
                    "{path} states `{lint}` in {stated} inner attributes"
                ));
            }
        }
    }
    assert!(
        restated.is_empty(),
        "the ordered reading is exercised by fixtures only while this holds: {restated:#?}"
    );

    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn the_production_code_region_removes_a_configured_item_and_keeps_the_rest() {
    oracles::the_configured_item_is_removed_and_the_rest_kept();
}

#[test]
fn the_production_code_region_excludes_typed_test_functions() {
    oracles::typed_test_functions_are_removed_and_later_code_is_kept();
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

#[test]
fn every_pr6_refusal_st16_variant_and_invariant_clause_names_a_test_or_an_owner() {
    mappings::every_promised_mapping_names_a_test_or_an_owner();
}
