//! Extended notes: `docs/internals/effects/tests/source_oracles.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

#[cfg(test)]
pub(super) mod oracles {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::effects::census_domain::{candidates_for, scan_module_declarations, sole_present};
    use crate::effects::tests::cfg::WHOLE_FILE_TEST_MODULES;
    use crate::effects::tests::{
        crate_roots, is_the_literal_mod_tests_form, repo_root, scanned_sources,
    };
    use crate::effects::{
        TOPOLOGY_MODULES, blank_comments, blank_comments_and_strings, externally_reachable_fns,
        production_code, production_region,
    };

    fn declared_production_children(
        declared_in: &Path,
        source: &str,
    ) -> Vec<(String, [PathBuf; 2])> {
        declared_children(declared_in, source)
            .into_iter()
            .filter(|(_, _, test_only)| !test_only)
            .map(|(name, candidates, _)| (name, candidates))
            .collect()
    }

    fn declared_children(declared_in: &Path, source: &str) -> Vec<(String, [PathBuf; 2], bool)> {
        refuse_unclassifiable_cfg_attr(declared_in, source);
        scan_module_declarations(source)
            .unwrap_or_else(|refusal| panic!("{}: {refusal}", declared_in.display()))
            .into_iter()
            .map(|declaration| {
                let candidates = candidates_for(
                    crate_roots(),
                    declared_in,
                    &declaration.inline_path,
                    &declaration.name,
                )
                .unwrap_or_else(|refusal| panic!("{refusal}"));
                (declaration.name, candidates, declaration.test_only)
            })
            .collect()
    }

    fn refuse_unclassifiable_cfg_attr(declared_in: &Path, source: &str) {
        let blanked = blank_comments_and_strings(source);
        let mut rest = blanked.as_str();
        while let Some(at) = rest.find("cfg_attr") {
            rest = &rest[at + "cfg_attr".len()..];
            let applied = &rest[..rest.find(']').unwrap_or(rest.len())];
            assert!(
                !applied.contains("cfg"),
                "`{}` writes `cfg_attr{applied}]`, which rustc can apply as a `cfg` that \
                 `scan_module_declarations` does not decide. A walk that cannot classify a \
                 declaration production or test-only must not classify it",
                declared_in.display()
            );
        }
    }

    mod domain {
        use std::collections::BTreeSet;
        use std::fs;
        use std::path::{Path, PathBuf};

        use crate::effects::census_domain::sole_present;
        use crate::effects::{blank_comments_and_strings, production_region};

        use super::declared_children;

        pub(super) struct ProductionModule {
            sources: Vec<(PathBuf, String)>,
        }

        impl ProductionModule {
            pub(super) fn walk(root: &Path) -> Self {
                let mut queue = vec![root.to_path_buf()];
                let mut seen = BTreeSet::new();
                let mut sources: Vec<(PathBuf, String)> = Vec::new();
                let mut accounted: BTreeSet<PathBuf> = BTreeSet::new();
                let mut test_owned: BTreeSet<PathBuf> = BTreeSet::new();
                while let Some(path) = queue.pop() {
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    let source = fs::read_to_string(&path).expect("a declared module file");
                    for (name, candidates, test_only) in declared_children(&path, &source) {
                        let resolved = sole_present(&candidates, &|candidate| candidate.is_file())
                            .unwrap_or_else(|present| {
                                panic!(
                                    "`{}` declares `mod {name};` and {present} of {candidates:?} \
                                     exist. A census domain that cannot name the file a \
                                     declaration resolves to is not a domain",
                                    path.display()
                                )
                            })
                            .clone();
                        accounted.insert(resolved.clone());
                        if test_only {
                            test_owned.insert(module_dir(&resolved));
                        } else {
                            queue.push(resolved);
                        }
                    }
                    sources.push((path, source));
                }
                sources.sort_by(|(left, _), (right, _)| left.cmp(right));
                assert!(
                    sources.len() > 1 && sources.iter().any(|(path, _)| path == root),
                    "the walk of `{}` returned {:?}: a module domain is the root plus what it \
                     declares",
                    root.display(),
                    sources.iter().map(|(path, _)| path).collect::<Vec<_>>()
                );
                refuse_unaccounted_files(root, &accounted, &test_owned);
                refuse_macro_declared_modules(&sources);
                Self { sources }
            }

            pub(super) fn sources_for_witness(&self) -> Vec<(PathBuf, String)> {
                self.sources.clone()
            }

