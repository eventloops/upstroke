//! The loop's branches, checked against the packet's list rather than against
//! the implementation.

use super::*;
use crate::topology::events::{AttemptNumber, DerivedOutcome, GenerationId};
use crate::topology::registry::TaskKey;

/// The transcribed list is the packet's list — seven branches, these labels, in
/// this order.
///
/// `decisions.sequential_substrate.loop` names them in one sentence, split on
/// `->`. A branch dropped from [`LoopBranch::ALL`] would make every other test
/// in this file pass by asking for less, which is exactly how step (g) survived
/// two review rounds in `recover.rs`.
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

/// Every branch this build does not perform says which, and why, in the type.
///
/// **The point of this test is the third disposition.** `RefusedByCheckpoint`
/// is a decision the packet licenses; `NotYetImplemented` is debt. A build that
/// conflated them would be indistinguishable from one that had quietly dropped
/// a branch — and "quietly dropped a branch" is the defect this whole module
/// exists because of.
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

    // And the debt, named rather than implied. This assertion is expected to
    // shrink as branches land; it must never grow.
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

    // **What is another slice's, cited rather than owed.** `ingest answers` is
    // not debt and is not a checkpoint refusal — the packet authorises exactly
    // two of those — so it carries the contract passage that assigns it.
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

    // The half-built one, and both halves in the branch's own words. A branch
    // that performs a durable append and reports `NotYetImplemented` would be
    // claiming the log is untouched when it is not; one that reported
    // `Performed` would be claiming an attempt ran.
    assert_eq!(
        LoopBranch::ReadyDispatch.disposition(),
        Disposition::Performed,
        "`loop` states this branch as four clauses and this build performs \
         three; the type says which three"
    );
}

/// Every `Step` a selection can produce maps to exactly one branch, or to none
/// for a stated reason.
///
/// The mapping is total by construction — `LoopBranch::of` matches on `Step`
/// exhaustively, so a new variant does not compile until someone decides which
/// branch it belongs to. What this test adds is the *two `None` arms*, which a
/// compiler cannot check: they are the claim that neither is a branch of the
/// loop, and each is wrong in a different and specific way if the claim slips.
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

/// A refusal says which branch, and — this is the part that matters — whether
/// anything happened.
///
/// **The two messages must not be interchangeable.** A branch that performed
/// nothing says so, and an operator reading it knows the log is untouched. A
/// branch that appended and then stopped says what it did, because an operator
/// told "not implemented" after a durable `task_dispatched` would go looking
/// for a run directory that does not match the message.
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

    // **No branch is `PartlyImplemented` today**, and that is a statement about
    // this build rather than about the type. `ReadyRetry` was the last one and
    // became `Performed` when its second half landed. The variant stays because
    // the next branch built in halves will need it, and this assertion is what
    // says so out loud the moment one appears — a half-built branch is the one
    // shape whose refusal has to say what it already did, because by then
    // `attempt_started` or `task_dispatched` is durable.
    assert!(
        LoopBranch::ALL
            .iter()
            .all(|branch| !matches!(branch.disposition(), Disposition::PartlyImplemented { .. })),
        "a branch is partly built again — assert its `performed ... does not ...` \
         message here, because an operator reading `not implemented` would look \
         for a run that had not started"
    );

    // Every refusal names its own branch, whatever its disposition. A message
    // that named the wrong one would send an operator to the wrong lane.
    for branch in LoopBranch::ALL {
        let refusal = branch.unimplemented().to_string();
        assert!(
            refusal.contains(branch.label()),
            "`{}`'s refusal does not name it: {refusal}",
            branch.label()
        );
    }
}

/// The bytes [`crate::effects::production_code`] blanked out of `source`.
///
/// The blanker is position-preserving, so the count is also the number of
/// source positions the region no longer offers a needle. A scan is only
/// meaningful over a region where this is non-zero: a comment or a literal left
/// standing is text a substring search reads as production code.
///
/// # Panics
///
/// When the region is not its source's length. That is the other half of the
/// same contract — an offset into a region that has changed length names a
/// different line of the file the census is reporting on.
fn blanked_bytes(source: &str, code: &str) -> usize {
    assert_eq!(
        code.len(),
        source.len(),
        "a production region of {} bytes against {} of source did not blank in place, so \
         every line number derived from an offset into it names a different line",
        code.len(),
        source.len()
    );
    source
        .as_bytes()
        .iter()
        .zip(code.as_bytes())
        .filter(|(from, to)| from != to)
        .count()
}

