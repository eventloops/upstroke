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

use super::census_domain::{CrateRoots, InventoryRefusal};
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

/// This package's target inventory, read once from `cargo metadata`.
///
/// **The acquisition lives here and the authority lives in
/// [`census_domain`](crate::effects::census_domain).** Reading a manifest means
/// starting a process, and `effects/allowlist.toml` records `allows = []` for
/// `src/effects.rs` on the strength of that file carrying no attribute and
/// reaching no denied primitive — "a stronger claim than any other entry in
/// this section makes", in the row's own words. This file is where the
/// machinery that drives a toolchain already lives, for the reason its prologue
/// gives: it is a whole-file test module, so a `Command::new(` in it is not in
/// any production census's domain. So the process start is here, the parse and
/// the resolution are beside the census that uses them, and neither half is
/// somewhere it would have to be argued for.
///
/// # Panics
///
/// When the inventory cannot be established. That is the fail-closed half of
/// `PR72-TARGETS-001`: which files Cargo compiles as crate roots decides which
/// file every `mod name;` in the tree resolves to, and a census that cannot
/// read the manifest must stop rather than fall back to a rule about file
/// stems — the rule this replaces, whose failures resolve to a real sibling
/// instead of announcing themselves.
pub(in crate::effects) fn crate_roots() -> &'static CrateRoots {
    static ROOTS: std::sync::OnceLock<CrateRoots> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| crate_roots_of(&repo_root()).unwrap_or_else(|refusal| panic!("{refusal}")))
}

/// The inventory of the package whose manifest sits in `manifest_dir`.
///
/// Separate from [`crate_roots`] and taking a directory, because a control that
/// only ever runs against this tree's own manifest cannot show what the reader
/// does with an arbitrary `[[bin]] path` — and an arbitrary `[[bin]] path` is
/// the whole of what the stem rule got wrong.
pub(in crate::effects) fn crate_roots_of(
    manifest_dir: &Path,
) -> Result<CrateRoots, InventoryRefusal> {
    let manifest = manifest_dir.join("Cargo.toml");
    CrateRoots::from_metadata_json(&cargo_metadata_json(&manifest)?, &manifest)
}

