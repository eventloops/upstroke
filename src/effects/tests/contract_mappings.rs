//! Extended notes: `docs/internals/effects/tests/contract_mappings.md`

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

    fn defining_test_sites(name: &str) -> Vec<String> {
        let needle = format!("fn {name}(");
        let mut sites = Vec::new();
        for (path, source) in scanned_sources() {
            let code = blank_comments_and_strings(&source);
            let Some(index) = code.find(&needle) else {
                continue;
            };
            let preceding = &code[index.saturating_sub(400)..index];
            if preceding.contains("#[test]") {
                sites.push(path);
            }
        }
        sites
    }

    pub(in crate::effects::tests) fn every_fault_row_name_is_a_test_in_the_tree() {
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

        assert!(
            defining_test_sites("a_test_this_tree_does_not_contain_and_never_will").is_empty(),
            "the predicate finds a test that does not exist, so its `absent` list means nothing"
        );

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

    pub(in crate::effects::tests) fn the_presence_predicate_refuses_a_non_test_shape() {
        let name = "concurrent_reclaimers_converge";
        let needle = format!("fn {name}(");

        let accepted = format!("#[test]\nfn {name}() {{ assert!(true); }}\n");
        let code = blank_comments_and_strings(&accepted);
        assert!(code.contains(&needle) && code.contains("#[test]"));

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

    pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {
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

        assert_eq!(ST16_VARIANTS.len(), 12);
        let letters: Vec<char> = ST16_VARIANTS.iter().map(|(letter, ..)| *letter).collect();
        assert_eq!(
            letters,
            ('a'..='l').collect::<Vec<char>>(),
            "the variants are not (a) through (l), in order and complete"
        );

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

        for (letter, _, test) in &ST16_VARIANTS {
            if T_CONTAINER_TESTS.contains(test) {
                continue;
            }
            assert!(
                matches!(letter, 'a' | 'b' | 'i'),
                "ST-16 ({letter}) is mapped to `{test}`, which the packet's own `test:` field does \
                 not name; only the variants whose clause is split across tests may do that"
            );
        }
    }
}