/// The region guard the source censuses in this file share, and what replaced
/// the ratio each of them used to open with.
///
/// Each opened with `code.len() * n > source.len()`. That guard was written for
/// a **truncating** region, where a short result really does mean "a census over
/// a fraction of a file reports zero for the part it never read".
/// [`crate::effects::production_code`] does not truncate — it overwrites
/// comments, literals and `#[cfg(test)]` items with spaces and keeps every
/// newline — so `code.len() == source.len()` whatever it removed, and the ratio
/// was already true before the blanker ran. It could not tell a working blanker
/// from one that had stopped removing anything, and over unblanked source a
/// needle quoted in a doc comment is counted as a call site.
///
/// So the two halves are asserted apart: something was blanked, and enough was
/// left to scan. `CODING_STANDARDS.md` §12 requires the first of every scan and
/// the length contract of the blanker;
/// [`the_blanked_region_count_falls_to_zero_when_nothing_was_removable`] is the
/// control that this number can reach zero, which is what the ratio could not.
///
/// `retained_floor` is each census's own tolerance, carried across unchanged:
/// the unblanked remainder must exceed one `retained_floor`th of the file.
///
/// # Panics
///
/// When the region changed length, blanked nothing, or retained less than its
/// floor.
fn assert_blanked_region(file: &str, source: &str, code: &str, retained_floor: usize) {
    let blanked = blanked_bytes(source, code);
    assert!(
        blanked > 0,
        "nothing was blanked out of {file}'s {} bytes. Either the file carries no comment \
         and no literal, or the blanker has stopped removing them — and the second reads \
         exactly like a clean file to every needle below",
        source.len()
    );
    let retained = source.len() - blanked;
    assert!(
        retained * retained_floor > source.len(),
        "{retained} of {file}'s {} bytes survived blanking, under one {retained_floor}th of \
         it — a census over a fraction of a file reports zero for the part it never read",
        source.len()
    );
}

/// The blanked-region count reaches zero, which is what the ratio it replaced
/// could not.
///
/// [`assert_blanked_region`] is the guard three source censuses in this file
/// open with, and a guard that cannot fail is not a guard. These two fixtures
/// are the control in both directions: one carries a removable region of each
/// kind the region function knows, the other carries none, and the retired
/// ratio is asserted here to be satisfied by the second.
#[test]
fn the_blanked_region_count_falls_to_zero_when_nothing_was_removable() {
    const REMOVABLE: &str = "// a line comment\n\
                             /* a block comment */\n\
                             fn go() -> usize {\n\
                             let quoted = \"a string literal\";\n\
                             quoted.len()\n\
                             }\n\
                             #[cfg(test)]\n\
                             mod fixture {\n\
                             fn one() -> usize { 1 }\n\
                             }\n";
    const NOTHING_REMOVABLE: &str = "fn go() -> usize {\n\
                                     1 + 1\n\
                                     }\n";

    let removable = crate::effects::production_code(REMOVABLE);
    assert_eq!(
        removable.len(),
        REMOVABLE.len(),
        "the region function is length-preserving by contract, which is the whole reason a \
         length ratio cannot report what it removed"
    );
    assert!(
        blanked_bytes(REMOVABLE, &removable) > 0,
        "a comment, a literal and a `#[cfg(test)]` item were left standing: {removable:?}"
    );

    let nothing = crate::effects::production_code(NOTHING_REMOVABLE);
    assert_eq!(
        blanked_bytes(NOTHING_REMOVABLE, &nothing),
        0,
        "a source with nothing removable in it must count zero, or the guard the censuses \
         open with is true of every input and proves nothing: {nothing:?}"
    );
    assert!(
        nothing.len() * 10 > NOTHING_REMOVABLE.len(),
        "the ratio these censuses used to carry is satisfied by a region that blanked \
         nothing, which is why it could not stand in for a blanked-region count"
    );
}

