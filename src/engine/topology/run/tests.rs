//! Extended notes: `docs/internals/engine/topology/run/tests.md`

use super::*;
use crate::topology::events::{AttemptNumber, DerivedOutcome, GenerationId};
use crate::topology::registry::TaskKey;

#[test]
fn the_transcribed_loop_branches_are_the_packets_seven() {
    assert_eq!(
        LoopBranch::ALL
            .iter()
            .map(|branch| branch.label())
            .collect::<Vec<_>>(),
        vec![
            "ingest answers",
            "integration",
            "ready_retry",
            "ready dispatch",
            "defer backoff",
            "hard block",
            "run-end closure",
        ],
        "transcribed from `decisions.sequential_substrate.loop`, in its order"
    );
}

#[test]
fn every_branch_states_what_this_build_does_with_it() {
    let refused: Vec<&str> = LoopBranch::ALL
        .iter()
        .filter(|branch| branch.disposition() == Disposition::RefusedByCheckpoint)
        .map(|branch| branch.label())
        .collect();
    assert_eq!(
        refused,
        vec!["integration", "run-end closure"],
        "`checkpoint_refusals` names exactly these two for PR7: \"integration \
         and run end beyond refusal\". A third refusal here is a build refusing \
         something the packet did not let it refuse"
    );

    let owed: Vec<&str> = LoopBranch::ALL
        .iter()
        .filter(|branch| branch.disposition() == Disposition::NotYetImplemented)
        .map(|branch| branch.label())
        .collect();
    assert!(
        owed.is_empty(),
        "the branches this build has not written. Every one of them is carried \
         in the type so that no instrument here has to notice its absence. \
         `defer backoff` left this list when `TopologyRun::step` grew its arm, \
         which is the shape every entry here is expected to leave by. It is \
         empty now: {owed:?}"
    );

    let elsewhere: Vec<&str> = LoopBranch::ALL
        .iter()
        .filter(|branch| matches!(branch.disposition(), Disposition::NotThisSlice { .. }))
        .map(|branch| branch.label())
        .collect();
    assert_eq!(
        elsewhere,
        vec!["ingest answers"],
        "a branch left this build's scope without saying which slice took it"
    );

    assert_eq!(
        LoopBranch::ReadyDispatch.disposition(),
        Disposition::Performed,
        "`loop` states this branch as four clauses and this build performs \
         three; the type says which three"
    );
}

#[test]
fn every_step_belongs_to_one_branch_or_to_none_for_a_reason() {
    let cases: Vec<(Step, Option<LoopBranch>)> = vec![
        (Step::Poisoned, None),
        (
            Step::Retry {
                key: TaskKey(0),
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
            },
            Some(LoopBranch::ReadyRetry),
        ),
        (
            Step::Dispatch {
                key: TaskKey(0),
                generation: GenerationId(0),
                continuing: false,
            },
            Some(LoopBranch::ReadyDispatch),
        ),
        (Step::Backoff, Some(LoopBranch::DeferBackoff)),
        (
            Step::HardBlock {
                questions: Vec::new(),
            },
            Some(LoopBranch::HardBlock),
        ),
        (
            Step::Closure(DerivedOutcome::NotEnding),
            Some(LoopBranch::Closure),
        ),
    ];
    for (step, expected) in cases {
        assert_eq!(
            LoopBranch::of(&step),
            expected,
            "`{step:?}` maps to the wrong branch"
        );
    }
}

#[test]
fn a_refusal_names_the_branch_and_says_whether_anything_happened() {
    let untouched = LoopBranch::HardBlock.unimplemented().to_string();
    assert!(
        untouched.contains("hard block"),
        "the refusal names the branch: {untouched}"
    );
    assert!(
        untouched.contains("no effect was performed")
            && untouched.contains("no event was appended"),
        "and says the run is untouched: {untouched}"
    );

    assert!(
        LoopBranch::ALL
            .iter()
            .all(|branch| !matches!(branch.disposition(), Disposition::PartlyImplemented { .. })),
        "a branch is partly built again — assert its `performed ... does not ...` \
         message here, because an operator reading `not implemented` would look \
         for a run that had not started"
    );

    for branch in LoopBranch::ALL {
        let refusal = branch.unimplemented().to_string();
        assert!(
            refusal.contains(branch.label()),
            "`{}`'s refusal does not name it: {refusal}",
            branch.label()
        );
    }
}