            pub(super) fn files(&self) -> Vec<PathBuf> {
                self.sources.iter().map(|(path, _)| path.clone()).collect()
            }

            pub(super) fn row_mapping_wildcards(&self) -> RowMappingScan {
                let mut scan = RowMappingScan {
                    scanned: Vec::new(),
                    offenders: Vec::new(),
                };
                for (path, source) in &self.sources {
                    let mut mappings = 0_usize;
                    let production = blank_comments_and_strings(&production_region(source));
                    let mut rest = production.as_str();
                    while let Some(at) = rest.find("fn row(") {
                        rest = &rest[at + "fn row(".len()..];
                        let body_end = rest.find("\n    }").unwrap_or(rest.len());
                        let body = &rest[..body_end];
                        mappings += 1;
                        for wildcard in ["_ =>", "_=>"] {
                            if body.contains(wildcard) {
                                scan.offenders.push(format!(
                                    "`{}`: a `row()` mapping falls back through `{wildcard}`, \
                                     so a site added later compiles with no declared row: …{}",
                                    path.display(),
                                    &body[..body.len().min(160)]
                                ));
                            }
                        }
                    }
                    scan.scanned.push((path.clone(), mappings));
                }
                scan
            }
        }

        pub(super) fn module_dir(file: &Path) -> PathBuf {
            if file.file_stem().is_some_and(|stem| stem == "mod") {
                file.parent().unwrap_or(file).to_path_buf()
            } else {
                file.with_extension("")
            }
        }

        pub(super) fn refuse_unaccounted_files(
            root: &Path,
            accounted: &BTreeSet<PathBuf>,
            test_owned: &BTreeSet<PathBuf>,
        ) {
            let owned = module_dir(root);
            if !owned.is_dir() {
                return;
            }
            let mut stack = vec![owned];
            let mut unaccounted: Vec<PathBuf> = Vec::new();
            while let Some(current) = stack.pop() {
                let entries = fs::read_dir(&current).unwrap_or_else(|error| {
                    panic!("`{}` is not readable: {error}", current.display())
                });
                for entry in entries {
                    let path = entry.expect("a directory entry").path();
                    if path.is_dir() {
                        if !test_owned.contains(&path) {
                            stack.push(path);
                        }
                    } else if path.extension().is_some_and(|ext| ext == "rs")
                        && !accounted.contains(&path)
                    {
                        unaccounted.push(path);
                    }
                }
            }
            unaccounted.sort();
            assert!(
                unaccounted.is_empty(),
                "no declaration in the module rooted at `{}` accounts for {unaccounted:?}. A \
                 census domain derived from declarations is only the module when the \
                 declarations account for every file of it; a macro at item position expands to \
                 a declaration this scan cannot read, and the file it declares would otherwise \
                 be scanned by nothing",
                root.display()
            );
        }

        fn item_position_macros(blanked: &str) -> Vec<(usize, String)> {
            let bytes = blanked.as_bytes();
            let mut depth = 0_usize;
            let mut found = Vec::new();
            for (at, byte) in bytes.iter().enumerate() {
                match byte {
                    b'{' | b'(' | b'[' => depth += 1,
                    b'}' | b')' | b']' => depth = depth.saturating_sub(1),
                    b'!' if depth == 0 => {
                        let mut after = at + 1;
                        while bytes.get(after).is_some_and(|b| b.is_ascii_whitespace()) {
                            after += 1;
                        }
                        if !matches!(bytes.get(after), Some(b'(' | b'[' | b'{')) {
                            continue;
                        }
                        let mut start = at;
                        while start > 0
                            && (bytes[start - 1].is_ascii_alphanumeric()
                                || bytes[start - 1] == b'_')
                        {
                            start -= 1;
                        }
                        if start == at {
                            continue;
                        }
                        let mut before = start;
                        while before > 0 && bytes[before - 1].is_ascii_whitespace() {
                            before -= 1;
                        }
                        let item = before == 0 || matches!(bytes[before - 1], b';' | b'}' | b']');
                        if item {
                            let name = blanked[start..at].to_owned();
                            let line = blanked[..start].matches('\n').count() + 1;
                            found.push((line, name));
                        }
                    }
                    _ => {}
                }
            }
            found
        }