/// **Every append the driver makes propagates its error.**
///
/// The append-error protocol is five obligations, and all five begin with the
/// error *reaching* the protocol. A `let _ = self.emit(..)` reaches none of
/// them: the fold is not poisoned, no reservation or invocation is cancelled,
/// and the command reports success for a run whose log does not contain the
/// line it just claimed to write.
///
/// Catalogue entry `PR7-SELECT-026` did exactly that to the
/// `Admitted::BudgetExceeded` arm and the whole suite stayed green, because the
/// arms whose append failure *is* armed by a fixture are not that one.
///
/// A **census rather than a fixture per arm**, for the reason the other four
/// single-authority censuses exist: a per-arm test proves the arm it names and
/// says nothing about the arm added next week. This proves the property over
/// every append site the driver has, including the ones not yet written.
///
/// The region is [`crate::effects::production_code`], which blanks comments and
/// strings — a `let _ = self.emit(` quoted in a doc comment must not fail this,
/// and a truncating region would let a site below the cut through, which is
/// `PR4-CENSUS-COMMENT-ORACLE` and is how the barrier census scanned 4.7% of
/// this very file.
///
/// The guard on that region is [`assert_blanked_region`], which counts what was
/// blanked. The length ratio this test used to open with could not: the region
/// function preserves length by contract, so the ratio held of a blanker that
/// had removed nothing and left every quoted `self.emit(` as a call site.
#[test]
fn every_driver_append_propagates_its_error() {
    const FILE: &str = "src/engine/topology/run.rs";

    let source =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE))
            .expect("the driver's own source");
    let code = crate::effects::production_code(&source);

    assert_blanked_region(FILE, &source, &code, 10);

    let needle = "self.emit(";
    let mut sites = 0;
    let mut unpropagated = Vec::new();
    for (at, _) in code.match_indices(needle) {
        sites += 1;
        // Walk to the matching close paren, then check what follows it.
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

/// **The loop chooses its branch through one selector.**
///
/// `decisions.sequential_substrate.loop` gives seven branches in one order, and
/// `select` is where that order lives. Catalogue entry `PR7-SELECT-015` added a
/// **second** selector — `select_rescan`, ordered Dispatch/Retry/Integrate
/// instead of Integrate/Retry/Dispatch — pointed `TopologyRun::step` at it, and
/// left canonical `select` untouched with every one of its tests still passing.
/// The whole suite was green.
///
/// That is the seams category in its purest form: `select.rs` is coherent,
/// `run.rs` is coherent, and the branch order the packet specifies is not the
/// one the run takes. No per-function test can see it, because each function is
/// right about itself.
///
/// The fifth single-authority census this slice owns, and the cheapest: the
/// driver reaches its branch order through exactly one call, and `checkpoint`
/// guards exactly that call's result. A second selector makes this count zero,
/// not two — which is why the assertion is on the **canonical** name rather than
/// on a total.
///
/// The region carries [`assert_blanked_region`] for the reason the append census
/// above does: the ratio both used to open with is true of a region that blanked
/// nothing, and a `select(` in a doc comment would then be counted as the call.
#[test]
fn the_loop_selects_through_one_function() {
    const FILE: &str = "src/engine/topology/run.rs";

    let source =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE))
            .expect("the driver's own source");
    let code = crate::effects::production_code(&source);

    assert_blanked_region(FILE, &source, &code, 10);

    // Calls, not definitions — neither is defined here, but the filter is the
    // one the barrier census learned to use and costs nothing.
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