#[test]
fn every_driver_append_propagates_its_error() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine/topology/run.rs"),
    )
    .expect("the driver's own source");
    let code = crate::effects::production_code(&source);

    assert!(
        code.len() * 10 > source.len(),
        "the production region is {} of {} bytes — a census over a fraction of a \
         file reports zero for the part it never read",
        code.len(),
        source.len()
    );

    let needle = "self.emit(";
    let mut sites = 0;
    let mut unpropagated = Vec::new();
    for (at, _) in code.match_indices(needle) {
        sites += 1;
        let mut depth = 0_i32;
        let mut end = None;
        for (offset, ch) in code[at + needle.len() - 1..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(at + needle.len() - 1 + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            unpropagated.push(format!("unbalanced call at byte {at}"));
            continue;
        };
        if !code[end..].trim_start().starts_with('?') {
            let line = code[..at].matches('\n').count() + 1;
            unpropagated.push(format!("line {line} (of the blanked region)"));
        }
    }

    assert!(
        sites >= 4,
        "only {sites} append sites found, so a green result here would prove nothing"
    );
    assert!(
        unpropagated.is_empty(),
        "these driver appends do not propagate their error, so the append-error \
         protocol never runs for them: {unpropagated:?}"
    );
}

#[test]
fn the_loop_selects_through_one_function() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine/topology/run.rs"),
    )
    .expect("the driver's own source");
    let code = crate::effects::production_code(&source);

    assert!(
        code.len() * 10 > source.len(),
        "the production region is {} of {} bytes",
        code.len(),
        source.len()
    );

    let calls = |needle: &str| {
        code.match_indices(needle)
            .filter(|(at, _)| !code[..*at].trim_end().ends_with("fn"))
            .count()
    };

    assert_eq!(
        calls("select("),
        1,
        "the driver reaches its branch order through {} calls to `select`. Zero \
         means a second selector was written and this one bypassed — the branch \
         order the packet specifies is then not the order the run takes, and \
         `select`'s own tests still pass",
        calls("select(")
    );
    assert_eq!(
        calls("checkpoint("),
        1,
        "`checkpoint` refuses the terminals this build does not implement. One \
         selector guarded by one checkpoint is the pair; a selected step that \
         reached the loop unguarded is `INV-07`'s failure"
    );
}

#[test]
fn the_frozen_pool_table_is_read_through_one_seam() {
    const FILE: &str = "src/engine/assembly.rs";

    let source =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE))
            .expect("a source file");
    let code = crate::effects::production_code(&source);
    assert!(
        code.len() * 2 > source.len(),
        "the production region of {FILE} is {} of {} bytes, so a count over it says little about \
         the file",
        code.len(),
        source.len()
    );

    use crate::effects::census_domain::{Call, production_calls};

    let calls = production_calls(&code, "pool_for", Call::Free);
    assert_eq!(
        calls, 1,
        "{FILE} resolves an agent's pool from the frozen table in {calls} places. One is \
         `AttemptPlans::pool_for`, which is the seam every caller is supposed to ask; a second is \
         a rule with two implementations, and `wrong_internal_assumption` is how this project \
         pays for those"
    );

    assert_eq!(
        production_calls(
            "use crate::capacity::pool_for;\nfn second() { pool_for(agent, pools); }\n",
            "pool_for",
            Call::Free,
        ),
        1,
        "the needle this census reads {FILE} with does not see a bare `pool_for(` behind a \
         `use`, which is how a second implementation is ordinarily written"
    );
    assert_eq!(
        production_calls(
            "fn asks() { self.pool_for(agent); }\n",
            "pool_for",
            Call::Free
        ),
        0,
        "the needle counts the seam's own callers, so every caller asking correctly would be \
         reported as a second implementation"
    );
}

