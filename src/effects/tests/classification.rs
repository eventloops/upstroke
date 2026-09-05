//! Extended notes: `docs/internals/effects/tests/classification.md`

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
