//! `(3) Wrapper classification`: the four checks that hold
//! `effects/wrappers.toml` against the tree it classifies.
//!
//! The domain derivation, both directions of the effectful/denied
//! correspondence, the funnel rows, and the `libc::` sweep. All four *read* --
//! `effects/wrappers.toml`, `clippy.toml`, and the modules those two name --
//! and none of them writes anything or starts a process.
//!
//! Everything they read with stays where it was. The schemas and readers
//! (`ModuleClassification`, `wrappers`, `denylist`, `scanned_sources`,
//! `repo_root`) are `super`'s, and the production scanners
//! (`externally_reachable_fns`, `production_region`,
//! `blank_comments_and_strings`) are `crate::effects`'. This file consumes
//! them; it re-derives none of them.
//!
//! **No name here is a test name.** The four `#[test]` wrappers stay in `super`
//! under the harness names the contract and CI know, and the four functions
//! below are deliberately named otherwise -- so `--list` over the test binary is
//! unchanged and nothing nests under `effects::tests::classification`.
//!
//! # Why the bodies sit inside a `cfg(test)` module
//!
//! A file reached by a plain `mod` declaration is inside every whole-tree
//! census's domain. That is the constraint `policy.rs` records, and the one
//! that kept the effectful build helpers out of it. The inline module closes it
//! here for both of the repository's source cutters at once:
//! [`crate::effects::production_region`] truncates at the first `#[cfg(test)]`
//! and [`crate::effects::production_code`] excises the item that attribute
//! attaches to, so the four bodies are outside both regions and this file reads
//! as the test logic it is.
//!
//! It does so **without moving the whole-file module census**.
//! `census_domain::declared_whole_file_test_modules` derives a skip only from a
//! **terminated** module declaration, and drops any candidate whose name
//! carries a `{` -- which an inline module with a body always does. So
//! `the_declared_whole_file_test_modules_are_seventeen_and_three_are_not_called_tests`
//! still counts seventeen and no pinned test is renamed. Measured, not argued:
//! declared the other way, this file joins that test's named set as a fourth
//! `["effects/tests/classification.rs", "engine/topology/scaffold.rs",
//! "events/log/premove.rs", "runner/container/fake.rs"]` against its expected
//! three, and the count below it reads eighteen.
//!
//! That terminated form is deliberately not spelled out here, for the reason
//! `policy.rs` gives: one written inside a comment is the exact shape that once
//! derived a phantom skip and removed a real file from every census below it,
//! and the blanking that now defeats it is not a reason to write another.
//!
//! The neighbour that makes the shape legible is
//! `src/runner/container/census/tests.rs`, whose bare `this_file_is_test_only`
//! marker module closes the *region* half only -- `production_code` excises the
//! marker and then scans that file in full -- and what keeps it out of the
//! whole-tree censuses is a real declaration one level up. This file can have
//! neither, so it wraps the bodies rather than marking above them.
//!
//! The `#![deny]` below deliberately stays **above** the cut. Blanking takes
//! the prose, so that attribute is all three whole-tree walks' per-file "this
//! region is empty" control has left to count here -- and a region that
//! collapses to nothing is exactly what that control exists to catch.
//!
//! The three effect denials are **restored** rather than inherited. `super`
//! allows them because it drives a compiler over fixtures it creates; nothing
//! in this file does, so the allowance has no business reaching it. Measured
//! rather than believed: one probe -- a `println!`, a `std::fs::write` and a
//! `std::process::Command` -- is refused three times here and emits no
//! `disallowed_*` at all from the identical lines in `tests.rs`, so the `deny`
//! is load-bearing and not a restatement of an ambient rule. That is also what
//! keeps this module out of `effects/allowlist.toml`: an allowance is what that
//! file records, and this module takes none.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

#[cfg(test)]
pub(super) mod checks {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;

    use crate::effects::tests::{
        ModuleClassification, denylist, repo_root, scanned_sources, wrappers,
    };
    use crate::effects::{
        CLASSIFIED_MODULES, CLIPPY_TOML, WRAPPERS_TOML, blank_comments_and_strings,
        externally_reachable_fns, production_region,
    };

    /// Every externally reachable `fn` of a legacy or shared module is classified.
    ///
    /// The domain is **derived from the modules**, not listed: a `pub fn` added to
    /// one of them fails this test until somebody decides what it is. That is the
    /// only half of `mechanism` (3) a test can hold — the classification itself is
    /// a review — and it is the half that omission attacks.
    pub(in crate::effects::tests) fn reachable_fns_are_classified() {
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
    pub(in crate::effects::tests) fn effectful_wrappers_are_denied() {
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
    pub(in crate::effects::tests) fn funnel_rows_name_a_site() {
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
    pub(in crate::effects::tests) fn libc_items_are_classified_and_denied() {
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
}