/// **The frozen pool table is read through one seam.**
///
/// `AttemptPlans::pool_for` exists so that the plan builder, the reviewer
/// profile and the driver's `RetryRequest` reach one answer. `79cd9c8` said it
/// gave the rule "one production implementation" and it did not: `assembly.rs`
/// called `crate::capacity::pool_for` from three places, two of them
/// character-for-character copies of the seam's body, and the seam's only caller
/// was `run.rs`. `reviews/FINDINGS.md` §19, claim (4).
///
/// **The needle is a free call to `pool_for`**, through the shared
/// [`crate::effects::census_domain::production_calls`]. It was the literal
/// `capacity::pool_for(`, which reasons about one direction only — a longer
/// identifier colliding with it — and not about the other: `use
/// crate::capacity::pool_for;` followed by a bare `pool_for(...)` is the
/// ordinary way to write a second implementation and that literal does not
/// match it. Both spellings are already live in this tree. `R5-SEAMS-002`.
///
/// **What it still cannot see, stated rather than left to be found**: a second
/// resolution that never names the function. `capacity::pool_for` is
/// `pools.iter().find(…)`, and a caller walking `self.pools` inline is a second
/// implementation of the rule with no `pool_for` in it. A name census cannot
/// reach that, so what this asserts is **one named resolution**, not one
/// resolution.
///
/// **The count is one and not zero.** Zero would mean the seam had been rewritten
/// to resolve pools some other way, which is the same defect from the other
/// side, so the assertion is an equality.
///
/// The needle controls at the end are controls on *identifier matching*; the
/// premise underneath them — that the region was blanked at all — is
/// [`assert_blanked_region`]'s, because the ratio this census used to open with
/// held of a blanker that had removed nothing.
#[test]
fn the_frozen_pool_table_is_read_through_one_seam() {
    const FILE: &str = "src/engine/assembly.rs";

    let source =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE))
            .expect("a source file");
    let code = crate::effects::production_code(&source);
    assert_blanked_region(FILE, &source, &code, 2);

    // **Free calls to `pool_for`, not the qualified spelling.** The needle was
    // the literal `capacity::pool_for(`, which does not match the ordinary way
    // to write a second implementation — `use crate::capacity::pool_for;` and
    // then a bare `pool_for(...)`. Both idioms are live in this tree
    // (`config.rs` writes the qualified form, `capacity.rs` the bare one), so
    // it is not a hypothetical spelling. `R5-SEAMS-002`, `PR7-R5-ATT-002`.
    //
    // `Call::Free` is what separates a second implementation from the seam's
    // own callers: the plan builder and the reviewer profile ask
    // `self.pool_for(...)`, a method call, and the trait method's definition is
    // filtered as a definition.
    use crate::effects::census_domain::{Call, production_calls};

    let calls = production_calls(&code, "pool_for", Call::Free);
    assert_eq!(
        calls, 1,
        "{FILE} resolves an agent's pool from the frozen table in {calls} places. One is \
         `AttemptPlans::pool_for`, which is the seam every caller is supposed to ask; a second is \
         a rule with two implementations, and `wrong_internal_assumption` is how this project \
         pays for those"
    );

    // Controls on the needle itself, both directions, because a needle that has
    // stopped matching reads exactly like a clean file.
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

/// Every `AttemptStarted4 { … }` literal in `code`, as `(line, pool field)`.
///
/// `code` is a blanked region, so a brace inside a comment or a string literal
/// is already a space and can neither open a body nor close one.
///
/// **Every literal, and the extent is the matching brace.** The scan this
/// replaces took `.find` — the first literal in the file — and sliced it to the
/// first `})` after the needle, so a second construction site was outside the
/// domain altogether and a delimiter that happened to fit the two live literals
/// stood in for the body's real end. They do not close alike: the dispatch arm's
/// is `})?;` and the retry arm's is inside `Ok(RetryOutcome::Start(Box::new(…)))`.
/// A rule that fits both by luck fits the next one by luck too.
///
/// # Panics
///
/// When a literal's braces do not balance, or when one carries no `pool` field.
/// Both are the census losing its subject, which is not the answer "the subject
/// is clean".
fn attempt_started_pools(code: &str) -> Vec<(usize, String)> {
    const NEEDLE: &str = "AttemptStarted4 {";

    let mut found = Vec::new();
    for (at, _) in code.match_indices(NEEDLE) {
        let line = code[..at].matches('\n').count() + 1;
        let open = at + NEEDLE.len() - 1;
        let mut depth = 0_i32;
        let mut end = None;
        for (offset, ch) in code[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            panic!("the `AttemptStarted4` at line {line} never closes its body");
        };
        let pool = code[open..end]
            .lines()
            .find_map(|field| field.trim().strip_prefix("pool:"))
            .unwrap_or_else(|| panic!("the `AttemptStarted4` at line {line} has no `pool` field"));
        found.push((line, pool.trim().to_owned()));
    }
    found
}

