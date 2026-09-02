//! The **contract mappings**: the three enumerations a frozen packet and its
//! reconciliation state, resolved against the tree that is supposed to satisfy
//! them.
//!
//! Four transcribed tables and the one census that answers all four.
//! `T_CONTAINER_TESTS` is `transaction_fault_matrix` row
//! `T-CONTAINER`'s `test:` field; `PR6_REFUSALS`, `ST16_VARIANTS` and
//! `PR6_CLAUSES` are the three mappings the PR6 reconciliation said it had
//! supplied and had not. None of them is in this repository, so each is a
//! literal here for the reason `policy.rs` gives about
//! [`crate::effects::tests::policy::PACKET_PRIMITIVES`]: the no-self-oracle
//! rule requires the expected values to come from the document's own text
//! rather than from the tree they are checked against.
//!
//! What resolves them is `defining_test_sites`, and it is deliberately one
//! predicate rather than two. Both bodies ask the same question of a name --
//! is it a `#[test]` function in real code, over source with comments and
//! string literals blanked -- and a second implementation of that question is
//! the shape `PR5D-VISIBILITY-CHECK-DUPLICATED` names: two hand-maintained
//! answers, one of which can break while the other stays green.
//!
//! Everything they read with stays where it was. The tree reader
//! (`scanned_sources`) is `super`'s and the blanker is `crate::effects`'. This
//! file consumes them and re-derives neither.
//!
//! **What is preserved here and not repaired.** `defining_test_sites` accepts a
//! `#[test]` anywhere in the 400 bytes preceding a signature, so a test
//! attribute far enough above a *different* function's signature is accepted;
//! the window is carried across byte-for-byte, because widening or narrowing it
//! changes what the two gates above accept and this is a move. The presence
//! boundary each gate states in its own words -- that a test with the right
//! name and a tautological body satisfies it completely -- is likewise the
//! docs' own, unedited.
//!
//! **No name here is a test name.** The three `#[test]` wrappers stay in
//! `super` under the harness names the contract, CI and `--list` know, and the
//! three functions below are deliberately named otherwise -- so `--list` over
//! the test binary is unchanged and nothing nests under
//! `effects::tests::contract_mappings`.
//!
//! **The R19 view-directory gate deliberately did not come with them**, and
//! that is what fixes this boundary. It is a mapping test by shape, but it
//! constructs a `ContainerName` to drive the mount side and the census side
//! against each other, and that identifier is one of the five needles
//! `runner::container::resolve::tests::no_module_outside_the_container_runner_writes_a_container_intent`
//! counts. That census scans the **whole** file rather than a production
//! region -- an inline `cfg(test)` module does not close it -- and it excludes
//! `src/effects/tests.rs` by exact path, with the exclusion's reason naming
//! that very test. A child holding it would need a second exclusion there,
//! which is a change to another slice's census rather than a consequence of
//! moving a declaration. So it stays with the harness: the same cut, for the
//! same reason, that left the effectful build helpers out of `policy.rs` and
//! the three regeneration writes out of `artifacts.rs`.
//!
//! Measured rather than argued, for the three tables that did move: none of
//! them names `ContainerIntent`, `ContainerName`, `containers_dir`,
//! `CONTAINERS_DIR` or the funnel's `write_intent`, in code or in prose, so
//! that census's domain answers exactly what it answered before.
//!
//! # Why the bodies sit inside a `cfg(test)` module
//!
//! The reason `classification.rs` records. A file reached by a plain `mod`
//! declaration is inside every whole-tree census's domain, and the tables below
//! are dense with names those censuses read: nineteen plus twenty-eight test
//! identifiers, several of them container-substrate names. The inline module
//! closes it for both of the repository's source cutters at once --
//! [`crate::effects::production_region`] truncates at the first `#[cfg(test)]`
//! and [`crate::effects::production_code`] excises the item that attribute
//! attaches to -- so none of those names is in any census's region and this
//! file reads as the test logic it is.
//!
//! It does so **without moving the whole-file module census**.
//! `census_domain::declared_whole_file_test_modules` derives a skip only from a
//! **terminated** declaration -- `mod name;` -- and an inline module with a
//! body opens a scope the scan reads declarations *inside* rather than naming a
//! file of its own. So
//! `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
//! still resolves `cfg::WHOLE_FILE_TEST_MODULES` and no pinned test is renamed.
//!
//! That terminated form is deliberately not spelled out here, for the reason
//! `policy.rs` gives: one written inside a comment is the exact shape that once
//! derived a phantom skip and removed a real file from every census below it,
//! and the blanking that now defeats it is not a reason to write another.
//!
//! The `#![deny]` below deliberately stays **above** the cut. Blanking takes
//! the prose, so that attribute is all three whole-tree walks' per-file "this
//! region is empty" control has left to count here -- and a region that
//! collapses to nothing is exactly what that control exists to catch. It also
//! keeps `every_production_region_that_stops_early_stops_at_a_module` answering
//! what it answered: the first `#[cfg(test)]` in this file attaches to a
//! module, which is what that region is for.
//!
//! The three effect denials are **restored** rather than inherited. `super`
//! allows them because it drives a compiler over fixtures it creates; nothing
//! in this file does -- both bodies read the tree and write nothing -- so the
//! allowance has no business reaching it. That is also what keeps this module
//! out of `effects/allowlist.toml`: an allowance is what that file records, and
//! this module takes none.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

#[cfg(test)]
pub(super) mod mappings {
    use std::collections::BTreeSet;

    use crate::effects::blank_comments_and_strings;
    use crate::effects::tests::scanned_sources;

    // -----------------------------------------------------------------------
    // The T-CONTAINER mechanical checklist
    // -----------------------------------------------------------------------

    /// The nineteen tests `transaction_fault_matrix` row `T-CONTAINER` names in its
    /// `test:` field, transcribed from the frozen packet.
    ///
    /// **Transcribed, not read.** The packet is not in this repository, so the list
    /// is a literal here the way
    /// [`PACKET_PRIMITIVES`](crate::effects::tests::policy::PACKET_PRIMITIVES)
    /// is — the no-self-oracle rule requires the expected values to come from the
    /// packet's text rather than from the tree, and a literal is the only shape
    /// that survives into CI.
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
    pub(in crate::effects::tests) fn every_fault_row_name_is_a_test_in_the_tree() {
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
    pub(in crate::effects::tests) fn the_presence_predicate_refuses_a_non_test_shape() {
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

    // -----------------------------------------------------------------------
    // R3b: the enumerations the reconciliation promised and did not supply
    // -----------------------------------------------------------------------

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
    pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {
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
}