#[test]
fn both_attempt_started_arms_take_their_pool_from_an_authority() {
    const SITES: &[(&str, &str)] = &[
        (
            "src/engine/topology/attempt.rs",
            "the dispatch arm: `plan.pool`, resolved by the assembler that owns the pool table",
        ),
        (
            "src/engine/topology/settle.rs",
            "the retry arm: `request.pool`, which the driver fills from `AttemptPlans::pool_for` \
             — the same authority, asked one step earlier",
        ),
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut invented: Vec<String> = Vec::new();
    let mut checked = 0_usize;
    for (file, why) in SITES {
        let source = std::fs::read_to_string(root.join(file)).expect("a source file");
        let code = crate::effects::production_code(&source);
        let at = code
            .find("AttemptStarted4 {")
            .unwrap_or_else(|| panic!("{file} no longer constructs an `AttemptStarted4`"));
        let rest = &code[at..];
        let body = &rest[..rest.find("})").unwrap_or(rest.len())];
        let pool = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("pool:"))
            .unwrap_or_else(|| panic!("{file}'s `AttemptStarted4` has no `pool` field"));
        checked += 1;
        if pool.trim().starts_with("None") {
            invented.push(format!("{file} — {why}"));
        }
    }

    assert_eq!(checked, SITES.len(), "a site stopped being found");
    assert!(
        invented.is_empty(),
        "these append `attempt_started` with a hard-coded `pool: None`, so the ledger and the \
         plan disagree about which pool the attempt drained: {invented:?}"
    );
}

/// `PR160-NOTES-SUCCESS-SETTLEMENT`. The `Settled {` notes summarised the
/// ready-dispatch branch as ending in `attempt_finished`, which is only the
/// rejected half. An accepted settlement never appends `attempt_finished` at
/// all: [`super::TopologyRun::promote_candidate`] runs instead and the branch
/// ends at `candidate_prepared` then `task_candidate_created`, the rule
/// `design/15_design_event_log_resume_run_layout.md` and
/// `design/26_design_merge_queue_protocol.md` state and the fold enforces.
/// Pin the two settlements separately so a reader reconstructing a successful
/// attempt's durable record is not sent looking for an event that is never
/// written; `src/export.rs` and `agent/proc/tests.rs` pin prose the same way.
#[test]
fn the_settled_notes_separate_the_successful_and_the_failed_settlement() {
    const NOTES: &str = include_str!("../../../../docs/internals/engine/topology/run.md");

    let settled = NOTES
        .split("\n## ")
        .find(|section| section.starts_with("`pub enum Progress` › `Settled {`"))
        .expect("the notes carry the `Settled {` heading the branch summary sits under");
    // Match on the prose, not on where its line breaks fall: a reflow must not
    // break the pin, only a changed claim.
    let settled = settled.split_whitespace().collect::<Vec<_>>().join(" ");

    for (proposition, pin) in [
        (
            "which settlement is appended depends on `accepted`",
            "depends on `accepted`",
        ),
        (
            "a rejected attempt settles with `attempt_finished`",
            "rejected attempt ends at `attempt_finished`",
        ),
        (
            "an accepted attempt appends no `attempt_finished`",
            "never appends `attempt_finished`",
        ),
        (
            "an accepted attempt settles at `candidate_prepared`",
            "`candidate_prepared`",
        ),
        (
            "and `task_candidate_created` follows it",
            "`task_candidate_created`",
        ),
    ] {
        assert!(
            settled.contains(pin),
            "the `Settled {{` summary must state that {proposition}; looked for {pin:?} in:\n{settled}"
        );
    }

    assert!(
        !settled.contains("the attempt through the Runner, and `attempt_finished`."),
        "the retired claim that the whole ready-dispatch branch ends in \
         `attempt_finished` must not come back:\n{settled}"
    );
}