        pub(super) fn refuse_macro_declared_modules(sources: &[(PathBuf, String)]) {
            let blanked: Vec<(PathBuf, String)> = sources
                .iter()
                .map(|(path, source)| (path.clone(), blank_comments_and_strings(source)))
                .collect();
            let mut defined: BTreeSet<String> = BTreeSet::new();
            for (_, source) in &blanked {
                let mut rest = source.as_str();
                while let Some(at) = rest.find("macro_rules!") {
                    rest = &rest[at + "macro_rules!".len()..];
                    let name: String = rest
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        defined.insert(name);
                    }
                }
            }
            let mut offenders = Vec::new();
            for (path, source) in &blanked {
                for (line, name) in item_position_macros(source) {
                    if !defined.contains(&name) {
                        offenders.push(format!("`{}:{line}` invokes `{name}!`", path.display()));
                    }
                }
            }
            assert!(
                offenders.is_empty(),
                "an item-position macro whose `macro_rules!` is not in this module expands to \
                 items the declaration scan cannot read, so it can declare a module -- with a \
                 `#[path]` even one outside the module's own directory, where the directory \
                 reconciliation cannot find it either: {offenders:?}"
            );
        }

        pub(super) struct RowMappingScan {
            scanned: Vec<(PathBuf, usize)>,
            offenders: Vec<String>,
        }

        impl RowMappingScan {
            pub(super) fn paths(&self) -> Vec<PathBuf> {
                self.scanned.iter().map(|(path, _)| path.clone()).collect()
            }

            pub(super) fn mappings(&self) -> usize {
                self.scanned.iter().map(|(_, found)| found).sum()
            }

            pub(super) fn read_without_a_mapping(&self) -> Vec<&PathBuf> {
                self.scanned
                    .iter()
                    .filter(|(_, found)| *found == 0)
                    .map(|(path, _)| path)
                    .collect()
            }