/// `cargo metadata` for one manifest, as its stdout.
///
/// `--no-deps` because only this package's targets are wanted, `--offline`
/// because a census must not depend on a network, and the cargo the test binary
/// was built by rather than whichever one is first on `PATH`: the MSRV job runs
/// `cargo +1.85.0`, and an inventory read by a different toolchain than the one
/// compiling the tree is an inventory of something else.
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
    /// How many **per-site** `#[expect(…)]` attributes of the recorded lints the
    /// file carries, or zero when its allowance is the module-level one.
    ///
    /// `standards/02_standards_automated_baseline.md`. A per-site
    /// expectation is narrower than a module-level allow and the compiler owns
    /// its count in both directions; this is the reviewed number that count is
    /// checked against, so an annotation appearing or vanishing has to pass
    /// through a row a reviewer reads.
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
/// **The readiness allowance is six per-site expectations, and every record
/// says the same six.**
///
/// `PR72-PLACEMENT-001`. The file used to open with a blanket
/// `#![allow(clippy::disallowed_methods)]`, and the census that guarded it —
/// `runner::container::tests::the_readiness_allowance_names_the_paths_it_is_\
/// written_against` — had to be the authority on which primitives the file
/// reaches, because nothing else was: it derives the denied set from
/// `clippy.toml` and compares it for equality, which is the only version of
/// that census worth having while a whole file is allowed.
///
/// It is not the authority any more. The lint is **denied** at file scope and
/// each of the six call sites carries its own
/// `#[expect(clippy::disallowed_methods, reason = …)]`, so under the
/// `-D warnings` the gate runs with, the compiler owns the count in both
/// directions: a seventh denied call is an error, and a site that stops
/// reaching a denied path is `unfulfilled_lint_expectations`. What is left for
/// a test is **documentation synchronisation** — that the file's prologue, the
/// six annotations and the `effects/allowlist.toml` row still say the same
/// thing — and that is all this does. The arithmetic census upstream keeps
/// its own job, which is now a second, independent reading of the same tree
/// rather than the only one.
///
/// Every needle here is contained in one line, deliberately: `PR72-WIN-EOL-003`
/// was two controls that searched for byte sequences spanning a line, which are
/// `\r\n` on the guest and hold on Unix and nowhere else. A needle that cannot
/// span a line ending cannot have that bug, so the records are written to keep
/// their phrases on one line rather than being folded back together here.
#[test]
fn the_readiness_expectations_are_per_site_and_both_records_say_so() {
    const READINESS: &str = "src/agent/proc/test_support/readiness.rs";
    const LINT: &str = "clippy::disallowed_methods";
    const SITES: usize = 6;
    const DECISION: &str = "standards/02_standards_automated_baseline.md";
    // The records are prose and spell the count as a word. The two are bound
    // rather than restated: changing `SITES` without changing the word fails
    // here instead of quietly searching for a phrase no record contains.
    const SPELLED: [&str; 8] = [
        "one", "two", "three", "four", "five", "six", "seven", "eight",
    ];
    let sites_in_words = SPELLED[SITES - 1];

    let source = fs::read_to_string(repo_root().join(READINESS)).expect("the readiness module");

    // (1) **All three governed lints are denied at file scope, and none is
    // allowed there.** The deny is what makes an expectation a narrowing.
    for lint in USED_GOVERNED_LINTS {
        assert_eq!(
            crate::effects::lint_levels::file_level_lint_state(&source, lint),
            Some("deny"),
            "{READINESS} must deny `{lint}` at file-module level"
        );
    }

    // (2) **Exactly six per-site expectations, each of them an `expect` of that
    // one lint, below module level, with a reason that names which site it is.**
    // The indices are asserted as a set: six annotations that all said "site 1
    // of 6" would satisfy a count and would mean the file had been copied
    // rather than read.
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
    // The reasons are read out of the source rather than out of the attribute
    // scan, because the scan blanks string literals — which is what keeps a
    // fixture in a doc comment invisible, and what means the reason's text has
    // to be read from the file itself.
    let indices: BTreeSet<usize> = (1..=SITES)
        .filter(|index| source.contains(&format!("site {index} of {SITES}")))
        .collect();
    assert_eq!(
        indices,
        (1..=SITES).collect::<BTreeSet<usize>>(),
        "each expectation's reason names which of the {SITES} sites it is"
    );

    // (3) **The row records the same lint and the same count**, and names the
    // decision that admits a per-site expectation at all.
    let list = allowlist();
    let row = list
        .funnel
        .iter()
        .find(|entry| entry.path == READINESS)
        .expect("the readiness row is in the funnel section");
    assert_eq!(row.allows, vec![LINT.to_owned()]);
    assert_eq!(row.expect_sites, SITES);

    // (4) **The prose in both records states the count, on one line each.**
    let phrase = format!("five distinct denied paths across {sites_in_words} sites");
    let shouted = phrase.to_uppercase();
    let allowlist_text =
        fs::read_to_string(repo_root().join(ALLOWLIST_TOML)).expect("the allowlist");
    for (record, text, needle) in [
        (READINESS, source.as_str(), phrase.as_str()),
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

    // (5) **The decision exists.** A record cited by two files and absent from
    // the tree is a citation nobody can follow.
    assert!(
        repo_root().join(DECISION).is_file(),
        "`{DECISION}` is cited by both records and is not in the tree"
    );
}

/// Whether `source`'s file-module prologue **denies** the governed `lint`.
///
/// `deny` and `forbid` both are: each makes the lint a build error for the whole
/// module tree, which is what a per-site expectation has to be narrowing.
/// `bare` because a row records `clippy::disallowed_methods` and a prologue may
/// write either spelling; the reader normalises both.
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
            // **The one shape permitted below module level**, and every clause
            // of it is load-bearing. `decisions/2026-08-30-readiness-lint-\
            // placement.md` amends `mechanism` (2)'s "only as module-level
            // attributes" for a per-site `#[expect]` and nothing else: an
            // `expect` the compiler refuses when it goes unfulfilled, carrying
            // its own reason, in a file that DENIES the lint at module level so
            // the expectation narrows a denial instead of decorating an
            // inheritance, and counted in a row a reviewer read.
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

    // A row recording per-site expectations for a file the scan never reached
    // is a row nothing checks. The count above only runs for files the scan
    // found an attribute in.
    for (path, (entry, _)) in &recorded {
        assert!(
            entry.expect_sites == 0 || carried.contains(*path),
            "{path} records {} per-site expectations and carries no governed attribute",
            entry.expect_sites
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

/// The Windows suite's job runs these fixtures on the self-hosted labels, and
/// on nothing else the contract can read.
///
/// The claim the `test` job discharges with an install step -- that
/// `clippy-driver` is present for the fixtures -- is discharged here by the
/// golden image the runner boots, which this contract cannot read; the decision
/// record binds re-curation to it instead. What the contract *can* read is
/// pinned: the labels exactly, the suite step exactly -- the command and the
/// count that says it executed, see
/// [`the_self_hosted_leg_counts_the_tests_it_ran`] -- the platform-default
/// shell on every `run:` step, and a field set with no `if:` or
/// `continue-on-error:`. The refusals are executed in [`WORKFLOW_ESCAPES`],
/// every row named `MUT-TEST-WINDOWS-*` and both `MUT-WINDOWS-WITNESS-*`.
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

/// The Windows tree is code-generated and linked on GitHub's current stable,
/// not only type-checked.
///
/// The self-hosted leg executes the suite with the image's toolchain, which
/// moves only by re-curation; `cargo check` and Clippy stop before codegen. The
/// witness is a hosted `cargo build --all-targets`, pinned exactly once on
/// exactly one `windows-latest` job and riding the Windows Clippy gate so that
/// job's step and checkout pins cover it. It links the library and binaries as
/// shipped and as test harnesses, so a Windows-only codegen or link failure in
/// any of them on current stable cannot pass every hosted leg; what it cannot
/// see is a failure that needs a toolchain newer than current stable, which no
/// leg has. Its carrier's toolchain input is pinned to `stable` too: the action
/// is pinned by commit, and the input is what decides which compiler runs. The
/// refusals are executed in [`WORKFLOW_ESCAPES`], `MUT-WINDOWS-BUILD-WITNESS-*`,
/// `MUT-WITNESS-CHECKOUT-REF` and `MUT-GATE-TOOLCHAIN-DOWNGRADED`.
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

/// No file in the repository outranks what `ci.yml` says CI compiles and runs.
///
/// Every other assertion in this section reads `ci.yml` and concludes something
/// about what CI does. Two repository files make that inference false without
/// touching the workflow at all. A `rust-toolchain.toml` overrides the rustup
/// default the pinned toolchain action sets, so every bare `cargo` command runs
/// a compiler the workflow never names -- the current-stable witness included,
/// and the MSRV floor with it. A `.cargo/config.toml` can bind
/// `target.<triple>.runner`, which Cargo applies to `cargo test`: every Windows
/// harness builds and a wrapper reports success without executing one, on the
/// one platform whose tests no other leg runs.
///
/// Neither exists, and this is what keeps it that way. Absence rather than a
/// parse: adding either is a deliberate act, and the same change must decide
/// what this contract then reads. `CLAUDE.md` already states the convention for
/// the toolchain file; this makes it enforceable rather than remembered.
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
    // Package selection, for the same reason. `--all-targets` applies to the
    // packages Cargo selected, and `workspace.default-members` chooses them.
    // TOML's parser, not a spelling. `[ workspace ]`, `[workspace] # note` and a
    // root `workspace.default-members = [...]` are one table to Cargo and three
    // different strings to a line scan, which is how the first two versions of
    // this check read and how each was shown a spelling it missed.
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

/// The leg whose tests left GitHub's runners reports that they ran.
///
/// Every other assertion here reads `ci.yml` and concludes what CI was *asked*
/// to do. Cargo can be asked for this suite and execute none of it: a
/// `target.<triple>.runner` in a repository `.cargo/config.toml`, in
/// `$CARGO_HOME`, in a directory above the checkout or in the process
/// environment hands each compiled harness to a wrapper that exits zero, and a
/// root `[workspace]` whose `default-members` name another crate builds no
/// harness of this one. Three of those are written where nothing reading this
/// repository can see them, and Cargo is free to add a fourth route.
///
/// So this leg counts instead of enumerating: a suite that did not execute
/// reports no `test result: ok.` line, and a job that cannot reach the floor
/// fails. It is pinned like every other script, and the pin is what stops the
/// count being deleted or its floor lowered to a number nothing has to clear.
///
/// It is not a defence against a pull request, and nothing in this file is: an
/// edit to `ci.yml` deletes this step as easily as any other, and the decision
/// record says where the boundary actually is. It is a defence against the
/// machine, which is the input this change added. The guest is provisioned
/// outside the repository, so its Cargo home and its environment are not in any
/// diff, and this is the leg saying it ran what it says it ran.
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
//     an inline module enclosing it. The files `cfg::WHOLE_FILE_TEST_MODULES`
//     lists are reached only that way.

// The census is `cfg`, beside this file; the two tests below are what it answers
// to. It decides predicates against `ci_model`'s targets -- the same table the
// workflow contract above is checked against -- so "no runner compiles this
// body" and "no job lints that platform" cannot drift apart.
//
// `pub(crate)` for one item and one reader: `cfg::WHOLE_FILE_TEST_MODULES` is
// the crate's only statement of the whole-file test-module population, and
// `engine::topology::recover::tests` floors its skip count at that list's
// length. Nothing else here is reachable from outside this directory.

pub(crate) mod cfg;

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
    let harness = fs::read_to_string(repo_root().join("src/workspace_manager/tests.rs"))
        .expect("src/workspace_manager/tests.rs");
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

    // **The token the fallback steps over is the whole token.**
    // `PR72-RESOLVER-003`. Everything above reads `word`s; the "anything else"
    // arm advanced by identifier *bytes*, and `r#mod` is not a run of them. It
    // consumed the `r`, met the `#`, stepped over it as a byte that opens no
    // attribute, and then read `mod …` — the **inside of a token** — as though
    // it stood at item position.
    //
    // Measured, both shapes refuse: valid Rust, and the scan will not answer
    // for the file. `let r#mod = 1;` becomes a `mod` item whose name is `=`,
    // and `use std::r#mod as tests;` becomes `mod as` with no terminator after
    // it. A refusal here is not a small failure — `declared_whole_file_test_
    // modules` panics on it, so every census that skips test modules stops on a
    // tree that compiles. Whether a given rescan refuses or instead *invents* a
    // declaration is decided by the byte after the embedded name, and neither
    // outcome is one this scan may have; the repair is that the inside of a
    // token is never read as one.
    let raw_binding = "fn f() { let r#mod = 1; }\n#[cfg(test)]\nmod tests;\n";
    let read = scan_module_declarations(raw_binding).unwrap_or_else(|refusal| {
        panic!("`let r#mod = 1;` is valid Rust and was refused: {refusal}")
    });
    assert_eq!(read.len(), 1, "{read:#?}");
    assert_eq!(read[0].name, "tests");
    assert!(read[0].test_only);

    // The second shape, inside a `#[cfg(test)]` module so that anything the
    // rescan derived would be test-only — a skip, for a file the crate never
    // declared. It declares no module at all.
    let raw_in_a_use = "#[cfg(test)]\nmod harness {\n    use std::r#mod as tests;\n}\n";
    assert_eq!(
        scan_module_declarations(raw_in_a_use),
        Ok(Vec::new()),
        "`use std::r#mod as tests;` declares no module, and the text inside `r#mod` is not an \
         item"
    );

    // And the token boundary itself, in both spellings and both directions: a
    // raw identifier is one token, an ordinary identifier that merely begins
    // with `r` is another, and a bare `r` is a third.
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

/// Whether a declaration is the literal `#[cfg(test)] mod tests;` form: that
/// name, at its parent's own top level, under **that** guard rather than one
/// that merely implies it.
///
/// Read by `the_whole_file_modules_are_read_from_the_declarations`, which
/// compares the files these resolve to against the `tests.rs` half of
/// `cfg::WHOLE_FILE_TEST_MODULES` — the half a `file_stem == "tests"` census
/// finds — and driven over synthetic input by
/// `a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal_mod_tests_form`.
///
/// **The guard has to *be* `test`, and that is the repair.** Membership used to
/// be an empty `inline_path` and `name == "tests"`, which never looked at the
/// guard at all — so `#[cfg(all(test, unix))] mod tests;` counted as the plain
/// form: same name, same file stem, still test-only, every comparison green,
/// while rustc compiles no such module on Windows and a census skipping by file
/// name goes on skipping a file that is not there. A repository whose
/// first-class target is Windows would have lost a whole test module on it with
/// the Linux suite green — the exact failure this census family exists to
/// catch. PR #101's reviewer found it and supplied the reproduction.
///
/// **The equality is predicate identity, not a text approximation.** `guard` is
/// `Predicate::render`'s output, and `Predicate::Test` is the only predicate
/// that renders as the bare `test`: `Other` is constructed for an atom whose
/// name is not `test`, or for a `name = "value"` form, and every combinator
/// renders with its own parentheses.
///
/// A guard written equivalently but not identically — `all(test)` — is refused
/// here too, and that direction is deliberate. It fails loudly, naming the
/// file, where admitting it means deciding equivalence for a rule whose whole
/// job is to say which files a *file-name* census may skip; a loud failure
/// costs a sentence in the slice that writes one, and the other direction costs
/// a platform.
///
/// A narrowed declaration is still test-only and still belongs in the domain
/// list. `cfg::WHOLE_FILE_TEST_MODULES`' doc comment says what happens then and
/// why the resulting disagreement is the signal.
///
/// Takes the three fields rather than a declaration, so the scan's own
/// `ScannedDeclaration` and the resolved `TestModuleDeclaration` are decided by
/// this one rule instead of by two copies of it
/// (`PR5D-VISIBILITY-CHECK-DUPLICATED`).
fn is_the_literal_mod_tests_form(name: &str, inline_path: &[String], guard: &str) -> bool {
    name == "tests" && inline_path.is_empty() && guard == "test"
}

/// A narrowed guard is still a whole-file test module and is **not** the
/// literal `#[cfg(test)] mod tests;` form.
///
/// The reproduction PR #101's reviewer supplied, driven over synthetic input
/// rather than by a real narrowed declaration under `src/`: writing one there
/// would make the tree the fixture and would cost that module its Windows
/// compilation for as long as it stood.
///
/// The mutation is one field wide. Every input the membership rule used to read
/// is identical between the positive and the negative below — the name, the
/// empty inline path, the resolved file stem, and `test_only` — so a rule that
/// does not read the guard cannot tell them apart, and nothing else in this
/// crate would have said so.
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

    // The positive: the form the `tests.rs` half of the census domain is a list
    // of, and the one a `file_stem == "tests"` census may skip.
    let plain = only("#[cfg(test)]\nmod tests;\n");
    assert_eq!(plain.guard, "test");
    assert!(plain.test_only);
    assert!(literal(&plain), "{plain:#?}");

    // The negatives, written both ways a narrowing reaches the declaration: one
    // attribute carrying a conjunction, and two attributes conjoined.
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

    // The other two ways out of the subset, so the guard is not the only thing
    // this rule reads: the inline ancestry `readiness.rs` is reached through,
    // and a declared name that is not `tests`. Both carry the bare `test` guard,
    // so each isolates one condition.
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

    // **A crate root owns its directory; an ordinary module does not**, and
    // which files are roots is read from this package's manifest rather than
    // from their names — `PR72-TARGETS-001`. `mod.rs` is the first case
    // wherever it sits; everything else is the first case exactly when the
    // manifest names it.
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
    // **The live instance the stem rule got wrong in this tree.**
    // `examples/probe.rs` is an `example` target, so it is a crate root and its
    // out-of-line children live in `examples/` — and `scanned_sources` walks
    // `examples/**`, so this is inside a census's domain rather than
    // hypothetical. A stem rule answers `examples/probe/`, which is a directory
    // Cargo does not compile out of.
    assert!(
        roots.is_root(&root.join("examples/probe.rs")),
        "`examples/probe.rs` is a target of this package: {:?}",
        roots.roots().collect::<Vec<_>>()
    );
    assert_eq!(
        named("examples/probe.rs", &[], "helper").map(|pair| pair[0].clone()),
        Ok(root.join("examples/helper.rs"))
    );
    // **The competing production sibling, decided rather than refused.** A
    // nested `src/a/lib.rs` this manifest never names is the ordinary module
    // `a::lib`, so `mod tests;` in it resolves to `src/a/lib/tests.rs`. Reading
    // it as a crate root points at `src/a/tests.rs` — a *different file*, a
    // sibling that may well be production, which the derivation would then
    // remove from every census as though `a/lib.rs` had declared it, and with
    // no `src/a/lib/tests.rs` present that wrong reading resolves rather than
    // refusing. The old derivation could not tell the two apart and refused
    // both; the manifest tells them apart.
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
    // The sibling the wrong reading would have claimed, named so the two
    // readings are visible side by side rather than asserted apart.
    assert_eq!(
        module_directory(roots, &root.join("src/a/other.rs")),
        Ok(root.join("src/a/other")),
        "an ordinary module owns a directory named after it, never its parent"
    );
    // **Outside the package is refused, not resolved.** An inventory is a
    // statement about one package; a file that is not inside it is one the
    // inventory says nothing about, and answering anyway would be the guess
    // this repair removed.
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

    // (7) **Zero and two candidates.** Two is `x.rs` and `x/mod.rs` both
    // present — a competing `mod.rs` that Rust itself refuses to compile and
    // that a resolver taking the first match would silently pick a side in.
    let pair = named("src/a.rs", &[], "b").expect("an ordinary module");
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

/// A census handed a source root the manifest does not describe is **refused**.
///
/// The other half of `PR72-TARGETS-001`'s fail-closed side. `source_root` is the
/// caller's claim about where the crate's sources live and the inventory is the
/// manifest's; when no target sits under it the two are about different trees,
/// and every module directory the census then resolves is resolved against an
/// inventory that says nothing about the files in hand. Driven, because no
/// caller in this tree passes such a root and an arm nobody has watched refuse
/// is an arm nobody has watched.
#[test]
#[should_panic(expected = "does not describe the tree this census was handed")]
fn a_census_handed_a_source_root_the_manifest_does_not_describe_is_refused() {
    let elsewhere = std::env::temp_dir().join("upstroke-not-this-package");
    let _ = crate::effects::census_domain::declared_whole_file_test_modules(&elsewhere, &[]);
}

/// The cfg census resolves a `mod name;` through the **same** target inventory.
///
/// `PR72-TARGETS-001`, second half. `cfg::module_dir` was a second copy of the
/// rule `census_domain` had already stopped trusting — `matches!(stem, "mod" |
/// "lib" | "main")` — and it was the copy that was still wrong on this tree
/// rather than only on a hypothetical manifest: `examples/probe.rs` is an
/// `example` target, `scanned_sources` walks `examples/**`, and the stem rule
/// puts that file's children in `examples/probe/`. `PR5D-VISIBILITY-CHECK-\
/// DUPLICATED` is the standing entry for a rule written twice; this is the
/// second copy retired, and this is the control that says so, because the tree
/// declares no `mod` inside `examples/probe.rs` today and a census that only
/// ran over the tree would not notice either reading.
#[test]
fn the_cfg_census_resolves_module_directories_through_the_target_inventory() {
    for (file, directory) in [
        // A crate root owns its own directory. All three of this package's
        // targets, so the answer is read from the manifest rather than from two
        // stems that happen to agree with it.
        ("src/lib.rs", "src"),
        ("src/main.rs", "src"),
        ("examples/probe.rs", "examples"),
        // `mod.rs` is a crate root's shape wherever it sits.
        ("src/engine/mod.rs", "src/engine"),
        // And an ordinary module owns a directory named after it — including
        // one whose stem is `lib`, which the retired rule read as a root.
        ("src/effects.rs", "src/effects"),
        ("src/a/lib.rs", "src/a/lib"),
        ("src/a/main.rs", "src/a/main"),
    ] {
        assert_eq!(cfg::module_dir(file), directory, "`{file}`");
    }
}

/// **The crate roots come from the manifest, and an arbitrary `[[bin]] path` is
/// one of them.**
///
/// `PR72-TARGETS-001`. Which files Cargo compiles as crate roots decides which
/// file every out-of-line `mod name;` in the tree resolves to, and the previous
/// derivation decided it from the file's stem: `lib.rs`/`main.rs` at the source
/// root was a root, the same stem deeper was refused, anything else was an
/// ordinary module. A manifest may name **any** path as a target, so the third
/// arm is a guess — and it is the arm that fails silently, because reading a
/// root as an ordinary module points its children one directory too deep, at a
/// sibling that may well exist.
///
/// Driven against a manifest built for it rather than against this tree, for
/// the reason a refusal is always driven here: this package's targets are
/// `src/lib.rs`, `src/main.rs` and `examples/probe.rs`, so nothing in it
/// exercises an arbitrary bin path at all. The **exact inventory** is asserted,
/// not a membership test, and the stem rule this replaces is written out beside
/// it and shown to disagree — a control that both readings pass is a control
/// that measures neither.
#[test]
fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_bin_path_is_one() {
    use crate::effects::census_domain::{CrateRoots, InventoryRefusal, module_directory};

    // The rule this replaces, written out so the disagreement is measured
    // rather than asserted. `mod` is common ground; the roots are not.
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

    // Each of the three cases, and the stem rule's answer beside it.
    for (file, owns, stem_says) in [
        // An arbitrary bin path is a crate root: its children live beside it.
        ("src/tools/odd.rs", "src/tools", "src/tools/odd"),
        // So is a `main.rs` that is not at the source root, because this
        // manifest says so — the case the old derivation refused outright.
        ("src/deep/nest/main.rs", "src/deep/nest", "src/deep/nest"),
        // And a `lib.rs` the manifest never names is an ordinary module, which
        // the old derivation also refused.
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
    // Two of the three disagree, and the two that do are the ones no rule about
    // file names can get right. The third is common ground and is here so the
    // comparison is not silently vacuous.
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

    // **Fail closed.** Every refusal is driven, because none is reachable from
    // this tree and an unreachable arm is one nobody has watched work.
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
        // A package that is not this one. Falling back to \"the first package\"
        // here is the fail-open shape: an inventory for somebody else's targets
        // reads as an inventory, and every module directory below is resolved
        // against it.
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

    // And the real package's inventory is the one the census resolves against,
    // read through the same reader.
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

/// **The file-level lint reader answers what rustc does**, on a table rustc
/// decides.
///
/// `PR72-LEVELS-001`. The reader returned at the *first* attribute naming the
/// lint, and a prologue is ordered: `#![deny(L)] #![allow(L)]` is a file where
/// `L` is allowed, and the reader called it a denial. Two censuses turn on that
/// answer — `every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist`
/// here and `runner::container::tests::every_child_module_of_the_container_
/// funnel_states_its_own_lint_level` — and the wrong answer is the reassuring
/// one: a module reported as having closed `PR6-LANEF-004` by a prologue whose
/// next line reopens it.
///
/// **No lexical restatement is accepted as authority.** The table below does not
/// say what each prologue means. Each row is compiled by `clippy-driver` under
/// this repository's own `clippy.toml`, against a body that reaches
/// `std::fs::write` — a denied path — and the *observed* diagnostics are the
/// verdict. The reader is asked the same question and its answer is turned into
/// a prediction of what the compiler must have emitted; the two are compared.
/// The only sentence written by hand is the bridge between a level and its
/// observable, and every arm of that bridge is exercised by a row, so a bridge
/// that was wrong could not stay green.
///
/// The rows include the two shapes that are the whole reason for the repair —
/// `deny` then `allow`, which must be **allow**, and `forbid` then `allow`,
/// which is `E0453` and not a level at all — and the decoys the blanking exists
/// for.
#[test]
fn the_file_level_lint_reader_answers_what_rustc_does() {
    use crate::effects::lint_levels::{Resolution, file_level_lint_resolution};

    /// A body that reaches a denied path exactly once, so a `disallowed_methods`
    /// diagnostic is produced by every level that does not suppress one.
    const BODY: &str = "pub fn go(p: &std::path::Path) { let _ = std::fs::write(p, \"x\"); }\n";
    const LINT: &str = "clippy::disallowed_methods";

    /// Compile one prologue and return whether it built, plus every diagnostic
    /// that carries a code, as `(level, code)`.
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

    /// What the compiler must have done, if the reader's answer is right.
    ///
    /// `(the crate builds, the levels at which the lint fired, E0453 present)`.
    /// The one hand-written sentence in this test, and every arm of it is
    /// reached by a row below.
    fn predict(resolution: Resolution) -> (bool, Vec<&'static str>, bool) {
        if resolution.refused_downgrade {
            // Not a level: the prologue is rejected and the lint never runs.
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
    // Every row is a prologue. Nothing here says what it means.
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
        // The qualified and bare spellings are one lint, and the order still
        // decides. `normalize_lint` is the bridge, and rustc accepts the bare
        // name (with a rename warning of its own, which this ignores).
        (
            "deny_then_allow_bare",
            "#![deny(clippy::disallowed_methods)]\n#![allow(disallowed_methods)]\n",
        ),
        // The decoys the blanking exists for: a level in prose and a level in a
        // string literal govern nothing, and an outer attribute on an item is
        // not the file module's.
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

        // The same prologue with the line endings the Windows guest gives it.
        assert_eq!(
            file_level_lint_resolution(&source.replace('\n', "\r\n"), LINT),
            resolution,
            "`{tag}` reads differently under CRLF"
        );
    }

    // **The table is not vacuous.** Four distinct compiler behaviours are
    // reached — clean, warned, errored, and rejected outright — so a reader
    // that collapsed to one answer could not pass.
    assert!(
        observed_shapes.len() >= 4,
        "the fixtures produced only {} distinct compiler outcomes: {observed_shapes:?}",
        observed_shapes.len()
    );

    // And the two claims the repair is named for, stated as values now that the
    // compiler has confirmed the reader on every row above.
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

    // **No file in this tree states one governed lint twice at file-module
    // level**, so the ordering above changes no answer today. That is the point
    // of measuring it: the repair is about what the reader does when one
    // arrives, and this says none has, rather than leaving it to be believed.
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
