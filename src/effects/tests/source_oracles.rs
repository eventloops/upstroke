//! The **source oracles**: the twelve checks that hold this crate's own lexical
//! instruments against the tree they read.
//!
//! Four instruments, and every whole-tree census in this repository is built on
//! one of them: [`crate::effects::blank_comments`] and
//! [`crate::effects::blank_comments_and_strings`] decide what a census can
//! *see*, [`crate::effects::production_region`] and
//! [`crate::effects::production_code`] decide what it is allowed to *count*,
//! and `census_domain::whole_file_test_modules` decides which files it skips
//! entirely. A defect in any of them is silent by construction — the census
//! stays green and its count is simply lower — so these twelve drive each
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
//! **No name here is a test name.** The twelve `#[test]` wrappers stay in
//! `super` under the harness names the contract, CI and `reviews/FINDINGS.md`
//! know, and the twelve functions below are deliberately named otherwise — so
//! every name `--list` reports for this file is one of those wrappers and
//! nothing nests under `effects::tests::source_oracles`. `effects/wrappers.toml` names
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
//! `the_whole_file_modules_are_read_from_the_declarations` is one of the twelve
//! bodies below, and a declaration written the other way would make this file a
//! member of the very set it is itself asserting the membership of.
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

    /// The production module rooted at `declared_in`'s **declarations**: each
    /// non-test `mod <name>;` it writes, with the two files that name can
    /// resolve to.
    ///
    /// Derived rather than enumerated, for the reason
    /// [`crate::effects::census_domain::declared_whole_file_test_modules`] gives
    /// at length. A `read_dir` filtered to `*.rs` entries gets a census domain
    /// wrong in both directions at once: a child written in the `<name>/mod.rs`
    /// layout is a *directory* entry and is skipped, and a `#[cfg(test)] mod
    /// row_cases;` child carries its `cfg` on the **declaration**, so
    /// [`production_region`] cannot cut it out of `row_cases.rs` and a stem that
    /// is not exactly `tests` is scanned as production.
    ///
    /// **What was checked about the machinery this reuses**, rather than assumed
    /// from the fact that it exists:
    ///
    /// * [`scan_module_declarations`] reads a file's module *structure* — brace
    ///   depth, the inline modules open at each point, and the `cfg` on each of
    ///   them — over [`crate::effects::blank_comments_and_strings`], so a `mod`
    ///   written in prose or in a string is spaces. That is what a text rule
    ///   gets wrong, and `policy.rs` records a phantom skip once derived from a
    ///   declaration written inside a comment.
    /// * [`candidates_for`] names both out-of-line layouts and folds the inline
    ///   path into the directory. It refuses a `#[path]` attribute rather than
    ///   resolving it, which is the one construct that could point a declaration
    ///   outside its own directory.
    /// * [`sole_present`] refuses zero candidates and refuses two, rather than
    ///   picking one. A domain that cannot name the file a declaration resolves
    ///   to is not a domain.
    /// * **The one thing the scan does not decide**, and it is a hole rather
    ///   than a choice: a `cfg_attr` that applies a `cfg` is invisible to it, so
    ///   `#[cfg_attr(all(), cfg(test))] mod hidden;` reads as unconditional.
    ///   This walk is the first caller whose answer depends on that
    ///   classification, so it is the change that would activate the hole —
    ///   and [`refuse_unclassifiable_cfg_attr`] stops the walk on one rather
    ///   than classifying it. The reasoning is there.
    /// * **The limitation that does *not* transfer.**
    ///   `declared_whole_file_test_modules` deliberately does not close over the
    ///   file graph, because there closing would *remove* a dozen files from
    ///   every census's domain — a change to what every census can see. Here the
    ///   walk below closes over production declarations, which only *adds* files
    ///   to one census. Opposite direction, so the precedent's reason not to does
    ///   not apply.
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

    /// Every `mod <name>;` the file writes, production and test-only alike, with
    /// the two files the name can resolve to and which kind it is.
    ///
    /// The test-only half is not part of any census domain. It is here because
    /// the **reconciliation** needs it: a `.rs` file under the module's
    /// directory is legitimate when *some* declaration accounts for it, and
    /// `tests.rs` is accounted for by a `#[cfg(test)] mod tests;`. Without that
    /// half the reconciliation would refuse the one file the split is required
    /// to leave behind.
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

    /// Stop, rather than classify, when a file writes a `cfg_attr` that could
    /// apply a `cfg`.
    ///
    /// [`scan_module_declarations`] treats a `cfg_attr` as significant only when
    /// it mentions `path` (`src/effects.rs`), so
    /// `#[cfg_attr(all(), cfg(test))] mod row_cases;` — which rustc applies as
    /// `#[cfg(test)]` and compiles only under `test` — is read there as an
    /// unconditional declaration. `declared_whole_file_test_modules` records the
    /// hole at length and calls it a hole rather than a choice.
    ///
    /// **Which censuses it can reach is decided here.** Before this walk existed,
    /// the row-mapping census read one file named by a literal and derived no
    /// domain at all, so no `cfg_attr` anywhere could change what it scanned.
    /// A walk that sorts declarations into production and test-only is the first
    /// reader of that classification, and so is the change that would put the
    /// hole in reach: a test-only child read as production drops a file of
    /// legitimate test code into a census that rejects a wildcard `row()` arm.
    ///
    /// **The scan is not widened to close it.** What [`scan_module_declarations`]
    /// decides is what every census in this crate measures; teaching it to
    /// evaluate `cfg_attr` predicates changes all of them at once and is its own
    /// change with its own review — the disposition
    /// `declared_whole_file_test_modules` records for exactly this. So the walk
    /// refuses instead, in the direction every `ScanRefusal` variant takes: a
    /// scan that cannot say what a file declares must not answer.
    ///
    /// The test is the scan's own `"cfg_attr" if raw.contains("path")` shape read
    /// for `cfg` rather than for `path`, and it is deliberately coarser than the
    /// attribute-to-item association that deciding this properly would need —
    /// that association *is* the widening. Any `cfg_attr` whose attribute could
    /// apply a `cfg` stops the walk, whether or not it sits on a `mod`.
    /// Over-refusal is the safe direction, and its cost here is measured rather
    /// than assumed: no file of the `topology::effects` module writes a
    /// `cfg_attr` at all, and no `cfg_attr` **attribute** anywhere in this crate
    /// is of the refused form — every one of them conditions an `allow` or a
    /// `path`. So the walk refuses nothing on this tree, and part (3) of the
    /// witness is what keeps saying so: it walks the real module, and a
    /// `cfg_attr` added to any file of it stops that test rather than quietly
    /// changing what the census counts.
    ///
    /// Comments and string literals are blanked first, so the `cfg_attr` written
    /// in the prose above is spaces by the time it is looked for. That is the
    /// defence `policy.rs` records a phantom skip for the want of.
    ///
    /// # Panics
    ///
    /// When `source` writes such an attribute. Driven, with a control that the
    /// same declaration without it is classified, by
    /// `the_row_mapping_census_domain_is_the_declared_module` part (4).
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

    /// The privacy boundary that makes the row-mapping census's domain
    /// *provably* the walk's output. It holds nothing else.
    mod domain {
        use std::collections::BTreeSet;
        use std::fs;
        use std::path::{Path, PathBuf};

        use crate::effects::census_domain::sole_present;
        use crate::effects::{blank_comments_and_strings, production_region};

        use super::declared_children;

        /// Every file of the production module rooted at one file — the root,
        /// and transitively each file its production `mod <name>;` declarations
        /// resolve to — each with the source the walk read for it.
        ///
        /// Transitive because a domain is the module, not its first level: a
        /// `row()` mapping in `effects/sites/worktree.rs` is as much inside
        /// `topology::effects` as one in `effects/sites.rs`. Bounded by the
        /// visited set rather than by a depth limit — candidates descend into
        /// directories, so the relation is a forest and a cycle is unreachable,
        /// which is the reason to hold the set rather than a reason to trust the
        /// shape.
        ///
        /// **A type rather than a `Vec<PathBuf>`, because the census's domain has
        /// to be provably this walk's output, and a list of paths is a value any
        /// caller can equally well write down by hand.** On this tree the two are
        /// indistinguishable *by value*: the module's seven production children
        /// are all flat `.rs` files, so a hard-coded list of the current eight
        /// paths and the derived domain are the same eight paths in the same
        /// order, and no assertion comparing values can tell a census that walks
        /// from one that enumerates. Two successive repairs tried to close that
        /// by counting the call lexically; a reviewer twice showed the same
        /// substitution walking through it — keep the call, bind its result to
        /// `_`, scan a hard-coded list instead, and there is still exactly one
        /// call and no `read_dir`.
        ///
        /// So the difference is made a *type* error instead. [`Self::walk`] is
        /// the only constructor, `sources` is private to this module, and the
        /// scan is [`Self::row_mapping_wildcards`], a method over source text
        /// only the walk can put in the struct. Under that substitution there is
        /// no `ProductionModule` to call the scan on, and the census does not
        /// compile.
        ///
        /// **The type closes the substitution and not the sibling.** A census
        /// that keeps the walk, keeps the call, and takes its answer from a
        /// helper beside it that reads the files itself compiles perfectly well,
        /// and no assertion whose domain is the census's *source text* sees it —
        /// which is the bypass two lexical repairs were shown to admit. That one
        /// is closed by [`RowMappingScan::scanned`]: the scan records the path of
        /// every file it read, the census asserts that collection is
        /// [`Self::files`], and a helper's stale eight against the walk's nine
        /// fails on a set difference rather than on a name.
        pub(super) struct ProductionModule {
            sources: Vec<(PathBuf, String)>,
        }

        impl ProductionModule {
            /// Walk the production module rooted at `root`.
            ///
            /// # Panics
            ///
            /// When a declared child cannot be read or cannot be resolved to
            /// exactly one file on disk, and when the walk returns nothing but
            /// the file it was handed.
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
                            // A test-only child owns its own subtree. Its files
                            // are not production and are not this domain's, so
                            // the reconciliation prunes there rather than
                            // descending into test code to account for them.
                            test_owned.insert(module_dir(&resolved));
                        } else {
                            queue.push(resolved);
                        }
                    }
                    sources.push((path, source));
                }
                sources.sort_by(|(left, _), (right, _)| left.cmp(right));
                // **The control that binds every caller**, placed here rather
                // than at each of them, for the reason
                // `census_domain::whole_file_test_modules` gives at
                // `src/effects.rs:1392`. A walk that found nothing but the file
                // it was handed is a domain that has stopped meaning anything,
                // and no caller can hold a `ProductionModule` without having
                // come through this line.
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

            /// The walked files with the source the walk read, for the one
            /// witness that drives [`refuse_macro_declared_modules`] against
            /// the real module. Not a way to obtain the domain: the census
            /// takes its scan from [`Self::row_mapping_wildcards`], and a
            /// caller holding this still cannot construct a [`RowMappingScan`].
            pub(super) fn sources_for_witness(&self) -> Vec<(PathBuf, String)> {
                self.sources.clone()
            }

            /// The domain, sorted.
            pub(super) fn files(&self) -> Vec<PathBuf> {
                self.sources.iter().map(|(path, _)| path.clone()).collect()
            }

            /// Scan the module's production regions for `row()` mappings.
            ///
            /// **Returns the path of every file it read.** That is what lets a
            /// caller state its domain as a claim about *values* rather than
            /// about its own source text; see [`RowMappingScan`].
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
                        // The body runs to the closing brace of the `match`,
                        // which is the first line at the function's own
                        // indentation that is exactly `    }`.
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
                    // One entry per file of the domain, whether or not it held a
                    // mapping, pushed at the end of the iteration that read it —
                    // so the collection cannot be anything but the set this loop
                    // walked.
                    scan.scanned.push((path.clone(), mappings));
                }
                scan
            }
        }

        /// The directory a module owns: `x/foo.rs` owns `x/foo/`, and
        /// `x/foo/mod.rs` owns `x/foo/`.
        pub(super) fn module_dir(file: &Path) -> PathBuf {
            if file.file_stem().is_some_and(|stem| stem == "mod") {
                file.parent().unwrap_or(file).to_path_buf()
            } else {
                file.with_extension("")
            }
        }

        /// Refuse a `.rs` file under the module's own directory that **no
        /// declaration accounts for**.
        ///
        /// The domain stays *declared*, not enumerated — this does not add the
        /// directory to it, and a file found here is a refusal rather than a new
        /// member. What it closes is the direction a declaration scan cannot see
        /// on its own: [`scan_module_declarations`] reads an item-position macro
        /// invocation's **tokens**, and a macro whose body lives in a file the
        /// walk never reads expands to `mod twelfth;` while its invocation site
        /// shows nothing module-shaped. The walk then stays at eight paths and
        /// every value assertion above it still holds, because they all agree
        /// about a domain that is quietly one file short.
        ///
        /// Reproduced before this existed: `macro_rules! declare_twelfth` in
        /// `src/topology/mod.rs`, `declare_twelfth!();` in `effects.rs`, and a
        /// wildcard `row()` in `effects/twelfth/mod.rs` — both row-mapping tests
        /// green, no test file touched, the wildcard missed. The scanner is not
        /// widened to evaluate macro expansion: that would change what every
        /// census in this crate measures, which is the disposition
        /// `declared_whole_file_test_modules` records. This refuses instead,
        /// which is the direction every `ScanRefusal` variant takes.
        ///
        /// A test-only child owns its own subtree and is pruned rather than
        /// descended into: its files are not production, so accounting for them
        /// would mean reading test code to decide something no census asks.
        ///
        /// # Panics
        ///
        /// When such a file exists. Driven in both directions, with the real
        /// module and a deliberately incomplete accounting, by
        /// `the_row_mapping_census_domain_is_the_declared_module` part (7).
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

        /// Every item-position macro invocation in `source`, as (line, name).
        ///
        /// Item position, not expression position: `const _: () = assert!(..)`
        /// is a macro at brace depth 0 and is **not** an item, so the character
        /// before the name decides. `;`, `}` and `]` (an attribute) precede an
        /// item; anything else is an expression context.
        fn item_position_macros(blanked: &str) -> Vec<(usize, String)> {
            let bytes = blanked.as_bytes();
            let mut depth = 0_usize;
            let mut found = Vec::new();
            for (at, byte) in bytes.iter().enumerate() {
                match byte {
                    b'{' | b'(' | b'[' => depth += 1,
                    b'}' | b')' | b']' => depth = depth.saturating_sub(1),
                    b'!' if depth == 0 => {
                        // A delimiter after the `!` is what makes this an
                        // invocation rather than `macro_rules! name {` or `!=`.
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

        /// Refuse an item-position macro invocation whose `macro_rules!` the
        /// walk cannot read.
        ///
        /// **This is the half [`refuse_unaccounted_files`] cannot reach.** That
        /// one reconciles the module's own directory, so it catches a hidden
        /// child that lands *inside* it. A macro expanding to
        /// `#[path = "effects_hidden.rs"] mod hidden;` puts the file beside the
        /// module rather than under it, and no directory walk rooted at the
        /// module finds it. Reported by the sixth frontier pass on `27c8b2b`,
        /// with a sequence that preserves behaviour while it hides: move
        /// `EventSite::row` into the hidden file and write `_ => ResourceRow::R21`,
        /// which every current variant already maps to, so eleven mappings
        /// remain and the floor still passes.
        ///
        /// **Why the definition's location is the right test.** The hole is a
        /// macro whose *expansion* the walk cannot see. When the `macro_rules!`
        /// is itself in one of the walked files, the walk does see it, and
        /// `scan_module_declarations` already refuses any macro body holding a
        /// module-shaped token sequence (`ScanRefusal::ModuleShapedMacroBody`).
        /// So an in-domain macro cannot hide a module, and one defined anywhere
        /// else cannot be ruled out from here. The refusal is exactly the
        /// second case.
        ///
        /// Its cost on this tree is **zero, measured rather than assumed**: the
        /// module writes one item-position invocation, `const_identity_walk!`
        /// at `src/topology/effects.rs:619`, and its `macro_rules!` is at
        /// `:599` in the same file.
        ///
        /// **What it does not close.** An in-domain macro that itself invokes an
        /// out-of-domain macro, and any expansion produced by a procedural
        /// macro, whose body is not token trees this crate can read at all.
        /// Neither is reachable on this tree — the module uses no proc macro at
        /// item position — and both would need the scanner to evaluate
        /// expansion, which is the widening this file declines for the reason
        /// `declared_whole_file_test_modules` records.
        ///
        /// # Panics
        ///
        /// When such an invocation exists. Driven in both directions by
        /// `the_row_mapping_census_domain_is_the_declared_module` part (8).
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

        /// What [`ProductionModule::row_mapping_wildcards`] read, and what it
        /// found in it.
        ///
        /// **`scanned` is the half that makes the census's domain checkable.**
        /// Until it existed, whether the census scanned the walk's output was
        /// answerable only by reading the census's *source* — first by counting
        /// its call to the scan, then by banning the names that can obtain
        /// source text — and a reviewer walked through both forms the same way:
        /// a sibling helper that scans a stale list of paths leaves the census's
        /// own body innocent of every needle, so the assertion's domain (that
        /// body) was never the claim's domain (the files the census reads).
        ///
        /// Recording the paths moves the question to run time. A caller compares
        /// [`Self::paths`] with [`ProductionModule::files`], and a scan that read
        /// eight files while the walk produced nine fails on the set difference.
        ///
        /// **The fields are private to this module and there is no constructor,
        /// so that comparison cannot be handed forged operands.** The first
        /// version of this type exposed both fields `pub(super)`, and a reviewer
        /// showed the equality still bypassable: a sibling helper scans the
        /// stale eight, then builds a `RowMappingScan` whose `scanned` is copied
        /// from `ProductionModule::files()` with a zero count for the ninth path
        /// it never read. The equality then compares the walk against **the
        /// helper's own report of what it read**, which is not evidence about
        /// what it read, and the wildcard in the unscanned child is missed with
        /// both tests green. That was measured, not argued: the bypass passed at
        /// `6dc5987`.
        ///
        /// It is the same defect as the lexical witness this type replaced, one
        /// level down — an assertion whose stated domain (the files scanned) is
        /// wider than the domain it counts (the files *reported*) — so the
        /// repair is the one `CODING_STANDARDS.md` prescribes under
        /// "Representing state": keep fields private when construction must
        /// preserve an invariant, and do not expose a representation and ask
        /// callers to behave. [`ProductionModule::row_mapping_wildcards`] is now
        /// the only expression in the crate that can produce this value, and it
        /// is a method on a type only [`ProductionModule::walk`] constructs.
        pub(super) struct RowMappingScan {
            /// Every file the scan read, in the order the walk sorted them, with
            /// the number of `row()` mappings found in its production region.
            ///
            /// A file holding no mapping is still an entry: this is the
            /// **domain**, not the hits. On this tree six of the eight hold
            /// none, so the distinction is measured rather than stipulated —
            /// `the_row_mapping_census_domain_is_the_declared_module` part (5)
            /// is where.
            scanned: Vec<(PathBuf, usize)>,
            /// Every mapping that falls back through a wildcard arm, named by
            /// the file it was found in.
            offenders: Vec<String>,
        }

        impl RowMappingScan {
            /// The files the scan read, in walk order.
            pub(super) fn paths(&self) -> Vec<PathBuf> {
                self.scanned.iter().map(|(path, _)| path.clone()).collect()
            }

            /// How many `row()` mappings were found across all of them.
            pub(super) fn mappings(&self) -> usize {
                self.scanned.iter().map(|(_, found)| found).sum()
            }

            /// The files that were read and held no mapping.
            ///
            /// The control that keeps [`Self::paths`] from being a tautology:
            /// non-empty on the real module, so the recorded collection is
            /// demonstrably the domain and not the hits.
            pub(super) fn read_without_a_mapping(&self) -> Vec<&PathBuf> {
                self.scanned
                    .iter()
                    .filter(|(_, found)| *found == 0)
                    .map(|(path, _)| path)
                    .collect()
            }

            /// Every mapping that falls back through a wildcard arm.
            pub(super) fn offenders(&self) -> &[String] {
                &self.offenders
            }
        }
    }

    use domain::{ProductionModule, refuse_macro_declared_modules, refuse_unaccounted_files};

    /// The body of the item whose signature line contains `signature`, read out
    /// of `source` with comments and string literals blanked.
    ///
    /// Blanked, because the question every caller asks is what the code *does*,
    /// and a name written in a doc comment beside it is not a call. Blanking is
    /// length-preserving, so the braces this matches are the braces the compiler
    /// sees.
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

    /// Run `body`, returning its panic message if it panicked.
    ///
    /// **The panic hook is deliberately left alone**, for the reason
    /// `workspace_manager::tests::panic_message` gives at length: the hook is
    /// process-global and these tests run in parallel, so two of them swapping it
    /// out and back can interleave and leave the process running with a no-op
    /// hook for good. A few lines of expected noise on stderr is the cheaper
    /// half of that trade.
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
    /// something fails to compile today. The `topology::effects` inventory is
    /// frozen in the sense that matters here — its seventy sites and their
    /// mappings are PR3's and no slice adds to them — so this scan guards a
    /// property no slice may change, whatever file the mappings sit in. They
    /// have not always sat in one: the module was split into per-concern
    /// children, which is what the domain below is derived rather than
    /// enumerated for.
    ///
    /// The mappings live in a **module** rather than a file since
    /// `topology::effects` was split into per-concern child modules:
    /// `EffectSiteId::row` stayed in the root and the eleven site enums' went to
    /// `sites.rs`. So the domain is a [`ProductionModule`] — a path here names a
    /// module, not a file — and
    /// `the_row_mapping_census_domain_is_the_declared_module` is what measures
    /// membership in it, in both directions.
    ///
    /// **The domain is bound to the walk here, over values.** The scan returns
    /// the path of every file it read, and the first assertion below is that
    /// that collection *is* [`ProductionModule::files`]. Two earlier repairs
    /// asserted the same intent over this function's **source text** — one
    /// counted the call, one banned the names that can open a file — and both
    /// were bypassed the same way, by a sibling helper that scans a stale list
    /// of paths and leaves this body innocent. A set difference does not care
    /// what the body says: a helper scanning yesterday's eight files while the
    /// walk produces nine fails on the ninth path.
    pub(in crate::effects::tests) fn site_row_mappings_have_no_wildcard_arm() {
        let module = ProductionModule::walk(&repo_root().join("src/topology/effects.rs"));
        let walked = module.files();
        let scan = module.row_mapping_wildcards();

        // THE DOMAIN, ASSERTED OVER VALUES: what was read equals what the walk
        // produced. Not "the body of this function names no reader" — that is a
        // claim about this text, and a helper beside it reading a stale list of
        // paths satisfies it while scanning the wrong set.
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

    /// The `row()` census's domain is the **declared** production module, and
    /// membership in it is measured in both directions.
    ///
    /// The two witnesses the first repoint carried measured neither. Planting a
    /// wildcard in `vocab.rs` proves that one already-included flat file is
    /// scanned; filtering the walk to nothing proves that the count floor fires.
    /// Both are true of a domain that is simply the wrong set. What follows
    /// drives the membership rule itself, and it writes nothing: this module
    /// restores the three effect denials (`#![deny]` above), so a fixture tree on
    /// disk is not available to it and is not needed.
    pub(in crate::effects::tests) fn the_row_mapping_census_domain_is_the_declared_module() {
        let effects = repo_root().join("src/topology/effects.rs");

        // (1) DECLARED, NOT ENUMERATED, and both out-of-line layouts are named.
        // The declaring file is the real one — only the text is synthetic — so
        // the directory the candidates are resolved in is the real one too.
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

        // (2) AND THE `mod.rs` CANDIDATE IS THE ONE THAT RESOLVES when it is the
        // one on disk. Measured on a real directory-layout module of this crate:
        // `src/topology.rs` does not exist and `src/topology/mod.rs` does.
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

        // (3) THE REAL DOMAIN, named file by file — and `tests.rs` is outside it
        // because its declaration carries `#[cfg(test)]`, not because of its stem.
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
        // Sorted the same way the walk sorts, so the assertion is about the set
        // and not about the order these eight were typed in: `PathBuf` orders by
        // component, so `effects` precedes `effects.rs`.
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

        // (4) A DECLARATION THE SCAN CANNOT CLASSIFY STOPS THE WALK.
        //
        // `scan_module_declarations` does not decide a `cfg_attr` that applies a
        // `cfg`, and the walk above is the first reader of its production /
        // test-only classification — so this walk is what would put that hole in
        // reach of the census, and `refuse_unclassifiable_cfg_attr` is what keeps
        // it out. Driven here rather than argued, because a refusal nothing
        // exercises is the same silence it exists to break.
        let refusal = panic_message(|| {
            declared_production_children(&effects, "#[cfg_attr(all(), cfg(test))]\nmod hidden;\n");
        })
        .expect("a `cfg_attr` that can apply a `cfg` has to stop the walk");
        assert!(
            refusal.contains("cannot classify a declaration"),
            "the walk stopped, but for some other reason: {refusal}"
        );
        // The control: the refusal is the attribute, not the declaration under
        // it. Without the `cfg_attr` the same `mod hidden;` classifies as
        // production and the walk carries on.
        assert_eq!(
            declared_production_children(&effects, "mod hidden;\n")
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["hidden"],
            "control: `mod hidden;` on its own is classified, so (4) measured the `cfg_attr`"
        );

        // (5) AND THE CENSUS'S DOMAIN IS A CLAIM ABOUT VALUES.
        //
        // Parts (1) to (4) test the walk. They cannot say whether the census
        // uses it. Two repairs answered that lexically — count the census's call
        // to the scan, then ban the names that can obtain source text — and a
        // reviewer walked through both, for the same reason each time: **the
        // domain of a lexical assertion is the census's source text, and the
        // claim is about the files the census reads.** A sibling helper that
        // scans a stale list of paths satisfies every such assertion while
        // scanning the wrong set, and no further needle reaches it, because the
        // reading it has to catch is not in the text it looks at.
        //
        // So the claim is made where it can be made over values.
        // `site_row_mappings_have_no_wildcard_arm` asserts that the paths
        // `RowMappingScan::scanned` recorded *are* `ProductionModule::files`;
        // a helper that scans eight files while the walk produces nine fails on
        // the ninth path, whatever its body says.
        //
        // What this part measures is the property that assertion rests on, which
        // nothing else states: **the recorded collection is the domain, not the
        // hits.** Six of the module's eight files hold no `row()` at all — every
        // mapping is in the root and in `sites.rs` — so a scan that recorded
        // only the files it found something in would record two paths, and the
        // census's equality would fail on the honest tree. That is also the
        // stale-helper shape exactly: a helper reports the set it looked at, and
        // the set it looked at is the wrong one.
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

        // (7) AND A FILE NO DECLARATION ACCOUNTS FOR STOPS THE WALK.
        //
        // Parts (1) to (5) all reason from the declarations. None of them can
        // see a declaration the scan cannot read, and there is one:
        // `scan_module_declarations` reads an item-position macro invocation's
        // *tokens*, so a macro whose body lives in a file the walk never reads
        // expands to `mod twelfth;` while its invocation shows nothing
        // module-shaped. Reproduced before the refusal existed — the macro in
        // `src/topology/mod.rs`, the invocation in `effects.rs`, a wildcard
        // `row()` in `effects/twelfth/mod.rs` — and **both row-mapping tests
        // stayed green with no test file touched**. Every assertion above agreed
        // about a domain one file short, which is what makes it invisible from
        // inside the declarations.
        //
        // `refuse_unaccounted_files` reconciles the declared set against the
        // module's own directory. The domain is still declared: a file found
        // here is a refusal, never a new member.
        //
        // Driven with the real module and no fixture tree, because this module
        // restores the three effect denials and writes nothing. The accounting
        // is the input, so both directions are reachable by varying it.
        let owned: BTreeSet<PathBuf> = [repo_root().join("src/topology/effects/tests")]
            .into_iter()
            .collect();
        let mut accounted: BTreeSet<PathBuf> = walked
            .iter()
            .filter(|path| *path != &effects)
            .cloned()
            .collect();
        accounted.insert(repo_root().join("src/topology/effects/tests.rs"));
        // The honest accounting refuses nothing — the control, and it is what
        // says the refusal below is about the missing entry rather than about
        // the directory being unreadable or the pruning being wrong.
        refuse_unaccounted_files(&effects, &accounted, &owned);
        // Drop one file from the accounting and it is named. That is exactly
        // the shape a macro-declared child presents: on disk, in no declaration.
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
        // And the test-only prune is load-bearing, not decoration: without it
        // `tests.rs` is a `.rs` file under the directory that the PRODUCTION
        // accounting does not hold, so the reconciliation would refuse the one
        // file every split of this shape is required to leave behind.
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

        // (8) AND A MACRO THE WALK CANNOT READ STOPS IT TOO.
        //
        // Part (7) reconciles the module's own DIRECTORY, so it catches a hidden
        // child that lands inside it. A macro expanding to
        // `#[path = "effects_hidden.rs"] mod hidden;` puts the file beside the
        // module instead, where no directory walk rooted at the module finds it
        // — the sixth pass's finding, with a sequence that preserves behaviour
        // while it hides: `EventSite::row` moved into the hidden file with
        // `_ => ResourceRow::R21`, which every current variant already maps to.
        //
        // The test is where the `macro_rules!` lives, not whether a macro is
        // present. A macro defined in a walked file is one the walk reads, and
        // `scan_module_declarations` already refuses a module-shaped body there;
        // one defined anywhere else cannot be ruled out from here. Synthetic
        // sources, because the input is a list of (path, source) pairs and both
        // directions are reachable by varying it.
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
        // THE CONTROL THAT MAKES IT THE DEFINITION'S LOCATION AND NOT THE MACRO:
        // the same invocation passes once the definition is in the walked set,
        // because then the scan reads the body and refuses a module-shaped one
        // itself.
        refuse_macro_declared_modules(&[
            (hidden.clone(), invocation),
            (
                repo_root().join("src/topology/effects/vocab.rs"),
                "macro_rules! declare_hidden { () => {}; }\n".to_owned(),
            ),
        ]);
        // And an expression-position macro is not an item: this is what keeps
        // the refusal's cost at zero on a module that writes
        // `const _: () = assert!(...)`.
        refuse_macro_declared_modules(&[(hidden, "const _: () = assert!(true);\n".to_owned())]);
        // The real module refuses nothing — measured, not assumed. Its one
        // item-position invocation is `const_identity_walk!`, defined in the
        // same file.
        refuse_macro_declared_modules(&module.sources_for_witness());

        // (6) BELT AND BRACES: THE CENSUS'S OWN BODY NAMES NO READER.
        //
        // **The domain of this part is the census's text, and so is its claim.**
        // It does not establish (5)'s property and is not written as though it
        // did — a helper beside the census leaves this body innocent, which is
        // why (5) exists and why a seventh needle would not help. What it closes
        // is the cheap form, an edit that opens a file inside the census itself:
        // it fails one step earlier than the set difference would, naming the
        // construct rather than a set of paths. It bans constructs rather than
        // counting a name, so unlike a call count it has no near-miss to slip
        // through. Needles are matched with comments and string literals
        // blanked, so one written in prose or in an assertion message is spaces
        // by the time it is looked for.
        //
        // The other half of the old part (5) is the compiler's and stays there:
        // `sources` is private to `domain`, `walk` is its only constructor and
        // the scan is a method on it, so a census that keeps the call, binds it
        // to `_` and scans a hard-coded list has nothing to call the scan on and
        // does not compile.
        let this_file = fs::read_to_string(repo_root().join("src/effects/tests/source_oracles.rs"))
            .expect("this file");
        let body = item_body(&this_file, "fn site_row_mappings_have_no_wildcard_arm");
        // Non-vacuity, both directions: `offenders` is code inside the census,
        // and `sole_present` is code inside the walk and inside part (2) above,
        // so its absence is what says the extractor stopped at the census's own
        // closing brace instead of running on through the file.
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
        // And the needle set can fire at all: this witness's own body reads a
        // file, and the same extractor over the same needles finds it. Without
        // this, six needles that matched nothing would read exactly like six
        // needles that found nothing to match.
        let own = item_body(
            &this_file,
            "fn the_row_mapping_census_domain_is_the_declared_module",
        );
        assert!(
            READERS.iter().any(|reader| own.contains(reader)),
            "control: the needle set cannot detect a file read even in a body that does one"
        );
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
    /// rule, `path.file_stem() == "tests"`. That covers the entries of
    /// [`WHOLE_FILE_TEST_MODULES`] whose file stem is `tests` — the ones a
    /// literal `#[cfg(test)] mod tests;` declares. The crate declares six
    /// more, and they are exactly the ones a census is most likely to trip over
    /// — a scaffold, a fake, a fixture and a readiness protocol exist to *name*
    /// what production names, and `scaffold.rs` sits inside the
    /// `engine/topology` domain one of those censuses walks.
    ///
    /// `agent/proc/test_support/readiness.rs` is the last of them and the one no
    /// **text** rule finds at all: it is declared `pub(crate) mod readiness;`,
    /// with no attribute of its own, inside `proc`'s inline `#[cfg(test)]
    /// pub(crate) mod test_support { … }`. Nothing in that file is
    /// `#[cfg(test)]`, so a census that did not skip it would read 500 lines of
    /// fixture — five denied effect calls among them — as production.
    ///
    /// **Every module named, not counted.** A count alone passes when the
    /// derivation swaps one file for another — same cardinality, different set
    /// — and names alone pass when it grows one more nobody looked at.
    /// [`WHOLE_FILE_TEST_MODULES`] is the census domain as a list of paths, so
    /// comparing against it refuses both, and refuses them by naming the file
    /// gained or lost rather than by printing two integers.
    ///
    /// **It pins identity, not independence.** What compares against that list
    /// is one resolver read two ways and a file-name rule, not three separate
    /// derivations, and a declaration form the resolver cannot see is missing
    /// from all of them at once. The comment on the second half of the body
    /// says which is which and names the form this scan misses.
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
        // **Identity is a `Path`, never a string.** `CODING_STANDARDS.md` §8:
        // a lossy display string is for diagnostics only, never identity.
        // `to_string_lossy` maps two distinct non-UTF-8 paths onto one, and
        // rewriting `\` to `/` turns a backslash -- a legal character in a Unix
        // file name -- into what reads as a separator, so an equality over the
        // rendered text can answer about a file that is not the one on disk.
        // `Path` compares component-wise and Windows accepts either separator,
        // so the forward-slash literals of `WHOLE_FILE_TEST_MODULES` match a
        // native path with no conversion at all. The one lossy rendering left
        // below is `declaring`, which is interpolated into a message and
        // compared with nothing; do not fold the two back together.
        fn relative<'a>(root: &std::path::Path, path: &'a std::path::Path) -> &'a std::path::Path {
            path.strip_prefix(root).unwrap_or(path)
        }
        // Both sides of every comparison are sorted the same way, so no
        // comparison depends on the order the list happens to be written in and
        // a failure names the offending file rather than printing two integers.
        // Sorting both is what makes that safe: `Path` orders component-wise,
        // which ranks `a.rs` after `a/b.rs` where a byte sort of the same text
        // does the reverse. No pair of entries differs that way *today*; a
        // slice adding one that did would otherwise fail here on ordering while
        // the set was right. A `Vec` rather than a set, so a duplicated entry
        // fails instead of being absorbed.
        fn sorted(mut paths: Vec<&std::path::Path>) -> Vec<&std::path::Path> {
            paths.sort_unstable();
            paths
        }
        // The expected value, and the two halves of it the rules below
        // partition into. `WHOLE_FILE_TEST_MODULES` holds `PathBuf`s relative
        // to `src`, which is what `relative` produces, so this borrows them as
        // paths and converts nothing. Filtering a sorted list keeps it sorted,
        // so the halves need no second sort.
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

        // **The two halves of `WHOLE_FILE_TEST_MODULES`, separated.** The
        // comparison above is satisfied by any derivation that reaches this
        // set; these two say *how* each file was reached, which is the part the
        // structural scan changed. The `tests.rs` half comes from a literal
        // `#[cfg(test)] mod tests;` — the form a text rule could find — and
        // the derivation must still find all of those after learning to read
        // structure, because a scan that resolved ancestry and lost the plain
        // case would trade one blind spot for another.
        //
        // **What reading one list buys, and what it does not.** These four
        // comparisons are not four independent derivations, and this test does
        // not claim they are. `whole_file_test_modules` calls
        // `declared_whole_file_test_modules` and resolves each declaration it
        // returns to a file, so `resolved` above and `declared` below are two
        // views over **one** resolver; `named` and `literal` are those same two
        // views filtered, one by the file-name rule `file_stem == "tests"` and
        // one by the declaration form. The file-name rule is the only genuinely
        // separate derivation here. What the list buys is identity — a module
        // swapped for another, renamed, added or dropped fails a comparison by
        // name rather than by cardinality — and what comparing the two halves
        // buys is the disagreement between the file-name rule and the
        // declaration form: a file called `tests.rs` that no literal
        // declaration reaches, a literal declaration resolving to a file not
        // called `tests.rs`, or a `tests.rs` whose declaration is guarded by
        // something narrower than `test` — present on the file-name side,
        // absent from the declaration side, and the one of the three that
        // silently costs a platform its test module — splits the halves apart
        // and fails.
        //
        // **The blind spot is shared, and it is not hypothetical.** A
        // declaration form the resolver cannot see is missing from all of this
        // at once — from both views, from both filters, and from the list,
        // which is maintained by hand from the same derivation. PR #101's
        // reviewer produced one and it reproduces: `#[cfg_attr(all(),
        // cfg(test))] mod hidden_tests;` is applied by rustc as `#[cfg(test)]`,
        // but `census_domain::scan_module_declarations` treats a `cfg_attr` as
        // significant only when it contains `path`, so it reads that
        // declaration as unconditional and omits the file. It is a stated limit
        // of the domain, recorded on
        // `census_domain::declared_whole_file_test_modules`. The gap predates
        // this change; widening the scan is its own change with its own review.
        //
        // Reading one expected value from the list is therefore not duplication
        // to restore. Writing the population out four times would add no
        // derivation and close no blind spot, and it would restore what PR
        // #97's review found: the same two counts were stated as English words
        // 37 times across ten files, so one slice adding a module falsified
        // every one of them at once while the `>=` floor stayed green.
        let declarations =
            crate::effects::census_domain::declared_whole_file_test_modules(&root, &files);
        // Each declaration named by the file it resolves to, through the shared
        // resolver rather than a second copy of the rule
        // (`PR5D-VISIBILITY-CHECK-DUPLICATED`), so this comparison is in the
        // same terms as the list.
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
        // **Membership is the declaration form, guard included.** This read
        // only the name and the inline path until PR #101's second pass, which
        // is a rule that never looked at `guard` though it is right there:
        // `#[cfg(all(test, unix))] mod tests;` still entails `test`, still
        // resolves to a file called `tests.rs`, and passed as the plain form
        // while Windows compiled no such module at all. The rule is
        // `is_the_literal_mod_tests_form`, shared with the fixture that drives
        // it over synthetic input, because a second copy here is the defect
        // `PR5D-VISIBILITY-CHECK-DUPLICATED` names.
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
        // Diagnostics, not identity: this names the files those declarations
        // were *written in*, for the failure message, and is compared with
        // nothing. §8 allows a lossy rendering here and only here.
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
        // And the one that is reached only through an inline ancestor, named
        // with the ancestry it was reached through. This is the whole of what
        // the structural scan buys, so it is asserted as a value rather than as
        // a count.
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

    /// [`production_code`] removes the item and keeps the file.
    ///
    /// Every shape here is one this tree actually contains, and each is a way a
    /// truncating region loses production code. The censuses that use this helper
    /// count over the whole tree, so a shape it mishandles is a hole nobody would
    /// see: the count would simply be lower.
    pub(in crate::effects::tests) fn the_configured_item_is_removed_and_the_rest_kept() {
        // A `mod tests;` declaration. The files that declare a whole-file
        // test module named `tests` — the `tests.rs` entries of
        // `WHOLE_FILE_TEST_MODULES` — end with one, and everything below it
        // used to be outside every region that truncates.
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

    /// Typed test wrappers must disappear without hiding later production.
    /// A return-type comma is not a field separator; a function-pointer field's
    /// comma still is. Incomplete items retain their bodies for the census.
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
