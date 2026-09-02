//! The **source oracles**: the eleven checks that hold this crate's own lexical
//! instruments against the tree they read.
//!
//! Four instruments, and every whole-tree census in this repository is built on
//! one of them: [`crate::effects::blank_comments`] and
//! [`crate::effects::blank_comments_and_strings`] decide what a census can
//! *see*, [`crate::effects::production_region`] and
//! [`crate::effects::production_code`] decide what it is allowed to *count*,
//! and `census_domain::whole_file_test_modules` decides which files it skips
//! entirely. A defect in any of them is silent by construction — the census
//! stays green and its count is simply lower — so these eleven drive each
//! instrument with input that reaches its failure path rather than measuring it
//! only on compliant input.
//!
//! The two site censuses at the top are here for the same reason and not a
//! different one: both answer a question about *source text* — a `row()` arm
//! that is a wildcard, a topology module naming a funnel in production — and
//! both would report silence rather than failure if the region they scan had
//! collapsed. Each carries its own non-vacuity control, and those controls are
//! what tie them to this file rather than to the inventory they read.
//!
//! Everything they read with stays where it was. The tree readers
//! (`scanned_sources`, `repo_root`) are `super`'s, and the instruments
//! themselves are `crate::effects`'. This file consumes them; it re-derives
//! none of them, and it defines no region of its own — which is what
//! `every_early_stop_is_at_a_module` counts two lines from its end.
//!
//! **No name here is a test name.** The eleven `#[test]` wrappers stay in
//! `super` under the harness names the contract, CI and `reviews/FINDINGS.md`
//! know, and the eleven functions below are deliberately named otherwise — so
//! `--list` over the test binary is unchanged and nothing nests under
//! `effects::tests::source_oracles`. `effects/wrappers.toml` names
//! `no_topology_module_calls_a_funnel_in_production` and `reviews/FINDINGS.md`
//! names three more; all four still resolve, because the harness did not move.
//!
//! # Why the bodies sit inside a `cfg(test)` module
//!
//! The reason `classification.rs` records, and here it is load-bearing twice
//! over rather than once. A file reached by a plain `mod` declaration is inside
//! every whole-tree census's domain, and the bodies below are an unusually rich
//! source of census needles: a table of funnel prefixes, a `RunnerRequest {`
//! quoted in prose, and the container-runtime literal three censuses in this
//! crate count files by. The inline module closes it for both source cutters at
//! once — [`crate::effects::production_region`] truncates at the first
//! `#[cfg(test)]` and [`crate::effects::production_code`] excises the item that
//! attribute attaches to — so none of those needles is in any census's region,
//! and this file reads as the test logic it is.
//!
//! It does so **without moving the whole-file module census**.
//! `census_domain::declared_whole_file_test_modules` derives a skip only from a
//! **terminated** declaration -- `mod name;` -- whose effective predicate
//! entails `cfg(test)`, and `super` declares this file with a plain `mod` at
//! its own top level: no attribute, and no inline `cfg(test)` ancestor in the
//! file that writes the declaration. The derivation deliberately does not close
//! over the file graph, so `super` being a test module itself does not make
//! this one. No skip is derived and no file leaves any census. That matters
//! more here than anywhere else in this directory:
//! `the_whole_file_modules_are_read_from_the_declarations` is one of the eleven
//! bodies below, and a declaration written the other way would make this file
//! the nineteenth module of the set it is itself asserting is eighteen.
//!
//! That terminated form is deliberately not spelled out here, for the reason
//! `policy.rs` gives: one written inside a comment is the exact shape that once
//! derived a phantom skip and removed a real file from every census below it,
//! and the blanking that now defeats it is not a reason to write another.
//!
//! The `#![deny]` below deliberately stays **above** the cut. Blanking takes
//! the prose, so that attribute is all three whole-tree walks' per-file "this
//! region is empty" control has left to count here — and a region that
//! collapses to nothing is exactly what that control exists to catch.
//!
//! The three effect denials are **restored** rather than inherited. `super`
//! allows them because it drives a compiler over fixtures it creates; nothing
//! in this file does — every body below reads the tree and writes nothing — so
//! the allowance has no business reaching it. That is also what keeps this
//! module out of `effects/allowlist.toml`: an allowance is what that file
//! records, and this module takes none.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

#[cfg(test)]
pub(super) mod oracles {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;