            pub(super) fn offenders(&self) -> &[String] {
                &self.offenders
            }
        }
    }

    use domain::{ProductionModule, refuse_macro_declared_modules, refuse_unaccounted_files};

    fn item_body(source: &str, signature: &str) -> String {
        let blanked = blank_comments_and_strings(source);
        let at = blanked
            .find(signature)
            .unwrap_or_else(|| panic!("`{signature}` is not in this file"));
        let open = at + blanked[at..].find('{').expect("the item has a body");
        let mut depth = 0_usize;
        for (offset, byte) in blanked[open..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return blanked[open..=open + offset].to_owned();
                    }
                }
                _ => {}
            }
        }
        panic!("`{signature}` has no closing brace")
    }

    fn panic_message(body: impl FnOnce()) -> Option<String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(body))
            .err()
            .map(|payload| {
                payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| {
                        payload
                            .downcast_ref::<&str>()
                            .map(|text| (*text).to_owned())
                    })
                    .unwrap_or_else(|| "<non-string panic payload>".to_owned())
            })
    }

    pub(in crate::effects::tests) fn site_row_mappings_have_no_wildcard_arm() {
        let module = ProductionModule::walk(&repo_root().join("src/topology/effects.rs"));
        let walked = module.files();
        let scan = module.row_mapping_wildcards();

        let scanned: Vec<PathBuf> = scan.paths();
        let unwalked: Vec<&PathBuf> = scanned
            .iter()
            .filter(|path| !walked.contains(path))
            .collect();
        let unscanned: Vec<&PathBuf> = walked
            .iter()
            .filter(|path| !scanned.contains(path))
            .collect();
        assert_eq!(
            scanned, walked,
            "this census read a different set of files from the one the walk produced, so its \
             domain is not the declared production module. Read and not walked: {unwalked:?}; \
             walked and not read: {unscanned:?}"
        );

        assert!(
            walked.len() >= 8,
            "only {} file(s) in the `topology::effects` production module, so this census is \
             looking at the wrong module: {walked:?}",
            walked.len()
        );
        let mappings: usize = scan.mappings();
        assert!(
            mappings >= 8,
            "only {mappings} `row()` mappings scanned, so this census is looking at the wrong \
             files"
        );
        assert!(scan.offenders().is_empty(), "{:#?}", scan.offenders());
    }

    pub(in crate::effects::tests) fn the_row_mapping_census_domain_is_the_declared_module() {
        let effects = repo_root().join("src/topology/effects.rs");

        let synthetic =
            "mod vocab;\n#[cfg(test)]\nmod row_cases;\nmod twelfth;\n#[cfg(test)]\nmod tests;\n";
        let declared = declared_production_children(&effects, synthetic);
        let names: Vec<&str> = declared.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            ["vocab", "twelfth"],
            "the domain is read from the declarations. `row_cases` is `#[cfg(test)]` on the \
             declaration, which is exactly what `production_region` cannot cut out of the file \
             it names, and its stem is not `tests`, which is what a file-name rule reads instead"
        );
        assert_eq!(
            declared[1].1,
            [
                repo_root().join("src/topology/effects/twelfth.rs"),
                repo_root().join("src/topology/effects/twelfth/mod.rs"),
            ],
            "a production child in the `<name>/mod.rs` layout has to be a candidate; it is a \
             directory entry, so a `read_dir` filtered to `*.rs` files never sees it"
        );

        let lib = repo_root().join("src/lib.rs");
        let lib_source = fs::read_to_string(&lib).expect("the crate root");
        let topology = declared_production_children(&lib, &lib_source)
            .into_iter()
            .find(|(name, _)| name == "topology")
            .expect("`src/lib.rs` declares `mod topology;`");
        assert_eq!(
            sole_present(&topology.1, &|candidate| candidate.is_file())
                .expect("exactly one candidate for `topology` is on disk"),
            &repo_root().join("src/topology/mod.rs"),
            "the resolution has to name the `<name>/mod.rs` file, not merely list it"
        );

        let module = ProductionModule::walk(&effects);
        let walked = module.files();
        let mut expected: Vec<PathBuf> = [
            "src/topology/effects.rs",
            "src/topology/effects/bijection.rs",
            "src/topology/effects/export.rs",
            "src/topology/effects/harness.rs",
            "src/topology/effects/registry.rs",
            "src/topology/effects/residue_authority.rs",
            "src/topology/effects/sites.rs",
            "src/topology/effects/vocab.rs",
        ]
        .iter()
        .map(|relative| repo_root().join(relative))
        .collect();
        expected.sort();
        assert_eq!(
            walked, expected,
            "the `row()` census reads the root and the seven production children of \
             `topology::effects`, and nothing else"
        );
        assert!(
            !walked.contains(&repo_root().join("src/topology/effects/tests.rs")),
            "`tests.rs` is declared `#[cfg(test)]` and is not production code: {walked:?}"
        );

        let refusal = panic_message(|| {
            declared_production_children(&effects, "#[cfg_attr(all(), cfg(test))]\nmod hidden;\n");
        })
        .expect("a `cfg_attr` that can apply a `cfg` has to stop the walk");
        assert!(
            refusal.contains("cannot classify a declaration"),
            "the walk stopped, but for some other reason: {refusal}"
        );
        assert_eq!(
            declared_production_children(&effects, "mod hidden;\n")
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["hidden"],
            "control: `mod hidden;` on its own is classified, so (4) measured the `cfg_attr`"
        );

        let scan = module.row_mapping_wildcards();
        assert_eq!(
            scan.paths(),
            walked,
            "the scan's record is not the walk's output, so the equality the census asserts is \
             between something else and something else"
        );
        let barren = scan.read_without_a_mapping();
        assert!(
            barren.len() >= 5,
            "only {} file(s) of the domain hold no `row()` mapping, so this part no longer \
             measures that a file with no hit is still recorded: {barren:?}",
            barren.len()
        );
        assert!(
            scan.mappings() > 0,
            "no file of the domain holds a `row()` mapping at all, so the scan found nothing \
             and the count above is vacuous"
        );

        let owned: BTreeSet<PathBuf> = [repo_root().join("src/topology/effects/tests")]
            .into_iter()
            .collect();
        let mut accounted: BTreeSet<PathBuf> = walked
            .iter()
            .filter(|path| *path != &effects)
            .cloned()
            .collect();
        accounted.insert(repo_root().join("src/topology/effects/tests.rs"));
        refuse_unaccounted_files(&effects, &accounted, &owned);
        let mut short = accounted.clone();
        let dropped = repo_root().join("src/topology/effects/vocab.rs");
        assert!(
            short.remove(&dropped),
            "the accounting did not hold `vocab.rs`"
        );
        let refusal = panic_message(|| {
            refuse_unaccounted_files(&effects, &short, &owned);
        })
        .expect("a file no declaration accounts for has to stop the walk");
        assert!(
            refusal.contains("no declaration in the module rooted at")
                && refusal.contains("vocab.rs"),
            "the walk stopped, but for some other reason: {refusal}"
        );
        let mut without_tests = accounted.clone();
        assert!(without_tests.remove(&repo_root().join("src/topology/effects/tests.rs")));
        let tests_refusal = panic_message(|| {
            refuse_unaccounted_files(&effects, &without_tests, &owned);
        })
        .expect("an unaccounted `tests.rs` has to stop the walk too");
        assert!(
            tests_refusal.contains("tests.rs"),
            "the refusal did not name the test file: {tests_refusal}"
        );

        let hidden = repo_root().join("src/topology/effects.rs");
        let invocation = "mod bijection;\ndeclare_hidden!();\n".to_owned();
        let refusal = panic_message(|| {
            refuse_macro_declared_modules(&[(hidden.clone(), invocation.clone())]);
        })
        .expect("an item-position macro the walk cannot read has to stop it");
        assert!(
            refusal.contains("declare_hidden") && refusal.contains("cannot read"),
            "the walk stopped, but for some other reason: {refusal}"
        );
        refuse_macro_declared_modules(&[
            (hidden.clone(), invocation),
            (
                repo_root().join("src/topology/effects/vocab.rs"),
                "macro_rules! declare_hidden { () => {}; }\n".to_owned(),
            ),
        ]);
        refuse_macro_declared_modules(&[(hidden, "const _: () = assert!(true);\n".to_owned())]);
        refuse_macro_declared_modules(&module.sources_for_witness());

        let this_file = fs::read_to_string(repo_root().join("src/effects/tests/source_oracles.rs"))
            .expect("this file");
        let body = item_body(&this_file, "fn site_row_mappings_have_no_wildcard_arm");
        assert!(
            body.contains("offenders") && !body.contains("sole_present"),
            "`item_body` did not isolate the row-mapping census: {body}"
        );
        const READERS: [&str; 6] = [
            "fs",
            "File",
            "read_to_string",
            "read_dir",
            "include_str",
            "include_bytes",
        ];
        for reader in READERS {
            assert!(
                !body.contains(reader),
                "the row-mapping census names `{reader}`, so it obtains source text from \
                 somewhere other than the walk it hands the scan — a second reader beside the \
                 domain equality part (5) describes, which is a defect whether or not that \
                 equality still holds: {body}"
            );
        }
        let own = item_body(
            &this_file,
            "fn the_row_mapping_census_domain_is_the_declared_module",
        );
        assert!(
            READERS.iter().any(|reader| own.contains(reader)),
            "control: the needle set cannot detect a file read even in a body that does one"
        );
    }

    pub(in crate::effects::tests) fn topology_production_names_no_funnel() {
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

    pub(in crate::effects::tests) fn the_reachable_fn_parser_finds_every_shape() {
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
        assert!(!found.contains(&"private".to_owned()));
        assert!(!found.contains(&"hidden".to_owned()));
        assert!(!found.contains(&"in_the_test_region".to_owned()));
        assert!(!found.contains(&"declared".to_owned()));
        assert!(!found.contains(&"private_default".to_owned()));

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

    pub(in crate::effects::tests) fn the_comment_blanker_models_raw_strings() {
        let exploit = r####"const A: &str = r#"x" //"#; const B: &str = "docker";"####;
        let blanked = blank_comments(exploit);
        assert!(
            blanked.contains("\"docker\""),
            "a raw string erased the literal after it: {blanked}"
        );

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

        let counted = "// one\n/* two\nthree */\nlet b = 1;\n";
        assert_eq!(
            blank_comments(counted).lines().count(),
            counted.lines().count(),
            "the blanker lost a line"
        );
    }

    fn notes_section(notes: &str, heading: &str) -> String {
        let after = notes
            .split_once(&format!("\n{heading}\n"))
            .map(|(_, rest)| rest)
            .unwrap_or_else(|| panic!("{heading} is not a heading in the effects notes"));
        after
            .split_once("\n## ")
            .map_or(after, |(section, _)| section)
            .to_owned()
    }

    fn worked_example(section: &str, heading: &str) -> (String, String) {
        let fenced = section
            .split_once("```text\n")
            .map(|(_, rest)| rest)
            .unwrap_or_else(|| panic!("{heading} carries no ```text worked example"));
        let fenced = fenced
            .split_once("\n```")
            .map_or(fenced, |(block, _)| block);
        let mut input = None;
        let mut output = None;
        for line in fenced.lines() {
            if let Some(rest) = line.strip_prefix("in: ") {
                input = Some(rest.to_owned());
            } else if let Some(rest) = line.strip_prefix("out: ") {
                output = Some(rest.to_owned());
            }
        }
        match (input, output) {
            (Some(input), Some(output)) => (input, output),
            _ => panic!("{heading}'s worked example needs an `in: ` line and an `out: ` line"),
        }
    }

    pub(in crate::effects::tests) fn the_notes_give_each_blanker_its_own_contract() {
        const NOTES: &str = "docs/internals/effects.md";
        const DELETING: &str = "## `pub fn blank_comments(source: &str) -> String {`";
        const BLANKING: &str = "## `pub fn blank_comments_and_strings(source: &str) -> String {`";
        const LENGTH_CLAIMS: [&str; 2] = [
            "replaced by spaces of the same length",
            "keeps every byte offset",
        ];

        let notes = fs::read_to_string(repo_root().join(NOTES))
            .expect("the effects notes")
            .replace("\r\n", "\n");
        let deleting = notes_section(&notes, DELETING);
        let blanking = notes_section(&notes, BLANKING);
        let (deleting_in, deleting_out) = worked_example(&deleting, DELETING);
        let (blanking_in, blanking_out) = worked_example(&blanking, BLANKING);

        assert_eq!(
            blank_comments(&deleting_in),
            deleting_out,
            "{DELETING}'s worked example is not what `blank_comments` returns for {deleting_in:?}"
        );
        assert_eq!(
            blank_comments_and_strings(&blanking_in),
            blanking_out,
            "{BLANKING}'s worked example is not what `blank_comments_and_strings` returns for \
             {blanking_in:?}"
        );
        assert!(
            deleting_out.contains("\"docker\"") && !deleting_out.contains("/*"),
            "{DELETING}'s example has to show a comment gone and a literal kept: {deleting_out:?}"
        );
        assert!(
            !blanking_out.contains("docker") && !blanking_out.contains("/*"),
            "{BLANKING}'s example has to show both blanked: {blanking_out:?}"
        );

        for (heading, section, preserves_length) in [
            (
                DELETING,
                &deleting,
                blank_comments(&deleting_in).len() == deleting_in.len(),
            ),
            (
                BLANKING,
                &blanking,
                blank_comments_and_strings(&blanking_in).len() == blanking_in.len(),
            ),
        ] {
            let flattened = section.split_whitespace().collect::<Vec<&str>>().join(" ");
            for claim in LENGTH_CLAIMS {
                assert_eq!(
                    flattened.contains(claim),
                    preserves_length,
                    "{heading}: the notes stating \"{claim}\" is {}, while the helper preserving \
                     its input's length is {preserves_length}",
                    flattened.contains(claim)
                );
            }
        }
    }

    pub(in crate::effects::tests) fn a_multi_byte_char_literal_keeps_the_blankers_phase() {
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

    pub(in crate::effects::tests) fn an_unfindable_item_end_blanks_the_attribute() {
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

        let region = production_code("fn above() {}\n#[cfg(test)]\nuse a::b\n");
        assert!(region.contains("fn above()"), "{region:?}");
        assert!(
            region.contains("use a::b"),
            "an unterminated item blanked the rest of the file: {region:?}"
        );
        assert!(!region.contains("#[cfg(test)]"), "{region:?}");

        let region =
            production_code("fn above() {}\n#[cfg(test)]\nmod tests {\n}\nfn below() {}\n");
        assert!(region.contains("fn above()") && region.contains("fn below()"));
        assert!(
            !region.contains("mod tests"),
            "a well-formed item is still removed: {region:?}"
        );
    }

    pub(in crate::effects::tests) fn the_whole_file_modules_are_read_from_the_declarations() {
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

        let modules = crate::effects::census_domain::whole_file_test_modules(&root, &files, 13);
        fn relative<'a>(root: &std::path::Path, path: &'a std::path::Path) -> &'a std::path::Path {
            path.strip_prefix(root).unwrap_or(path)
        }
        fn sorted(mut paths: Vec<&std::path::Path>) -> Vec<&std::path::Path> {
            paths.sort_unstable();
            paths
        }
        let expected = sorted(
            WHOLE_FILE_TEST_MODULES
                .iter()
                .map(std::path::PathBuf::as_path)
                .collect(),
        );
        let stem_is_tests =
            |path: &std::path::Path| path.file_stem().is_some_and(|stem| stem == "tests");
        let expected_named_tests: Vec<&std::path::Path> = expected
            .iter()
            .copied()
            .filter(|path| stem_is_tests(path))
            .collect();
        let expected_not_named_tests: Vec<&std::path::Path> = expected
            .iter()
            .copied()
            .filter(|path| !stem_is_tests(path))
            .collect();

        let named = sorted(
            modules
                .iter()
                .filter(|path| path.file_stem().is_none_or(|stem| stem != "tests"))
                .map(|path| relative(&root, path))
                .collect(),
        );
        assert_eq!(
            named, expected_not_named_tests,
            "these are the whole-file test modules a `file_stem == \"tests\"` rule does not see, and \
             a census that uses that rule reads them as production"
        );
        let resolved = sorted(modules.iter().map(|path| relative(&root, path)).collect());
        assert_eq!(
            resolved, expected,
            "the crate's whole-file test modules are not what `WHOLE_FILE_TEST_MODULES` lists; a \
             census skipping only the ones named `tests.rs` by file name leaves the rest inside \
             its domain"
        );

        let declarations =
            crate::effects::census_domain::declared_whole_file_test_modules(&root, &files);
        fn declared_file<'a>(
            root: &std::path::Path,
            declaration: &'a crate::effects::census_domain::TestModuleDeclaration,
        ) -> &'a std::path::Path {
            let resolved =
                crate::effects::census_domain::sole_present(&declaration.candidates, &|path| {
                    path.is_file()
                })
                .expect("a derived declaration resolves to exactly one file");
            relative(root, resolved)
        }
        let declared = sorted(
            declarations
                .iter()
                .map(|declaration| declared_file(&root, declaration))
                .collect(),
        );
        assert_eq!(
            declared, expected,
            "reading the declarations resolves a different population than \
             `WHOLE_FILE_TEST_MODULES` lists"
        );
        let is_literal = |declaration: &crate::effects::census_domain::TestModuleDeclaration| {
            is_the_literal_mod_tests_form(
                &declaration.name,
                &declaration.inline_path,
                &declaration.guard,
            )
        };
        let literal = sorted(
            declarations
                .iter()
                .filter(|declaration| is_literal(declaration))
                .map(|declaration| declared_file(&root, declaration))
                .collect(),
        );
        let declaring: Vec<String> = declarations
            .iter()
            .filter(|declaration| is_literal(declaration))
            .map(|declaration| {
                relative(&root, &declaration.declared_in)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            literal, expected_named_tests,
            "these are the whole-file test modules declared by a literal `#[cfg(test)] mod \
             tests;` -- that name, that guard, at their parent's own top level -- and the \
             file-name rule finds exactly them. A declaration narrowed to `all(test, <platform>)` \
             resolves to a `tests.rs` and is missing from the left side only: it is still a \
             whole-file test module, it is not this form, and what a file-name census should do \
             about a module that exists on only some platforms is the question this failure \
             asks. The declarations were read in {declaring:?}"
        );
        let inherited: Vec<(&std::path::Path, String, Vec<String>, String)> = declarations
            .iter()
            .filter(|declaration| !declaration.inline_path.is_empty())
            .map(|declaration| {
                (
                    relative(&root, &declaration.declared_in),
                    declaration.name.clone(),
                    declaration.inline_path.clone(),
                    declaration.guard.clone(),
                )
            })
            .collect();
        assert_eq!(
            inherited,
            vec![(
                std::path::Path::new("agent/proc.rs"),
                "readiness".to_owned(),
                vec!["test_support".to_owned()],
                "test".to_owned(),
            )],
            "the declarations reached only through an inline `cfg(test)` ancestor are not what \
             this tree contains"
        );
    }

    pub(in crate::effects::tests) fn the_configured_item_is_removed_and_the_rest_kept() {
        let region = production_code("fn above() {}\n#[cfg(test)]\nmod tests;\nfn below() {}\n");
        assert!(region.contains("fn above()"), "{region:?}");
        assert!(region.contains("fn below()"), "{region:?}");
        assert!(!region.contains("mod tests;"), "{region:?}");
        assert_eq!(
            region.lines().count(),
            4,
            "the item is blanked in place, so line numbers survive: {region:?}"
        );

        let region = production_code(
            "fn above() {}\n#[cfg(test)]\nmod tests {\n    fn inner() { let _ = 1; }\n}\nfn below() {}\n",
        );
        assert!(region.contains("fn above()") && region.contains("fn below()"));
        assert!(!region.contains("fn inner()"), "{region:?}");

        let region = production_code("use a::b;\n#[cfg(test)]\nuse c::d;\nfn below() {}\n");
        assert!(region.contains("use a::b;") && region.contains("fn below()"));
        assert!(!region.contains("use c::d;"), "{region:?}");

        let region = production_code("#[cfg(test)]\nuse a::{b, c};\nfn below() {}\n");
        assert!(region.contains("fn below()"));
        assert!(!region.contains("a::"), "{region:?}");
        assert!(
            !region.contains(';'),
            "the trailing `;` goes with the item: {region:?}"
        );

        let region = production_code(
            "#[cfg(test)]\npub const ALL: &str = \"x\";\n#[cfg(test)]\npub(super) fn f() { g(); }\nfn below() {}\n",
        );
        assert!(region.contains("fn below()"));
        assert!(
            !region.contains("ALL") && !region.contains("g();"),
            "{region:?}"
        );

        let region = production_code(
            "struct S {\n    kept: u8,\n    #[cfg(test)]\n    gone: Option<u8>,\n    also_kept: u8,\n}\n",
        );
        assert!(region.contains("kept: u8,") && region.contains("also_kept: u8,"));
        assert!(!region.contains("gone"), "{region:?}");

        let region =
            production_code("#[cfg(test)]\n#[allow(dead_code)]\nmod tests;\nfn below() {}\n");
        assert!(region.contains("fn below()"));
        assert!(!region.contains("mod tests;"), "{region:?}");
    }

    pub(in crate::effects::tests) fn typed_test_functions_are_removed_and_later_code_is_kept() {
        for prefix in [
            "",
            "pub ",
            "pub(super) ",
            "pub(in crate::effects) ",
            "pub(crate) async unsafe ",
            "extern \"C\" ",
        ] {
            let source = format!(
                "#[cfg(test)]\n{prefix}fn excluded() -> Result<(RunReport, RunState), UpstrokeError> {{\n\
                 let hidden = HostRunner::new();\nOk((report, state))\n}}\n\
                 fn production() {{ let visible = HostRunner::new(); }}\n"
            );
            let region = production_code(&source);
            assert!(
                !region.contains("hidden"),
                "typed test function survived its cfg removal: {prefix:?}: {region:?}"
            );
            assert!(region.contains("fn production()"), "{prefix:?}: {region:?}");
            assert!(region.contains("let visible"), "{prefix:?}: {region:?}");
            assert_eq!(region.matches("HostRunner::new(").count(), 1, "{region:?}");
            assert_eq!(region.len(), source.len());
            assert_eq!(region.lines().count(), source.lines().count());
        }

        for field in [
            "callback: fn() -> Result<A, B>,",
            "generic: BTreeMap<K, V>,",
            "callback: unsafe extern \"C\" fn() -> Result<A, B>,",
        ] {
            let source =
                format!("struct S {{ #[cfg(test)] {field} kept: u8, }}\nfn production() {{}}\n");
            let region = production_code(&source);
            assert!(region.contains("kept: u8"), "{field}: {region:?}");
            assert!(region.contains("fn production()"), "{field}: {region:?}");
        }

        for source in [
            "#[cfg(test)]\nfn broken() -> Result<A, B>\nfn production() { let visible = HostRunner::new(); }\n",
            "#[cfg(test)] fn broken() -> Result<A, B> { fn production() {}\n",
            "#[cfg(test)] fn broken() -> Result<A, B>\n",
            "#[cfg(test)] pub(super fn broken() -> Result<A, B> { fn production() {}\n",
        ] {
            let region = production_code(source);
            assert!(
                region.contains("fn broken()"),
                "an incomplete test item swallowed later source: {region:?}"
            );
            assert_eq!(
                region.contains("fn production()"),
                source.contains("fn production()")
            );
        }
    }

    pub(in crate::effects::tests) fn a_configured_attribute_in_prose_is_inert() {
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
        let region = production_code(
            "// prose: #[cfg(test)] mod tests;\n#[cfg(test)]\nmod tests;\nfn kept() {}\n",
        );
        assert!(region.contains("fn kept()"), "{region:?}");
        assert!(!region.contains("mod tests;"), "{region:?}");
    }

    pub(in crate::effects::tests) fn the_whole_region_contains_the_truncated_one() {
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

    pub(in crate::effects::tests) fn every_early_stop_is_at_a_module() {
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

        assert!(
            at_a_module > 20,
            "only {at_a_module} file(s) cut at a module; the scan is not reading the tree"
        );

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
}