/// **Both arms of `attempt_started` get their pool from an authority.**
///
/// `attempt_started` is appended from two places and they reach it differently:
/// the dispatch arm builds its plan first and reads `plan.pool`; the retry arm
/// appends **before** its plan exists, because `settle::retry` produces the
/// event and the plan is built after. Sol's `R3-SEAMS-001` is what that
/// asymmetry produced — the retry passed `pool: None`, so a resumed run's ledger
/// recorded no pool while the plan it then built resolved one, and the two
/// disagreed about the same attempt.
///
/// The needle is the field's value in **every** production `AttemptStarted4`
/// literal of each listed file, through [`attempt_started_pools`]. A hard-coded
/// `None` fails; anything that names something does not, because this census's
/// claim is "not invented here", not "non-empty".
///
/// **The domain is one literal per site, and that count is asserted.** The scan
/// this replaces read the first literal in each file and stopped, so a second
/// construction site — the ordinary way a third arm arrives — lay outside the
/// scanned domain while `checked == SITES.len()` still read as full coverage.
/// The control at the end of the test is that second-position violation.
///
/// # Two corrections to what this test was said to be
///
/// **It is not the only witness available, and the claim that it was is false.**
/// `79cd9c8`'s message argued a source census was structurally necessary because
/// "a retry is only reachable *within* one process … and **no driver fixture can
/// reach the arm**". One does: the fixture is
/// `recover::tests::the_retaining_incarnation_retries_in_place`, and it exists —
/// **named, not cited by line**. The first draft of this block quoted
/// `recover/tests.rs:5488` as terminal output — correct **at `c01a844`** — and the
/// very next commit inserted nineteen lines above it. `PR7-R6-ATT-003`, and
/// the rule it gives: a doc comment names an item, because a line number is a
/// claim about a version of a file and decays silently. The doc-comment filter
/// (`| grep -v '///'`) is the other half — a needle quoted here would otherwise
/// match its own quotation, `reviews/FINDINGS.md` §4.
///
/// It drives `TopologyRun::step` twice in one process and the second iteration
/// **is** the retained-generation retry. It now asserts the pool on both
/// `attempt_started` appends, which is the behavioural witness this census was
/// offered in place of. `reviews/FINDINGS.md` §19, claim (3).
///
/// **And this census does not read the file the defect was in.** The two sites
/// below are `attempt.rs` and `settle.rs`; the literal `None` that
/// `R3-SEAMS-001` found was in **`run.rs`**, which fills `settle::retry`'s
/// `RetryRequest`, and `settle.rs`'s own literal reads `request.pool` and was
/// correct throughout. Measured at `5a08f19`: restoring `pool: None` in
/// `run.rs` leaves this census green **and the entire suite green** — 1698 + 8
/// passed, 0 failed. The behavioural assertion above is what kills it. §19,
/// claim (2).
///
/// So this census keeps a real and narrower job: the two *literals* name an
/// authority rather than inventing a value. It is not a witness that the value
/// arriving at them is right.
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
        assert_blanked_region(file, &source, &code, 10);

        let literals = attempt_started_pools(&code);
        assert_eq!(
            literals.len(),
            1,
            "{file} builds {} production `AttemptStarted4` literals and this census claims \
             one arm per site. Zero means it no longer constructs one and the site has \
             moved; a second is a third arm, and it needs its own `SITES` entry naming the \
             authority it reads rather than a scan that stops at the first",
            literals.len()
        );
        for (line, pool) in literals {
            checked += 1;
            if pool.starts_with("None") {
                invented.push(format!("{file}:{line} — {why}"));
            }
        }
    }

    assert_eq!(
        checked,
        SITES.len(),
        "the per-site count above pins each file's boundary; this is the domain's size. Two \
         arms are the whole of what this census claims, and it inspected {checked} literals"
    );
    assert!(
        invented.is_empty(),
        "these append `attempt_started` with a hard-coded `pool: None`, so the ledger and the \
         plan disagree about which pool the attempt drained: {invented:?}"
    );

    // **The control the scan this replaced could not pass**, and the reason the
    // domain is now every literal rather than the first. With the first arm
    // correct, `.find` reported the file clean and the `pool: None` below it was
    // never read. `CODING_STANDARDS.md` §12: a positive control inside a
    // truncated domain does not prove the whole named domain was scanned.
    const SECOND_ARM_INVENTS_ITS_POOL: &str = "fn dispatch() {\n\
                                               let started = AttemptStarted4 {\n\
                                               pool: plan.pool.clone(),\n\
                                               };\n\
                                               }\n\
                                               fn retry() {\n\
                                               let started = AttemptStarted4 {\n\
                                               pool: None,\n\
                                               };\n\
                                               }\n";
    let control = attempt_started_pools(SECOND_ARM_INVENTS_ITS_POOL);
    assert_eq!(
        control.len(),
        2,
        "the needle reads one literal per file again, so every construction site after the \
         first is outside the domain this census reports on: {control:?}"
    );
    assert!(
        control[1].1.starts_with("None"),
        "a hard-coded `pool: None` in the second literal is what this census exists to \
         catch, and the scan did not see it: {control:?}"
    );
    assert!(
        !control[0].1.starts_with("None"),
        "the control's first literal names an authority, so reporting it would make every \
         correct arm an offender and the census's greens meaningless: {control:?}"
    );
}