    use crate::effects::tests::{repo_root, scanned_sources};
    use crate::effects::{
        TOPOLOGY_MODULES, blank_comments, blank_comments_and_strings, externally_reachable_fns,
        production_code, production_region,
    };

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
    pub(in crate::effects::tests) fn site_row_mappings_have_no_wildcard_arm() {
        let source =
            std::fs::read_to_string("src/topology/effects.rs").expect("the frozen inventory");
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
    pub(in crate::effects::tests) fn the_comment_blanker_models_raw_strings() {
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
    pub(in crate::effects::tests) fn a_multi_byte_char_literal_keeps_the_blankers_phase() {
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
    pub(in crate::effects::tests) fn an_unfindable_item_end_blanks_the_attribute() {
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
        let region =
            production_code("fn above() {}\n#[cfg(test)]\nmod tests {\n}\nfn below() {}\n");
        assert!(region.contains("fn above()") && region.contains("fn below()"));
        assert!(
            !region.contains("mod tests"),
            "a well-formed item is still removed: {region:?}"
        );
    }

    /// **The whole-file test modules a census skips are the crate's own
    /// declarations, structurally resolved — not a file-name rule.**
    ///
    /// The class boundary for `PR7-R5-ATT-001`. Four whole-tree censuses skip test
    /// files; three took the set from
    /// [`census_domain::declared_whole_file_test_modules`] and one wrote its own
    /// rule, `path.file_stem() == "tests"`. That covers the fourteen files a
    /// literal `#[cfg(test)] mod tests;` declares. The crate declares four more,
    /// and they are exactly the ones a census is most likely to trip over — a
    /// scaffold, a fake and a readiness protocol exist to *name* what production
    /// names, and `scaffold.rs` sits inside the `engine/topology` domain one of
    /// those censuses walks.
    ///
    /// `agent/proc/test_support/readiness.rs` is the fourth and the one no
    /// **text** rule finds at all: it is declared `pub(crate) mod readiness;`,
    /// with no attribute of its own, inside `proc`'s inline `#[cfg(test)]
    /// pub(crate) mod test_support { … }`. Nothing in that file is
    /// `#[cfg(test)]`, so a census that did not skip it would read 500 lines of
    /// fixture — five denied effect calls among them — as production.
    ///
    /// Named individually rather than counted, because a count alone would pass if
    /// the derivation swapped one file for another. Counted as well, because
    /// names alone would pass if the derivation grew a nineteenth nobody looked
    /// at.
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
        let relative = |path: &std::path::Path| {
            path.strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        };
        let named: Vec<String> = modules
            .iter()
            .filter(|path| path.file_stem().is_none_or(|stem| stem != "tests"))
            .map(|path| relative(path))
            .collect();

        assert_eq!(
            named,
            vec![
                "agent/proc/test_support/readiness.rs".to_owned(),
                "engine/topology/scaffold.rs".to_owned(),
                "events/log/premove.rs".to_owned(),
                "runner/container/fake.rs".to_owned(),
            ],
            "these are the whole-file test modules a `file_stem == \"tests\"` rule does not see, and \
             a census that uses that rule reads them as production"
        );
        assert_eq!(
            modules.len(),
            20,
            "the crate declares {} whole-file test modules; a census skipping sixteen of them by \
             file name leaves the rest inside its domain",
            modules.len()
        );

        // **The two halves of the eighteen, separated.** The count above is
        // satisfied by any eighteen files; these two say *how* each was reached,
        // which is the part the structural scan changed. Fourteen come from a
        // literal `#[cfg(test)] mod tests;` — the form a text rule could find —
        // and the derivation must still find all fourteen after learning to read
        // structure, because a scan that resolved ancestry and lost the plain
        // case would trade one blind spot for another.
        let declarations =
            crate::effects::census_domain::declared_whole_file_test_modules(&root, &files);
        assert_eq!(declarations.len(), 20);
        let literal: Vec<String> = declarations
            .iter()
            .filter(|declaration| declaration.inline_path.is_empty() && declaration.name == "tests")
            .map(|declaration| relative(&declaration.declared_in))
            .collect();
        assert_eq!(
            literal.len(),
            16,
            "sixteen files declare `#[cfg(test)] mod tests;` at their top level and the scan \
             found {}: {literal:?}",
            literal.len()
        );
        // And the one that is reached only through an inline ancestor, named
        // with the ancestry it was reached through. This is the whole of what
        // the structural scan buys, so it is asserted as a value rather than as
        // a count.
        let inherited: Vec<(String, String, Vec<String>, String)> = declarations
            .iter()
            .filter(|declaration| !declaration.inline_path.is_empty())
            .map(|declaration| {
                (
                    relative(&declaration.declared_in),
                    declaration.name.clone(),
                    declaration.inline_path.clone(),
                    declaration.guard.clone(),
                )
            })
            .collect();
        assert_eq!(
            inherited,
            vec![(
                "agent/proc.rs".to_owned(),
                "readiness".to_owned(),
                vec!["test_support".to_owned()],
                "test".to_owned(),
            )],
            "the declarations reached only through an inline `cfg(test)` ancestor are not what \
             this tree contains"
        );
    }

    /// [`production_code`] removes the item and keeps the file.
    ///
    /// Every shape here is one this tree actually contains, and each is a way a
    /// truncating region loses production code. The censuses that use this helper
    /// count over the whole tree, so a shape it mishandles is a hole nobody would
    /// see: the count would simply be lower.
    pub(in crate::effects::tests) fn the_configured_item_is_removed_and_the_rest_kept() {
        // A `mod tests;` declaration. Fourteen files in this tree end with one, and
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
        let region =
            production_code("#[cfg(test)]\n#[allow(dead_code)]\nmod tests;\nfn below() {}\n");
        assert!(region.contains("fn below()"));
        assert!(!region.contains("mod tests;"), "{region:?}");
    }

    /// A `#[cfg(test)]` that is prose neither cuts nor is removed.
    ///
    /// The two attacks the `//`-only strip this replaced could not see, both
    /// measured against the barrier census: with either one planted as line 1 of a
    /// production file, a second `TopologyFold::parse_log` route in the same file
    /// became invisible and the census passed.
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
    pub(in crate::effects::tests) fn every_early_stop_is_at_a_module() {
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
}
