// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/workspace_manager.rs` -- the shape
// `src/runner/container/tests.rs` established for a funnel's own test module.
// This suite drives the site-taking APIs and plants the residue they are meant
// to find, so it names `fs::write`, `fs::create_dir_all`,
// `std::process::Command` and `println!` directly.
//
// `PR6-LANEF-004`: a Rust lint level is scoped by the MODULE TREE and not by
// the file, so without an attribute here the parent's inner allow would reach
// this file silently and no reviewed record would name the file doing the work.
// All three are needed and all three are measured; none is inherited.
// `decisions.effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use super::*;
// The `m4-workspace` split moved the path-hygiene predicates into a child, and
// `use super::*` reaches the parent's namespace only. `is_reparse_point` is the
// one item this suite drives that the parent does not call itself -- both
// platform arms are asserted here directly -- so it is named through the
// child's own `pub(super)` surface. No test is renamed, no assertion changes
// and no body moves; this line is the whole of what the extraction owes this
// file.
use super::containment::is_reparse_point;

use std::collections::BTreeSet;

// The repository fixture and the three Git helpers are `fixture`'s, not
// this module's: `src/engine/topology/**` needs them too and cannot reach
// an effect primitive of its own. See that module for why they moved.
use super::fixture::{Fixture, git, git_out, scratch};
// The observing scratch tree: its drop reclaims the directory and reports a
// reclaim that failed, where `scratch` above hands back a bare path nothing
// removes. The `canonical_prefix` tests below build no repository, so this is
// the fixture they take.
use crate::rundir::scratch_tree::acquire;
// The slot-component grammar, named so the run-id restatement of it can be
// held to the same verdicts.
// Named here rather than borrowed from the parent's import list. The split
// moved the items that needed them into children -- the hook observers, the
// residue classifier, the slot vocabulary and the changed-path decoder -- so
// `src/workspace_manager.rs` no longer imports these and `use super::*` no
// longer carries them. Same names, same crate paths, no new dependency.
//
// `OsStr` carries the `cfg` of its only user. It is named here for the same
// reason as the rest -- the root pruned `use std::ffi::{OsStr, OsString};` to
// `OsString` -- but its one call site is inside a `#[cfg(unix)]` test, so on
// Windows the item is compiled out and an ungated import is an `unused_imports`
// error under the guest's `-D warnings`. The gate is on the import rather than
// the call site so the moved line stays byte-identical.
#[cfg(unix)]
use std::ffi::OsStr;
use std::sync::{Arc, Mutex};

use crate::topology::effects::{
    ClassHistogram, Evidence, EvidenceLabel, HookHarness, Injection, InjectionMode, ObjectResidue,
    ResidueElement, ResourceRow, SamplingRecord, SnapshotSite, SyntheticRecord,
};
use crate::topology::paths::GitPath;

/// A harness that answers `Proceed` and records everything.
fn harness() -> (HarnessEffects, Arc<Mutex<HookHarness>>) {
    let shared = Arc::new(Mutex::new(HookHarness::new()));
    (HarnessEffects::new(Arc::clone(&shared)), shared)
}

fn refusal_of(error: &UpstrokeError) -> String {
    error.to_string()
}

/// Every site the four groups this lane owns declare, derived from the
/// enums rather than written out. A group that grows a variant grows this.
fn lane_sites() -> Vec<EffectSiteId> {
    EffectSiteId::all()
        .into_iter()
        .filter(|site| {
            matches!(
                site,
                EffectSiteId::Worktree(_)
                    | EffectSiteId::Snapshot(_)
                    | EffectSiteId::Ref(_)
                    | EffectSiteId::Object(_)
            )
        })
        .collect()
}

// -----------------------------------------------------------------------
// R18: repo_key and the execution root
// -----------------------------------------------------------------------

/// The digest is pinned against values computed **outside this program**,
/// from the packet's formula, so the function is never its own oracle.
///
/// `decisions.workspace_candidates.execution_root`: "repo_key v1 =
/// hex16(sha256('upstroke-repo-key-v1' NUL canonical common git dir bytes))".
///
/// Independently computed with:
/// `python3 -c "import hashlib;
///  print(hashlib.sha256(b'upstroke-repo-key-v1\x00' + P).hexdigest()[:16])"`
#[test]
fn the_repo_key_is_the_packets_digest_and_not_a_neighbouring_one() {
    assert_eq!(
        repo_key_v1(Path::new("/srv/upstroke/.git")),
        "fb0df4eb6c4a32c7",
        "the digest of the packet's own formula"
    );
    assert_eq!(
        repo_key_v1(Path::new("/srv/upstroke/.git/")),
        "8d01d05e9d96ec4b",
        "a trailing separator is different bytes and must be a different key"
    );
    assert_eq!(
        repo_key_v1(Path::new(r"C:\repos\upstroke\.git")),
        "a6a151c73796e709",
        "a Windows-shaped path hashes its own bytes on either platform"
    );

    // The two neighbouring formulas a transcription slip produces: the
    // domain prefix dropped, and the NUL separator dropped. Neither may be
    // what the function computes.
    assert_ne!(
        repo_key_v1(Path::new("/srv/upstroke/.git")),
        "85185d58540dc79c",
        "the NUL separator is part of the formula"
    );
    assert_ne!(
        repo_key_v1(Path::new("/srv/upstroke/.git")),
        "c2ed95c96b45a16d",
        "the domain prefix is `upstroke-repo-key-v1`"
    );

    // hex16 is sixteen hexadecimal characters, and it is a prefix of the
    // full digest rather than a fold of it.
    let key = repo_key_v1(Path::new("/srv/upstroke/.git"));
    assert_eq!(key.len(), 16);
    assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!("fb0df4eb6c4a32c75b6a8adf8ad24da6752f85b84a984a0ac8b89163527c849d".starts_with(&key));
}

#[cfg(unix)]
#[test]
fn the_repo_key_hashes_path_bytes_a_string_cannot_carry() {
    use std::os::unix::ffi::OsStringExt as _;
    let path = PathBuf::from(OsString::from_vec(b"/tmp/\xff\xfe/.git".to_vec()));
    assert_eq!(
        repo_key_v1(&path),
        "d43422329d1de7f3",
        "a repository path is bytes on Unix, and the key is over those bytes"
    );
}

/// The property no digest constant can pin: the key is taken over the
/// *common* git dir, so two linked worktrees of one repository agree and
/// two repositories differ.
#[test]
fn the_repo_key_is_the_repositorys_and_not_the_worktrees() {
    let fixture = Fixture::created("repokey-common");
    let linked = fixture.root.join("linked");
    git(
        &fixture.base,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &linked.to_string_lossy(),
            &fixture.head,
        ],
    );
    let from_linked = WorkspaceManager::derive(
        &linked,
        &fixture.private,
        "01KZSWEEP00000000000000002",
        "inc-1",
    )
    .expect("derive");
    assert_eq!(
        from_linked.repo_key(),
        fixture.manager.repo_key(),
        "a linked worktree of the same repository has the same common git dir"
    );

    let other = Fixture::new("repokey-other");
    assert_ne!(
        other.manager.repo_key(),
        fixture.manager.repo_key(),
        "a different repository has a different common git dir"
    );

    git(
        &fixture.base,
        &["worktree", "remove", "--force", &linked.to_string_lossy()],
    );
}

#[test]
fn the_execution_root_is_the_path_the_packet_names() {
    let fixture = Fixture::new("root-shape");
    // `strip_verbatim` on the canonical root, not the raw canonical root:
    // on Windows `fs::canonicalize` answers `\\?\C:\...`, which Git cannot
    // open, so the recorded root is the Win32 spelling of the same
    // directory. The expected value is built from the packet's own formula
    // — `<private_root>/workspaces/<repo_key>/<run_id>` — rather than from
    // the manager.
    let expected = strip_verbatim(
        fixture
            .private
            .canonicalize()
            .expect("canonical private root"),
    )
    .join("workspaces")
    .join(fixture.manager.repo_key())
    .join(super::fixture::RUN_ID);
    assert_eq!(fixture.manager.execution_root(), expected.as_path());
    assert!(
        !fixture
            .manager
            .execution_root()
            .to_string_lossy()
            .contains("\\\\?\\"),
        "the recorded root is a path Git can open"
    );
}

#[test]
fn the_execution_root_is_pruned_only_when_it_is_empty() {
    let fixture = Fixture::created("root-prune");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    assert!(
        !fixture
            .manager
            .remove_execution_root(&mut NoHooks)
            .expect("attempt the removal"),
        "R18 is pruned by finalization *when empty*; a live worktree is not empty"
    );
    assert!(fixture.manager.execution_root().is_dir());

    fixture
        .manager
        .remove_worktree(&mut NoHooks, &slot)
        .expect("forced removal");
    fixture
        .manager
        .remove_intent(&mut NoHooks, &slot)
        .expect("intent removal");
    assert!(
        fixture
            .manager
            .remove_execution_root(&mut NoHooks)
            .expect("remove the root"),
        "an empty root is pruned"
    );
    assert!(!fixture.manager.execution_root().exists());
}

// -----------------------------------------------------------------------
// The containment refusals — real temp repositories, one test per reason
// -----------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn a_symlink_below_the_private_root_refuses_the_execution_root() {
    let fixture = Fixture::new("symlink-chain");
    let workspaces = fixture
        .private
        .canonicalize()
        .expect("canonical")
        .join("workspaces");
    let elsewhere = fixture.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("target directory");
    std::os::unix::fs::symlink(&elsewhere, &workspaces).expect("plant the symlink");

    let error = fixture
        .manager
        .create_execution_root(&mut NoHooks)
        .expect_err("a reparse point on the chain refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point"),
        "the refusal must name its reason, not merely fail: {message}"
    );
    assert!(
        message.contains(&workspaces.display().to_string()),
        "and name the component: {message}"
    );
    assert!(
        !fixture.manager.execution_root().exists(),
        "and perform no effect"
    );
}

/// `PR28-REPLAY-FAILS-OPEN`. A misspelled site name is byte-for-byte
/// indistinguishable from "this site was deliberately left to measure", so a
/// parser that only looks for the site it wants cannot tell them apart. It
/// returned `None`, the run measured fresh, and an operator who asked to
/// replay a red run got a green one that replayed nothing.
///
/// Measured before the fix: `Object.CandidateStgae=44365` produced
/// `budget=39916us` with no `replayed` marker.
#[test]
fn a_replay_spec_naming_an_unknown_site_is_refused_rather_than_ignored() {
    let stage = EffectSiteId::Object(ObjectSite::CandidateStage);
    assert_eq!(
        parse_budget_spec("Object.CandidateStgae=44365", stage),
        Err(BudgetSpecError::UnknownSite(
            "Object.CandidateStgae".to_owned()
        )),
    );
    // The control that makes the assertion mean something: the *correctly*
    // spelled name is accepted, so the refusal is about the typo and not
    // about the parser rejecting everything.
    assert_eq!(
        parse_budget_spec("Object.CandidateStage=44365", stage),
        Ok(Some(std::time::Duration::from_micros(44365))),
    );
}

/// `PR28-REPLAY-FAILS-OPEN`, second shape. The old parser took the **first**
/// entry matching the site and then tried to parse it; a malformed first
/// entry produced `None` and a later valid duplicate was never reached.
///
/// Measured before the fix: `...=abc,...=44365` produced `budget=40971us`
/// with no `replayed` marker, while `...=44365,...=abc` replayed at 44365 —
/// so which entry won depended on order, which is reason enough to refuse
/// duplicates outright.
#[test]
fn a_duplicated_site_is_refused_rather_than_resolved_by_order() {
    let stage = EffectSiteId::Object(ObjectSite::CandidateStage);
    assert_eq!(
        parse_budget_spec(
            "Object.CandidateStage=abc,Object.CandidateStage=44365",
            stage
        ),
        Err(BudgetSpecError::NotAPositiveInteger(
            "Object.CandidateStage=abc".to_owned()
        )),
    );
    assert_eq!(
        parse_budget_spec("Object.CandidateStage=1,Object.CandidateStage=2", stage),
        Err(BudgetSpecError::Duplicate(
            "Object.CandidateStage".to_owned()
        )),
    );
}

/// `PR28-REPLAY-UNBOUNDED`. `u64::MAX` parsed happily. Measured before the
/// fix: the printed ladder's first rung asked for 2_049_638_230_412_172_119
/// microseconds — about **64,949 years** — and `kill_git_child` sleeps
/// unconditionally, so the run would have ended at CI's job timeout rather
/// than at any assertion.
#[test]
fn a_replay_budget_past_the_ceiling_is_refused() {
    let stage = EffectSiteId::Object(ObjectSite::CandidateStage);
    assert_eq!(
        parse_budget_spec("Object.CandidateStage=18446744073709551615", stage),
        Err(BudgetSpecError::AboveCeiling {
            site: "Object.CandidateStage".to_owned(),
            micros: u64::MAX,
        }),
    );
    // The boundary itself is allowed, so the ceiling is a limit and not an
    // off-by-one that silently narrows what a replay may ask for.
    assert_eq!(
        parse_budget_spec(
            &format!("Object.CandidateStage={MAX_REPLAY_BUDGET_US}"),
            stage
        ),
        Ok(Some(std::time::Duration::from_micros(MAX_REPLAY_BUDGET_US))),
    );
}

/// The legitimate partial replay this must not break: pin some sites, leave
/// the rest to measure. Witnessed by hand while building the feature — two
/// sites replayed and two measured in the same run — and pinned here so the
/// validation added above cannot quietly turn "unmentioned" into an error.
#[test]
fn a_spec_that_omits_a_site_leaves_that_site_to_be_measured() {
    assert_eq!(
        parse_budget_spec(
            "Object.CandidateStage=44365",
            EffectSiteId::Object(ObjectSite::ProposalCherryPick)
        ),
        Ok(None),
    );
    assert_eq!(
        parse_budget_spec("", EffectSiteId::Object(ObjectSite::CandidateStage)),
        Ok(None),
    );
}

/// A malformed entry is refused even when it names no site this run asks
/// about, because the operator's intent was to replay and half a spec cannot
/// deliver it.
#[test]
fn a_malformed_entry_is_refused_even_for_a_site_this_run_does_not_want() {
    assert_eq!(
        parse_budget_spec(
            "Object.ProposalCherryPick",
            EffectSiteId::Object(ObjectSite::CandidateStage)
        ),
        Err(BudgetSpecError::Malformed(
            "Object.ProposalCherryPick".to_owned()
        )),
    );
}

/// `PR28-NOTFOUND-NOT-CONVERGENCE`. A removal whose path is already gone has
/// achieved what it was asked to achieve. Before the fix this returned
/// `Err(NotFound)`, and `remove_worktree` would then skip the Git-admin
/// cleanup that follows it for a tree that was already deleted.
///
/// On Windows the sequence needs no second actor: an attempt answers
/// `ERROR_ACCESS_DENIED` for a delete-pending name, the last handle closes,
/// and the next attempt finds nothing. Verified red before the fix by a
/// scratch test on the guest, which asserted `is_err()` and passed.
#[test]
fn removing_a_path_that_is_already_gone_is_convergence_not_failure() {
    let root = scratch("already-gone");
    fs::create_dir_all(&root).expect("fixture root");
    let absent = root.join("no-such-tree");
    assert!(!absent.exists(), "the fixture must not create it");
    assert!(
        remove_tree_once_handles_close(&absent).is_ok(),
        "a path that is already gone is the outcome a removal wanted"
    );
    // And the ordinary case still works, so the arm above has not been
    // widened into "every error is success".
    let present = root.join("a-tree");
    fs::create_dir_all(present.join("nested")).expect("a tree to remove");
    assert!(remove_tree_once_handles_close(&present).is_ok());
    assert!(!present.exists(), "and it is actually gone");
    // `scratch` has no `Drop` guard, so a test that does not remove its own
    // root leaves one empty directory in the temp dir per process, forever.
    // Three had already accumulated from this test alone before it was
    // noticed -- the same leak recorded against `rundir.rs::scratch` in
    // `reviews/FINDINGS.md`, reintroduced by the test that reported it.
    fs::remove_dir_all(&root).expect("this test cleans up after itself");
}

/// Run `body`, returning its panic message if it panicked.
///
/// **The panic hook is deliberately left alone.** An earlier version took it,
/// installed a no-op and restored it afterwards, to keep intentional panics
/// out of the test output. The hook is process-global and these tests run in
/// parallel, so two of them interleaving —
/// `A` takes `H` installs `a`; `B` takes `a` installs `b`; `A` restores `H`;
/// `B` restores `a` — leaves the process running with a no-op hook for good,
/// and every later panic anywhere in the suite loses its message and
/// backtrace. Tidy output is not worth that: the noise below is a few lines,
/// and the alternative silently disarms the diagnostics of every other test.
fn panic_message(body: impl FnOnce()) -> Option<String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(body))
        .err()
        .map(|payload| {
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "<non-string panic payload>".to_owned())
        })
}

/// The refusal `sampled_command` gives for a site it does not handle. Matched
/// rather than assumed, so a panic for any *other* reason is not silently read
/// as absence — see [`has_sampling_command`].
const NO_SAMPLING_COMMAND: &str = "is not one of the four commands the contract samples";

/// Whether `sampled_command` handles `site`, distinguishing "does not handle
/// it" from "blew up for some other reason".
///
/// Treating *every* panic as absence was a real hole: an arm added to
/// `sampled_command` but omitted from `SAMPLED_SITES` — validating its slot
/// and panicking because the test supplies a task slot — read as absent, which
/// matched its absence from the list, and the drift went undetected. Measured.
fn has_sampling_command(site: EffectSiteId, fixture: &Fixture, slot: &Slot) -> bool {
    match panic_message(|| {
        let _unused = sampled_command(site, fixture, slot);
    }) {
        None => true,
        Some(message) if message.contains(NO_SAMPLING_COMMAND) => false,
        Some(message) => panic!(
            "`{}` panicked for a reason other than being unsampled, so this test cannot \
                 tell whether it has a command: {message}",
            site.name()
        ),
    }
}

/// The partition itself, over **every** real site rather than one example.
///
/// An earlier version of this test named `Object.CandidateCommitTree` alone,
/// which asserted an instance and not the property: replacing the
/// `SAMPLED_SITES` membership check with `named == CandidateCommitTree`
/// special-cased that one site, left every other unsampled site failing open,
/// and the whole suite stayed green. Measured.
#[test]
fn every_real_site_is_either_replayable_or_refused_as_unsampled() {
    let all = EffectSiteId::all();
    assert!(
        all.len() > SAMPLED_SITES.len(),
        "there must be unsampled sites, or this test asserts nothing"
    );
    for site in all {
        let spec = format!("{}=1000", site.name());
        let got = parse_budget_spec(&spec, site);
        if SAMPLED_SITES.contains(&site) {
            assert_eq!(
                got,
                Ok(Some(std::time::Duration::from_micros(1000))),
                "{} is sampled and must be replayable",
                site.name()
            );
        } else {
            assert_eq!(
                got,
                Err(BudgetSpecError::NotSampled(site.name().to_owned())),
                "{} is a real site this sampler never drives, so naming it must \
                     refuse rather than fall through to a fresh measurement",
                site.name()
            );
        }
    }
}

/// `SAMPLED_SITES` and the command table are two statements of one fact.
/// Nothing but this test makes them agree: adding an arm to `sampled_command`
/// without adding the site here would leave the new arm unreachable through a
/// replay and unobserved by any other test.
#[test]
fn the_replayable_site_list_matches_the_commands_that_exist() {
    let fixture = Fixture::created("site-list-agreement");
    let slot = fixture.task("agree", 0);
    for site in EffectSiteId::all() {
        let has_command = has_sampling_command(site, &fixture, &slot);
        assert_eq!(
            has_command,
            SAMPLED_SITES.contains(&site),
            "{} has a sampling command but is not in SAMPLED_SITES (or the reverse); \
                 the two lists have drifted",
            site.name()
        );
    }
}

/// **Every** refusal must reach the operator through the environment edge,
/// not merely the one an earlier test happened to use.
///
/// With only `UnknownSite` covered, `Err(BudgetSpecError::NotSampled(_)) => None`
/// survived: the direct parser test still saw `NotSampled`, the edge test still
/// panicked for its typo, and a real-but-unsampled spec fell through to four
/// fresh measurements. Asserting the variant name appears also pins the
/// `{error:?}` in the message, whose removal would hide the distinction the
/// two errors exist to draw.
#[test]
fn every_refusal_reaches_the_operator_through_the_environment_edge() {
    let stage = EffectSiteId::Object(ObjectSite::CandidateStage);
    for (spec, variant) in [
        ("Object.CandidateStgae=1", "UnknownSite"),
        ("Object.CandidateCommitTree=1", "NotSampled"),
        ("Object.CandidateStage", "Malformed"),
        ("Object.CandidateStage=abc", "NotAPositiveInteger"),
        ("Object.CandidateStage=99999999", "AboveCeiling"),
        (
            "Object.CandidateStage=1,Object.CandidateStage=2",
            "Duplicate",
        ),
    ] {
        let message = panic_message(|| {
            let _unused: Option<std::time::Duration> = budget_from_var(Ok(spec.to_owned()), stage);
        })
        .unwrap_or_else(|| panic!("`{spec}` must be refused at the edge, not silently measured"));
        assert!(
            message.contains(variant),
            "the refusal for `{spec}` must name {variant} so the operator can tell \
                 the refusals apart; got: {message}"
        );
    }
}

/// The repair's own blind spot. Validating against the whole `EffectSiteId`
/// registry accepted any *real* site, including the many this sampler never
/// drives — so `Object.CandidateCommitTree` parsed, matched none of the four
/// sampled sites, and every one of them fell through to a fresh measurement
/// while the run reported success.
///
/// Measured on the unfixed repair: zero `replayed` markers, four fresh
/// budgets, `test result: ok`. No mutation was needed to expose it.
#[test]
fn a_real_site_this_sampler_never_drives_is_refused() {
    let stage = EffectSiteId::Object(ObjectSite::CandidateStage);
    assert_eq!(
        parse_budget_spec("Object.CandidateCommitTree=44365", stage),
        Err(BudgetSpecError::NotSampled(
            "Object.CandidateCommitTree".to_owned()
        )),
    );
    // Every site the sampler *does* drive is still accepted, so the new check
    // narrows the domain to exactly the replayable set and no further.
    for site in SAMPLED_SITES {
        assert_eq!(
            parse_budget_spec(&format!("{}=1000", site.name()), site),
            Ok(Some(std::time::Duration::from_micros(1000))),
            "{} is sampled and must be replayable",
            site.name()
        );
    }
}

/// `std::env::var` reports `NotPresent` and `NotUnicode` as different errors.
/// Collapsing them made a spec that was set but not valid UTF-8 look exactly
/// like no spec at all — every site measuring fresh, against the adjacent
/// promise that an unhonourable spec is refused.
///
/// This tests the environment *edge* rather than the parser. All the other
/// replay tests call `parse_budget_spec` directly, so before this existed the
/// panic arm could be replaced with `Err(_) => None` and the whole suite
/// stayed green.
#[test]
fn an_absent_variable_is_absence_and_a_present_one_is_not() {
    let stage = EffectSiteId::Object(ObjectSite::CandidateStage);
    assert_eq!(
        budget_from_var(Err(std::env::VarError::NotPresent), stage),
        None,
        "no spec means measure, which is the ordinary case"
    );
    assert_eq!(
        budget_from_var(Ok("Object.CandidateStage=44365".to_owned()), stage),
        Some(std::time::Duration::from_micros(44365)),
    );
}

/// The edge must refuse a spec the parser rejects, not just an unreadable
/// variable. Without this, replacing the parse-error arm with
/// `Err(_) => None` leaves every other replay test green: they all call
/// `parse_budget_spec` directly and never exercise what the edge does with
/// its answer.
#[test]
#[should_panic(expected = "not a spec this run can honour")]
fn a_spec_the_parser_rejects_is_refused_at_the_environment_edge() {
    let _ = budget_from_var(
        Ok("Object.CandidateStgae=44365".to_owned()),
        EffectSiteId::Object(ObjectSite::CandidateStage),
    );
}

/// The other half of the edge: a value that is present but unusable must not
/// be mistaken for absence.
#[test]
#[should_panic(expected = "not valid UTF-8")]
fn a_present_but_non_unicode_variable_is_refused_not_treated_as_unset() {
    let invalid = {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            std::ffi::OsString::from_vec(vec![0x66, 0x6f, 0x80, 0x6f])
        }
        #[cfg(not(unix))]
        {
            use std::os::windows::ffi::OsStringExt as _;
            std::ffi::OsString::from_wide(&[0x66, 0x6f, 0xD800, 0x6f])
        }
    };
    let _ = budget_from_var(
        Err(std::env::VarError::NotUnicode(invalid)),
        EffectSiteId::Object(ObjectSite::CandidateStage),
    );
}

/// A process the engine killed can still be closing its handles, and on Windows
/// that makes `remove_dir_all` answer `ERROR_SHARING_VIOLATION` for a worktree
/// that is about to become removable. `remove_worktree` retries across that
/// window rather than reporting a hard `Io` failure.
///
/// This is the deterministic form of what the residue sampler hits at random:
/// the sampler kills a real `git` child at an unseeded point and *sometimes*
/// leaves a handle, so it proves the condition exists but cannot be re-run to
/// prove a fix. Here the handle is held on purpose and released on a timer.
///
/// The second half is the one that matters. A handle held **past** the retry
/// budget must still fail: a retry that waits forever is not a fix, it is a
/// hang, and one that swallows the error hides a genuinely locked worktree.
#[cfg(windows)]
#[test]
fn a_worktree_whose_killed_child_is_still_closing_is_removed_not_refused() {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::sync::mpsc;

    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    for (hold, expected_removal) in [
        (std::time::Duration::from_millis(300), true),
        (std::time::Duration::MAX, false),
    ] {
        let fixture = Fixture::created("closing-handle");
        let slot = fixture.task("alpha", 1);
        fixture
            .manager
            .write_intent(&mut NoHooks, &slot)
            .expect("the intent must be durable");
        fixture
            .manager
            .add_worktree(&mut NoHooks, &slot, &fixture.head)
            .expect("the worktree the killed child was working in");

        // A file inside the worktree, held open with the sharing mode a running
        // process has. Opened on another thread so the handle outlives this
        // statement and drops on a timer -- exactly the shape of a process that
        // has exited while its last handle is still closing.
        let target = fixture.manager.execution_root().join(slot.relative());
        let held = target.join("held-by-the-dying-child");
        fs::write(&held, b"bytes the child had open").expect("plant the file");
        let (opened, ready) = mpsc::channel();
        let holder = std::thread::spawn(move || {
            // `FILE_SHARE_READ` alone, deliberately: Rust's `File::open` asks for
            // `FILE_SHARE_DELETE` too, and a handle that shares deletion does not
            // block one -- the first version of this test removed the worktree
            // happily and proved nothing. A `git` child holds its files without
            // delete sharing, which is what makes the kernel answer
            // `ERROR_SHARING_VIOLATION`.
            let file = fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .open(&held)
                .expect("hold the file open the way a git child does");
            opened.send(()).expect("announce the handle is held");
            if hold == std::time::Duration::MAX {
                // Held for the whole test: the control that proves the retry
                // is bounded and still reports a real lock.
                std::thread::sleep(std::time::Duration::from_secs(5));
            } else {
                std::thread::sleep(hold);
            }
            drop(file);
        });
        ready
            .recv()
            .expect("the handle is held before removal is attempted");

        let outcome = fixture.manager.remove_worktree(&mut NoHooks, &slot);
        if expected_removal {
            outcome.expect("removal retries across the closing handle");
            assert!(
                !target.exists(),
                "and the worktree is actually gone, not merely un-refused"
            );
        } else {
            let error = outcome.expect_err(
                "a handle held past the retry budget is a locked worktree, not a closing one, \
                     and must be reported rather than waited on forever",
            );
            assert!(
                target.exists(),
                "and the worktree is still there, which is what the error says: {}",
                refusal_of(&error)
            );
        }
        let _ = holder.join();
    }
}

/// The Windows half of `expected_failures_refusals[0]`: a **junction** is a
/// reparse point that is not a symbolic link, so a refusal written against
/// `FileType::is_symlink` alone would pass every Linux test and refuse
/// nothing an operator can actually build with `mklink /J`.
#[cfg(windows)]
#[test]
fn a_junction_below_the_private_root_refuses_the_execution_root() {
    let fixture = Fixture::new("junction-chain");
    let workspaces = fixture
        .private
        .canonicalize()
        .expect("canonical")
        .join("workspaces");
    let elsewhere = fixture.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("target directory");
    let made = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&workspaces)
        .arg(&elsewhere)
        .output()
        .expect("run mklink");
    assert!(
        made.status.success(),
        "the fixture must really create a junction: {}",
        String::from_utf8_lossy(&made.stderr)
    );

    // The premise this test exists to hold: a junction is *not* what
    // `FileType::is_symlink` is about on every reparse tag, so the check
    // has to read the attribute.
    let metadata = fs::symlink_metadata(&workspaces).expect("junction metadata");
    assert!(
        is_reparse_point(&metadata),
        "the detector must see a junction as a reparse point"
    );

    let error = fixture
        .manager
        .create_execution_root(&mut NoHooks)
        .expect_err("a junction on the chain refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point"),
        "the refusal must name its reason: {message}"
    );
    assert!(!fixture.manager.execution_root().exists());
}

/// A directory reparse point at `link` naming `target`, on either platform.
///
/// A POSIX symlink and a Windows **junction** are the two shapes
/// `expected_failures_refusals[0]` names ("symlink/junction on the chain"),
/// and they are the two an operator can actually build: a Windows
/// *symbolic* link needs a privilege the guest's test user does not have,
/// while `mklink /J` needs none.
fn plant_directory_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("plant the symlink");
    #[cfg(windows)]
    {
        let made = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("run mklink");
        assert!(
            made.status.success(),
            "the fixture must really create a junction: {}",
            String::from_utf8_lossy(&made.stderr)
        );
    }
    let metadata = fs::symlink_metadata(link).expect("link metadata");
    // The premise, asserted rather than assumed — and it is exactly the
    // difference between the two calls this test exists to keep apart.
    assert!(
        is_reparse_point(&metadata),
        "the fixture must really be a reparse point"
    );
    assert!(
        fs::metadata(link).expect("target metadata").is_dir(),
        "and it must resolve to a real directory, or a check that followed \
             it would refuse for the wrong reason"
    );
}

/// The **leaf** of the managed base and of the private root is a chain
/// component too.
///
/// `execution_root` is "created only when the managed base is a real
/// directory with **no symlink/reparse point on the chain**", and
/// `refuse_unreal_directory` is the only check either leaf gets:
/// `reparse_point_below` walks the components *under* its anchor, and
/// `canonical_prefix` resolves a link rather than refusing it. So a leaf
/// that is a link to a real directory reaches every effect unless this
/// function reads the link itself.
///
/// `PR5-CORRECTNESS-003`: the existing coverage planted its link *below*
/// the private root, where `refuse_reparse_points` catches it, so
/// `fs::symlink_metadata` -> `fs::metadata` here survived the whole suite.
///
/// All three call sites, because the class is the call and not the
/// argument: `derive`'s base, `derive`'s private root, and `revalidate`'s
/// base — the last reached by replacing an already-derived base with a link
/// to itself, which is the sequence "every create/reclaim/delete
/// revalidates" exists for.
#[test]
fn a_managed_base_or_private_root_that_is_itself_a_link_refuses_before_any_effect() {
    let mut refused = Vec::new();

    // (1) derive's base.
    let fixture = Fixture::new("leaf-link-base");
    let real = fixture.base.canonicalize().expect("canonical base");
    let link = fixture.root.join("base-link");
    plant_directory_link(&real, &link);
    let error = WorkspaceManager::derive(
        &link,
        &fixture.private,
        "01KZSWEEP00000000000000009",
        "inc-1",
    )
    .expect_err("a managed base that is a link refuses");
    refused.push(("derive/base", refusal_of(&error)));

    // (2) derive's private root.
    let fixture = Fixture::new("leaf-link-private");
    let real = fixture.private.canonicalize().expect("canonical private");
    let link = fixture.root.join("private-link");
    plant_directory_link(&real, &link);
    let error =
        WorkspaceManager::derive(&fixture.base, &link, "01KZSWEEP00000000000000009", "inc-1")
            .expect_err("a private root that is a link refuses");
    refused.push(("derive/private-root", refusal_of(&error)));

    // (3) revalidate's base — the link arrives *after* derive succeeded.
    let fixture = Fixture::created("leaf-link-revalidate");
    let base = fixture.manager.base().to_path_buf();
    let moved = fixture.root.join("moved-away");
    fs::rename(&base, &moved).expect("move the real repository aside");
    plant_directory_link(&moved, &base);
    let error = fixture
        .manager
        .revalidate()
        .expect_err("a base that became a link refuses on revalidation");
    refused.push(("revalidate/base", refusal_of(&error)));
    // And the refusal reaches the primitives, not merely the private check.
    let slot = fixture.task("alpha", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect_err("and every primitive revalidates first");

    assert_eq!(refused.len(), 3, "three call sites, each driven");
    for (site, message) in &refused {
        assert!(
            message.contains("not a real directory"),
            "{site}: the refusal must name its reason: {message}"
        );
    }
    // Distinct paths named, so one refusal cannot stand in for three.
    let named: std::collections::BTreeSet<&str> = refused.iter().map(|(site, _)| *site).collect();
    assert_eq!(named.len(), 3, "three distinct call sites: {named:?}");
}

/// The walk's own guarantee, driven directly: a root that is not plain
/// components below the anchor — `..` in the remainder, or no common prefix
/// at all — is refused rather than walked. The walk answered "no reparse
/// point" for such a root, true of a chain it never inspected, and on Linux
/// `derive` then succeeded with `<private>/workspaces/<key>/../../../escape`
/// as its execution root. Through `derive` these shapes are now refused
/// earlier, as run ids; this pins the arm behind that one.
#[test]
fn an_execution_root_with_no_plain_chain_below_the_private_root_refuses_before_any_effect() {
    let fixture = Fixture::new("root-not-below");
    let anchor = fixture.manager.private_root().to_path_buf();
    for root in [
        anchor
            .join("workspaces")
            .join("k")
            .join("..")
            .join("..")
            .join("..")
            .join("escape"),
        fixture.root.join("escape"),
    ] {
        let error = refuse_reparse_points(&anchor, &root, Leaf::Directory)
            .expect_err("a root with no plain chain below the anchor refuses");
        let message = refusal_of(&error);
        assert!(
            message.contains("does not lie below the authorized private root"),
            "{}: the refusal must name its reason: {message}",
            root.display()
        );
    }
    assert!(
        !fixture.root.join("escape").exists() && !anchor.join("workspaces").exists(),
        "and perform no effect"
    );
}

/// A run id is one plain component, refused at `derive` before any path is
/// built. `Path::join` lets an absolute id replace the whole prefix, so an
/// absolute id naming a peer run's root aliased that root: `revalidate`
/// treated the peer's worktree as this manager's slot and `remove_worktree`
/// could have deleted its checkout and Git admin entry. `.` passed the walk
/// because `components()` folds a non-leading `.` away, aliasing the repo-key
/// directory. Every shape, on every platform.
#[test]
fn a_run_id_that_is_not_one_plain_component_is_refused_before_any_path_is_built() {
    let victim = Fixture::created("alias-victim");
    let slot = victim.add_task(&mut NoHooks, "k1", 1);
    let victim_root = victim
        .manager
        .execution_root()
        .to_str()
        .expect("a UTF-8 scratch path")
        .to_owned();
    for run_id in [
        victim_root.as_str(),
        ".",
        "..",
        "",
        "../../../escape",
        "/escape",
        "a/b",
        "-run",
        "run.",
    ] {
        let error = WorkspaceManager::derive(&victim.base, &victim.private, run_id, "inc-2")
            .expect_err("a run id that is not one plain component refuses");
        let message = refusal_of(&error);
        assert!(
            message.contains("refusing the run id"),
            "{run_id:?}: the refusal must name its reason: {message}"
        );
    }
    assert!(
        victim.manager.slot_path(&slot).is_dir(),
        "and the peer's worktree is untouched"
    );
    assert!(
        !victim.root.join("escape").exists(),
        "and nothing was built"
    );
}

/// `execution_root`: "every create/reclaim/delete revalidates". The walk
/// pushed the first child before its first `symlink_metadata`, so the private
/// root itself was never examined: renamed away after `derive` and replaced
/// by a link to an unrelated directory, every component under it was read
/// through the link and `create_execution_root` built the hierarchy under
/// the link's target. A symlink here, a junction on Windows.
#[test]
fn a_private_root_replaced_by_a_link_after_derive_refuses_every_revalidation() {
    let fixture = Fixture::new("anchor-link");
    let private = fixture.manager.private_root().to_path_buf();
    let elsewhere = fixture.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("the unrelated directory");
    let moved = fixture.root.join("private-moved");
    fs::rename(&private, &moved).expect("move the real private root aside");
    plant_directory_link(&elsewhere, &private);

    let error = fixture
        .manager
        .create_execution_root(&mut NoHooks)
        .expect_err("a private root that became a link refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("not a real directory"),
        "the refusal must name its reason: {message}"
    );
    assert!(
        !elsewhere.join("workspaces").exists() && !moved.join("workspaces").exists(),
        "and nothing is built under the link's target"
    );
}

/// The same exchange one level up: the private root is still a real
/// directory at its recorded path, but an ancestor is now a link, so the
/// recorded canonical root no longer resolves to itself. The anchored walk
/// never looks above the anchor; the canonical pin does.
#[test]
fn a_link_planted_above_the_private_root_after_derive_refuses_every_revalidation() {
    let fixture = Fixture::new("anchor-ancestor");
    let holder = fixture.root.join("holder");
    fs::create_dir_all(holder.join("private")).expect("the held private root");
    let manager = WorkspaceManager::derive(
        &fixture.base,
        &holder.join("private"),
        "01KZSWEEP00000000000000007",
        "inc-1",
    )
    .expect("derive under the holder");
    let moved = fixture.root.join("holder-moved");
    fs::rename(&holder, &moved).expect("move the holder aside");
    plant_directory_link(&moved, &holder);

    let error = manager
        .create_execution_root(&mut NoHooks)
        .expect_err("a link above the private root refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point"),
        "the refusal must name its reason: {message}"
    );
    assert!(
        !moved.join("private").join("workspaces").exists(),
        "and nothing is built through the link"
    );
}

/// A regular file where a directory of the chain should be is a broken
/// chain, reported where it stands and on every platform alike: the walk
/// reads the component's type rather than waiting for the `ENOTDIR` only
/// Unix raises one component later, and never walks past it. Walking past
/// handed the failure to the next effect, or to none: `remove_execution_root`
/// asks `exists()`, which folded the file into "nothing to remove" and
/// answered `Ok(false)` with no path named.
#[test]
fn a_regular_file_on_the_chain_is_reported_where_it_stands_and_never_as_nothing_to_remove() {
    let fixture = Fixture::new("file-on-chain");
    let file = fixture.manager.private_root().join("workspaces");
    fs::write(&file, "not a directory\n").expect("plant the file");

    let error = fixture
        .manager
        .revalidate()
        .expect_err("a file on the chain is a broken chain, not absence");
    assert!(
        matches!(
            &error,
            UpstrokeError::Io { path, source }
                if path == &file && source.kind() == std::io::ErrorKind::NotADirectory
        ),
        "the walk names the file itself, as not a directory: {error}"
    );
    fixture
        .manager
        .create_execution_root(&mut NoHooks)
        .expect_err("the create revalidates and refuses");
    fixture
        .manager
        .remove_execution_root(&mut NoHooks)
        .expect_err(
            "the removal revalidates and refuses rather than answering \"nothing to remove\"",
        );
}

/// `DESIGN.md` §15: every create, reclaim and delete revalidates inside its
/// effect funnel, after the before-hook and immediately before the effect. A
/// revalidation outside the funnel left a window: a `Before` hook — the seam
/// `a_registration_rebound_after_validation_keeps_its_admin_state` already
/// drives — that renamed the private root away and planted a link in its
/// place after the check had passed, and `create_dir_all` then built the
/// hierarchy under the link's target. The hook here does exactly that at the
/// create's `Before`, so the only check that can refuse is the one inside.
#[test]
fn a_private_root_exchanged_between_the_before_hook_and_the_effect_is_still_refused() {
    struct ExchangeAtBefore {
        private: PathBuf,
        moved: PathBuf,
        elsewhere: PathBuf,
    }

    impl EffectHooks for ExchangeAtBefore {
        fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            if site == EffectSiteId::Worktree(WorktreeSite::CreateExecutionRoot)
                && phase == HookPhase::Before
            {
                fs::rename(&self.private, &self.moved).expect("move the real private root aside");
                plant_directory_link(&self.elsewhere, &self.private);
            }
            Injection::Proceed
        }

        fn refusal_cause(&self) -> Option<String> {
            None
        }
    }

    let fixture = Fixture::new("anchor-exchanged-in-funnel");
    let elsewhere = fixture.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("the unrelated directory");
    let mut hooks = ExchangeAtBefore {
        private: fixture.manager.private_root().to_path_buf(),
        moved: fixture.root.join("private-moved"),
        elsewhere: elsewhere.clone(),
    };

    let error = fixture
        .manager
        .create_execution_root(&mut hooks)
        .expect_err("the check inside the funnel sees the exchanged root");
    let message = refusal_of(&error);
    assert!(
        message.contains("not a real directory"),
        "the refusal must name its reason: {message}"
    );
    assert!(
        !elsewhere.join("workspaces").exists() && !hooks.moved.join("workspaces").exists(),
        "and nothing is built under the link's target or the moved root"
    );
}

/// An absent managed base, or an absent private root, is "not a real
/// directory" — the refusal `execution_root` names — and not an I/O failure
/// to read it. Only an actual not-found becomes absence; a leaf that cannot
/// be read for any other reason stays an error.
#[test]
fn an_absent_managed_base_or_private_root_refuses_as_not_a_real_directory() {
    let fixture = Fixture::new("absent-leaf");
    let absent = fixture.root.join("absent");
    for (site, base, private) in [
        ("derive/base", absent.as_path(), fixture.private.as_path()),
        (
            "derive/private-root",
            fixture.base.as_path(),
            absent.as_path(),
        ),
    ] {
        let error = WorkspaceManager::derive(base, private, "01KZSWEEP00000000000000009", "inc-1")
            .expect_err("an absent leaf refuses");
        let message = refusal_of(&error);
        assert!(
            message.contains("not a real directory"),
            "{site}: the refusal must name its reason: {message}"
        );
        assert!(
            message.contains(&absent.display().to_string()),
            "{site}: and the path: {message}"
        );
    }
}

/// `canonical_prefix` peels past **absence** only. A prefix the filesystem
/// refuses to resolve for any other reason is an error, not a component to
/// rejoin lexically: the peel used to discard every `canonicalize` failure,
/// so a link loop under the root produced a "canonical" path the filesystem
/// had never verified. Evaluated on the Unix legs; a loop needs a symbolic
/// link, which the Windows guest's test user cannot create.
#[cfg(unix)]
#[test]
fn canonical_prefix_propagates_a_resolution_failure_that_is_not_absence() {
    let tree = acquire(&std::env::temp_dir(), "canonical-loop").expect("acquire a scratch tree");
    let real = tree.path().canonicalize().expect("canonical scratch");
    let link = real.join("loop");
    std::os::unix::fs::symlink(&link, &link).expect("plant the loop");
    let below = link.join("child");
    let error = canonical_prefix(&below).expect_err("a link loop is not absence");
    assert!(
        matches!(
            &error,
            UpstrokeError::Io { path, source }
                if path == &below && source.kind() != std::io::ErrorKind::NotFound
        ),
        "the error names the path that could not be resolved and its reason: {error}"
    );
}

/// A `..` below a component that does not exist has no directory to refer
/// to. POSIX cannot traverse the absent directory, so the peel meets the
/// `..`, finds no plain component left to peel, and returns that failure
/// naming the prefix it stopped at — rather than the raw path, whose lexical
/// `starts_with` would have answered "inside" for a path the filesystem
/// never resolved. Evaluated on the Unix legs; Win32 resolves the same shape
/// before the filesystem sees it, and the test below pins that answer.
#[cfg(unix)]
#[test]
fn a_dot_dot_below_an_absent_component_is_refused_rather_than_compared_lexically() {
    let tree = acquire(&std::env::temp_dir(), "canonical-dotdot").expect("acquire a scratch tree");
    let real = tree.path().canonicalize().expect("canonical scratch");
    let path = real.join("missing").join("..").join("x");
    let error = canonical_prefix(&path).expect_err("no canonical form exists");
    let stopped_at = real.join("missing").join("..");
    assert!(
        matches!(
            &error,
            UpstrokeError::Io { path, source }
                if path == &stopped_at && source.kind() == std::io::ErrorKind::NotFound
        ),
        "the error names the prefix the peel stopped at: {error}"
    );
}

/// A relative path is anchored at the current directory, so `missing` and
/// `./missing` resolve alike. The peel used to stop at the empty parent and
/// report the first component as a failed prefix, while the same path spelt
/// `./missing` peeled to `.` and resolved — two answers for one path, and a
/// public caller (`quiescence`) failed as I/O on one spelling and reached its
/// ordinary verdict on the other.
#[test]
fn canonical_prefix_anchors_a_relative_path_at_the_current_directory() {
    let first = PathBuf::from(format!("upstroke-absent-{}", std::process::id()));
    let cwd = strip_verbatim(
        std::env::current_dir()
            .expect("current directory")
            .canonicalize()
            .expect("canonical current directory"),
    );
    let expected = cwd.join(&first).join("x");
    assert_eq!(
        canonical_prefix(&first.join("x")).expect("anchored at the current directory"),
        expected
    );
    assert_eq!(
        canonical_prefix(&Path::new(".").join(&first).join("x"))
            .expect("the same path spelt with `.`"),
        expected
    );
}

/// A relative path whose first component exists and whose next does not:
/// the peel finds the existing prefix under the current directory and
/// rejoins the rest, exactly as it does for an absolute path. `src/` exists
/// at the crate root, which is where the suite runs.
#[test]
fn canonical_prefix_resolves_an_existing_relative_prefix_and_rejoins_the_rest() {
    let cwd = strip_verbatim(
        std::env::current_dir()
            .expect("current directory")
            .canonicalize()
            .expect("canonical current directory"),
    );
    let absent = format!("upstroke-absent-{}", std::process::id());
    let path = Path::new("src").join(&absent).join("x");
    assert_eq!(
        canonical_prefix(&path).expect("an existing relative prefix resolves"),
        cwd.join("src").join(&absent).join("x")
    );
}

/// An anchor that names nothing is the same refusal as one that is a file
/// or a link — at the anchor's own check, and at its canonical pin should it
/// vanish between the two, which routes absence through the same predicate.
/// That race cannot be staged from a test; the arm is decided by the rule
/// this drives at the function's edge.
#[test]
fn an_absent_anchor_refuses_as_not_a_real_directory() {
    let fixture = Fixture::new("absent-anchor");
    let anchor = fixture.root.join("never-created");
    let error = refuse_reparse_points(&anchor, &anchor.join("workspaces"), Leaf::Directory)
        .expect_err("an absent anchor refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("not a real directory"),
        "the refusal must name its reason: {message}"
    );
}

/// A run id is the canonical ULID `DESIGN.md` §15 specifies, as this crate's
/// generator spells it, and nothing else by shape: a lowercase or mixed-case
/// spelling names the same root as its uppercase twin on a case-insensitive
/// filesystem, so two managers of one root cannot be derived through a case
/// variant, and the "manager's own slot" classification cannot be reached
/// through a non-canonical id at all.
#[test]
fn a_run_id_is_the_canonical_ulid_and_a_case_variant_is_refused() {
    let fixture = Fixture::new("run-id-canonical");
    let generated = crate::ulid::ulid();
    WorkspaceManager::derive(&fixture.base, &fixture.private, &generated, "inc-1")
        .expect("a generated id is the canonical spelling");
    let lowered = generated.to_ascii_lowercase();
    let mixed: String = generated
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i % 2 == 0 {
                c.to_ascii_lowercase()
            } else {
                c
            }
        })
        .collect();
    for run_id in [
        "run1",
        "RUN1",
        lowered.as_str(),
        mixed.as_str(),
        "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
    ] {
        let error = WorkspaceManager::derive(&fixture.base, &fixture.private, run_id, "inc-1")
            .expect_err("a non-canonical run id refuses");
        let message = refusal_of(&error);
        assert!(
            message.contains("refusing the run id"),
            "{run_id:?}: the refusal must name its reason: {message}"
        );
    }
    assert!(
        !fixture.private.join("workspaces").exists(),
        "and nothing was built for any of them"
    );
}

/// The Windows half of the test above: Win32 resolves `..` lexically before
/// the filesystem sees it, so `missing\..` canonicalizes to the scratch
/// directory and the peel rejoins `x` onto it. Evaluated on `test (winguest)`.
#[cfg(windows)]
#[test]
fn windows_resolves_a_dot_dot_below_an_absent_component_before_the_filesystem_sees_it() {
    let tree = acquire(&std::env::temp_dir(), "canonical-dotdot").expect("acquire a scratch tree");
    let real = strip_verbatim(tree.path().canonicalize().expect("canonical scratch"));
    let path = real.join("missing").join("..").join("x");
    assert_eq!(
        canonical_prefix(&path).expect("Win32 resolves the `..`"),
        real.join("x")
    );
}

/// The in-funnel check walks down to the directory the effect acts in, not
/// only to the root. A non-recursive `remove_file` on `<root>/intents/<name>`
/// follows a link in its *parent*: an `intents/` exchanged at the `Before`
/// hook for a link to a victim directory holding a file of the same name
/// deleted the victim's file with every check passed. The write direction
/// and the `tasks/` direction are the two tests below.
#[test]
fn an_intents_directory_exchanged_at_the_before_hook_refuses_the_intent_removal() {
    let fixture = Fixture::created("intents-exchanged-remove");
    let slot = fixture.task("alpha", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("write the intent");
    let intent = fixture.manager.intent_path(&slot);
    let name = intent
        .file_name()
        .expect("an intent file name")
        .to_os_string();
    let victim = fixture.root.join("victim");
    fs::create_dir_all(&victim).expect("victim directory");
    let victim_file = victim.join(&name);
    fs::write(&victim_file, "not yours\n").expect("victim file");
    let intents = fixture.manager.execution_root().join("intents");
    let mut hooks = ExchangeAtBefore {
        site: slot.remove_intent_site(),
        original: intents.clone(),
        moved: fixture.root.join("intents-moved"),
        victim,
    };

    let error = fixture
        .manager
        .remove_intent(&mut hooks, &slot)
        .expect_err("the check inside the funnel sees the exchanged intents directory");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point")
            && message.contains(&intents.display().to_string()),
        "the refusal must name its reason and the substituted component: {message}"
    );
    assert!(victim_file.exists(), "the victim's file is untouched");
    assert!(
        hooks.moved.join(&name).exists(),
        "and the real intent is still where the hook moved it"
    );
}

/// The write direction: `write_intent` stages and renames inside `intents/`,
/// so an exchanged `intents/` would put the record, and its staging file, in
/// the victim directory.
#[test]
fn an_intents_directory_exchanged_at_the_before_hook_refuses_the_intent_write() {
    let fixture = Fixture::created("intents-exchanged-write");
    let slot = fixture.task("beta", 1);
    let victim = fixture.root.join("victim");
    fs::create_dir_all(&victim).expect("victim directory");
    let intents = fixture.manager.execution_root().join("intents");
    let mut hooks = ExchangeAtBefore {
        site: slot.write_intent_site(),
        original: intents.clone(),
        moved: fixture.root.join("intents-moved"),
        victim: victim.clone(),
    };

    let error = fixture
        .manager
        .write_intent(&mut hooks, &slot)
        .expect_err("the check inside the funnel sees the exchanged intents directory");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point")
            && message.contains(&intents.display().to_string()),
        "the refusal must name its reason and the substituted component: {message}"
    );
    assert_eq!(
        fs::read_dir(&victim).expect("victim listing").count(),
        0,
        "nothing was staged or written in the victim directory"
    );
}

/// The `tasks/` direction: `add_worktree` creates the slot's parent and runs
/// `git worktree add` at the slot path, so an exchanged `tasks/` would put a
/// checkout in the victim directory.
#[test]
fn a_tasks_directory_exchanged_at_the_before_hook_refuses_the_worktree_add() {
    let fixture = Fixture::created("tasks-exchanged-add");
    let slot = fixture.task("gamma", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("write the intent");
    let victim = fixture.root.join("victim");
    fs::create_dir_all(&victim).expect("victim directory");
    let tasks = fixture.manager.execution_root().join("tasks");
    let mut hooks = ExchangeAtBefore {
        site: slot.add_site(),
        original: tasks.clone(),
        moved: fixture.root.join("tasks-moved"),
        victim: victim.clone(),
    };

    let error = fixture
        .manager
        .add_worktree(&mut hooks, &slot, &fixture.head)
        .expect_err("the check inside the funnel sees the exchanged tasks directory");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point")
            && message.contains(&tasks.display().to_string()),
        "the refusal must name its reason and the substituted component: {message}"
    );
    assert_eq!(
        fs::read_dir(&victim).expect("victim listing").count(),
        0,
        "no checkout was created in the victim directory"
    );
}

/// A `Before` hook that exchanges one directory under the execution root for
/// a link to `victim`, at one site.
struct ExchangeAtBefore {
    site: EffectSiteId,
    original: PathBuf,
    moved: PathBuf,
    victim: PathBuf,
}

impl EffectHooks for ExchangeAtBefore {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        if site == self.site && phase == HookPhase::Before {
            fs::rename(&self.original, &self.moved).expect("move the real directory aside");
            plant_directory_link(&self.victim, &self.original);
        }
        Injection::Proceed
    }

    fn refusal_cause(&self) -> Option<String> {
        None
    }
}

/// Every Git command runs with `core.hooksPath` at `<root>/hooks-none`, and
/// the in-funnel chain check walks the effect's target, not that path. A
/// `hooks-none` exchanged at the `Before` hook for a link to an outside
/// directory holding an executable `post-checkout` had `git worktree add`
/// run it. The Git runner now walks the hooks path immediately before every
/// command.
#[test]
fn a_hooks_path_exchanged_at_the_before_hook_refuses_the_worktree_add_and_runs_no_hook() {
    let fixture = Fixture::created("hooks-exchanged");
    let slot = fixture.task("delta", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("write the intent");
    let outside = fixture.root.join("outside-hooks");
    fs::create_dir_all(&outside).expect("outside hooks directory");
    let marker = fixture.root.join("hook-ran.marker");
    let script = outside.join("post-checkout");
    fs::write(&script, format!("#!/bin/sh\n: > '{}'\n", marker.display())).expect("hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("executable hook");
    }
    let hooks_dir = fixture.manager.execution_root().join("hooks-none");
    let mut hooks = ExchangeAtBefore {
        site: slot.add_site(),
        original: hooks_dir.clone(),
        moved: fixture.root.join("hooks-moved"),
        victim: outside,
    };

    let error = fixture
        .manager
        .add_worktree(&mut hooks, &slot, &fixture.head)
        .expect_err("the Git runner sees the exchanged hooks path");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point")
            && message.contains(&hooks_dir.display().to_string()),
        "the refusal must name its reason and the substituted component: {message}"
    );
    assert!(!marker.exists(), "the outside hook never ran");
    assert!(
        !fixture.manager.slot_path(&slot).exists(),
        "and no worktree was created"
    );
}

/// `write_intent` staged through a fixed name, `<intent>.tmp`, opened with
/// `File::create`, which follows a link planted there: the record went into
/// the victim file the link named, and the link was renamed into place as
/// the intent. The staging name is now unique per call and opened
/// `create_new`, so a planted name is never followed. A file link needs a
/// symlink, which the Windows guest's test user cannot create.
#[cfg(unix)]
#[test]
fn a_link_planted_at_the_old_staging_name_is_never_followed_by_the_intent_write() {
    let fixture = Fixture::created("staging-planted");
    let slot = fixture.task("epsilon", 1);
    let victim = fixture.root.join("victim.txt");
    fs::write(&victim, "victim bytes\n").expect("victim");
    let intent = fixture.manager.intent_path(&slot);
    let old_staging = intent.with_extension("tmp");
    std::os::unix::fs::symlink(&victim, &old_staging).expect("plant the link at the old name");

    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("the write stages through its own unique name and lands");
    assert_eq!(
        fs::read(&victim).expect("victim"),
        b"victim bytes\n",
        "the victim is untouched"
    );
    let landed = fs::symlink_metadata(&intent).expect("the intent");
    assert!(
        landed.is_file() && landed.len() > 0,
        "the intent landed as a regular file with the record"
    );
    assert!(
        fs::symlink_metadata(&old_staging)
            .expect("the planted link")
            .file_type()
            .is_symlink(),
        "the planted link is still a link, and not what landed"
    );
}

/// A link planted at the intent's own name is a reparse point on a path the
/// write acts through (its rename target), and refuses like a link anywhere
/// else on the chain: the victim it named is untouched and nothing lands. A
/// file link needs a symlink, which the Windows guest's test user cannot
/// create.
#[cfg(unix)]
#[test]
fn a_link_planted_at_the_intent_name_refuses_the_intent_write() {
    let fixture = Fixture::created("intent-name-planted");
    let slot = fixture.task("eta", 1);
    let victim = fixture.root.join("victim.txt");
    fs::write(&victim, "victim bytes\n").expect("victim");
    let intent = fixture.manager.intent_path(&slot);
    std::os::unix::fs::symlink(&victim, &intent).expect("plant the link at the intent's name");

    let error = fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect_err("a link at the intent's name refuses the write");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point")
            && message.contains(&intent.display().to_string()),
        "the refusal must name its reason and the link: {message}"
    );
    assert_eq!(
        fs::read(&victim).expect("victim"),
        b"victim bytes\n",
        "the victim is untouched"
    );
    assert!(
        fs::symlink_metadata(&intent)
            .expect("the link")
            .file_type()
            .is_symlink(),
        "and the planted link is still a link: nothing landed"
    );
}

/// The durable-intent precondition sat before the funnel, so an intent
/// removed at the `Before` hook still yielded a worktree, one that
/// `reclaim_intents` could never find. The check now runs inside the funnel
/// after the hook, and afterwards `intents()` and `reclaim_intents` agree
/// that there is nothing.
#[test]
fn an_intent_removed_at_the_before_hook_refuses_the_worktree_add() {
    struct RemoveIntentAtBefore {
        site: EffectSiteId,
        intent: PathBuf,
    }

    impl EffectHooks for RemoveIntentAtBefore {
        fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            if site == self.site && phase == HookPhase::Before {
                fs::remove_file(&self.intent).expect("remove the intent");
            }
            Injection::Proceed
        }

        fn refusal_cause(&self) -> Option<String> {
            None
        }
    }

    let fixture = Fixture::created("intent-removed-in-hook");
    let slot = fixture.task("zeta", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("write the intent");
    let mut hooks = RemoveIntentAtBefore {
        site: slot.add_site(),
        intent: fixture.manager.intent_path(&slot),
    };

    let error = fixture
        .manager
        .add_worktree(&mut hooks, &slot, &fixture.head)
        .expect_err("the check inside the funnel sees the removed intent");
    let message = refusal_of(&error);
    assert!(
        message.contains("durable intent"),
        "the refusal must name its reason: {message}"
    );
    assert!(
        !fixture.manager.slot_path(&slot).exists(),
        "and no worktree was created"
    );
    let recorded = fixture.manager.intents().expect("intents").len();
    let reclaimed = fixture
        .manager
        .reclaim_intents(&mut NoHooks)
        .expect("reclaim")
        .slots
        .len();
    assert_eq!(
        (recorded, reclaimed),
        (0, 0),
        "nothing is recorded and nothing is reclaimed, so the two agree"
    );
}

/// One substitution per path the table names for a primitive: a `Before`
/// hook exchanges the path for a link to an outside victim, the primitive
/// must refuse naming the substituted component, and the victim must be
/// untouched. Generated from `every_primitive` and `acted_through_paths`, so
/// every path the table names is driven; the case count is pinned as a
/// regression pin on the table's own size — a path dropped from the table
/// is a case that stops being generated and a number that stops matching —
/// and it is not a proof that the table is complete, which it is not (see
/// `ActedThrough`). A file link needs a symlink, which the Windows guest's
/// test user cannot create, so the file-leaf cases run on the Unix legs and
/// are counted as skipped on Windows.
#[test]
fn every_path_a_primitive_acts_through_refuses_a_link_planted_at_the_before_hook() {
    let mut driven = 0_usize;
    let mut skipped = 0_usize;
    let mut driven_primitives = BTreeSet::new();
    for primitive in every_primitive() {
        let count = SubstitutionCase::new(primitive).paths().len();
        assert!(count > 0, "{primitive:?} acts through nothing?");
        for index in 0..count {
            let case = SubstitutionCase::new(primitive);
            let (_, target, leaf) = case.paths()[index].clone();
            let existing_kind = fs::symlink_metadata(&target).ok().map(|m| m.file_type());
            let file_link = leaf == Leaf::Entry && existing_kind.is_some_and(|k| k.is_file());
            if file_link && cfg!(windows) {
                skipped += 1;
                continue;
            }
            let victim = case.fixture.root.join(format!("victim-{index}"));
            let mut hooks = SubstituteAtBefore {
                site: case.site,
                target: target.clone(),
                moved: target.with_extension("moved-away"),
                victim: victim.clone(),
                file_link,
            };
            // The victim carries a sentinel, and a copy of whatever the
            // primitive would have found through the link (the intent, the
            // registration's `gitdir` and `locked`), so a walk that followed
            // the link would find a plausible tree, not an empty one.
            if file_link {
                fs::write(&victim, b"victim bytes\n").expect("victim file");
            } else {
                fs::create_dir_all(&victim).expect("victim directory");
                fs::write(victim.join("sentinel.txt"), b"sentinel\n").expect("sentinel");
                if existing_kind.is_some_and(|k| k.is_dir()) {
                    copy_files_shallow(&target, &victim);
                }
            }
            let before = snapshot(&victim);

            let error = case.run(&mut hooks).expect_err(&format!(
                "{primitive:?} through a link at {} must refuse",
                target.display()
            ));
            let message = refusal_of(&error);
            assert!(
                message.contains("symlink or reparse point")
                    && message.contains(&target.display().to_string()),
                "{primitive:?}: the refusal must name its reason and the substituted \
                 component {}: {message}",
                target.display()
            );
            assert_eq!(
                snapshot(&victim),
                before,
                "{primitive:?}: the victim behind {} is untouched",
                target.display()
            );
            driven += 1;
            driven_primitives.insert(format!("{primitive:?}"));
        }
    }
    assert_eq!(
        driven_primitives.len(),
        every_primitive().len(),
        "every primitive was driven: {driven_primitives:?}"
    );
    // The pinned count: the table's paths, resolved. Scaffolding is five
    // paths and Registration three, so the fourteen primitives resolve to
    // twelve root cases, four intent, five add, ten Git working directory,
    // five removal and three ref cases.
    let expected_total = 39;
    assert_eq!(
        driven + skipped,
        expected_total,
        "the table generated {driven} driven and {skipped} skipped cases; a path added to or \
         dropped from `Primitive::acted_through` changes this number"
    );
    if cfg!(unix) {
        assert_eq!(skipped, 0, "every case runs on Unix");
    }
}

/// Every funnel primitive, listed here rather than in production (§12: a
/// test-only item mid-file cuts the effects census's production region) and
/// pinned complete by the exhaustive match below, which stops compiling when
/// a variant is added without a place in the list.
fn every_primitive() -> Vec<Primitive> {
    use Primitive as P;
    let all = vec![
        P::CreateExecutionRoot,
        P::RemoveExecutionRoot,
        P::WriteIntent,
        P::RemoveIntent,
        P::AddWorktree,
        P::VerifyWorktree,
        P::RemoveWorktree,
        P::CandidateStage,
        P::CandidateWriteTree,
        P::ProposalCherryPick,
        P::RepairMaterialize,
        P::CreateRef,
        P::CompareAndSwapRef,
        P::DeleteRef,
    ];
    for primitive in &all {
        match primitive {
            P::CreateExecutionRoot
            | P::RemoveExecutionRoot
            | P::WriteIntent
            | P::RemoveIntent
            | P::AddWorktree
            | P::VerifyWorktree
            | P::RemoveWorktree
            | P::CandidateStage
            | P::CandidateWriteTree
            | P::ProposalCherryPick
            | P::RepairMaterialize
            | P::CreateRef
            | P::CompareAndSwapRef
            | P::DeleteRef => {}
        }
    }
    assert_eq!(all.len(), 14, "one entry per variant");
    all
}

/// One generated case's fixture, in the state its primitive needs.
struct SubstitutionCase {
    fixture: Fixture,
    primitive: Primitive,
    slot: Option<Slot>,
    registration: Option<PathBuf>,
    site: EffectSiteId,
    refname: String,
}

impl SubstitutionCase {
    fn new(primitive: Primitive) -> Self {
        use Primitive as P;
        let created = !matches!(primitive, P::CreateExecutionRoot);
        let fixture = if created {
            Fixture::created("acted-through")
        } else {
            Fixture::new("acted-through")
        };
        let slot = fixture.task("table", 1);
        let refname = "refs/upstroke/test/table".to_owned();
        let mut registration = None;
        match primitive {
            P::RemoveIntent | P::AddWorktree => {
                fixture
                    .manager
                    .write_intent(&mut NoHooks, &slot)
                    .expect("write the intent");
            }
            P::VerifyWorktree
            | P::RemoveWorktree
            | P::CandidateStage
            | P::CandidateWriteTree
            | P::ProposalCherryPick
            | P::RepairMaterialize => {
                fixture.add_task(&mut NoHooks, "table", 1);
                if primitive == P::RemoveWorktree {
                    let path = fixture.manager.slot_path(&slot);
                    registration = fixture
                        .manager
                        .revalidate_removal(&path)
                        .expect("the registration");
                    assert!(registration.is_some(), "the added worktree is registered");
                }
            }
            P::CompareAndSwapRef | P::DeleteRef => {
                fixture
                    .manager
                    .create_ref_zero_old(
                        &mut NoHooks,
                        RefSite::CreateCandidates,
                        &refname,
                        &fixture.head,
                    )
                    .expect("the ref to move or delete");
            }
            P::CreateExecutionRoot | P::RemoveExecutionRoot | P::WriteIntent | P::CreateRef => {}
        }
        let site = match primitive {
            P::CreateExecutionRoot => EffectSiteId::Worktree(WorktreeSite::CreateExecutionRoot),
            P::RemoveExecutionRoot => EffectSiteId::Worktree(WorktreeSite::RemoveExecutionRoot),
            P::WriteIntent => slot.write_intent_site(),
            P::RemoveIntent => slot.remove_intent_site(),
            P::AddWorktree => slot.add_site(),
            P::VerifyWorktree => EffectSiteId::Worktree(WorktreeSite::Verify),
            P::RemoveWorktree => slot.remove_site(),
            P::CandidateStage => EffectSiteId::Object(ObjectSite::CandidateStage),
            P::CandidateWriteTree => EffectSiteId::Object(ObjectSite::CandidateWriteTree),
            P::ProposalCherryPick => EffectSiteId::Object(ObjectSite::ProposalCherryPick),
            P::RepairMaterialize => EffectSiteId::Object(ObjectSite::RepairMaterialize),
            P::CreateRef => EffectSiteId::Ref(RefSite::CreateCandidates),
            P::CompareAndSwapRef => EffectSiteId::Ref(RefSite::CompareAndSwapIntegration),
            P::DeleteRef => EffectSiteId::Ref(RefSite::DeleteCandidatePin),
        };
        let slot = if matches!(
            primitive,
            P::CreateExecutionRoot
                | P::RemoveExecutionRoot
                | P::CreateRef
                | P::CompareAndSwapRef
                | P::DeleteRef
        ) {
            None
        } else {
            Some(slot)
        };
        Self {
            fixture,
            primitive,
            slot,
            registration,
            site,
            refname,
        }
    }

    fn paths(&self) -> Vec<(PathBuf, PathBuf, Leaf)> {
        self.fixture
            .manager
            .acted_through_paths(
                self.primitive,
                self.slot.as_ref(),
                self.registration.as_deref(),
            )
            .expect("the table resolves for a case built for it")
    }

    fn run(&self, hooks: &mut dyn EffectHooks) -> Result<(), UpstrokeError> {
        use Primitive as P;
        let manager = &self.fixture.manager;
        let slot = self.slot.as_ref();
        let head = &self.fixture.head;
        let side = &self.fixture.side;
        let slot_of = || slot.expect("a slot primitive has a slot");
        match self.primitive {
            P::CreateExecutionRoot => manager.create_execution_root(hooks),
            P::RemoveExecutionRoot => manager.remove_execution_root(hooks).map(drop),
            P::WriteIntent => manager.write_intent(hooks, slot_of()),
            P::RemoveIntent => manager.remove_intent(hooks, slot_of()),
            P::AddWorktree => manager.add_worktree(hooks, slot_of(), head).map(drop),
            P::VerifyWorktree => manager
                .verify_worktree(hooks, slot_of(), &Quiescence::AtBase(head.clone()))
                .map(drop),
            P::RemoveWorktree => manager.remove_worktree(hooks, slot_of()),
            P::CandidateStage => manager.candidate_stage(hooks, slot_of()),
            P::CandidateWriteTree => manager.candidate_write_tree(hooks, slot_of()).map(drop),
            P::ProposalCherryPick => manager
                .proposal_cherry_pick(hooks, slot_of(), side)
                .map(drop),
            P::RepairMaterialize => manager.repair_materialize(hooks, slot_of(), side),
            P::CreateRef => {
                manager.create_ref_zero_old(hooks, RefSite::CreateCandidates, &self.refname, head)
            }
            P::CompareAndSwapRef => manager.compare_and_swap_ref(
                hooks,
                RefSite::CompareAndSwapIntegration,
                &self.refname,
                head,
                side,
            ),
            P::DeleteRef => manager.delete_ref_expected_old(
                hooks,
                RefSite::DeleteCandidatePin,
                &self.refname,
                head,
            ),
        }
    }
}

/// A `Before` hook that exchanges `target` for a link to `victim`: a
/// directory link (a junction on Windows), or a file symlink on Unix.
struct SubstituteAtBefore {
    site: EffectSiteId,
    target: PathBuf,
    moved: PathBuf,
    victim: PathBuf,
    file_link: bool,
}

impl EffectHooks for SubstituteAtBefore {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        if site == self.site && phase == HookPhase::Before {
            if fs::symlink_metadata(&self.target).is_ok() {
                fs::rename(&self.target, &self.moved).expect("move the real path aside");
            } else if let Some(parent) = self.target.parent() {
                fs::create_dir_all(parent).expect("the substituted path's parent");
            }
            if self.file_link {
                #[cfg(unix)]
                std::os::unix::fs::symlink(&self.victim, &self.target)
                    .expect("plant the file link");
                #[cfg(windows)]
                panic!("file-link cases are skipped on Windows");
            } else {
                plant_directory_link(&self.victim, &self.target);
            }
        }
        Injection::Proceed
    }

    fn refusal_cause(&self) -> Option<String> {
        None
    }
}

/// The regular files directly inside `from`, copied into `into`.
fn copy_files_shallow(from: &Path, into: &Path) {
    for entry in fs::read_dir(from).expect("list the moved directory") {
        let entry = entry.expect("entry");
        if entry.file_type().expect("type").is_file() {
            fs::copy(entry.path(), into.join(entry.file_name())).expect("copy into the victim");
        }
    }
}

/// A victim's state: its bytes if a file, otherwise every entry's name and
/// kind one level down and every file's bytes.
fn snapshot(victim: &Path) -> Vec<(String, Vec<u8>)> {
    let metadata = fs::symlink_metadata(victim).expect("the victim exists");
    if metadata.is_file() {
        return vec![("<file>".to_owned(), fs::read(victim).expect("victim bytes"))];
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(victim).expect("list the victim") {
        let entry = entry.expect("entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().expect("type");
        let bytes = if kind.is_file() {
            fs::read(entry.path()).expect("file bytes")
        } else if kind.is_dir() {
            b"<dir>".to_vec()
        } else {
            b"<link>".to_vec()
        };
        entries.push((name, bytes));
    }
    entries.sort();
    entries
}

/// The reviewer's P1 sequence, by name: the registration's admin directory
/// captured before the `Before` hook is exchanged for a link to a victim
/// holding copied `gitdir` and `locked` entries; `registration_still_names`
/// used to follow it and `remove_file(admin/locked)` deleted the victim's
/// file. The generated test above covers it too; this is the named witness.
#[test]
fn a_registration_admin_directory_exchanged_at_the_before_hook_refuses_the_worktree_removal() {
    let fixture = Fixture::created("admin-exchanged");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let admin = fixture
        .manager
        .revalidate_removal(&path)
        .expect("registration")
        .expect("registered");
    fs::write(admin.join("locked"), b"do not remove\n").expect("the locked marker");
    let victim = fixture.root.join("victim-admin");
    fs::create_dir_all(&victim).expect("victim");
    copy_files_shallow(&admin, &victim);
    let before = snapshot(&victim);
    let mut hooks = SubstituteAtBefore {
        site: slot.remove_site(),
        target: admin.clone(),
        moved: admin.with_extension("moved-away"),
        victim: victim.clone(),
        file_link: false,
    };

    let error = fixture
        .manager
        .remove_worktree(&mut hooks, &slot)
        .expect_err("the walk sees the exchanged admin directory");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point")
            && message.contains(&admin.display().to_string()),
        "the refusal must name its reason and the substituted component: {message}"
    );
    assert_eq!(
        snapshot(&victim),
        before,
        "the victim's `locked` and `gitdir` are untouched"
    );
    assert!(
        path.exists(),
        "and the checkout was not deleted either: every check runs first"
    );
}

/// The reviewer's P2 sequence for the add, by name: `intents/` exchanged for
/// a link to a victim holding a same-named intent file, through which the
/// add's metadata read used to authorise a worktree whose intent lived
/// outside the root.
#[test]
fn an_intents_directory_exchanged_at_the_before_hook_refuses_the_worktree_add() {
    let fixture = Fixture::created("intents-exchanged-add");
    let slot = fixture.task("beta", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("write the intent");
    let intents = fixture.manager.execution_root().join("intents");
    let victim = fixture.root.join("victim-intents");
    fs::create_dir_all(&victim).expect("victim");
    copy_files_shallow(&intents, &victim);
    let before = snapshot(&victim);
    let mut hooks = SubstituteAtBefore {
        site: slot.add_site(),
        target: intents.clone(),
        moved: intents.with_extension("moved-away"),
        victim: victim.clone(),
        file_link: false,
    };

    let error = fixture
        .manager
        .add_worktree(&mut hooks, &slot, &fixture.head)
        .expect_err("the walk sees the exchanged intents directory");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point")
            && message.contains(&intents.display().to_string()),
        "the refusal must name its reason and the substituted component: {message}"
    );
    assert_eq!(snapshot(&victim), before, "the victim is untouched");
    assert!(
        !fixture.manager.slot_path(&slot).exists(),
        "and no worktree was created"
    );
}

/// The staging name carries no part of the record's name, so a slot at the
/// old maximum still lands: a 207-byte task key gives a 224-byte intent name,
/// and the previous `.<intent>.<ULID>.tmp` staging name was 256 bytes, one
/// over `NAME_MAX`.
#[test]
fn a_slot_name_at_the_old_maximum_still_lands_its_intent() {
    let fixture = Fixture::created("long-key");
    let key = "a".repeat(207);
    let slot = fixture.task(&key, 1);
    assert_eq!(
        slot.intent_name().len(),
        224,
        "the premise: the intent name is at the old maximum"
    );
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("a valid slot name at the old maximum lands");
    assert!(fixture.manager.intent_path(&slot).is_file());
    assert_eq!(
        fixture.manager.intents().expect("intents"),
        vec![slot],
        "and the record is listed"
    );
}

/// The §8 staging protocol's recovery rule: a staging file is never an
/// intent, and no filename proves who wrote a file. An orphan a crash left
/// behind used to fail `intents()` forever, which blocked every reclaim;
/// `intents()` ignores it, `reclaim_intents` reports it on its outcome and
/// leaves it in place, and a retry of the write lands beside it. The orphan
/// has the exact shape `write_intent` produces, a real ULID included.
#[test]
fn a_staging_orphan_is_ignored_by_intents_reported_by_reclaim_and_never_removed() {
    let fixture = Fixture::created("staging-orphan");
    let slot = fixture.task("alpha", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("a real intent");
    let intents = fixture.manager.execution_root().join("intents");
    let orphan = intents.join(format!(".stage-task-{}.tmp", crate::ulid::ulid()));
    fs::write(&orphan, b"{\"half\":").expect("plant the orphan");

    assert_eq!(
        fixture
            .manager
            .intents()
            .expect("intents ignore the orphan"),
        vec![slot.clone()],
        "the real intent is listed and the orphan is not"
    );
    let reclaimed = fixture
        .manager
        .reclaim_intents(&mut NoHooks)
        .expect("reclaim reports the orphan and removes the rest");
    assert_eq!(reclaimed.slots, vec![slot.clone()]);
    assert_eq!(
        reclaimed.staging_leftovers,
        vec![orphan.clone()],
        "the orphan is reported on the outcome"
    );
    assert!(
        orphan.exists(),
        "and left where it was: no filename proves who wrote it"
    );
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("a retry of the write lands beside it");
    assert!(fixture.manager.intent_path(&slot).is_file());
    assert!(orphan.exists(), "the retry staged under its own fresh name");
}

/// `hooks-none` must be empty, not only a real link-free directory: a hook
/// written into it runs under every Git command. A `Before` hook writes an
/// executable `post-checkout` into the existing directory; the add refuses
/// naming the entry, and the hook never runs.
#[test]
fn a_hook_written_into_hooks_none_at_the_before_hook_refuses_the_worktree_add_and_never_runs() {
    struct WriteHookAtBefore {
        site: EffectSiteId,
        script: PathBuf,
        marker: PathBuf,
    }

    impl EffectHooks for WriteHookAtBefore {
        fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            if site == self.site && phase == HookPhase::Before {
                fs::write(
                    &self.script,
                    format!("#!/bin/sh\n: > '{}'\n", self.marker.display()),
                )
                .expect("write the hook");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    fs::set_permissions(&self.script, fs::Permissions::from_mode(0o755))
                        .expect("executable hook");
                }
            }
            Injection::Proceed
        }

        fn refusal_cause(&self) -> Option<String> {
            None
        }
    }

    let fixture = Fixture::created("hooks-none-written");
    let slot = fixture.task("theta", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("write the intent");
    let hooks_dir = fixture.manager.execution_root().join("hooks-none");
    let marker = fixture.root.join("hook-ran.marker");
    let mut hooks = WriteHookAtBefore {
        site: slot.add_site(),
        script: hooks_dir.join("post-checkout"),
        marker: marker.clone(),
    };

    let error = fixture
        .manager
        .add_worktree(&mut hooks, &slot, &fixture.head)
        .expect_err("a hooks path that holds a hook refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("hook-free") && message.contains("post-checkout"),
        "the refusal must name its reason and the entry: {message}"
    );
    assert!(!marker.exists(), "the hook never ran");
    assert!(
        !fixture.manager.slot_path(&slot).exists(),
        "and no worktree was created"
    );
}

/// `NotFound` from `canonicalize` does not say a component is absent: a link
/// that exists with an absent target answers the same, and the peel used to
/// walk past it and reconstruct the path through a link that is there. The
/// link is read without following, and refuses as a reparse point on the
/// chain. A dangling link needs a symlink, which the Windows guest's test
/// user cannot create.
#[cfg(unix)]
#[test]
fn canonical_prefix_refuses_a_dangling_link_rather_than_peeling_past_it() {
    let tree =
        acquire(&std::env::temp_dir(), "canonical-dangling").expect("acquire a scratch tree");
    let real = strip_verbatim(tree.path().canonicalize().expect("canonical scratch"));
    let link = real.join("link");
    std::os::unix::fs::symlink(real.join("missing"), &link).expect("plant the dangling link");
    let error = canonical_prefix(&link.join("child"))
        .expect_err("a dangling link on the chain is a reparse point, not absence");
    let message = refusal_of(&error);
    assert!(
        message.contains("symlink or reparse point")
            && message.contains(&link.display().to_string()),
        "the refusal must name its reason and the link: {message}"
    );
}

/// Only the exact shape `write_intent` produces is a staging file. A name
/// that merely resembles one is nobody's to hide or delete: `intents()`
/// reports it as the malformed file it is, and `reclaim_intents` leaves it
/// where it is.
#[test]
fn a_name_that_merely_resembles_a_staging_file_is_neither_hidden_nor_removed() {
    let fixture = Fixture::created("staging-lookalike");
    let intents = fixture.manager.execution_root().join("intents");
    let lookalike = intents.join(".stage-report.tmp");
    fs::write(&lookalike, b"someone's report\n").expect("plant the lookalike");

    let listed = fixture
        .manager
        .intents()
        .expect_err("a malformed name in the intent directory is reported");
    assert!(
        refusal_of(&listed).contains("unexpected file"),
        "reported as the malformed file it is: {listed}"
    );
    fixture
        .manager
        .reclaim_intents(&mut NoHooks)
        .expect_err("reclaim stops at the malformed name too");
    assert_eq!(
        fs::read(&lookalike).expect("the lookalike"),
        b"someone's report\n",
        "and it is untouched"
    );
}

/// An orphan of each kind is reported and left in place, whatever its
/// spelling within the generator's range, and no removal site fires for it:
/// deleting it would be cleanup that infers ownership from a filename (§8).
#[test]
fn a_staging_orphan_of_each_kind_is_reported_and_no_removal_site_fires() {
    let fixture = Fixture::created("staging-orphan-kinds");
    let intents = fixture.manager.execution_root().join("intents");
    let mut orphans = Vec::new();
    for kind in ["task", "staging", "snapshot"] {
        let orphan = intents.join(format!(".stage-{kind}-{}.tmp", crate::ulid::ulid()));
        fs::write(&orphan, b"{").expect("plant the orphan");
        orphans.push(orphan);
    }
    orphans.sort();
    let (mut hooks, shared) = harness();

    let reclaimed = fixture
        .manager
        .reclaim_intents(&mut hooks)
        .expect("reclaim reports every orphan");
    assert!(
        reclaimed.slots.is_empty(),
        "no intent was recorded, so none is reclaimed"
    );
    assert_eq!(
        reclaimed.staging_leftovers, orphans,
        "every orphan is reported, in order"
    );
    for orphan in &orphans {
        assert!(orphan.exists(), "{} is still there", orphan.display());
    }
    let observed = shared.lock().expect("harness").coverage().to_vec();
    for site in [
        EffectSiteId::Worktree(WorktreeSite::RemoveIntent),
        EffectSiteId::Worktree(WorktreeSite::RemoveStagingIntent),
        EffectSiteId::Snapshot(SnapshotSite::RemoveIntent),
    ] {
        assert!(
            !observed.iter().any(|seen| seen.site == site),
            "no removal ran under {site} for a leftover nobody can prove they own: {observed:?}"
        );
    }
}

/// `commit_parent` and `commit_tree_sha` used to fold every failure of the
/// command into `None` through `git_ok(..).ok()`, a containment refusal
/// included: an entry appearing in `hooks-none` between the gate's own
/// `worktree list` and the lookup's `rev-parse` made the per-command check
/// refuse, and `.ok()` turned that refusal into "not a commit". The gate
/// reads the hooks path through the same runner, so from the public methods
/// it refuses first; the lookup itself is driven here directly, as the
/// second command in that sequence. Only Git's own quiet "no such object"
/// (exit status 1, nothing on stderr) is `None`; a refusal and a Git failure
/// that speaks both propagate.
#[test]
fn a_hook_planted_in_hooks_none_makes_the_object_lookups_refuse_rather_than_answer_none() {
    let fixture = Fixture::created("lookup-refuses");
    let lookup = |spec: String| {
        [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("--quiet"),
            OsString::from(spec),
        ]
    };

    // A Git failure that speaks is an error, not absence: peeling a tree to
    // a commit exits 1 with a message.
    let spoken = fixture
        .manager
        .quiet_object_lookup(&lookup(format!("{}^{{tree}}^{{commit}}", fixture.head)))
        .expect_err("a Git failure that speaks propagates");
    assert!(
        matches!(&spoken, UpstrokeError::Git { message } if message.contains("rev-parse")),
        "the error names the command: {spoken}"
    );

    fs::write(
        fixture
            .manager
            .execution_root()
            .join("hooks-none")
            .join("post-checkout"),
        b"#!/bin/sh\nexit 0\n",
    )
    .expect("plant a hook");
    let refused = fixture
        .manager
        .quiet_object_lookup(&lookup(format!("{}^{{commit}}^", fixture.head)))
        .expect_err("the lookup propagates the runner's refusal rather than answering None");
    let message = refusal_of(&refused);
    assert!(
        message.contains("hook-free") && message.contains("post-checkout"),
        "the refusal must name its reason: {message}"
    );
    for (name, result) in [
        (
            "commit_parent",
            fixture.manager.commit_parent(&fixture.head),
        ),
        (
            "commit_tree_sha",
            fixture.manager.commit_tree_sha(&fixture.head),
        ),
    ] {
        let error = result.expect_err(&format!("{name} must refuse, not answer None"));
        let message = refusal_of(&error);
        assert!(
            message.contains("hook-free") && message.contains("post-checkout"),
            "{name}: the refusal must name its reason: {message}"
        );
    }
}

/// A prefix that resolves to a regular file cannot carry components below
/// it, and the filesystem said so; rejoining them lexically would hand back
/// exactly the unverified path `canonical_prefix` exists not to. Reported as
/// `NotADirectory` at the file, on every platform.
#[test]
fn canonical_prefix_refuses_a_prefix_that_is_a_regular_file_with_components_below_it() {
    let tree =
        acquire(&std::env::temp_dir(), "canonical-file-prefix").expect("acquire a scratch tree");
    let real = strip_verbatim(tree.path().canonicalize().expect("canonical scratch"));
    let file = real.join("file");
    fs::write(&file, "not a directory\n").expect("plant the file");
    let error = canonical_prefix(&file.join("child"))
        .expect_err("a regular file cannot carry components below it");
    assert!(
        matches!(
            &error,
            UpstrokeError::Io { path, source }
                if path == &file && source.kind() == std::io::ErrorKind::NotADirectory
        ),
        "the error names the file and says it is not a directory: {error}"
    );
}

/// Restores a directory's mode on drop, so a fixture made unreadable or
/// unwritable for one assertion is reclaimable again whatever the assertion
/// did.
#[cfg(unix)]
struct RestoreMode {
    path: PathBuf,
}

#[cfg(unix)]
impl Drop for RestoreMode {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt as _;
        match fs::set_permissions(&self.path, fs::Permissions::from_mode(0o755)) {
            Ok(()) => {}
            // Already gone: nothing to restore, and a second panic while
            // unwinding would abort the whole test process.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("restore the mode of {}: {error}", self.path.display()),
        }
    }
}

/// `add_worktree` read its intent with `is_file()`, which folds every
/// metadata failure into `false`: an `intents/` the process cannot search
/// answered `AddWithoutIntent`, "its durable intent does not exist", for an
/// intent that was there. A read failure is an error naming the intent.
/// Evaluated on the Unix legs, where a mode bit binds a non-root user.
#[cfg(unix)]
#[test]
fn an_intent_that_cannot_be_read_is_an_error_and_not_an_absent_intent() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::created("intent-unreadable");
    let slot = fixture.task("alpha", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("write the intent");
    let intents = fixture.manager.execution_root().join("intents");
    let _restore = RestoreMode {
        path: intents.clone(),
    };
    fs::set_permissions(&intents, fs::Permissions::from_mode(0o000)).expect("make it unsearchable");
    // The injection must bite: root, or a process holding CAP_DAC_OVERRIDE,
    // reads through a mode of 000, and this test would then measure nothing.
    let intent = fixture.manager.intent_path(&slot);
    assert!(
        fs::symlink_metadata(&intent).is_err(),
        "prerequisite not met: the mode bit did not bind (running as root or with \
         CAP_DAC_OVERRIDE); this test needs an unprivileged user"
    );

    let error = fixture
        .manager
        .add_worktree(&mut NoHooks, &slot, &fixture.head)
        .expect_err("an intent that cannot be read is not an intent that is absent");
    assert!(
        matches!(&error, UpstrokeError::Io { path, .. } if path == &intent),
        "the error names the intent it could not read: {error}"
    );
}

/// `remove_execution_root` discarded the result of removing an empty
/// scaffolding directory with `let _ =`, so a scaffolding directory nothing
/// could remove kept the root and the caller learnt only `Ok(false)`. The
/// failure now names the directory. Evaluated on the Unix legs, where a
/// mode bit binds a non-root user.
#[cfg(unix)]
#[test]
fn a_scaffolding_directory_that_cannot_be_removed_is_reported_not_swallowed() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::created("scaffolding-unremovable");
    let root = fixture.manager.execution_root().to_path_buf();
    let _restore = RestoreMode { path: root.clone() };
    fs::set_permissions(&root, fs::Permissions::from_mode(0o555))
        .expect("make the root unwritable");
    // The injection must bite: root, or a process holding CAP_DAC_OVERRIDE,
    // writes through a mode of 555, the removal would then succeed and the
    // fixture's root would be gone before the assertion.
    let probe = root.join("probe-write");
    if fs::create_dir(&probe).is_ok() {
        fs::remove_dir(&probe).expect("remove the probe");
        panic!(
            "prerequisite not met: the mode bit did not bind (running as root or with \
             CAP_DAC_OVERRIDE); this test needs an unprivileged user"
        );
    }

    let error = fixture
        .manager
        .remove_execution_root(&mut NoHooks)
        .expect_err("a scaffolding directory that cannot be removed is reported");
    assert!(
        matches!(
            &error,
            UpstrokeError::Filesystem { operation: "remove", path, .. }
                if path.starts_with(&root) && path != &root
        ),
        "the error names the removal and the scaffolding directory it could not remove: {error}"
    );
}

/// A **deletion** revalidates the chain, and refuses before it acts
/// (`PR5-WORKSPACE-009`).
///
/// `execution_root`: "every create/reclaim/**delete** revalidates."
/// `remove_execution_root` had three test callers and all three ran against
/// an intact chain, so deleting its `self.revalidate()?;` line changed
/// nothing observable — the create and reclaim thirds of the sentence were
/// covered and the delete third was not. That third is the one where the
/// consequence is a deletion outside the managed tree, so the sentinel here
/// is *outside* the fixture and its bytes are compared after.
#[test]
fn a_deletion_revalidates_the_chain_and_refuses_before_it_deletes() {
    let fixture = Fixture::created("delete-revalidates");
    let sentinel_dir = fixture.root.join("outside");
    fs::create_dir_all(&sentinel_dir).expect("sentinel directory");
    let sentinel = sentinel_dir.join("keepme.txt");
    let sentinel_bytes = b"a file the managed tree has no business deleting";
    fs::write(&sentinel, sentinel_bytes).expect("sentinel");

    // A validated component of the chain becomes a link to the sentinel's
    // directory, after derive already succeeded.
    let base = fixture.manager.base().to_path_buf();
    let moved = fixture.root.join("moved-away");
    fs::rename(&base, &moved).expect("move the real repository aside");
    plant_directory_link(&moved, &base);

    let message = refusal_of(
        &fixture
            .manager
            .remove_execution_root(&mut NoHooks)
            .expect_err("a deletion on a chain that changed refuses"),
    );
    assert!(
        message.contains("not a real directory"),
        "the refusal must name its reason: {message}"
    );
    assert!(
        fixture.manager.execution_root().exists(),
        "and it refused BEFORE acting: the execution root is still there"
    );
    assert_eq!(
        fs::read(&sentinel).expect("sentinel"),
        sentinel_bytes,
        "the sentinel outside the managed tree is byte-identical"
    );
}

/// A **reclaim** revalidates the chain, and refuses before it removes
/// anything.
///
/// `execution_root`: "every create/**reclaim**/delete revalidates." The
/// create third dies at
/// `a_symlink_below_the_private_root_refuses_the_execution_root` and the
/// delete third at the test above. The reclaim third had no fixture that
/// could see it: deleting `reclaim_intents`' own `self.revalidate()?;`
/// left the whole suite green on Linux and on the Windows guest.
///
/// **The shape is what makes it visible, and it is not the obvious one.**
/// `remove_worktree` and `remove_intent` each revalidate on their own
/// before they act, so the outer check is masked the moment there is
/// anything to remove — and every other fixture reaching `reclaim_intents`
/// does so with at least one intent written. The one shape where the outer
/// check is the sole guard is a reclaim over an execution root with **no
/// intents**, where an unguarded version answers `Ok([])` instead of the
/// containment refusal. The otherwise identical fixture that writes one
/// intent first was measured against that same edit and does **not**
/// distinguish it, so the emptiness below is the premise and is asserted
/// rather than assumed.
///
/// The sentinel is *outside* the fixture and its bytes are compared after,
/// for the reason the deletion test gives: what a reclaim through an
/// exchanged ancestor reaches is a removal outside the managed tree.
#[test]
fn a_reclaim_revalidates_the_chain_and_refuses_before_it_removes() {
    let fixture = Fixture::created("reclaim-revalidates");
    let sentinel_dir = fixture.root.join("outside");
    fs::create_dir_all(&sentinel_dir).expect("sentinel directory");
    let sentinel = sentinel_dir.join("keepme.txt");
    let sentinel_bytes = b"a file the managed tree has no business deleting";
    fs::write(&sentinel, sentinel_bytes).expect("sentinel");

    // The premise: nothing to remove, so no inner revalidation can stand in
    // for the outer one. A fixture that grew an intent here would still
    // pass and would stop measuring anything.
    assert!(
        fixture
            .manager
            .intents()
            .expect("the intents of a freshly created root")
            .is_empty(),
        "this fixture is the no-intent reclaim, and only that shape is \
             guarded by `reclaim_intents`' own revalidation"
    );

    // A validated component of the chain becomes a link to the sentinel's
    // directory, after derive already succeeded.
    let base = fixture.manager.base().to_path_buf();
    let moved = fixture.root.join("moved-away");
    fs::rename(&base, &moved).expect("move the real repository aside");
    plant_directory_link(&moved, &base);

    let message = refusal_of(
        &fixture
            .manager
            .reclaim_intents(&mut NoHooks)
            .expect_err("a reclaim on a chain that changed refuses"),
    );
    assert!(
        message.contains("not a real directory"),
        "the refusal must name its reason: {message}"
    );
    assert_eq!(
        fs::read(&sentinel).expect("sentinel"),
        sentinel_bytes,
        "the sentinel outside the managed tree is byte-identical"
    );
}

#[test]
fn a_root_inside_a_repository_worktree_refuses() {
    let fixture = Fixture::new("root-inside");
    // The private root *is* the repository checkout: the execution root
    // would then live inside a worktree of the repository it manages.
    let error = WorkspaceManager::derive(
        &fixture.base,
        &fixture.base,
        "01KZSWEEP00000000000000003",
        "inc-1",
    )
    .expect_err("a root inside a repository worktree refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("inside the repository worktree"),
        "the refusal must name its reason: {message}"
    );
}

#[test]
fn a_foreign_repository_worktree_inside_the_root_refuses() {
    let fixture = Fixture::created("worktree-inside");
    let intruder = fixture.manager.execution_root().join("intruder");
    git(
        &fixture.base,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &intruder.to_string_lossy(),
            &fixture.head,
        ],
    );
    let error = fixture
        .manager
        .revalidate()
        .expect_err("a foreign worktree inside the root refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("is inside it"),
        "the refusal must name its reason: {message}"
    );

    // And the manager's own slots are not foreign, which is the half a
    // literal reading of the sentence would get wrong.
    git(
        &fixture.base,
        &["worktree", "remove", "--force", &intruder.to_string_lossy()],
    );
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    fixture
        .manager
        .revalidate()
        .expect("the manager's own worktree is not a foreign one");
    fixture
        .manager
        .remove_worktree(&mut NoHooks, &slot)
        .expect("remove");
}

#[test]
fn nothing_outside_the_execution_root_is_ever_deleted() {
    let fixture = Fixture::created("containment");
    let outside = fixture.root.join("precious");
    fs::create_dir_all(&outside).expect("outside directory");
    fs::write(outside.join("keep.txt"), "keep\n").expect("outside file");

    let error = fixture
        .manager
        .contained(&outside)
        .expect_err("a path outside the root refuses");
    let message = refusal_of(&error);
    assert!(
        message.contains("outside the execution root"),
        "the refusal must name its reason: {message}"
    );
    assert!(outside.join("keep.txt").exists(), "and delete nothing");

    // The root itself is not inside itself: a removal that accepted it
    // would delete the whole root through a per-slot primitive.
    assert!(
        fixture
            .manager
            .contained(fixture.manager.execution_root())
            .is_err(),
        "the root is not a contained target of a slot removal"
    );
    fixture
        .manager
        .contained(&fixture.manager.execution_root().join("tasks").join("kx-g1"))
        .expect("a slot path is contained");
}

#[test]
fn a_slot_name_that_could_escape_the_root_refuses() {
    let fixture = Fixture::created("slot-names");
    for hostile in ["..", "../escape", "a/b", "-force", "", "naïve"] {
        let slot = Slot::Task {
            key: hostile.to_owned(),
            generation: 1,
        };
        let error = fixture
            .manager
            .write_intent(&mut NoHooks, &slot)
            .expect_err("a hostile slot name refuses");
        let message = refusal_of(&error);
        assert!(
            message.contains("slot name"),
            "the refusal must name its reason for `{hostile}`: {message}"
        );
    }
    fixture
        .manager
        .write_intent(
            &mut NoHooks,
            &Slot::Task {
                key: "ok_key-1".to_owned(),
                generation: 1,
            },
        )
        .expect("a legal name is accepted");
}

/// The hostile slot names, one per **mechanism** by which a name escapes
/// containment or changes a command's meaning.
///
/// Kept as a table with its mechanism named so that hostility is a
/// distinct-value count rather than a claim in prose: two entries that
/// escape the same way are one test, and the count below is asserted.
const HOSTILE_SLOT_NAMES: &[(&str, &str)] = &[
    ("..", "the parent directory itself"),
    ("../escape", "traversal through a separator"),
    ("a/b", "a POSIX separator, so the name is two components"),
    ("a\\b", "a Windows separator, which POSIX-only checks miss"),
    (
        "-force",
        "a leading dash the Git commands would read as an option",
    ),
    ("", "empty, which collapses the path component away"),
    ("naïve", "non-ASCII, whose NFC/NFD forms are two names"),
    (
        ".",
        "the current directory, which aliases the namespace root",
    ),
];

/// Every public primitive that turns a `&Slot` into a path refuses a
/// hostile name — over a list **derived from this module's own
/// signatures**, not from the ones the author remembered.
///
/// `a_slot_name_that_could_escape_the_root_refuses` exercises exactly one
/// primitive, `write_intent`. That is the `bounded_grid` failure this
/// project has recorded three times: the grid varies the hostile name and
/// holds the primitive fixed, so it stays green while
/// `candidate_stage`, `candidate_write_tree`, `proposal_cherry_pick`,
/// `repair_materialize` and `changed_paths` run `git add -A`,
/// `git write-tree`, `git cherry-pick` and `git diff` with a working
/// directory the name placed outside the execution root. `Slot`'s fields
/// are `pub`, so the name is caller data at every one of those entry
/// points.
///
/// The derivation is the scan below: a primitive that can refuse is a
/// `pub fn` taking `slot: &Slot` and returning a `Result`. Adding one
/// without an arm here fails this test by name.
#[test]
fn every_slot_taking_primitive_refuses_a_hostile_slot_name() {
    let fixture = Fixture::created("slot-grid");
    let manager = &fixture.manager;
    let head = fixture.head.clone();

    type Call<'a> = Box<dyn Fn(&Slot) -> Result<(), UpstrokeError> + 'a>;
    let primitives: Vec<(&str, Call<'_>)> = vec![
        (
            "write_intent",
            Box::new(|slot| manager.write_intent(&mut NoHooks, slot)),
        ),
        (
            "remove_intent",
            Box::new(|slot| manager.remove_intent(&mut NoHooks, slot)),
        ),
        (
            "add_worktree",
            Box::new(|slot| manager.add_worktree(&mut NoHooks, slot, &head).map(drop)),
        ),
        (
            "verify_worktree",
            Box::new(|slot| {
                manager
                    .verify_worktree(&mut NoHooks, slot, &Quiescence::AtBase(head.clone()))
                    .map(drop)
            }),
        ),
        (
            "remove_worktree",
            Box::new(|slot| manager.remove_worktree(&mut NoHooks, slot)),
        ),
        (
            "candidate_stage",
            Box::new(|slot| manager.candidate_stage(&mut NoHooks, slot)),
        ),
        (
            "candidate_write_tree",
            Box::new(|slot| manager.candidate_write_tree(&mut NoHooks, slot).map(drop)),
        ),
        (
            "proposal_cherry_pick",
            Box::new(|slot| {
                manager
                    .proposal_cherry_pick(&mut NoHooks, slot, &fixture.side)
                    .map(drop)
            }),
        ),
        (
            "repair_materialize",
            Box::new(|slot| {
                manager
                    .repair_materialize(&mut NoHooks, slot, &fixture.side)
                    .map(drop)
            }),
        ),
        (
            "changed_paths",
            Box::new(|slot| manager.changed_paths(slot, &head).map(drop)),
        ),
        (
            "candidate_diff",
            Box::new(|slot| manager.candidate_diff(slot, &head, &head).map(drop)),
        ),
    ];

    let covered: BTreeSet<String> = primitives
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    assert_eq!(
        covered.len(),
        primitives.len(),
        "each primitive appears once"
    );
    assert_eq!(
        covered,
        slot_taking_fallible_primitives(),
        "the grid must be this module's slot-taking fallible `pub fn`s, derived from its \
             signatures — a primitive with no arm here is one nothing refuses for"
    );

    let mechanisms: BTreeSet<&str> = HOSTILE_SLOT_NAMES.iter().map(|(_, why)| *why).collect();
    assert_eq!(
        mechanisms.len(),
        HOSTILE_SLOT_NAMES.len(),
        "every hostile name is a distinct escape mechanism, not a restatement"
    );
    assert_eq!(mechanisms.len(), 8, "eight distinct mechanisms");

    // Something outside the root that a successful escape would reach, so
    // "it refused" is not the only thing asserted.
    let outside = fixture.root.join("precious");
    fs::create_dir_all(&outside).expect("outside directory");
    fs::write(outside.join("keep.txt"), "keep\n").expect("outside file");

    for (name, call) in &primitives {
        for (hostile, why) in HOSTILE_SLOT_NAMES {
            let slot = Slot::Task {
                key: (*hostile).to_owned(),
                generation: 1,
            };
            let Err(error) = call(&slot) else {
                panic!("`{name}` accepted the slot name `{hostile}` ({why})")
            };
            let message = refusal_of(&error);
            assert!(
                message.contains("slot name"),
                "`{name}` must refuse `{hostile}` by naming the slot name: {message}"
            );
        }
    }

    assert_eq!(
        fs::read_to_string(outside.join("keep.txt")).expect("still there"),
        "keep\n",
        "and nothing outside the execution root was touched"
    );
}

/// The names of this module's `pub fn`s that take `slot: &Slot` and return
/// a `Result` — read out of the source rather than listed.
///
/// `slot_path` and `intent_path` are deliberately not in this set: they
/// return a `PathBuf` infallibly and are path arithmetic, which is why the
/// predicate is "returns a `Result`" rather than "mentions a `Slot`".
fn slot_taking_fallible_primitives() -> BTreeSet<String> {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/workspace_manager.rs"),
    )
    .expect("read this module's source");
    let mut names = BTreeSet::new();
    let mut seen_fns = 0_usize;
    for chunk in source.split("\n    pub fn ").skip(1) {
        seen_fns += 1;
        let Some((signature, _)) = chunk.split_once('{') else {
            continue;
        };
        let Some(name) = signature.split(['(', '<']).next() else {
            continue;
        };
        if signature.contains("slot: &Slot") && signature.contains("-> Result<") {
            names.insert(name.to_owned());
        }
    }
    assert!(
        seen_fns > 30,
        "the scan read this module's signatures rather than nothing: {seen_fns}"
    );
    names
}

/// `slice_contract.invariants_introduced[1]`: "worktree and snapshot
/// intents **synced before** the add".
///
/// `WriteIntent` and `Add` are separate sites — each carries its own hooks
/// and the cancellation clause is stated per clause — so no single funnel
/// body can order them. That makes the ordering a *caller's* obligation,
/// and an unchecked obligation is one the first schema-4 caller in PR7–PR10
/// can drop silently: the add succeeds, and the worktree it created is
/// invisible to `reclaim_intents`, which walks intents. The second half of
/// this test is the consequence the refusal exists to prevent.
#[test]
fn an_add_without_a_durable_intent_refuses_and_leaves_nothing_registered() {
    let fixture = Fixture::created("add-without-intent");
    let slots = [
        fixture.task("alpha", 1),
        Slot::Staging { sequence: 7 },
        Slot::Snapshot {
            name: SnapshotName::gates(1, 1),
        },
    ];
    assert_eq!(
        slots
            .iter()
            .map(Slot::add_site)
            .collect::<BTreeSet<_>>()
            .len(),
        3,
        "one slot per add site: Worktree.Add, Worktree.AddStaging, Snapshot.Add"
    );

    for slot in &slots {
        let path = fixture.manager.slot_path(slot);
        let error = fixture
            .manager
            .add_worktree(&mut NoHooks, slot, &fixture.head)
            .expect_err("an add with no durable intent refuses");
        let message = refusal_of(&error);
        assert!(
            message.contains("durable intent"),
            "the refusal must name its reason: {message}"
        );
        assert!(!path.exists(), "and no worktree directory was created");
        assert!(
            fixture
                .manager
                .worktree_records()
                .expect("records")
                .iter()
                .all(|record| record.path != path),
            "and nothing was registered with Git either"
        );

        // The same add, with the intent durable, succeeds — so the refusal
        // is about the ordering and not about the slot.
        fixture
            .manager
            .write_intent(&mut NoHooks, slot)
            .expect("intent");
        fixture
            .manager
            .add_worktree(&mut NoHooks, slot, &fixture.head)
            .expect("the same add once the intent is durable");
        assert!(path.exists());
    }

    // And the reason: every worktree the manager created is reachable from
    // an intent, so reclaim finds all three.
    let reclaimed = fixture
        .manager
        .reclaim_intents(&mut NoHooks)
        .expect("reclaim");
    assert_eq!(
        reclaimed.slots.iter().collect::<BTreeSet<_>>(),
        slots.iter().collect::<BTreeSet<_>>(),
        "reclaim walks intents, so an add without one would leave a worktree it never sees"
    );
    for slot in &slots {
        assert!(!fixture.manager.slot_path(slot).exists());
    }
}

// -----------------------------------------------------------------------
// INV-17: the ref primitives
// -----------------------------------------------------------------------

#[test]
fn a_symbolic_ref_is_refused_without_touching_the_victim() {
    let fixture = Fixture::created("symbolic-ref");
    git(
        &fixture.base,
        &["symbolic-ref", "refs/upstroke/sym", "refs/heads/main"],
    );
    let before = git(&fixture.base, &["rev-parse", "refs/heads/main"]);

    for attempt in [
        fixture.manager.create_ref_zero_old(
            &mut NoHooks,
            RefSite::CreateCandidates,
            "refs/upstroke/sym",
            &fixture.head,
        ),
        fixture.manager.delete_ref_expected_old(
            &mut NoHooks,
            RefSite::DeleteCandidatesRef,
            "refs/upstroke/sym",
            &fixture.head,
        ),
        // The CAS arm, which `ref_rules` names beside the other two and
        // which this loop did not drive. `--no-deref` on all three
        // invocations is unreachable defence in depth *because* this guard
        // runs first, so the guard is the thing that has to be complete —
        // and it was covering two of the three primitives it protects.
        fixture.manager.compare_and_swap_ref(
            &mut NoHooks,
            RefSite::CompareAndSwapIntegration,
            "refs/upstroke/sym",
            &fixture.head,
            &fixture.seed,
        ),
    ] {
        let message = refusal_of(&attempt.expect_err("a symbolic ref refuses"));
        assert!(
            message.contains("symbolic ref") && message.contains("INV-17"),
            "the refusal must name its reason: {message}"
        );
    }
    assert_eq!(
        git(&fixture.base, &["rev-parse", "refs/heads/main"]),
        before,
        "and the victim is untouched"
    );
    assert_eq!(
        git(&fixture.base, &["symbolic-ref", "refs/upstroke/sym"]),
        "refs/heads/main",
        "and the symbolic ref itself is untouched"
    );
}

#[test]
fn a_checked_out_ref_is_refused_before_any_publication() {
    let fixture = Fixture::created("checked-out");
    let message = refusal_of(
        &fixture
            .manager
            .assert_publishable("refs/heads/main")
            .expect_err("a checked-out ref refuses"),
    );
    assert!(
        message.contains("checked out in the worktree"),
        "the refusal must name its reason: {message}"
    );
    fixture
        .manager
        .assert_publishable("refs/heads/upstroke/run-1")
        .expect("a ref no worktree has checked out is publishable");
}

/// A compare-and-swap honours the **caller's recorded** expected-old, not
/// a fresh reading of the ref (`PR5-WORKSPACE-030`).
///
/// `invariants[16].recovery`: "symbolic or **substituted** refs refuse".
/// The suite owns a "wrong expected-old refuses" assertion but drives it
/// through `delete_ref_expected_old`, never through the CAS; and the CAS's
/// two production callers both pass the true current value, so a body that
/// replaced the caller's recorded SHA with a fresh reread produced the
/// identical argument every time it ran. The distinguishing manipulation is
/// a **third** SHA substituted between the caller recording expected-old
/// and the swap — under it, a self-oracle sees its own reading as current
/// and overwrites another writer's value.
#[test]
fn a_compare_and_swap_refuses_a_ref_substituted_since_the_caller_recorded_it() {
    let fixture = Fixture::created("cas-substituted");
    let name = "refs/upstroke/runs/run-1/integration";
    fixture
        .manager
        .create_ref_zero_old(
            &mut NoHooks,
            RefSite::CreateIntegration,
            name,
            &fixture.head,
        )
        .expect("create");
    // What the caller recorded, before anyone else touched the ref.
    let recorded = fixture.head.clone();

    // A third value, from a writer this caller never saw.
    git(&fixture.base, &["update-ref", name, &fixture.side]);
    assert_eq!(
        fixture.manager.direct_ref_target(name).expect("read"),
        Some(fixture.side.clone()),
        "the ref really was substituted"
    );
    assert_ne!(recorded, fixture.side);

    let error = fixture
        .manager
        .compare_and_swap_ref(
            &mut NoHooks,
            RefSite::CompareAndSwapIntegration,
            name,
            &recorded,
            &fixture.seed,
        )
        .expect_err("a substituted ref refuses the swap");
    assert_eq!(
        fixture.manager.direct_ref_target(name).expect("read"),
        Some(fixture.side.clone()),
        "and the other writer's value is untouched: {error}"
    );

    // The same swap against the true current value succeeds, so the
    // refusal above is about the substitution and not about the primitive.
    fixture
        .manager
        .compare_and_swap_ref(
            &mut NoHooks,
            RefSite::CompareAndSwapIntegration,
            name,
            &fixture.side,
            &fixture.seed,
        )
        .expect("expected-old matching the current value swaps");
    assert_eq!(
        fixture.manager.direct_ref_target(name).expect("read"),
        Some(fixture.seed.clone())
    );
}

/// The direct-ref reader refuses a **symbolic ref that resolves to the
/// expected object** (`PR5-WORKSPACE-031`).
///
/// `ref_rules`: "all refs **direct** … symbolic refs refused". Every call
/// of `direct_ref_target` in this file is on a ref the fixture created with
/// `create_ref_zero_old`, so the reader was never once pointed at a
/// symbolic ref; the one test that builds a symbolic ref reads its victim
/// back through a raw `git symbolic-ref` helper instead. The case that
/// separates a non-dereferencing `show-ref --verify` from a dereferencing
/// `rev-parse --verify` is exactly the one never constructed: an indirection
/// that yields the **right** object, and so hides itself.
#[test]
fn a_symbolic_ref_that_resolves_to_the_expected_object_is_still_refused() {
    let fixture = Fixture::created("symbolic-reader");
    let direct = "refs/upstroke/runs/run-1/candidates/kalpha/1";
    let symbolic = "refs/upstroke/runs/run-1/integration";
    fixture
        .manager
        .create_ref_zero_old(
            &mut NoHooks,
            RefSite::CreateCandidates,
            direct,
            &fixture.head,
        )
        .expect("create the direct ref");
    git(&fixture.base, &["symbolic-ref", symbolic, direct]);
    assert_eq!(
        git(&fixture.base, &["rev-parse", "--verify", symbolic]),
        fixture.head,
        "dereferencing yields exactly the object a caller expects, which is what              makes this the hiding case"
    );

    let error = refusal_of(
        &fixture
            .manager
            .direct_ref_target(symbolic)
            .expect_err("a symbolic ref is not a direct one, whatever it resolves to"),
    );
    assert!(
        error.contains("symbolic"),
        "the refusal must name its reason: {error}"
    );
    // And the direct ref beside it still reads back, so the reader has not
    // simply stopped working.
    assert_eq!(
        fixture.manager.direct_ref_target(direct).expect("read"),
        Some(fixture.head.clone())
    );
}

#[test]
fn refs_are_created_zero_old_and_moved_or_deleted_only_expected_old() {
    let fixture = Fixture::created("ref-rules");
    let name = "refs/upstroke/runs/run-1/candidates/kalpha/1";
    fixture
        .manager
        .create_ref_zero_old(&mut NoHooks, RefSite::CreateCandidates, name, &fixture.head)
        .expect("zero-old creation");
    assert_eq!(
        fixture
            .manager
            .direct_ref_target(name)
            .expect("read the ref"),
        Some(fixture.head.clone())
    );

    fixture
        .manager
        .create_ref_zero_old(&mut NoHooks, RefSite::CreateCandidates, name, &fixture.seed)
        .expect_err("zero-old refuses a ref that already exists");
    assert_eq!(
        fixture.manager.direct_ref_target(name).expect("read"),
        Some(fixture.head.clone()),
        "and leaves it where it was"
    );

    fixture
        .manager
        .delete_ref_expected_old(
            &mut NoHooks,
            RefSite::DeleteCandidatesRef,
            name,
            &fixture.seed,
        )
        .expect_err("a wrong expected-old refuses");
    assert_eq!(
        fixture.manager.direct_ref_target(name).expect("read"),
        Some(fixture.head.clone()),
        "and leaves it where it was"
    );

    fixture
        .manager
        .delete_ref_expected_old(
            &mut NoHooks,
            RefSite::DeleteCandidatesRef,
            name,
            &fixture.head,
        )
        .expect("the right expected-old deletes");
    assert_eq!(fixture.manager.direct_ref_target(name).expect("read"), None);
}

/// The trap this project's guard exists for: a fix that introduces a
/// defect. `git update-ref -d <ref> 0{40}` **succeeds and deletes**,
/// because the null id means "must not exist"; a primitive that passed it
/// through would perform an unconditional delete under a name that
/// promises expected-old.
#[test]
fn the_null_object_id_is_never_an_expected_old_value() {
    let fixture = Fixture::created("null-old");
    let name = "refs/upstroke/runs/run-1/candidate-prepared/kalpha/1";
    fixture
        .manager
        .create_ref_zero_old(
            &mut NoHooks,
            RefSite::PinCandidatePrepared,
            name,
            &fixture.head,
        )
        .expect("pin");

    for null in ["0".repeat(40), "0".repeat(64)] {
        let message = refusal_of(
            &fixture
                .manager
                .delete_ref_expected_old(&mut NoHooks, RefSite::DeleteCandidatePin, name, &null)
                .expect_err("the null expected-old refuses"),
        );
        assert!(
            message.contains("null object id") && message.contains("INV-17"),
            "the refusal must name its reason: {message}"
        );
    }
    assert_eq!(
        fixture.manager.direct_ref_target(name).expect("read"),
        Some(fixture.head.clone()),
        "and the pin is still there"
    );

    // The measurement this refusal is derived from: raw Git really does
    // delete on the null id, so the refusal is guarding a live hazard and
    // not a hypothetical one.
    let raw = git_out(
        &fixture.base,
        &["update-ref", "--no-deref", "-d", name, &"0".repeat(40)],
    );
    assert!(
        raw.status.success()
            && fixture
                .manager
                .direct_ref_target(name)
                .expect("read")
                .is_none(),
        "raw `git update-ref -d <ref> 0{{40}}` deletes unconditionally; that is why the \
             primitive refuses it"
    );
}

/// The other side of the same trap (`PR126-OBJECT-NEW-SIDE-ACCEPTS-NULL-ID`):
/// a null **new** value means "must not exist afterwards", so raw Git turns a
/// compare-and-swap whose expected-old matches into a delete, and a create of
/// an absent ref into a success that creates nothing. Both primitives refuse
/// it before the mutating `update-ref`, at both hash lengths whatever the
/// repository's format (the refusal is on the value, and the symbolic-ref
/// and checked-out checks that run before it read Git without mutating), and
/// the raw measurement is executed in a repository of each format, with the
/// null id spelt at that format's length, so the refusal guards a live hazard
/// and not a hypothetical one.
#[test]
fn the_null_object_id_is_never_a_new_value_through_create_or_compare_and_swap() {
    for fixture in [
        Fixture::created("null-new-sha1"),
        Fixture::created_sha256("null-new-sha256"),
    ] {
        null_new_value_refuses_and_raw_git_would_not(&fixture);
    }
}

fn null_new_value_refuses_and_raw_git_would_not(fixture: &Fixture) {
    let existing = "refs/upstroke/runs/run-1/integration";
    let absent = "refs/upstroke/runs/run-1/candidates/kalpha/1";
    fixture
        .manager
        .create_ref_zero_old(
            &mut NoHooks,
            RefSite::CreateIntegration,
            existing,
            &fixture.head,
        )
        .expect("create");

    for null in ["0".repeat(40), "0".repeat(64)] {
        let message = refusal_of(
            &fixture
                .manager
                .compare_and_swap_ref(
                    &mut NoHooks,
                    RefSite::CompareAndSwapIntegration,
                    existing,
                    &fixture.head,
                    &null,
                )
                .expect_err("a null new value refuses the swap"),
        );
        assert!(
            message.contains("null object id") && message.contains("must not exist afterwards"),
            "the refusal must name its reason: {message}"
        );
        assert_eq!(
            fixture
                .manager
                .direct_ref_target(existing)
                .expect("read")
                .as_deref(),
            Some(fixture.head.as_str()),
            "and the ref is still there"
        );

        let message = refusal_of(
            &fixture
                .manager
                .create_ref_zero_old(&mut NoHooks, RefSite::CreateCandidates, absent, &null)
                .expect_err("a null new value refuses the create"),
        );
        assert!(
            message.contains("null object id"),
            "the refusal must name its reason: {message}"
        );
        assert_eq!(
            fixture.manager.direct_ref_target(absent).expect("read"),
            None,
            "and nothing was created"
        );
    }

    // The measurement the refusals are derived from, in this repository's
    // own format: raw Git deletes through the swap when the old value
    // matches, and creates nothing through the create when the ref is
    // absent, exiting 0 both times.
    let null = "0".repeat(fixture.head.len());
    let raw = git_out(
        &fixture.base,
        &["update-ref", "--no-deref", existing, &null, &fixture.head],
    );
    assert!(
        raw.status.success()
            && fixture
                .manager
                .direct_ref_target(existing)
                .expect("read")
                .is_none(),
        "raw `git update-ref <ref> <null> <old>` deletes when the old value matches; that is why \
         the primitive refuses it"
    );
    let raw = git_out(
        &fixture.base,
        &["update-ref", "--no-deref", absent, &null, ""],
    );
    assert!(
        raw.status.success()
            && fixture
                .manager
                .direct_ref_target(absent)
                .expect("read")
                .is_none(),
        "raw `git update-ref <ref> <null> \"\"` exits 0 and creates nothing; that is why the \
         primitive refuses it"
    );
}

#[test]
fn a_malformed_object_id_never_reaches_the_ref_command() {
    let fixture = Fixture::created("malformed-oid");
    for hostile in ["--delete", "", "refs/heads/main", "zzzz", &"a".repeat(39)] {
        let message = refusal_of(
            &fixture
                .manager
                .create_ref_zero_old(
                    &mut NoHooks,
                    RefSite::CreateIntegration,
                    "refs/heads/upstroke/run-1",
                    hostile,
                )
                .expect_err("a malformed object id refuses"),
        );
        assert!(
            message.contains("full hexadecimal object id"),
            "the refusal must name its reason for `{hostile}`: {message}"
        );
    }
    assert_eq!(
        fixture
            .manager
            .direct_ref_target("refs/heads/upstroke/run-1")
            .expect("read"),
        None
    );
}

#[test]
fn an_unexpected_ref_under_the_run_namespace_refuses() {
    let fixture = Fixture::created("unexpected-refs");
    let namespace = "refs/upstroke/runs/run-1/";
    let mine = "refs/upstroke/runs/run-1/candidates/kalpha/1".to_owned();
    fixture
        .manager
        .create_ref_zero_old(
            &mut NoHooks,
            RefSite::CreateCandidates,
            &mine,
            &fixture.head,
        )
        .expect("create");
    fixture
        .manager
        .refuse_unexpected_refs(namespace, std::slice::from_ref(&mine))
        .expect("the namespace carries only what is expected");

    git(
        &fixture.base,
        &[
            "update-ref",
            "refs/upstroke/runs/run-1/stowaway",
            &fixture.seed,
        ],
    );
    let message = refusal_of(
        &fixture
            .manager
            .refuse_unexpected_refs(namespace, std::slice::from_ref(&mine))
            .expect_err("an unexpected ref refuses"),
    );
    assert!(
        message.contains("unexpected ref") && message.contains("stowaway"),
        "the refusal must name its reason and the ref: {message}"
    );
}

/// A **packed** unexpected ref refuses, and so does a **nested** one
/// (`PR5-WORKSPACE-033`).
///
/// `expected_failures_refusals[2]` is "unexpected refs under the run
/// namespace" with no exception for how Git happens to be storing them.
/// The test above plants its stowaway with a plain `update-ref` and never
/// runs `pack-refs`, so the stowaway is a loose file and rewriting
/// `refs_under` to walk `<common git dir>/refs` and ignore `packed-refs`
/// entirely still found it. Nothing in this file called `pack-refs` at all,
/// and no fixture nested a ref deeper than the two-level
/// `candidates/kalpha/1`.
#[test]
fn a_packed_or_nested_unexpected_ref_refuses_too() {
    let fixture = Fixture::created("packed-refs");
    let namespace = "refs/upstroke/runs/run-1/";
    let mine = "refs/upstroke/runs/run-1/candidates/kalpha/1".to_owned();
    fixture
        .manager
        .create_ref_zero_old(
            &mut NoHooks,
            RefSite::CreateCandidates,
            &mine,
            &fixture.head,
        )
        .expect("create");

    let nested = "refs/upstroke/runs/run-1/candidates/kalpha/deeper/still/1";
    git(&fixture.base, &["update-ref", nested, &fixture.seed]);
    git(&fixture.base, &["pack-refs", "--all"]);
    assert!(
        fixture.base.join(".git/packed-refs").is_file(),
        "the fixture really packed the refs"
    );
    assert!(
        !fixture.base.join(".git").join(nested).exists(),
        "and the stowaway is no longer a loose file, which is the whole point"
    );

    let listed: Vec<String> = fixture
        .manager
        .refs_under(namespace)
        .expect("enumerate")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(
        listed.contains(&nested.to_owned()),
        "a packed ref is still a ref under the namespace: {listed:?}"
    );

    let message = refusal_of(
        &fixture
            .manager
            .refuse_unexpected_refs(namespace, std::slice::from_ref(&mine))
            .expect_err("a packed, nested, unexpected ref refuses"),
    );
    assert!(
        message.contains("unexpected ref") && message.contains("deeper/still"),
        "the refusal must name the packed nested ref: {message}"
    );

    // And every ref is still exactly where it was: a refusal acts on
    // nothing.
    assert_eq!(
        fixture.manager.direct_ref_target(&mine).expect("read"),
        Some(fixture.head.clone())
    );
    assert_eq!(
        fixture.manager.direct_ref_target(nested).expect("read"),
        Some(fixture.seed.clone())
    );
}

/// The integration ref checked out in a **second linked worktree** refuses
/// (`PR5-WORKSPACE-032`).
///
/// `integration_ref`: "never checked out; `assert_publishable()` before
/// every prepare/CAS/recovery". The one test of the refusal asks about
/// `refs/heads/main`, which is checked out in the *primary* worktree — so
/// truncating the scan to the first worktree record still refused it, and
/// the negative case is a ref checked out nowhere. A linked worktree is
/// exactly the shape this manager creates for its own work, so it is the
/// one that had to be built.
#[test]
fn an_integration_ref_checked_out_in_a_second_worktree_is_refused() {
    let fixture = Fixture::created("checked-out-elsewhere");
    let refname = "refs/heads/upstroke/run-1";
    git(&fixture.base, &["branch", "upstroke/run-1", &fixture.head]);
    fixture
        .manager
        .assert_publishable(refname)
        .expect("checked out nowhere yet");

    let elsewhere = fixture.root.join("elsewhere");
    git(
        &fixture.base,
        &[
            "worktree",
            "add",
            "-q",
            &elsewhere.to_string_lossy(),
            "upstroke/run-1",
        ],
    );
    assert_eq!(
        git(&elsewhere, &["symbolic-ref", "-q", "--", "HEAD"]),
        refname,
        "the second worktree really has the integration ref checked out"
    );
    assert!(
        fixture.manager.worktree_records().expect("records").len() >= 2,
        "and it is not the first record, which a truncated scan would still see"
    );

    let message = refusal_of(
        &fixture
            .manager
            .assert_publishable(refname)
            .expect_err("a ref checked out in a linked worktree refuses"),
    );
    assert!(
        message.contains("checked out in the worktree"),
        "the refusal must name its reason: {message}"
    );
}

// -----------------------------------------------------------------------
// Intents: synced before the add, and reclaimed
// -----------------------------------------------------------------------

#[test]
fn the_intent_is_durable_before_the_add_and_reclaim_removes_it() {
    let fixture = Fixture::created("intent-order");
    let (mut hooks, shared) = harness();
    let slot = fixture.task("alpha", 1);

    fixture
        .manager
        .write_intent(&mut hooks, &slot)
        .expect("intent");
    assert!(
        fixture.manager.intent_path(&slot).is_file(),
        "the intent is durable before the add is issued"
    );
    assert!(
        !fixture.manager.slot_path(&slot).exists(),
        "and the worktree does not exist yet — this is the interrupted-add prefix"
    );

    // The cancellation clause, exactly: "an interrupted worktree or
    // snapshot add leaves a durable intent that reclaim removes."
    let reclaimed = fixture
        .manager
        .reclaim_intents(&mut hooks)
        .expect("reclaim");
    assert_eq!(reclaimed.slots, vec![slot.clone()]);
    assert!(!fixture.manager.intent_path(&slot).exists());
    assert!(fixture.manager.intents().expect("intents").is_empty());

    // And the hook order the sentence is about, from the harness's own
    // first-observation order.
    fixture
        .manager
        .write_intent(&mut hooks, &slot)
        .expect("intent again");
    fixture
        .manager
        .add_worktree(&mut hooks, &slot, &fixture.head)
        .expect("add");
    let observed = shared.lock().expect("harness").coverage().to_vec();
    let index = |site: EffectSiteId, phase: HookPhase| {
        observed
            .iter()
            .position(|seen| seen.site == site && seen.phase == phase)
            .unwrap_or_else(|| panic!("{site} {phase} was never observed"))
    };
    assert!(
        index(
            EffectSiteId::Worktree(WorktreeSite::WriteIntent),
            HookPhase::After
        ) < index(EffectSiteId::Worktree(WorktreeSite::Add), HookPhase::Before),
        "the intent's after phase precedes the add's before phase"
    );
}

/// A refusal at `Before(Worktree.Add)` refuses **before any effect**
/// (`PR5-CONF-003`).
///
/// `identity` says "the funnel itself calls hook(Before, site) -> primitive
/// -> hook(After, site)" and `scope` requires "every effect through typed
/// funnel APIs taking a site". `add_worktree`'s scaffolding
/// `fs::create_dir_all` sat *outside* the `funnel(...)` call, so the Before
/// hook was not the first thing that happened: the directory was already on
/// disk when the refusal was returned. The module doc at the top of this file
/// claims "every effect is issued inside a `funnel` call", and
/// `effects/wrappers.toml` classified `add_worktree` as `effect_free` —
/// which that file defines as "reaches no effect of its own".
///
/// The two axes this crosses are the *hook answer* and the *filesystem*. The
/// sibling below holds the hook answer at `Proceed` and reads the durability
/// ledger; every other add test proceeds too. What varies here is the
/// answer — `Injection::Error` at the add's Before — and what is held
/// constant is the state the effect would change: the scaffolding directory
/// is asserted absent before the call, so its absence afterwards is the
/// claim rather than an accident of the fixture.
#[test]
fn a_refusal_at_the_adds_before_hook_leaves_the_filesystem_untouched() {
    /// Refuses at the add's `Before`, and records that it did.
    #[derive(Default)]
    struct RefuseAtAddBefore {
        refused: bool,
    }

    impl EffectHooks for RefuseAtAddBefore {
        fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            if site == EffectSiteId::Worktree(WorktreeSite::Add) && phase == HookPhase::Before {
                self.refused = true;
                return Injection::Error;
            }
            Injection::Proceed
        }

        fn refusal_cause(&self) -> Option<String> {
            None
        }
    }

    let fixture = Fixture::created("add-before-refusal");
    let slot = fixture.task("alpha", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("the intent must be durable, or the add refuses for another reason");

    // The directory the effect would create. `slot_path` is private to the
    // manager, so it is derived the way the funnel derives it and then
    // asserted absent — a fixture that already had it would pass this test
    // for a funnel that created it far too early.
    let target = fixture.manager.execution_root().join(slot.relative());
    let scaffolding = target
        .parent()
        .expect("the slot target has a parent")
        .to_path_buf();
    let _ = fs::remove_dir_all(&scaffolding);
    assert!(
        !scaffolding.exists(),
        "the premise: the scaffolding directory must be absent before the call"
    );

    let mut hooks = RefuseAtAddBefore::default();
    let refusal = fixture
        .manager
        .add_worktree(&mut hooks, &slot, &fixture.head)
        .expect_err("the armed Before hook must refuse the add");
    assert!(
        hooks.refused,
        "the hook never fired, so nothing here is measured"
    );
    assert!(
        refusal.to_string().contains("before"),
        "the refusal must name the phase it came from: {refusal}"
    );
    assert!(
        !scaffolding.exists(),
        "the add's Before hook refused and {} exists anyway: an effect ran \
             before the funnel's first hook",
        scaffolding.display()
    );
    assert!(
        !target.exists(),
        "the worktree itself must not exist either: {}",
        target.display()
    );
}

/// The intent is **synced** — file and containing directory — before the
/// add's first hook (`PR5-WORKSPACE-015`, `PR5-WORKSPACE-016`).
///
/// `invariants_introduced[1]` is "worktree and snapshot intents **synced**
/// before add", and the test above checks that the intent *exists and
/// parses* before `Worktree.Add` fires. Those are different claims, and an
/// unsynced file satisfies the weaker one exactly as well as a synced one:
/// with both `sync_all` calls deleted from `write_synced`, every assertion
/// in this file still passed. The observer below crosses the two axes the
/// lane had separately — the hook order, and the durability ledger — by
/// reading the ledger *at* the add's `Before` hook rather than afterwards.
#[test]
fn the_intent_and_its_directory_are_synced_before_the_add_begins() {
    /// Snapshots the durability ledger at the first `Worktree.Add` Before.
    struct LedgerAtAdd {
        inner: HarnessEffects,
        at_add: Option<Vec<crate::util::DurableRecord>>,
    }

    impl EffectHooks for LedgerAtAdd {
        fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            if site == EffectSiteId::Worktree(WorktreeSite::Add)
                && phase == HookPhase::Before
                && self.at_add.is_none()
            {
                self.at_add = Some(self.inner.ledger().records());
            }
            self.inner.phase(site, phase)
        }

        fn durability_ledger(&self) -> DurabilityLedger {
            self.inner.durability_ledger()
        }

        fn refusal_cause(&self) -> Option<String> {
            self.inner.refusal_cause()
        }
    }

    let fixture = Fixture::created("intent-durability");
    let slot = fixture.task("alpha", 1);
    let intent = fixture.manager.intent_path(&slot);
    let intents_dir = intent
        .parent()
        .expect("the intents directory")
        .to_path_buf();
    let mut hooks = LedgerAtAdd {
        inner: HarnessEffects::new(Arc::new(Mutex::new(HookHarness::new()))).recording_durability(),
        at_add: None,
    };

    fixture
        .manager
        .write_intent(&mut hooks, &slot)
        .expect("intent");
    fixture
        .manager
        .add_worktree(&mut hooks, &slot, &fixture.head)
        .expect("add");

    let at_add = hooks
        .at_add
        .expect("the add's Before hook never fired, so nothing here is measured");
    let steps: Vec<DurableStep> = at_add.iter().map(|record| record.step).collect();
    // The staged file, its rename, and the directory entry: both halves of
    // `write_synced`'s durability contract, on every platform
    // (`PR5-CONF-013`). This used to fork on `cfg!(unix)` because a
    // directory fsync was held not to be a call this crate could make on
    // Windows; `util::fsync_dir` makes it.
    let expected = vec![
        DurableStep::SyncedFile,
        DurableStep::Renamed,
        DurableStep::SyncedDirectory,
    ];
    assert_eq!(
        steps, expected,
        "the durability sequence complete at the moment the add begins: {at_add:?}"
    );
    // The staged name is unique per call and never the published one; what
    // is pinned is where it lived and that it is gone once published.
    let staged = &at_add[0].path;
    assert!(
        staged.parent() == Some(intents_dir.as_path())
            && staged != &intent
            && staged
                .extension()
                .is_some_and(|extension| extension == "tmp")
            && !staged.exists(),
        "the sync is of the staged intent, in the intents directory, under a name that is \
         not the published one and is gone once published: {}",
        staged.display()
    );
    assert_eq!(
        at_add[0].len,
        fs::metadata(&intent).expect("the intent").len(),
        "the whole intent file was synced, not a prefix of it"
    );
    assert!(at_add[0].len > 0, "the intent has bytes at all");
    assert_eq!(
        at_add[1].path, intent,
        "the rename lands on the intent name"
    );
    #[cfg(unix)]
    assert_eq!(
        at_add[2].path, intents_dir,
        "the directory sync is of the directory the rename changed"
    );
    let _ = &intents_dir;
}

/// `snapshots`: "an interrupted add leaves a registered-but-unpopulated
/// worktree that the intent-based reclaim removes and prunes".
///
/// The residue is constructed the way `git worktree add` leaves it — the
/// registration plus the `initializing` lock Git itself holds for the whole
/// of the add — because measured, `git worktree prune` **skips** a locked
/// entry and `git worktree remove --force` refuses one. A reclaim that did
/// not clear the lock would leave exactly the residue `cleanup` promises
/// never blocks it.
#[test]
fn reclaim_removes_a_registered_but_unpopulated_worktree() {
    let fixture = Fixture::created("unpopulated");
    let slot = fixture.task("alpha", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("intent");
    let path = fixture.manager.slot_path(&slot);
    register_unpopulated(&fixture, &path);

    assert!(
        fixture
            .manager
            .worktree_records()
            .expect("records")
            .iter()
            .any(|record| record.locked.as_deref() == Some("initializing")),
        "the fixture must really build the residue it is about"
    );
    assert_eq!(
        fixture
            .manager
            .quiescence(&path, &Quiescence::AtBase(fixture.head.clone()))
            .expect("verify"),
        Err(VerifyFailure::Unpopulated)
    );

    fixture
        .manager
        .reclaim_intents(&mut NoHooks)
        .expect("reclaim");
    assert!(
        !fixture
            .manager
            .worktree_records()
            .expect("records")
            .iter()
            .any(|record| record.path.ends_with("kalpha-g1")),
        "the registration is pruned"
    );
    assert!(!path.exists());
    assert!(!fixture.manager.intent_path(&slot).exists());
}

/// A non-canonical intent name beside the canonical one is refused before
/// anything is removed (`PR118-RECLAIM-REGRESSION-PINNED-AT-PARSER-ONLY`).
///
/// `str::parse` reads `03` as 3, so before `Slot::from_intent_name` compared
/// the parsed slot with its own rendering, `tasks.kalpha-g03.intent` produced
/// the slot whose intent file is `tasks.kalpha-g3.intent`: reclaim removed the
/// legitimate worktree and the canonical intent and left the `g03` file for
/// every later start to enumerate again. The parser's own test pins the
/// verdict; this one pins the composition through `intents()` and
/// `reclaim_intents()`: the walk refuses the file it cannot account for, the
/// refusal names the file, and it comes before any removal.
#[test]
fn reclaim_refuses_a_non_canonical_intent_name_before_removing_anything() {
    let fixture = Fixture::created("non-canonical-intent");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 3);
    let worktree = fixture.manager.slot_path(&slot);
    let canonical = fixture.manager.intent_path(&slot);
    assert!(
        worktree.is_dir() && canonical.is_file(),
        "the legitimate slot exists before the stray file is planted"
    );
    let bytes = fs::read(&canonical).expect("read the canonical intent");
    let stray = canonical.with_file_name("tasks.kalpha-g03.intent");
    fs::write(&stray, &bytes).expect("plant the non-canonical intent");

    let error = fixture
        .manager
        .reclaim_intents(&mut NoHooks)
        .expect_err("the walk refuses a file no slot renders");
    assert!(
        matches!(
            &error,
            UpstrokeError::Git { message }
                if message.contains("unexpected file `tasks.kalpha-g03.intent`")
        ),
        "refused for the wrong reason: {error}"
    );
    assert!(worktree.is_dir(), "the legitimate worktree is untouched");
    assert_eq!(
        fs::read(&canonical).expect("read the canonical intent back"),
        bytes,
        "the canonical intent is untouched"
    );
    assert!(
        stray.is_file(),
        "the stray file is left for the operator: nothing is deleted on inference"
    );
}

#[test]
fn reclaim_removes_the_registration_whose_commondir_is_empty() {
    let fixture = Fixture::created("empty-commondir");
    let slot = fixture.task("alpha", 1);
    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("intent");
    let path = fixture.manager.slot_path(&slot);
    register_unpopulated(&fixture, &path);
    let admin = fixture
        .manager
        .revalidate_removal(&path)
        .expect("admin dir")
        .expect("registered");
    fs::write(admin.join("commondir"), []).expect("truncate commondir");

    fixture
        .manager
        .reclaim_intents(&mut NoHooks)
        .expect("the exact registration is recoverable without Git enumeration");
    assert!(!path.exists(), "the unpopulated checkout stays absent");
    assert!(!admin.exists(), "the proved registration is removed");
    assert!(!fixture.manager.intent_path(&slot).exists());
    fixture
        .manager
        .worktree_records()
        .expect("Git enumeration works again");
}

/// `target` relative to `from`, both canonicalised: `..` up to the common
/// ancestor, then down. What Git 2.48's `worktree.useRelativePaths` writes.
fn relative_from(from: &Path, target: &Path) -> PathBuf {
    let from = fs::canonicalize(from).expect("from exists");
    let target = fs::canonicalize(target).expect("target exists");
    let mut from = from.components().peekable();
    let mut target = target.components().peekable();
    while from.peek().is_some() && from.peek() == target.peek() {
        from.next();
        target.next();
    }
    let mut relative = PathBuf::new();
    for _ in from {
        relative.push("..");
    }
    for component in target {
        relative.push(component);
    }
    relative
}

/// A relative registration, Git 2.48's `worktree.useRelativePaths` form, is
/// joined to its registration directory and canonicalised, so the slot's own
/// registration rewritten relative still binds the slot; one that resolves to a
/// foreign directory inside the root refuses by containment, as an absolute
/// one would.
#[test]
fn a_relative_registration_still_binds_its_checkout() {
    let fixture = Fixture::created("relative-registration");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let admin = fixture
        .manager
        .revalidate_removal(&path)
        .expect("admin dir")
        .expect("registered");
    let relative = relative_from(&admin, &path.join(".git"));
    assert!(
        relative.starts_with(".."),
        "the fixture wrote a relative path"
    );
    fs::write(admin.join("gitdir"), format!("{}\n", relative.display())).expect("relative gitdir");
    let bound = fixture
        .manager
        .revalidate_removal(&path)
        .expect("a relative registration resolves")
        .expect("and still binds the slot");
    assert_eq!(bound, admin);

    let foreign = fixture.manager.execution_root().join("foreign");
    fs::create_dir_all(&foreign).expect("a foreign directory inside the root");
    let relative = relative_from(&admin, &foreign);
    fs::write(
        admin.join("gitdir"),
        format!("{}/.git\n", relative.display()),
    )
    .expect("relative gitdir naming the foreign directory");
    fixture
        .manager
        .revalidate_removal(&path)
        .expect_err("a registration resolving to a foreign directory inside the root refuses");
}

/// Git failing to enumerate is an error, never "not registered": a zero-length
/// `commondir`, the interrupted-add residue `revalidate_removal` documents,
/// makes `git worktree list` fail, and the classifier propagates that rather
/// than reading the registered-but-unpopulated worktree as absent.
#[test]
fn a_failed_worktree_list_is_an_error_not_an_absent_registration() {
    let fixture = Fixture::created("failed-list");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let admin = fixture
        .manager
        .revalidate_removal(&path)
        .expect("admin dir")
        .expect("registered");
    fs::write(admin.join("commondir"), []).expect("truncate commondir");
    let error = record_for(&fixture.base, &path).expect_err("Git could not enumerate");
    assert!(
        error.to_string().contains("worktree list"),
        "the error names the command: {error}"
    );
    classify_object_residue(
        EffectSiteId::Worktree(WorktreeSite::Add),
        &ResidueTarget::new(&fixture.base).at(&path),
    )
    .expect_err("the classifier propagates the failure and does not answer for Git");
}

#[test]
fn malformed_gitdir_refuses_before_removal() {
    for (case, bytes) in [("empty", &b""[..]), ("partial", &b"not-a-dot-git-path"[..])] {
        let fixture = Fixture::created(&format!("bad-gitdir-{case}"));
        let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
        let path = fixture.manager.slot_path(&slot);
        let admin = fixture
            .manager
            .revalidate_removal(&path)
            .expect("admin dir")
            .expect("registered");
        fs::write(admin.join("gitdir"), bytes).expect("replace gitdir");

        fixture
            .manager
            .remove_worktree(&mut NoHooks, &slot)
            .expect_err("unbound registration refuses");
        assert!(path.exists(), "{case}: refusal precedes checkout deletion");
        assert!(
            admin.exists(),
            "{case}: refusal precedes registration deletion"
        );
    }
}

#[test]
fn an_absent_registration_gitdir_is_already_gone_for_forced_cleanup() {
    let fixture = Fixture::created("absent-gitdir-converges");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let admin = fixture
        .manager
        .revalidate_removal(&path)
        .expect("admin dir")
        .expect("registered");
    fs::remove_file(admin.join("gitdir")).expect("interrupt before gitdir survives");

    fixture
        .manager
        .remove_worktree(&mut NoHooks, &slot)
        .expect("missing identity metadata is forced-cleanup convergence");
    assert!(!path.exists(), "the exact contained checkout is reclaimed");
}

#[test]
fn a_missing_stored_worktree_directory_refuses_before_checkout_deletion() {
    let fixture = Fixture::created("missing-worktrees-store");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let admin = fixture
        .manager
        .revalidate_removal(&path)
        .expect("admin dir")
        .expect("registered");
    let worktrees = admin.parent().expect("worktrees directory").to_path_buf();
    let moved = fixture.root.join("stored-worktrees-moved");
    fs::rename(&worktrees, &moved).expect("move the stored metadata");

    fixture
        .manager
        .remove_worktree(&mut NoHooks, &slot)
        .expect_err("missing stored metadata refuses");
    assert!(path.exists(), "refusal precedes checkout deletion");
    fs::rename(moved, worktrees).expect("restore fixture metadata");
}

#[test]
fn a_registration_rebound_after_validation_keeps_its_admin_state() {
    struct RebindAtBefore {
        gitdir: PathBuf,
        replacement: PathBuf,
    }

    impl EffectHooks for RebindAtBefore {
        fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            if site == EffectSiteId::Worktree(WorktreeSite::Remove) && phase == HookPhase::Before {
                fs::write(&self.gitdir, format!("{}\n", self.replacement.display()))
                    .expect("replace the registration identity");
            }
            Injection::Proceed
        }

        fn refusal_cause(&self) -> Option<String> {
            None
        }
    }

    let fixture = Fixture::created("admin-rebound");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let admin = fixture
        .manager
        .revalidate_removal(&path)
        .expect("admin dir")
        .expect("registered");
    let locked = admin.join("locked");
    fs::write(&locked, "do not remove\n").expect("replacement state");
    let other = fixture.root.join("other").join(".git");
    let mut hooks = RebindAtBefore {
        gitdir: admin.join("gitdir"),
        replacement: other,
    };

    fixture
        .manager
        .remove_worktree(&mut hooks, &slot)
        .expect_err("changed identity refuses");
    assert!(locked.exists(), "identity is rechecked before lock removal");
    assert!(admin.exists(), "identity is rechecked before admin removal");
}

#[test]
fn registration_gitdir_requires_an_absolute_normalized_path() {
    // A relative `gitdir` is Git 2.48's own form and is joined to the
    // registration directory (`a_relative_registration_still_binds_its_checkout`);
    // what an absolute one may not do is traverse or alias.
    let admin = Path::new("/repository/.git/worktrees/example");
    for bytes in [
        &b"/absolute/../traversal/.git"[..],
        &b"/absolute/./alias/.git"[..],
    ] {
        registration_checkout(admin, bytes).expect_err("non-normalized gitdir refuses");
    }
}

#[cfg(windows)]
#[test]
fn registration_gitdir_refuses_invalid_utf8_on_windows() {
    registration_checkout(
        Path::new(r"C:\repository\.git\worktrees\example"),
        b"C:\\worktree-\xff\\.git",
    )
    .expect_err("lossy path aliases are not registration identity");
}

#[cfg(unix)]
#[test]
fn registration_gitdir_decodes_non_utf8_path_bytes() {
    use std::os::unix::ffi::OsStrExt as _;

    let decoded = registration_checkout(
        Path::new("/repository/.git/worktrees/example"),
        b"/tmp/non-utf8-\xff/.git\n",
    )
    .expect("byte-valid registration");
    assert_eq!(
        decoded.as_os_str().as_bytes(),
        b"/tmp/non-utf8-\xff",
        "registration discovery is byte-preserving on Unix"
    );
}

/// Build the state a killed `git worktree add` leaves: the registration
/// exists and Git still holds its `initializing` lock.
fn register_unpopulated(fixture: &Fixture, path: &Path) {
    git(
        &fixture.base,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &path.to_string_lossy(),
            &fixture.head,
        ],
    );
    let admin = fixture
        .manager
        .revalidate_removal(path)
        .expect("admin dir")
        .expect("the worktree is registered");
    fs::write(admin.join("locked"), "initializing\n").expect("hold the initializing lock");
    fs::remove_dir_all(path).expect("un-populate the checkout");
}

/// The other half of the cancellation clause: "an ephemeral snapshot commit
/// created *before* the intent is left to Git".
#[test]
fn an_ephemeral_snapshot_commit_created_before_the_intent_is_left_to_git() {
    let fixture = Fixture::created("ephemeral-before-intent");
    let (mut hooks, _shared) = harness();
    let tree = git(
        &fixture.base,
        &["rev-parse", &format!("{}^{{tree}}", fixture.head)],
    );

    let commit = fixture
        .manager
        .snapshot_commit_tree(&mut hooks, &tree, &fixture.head)
        .expect("ephemeral commit");
    let slot = Slot::Snapshot {
        name: SnapshotName::gates(1, 1),
    };
    assert!(
        !fixture.manager.intent_path(&slot).exists(),
        "the object exists and nothing durable claims it yet"
    );
    assert!(
        unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&commit),
        "so it is unreferenced R27 residue: Git's"
    );

    // Nothing reclaims it and nothing may: the resume action is to leave it.
    fixture
        .manager
        .reclaim_intents(&mut hooks)
        .expect("reclaim finds no intent");
    assert!(
        unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&commit),
        "reclaim leaves the object exactly where Git put it"
    );

    // And the full sequence puts the commit-tree before the intent.
    //
    // **On a harness of its own** (`PR5-WORKSPACE-022`). `HookHarness::
    // coverage()` is a *first-observation* log — one entry per `(site,
    // phase)` however many times it fires — and this test has already
    // driven `snapshot_commit_tree` standalone above, through `hooks`. So
    // the entry a `position()` first-match found for
    // `SnapshotCommitTree/After` was that earlier, unrelated invocation,
    // which precedes every event `add_snapshot` emits whatever order
    // `add_snapshot` uses internally: the assertion below passed with the
    // intent written first, which is exactly what it exists to forbid, and
    // the ordering it names was never measured at all. A fresh harness
    // starts empty, so every index below is this call's own. Taking a
    // *mark* into the old log would not have worked either — the second
    // occurrence is not recorded, so there is nothing after the mark.
    let (mut measured, ordering) = harness();
    let snapshot = fixture
        .manager
        .add_snapshot(
            &mut measured,
            &SnapshotName::gates(2, 1),
            &SnapshotInput::Tree {
                tree: tree.clone(),
                parent: fixture.head.clone(),
            },
        )
        .expect("snapshot");
    let observed = ordering.lock().expect("harness").coverage().to_vec();
    let index = |site: EffectSiteId, phase: HookPhase| {
        observed
            .iter()
            .position(|seen| seen.site == site && seen.phase == phase)
            .unwrap_or_else(|| panic!("{site} {phase} was never observed inside this add_snapshot"))
    };
    assert!(
        index(
            EffectSiteId::Object(ObjectSite::SnapshotCommitTree),
            HookPhase::After
        ) < index(
            EffectSiteId::Snapshot(SnapshotSite::WriteIntent),
            HookPhase::Before
        ),
        "the ephemeral commit is created before the intent"
    );
    // The fresh harness is load-bearing, so it is checked rather than
    // trusted: this log holds this add's own commit-tree and nothing
    // earlier could have supplied it.
    assert_eq!(
        ordering.lock().expect("harness").count(
            EffectSiteId::Object(ObjectSite::SnapshotCommitTree),
            HookPhase::After
        ),
        1,
        "this add's commit-tree fired exactly once, on a log that began empty"
    );
    assert_eq!(snapshot.ephemeral.as_deref(), Some(snapshot.head.as_str()));
    assert!(
        !unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&snapshot.head),
        "and the add makes it the snapshot HEAD: R24, no longer R27"
    );
}

/// An integration snapshot **creates no object**, and two snapshot names in
/// one repository are two live checkouts (`PR5-CONF-007`, `PR5-CONF-008`).
///
/// One function, two clauses of `workspace_candidates`, and neither had a
/// witness — both mutations survived the whole suite:
///
/// * make the `SnapshotInput::Commit` arm fabricate an ephemeral commit and
///   return it as the head, against "integration snapshots check out the
///   proposal or head commit and **create no object**";
/// * ignore the supplied `SnapshotName`, derive one slot from the judged
///   tree and hand back the existing checkout on later calls, against "one
///   snapshot for the gate set and one fresh snapshot per reviewer, **never
///   reused across roles or attempts**".
///
/// The measured cause was the same for both: `SnapshotInput::Commit` and
/// `SnapshotName::review` were **constructed nowhere in the crate**, and
/// `add_snapshot`'s two callers each used a separate fixture, so no fixture
/// ever held two snapshots alive at once. The recorded carry justification
/// said a second live request needed orchestration "PR5's scope stops
/// before"; it is two calls in one fixture, and here they are.
/// `review-common`'s standing ruling is the general form: *"'No production
/// caller' has a shelf life of one slice."*
///
/// The two axes are the *input variant* and the *name*, and this test is one
/// test rather than two because the surviving pair is one function: every
/// other `add_snapshot` call in the tree holds both constant at
/// `Tree`/`gates(1, 1)`.
#[test]
fn snapshots_create_no_object_for_a_commit_and_never_share_a_checkout() {
    let fixture = Fixture::created("snapshot-clauses");
    let common = PathBuf::from(git(
        &fixture.base,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ));

    // (1) The integration case: an existing commit, and no object created.
    let before = loose_objects(&common);
    let integration = fixture
        .manager
        .add_snapshot(
            &mut NoHooks,
            &SnapshotName::integration(1),
            &SnapshotInput::Commit(fixture.head.clone()),
        )
        .expect("Snapshot.WriteIntent + Snapshot.Add");
    assert_eq!(
        integration.head, fixture.head,
        "an integration snapshot checks out the commit it was given"
    );
    assert_eq!(
        integration.ephemeral, None,
        "…and records no ephemeral commit, because it created none"
    );
    assert_eq!(
        loose_objects(&common),
        before,
        "an integration snapshot created an object; `workspace_candidates` says it \
             checks out the proposal or head commit and **creates no object**"
    );
    assert_eq!(
        git(&integration.path, &["rev-parse", "HEAD"]),
        fixture.head,
        "and the checkout really is at that commit"
    );

    // (2) Two names, one fixture, both alive at once — the shape no fixture
    // in the tree built, and the whole reason the name could be ignored.
    let tree = git(&fixture.base, &["rev-parse", "HEAD^{tree}"]);
    let gates = fixture
        .manager
        .add_snapshot(
            &mut NoHooks,
            &SnapshotName::gates(1, 1),
            &SnapshotInput::Tree {
                tree: tree.clone(),
                parent: fixture.head.clone(),
            },
        )
        .expect("the gate snapshot");
    let reviewer = fixture
        .manager
        .add_snapshot(
            &mut NoHooks,
            &SnapshotName::review(1, 1, 0),
            &SnapshotInput::Tree {
                tree: tree.clone(),
                parent: fixture.head.clone(),
            },
        )
        .expect("a reviewer's snapshot on the same judged tree");

    assert_ne!(
        gates.slot, reviewer.slot,
        "the gate set and a reviewer are different roles and must not share a slot"
    );
    assert_ne!(
        gates.path,
        reviewer.path,
        "…and therefore not a checkout either: {} vs {}",
        gates.path.display(),
        reviewer.path.display()
    );
    assert!(
        gates.path.is_dir() && reviewer.path.is_dir(),
        "both snapshots are live at once; that is what 'never reused across roles \
             or attempts' means and what no fixture built"
    );
    // Not merely different names for one directory: each is separately
    // registered, and the kernel agrees they are two.
    assert_ne!(
        git(&gates.path, &["rev-parse", "--absolute-git-dir"]),
        git(&reviewer.path, &["rev-parse", "--absolute-git-dir"]),
        "two registered worktrees, not one directory under two names"
    );

    // The same role at a later attempt is a third, again without reuse.
    let retry = fixture
        .manager
        .add_snapshot(
            &mut NoHooks,
            &SnapshotName::gates(1, 2),
            &SnapshotInput::Tree {
                tree,
                parent: fixture.head.clone(),
            },
        )
        .expect("the gate snapshot of the next attempt");
    assert_ne!(
        retry.path, gates.path,
        "attempt 2's gate snapshot must not be attempt 1's checkout"
    );

    for snapshot in [&integration, &gates, &reviewer, &retry] {
        fixture
            .manager
            .remove_snapshot(&mut NoHooks, snapshot)
            .expect("Snapshot.Remove + Snapshot.RemoveIntent");
    }
}

// -----------------------------------------------------------------------
// Worktree.Verify and forced removal
// -----------------------------------------------------------------------

/// Every loose object in `objects/??/`, sorted.
///
/// Loose rather than `fsck`-reachable on purpose: a tree `write-tree`
/// creates is referenced by nothing, so a reachability oracle would not see
/// it, and the thing `identity` forbids is the *write*, not the reference.
fn loose_objects(common_git_dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let objects = common_git_dir.join("objects");
    let Ok(fanout) = fs::read_dir(&objects) else {
        return found;
    };
    for entry in fanout.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.len() != 2 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        if let Ok(inner) = fs::read_dir(entry.path()) {
            for object in inner.flatten() {
                found.push(format!("{name}{}", object.file_name().to_string_lossy()));
            }
        }
    }
    found.sort();
    found
}

/// `Worktree.Verify` writes **nothing** — no object, and not the index
/// (`PR5-CONF-002`).
///
/// `identity` says "Worktree.Verify is a read-only quiescence observation
/// (no effect)" and `WorktreeSite::Verify::is_read_only()` is frozen at
/// `true`. The implementation ran `git write-tree`, whose own comment
/// claimed it "creates no object that is not already implied by the index it
/// reads" — and measured against git 2.43.0, an index carrying staged
/// content whose tree object was never written gains **two loose objects**,
/// with the index rewritten 104 → 165 bytes as the `TREE` cache-tree
/// extension is added. A second `git write-tree` inserted into `quiescence`
/// survived the whole suite, because nothing observed the object store or
/// the index around Verify.
///
/// The two axes this crosses are the *verdict* and the *state of the object
/// store*. Every other Verify test holds the store constant — it calls
/// `write-tree` in the fixture first, which leaves a valid cache-tree and
/// every tree already present, the one state in which `write-tree` writes
/// nothing. What varies here is the state: the reachable prefix
/// `Object.CandidateStage` leaves *before* `Object.CandidateWriteTree` runs.
/// Both verdicts are driven in it, so a repair that were read-only only on
/// the failing path would fail here.
#[test]
fn verify_writes_no_object_and_does_not_rewrite_the_index() {
    let fixture = Fixture::created("verify-readonly");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let common = PathBuf::from(git(
        &path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ));
    let index = git_dir_of(&path)
        .expect("git dir")
        .expect("a linked worktree")
        .join("index");

    // ---- The mismatch verdict, in the state where write-tree writes ----
    //
    // The reachable prefix: content staged into the index, and no tree
    // object written for it. `git add` writes the blob and invalidates the
    // cache-tree; the trees are what `Object.CandidateWriteTree` would add.
    // The recorded tree is an *older* one that really is in the store —
    // which is the production shape, since it was written by an earlier
    // `Object.CandidateWriteTree`.
    fs::write(path.join("staged.txt"), "staged\n").expect("stage a file");
    fs::create_dir_all(path.join("nested")).expect("a subdirectory");
    fs::write(path.join("nested/deep.txt"), "deep\n").expect("stage a nested file");
    git(&path, &["add", "-A"]);
    let recorded = git(&path, &["rev-parse", "HEAD^{tree}"]);

    let before_objects = loose_objects(&common);
    let before_index = fs::read(&index).expect("the index");
    let mismatch = fixture
        .manager
        .verify_worktree(
            &mut NoHooks,
            &slot,
            &Quiescence::HoldsTree(recorded.clone()),
        )
        .expect("verify");
    assert!(
        matches!(mismatch, Err(VerifyFailure::TreeMismatch { .. })),
        "staged content the recorded tree does not carry is a mismatch: {mismatch:?}"
    );
    assert_eq!(
        loose_objects(&common),
        before_objects,
        "Worktree.Verify created an object; `identity` calls it a read-only \
             observation with no effect"
    );
    assert_eq!(
        fs::read(&index).expect("the index"),
        before_index,
        "Worktree.Verify rewrote {} ({} bytes before); a read-only observation \
             does not update the index's cache-tree",
        index.display(),
        before_index.len()
    );

    // The premise, proved rather than asserted: this really is a state in
    // which `write-tree` writes. Run it, and watch the store grow. Without
    // this the two assertions above would pass just as well against the one
    // state — valid cache-tree, every tree present — in which the pre-repair
    // code was already read-only by accident.
    let held = git(&path, &["write-tree"]);
    let after_control = loose_objects(&common);
    assert!(
        after_control.len() > before_objects.len(),
        "the control: `git write-tree` here must create objects, or this test \
             measures nothing ({} then, {} now)",
        before_objects.len(),
        after_control.len()
    );
    assert!(
        after_control.contains(&held),
        "and one of them is the tree the index holds"
    );

    // ---- The holds-it verdict, in the state where write-tree rewrites ----
    //
    // A different discriminator, because a different half of the effect is
    // available: the trees now all exist, so `write-tree` would create no
    // object — but the cache-tree can be invalidated without changing what
    // the index *holds*, and then `write-tree` rewrites `.git/index` to put
    // it back. Measured on git 2.43.0: same tree id, 0 new objects, index
    // bytes changed.
    fs::write(path.join("staged.txt"), "other\n").expect("change the file");
    git(&path, &["add", "staged.txt"]);
    fs::write(path.join("staged.txt"), "staged\n").expect("change it back");
    git(&path, &["add", "staged.txt"]);

    let before_objects = loose_objects(&common);
    let before_index = fs::read(&index).expect("the index");
    let held_verdict = fixture
        .manager
        .verify_worktree(&mut NoHooks, &slot, &Quiescence::HoldsTree(held.clone()))
        .expect("verify");
    assert_eq!(
        held_verdict,
        Ok(()),
        "the worktree does hold {held}, and Verify must still say so"
    );
    assert_eq!(
        loose_objects(&common),
        before_objects,
        "Worktree.Verify created an object on the quiescent path"
    );
    assert_eq!(
        fs::read(&index).expect("the index"),
        before_index,
        "Worktree.Verify rewrote {} on the quiescent path: a read-only \
             observation does not restore the index's cache-tree",
        index.display()
    );

    // The second control: `write-tree` in this state writes no object and
    // rewrites the index anyway, so the assertion that bit is the index one.
    assert_eq!(
        git(&path, &["write-tree"]),
        held,
        "the index still holds the same tree"
    );
    assert_eq!(
        loose_objects(&common),
        before_objects,
        "the control: no object was available to create here"
    );
    assert_ne!(
        fs::read(&index).expect("the index"),
        before_index,
        "the control: `git write-tree` here must rewrite the index, or the \
             assertion above measures nothing"
    );
}

#[test]
fn worktree_verify_answers_every_non_quiescence_by_name() {
    let fixture = Fixture::created("verify");
    let slot = fixture.task("alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let at_head = Quiescence::AtBase(fixture.head.clone());

    assert_eq!(
        fixture.manager.quiescence(&path, &at_head).expect("verify"),
        Err(VerifyFailure::NotRegistered)
    );

    fixture.add_task(&mut NoHooks, "alpha", 1);
    assert_eq!(
        fixture.manager.quiescence(&path, &at_head).expect("verify"),
        Ok(()),
        "a fresh detached worktree at the recorded base is quiescent"
    );

    // HEAD elsewhere.
    assert!(matches!(
        fixture
            .manager
            .quiescence(&path, &Quiescence::AtBase(fixture.seed.clone()))
            .expect("verify"),
        Err(VerifyFailure::HeadMismatch { .. })
    ));

    // The retained cumulative tree.
    let tree = git(&path, &["write-tree"]);
    assert_eq!(
        fixture
            .manager
            .quiescence(&path, &Quiescence::HoldsTree(tree))
            .expect("verify"),
        Ok(())
    );
    assert!(matches!(
        fixture
            .manager
            .quiescence(&path, &Quiescence::HoldsTree("0".repeat(40)))
            .expect("verify"),
        Err(VerifyFailure::TreeMismatch { .. })
    ));

    // Every administrative residue element, one at a time.
    let git_dir = git_dir_of(&path)
        .expect("git dir")
        .expect("linked worktree");
    for (name, element) in [
        ("index.lock", ResidueElement::IndexLock),
        ("CHERRY_PICK_HEAD", ResidueElement::CherryPickHead),
        ("MERGE_HEAD", ResidueElement::MergeHead),
        ("MERGE_MSG", ResidueElement::MergeMsg),
    ] {
        fs::write(git_dir.join(name), "x\n").expect("plant residue");
        assert_eq!(
            fixture.manager.quiescence(&path, &at_head).expect("verify"),
            Err(VerifyFailure::Residue(element)),
            "{name} must make the worktree non-quiescent"
        );
        fs::remove_file(git_dir.join(name)).expect("clear residue");
    }
    fs::create_dir_all(git_dir.join("sequencer")).expect("plant sequencer state");
    assert_eq!(
        fixture.manager.quiescence(&path, &at_head).expect("verify"),
        Err(VerifyFailure::Residue(ResidueElement::SequencerState))
    );
    fs::remove_dir_all(git_dir.join("sequencer")).expect("clear");

    // A missing checkout.
    fs::remove_dir_all(&path).expect("remove the checkout");
    assert_eq!(
        fixture.manager.quiescence(&path, &at_head).expect("verify"),
        Err(VerifyFailure::Missing)
    );
}

#[test]
fn forced_removal_clears_every_administrative_residue_and_is_idempotent() {
    let fixture = Fixture::created("forced-removal");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let git_dir = git_dir_of(&path)
        .expect("git dir")
        .expect("linked worktree");
    // The element list is `ResidueElement::ALL` — PR3's, frozen — not an
    // array written here. The hand-written array this replaced named six
    // of the seven filesystem elements and omitted the `locked` marker of
    // a registered-but-unpopulated worktree, so the test's own name
    // ("every administrative residue") overclaimed: `git worktree prune`
    // *skips* a locked entry, which is why `remove_worktree` clears it, and
    // deleting that clearing left this test green. Measured as a surviving
    // mutation against this test alone.
    let mut planted = 0;
    for element in ResidueElement::ALL {
        match element {
            ResidueElement::IndexLock => {
                fs::write(git_dir.join("index.lock"), "x\n").expect("plant");
            }
            ResidueElement::CherryPickHead => {
                fs::write(git_dir.join("CHERRY_PICK_HEAD"), "x\n").expect("plant");
            }
            ResidueElement::MergeHead => {
                fs::write(git_dir.join("MERGE_HEAD"), "x\n").expect("plant");
            }
            ResidueElement::MergeMsg => {
                fs::write(git_dir.join("MERGE_MSG"), "x\n").expect("plant");
            }
            ResidueElement::OrigHead => {
                fs::write(git_dir.join("ORIG_HEAD"), "x\n").expect("plant");
            }
            ResidueElement::SequencerState => {
                fs::create_dir_all(git_dir.join("sequencer")).expect("plant");
            }
            ResidueElement::RegisteredUnpopulatedWorktree => {
                // Git holds this for the whole of an interrupted `add`, and
                // it is the one element that *blocks* the reclaim path.
                fs::write(git_dir.join("locked"), "initializing\n").expect("plant");
            }
            // Not administrative residue in a git dir: objects are R27 and
            // leave with Git, never with the worktree.
            ResidueElement::UnreferencedObject | ResidueElement::TemporaryObjectFile => {
                continue;
            }
        }
        planted += 1;
    }
    assert_eq!(
        planted,
        ResidueElement::ALL.len() - 2,
        "every element of the frozen enum except the two object classes is planted"
    );
    assert_eq!(planted, 7, "seven administrative elements");

    fixture
        .manager
        .remove_worktree(&mut NoHooks, &slot)
        .expect("forced removal succeeds over administrative residue");
    assert!(!path.exists());
    assert!(!git_dir.exists(), "the residue left with the worktree");
    assert!(
        !fixture
            .manager
            .worktree_records()
            .expect("records")
            .iter()
            .any(|record| record.path.ends_with("kalpha-g1"))
    );

    fixture
        .manager
        .remove_worktree(&mut NoHooks, &slot)
        .expect("and is idempotent");
}

// -----------------------------------------------------------------------
// Byte-safe changed paths
// -----------------------------------------------------------------------

/// One `-z --name-status` record: a status field and its path fields.
fn status_record(status: &[u8], paths: &[&[u8]]) -> Vec<u8> {
    let mut bytes = status.to_vec();
    bytes.push(0);
    for path in paths {
        bytes.extend_from_slice(path);
        bytes.push(0);
    }
    bytes
}

#[test]
fn changed_paths_decode_byte_wise_and_one_undecodable_path_is_repo_wide() {
    // Hostile, and hostile in independent directions: order, case,
    // separators inside a name, a multi-byte name, a name that is longer
    // than any plausible buffer, and a leading-dot name. The status letters
    // vary independently of the paths, so a decoder that ignored the status
    // field and one that mis-read it are different observations.
    let hostile: &[(&[u8], &[u8])] = &[
        (b"M", b"src/Zebra/UBER.rs"),
        (b"A", b"a b/c\td.rs"),
        (b"D", b".hidden"),
        (b"T", "docs/\u{fc}nicode.md".as_bytes()),
        (
            b"M",
            b"a/very/deep/directory/chain/that/keeps/going/well/past/any/plausible/buffer/size/f.rs",
        ),
        (b"A", b"build.rs"),
    ];
    let mut bytes = Vec::new();
    for (status, path) in hostile {
        bytes.extend_from_slice(&status_record(status, &[path]));
    }
    let decoded = decode_changed_paths(&bytes);
    let paths = decoded.prefixes().expect("every path decoded").to_vec();
    assert_eq!(
        paths.len(),
        hostile.len(),
        "one entry per path, and the count is what says so"
    );
    assert_eq!(
        paths.iter().map(GitPath::as_str).collect::<Vec<_>>().len(),
        paths
            .iter()
            .map(GitPath::as_str)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "and they are distinct"
    );
    for (_, path) in hostile {
        let expected = std::str::from_utf8(path).expect("fixture is UTF-8");
        assert!(
            paths.iter().any(|seen| seen.as_str() == expected),
            "`{expected}` survived the round trip"
        );
    }

    // One undecodable path makes the whole answer repo-wide, not a
    // silently shorter list.
    let mut poisoned = bytes.clone();
    poisoned.extend_from_slice(&status_record(b"M", &[b"bad/\xff\xfe.rs"]));
    assert!(
        decode_changed_paths(&poisoned).is_repo_wide(),
        "an undecodable path is never dropped: the region becomes repo-wide"
    );
    assert!(
        decode_changed_paths(b"")
            .prefixes()
            .expect("empty")
            .is_empty()
    );
}

/// **Both** endpoints of a detected rename reach the region.
///
/// `path_policy.actual` is "`--name-status` … both rename endpoints", and
/// the old endpoint is the one another owner may hold a lease on: an answer
/// that carries only the destination lets two overlapping edits be admitted
/// at once (`PR5-CORRECTNESS-005`). Copies carry two endpoints for the same
/// reason and are decoded the same way.
///
/// The expected paths are written here, not derived from the record, and
/// the record is written to the grammar in Git's own documentation rather
/// than produced by this decoder's inverse.
#[test]
fn both_endpoints_of_a_rename_or_copy_record_reach_the_region() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&status_record(
        b"R100",
        &[b"src/auth.rs", b"archive/auth.rs"],
    ));
    bytes.extend_from_slice(&status_record(b"C75", &[b"src/lib.rs", b"src/copy.rs"]));
    bytes.extend_from_slice(&status_record(b"A", &[b"src/added.rs"]));
    bytes.extend_from_slice(&status_record(b"D", &[b"src/gone.rs"]));

    let decoded = decode_changed_paths(&bytes);
    let paths: Vec<&str> = decoded
        .prefixes()
        .expect("decoded")
        .iter()
        .map(GitPath::as_str)
        .collect();
    assert_eq!(
        paths,
        vec![
            "archive/auth.rs",
            "src/added.rs",
            "src/auth.rs",
            "src/copy.rs",
            "src/gone.rs",
            "src/lib.rs",
        ],
        "six endpoints from four records: a rename and a copy carry two each"
    );
}

/// A status field this grammar does not recognise makes the region
/// repo-wide rather than shorter.
///
/// `prediction` classifies "unsafe or unparsable forms" as repo-wide, and
/// repo-wide overlaps everything — so the unparsable direction refuses
/// rather than admits. The most important cell is the first: it is exactly
/// what this decoder sees if the invocation ever reverts to `--name-only`,
/// so that regression cannot produce a plausible short answer.
#[test]
fn an_unparsable_status_record_is_repo_wide_and_never_shorter() {
    let cases: &[(&str, Vec<u8>)] = &[
        ("--name-only output, where a path arrives as a status", {
            let mut bytes = Vec::new();
            for path in [b"archive/auth.rs".as_slice(), b"src/added.rs".as_slice()] {
                bytes.extend_from_slice(path);
                bytes.push(0);
            }
            bytes
        }),
        (
            "a rename record with only one endpoint",
            status_record(b"R100", &[b"src/auth.rs"]),
        ),
        (
            "a status letter that is not one of Git's",
            status_record(b"Z", &[b"src/auth.rs"]),
        ),
        (
            "a single-endpoint letter carrying a score",
            status_record(b"M50", &[b"src/auth.rs"]),
        ),
        (
            "a rename letter carrying no score",
            status_record(b"R", &[b"src/auth.rs", b"archive/auth.rs"]),
        ),
        (
            "a rename score that is not a number",
            status_record(b"Rxx", &[b"src/auth.rs", b"archive/auth.rs"]),
        ),
        (
            "a status field that does not decode",
            status_record(b"\xff", &[b"src/auth.rs"]),
        ),
    ];
    for (name, bytes) in cases {
        assert!(
            decode_changed_paths(bytes).is_repo_wide(),
            "{name}: an unparsable record must be repo-wide, not a shorter list"
        );
    }
    assert_eq!(cases.len(), 7, "seven independent unparsable shapes");
}

/// A Git child that **fails** inside a funnel body records `Before` only
/// (`PR5-WORKSPACE-047`).
///
/// `effect_site_inventory.identity`: "each Object site has exactly the
/// parent-executed hook phases `Before` (no object) and `After` (object
/// present and referenced as `row()` states…)". Every failure path the
/// suite drove refused *before* the funnel was entered — slot-name
/// refusals, `AddWithoutIntent`, symbolic-ref, malformed-oid, containment —
/// so no hooks fired at all, and the harness's own `Injection::Error` is
/// applied at a phase rather than to the primitive. The state the sentence
/// is about was therefore never built: `Before` recorded, the primitive
/// failed, `After` not claimed. A funnel that claimed `After` from an
/// unconditional cleanup guard would say an object is present and
/// referenced when the child that would have written it exited non-zero.
///
/// Both funnel shapes are driven: the shared `funnel()` helper, and the
/// hand-rolled `commit_tree` sequence that also carries `IdUnread`.
#[test]
fn a_git_child_that_fails_inside_a_funnel_records_before_and_never_claims_after() {
    let fixture = Fixture::created("funnel-failure");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    // Well-formed and absent: it passes every argument check and then makes
    // the child itself fail, which is the only way into the funnel body.
    let absent = "0".repeat(39) + "1";

    /// One failing drive: it runs a Git child that exits non-zero inside a
    /// funnel body and answers whether the call really failed.
    type FailingDrive = Box<dyn Fn(&mut dyn EffectHooks) -> bool>;
    let cases: Vec<(&str, EffectSiteId, FailingDrive)> = vec![
        (
            "the shared funnel (Object.ProposalCherryPick)",
            EffectSiteId::Object(ObjectSite::ProposalCherryPick),
            Box::new(|hooks: &mut dyn EffectHooks| {
                let fixture = Fixture::created("funnel-failure-cherry");
                let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
                fixture
                    .manager
                    .proposal_cherry_pick(hooks, &slot, &("0".repeat(39) + "1"))
                    .is_err()
            }),
        ),
        (
            "the commit-tree sequence (Object.CandidateCommitTree)",
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            Box::new(|hooks: &mut dyn EffectHooks| {
                let fixture = Fixture::created("funnel-failure-commit");
                fixture
                    .manager
                    .candidate_commit_tree(
                        hooks,
                        &("0".repeat(39) + "1"),
                        &fixture.head,
                        "upstroke: candidate",
                    )
                    .is_err()
            }),
        ),
    ];

    for (what, site, drive) in cases {
        let (mut hooks, shared) = harness();
        assert!(
            drive(&mut hooks),
            "{what}: the child was supposed to fail and did not"
        );
        let harness = shared.lock().expect("harness");
        assert_eq!(
            harness.count(site, HookPhase::Before),
            1,
            "{what}: the funnel was entered, so Before fired once"
        );
        assert_eq!(
            harness.count(site, HookPhase::After),
            0,
            "{what}: the primitive failed, so there is no object present and referenced                  for After to be claiming"
        );
    }

    let _ = &slot;
    let _ = &absent;
}

/// Two generations of one task key are two different worktrees
/// (`PR5-WORKSPACE-010`).
///
/// `manager`: "detached linked worktrees with durable synced intents
/// (`tasks/k<key>-g<gen>`, `merge/s<seq>`)". Every Task slot in this file
/// is built at a single generation, so the two paths that would collide
/// were never both constructed and dropping `-g<generation>` from
/// `relative()` was invisible. `intent_name` still carried the generation,
/// and it is `intent_name` the round-trip tests exercise — so the
/// injectivity they prove is the file name's, not the worktree path's.
#[test]
fn two_generations_of_one_task_key_are_two_worktrees() {
    let fixture = Fixture::created("generations");
    let first = fixture.task("alpha", 0);
    let second = fixture.task("alpha", 1);

    assert_ne!(
        fixture.manager.slot_path(&first),
        fixture.manager.slot_path(&second),
        "one key at two generations must not name one directory"
    );
    assert!(
        first
            .relative()
            .ends_with("tasks/k alpha-g0".replace(' ', "").as_str()),
        "the packet spells it tasks/k<key>-g<gen>: {}",
        first.relative().display()
    );
    assert!(
        second.relative().ends_with("tasks/kalpha-g1"),
        "{}",
        second.relative().display()
    );

    // And both really exist at once, which is the state a collision
    // destroys: the second add would land in the first's checkout.
    fixture.add_task(&mut NoHooks, "alpha", 0);
    fixture.add_task(&mut NoHooks, "alpha", 1);
    for slot in [&first, &second] {
        assert_eq!(
            fixture
                .manager
                .quiescence(
                    &fixture.manager.slot_path(slot),
                    &Quiescence::AtBase(fixture.head.clone())
                )
                .expect("verify"),
            Ok(()),
            "{slot:?} is its own quiescent worktree"
        );
    }
    assert_eq!(
        fixture
            .manager
            .worktree_records()
            .expect("records")
            .iter()
            .filter(|record| record.path.starts_with(fixture.manager.execution_root()))
            .count(),
        2,
        "two registrations, not one directory registered twice"
    );
}

/// A task worktree is **detached** even when the commit-ish is a branch
/// name (`PR5-WORKSPACE-012`).
///
/// `git worktree add <path> <sha>` detaches HEAD with or without
/// `--detach`, and every fixture in this file passes a raw 40-hex id — so
/// the flag was behaviour-neutral on everything the suite built, and
/// nothing ever read HEAD's attachment state after an add or enumerated the
/// refs an add created. A branch name is the one commit-ish where the flag
/// decides, and without it `git worktree add` checks the branch out **and
/// locks it to that worktree**, which is the state `integration_ref`'s
/// "never checked out" forbids for the run namespace.
#[test]
fn a_task_worktree_is_detached_even_when_the_base_names_a_branch() {
    let fixture = Fixture::created("detached");
    let slot = fixture.task("alpha", 1);
    let before: BTreeSet<String> = fixture
        .manager
        .refs_under("refs/heads")
        .expect("refs")
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    fixture
        .manager
        .write_intent(&mut NoHooks, &slot)
        .expect("intent");
    fixture
        .manager
        .add_worktree(&mut NoHooks, &slot, "side")
        .expect("add at a branch NAME, not a sha");

    let path = fixture.manager.slot_path(&slot);
    assert_eq!(
        git(&path, &["rev-parse", "HEAD"]),
        fixture.side,
        "the worktree is at the branch's commit"
    );
    assert_eq!(
        git(&path, &["rev-parse", "--symbolic-full-name", "HEAD"]),
        "HEAD",
        "and its HEAD is detached rather than pointing at refs/heads/side"
    );
    let after: BTreeSet<String> = fixture
        .manager
        .refs_under("refs/heads")
        .expect("refs")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(after, before, "the add created and moved no branch ref");
}

/// A clean worktree of a **different repository** at the recorded path
/// fails verification (`PR5-WORKSPACE-019`).
///
/// `generation`: "`Worktree.Verify`: the recorded path is a linked worktree
/// of **this** repository". `worktree_verify_answers_every_non_quiescence_
/// by_name` drives every other failure by name and never this one — no
/// fixture built a second repository — so the identity half of the sentence
/// was unobserved and deleting the common-git-dir comparison changed
/// nothing. The foreign worktree holds the **same commit object**, so a
/// verifier that only compared HEAD would still pass it.
#[test]
fn a_worktree_of_another_repository_at_the_recorded_path_is_not_this_ones() {
    let fixture = Fixture::created("foreign-repo");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);

    // A second repository holding the very same commit object, so identity
    // is the only thing that separates its checkout from the real one.
    let foreign = fixture.root.join("foreign");
    fs::create_dir_all(&foreign).expect("foreign repo");
    git(&foreign, &["init", "-q", "-b", "main"]);
    git(&foreign, &["config", "user.email", "tests@upstroke.local"]);
    git(&foreign, &["config", "user.name", "upstroke tests"]);
    git(
        &foreign,
        &["fetch", "-q", &fixture.base.to_string_lossy(), "main"],
    );
    let fetched = git(&foreign, &["rev-parse", "FETCH_HEAD"]);
    assert_eq!(
        fetched, fixture.head,
        "the foreign repository holds the identical commit object"
    );

    // The recorded path stays registered in **this** repository — a
    // verifier that stopped at "is it registered here" must still be
    // reached — while the checkout sitting there belongs to the other one.
    let theirs = fixture.root.join("theirs");
    git(
        &foreign,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &theirs.to_string_lossy(),
            &fetched,
        ],
    );
    let foreign_gitfile = fs::read(theirs.join(".git")).expect("their .git file");
    fs::write(path.join(".git"), &foreign_gitfile).expect("point the checkout at their repo");

    assert!(
        fixture
            .manager
            .quiescence(&path, &Quiescence::AtBase(fixture.head.clone()))
            .expect("verify")
            != Err(VerifyFailure::NotRegistered),
        "the path is still registered here, so this is not the registration check"
    );
    assert_eq!(
        git(&path, &["rev-parse", "HEAD"]),
        fixture.head,
        "and a HEAD-only verifier would see exactly what it expects"
    );
    assert_eq!(
        fixture
            .manager
            .quiescence(&path, &Quiescence::AtBase(fixture.head.clone()))
            .expect("verify"),
        Err(VerifyFailure::ForeignRepository),
        "but it is another repository's worktree at this repository's recorded path"
    );
}

/// The recorded base is honoured **after the worktree's HEAD has moved off
/// it** (`PR5-WORKSPACE-038`).
///
/// `path_policy.actual` specifies `git diff-tree -r -z -M --name-status
/// base tree` — a diff between two *recorded* values. Every other fixture
/// in this file leaves the worktree checked out at exactly the base it then
/// passes, so `diff --cached <base>` and a bare `diff --cached` name the
/// same diff and a primitive that had quietly stopped honouring its
/// argument was indistinguishable from one that honoured it. Nothing here
/// asserts a spelling; it moves the one variable the two readings disagree
/// about and checks the answer.
#[test]
fn changed_paths_honour_the_recorded_base_after_head_has_moved_off_it() {
    let fixture = Fixture::created("changed-paths-moved-head");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    fs::write(path.join("staged.rs"), "fn main() {}\n").expect("add");
    fixture
        .manager
        .candidate_stage(&mut NoHooks, &slot)
        .expect("stage");

    // Move the worktree's HEAD to the seed, keeping the index. `head` is
    // still the recorded base; HEAD is now `seed`, one commit behind it.
    git(&path, &["reset", "-q", "--soft", &fixture.seed]);
    assert_eq!(
        git(&path, &["rev-parse", "HEAD"]),
        fixture.seed,
        "the worktree's HEAD really moved off the recorded base"
    );
    assert_ne!(fixture.seed, fixture.head, "the two must differ at all");

    let against_base: Vec<String> = fixture
        .manager
        .changed_paths(&slot, &fixture.head)
        .expect("capture")
        .prefixes()
        .expect("decoded")
        .iter()
        .map(|path| path.as_str().to_owned())
        .collect();
    assert_eq!(
        against_base,
        vec!["staged.rs".to_owned()],
        "the diff is against the recorded base, not against wherever HEAD is now"
    );

    // And the two readings really are different here, so the assertion
    // above is not passing for want of a distinction: `b.txt` arrived
    // between the seed and the base, so a HEAD-relative diff carries it.
    let head_relative = git(&path, &["diff", "--cached", "--name-only"]);
    let head_relative: Vec<&str> = head_relative.lines().collect();
    assert!(
        head_relative.contains(&"b.txt"),
        "the fixture does not separate the two readings: {head_relative:?}"
    );
}

/// Each commit-tree site commits onto the **recorded** parent after HEAD
/// has moved (`PR5-WORKSPACE-023`, `PR5-WORKSPACE-042`).
///
/// `snapshots` says "the snapshot funnel first creates an ephemeral commit
/// of that tree on **the recorded parent**", and `candidate` says
/// `parent_sha == base_sha`. Both were asserted against a base the
/// repository's HEAD already equalled, so `commit-tree <tree> -p <recorded>`
/// and a body that had re-read the world produced the same commit. The
/// manipulation is one line — move HEAD — and it is the only one that
/// separates a primitive that honours its argument from one that does not.
#[test]
fn the_commit_tree_sites_use_the_recorded_parent_and_not_current_head() {
    let fixture = Fixture::created("recorded-parent");
    let tree = git(&fixture.base, &["rev-parse", "HEAD^{tree}"]);
    let recorded = fixture.head.clone();

    git(
        &fixture.base,
        &["checkout", "-q", "--detach", &fixture.side],
    );
    assert_eq!(
        git(&fixture.base, &["rev-parse", "HEAD"]),
        fixture.side,
        "HEAD moved off the recorded parent"
    );
    assert_ne!(fixture.side, recorded);

    for (what, commit) in [
        (
            "snapshot",
            fixture
                .manager
                .snapshot_commit_tree(&mut NoHooks, &tree, &recorded)
                .expect("the ephemeral snapshot commit"),
        ),
        (
            "candidate",
            fixture
                .manager
                .candidate_commit_tree(&mut NoHooks, &tree, &recorded, "upstroke: candidate")
                .expect("the candidate commit"),
        ),
    ] {
        let parents = git(
            &fixture.base,
            &["rev-list", "--parents", "-n", "1", &commit],
        );
        let parents: Vec<&str> = parents.split_whitespace().skip(1).collect();
        assert_eq!(
            parents,
            vec![recorded.as_str()],
            "{what}: the sole parent is the recorded one, not current HEAD ({})",
            fixture.side
        );
        assert_eq!(
            git(&fixture.base, &["rev-parse", &format!("{commit}^{{tree}}")]),
            tree,
            "{what}: the tree is the supplied one"
        );
    }
}

/// An undecodable byte in a rename **source** makes the region repo-wide
/// (`PR5-WORKSPACE-036`).
///
/// `path_policy.actual`: "both rename endpoints; NUL-delimited bytes;
/// GitPath byte-safe; **undecodable -> repo-wide**". The lane had solid
/// coverage on each axis separately and never their intersection: every
/// rename fixture's four endpoints are valid UTF-8, and every undecodable
/// fixture plants its bad byte in a single-endpoint record. So the one
/// field a source-dropping decoder treats differently was never hostile,
/// and "both endpoints or repo-wide" could not be told from "the
/// destination, plus the source when it happens to decode" — which loses a
/// path another owner may hold a lease on, silently.
#[test]
fn an_undecodable_rename_source_makes_the_region_repo_wide() {
    // A rename whose DESTINATION is perfectly ordinary, so a decoder that
    // returns what it could read returns something plausible.
    let source_bad = status_record(b"R100", &[b"src/\xff\xfe.rs", b"archive/auth.rs"]);
    assert!(
        decode_changed_paths(&source_bad).is_repo_wide(),
        "an undecodable rename source is not a path that may be quietly dropped"
    );

    // The other endpoint, and a copy record, so this is the field rather
    // than the record kind.
    let destination_bad = status_record(b"R100", &[b"src/auth.rs", b"archive/\xff.rs"]);
    assert!(decode_changed_paths(&destination_bad).is_repo_wide());
    let copy_source_bad = status_record(b"C75", &[b"src/\xff.rs", b"copy/auth.rs"]);
    assert!(decode_changed_paths(&copy_source_bad).is_repo_wide());

    // And the same record with both endpoints decodable is NOT repo-wide,
    // so the assertions above are about the undecodable byte rather than
    // about rename records in general.
    let both_fine = status_record(b"R100", &[b"src/auth.rs", b"archive/auth.rs"]);
    let decoded = decode_changed_paths(&both_fine);
    assert!(!decoded.is_repo_wide());
    let paths: Vec<&str> = decoded
        .prefixes()
        .expect("decoded")
        .iter()
        .map(GitPath::as_str)
        .collect();
    assert_eq!(paths, vec!["archive/auth.rs", "src/auth.rs"]);
}

#[test]
fn changed_paths_come_from_the_index_of_the_recorded_worktree() {
    let fixture = Fixture::created("changed-paths");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    fs::write(path.join("a.txt"), "changed\n").expect("edit");
    fs::create_dir_all(path.join("nested")).expect("nested");
    fs::write(path.join("nested/new.rs"), "fn main() {}\n").expect("add");
    fixture
        .manager
        .candidate_stage(&mut NoHooks, &slot)
        .expect("stage");

    let captured = fixture
        .manager
        .changed_paths(&slot, &fixture.head)
        .expect("capture");
    let paths: Vec<&str> = captured
        .prefixes()
        .expect("decoded")
        .iter()
        .map(GitPath::as_str)
        .collect();
    assert_eq!(paths, vec!["a.txt", "nested/new.rs"]);
}

/// The same claim against **real Git**, over the change kinds the previous
/// test does not contain.
///
/// `PR5-CORRECTNESS-005`: the shipped invocation was `--name-only`, and
/// rename detection is Git's default — so a staged rename produced the
/// destination alone and the source, which another owner may hold a lease
/// on, silently left the region. That coverage held "one modification and
/// one addition", the two kinds where every invocation agrees.
///
/// Four kinds here, and the expected list is written out rather than
/// derived: a rename (two endpoints), a deletion, an addition, and a
/// modification. The rename is made by moving the file on disk and staging
/// through the production funnel, so detection is Git's decision at diff
/// time and not something the fixture asserted into being — which is also
/// why the record is checked to really be an `R`.
#[test]
fn every_change_kind_reaches_the_region_including_both_rename_endpoints() {
    let fixture = Fixture::created("changed-paths-kinds");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);

    // A base inside the worktree holding one file of each kind's
    // pre-state, so all four kinds can be produced against one commit.
    fs::write(path.join("kept.txt"), "before\n").expect("kept");
    fs::write(path.join("doomed.txt"), "doomed\n").expect("doomed");
    fs::write(path.join("moved.txt"), "moved\n").expect("moved");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-q", "-m", "the base for this diff"]);
    let base = git(&path, &["rev-parse", "HEAD"]);

    // moved.txt -> archive/moved.txt, byte-identical: 100% similarity.
    fs::create_dir_all(path.join("archive")).expect("archive dir");
    fs::rename(path.join("moved.txt"), path.join("archive/moved.txt")).expect("move");
    fs::remove_file(path.join("doomed.txt")).expect("delete");
    fs::write(path.join("added.rs"), "fn main() {}\n").expect("add");
    fs::write(path.join("kept.txt"), "after\n").expect("modify");

    fixture
        .manager
        .candidate_stage(&mut NoHooks, &slot)
        .expect("stage");

    // Git really did detect a rename here, rather than reporting a delete
    // and an add — otherwise this fixture would pass under `--name-only`
    // too and would be witnessing nothing.
    let records = git(&path, &["diff", "--cached", "--name-status", "-M", &base]);
    assert!(
        records.contains("R100\tmoved.txt\tarchive/moved.txt"),
        "the fixture must contain a *detected* rename, or it tests nothing: {records}"
    );

    let captured = fixture
        .manager
        .changed_paths(&slot, &base)
        .expect("capture");
    let paths: Vec<&str> = captured
        .prefixes()
        .expect("decoded")
        .iter()
        .map(GitPath::as_str)
        .collect();
    assert_eq!(
        paths,
        vec![
            "added.rs",
            "archive/moved.txt",
            "doomed.txt",
            "kept.txt",
            "moved.txt",
        ],
        "both rename endpoints, the deletion, the addition and the modification"
    );
}

#[cfg(unix)]
#[test]
fn a_repository_path_a_string_cannot_carry_makes_the_region_repo_wide() {
    use std::os::unix::ffi::OsStrExt as _;
    let fixture = Fixture::created("nonutf8-paths");
    let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
    let path = fixture.manager.slot_path(&slot);
    let hostile = path.join(OsStr::from_bytes(b"bad-\xff\xfe.txt"));
    if fs::write(&hostile, "bytes\n").is_err() {
        // A filesystem that refuses the name cannot host the fixture; the
        // pure-byte case above still covers the decision.
        return;
    }
    fixture
        .manager
        .candidate_stage(&mut NoHooks, &slot)
        .expect("stage");
    assert!(
        fixture
            .manager
            .changed_paths(&slot, &fixture.head)
            .expect("capture")
            .is_repo_wide(),
        "a path no string can carry makes the region repo-wide rather than shorter"
    );
}

// -----------------------------------------------------------------------
// The Object group: rows, IdUnread, and the residue classifier
// -----------------------------------------------------------------------

/// `slice_contract.proof_tests[7]`: "after each creation primitive the
/// object is referenced by exactly the row `row()` names (index/HEAD
/// inspection; fsck for R27)".
///
/// The expected row per site comes from the frozen `ObjectSite::row()`, and
/// the *observation* comes from Git — index, HEAD, or `fsck`. The two are
/// independent: nothing here asks the site what it expects and then asks it
/// again what it found.
#[test]
fn after_each_object_primitive_the_object_is_referenced_by_the_row_row_names() {
    let fixture = Fixture::created("object-rows");
    let mut checked = Vec::new();

    // R9: Object.CandidateStage — blobs behind the task worktree index.
    let task = fixture.add_task(&mut NoHooks, "alpha", 1);
    let task_path = fixture.manager.slot_path(&task);
    fs::write(task_path.join("staged.txt"), "staged\n").expect("edit");
    let blob = git(&task_path, &["hash-object", "staged.txt"]);
    fixture
        .manager
        .candidate_stage(&mut NoHooks, &task)
        .expect("stage");
    assert_eq!(ObjectSite::CandidateStage.row(), ResourceRow::R9);
    assert!(
        git(&task_path, &["ls-files", "-s"]).contains(&blob),
        "the staged blob is referenced by the task worktree index"
    );
    assert!(
        !unreachable_objects(&task_path)
            .expect("fsck")
            .contains(&blob),
        "so it is not R27"
    );
    checked.push(ObjectSite::CandidateStage);

    // R9: Object.CandidateWriteTree — trees behind that index's cache-tree.
    let tree = fixture
        .manager
        .candidate_write_tree(&mut NoHooks, &task)
        .expect("write-tree");
    assert_eq!(ObjectSite::CandidateWriteTree.row(), ResourceRow::R9);
    assert!(
        !unreachable_objects(&task_path)
            .expect("fsck")
            .contains(&tree),
        "the tree is reachable through the index's cache-tree extension: R9, not R27"
    );
    checked.push(ObjectSite::CandidateWriteTree);

    // R27: Object.SnapshotCommitTree — unreferenced until Snapshot.Add.
    let ephemeral = fixture
        .manager
        .snapshot_commit_tree(&mut NoHooks, &tree, &fixture.head)
        .expect("ephemeral commit");
    assert_eq!(ObjectSite::SnapshotCommitTree.row(), ResourceRow::R27);
    assert!(
        unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&ephemeral),
        "the ephemeral commit is unreferenced: R27"
    );
    checked.push(ObjectSite::SnapshotCommitTree);

    // R27: Object.CandidateCommitTree — unreferenced until the pin.
    let candidate = fixture
        .manager
        .candidate_commit_tree(&mut NoHooks, &tree, &fixture.head, "candidate")
        .expect("candidate commit");
    assert_eq!(ObjectSite::CandidateCommitTree.row(), ResourceRow::R27);
    assert!(
        unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&candidate),
        "the candidate commit is unreferenced: R27"
    );
    // …and R23 once pinned, which is the row that then accounts for it.
    fixture
        .manager
        .create_ref_zero_old(
            &mut NoHooks,
            RefSite::PinCandidatePrepared,
            "refs/upstroke/runs/run-1/candidate-prepared/kalpha/1",
            &candidate,
        )
        .expect("pin");
    assert_eq!(RefSite::PinCandidatePrepared.row(), ResourceRow::R23);
    assert!(
        !unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&candidate),
        "the pin moves it out of R27 and into the row that references it"
    );
    checked.push(ObjectSite::CandidateCommitTree);

    // R10: Object.ProposalCherryPick — through the staging HEAD.
    let staging = Slot::Staging { sequence: 1 };
    fixture
        .manager
        .write_intent(&mut NoHooks, &staging)
        .expect("staging intent");
    let staging_path = fixture
        .manager
        .add_worktree(&mut NoHooks, &staging, &fixture.head)
        .expect("staging worktree");
    let proposal = fixture
        .manager
        .proposal_cherry_pick(&mut NoHooks, &staging, &fixture.side)
        .expect("cherry-pick");
    assert_eq!(ObjectSite::ProposalCherryPick.row(), ResourceRow::R10);
    assert_eq!(
        git(&staging_path, &["rev-parse", "HEAD"]),
        proposal,
        "the proposal commit is the staging worktree's HEAD"
    );
    assert!(
        !unreachable_objects(&staging_path)
            .expect("fsck")
            .contains(&proposal),
        "so it is not R27 while the staging worktree exists"
    );
    checked.push(ObjectSite::ProposalCherryPick);

    // R9: Object.RepairMaterialize — merge objects behind the repair index.
    let repair = fixture.add_task(&mut NoHooks, "repair", 1);
    let repair_path = fixture.manager.slot_path(&repair);
    fixture
        .manager
        .repair_materialize(&mut NoHooks, &repair, &fixture.side)
        .expect("materialize");
    assert_eq!(ObjectSite::RepairMaterialize.row(), ResourceRow::R9);
    assert!(
        index_differs_from_head(&repair_path).expect("index"),
        "the materialization is staged in the repair worktree's index"
    );
    let materialized = git(&repair_path, &["rev-parse", ":c.txt"]);
    assert!(
        !unreachable_objects(&repair_path)
            .expect("fsck")
            .contains(&materialized),
        "index-referenced, so R9 rather than R27"
    );
    checked.push(ObjectSite::RepairMaterialize);

    // And the domain is the enum's, not the author's memory.
    checked.sort();
    checked.dedup();
    assert_eq!(
        checked.len(),
        ObjectSite::ALL.len(),
        "every Object site the frozen enum declares has a row observation; missing: {:?}",
        ObjectSite::ALL
            .iter()
            .filter(|site| !checked.contains(site))
            .collect::<Vec<_>>()
    );

    // The scrub releases what the worktree held — and `cleanup` states the
    // disjunction the release obeys: "objects released to R27 **or
    // accounted by the candidate pin/ref**". Both halves are asserted,
    // because a test that checked only the first would be measuring
    // whichever half the fixture happened to build.
    fixture
        .manager
        .remove_worktree(&mut NoHooks, &task)
        .expect("scrub");
    assert!(
        !unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&blob),
        "the staged blob is in the candidate commit's tree, so the candidate-prepared pin \
             (R23) still accounts for it after the scrub"
    );
    fixture
        .manager
        .delete_ref_expected_old(
            &mut NoHooks,
            RefSite::DeleteCandidatePin,
            "refs/upstroke/runs/run-1/candidate-prepared/kalpha/1",
            &candidate,
        )
        .expect("prune the pin expected-old");
    assert!(
        unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&blob),
        "and once no pin or ref references it, it is R27"
    );
    fixture
        .manager
        .remove_worktree(&mut NoHooks, &staging)
        .expect("reclaim staging");
    assert!(
        unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&proposal),
        "and removing the staging worktree releases the proposal objects"
    );
}

/// `slice_contract.proof_tests[7]`: "IdUnread hook tests for the
/// commit-tree primitives".
#[test]
fn the_commit_tree_primitives_consult_their_id_unread_point() {
    let fixture = Fixture::created("id-unread");
    let (mut hooks, shared) = harness();
    let tree = git(
        &fixture.base,
        &["rev-parse", &format!("{}^{{tree}}", fixture.head)],
    );
    fixture
        .manager
        .snapshot_commit_tree(&mut hooks, &tree, &fixture.head)
        .expect("ephemeral");
    fixture
        .manager
        .candidate_commit_tree(&mut hooks, &tree, &fixture.head, "candidate")
        .expect("candidate");

    let harness = shared.lock().expect("harness");
    let mut with_point = Vec::new();
    for site in lane_sites() {
        let declared = site.sub_effects().contains(&SubEffectPoint::IdUnread);
        let reached = harness.reached_point(site, SubEffectPoint::IdUnread, InjectionMode::Kill);
        assert_eq!(
            declared, reached,
            "`{site}` declares IdUnread = {declared} but the funnels reached it = {reached}"
        );
        if declared {
            with_point.push(site);
        }
    }
    assert_eq!(
        with_point.len(),
        2,
        "exactly the two commit-tree sites expose IdUnread: {with_point:?}"
    );
    assert!(
        !harness.observed(
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            HookPhase::Point {
                point: SubEffectPoint::IdUnread,
                mode: InjectionMode::Kill,
            }
        ),
        "reaching a point is not executing its injection: nothing was armed"
    );
}

/// The durable state a kill at `IdUnread` leaves, without aborting this
/// process: the object is written and no id was recorded.
///
/// `transaction_fault_matrix[T-CAND-OBJ].resume_action` for that prefix is
/// "(a) nothing to delete: the unpinned object is left to Git (never
/// adopted)". The abort itself is exercised by
/// `a_kill_at_id_unread_aborts_before_the_id_is_recorded`, which runs in a
/// child process because `Injection::Kill` aborts by design.
#[test]
fn a_kill_at_id_unread_leaves_a_gc_owned_object_nothing_adopts() {
    let fixture = Fixture::created("id-unread-residue");
    let tree = git(
        &fixture.base,
        &["rev-parse", &format!("{}^{{tree}}", fixture.head)],
    );
    let commit = fixture
        .manager
        .candidate_commit_tree(&mut NoHooks, &tree, &fixture.head, "candidate")
        .expect("candidate commit");

    // The parent never recorded the id: exactly `IdUnread`.
    let target = ResidueTarget::new(&fixture.base);
    assert_eq!(
        classify_object_residue(
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            &target
        )
        .expect("classify"),
        ObjectResidue::Internal
    );
    // And with the id recorded, the very same durable state is the after
    // phase. The classifier's answer is a function of the record, which is
    // what `IdUnread` is defined by the absence of.
    assert_eq!(
        classify_object_residue(
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            &ResidueTarget::new(&fixture.base).published(&commit)
        )
        .expect("classify"),
        ObjectResidue::After
    );

    let before = unreachable_objects(&fixture.base).expect("fsck");
    assert!(before.contains(&commit));
    fixture
        .manager
        .reclaim_intents(&mut NoHooks)
        .expect("the tabled recovery");
    let after = unreachable_objects(&fixture.base).expect("fsck");
    assert!(
        after.contains(&commit),
        "fsck still lists the object unreachable and untouched: the run never deletes it"
    );
}

/// The abort half, in a child process. `Injection::Kill` calls
/// `std::process::abort` on purpose — a coordinator that died running
/// destructors would not be the thing under test — so the only way to
/// observe it is from outside.
#[test]
fn a_kill_at_id_unread_aborts_before_the_id_is_recorded() {
    let record = scratch("id-unread-kill").join("record");
    let helper = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "workspace_manager::tests::id_unread_kill_helper",
            "--ignored",
            "--nocapture",
        ])
        .env(ID_UNREAD_RECORD, &record)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .expect("run the helper");
    assert!(
        !helper.status.success(),
        "the helper must die at the point rather than finish"
    );
    let written = fs::read_to_string(&record).expect("the helper recorded its repository");
    let mut lines = written.lines();
    let repository = PathBuf::from(lines.next().expect("repository path"));
    let tree = lines.next().expect("tree").to_owned();

    // The child died at `IdUnread`: the object is in the store and no id
    // was ever recorded anywhere.
    let unreachable = unreachable_objects(&repository).expect("fsck");
    assert!(
        !unreachable.is_empty(),
        "the object the child wrote survives its death"
    );
    assert_eq!(
        classify_object_residue(
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            &ResidueTarget::new(&repository)
        )
        .expect("classify"),
        ObjectResidue::Internal,
        "and the durable state classifies as the internal residue class"
    );
    assert!(!tree.is_empty());
    let _ = fs::remove_dir_all(repository.parent().unwrap_or(&repository));
}

/// Where the helper tells its parent which repository to inspect.
const ID_UNREAD_RECORD: &str = "UPSTROKE_PR5A_ID_UNREAD_RECORD";

/// Spawned by `a_kill_at_id_unread_aborts_before_the_id_is_recorded`.
#[test]
#[ignore = "subprocess helper"]
fn id_unread_kill_helper() {
    let Some(record) = std::env::var_os(ID_UNREAD_RECORD) else {
        return;
    };
    let fixture = Fixture::created("id-unread-helper");
    let tree = git(
        &fixture.base,
        &["rev-parse", &format!("{}^{{tree}}", fixture.head)],
    );
    fs::write(&record, format!("{}\n{tree}\n", fixture.base.display()))
        .expect("record the repository before dying");
    let manager = fixture.manager.clone();
    let head = fixture.head.clone();
    // Keep the repository: the parent inspects it after this process dies,
    // and `Fixture`'s destructor would remove it — which is also exactly
    // what an aborting process does not run.
    std::mem::forget(fixture);

    struct KillAtIdUnread;
    impl EffectHooks for KillAtIdUnread {
        fn phase(&mut self, _site: EffectSiteId, phase: HookPhase) -> Injection {
            match phase {
                HookPhase::Point {
                    point: SubEffectPoint::IdUnread,
                    ..
                } => Injection::Kill,
                _ => Injection::Proceed,
            }
        }

        fn refusal_cause(&self) -> Option<String> {
            None
        }
    }
    let _ = manager.candidate_commit_tree(&mut KillAtIdUnread, &tree, &head, "candidate");
    unreachable!("the funnel aborts at IdUnread");
}

// -----------------------------------------------------------------------
// The residue classifier: totality, elements, and kill sampling
// -----------------------------------------------------------------------

/// Write an object nothing references.
fn write_orphan(repository: &Path, content: &str) -> String {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn git hash-object");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(content.as_bytes())
        .expect("feed the object");
    let output = child.wait_with_output().expect("hash-object");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn classify(site: EffectSiteId, target: &ResidueTarget<'_>) -> ObjectResidue {
    classify_object_residue(site, target).expect("the classifier answers")
}

#[test]
fn classify_object_residue_refuses_a_site_that_registers_no_class() {
    let fixture = Fixture::created("classifier-domain");
    let target = ResidueTarget::new(&fixture.base);
    for site in lane_sites() {
        let answer = classify_object_residue(site, &target);
        assert_eq!(
            answer.is_ok(),
            !site.residue_classes().is_empty(),
            "`{site}` registers {} residue classes, so the classifier must {} it",
            site.residue_classes().len(),
            if site.residue_classes().is_empty() {
                "refuse"
            } else {
                "answer for"
            }
        );
    }
    let message = refusal_of(
        &classify_object_residue(EffectSiteId::Worktree(WorktreeSite::Verify), &target)
            .expect_err("a site with no class refuses"),
    );
    assert!(
        message.contains("registers no residue class"),
        "the refusal must name its reason: {message}"
    );
}

/// `command_internal_sub_effects`: "the classifier is **total** over
/// `{None, Internal, After}` for every Object site and for `Worktree.Add` /
/// `Snapshot.Add`".
///
/// Totality is proved by *producing all three at every site*, not by an
/// exhaustive `match` returning a default. The site list is
/// [`residue_classified_sites`], derived from the frozen enums — a grid over
/// the sites its author remembered is the `bounded_grid` failure this
/// project has recorded three times.
#[test]
fn the_classifier_is_total_over_three_classes_for_every_registered_site() {
    let sites = residue_classified_sites();
    assert_eq!(
        sites.len(),
        ObjectSite::ALL.len() + 3,
        "six Object sites plus Worktree.Add, Worktree.AddStaging and Snapshot.Add: {sites:?}"
    );
    for site in &sites {
        let observed = observed_three_classes(*site);
        assert_eq!(
            observed,
            [
                ObjectResidue::None,
                ObjectResidue::Internal,
                ObjectResidue::After
            ],
            "`{site}` must answer each of the three classes for the state that is that class"
        );
    }
    // And every value of the codomain was produced, which is the property
    // a per-site assertion alone would not state.
    assert_eq!(ObjectResidue::ALL.len(), 3);
}

/// Drive one site through a state of each class, in the order
/// `[None, Internal, After]`.
///
/// A site with no arm here panics rather than being skipped: that is what
/// makes the domain the enum's rather than this function's.
fn observed_three_classes(site: EffectSiteId) -> [ObjectResidue; 3] {
    let tag = format!("total-{}", site.variant().to_lowercase());
    let fixture = Fixture::created(&tag);
    let base = fixture.base.clone();
    assert!(
        unreachable_objects(&base).expect("fsck").is_empty(),
        "the fixture must start with an empty R27, or `None` would be unobservable"
    );

    match site {
        EffectSiteId::Worktree(WorktreeSite::Add | WorktreeSite::AddStaging)
        | EffectSiteId::Snapshot(SnapshotSite::Add) => {
            let slot = match site {
                EffectSiteId::Worktree(WorktreeSite::Add) => fixture.task("alpha", 1),
                EffectSiteId::Worktree(WorktreeSite::AddStaging) => Slot::Staging { sequence: 1 },
                _ => Slot::Snapshot {
                    name: SnapshotName::gates(1, 1),
                },
            };
            let path = fixture.manager.slot_path(&slot);
            let none = classify(site, &ResidueTarget::new(&base).at(&path));
            register_unpopulated(&fixture, &path);
            let internal = classify(site, &ResidueTarget::new(&base).at(&path));
            fixture
                .manager
                .remove_worktree(&mut NoHooks, &slot)
                .expect("clear the residue");
            // The intent is synced before the add — the add funnel refuses
            // otherwise, which is what makes an interrupted add reclaimable.
            fixture
                .manager
                .write_intent(&mut NoHooks, &slot)
                .expect("intent");
            fixture
                .manager
                .add_worktree(&mut NoHooks, &slot, &fixture.head)
                .expect("a completed add");
            let after = classify(site, &ResidueTarget::new(&base).at(&path));
            [none, internal, after]
        }
        EffectSiteId::Object(ObjectSite::CandidateStage) => {
            let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
            let path = fixture.manager.slot_path(&slot);
            fs::write(path.join("a.txt"), "edited\n").expect("unstaged change");
            let none = classify(site, &ResidueTarget::new(&base).at(&path));
            let git_dir = git_dir_of(&path).expect("git dir").expect("linked");
            // The Internal state is built so that `index.lock` is the ONLY
            // thing that makes it Internal: the edit is staged first, so
            // the index already reflects the working tree and the
            // unstaged-changes half of the after-phase says `After`. A
            // classifier that dropped the lock check would answer `After`
            // here — which is a real reachable state, a second `git add`
            // killed on an already-clean worktree — and a fixture that
            // left the change unstaged would confound the two
            // discriminators and stay green. Measured: this arm with the
            // change unstaged survives deleting the lock check from
            // `after_reference_present`.
            fixture
                .manager
                .candidate_stage(&mut NoHooks, &slot)
                .expect("stage, so the index already reflects the tree");
            fs::write(git_dir.join("index.lock"), "").expect("plant the lock");
            let internal = classify(site, &ResidueTarget::new(&base).at(&path));
            fs::remove_file(git_dir.join("index.lock")).expect("clear the lock");
            fs::write(path.join("a.txt"), "edited again\n").expect("a second unstaged change");
            fixture
                .manager
                .candidate_stage(&mut NoHooks, &slot)
                .expect("stage");
            let after = classify(site, &ResidueTarget::new(&base).at(&path));
            [none, internal, after]
        }
        EffectSiteId::Object(ObjectSite::CandidateWriteTree) => {
            let slot = fixture.add_task(&mut NoHooks, "alpha", 1);
            let path = fixture.manager.slot_path(&slot);
            let none = classify(site, &ResidueTarget::new(&base).at(&path));
            write_orphan(&base, "an object nothing references\n");
            let internal = classify(site, &ResidueTarget::new(&base).at(&path));
            let tree = fixture
                .manager
                .candidate_write_tree(&mut NoHooks, &slot)
                .expect("write-tree");
            let after = classify(site, &ResidueTarget::new(&base).at(&path).published(&tree));
            [none, internal, after]
        }
        EffectSiteId::Object(ObjectSite::SnapshotCommitTree | ObjectSite::CandidateCommitTree) => {
            let none = classify(site, &ResidueTarget::new(&base));
            write_orphan(&base, "an object nothing references\n");
            let internal = classify(site, &ResidueTarget::new(&base));
            let tree = git(&base, &["rev-parse", &format!("{}^{{tree}}", fixture.head)]);
            let commit = if site == EffectSiteId::Object(ObjectSite::SnapshotCommitTree) {
                fixture
                    .manager
                    .snapshot_commit_tree(&mut NoHooks, &tree, &fixture.head)
                    .expect("ephemeral commit")
            } else {
                fixture
                    .manager
                    .candidate_commit_tree(&mut NoHooks, &tree, &fixture.head, "candidate")
                    .expect("candidate commit")
            };
            let after = classify(site, &ResidueTarget::new(&base).published(&commit));
            [none, internal, after]
        }
        EffectSiteId::Object(ObjectSite::ProposalCherryPick) => {
            let slot = Slot::Staging { sequence: 1 };
            fixture
                .manager
                .write_intent(&mut NoHooks, &slot)
                .expect("intent");
            let path = fixture
                .manager
                .add_worktree(&mut NoHooks, &slot, &fixture.head)
                .expect("staging worktree");
            let bare = ResidueTarget::new(&base).at(&path).from_base(&fixture.head);
            let none = classify(site, &bare);
            let git_dir = git_dir_of(&path).expect("git dir").expect("linked");
            fs::write(git_dir.join("CHERRY_PICK_HEAD"), &fixture.side).expect("plant");
            let internal = classify(site, &bare);
            fs::remove_file(git_dir.join("CHERRY_PICK_HEAD")).expect("clear");
            let proposal = fixture
                .manager
                .proposal_cherry_pick(&mut NoHooks, &slot, &fixture.side)
                .expect("cherry-pick");
            let after = classify(
                site,
                &ResidueTarget::new(&base)
                    .at(&path)
                    .from_base(&fixture.head)
                    .published(&proposal),
            );
            [none, internal, after]
        }
        EffectSiteId::Object(ObjectSite::RepairMaterialize) => {
            let slot = fixture.add_task(&mut NoHooks, "repair", 1);
            let path = fixture.manager.slot_path(&slot);
            let none = classify(site, &ResidueTarget::new(&base).at(&path));
            let git_dir = git_dir_of(&path).expect("git dir").expect("linked");
            fs::write(git_dir.join("index.lock"), "").expect("plant the lock");
            let internal = classify(site, &ResidueTarget::new(&base).at(&path));
            fs::remove_file(git_dir.join("index.lock")).expect("clear the lock");
            fixture
                .manager
                .repair_materialize(&mut NoHooks, &slot, &fixture.side)
                .expect("materialize");
            // Measured, git 2.43: `cherry-pick --no-commit` writes
            // **`MERGE_MSG`**, not `CHERRY_PICK_HEAD` — that file is only
            // set when the pick is going to commit. So the after phase of
            // this site reads the index, and a file the frozen element list
            // does register (`CHERRY_PICK_HEAD`) is one the real command
            // never leaves. It is still constructed synthetically, because
            // `ObjectSite::RepairMaterialize.residue_elements()` registers
            // it and PR3 froze that; it will simply never appear in a
            // sampled histogram.
            assert!(
                !git_dir.join("CHERRY_PICK_HEAD").exists(),
                "a successful `cherry-pick --no-commit` sets no CHERRY_PICK_HEAD"
            );
            assert!(
                git_dir.join("MERGE_MSG").exists(),
                "what it does leave is MERGE_MSG, which this site's element list does not \
                     register and which `Worktree.Verify` reads as merge state — so the tabled \
                     recovery is entered either way"
            );
            let after = classify(site, &ResidueTarget::new(&base).at(&path));
            [none, internal, after]
        }
        other => panic!(
            "`{other}` registers a residue class and this grid has no arm for it; the domain \
                 is the frozen enums', not this function's"
        ),
    }
}

/// `command_internal_sub_effects`, synthetic half: "each residue element …
/// is constructed in a real temporary repository at the site's worktree,
/// `classify_object_residue` returns `Internal`, `Worktree.Verify` fails,
/// and the tabled recovery converges with fsck showing the objects
/// unreachable and untouched".
///
/// **The `Verify`-fails half is asserted where it holds and its negation
/// where it does not, and the partition is a count.**
/// [`element_breaks_quiescence`] carries the argument: an unreferenced
/// object and a Git temporary object file live in the shared object store,
/// are R27 — "Git's" — and are left by ordinary Git use, so a
/// `Worktree.Verify` that saw them would refuse to reuse an `OpenNoAttempt`
/// worktree in almost every real repository. Reported as a boundary, not
/// concealed as an omission.
#[test]
fn every_registered_residue_element_is_constructed_and_recovers() {
    let mut records: Vec<(EffectSiteId, SyntheticRecord)> = Vec::new();
    let mut quiescence_broken = 0usize;
    let mut object_store_only = 0usize;

    for site in residue_classified_sites() {
        for element in site.residue_elements() {
            let record = construct_and_recover(site, *element);
            assert!(record.constructed, "`{site}`/{element:?} was constructed");
            assert_eq!(
                record.classified,
                ObjectResidue::Internal,
                "`{site}`/{element:?} classifies Internal"
            );
            assert!(record.recovered, "`{site}`/{element:?} recovers");
            if element_breaks_quiescence(*element) {
                quiescence_broken += 1;
            } else {
                object_store_only += 1;
            }
            records.push((site, record));
        }
    }

    // Distinct-value counts rather than prose: the grid is 24 (site,
    // element) pairs, and the two halves of the Verify boundary are 12 and
    // 12. A site that grows an element, or an element that changes side,
    // moves one of these.
    assert_eq!(
        records.len(),
        residue_classified_sites()
            .iter()
            .map(|site| site.residue_elements().len())
            .sum::<usize>(),
        "one record per (site, element) the frozen enums register"
    );
    assert_eq!(records.len(), 24, "the frozen grid is 24 pairs");
    assert_eq!(
        quiescence_broken, 12,
        "elements that make a worktree non-quiescent"
    );
    assert_eq!(object_store_only, 12, "elements that are R27 and Git's");
    assert!(
        records
            .iter()
            .all(|(_, record)| record.classified == ObjectResidue::Internal),
        "every element of every registered class classifies into that class"
    );

    // The evidence record, in the packet's own type, per site.
    for site in residue_classified_sites() {
        let synthetic: Vec<SyntheticRecord> = records
            .iter()
            .filter(|(seen, _)| *seen == site)
            .map(|(_, record)| *record)
            .collect();
        assert_eq!(synthetic.len(), site.residue_elements().len());
        let evidence = Evidence::RecoveryProven {
            synthetic,
            sampling: SamplingRecord {
                n: SAMPLING_N,
                histogram: ClassHistogram::default(),
                unclassified: 0,
                recovered: true,
            },
        };
        assert_eq!(
            evidence.label(),
            EvidenceLabel::RecoveryProven,
            "a residue class never carries an executed-hook claim"
        );
        assert!(!evidence.claims_execution());
    }
}

/// Construct one element at one site, classify it, check quiescence, and
/// run the tabled recovery.
fn construct_and_recover(site: EffectSiteId, element: ResidueElement) -> SyntheticRecord {
    let tag = format!("syn-{}-{element:?}", site.variant().to_lowercase());
    let fixture = Fixture::created(&tag);
    let base = fixture.base.clone();

    // The site's owning worktree, and the state in which its after-phase
    // reference is absent — which is the state the sentence is about.
    let (slot, path) = match site {
        EffectSiteId::Worktree(WorktreeSite::AddStaging)
        | EffectSiteId::Object(ObjectSite::ProposalCherryPick) => {
            let slot = Slot::Staging { sequence: 1 };
            fixture
                .manager
                .write_intent(&mut NoHooks, &slot)
                .expect("intent");
            let path = fixture.manager.slot_path(&slot);
            (Some(slot), path)
        }
        EffectSiteId::Snapshot(SnapshotSite::Add) => {
            let slot = Slot::Snapshot {
                name: SnapshotName::gates(1, 1),
            };
            fixture
                .manager
                .write_intent(&mut NoHooks, &slot)
                .expect("intent");
            let path = fixture.manager.slot_path(&slot);
            (Some(slot), path)
        }
        EffectSiteId::Object(ObjectSite::SnapshotCommitTree | ObjectSite::CandidateCommitTree) => {
            (None, base.clone())
        }
        _ => {
            let slot = fixture.task("alpha", 1);
            fixture
                .manager
                .write_intent(&mut NoHooks, &slot)
                .expect("intent");
            let path = fixture.manager.slot_path(&slot);
            (Some(slot), path)
        }
    };

    // A populated worktree for every site whose residue lives inside one;
    // the three `Add` sites are about a worktree that was never populated.
    let is_add_site = matches!(
        site,
        EffectSiteId::Worktree(WorktreeSite::Add | WorktreeSite::AddStaging)
            | EffectSiteId::Snapshot(SnapshotSite::Add)
    );
    if let Some(slot) = slot.as_ref() {
        if is_add_site {
            register_unpopulated(&fixture, &path);
        } else {
            fixture
                .manager
                .add_worktree(&mut NoHooks, slot, &fixture.head)
                .expect("worktree");
            if site == EffectSiteId::Object(ObjectSite::CandidateStage) {
                // The after-phase reference of `git add -A` is an index that
                // reflects the working tree, so the interrupted prefix has
                // an unstaged change in it.
                fs::write(path.join("a.txt"), "edited\n").expect("unstaged change");
            }
        }
    }

    let object = construct_element(&fixture, &path, element);
    let target = ResidueTarget::new(&base).at(&path).from_base(&fixture.head);
    let classified = classify(site, &target);

    // The quiescence half, asserted in both directions.
    if let Some(slot) = slot.as_ref() {
        let verified = fixture
            .manager
            .verify_worktree(
                &mut NoHooks,
                slot,
                &Quiescence::AtBase(fixture.head.clone()),
            )
            .expect("verify");
        assert_eq!(
            verified.is_err(),
            element_breaks_quiescence(element),
            "`{site}`/{element:?}: Worktree.Verify must {} — see element_breaks_quiescence",
            if element_breaks_quiescence(element) {
                "fail"
            } else {
                "pass, because this element is R27 and Git's"
            }
        );
    }

    // The tabled recovery: the site's before-phase action. Forced removal
    // and a fresh add for a worktree site; nothing at all for the two
    // commit-tree sites, whose T-CAND-OBJ (a) action is "nothing to delete:
    // the unpinned object is left to Git".
    let before = unreachable_objects(&base).expect("fsck");
    let mut recovered = true;
    if let Some(slot) = slot.as_ref() {
        fixture
            .manager
            .remove_worktree(&mut NoHooks, slot)
            .expect("forced removal");
        fixture
            .manager
            .add_worktree(&mut NoHooks, slot, &fixture.head)
            .expect("recreate");
        recovered = fixture
            .manager
            .verify_worktree(
                &mut NoHooks,
                slot,
                &Quiescence::AtBase(fixture.head.clone()),
            )
            .expect("verify")
            .is_ok();
    }
    let after = unreachable_objects(&base).expect("fsck");
    if let Some(object) = object.as_deref() {
        assert!(
            before.iter().any(|id| id == object) && after.iter().any(|id| id == object),
            "fsck lists `{object}` unreachable before and after the recovery, untouched"
        );
    }
    assert!(
        before.iter().all(|id| after.contains(id)),
        "the recovery deletes no object: R27 is Git's"
    );

    SyntheticRecord {
        element,
        constructed: true,
        classified,
        recovered,
    }
}

/// Build one residue element in a real repository, returning the object id
/// when the element is one.
fn construct_element(fixture: &Fixture, path: &Path, element: ResidueElement) -> Option<String> {
    let git_dir = || {
        git_dir_of(path)
            .expect("git dir")
            .expect("the worktree has a git dir")
    };
    match element {
        ResidueElement::UnreferencedObject => Some(write_orphan(
            &fixture.base,
            "an object an interrupted command wrote\n",
        )),
        ResidueElement::TemporaryObjectFile => {
            let objects = object_directory(&fixture.base).expect("object directory");
            fs::write(objects.join("tmp_obj_synthetic"), b"partial").expect("temp object");
            None
        }
        ResidueElement::IndexLock => {
            fs::write(git_dir().join("index.lock"), "").expect("index.lock");
            None
        }
        ResidueElement::CherryPickHead => {
            fs::write(git_dir().join("CHERRY_PICK_HEAD"), &fixture.side).expect("plant");
            None
        }
        ResidueElement::MergeHead => {
            fs::write(git_dir().join("MERGE_HEAD"), &fixture.side).expect("plant");
            None
        }
        ResidueElement::MergeMsg => {
            fs::write(git_dir().join("MERGE_MSG"), "interrupted\n").expect("plant");
            None
        }
        ResidueElement::OrigHead => {
            fs::write(git_dir().join("ORIG_HEAD"), &fixture.head).expect("plant");
            None
        }
        ResidueElement::SequencerState => {
            let sequencer = git_dir().join("sequencer");
            fs::create_dir_all(&sequencer).expect("sequencer directory");
            fs::write(sequencer.join("todo"), "pick abc\n").expect("plant");
            None
        }
        // Already built by `register_unpopulated` before this is called:
        // the element *is* the state of the worktree, not a file added to
        // one.
        ResidueElement::RegisteredUnpopulatedWorktree => None,
    }
}

/// The frozen sample count, per site.
///
/// `command_internal_sub_effects`: "the Git child of the site is killed at
/// uncontrolled points through the process funnel across N runs (N frozen
/// per site in the registry …)". Eight, and the same for all four sampled
/// commands, because the claim each sample carries is per sample — "every
/// observed residue must classify into exactly one class and recover by the
/// classified action" — and is not a coverage claim about the classes. The
/// delays are a ladder across a *measured* uninterrupted run of the same
/// command in the same repository rather than a fixed duration, so the
/// sampler lands inside the command on a fast machine and on a slow one.
const SAMPLING_N: u32 = 8;

/// `slice_contract.proof_tests[7]`: "sampling harness kills the Git child
/// of `git add`, `write-tree`, `cherry-pick`, and `worktree add` across N
/// runs and every observed residue classifies into exactly one class and
/// recovers (histogram recorded; **Internal not required**)".
///
/// # The stability claim
///
/// This harness is nondeterministic by construction and the assertion is
/// chosen so that it is not: what is asserted is that **every** sample
/// classified into one of the three classes and recovered by that class's
/// tabled action, and that `unclassified == 0`. Which class a given sample
/// lands in is a race between the kill and Git, so the *counts* cannot be
/// asserted — a suite that required `Internal` would be red whenever the
/// machine was fast, and "no residue observed" is not a class.
///
/// # What the counts being unassertable does **not** excuse
///
/// It used to excuse two things, and `PR5-CONF-004` is both of them
/// (Fable's `PR5-CONF-002` is the same defect).
///
/// **The tally had no oracle.** `histogram.internal += 1` →
/// `histogram.none += 1` at the classifier's own match survived the whole
/// suite: every count moved, every assertion here was about the *total*, and
/// the total is invariant under a swap. So the observations are now kept
/// per sample and tallied a second time, here, by a different expression
/// over the same list — the two axes are the *classifier's answer* and *the
/// bucket it is counted in*, and only crossing them can see a bucket that is
/// counted under the wrong name.
///
/// **The histogram was never written down.** `outputs` requires, per site,
/// "sampling N **and observed-class histogram**", and
/// `effects/residue-classes.json` carried the N and not the histogram — its
/// own note conceded that the histogram "is printed … and is a property of
/// the machine, never asserted", which is a description of the omission
/// rather than a discharge of it. A byte-pinned artifact genuinely cannot
/// hold a machine-varying count, so the histogram goes to a **separate,
/// machine-varying evidence file**, this test writes it, and this test reads
/// it back: the record exists as a file a gate can collect, and the clause
/// is discharged by something other than stdout nobody keeps.
#[test]
fn sampled_git_child_kills_every_residue_classified_and_recovered() {
    let mut records = Vec::new();
    for site in SAMPLED_SITES {
        let run = sample_site(site);
        let record = run.record;
        println!(
            "residue sampling {site}: n={} none={} internal={} after={} unclassified={}",
            record.n,
            record.histogram.none,
            record.histogram.internal,
            record.histogram.after,
            record.unclassified
        );
        assert_eq!(record.n, SAMPLING_N);
        assert_eq!(
            run.observed.len(),
            SAMPLING_N as usize,
            "one observation per sample, or the tally below is over the wrong list"
        );

        // The independent tally. Not `tally()` again — a second call to the
        // code under test agrees with itself by construction — but a count
        // per class written out separately, so a bucket incremented under
        // the wrong name is a disagreement rather than an invisible swap.
        let counted = |wanted: ObjectResidue| -> u32 {
            u32::try_from(
                run.observed
                    .iter()
                    .filter(|sample| **sample == Some(wanted))
                    .count(),
            )
            .expect("a sample count fits in u32")
        };
        assert_eq!(
            (
                record.histogram.none,
                record.histogram.internal,
                record.histogram.after
            ),
            (
                counted(ObjectResidue::None),
                counted(ObjectResidue::Internal),
                counted(ObjectResidue::After)
            ),
            "{site}: the histogram does not count what the classifier answered: \
                 {:?}",
            run.observed
        );
        assert_eq!(
            record.histogram.total(),
            SAMPLING_N,
            "every sample is accounted for by exactly one class"
        );
        assert_eq!(
            record.unclassified, 0,
            "an unclassifiable residue is durable state no tabled action recovers"
        );
        assert!(
            record.recovered,
            "every sample recovered by its classified action"
        );
        records.push((site, record, run.budget, run.replayed));
    }
    assert_eq!(
        records.len(),
        4,
        "the four commands the contract's proof_tests name"
    );

    // What was actually spawned, when the kill fired and what it did to the
    // child — counted independently of the site labels and of the
    // observation list; see `SAMPLED_LAUNCHES`. What is counted per command
    // SHAPE rather than per site record is counted so because "the Git
    // child of `git add`, `write-tree`, `cherry-pick`, and `worktree add`"
    // is a claim about four commands and a site label is not one: two sites
    // that sampled the same shape would leave four records and four labels
    // intact. The kill floor at the end is the one claim that is neither —
    // it is over the sampling as a whole, and only over kills that *landed*,
    // for the reason written there.
    {
        let log = SAMPLED_LAUNCHES.lock().expect("the launch log");
        assert_eq!(
            log.len(),
            4 * SAMPLING_N as usize,
            "every sampled site must launch exactly its frozen N children, \
                 and an observation is pushed whether or not one was"
        );
        for (label, fixed) in [
            ("git add", WorkspaceManager::CANDIDATE_STAGE_ARGV[0]),
            (
                "git write-tree",
                WorkspaceManager::CANDIDATE_WRITE_TREE_ARGV[0],
            ),
            (
                "git cherry-pick",
                WorkspaceManager::PROPOSAL_CHERRY_PICK_ARGV[0],
            ),
            ("git worktree add", WorkspaceManager::WORKTREE_ADD_ARGV[0]),
        ] {
            let shape: Vec<&SampledLaunch> = log
                .iter()
                .filter(|launch| launch.argv[0] == fixed)
                .collect();
            let launched = shape.len();
            assert_eq!(
                launched, SAMPLING_N as usize,
                "{label}: the sampler launched it {launched} times, not N — the four \
                     command SHAPES are what the contract samples, not four site labels"
            );

            // The premise of every count below. A child that failed on its
            // own left the fixture's residue, not the kill's, and a reading
            // of the status loose enough to call that a kill would keep
            // counting kills after the kill was gone.
            let failed: Vec<Option<i32>> = shape
                .iter()
                .filter_map(|launch| match launch.end {
                    LaunchEnd::Failed(code) => Some(code),
                    LaunchEnd::Killed | LaunchEnd::Completed => None,
                })
                .collect();
            assert!(
                failed.is_empty(),
                "{label}: a sampled child neither died by the kill nor reached its \
                     own successful exit (codes {failed:?}) — what the classifier then \
                     saw is this fixture's failure, and no count of kills over these \
                     samples means anything"
            );

            // The rung each kill was **aimed at**.
            // `command_internal_sub_effects` (ii) says "killed at
            // **uncontrolled points** through the process funnel", and one
            // fixed delay is one point sampled N times: the ladder is that
            // clause, so it is asserted beside the kill rather than left to
            // the reader of `sample_site`.
            //
            // This is the aim and only the aim — `PR5-R5-001`. These are
            // the parameters the caller passed, so `sample_site` computing
            // a ladder is the whole of what they can witness: deleting
            // `std::thread::sleep(after)` from `kill_git_child`, which
            // fires every kill at the spawn instant and is the exact
            // negation of the clause cited above, left this list a perfect
            // ladder and the suite green on Linux and on the Windows guest.
            // The two assertions after it are over what the kills *did*.
            let delays: Vec<std::time::Duration> =
                shape.iter().map(|launch| launch.after).collect();
            assert!(
                delays.windows(2).all(|rungs| rungs[0] < rungs[1]),
                "{label}: the N kills must be aimed at N distinct, increasing points \
                     through the command, not at one point N times: {delays:?}"
            );

            // **A kill fired at every one of this command's children**
            // (`PR5-R5-002`). `slice_contract.proof_tests[8]` names four
            // commands — "the Git child of `git add`, `write-tree`,
            // `cherry-pick`, and `worktree add`" — and a floor over the
            // sampling as a whole discharges the clause for none of them:
            // guarding the kill with `if args[0] != "add"` left `git add`'s
            // eight children to reach their own exit 0, and the floor
            // below, the `Failed` arm above, the ladder and the whole suite
            // stayed green on both platforms.
            //
            // What is counted is kills **fired**, not kills that won their
            // race. Landing is a race against a command that may already
            // have finished — `git add` has measured 1 in 8 and 2 in 8, for
            // the reason written at the floor below — so a per-shape floor
            // on landings would stand on a margin of one or two samples and
            // would be red on the next machine. Firing is not a
            // race: the sampler either aimed a kill at this command's child
            // or it did not, N times, which is per command and exact.
            //
            // `fired` is written by `SampledChild::kill` itself, so an edit
            // that skips the kill skips the record with it. A count over
            // records written beside the call is what `PR5-R5-002` walked
            // past.
            let unfired = shape.iter().filter(|launch| launch.fired.is_none()).count();
            assert_eq!(
                unfired, 0,
                "{label}: {unfired} of this command's {launched} sampled children were \
                     never fired at — the contract names this command among the four whose \
                     Git child this harness kills, and a kill skipped for one of the four is \
                     invisible to every count taken over all four"
            );

            // **When each kill fired**, read off the clock by the kill
            // rather than off the delay the caller asked for
            // (`PR5-R5-001`). A kill cannot fire before the wait that
            // precedes it has elapsed, so with the ladder above this pins N
            // firings to N distinct, strictly increasing floors: the i-th
            // kill fired later than the rung the (i-1)-th was aimed at, and
            // no two of them can be the spawn instant. That is the
            // strongest statement available deterministically — a *ceiling*
            // on a firing would be an assertion about the scheduler — and
            // it is what the deleted wait destroys, since every kill then
            // fires within microseconds of its spawn.
            for launch in &shape {
                let fired = launch
                    .fired
                    .expect("every child was fired at, asserted just above");
                assert!(
                    fired >= launch.after,
                    "{label}: a kill fired {fired:?} after its child was spawned, sooner \
                         than the {:?} rung it was aimed at — a kill that does not wait its \
                         rung is the spawn instant sampled again, and the ladder above is \
                         then a ladder of nothing",
                    launch.after
                );
            }

            // How many of those N fired kills won their race with the
            // command. Machine-varying, so printed rather than asserted —
            // the same treatment, and for the same reason, as the class
            // histogram above. `git add` has measured 1/8 on Linux and on
            // the Windows guest and 2/8 on that same guest a run later; see
            // the floor below for why that number is reported, not required.
            let killed = shape
                .iter()
                .filter(|launch| launch.end == LaunchEnd::Killed)
                .count();
            println!("kill sampling {label}: killed {killed}/{launched}");
        }

        // **The kill itself** (`PR5-R4-001`), and the one assertion in this
        // test that a completed run does not also satisfy.
        //
        // Until this existed nothing here could tell a killed child from a
        // finished one. With `child.kill()` deleted the sampler still
        // spawned 4 × N children, `SAMPLED_LAUNCHES` still counted them,
        // every residue still classified into a legal class, recovery was
        // still idempotent and `effects/residue-histogram.json` was still
        // written and read back — recording *completion* residue under the
        // kill's name. Only the wait status changes, so only the wait
        // status can be the oracle.
        //
        // **Over the sampling as a whole, not per sample and not per
        // shape.** Per sample is wrong because a child that reaches its own
        // exit before its rung elapses is legal: the last rung is 8/9 of a
        // measured uninterrupted run and is meant to reach past the end of
        // a fast one. Per shape is wrong for a subtler reason — `git add`
        // measures **1** kill in 8 on Linux and on the Windows guest, and
        // **2** on that guest a run later, because `measure_budget`'s probe
        // writes the 1 200 blobs its samples then find already in the object
        // store. (The number moving between runs is the point, not a
        // correction to it: a floor set at either one is a floor on the
        // wrong side of the other on some machine.) Measured
        // outside this suite, same content in three worktrees of one
        // repository: 44 ms for the first `git add -A`, 10 ms and 9 ms for
        // the next two, with the loose-object count unmoved at 1 203. So
        // the samples run in about a fifth of the run their ladder is
        // scaled to, only the shortest rung lands inside them, and a
        // per-shape floor would be an assertion standing on a margin of one
        // sample — a red suite on the next machine rather than a stronger
        // proof. The per-shape counts are printed above so that margin
        // stays visible without being load-bearing.
        //
        // What discharges the **four-command** clause is therefore not this
        // floor — that was `PR5-R5-002` — but the per-shape firing count
        // above. The two are a pair, and each covers what the other cannot:
        // the firing count proves no command was skipped and could not tell
        // a kill from a call that recorded itself and threw its effect
        // away; this floor proves the kills are real and cannot tell which
        // of the four commands they reached.
        let killed = log
            .iter()
            .filter(|launch| launch.end == LaunchEnd::Killed)
            .count();
        assert!(
            killed > 0,
            "not one of the {} sampled Git children died by the kill — this harness \
                 sampled the residue its commands left when they FINISHED, and every \
                 other assertion in this test accepts that residue. Ends: {:?}",
            log.len(),
            log.iter().map(|launch| launch.end).collect::<Vec<_>>()
        );
    }

    // The evidence file `outputs` asks for, written and then read back.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(crate::effects::RESIDUE_HISTOGRAM_JSON);
    let emitted = serde_json::to_string_pretty(&serde_json::json!({
        "note": "decisions.effect_site_inventory.outputs, the observed-class \
                     histogram half: written by \
                     workspace_manager::tests::sampled_git_child_kills_every_residue_\
                     classified_and_recovered on every run. Machine-varying by \
                     construction -- which class a sample lands in is a race between \
                     the kill and Git -- so it is emitted here rather than pinned into \
                     effects/residue-classes.json, which carries the declarations.",
        "sampling_n": SAMPLING_N,
        "sites": records
            .iter()
            .map(|(site, record, budget, replayed)| serde_json::json!({
                "site": site.name(),
                "n": record.n,
                // The timescale the kill ladder was cut from. A red run's
                // artifact carries it, and `UPSTROKE_RESIDUE_BUDGET_US` feeds it
                // back -- which is what makes this sampler reproducible, since
                // it has no seed and this duration is the only variance a
                // replay can pin. Wake times, Git's own progress, cache state
                // and scheduling still vary between runs.
                "budget_us": u64::try_from(budget.as_micros()).unwrap_or(u64::MAX),
                "budget_replayed": replayed,
                "none": record.histogram.none,
                "internal": record.histogram.internal,
                "after": record.histogram.after,
                "unclassified": record.unclassified,
                "recovered": record.recovered,
            }))
            .collect::<Vec<_>>(),
    }))
    .expect("the histogram serializes");
    fs::write(&path, emitted + "\n").expect("write the observed-class histogram");

    let back: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read the histogram back"))
            .expect("the emitted histogram parses");
    let sites = back["sites"].as_array().expect("a sites array");
    assert_eq!(sites.len(), 4, "one histogram per sampled site");
    for (entry, (site, record, budget, _)) in sites.iter().zip(&records) {
        assert_eq!(
            entry["site"],
            site.name(),
            "the sites are in sampling order"
        );
        let total = ["none", "internal", "after"]
            .iter()
            .map(|class| entry[*class].as_u64().expect("a count"))
            .sum::<u64>();
        assert_eq!(
            total,
            u64::from(SAMPLING_N),
            "{site}: the written histogram accounts for every sample"
        );
        assert_eq!(entry["unclassified"], record.unclassified);
        assert_eq!(
            entry["budget_us"].as_u64(),
            u64::try_from(budget.as_micros()).ok(),
            "{site}: the artifact must carry the timescale a replay needs, or a \
                 red CI run cannot be reproduced from it"
        );
    }
}

/// One site's sampling run: the packet's record, and the per-sample
/// observations it was tallied from.
///
/// The two are carried separately because the record alone cannot be
/// checked (`PR5-CONF-004`). `histogram.internal += 1` → `histogram.none +=
/// 1` survived the whole suite: which bucket a sample lands in is a race, so
/// no assertion on the *counts* can catch a swapped arm, and the only
/// available oracle is the classifier's own answers, tallied a second time
/// by something that is not the code under test.
struct SamplingRun {
    record: SamplingRecord,
    /// The timescale the kill ladder was cut from, and whether it was measured
    /// on this machine or replayed from `UPSTROKE_RESIDUE_BUDGET_US`. This is the
    /// only variance a replay can pin -- see [`measure_budget`] -- so it is the
    /// most a recorded run can offer. It is not *all* the variance: actual wake
    /// times, Git's progress, cache state and scheduling still differ, so a
    /// replay reproduces the nominal ladder rather than the original run.
    budget: std::time::Duration,
    replayed: bool,
    /// What `classify_object_residue` answered for each sample, in order.
    /// `None` is a sample it could not classify at all.
    observed: Vec<Option<ObjectResidue>>,
}

/// Tally per-sample observations into the packet's histogram.
///
/// The single place the mapping from class to bucket is written, so the test
/// can check it against an independent tally of the same list.
fn tally(observed: &[Option<ObjectResidue>]) -> (ClassHistogram, u32) {
    let mut histogram = ClassHistogram::default();
    let mut unclassified = 0;
    for sample in observed {
        match sample {
            Some(ObjectResidue::None) => histogram.none += 1,
            Some(ObjectResidue::Internal) => histogram.internal += 1,
            Some(ObjectResidue::After) => histogram.after += 1,
            None => unclassified += 1,
        }
    }
    (histogram, unclassified)
}

/// No sampled funnel builds a Git argument from a literal (Fable's
/// `PR5-CONF-004`).
///
/// Sharing the lists makes the *transcription* impossible; this is what
/// stops a funnel growing an argument beside its list and putting the
/// divergence back. `command_internal_sub_effects` (ii) says the sampled
/// child is "the Git child of the site", and a child spawned with a
/// different argv is a different child however faithful the list is.
///
/// The two axes are the *shared list* and the *call site that uses it*.
/// Sharing covers the first; a funnel that appends `"--force"` inline is
/// still an un-shared argument, and only reading the call sites can see it.
/// The dynamic arguments each funnel legitimately adds — a path, a commit —
/// are counted rather than forbidden, so growing one is a change to this
/// number rather than a silent widening.
#[test]
fn no_sampled_funnel_builds_its_argv_from_a_literal() {
    // (function, how many `OsString::from(<expression>)` arguments it adds
    // beyond its shared list, and what they are).
    const SAMPLED: &[(&str, usize, &str)] = &[
        (
            "pub fn add_worktree(",
            1,
            "the commit; the path is a PathBuf",
        ),
        ("pub fn candidate_stage(", 0, "none"),
        ("pub fn candidate_write_tree(", 0, "none"),
        ("pub fn proposal_cherry_pick(", 1, "the commit to pick"),
    ];
    // CRLF normalized first: the Windows guest checks this tree out with it,
    // and `find("\n    }\n")` does not match `\r\n    }\r\n`. Measured — this
    // census passed on Linux and panicked "the function ends" on the guest.
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/workspace_manager.rs"))
            .expect("the funnel module's source")
            .replace("\r\n", "\n");
    for (signature, dynamic, what) in SAMPLED {
        let body = source
            .split_once(signature)
            .unwrap_or_else(|| panic!("`{signature}` is no longer in this file"))
            .1;
        let body = &body[..body.find("\n    }\n").expect("the function ends")];
        let literals = body.matches("OsString::from(\"").count();
        assert_eq!(
            literals, 0,
            "`{signature}` builds {literals} Git argument(s) from a string literal; \
                 every fixed argument belongs in the shared list the kill sampler reads, \
                 or the sampler stops running the command the funnel runs"
        );
        let dynamics = body.matches("OsString::from(").count();
        assert_eq!(
            dynamics, *dynamic,
            "`{signature}` adds {dynamics} dynamic Git argument(s), not {dynamic} \
                 ({what}); if that is deliberate, `sampled_command` needs the same one"
        );
    }
}

/// Kill the Git child of one site `SAMPLING_N` times and classify what is
/// left.
fn sample_site(site: EffectSiteId) -> SamplingRun {
    let tag = format!("sample-{}", site.variant().to_lowercase());
    let fixture = Fixture::created(&tag);
    let base = fixture.base.clone();
    let measured = measure_budget(site, &fixture);
    let replayed = replayed_budget(site);
    let budget = replayed.unwrap_or(measured);
    println!(
        "residue sampling {site}: budget={}us{} ladder={:?}",
        budget.as_micros(),
        if replayed.is_some() {
            format!(" (replayed; measured {}us)", measured.as_micros())
        } else {
            String::new()
        },
        (0..SAMPLING_N)
            .map(|run| budget
                .mul_f64(f64::from(run + 1) / f64::from(SAMPLING_N + 1))
                .as_micros())
            .collect::<Vec<_>>()
    );
    let mut observed: Vec<Option<ObjectResidue>> = Vec::new();
    let mut recovered = true;

    for run in 0..SAMPLING_N {
        let slot = sample_slot(site, &fixture, run);
        fixture
            .manager
            .write_intent(&mut NoHooks, &slot)
            .expect("intent");
        let path = fixture.manager.slot_path(&slot);
        if site != EffectSiteId::Worktree(WorktreeSite::Add) {
            fixture
                .manager
                .add_worktree(&mut NoHooks, &slot, &fixture.head)
                .expect("worktree");
            populate_for_sampling(site, &path);
        }

        let (args, cwd) = sampled_command(site, &fixture, &slot);
        let delay = budget.mul_f64(f64::from(run + 1) / f64::from(SAMPLING_N + 1));
        kill_git_child(&cwd, &args, delay);

        let target = ResidueTarget::new(&base).at(&path).from_base(&fixture.head);
        observed.push(classify_object_residue(site, &target).ok());
        if !recover_sample(&fixture, &slot) {
            recovered = false;
        }
    }

    let (histogram, unclassified) = tally(&observed);
    SamplingRun {
        budget,
        replayed: replayed.is_some(),
        record: SamplingRecord {
            n: SAMPLING_N,
            histogram,
            unclassified,
            recovered,
        },
        observed,
    }
}

fn sample_slot(site: EffectSiteId, fixture: &Fixture, run: u32) -> Slot {
    match site {
        EffectSiteId::Object(ObjectSite::ProposalCherryPick) => Slot::Staging {
            sequence: u64::from(run),
        },
        _ => fixture.task("alpha", run),
    }
}

/// The exact command the site's funnel runs, and where it runs it.
fn sampled_command(site: EffectSiteId, fixture: &Fixture, slot: &Slot) -> (Vec<String>, PathBuf) {
    let path = fixture.manager.slot_path(slot);
    // Read from the funnel's own lists rather than transcribed from them
    // (Fable's `PR5-CONF-004`): the transcription was faithful and nothing
    // kept it so, and a funnel that grew a flag would leave the sampler
    // sampling a stale command with every assertion here still green.
    let fixed = |argv: &[&str]| -> Vec<String> { argv.iter().map(|a| (*a).to_owned()).collect() };
    match site {
        EffectSiteId::Object(ObjectSite::CandidateStage) => {
            (fixed(&WorkspaceManager::CANDIDATE_STAGE_ARGV), path)
        }
        EffectSiteId::Object(ObjectSite::CandidateWriteTree) => {
            (fixed(&WorkspaceManager::CANDIDATE_WRITE_TREE_ARGV), path)
        }
        EffectSiteId::Object(ObjectSite::ProposalCherryPick) => {
            let mut argv = fixed(&WorkspaceManager::PROPOSAL_CHERRY_PICK_ARGV);
            argv.push(fixture.side.clone());
            (argv, path)
        }
        EffectSiteId::Worktree(WorktreeSite::Add) => {
            let mut argv = fixed(&WorkspaceManager::WORKTREE_ADD_ARGV);
            argv.push(path.to_string_lossy().into_owned());
            argv.push(fixture.head.clone());
            (argv, fixture.base.clone())
        }
        other => panic!("`{other}` is not one of the four commands the contract samples"),
    }
}

/// How long the same command takes when nothing kills it.
///
/// Measured in a **probe slot of its own**, which is then removed. The
/// first draft measured it in the very worktree the next sample would kill
/// in, and the probe therefore *performed* the command first: `write-tree`
/// found a valid cache-tree and every one of its eight samples classified
/// `None`, which read as a stable histogram and was an artefact of the
/// fixture. A probe that mutates the state under test is the
/// "environment assumption in a test" class this project has recorded.
fn measure_budget(site: EffectSiteId, fixture: &Fixture) -> std::time::Duration {
    let probe = match site {
        EffectSiteId::Object(ObjectSite::ProposalCherryPick) => Slot::Staging { sequence: 9_999 },
        _ => fixture.task("probe", 9_999),
    };
    fixture
        .manager
        .write_intent(&mut NoHooks, &probe)
        .expect("probe intent");
    let path = fixture.manager.slot_path(&probe);
    let elapsed = if site == EffectSiteId::Worktree(WorktreeSite::Add) {
        let (args, cwd) = sampled_command(site, fixture, &probe);
        let start = std::time::Instant::now();
        let output = git_out(&cwd, &args.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(output.status.success(), "the probe must really run");
        start.elapsed()
    } else {
        fixture
            .manager
            .add_worktree(&mut NoHooks, &probe, &fixture.head)
            .expect("probe worktree");
        populate_for_sampling(site, &path);
        let (args, cwd) = sampled_command(site, fixture, &probe);
        let start = std::time::Instant::now();
        let output = git_out(&cwd, &args.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(
            output.status.success(),
            "the probe must really run: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        start.elapsed()
    };
    fixture
        .manager
        .remove_worktree(&mut NoHooks, &probe)
        .expect("remove the probe");
    fixture
        .manager
        .remove_intent(&mut NoHooks, &probe)
        .expect("remove the probe intent");
    elapsed.max(std::time::Duration::from_micros(200))
}

/// A recorded timescale to replay instead of measuring one.
///
/// **There is no random number generator in this sampler and nothing to seed.**
/// The kill ladder is a fixed set of fractions — `(run + 1) / (SAMPLING_N + 1)` —
/// of a single duration, and that duration is how long one real `git` took on
/// this machine at this moment. Load, page cache and disk move it; the fractions
/// never move. So the measured budget is the only variance a replay can pin,
/// and pinning it is the nearest thing to a reproducible failure available here
/// -- the ladder becomes identical, though the wake times it produces and Git's
/// own progress against them still vary run to run.
///
/// A fixed default would be worse than the status quo: the point of re-measuring
/// on every run is that the ladder lands in different places on different
/// machines, which is how a sampler with `SAMPLING_N = 8` covers more than eight
/// kill points over its lifetime. The measurement stays; it is merely recorded,
/// and overridable when replaying one specific red run.
/// The sites this sampler drives, and therefore the only sites a replay spec
/// may name.
///
/// Shared with [`parse_budget_spec`] so the two cannot disagree. Validating
/// against the whole `EffectSiteId` registry was not enough: it accepted
/// `Object.CandidateCommitTree` — a real site nothing here samples — whereupon
/// every site fell through to a fresh measurement and the run reported success
/// having replayed nothing. That is the same fail-open the strictness was added
/// to remove, wearing a name that passes validation.
const SAMPLED_SITES: [EffectSiteId; 4] = [
    EffectSiteId::Object(ObjectSite::CandidateStage),
    EffectSiteId::Object(ObjectSite::CandidateWriteTree),
    EffectSiteId::Object(ObjectSite::ProposalCherryPick),
    EffectSiteId::Worktree(WorktreeSite::Add),
];

/// Why a `UPSTROKE_RESIDUE_BUDGET_US` spec was refused.
#[derive(Debug, PartialEq, Eq)]
enum BudgetSpecError {
    /// An entry with no `=`.
    Malformed(String),
    /// A site name no `EffectSiteId` answers to. Almost always a typo — and a
    /// typo is **indistinguishable from "leave this site to measure"** unless
    /// every entry is validated, which is why this is an error rather than a
    /// miss.
    UnknownSite(String),
    /// A real site that this sampler never drives. Distinct from
    /// [`Self::UnknownSite`] because the fix is different: the name is spelled
    /// correctly and simply cannot be replayed, which the message should say
    /// rather than claiming it does not exist.
    NotSampled(String),
    /// A value that is not a positive whole number of microseconds.
    NotAPositiveInteger(String),
    /// A value past [`MAX_REPLAY_BUDGET_US`].
    AboveCeiling { site: String, micros: u64 },
    /// The same site named twice, where which one wins would depend on order.
    Duplicate(String),
}

/// The largest replay budget a spec may ask for.
///
/// Four sites each sleep a ladder summing to about four budgets, so the whole
/// sampling costs roughly `16 x budget`. At five seconds that is 80 seconds of
/// sleeping — generous, since the largest budget ever *measured* here is about
/// 45 ms, and bounded, which unbounded input was not: `u64::MAX` parsed happily
/// and asked its first rung to sleep about 64,949 years, ending only at CI's
/// job timeout.
const MAX_REPLAY_BUDGET_US: u64 = 5_000_000;

/// Parse a replay spec, validating **every** entry, and return the budget for
/// `site` if the spec names it.
///
/// Takes the spec as an argument rather than reading the environment, so it can
/// be unit tested without mutating process-global state that parallel tests
/// share. [`replayed_budget`] does the environment read at the edge.
fn parse_budget_spec(
    raw: &str,
    site: EffectSiteId,
) -> Result<Option<std::time::Duration>, BudgetSpecError> {
    let mut found = None;
    let mut seen: Vec<String> = Vec::new();
    for entry in raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (name, micros) = entry
            .split_once('=')
            .ok_or_else(|| BudgetSpecError::Malformed(entry.to_owned()))?;
        let (name, micros) = (name.trim(), micros.trim());
        // Validated against the real site registry, not a list of the four this
        // test samples, so a name that is merely misspelled is caught the same
        // way as one that does not exist at all.
        let named = EffectSiteId::from_name(name)
            .map_err(|_| BudgetSpecError::UnknownSite(name.to_owned()))?;
        if !SAMPLED_SITES.contains(&named) {
            return Err(BudgetSpecError::NotSampled(name.to_owned()));
        }
        if seen.iter().any(|already| already == name) {
            return Err(BudgetSpecError::Duplicate(name.to_owned()));
        }
        seen.push(name.to_owned());
        let value = micros
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| BudgetSpecError::NotAPositiveInteger(entry.to_owned()))?;
        if value > MAX_REPLAY_BUDGET_US {
            return Err(BudgetSpecError::AboveCeiling {
                site: name.to_owned(),
                micros: value,
            });
        }
        if named == site {
            found = Some(std::time::Duration::from_micros(value));
        }
    }
    Ok(found)
}

/// Decide a replay from what the environment gave us.
///
/// Takes the [`std::env::var`] result rather than reading it, so the three
/// cases can be tested without mutating process-global state that parallel
/// tests share. The distinction matters: `var` reports **`NotPresent` and
/// `NotUnicode` as different errors**, and collapsing them made a spec that
/// was present but not valid UTF-8 look exactly like no spec at all — every
/// site measuring fresh while the adjacent promise says an unhonourable spec
/// is refused.
fn budget_from_var(
    var: Result<String, std::env::VarError>,
    site: EffectSiteId,
) -> Option<std::time::Duration> {
    let raw = match var {
        Ok(raw) => raw,
        Err(std::env::VarError::NotPresent) => return None,
        Err(std::env::VarError::NotUnicode(raw)) => panic!(
            "UPSTROKE_RESIDUE_BUDGET_US is set but is not valid UTF-8 ({raw:?}), so it \
                 cannot be parsed. It will not be treated as unset."
        ),
    };
    match parse_budget_spec(&raw, site) {
        Ok(found) => found,
        Err(error) => panic!(
            "UPSTROKE_RESIDUE_BUDGET_US is not a spec this run can honour: {error:?}. \
                 Fix the spec or unset it; it will not be silently ignored."
        ),
    }
}

/// The environment read itself, kept to one line so nothing but the read is
/// untestable.
fn replayed_budget(site: EffectSiteId) -> Option<std::time::Duration> {
    budget_from_var(std::env::var("UPSTROKE_RESIDUE_BUDGET_US"), site)
}

/// Enough work in the worktree that the sampled command has a middle to be
/// killed in: many files across many directories, so `git add` writes many
/// blobs and `write-tree` writes many trees.
fn populate_for_sampling(site: EffectSiteId, path: &Path) {
    if site == EffectSiteId::Object(ObjectSite::ProposalCherryPick) {
        return;
    }
    for directory in 0..60 {
        let bulk = path.join(format!("bulk{directory}"));
        fs::create_dir_all(&bulk).expect("bulk directory");
        for index in 0..20 {
            fs::write(
                bulk.join(format!("f{index}.txt")),
                format!("{directory}-{index}-{}", "x".repeat(2048)),
            )
            .expect("bulk file");
        }
    }
    if site == EffectSiteId::Object(ObjectSite::CandidateWriteTree) {
        // `write-tree` reads an index, so the bulk has to be in one.
        git(path, &["add", "-A"]);
    }
}

/// How one sampled Git child ended.
///
/// The wait status is the **only** thing in this harness that the kill
/// changes, so it is the only place the kill can be observed. Everything
/// else — the spawn, the argv, the residue, its class, the recovery, the
/// evidence file — is identical whether the child was killed or ran to its
/// own end, which is exactly why `PR5-R4-001` could delete `child.kill()`
/// and keep the suite green on both platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchEnd {
    /// The status carries this platform's signature of [`Child::kill`].
    ///
    /// [`Child::kill`]: std::process::Child::kill
    Killed,
    /// The child reached its own **successful** exit before the kill
    /// landed. Legal — the delay ladder deliberately reaches past the end
    /// of a fast run — and the reason the kill floor is asserted over the
    /// sampling as a whole rather than per sample.
    Completed,
    /// Neither: the sampled command failed on its own. Then what the
    /// classifier saw is the *fixture's* residue, not the kill's, and no
    /// count of kills over these samples means anything.
    Failed(Option<i32>),
}

/// Read a [`LaunchEnd`] off a wait status.
///
/// The signature is a **value** per platform, not the property
/// `!status.success()`: a command that merely failed also fails to succeed,
/// and reading that as a kill is how a kill-count keeps counting after the
/// kill is gone. The third arm exists so that such a command reddens the
/// suite instead of being miscounted.
fn launch_end(status: &std::process::ExitStatus) -> LaunchEnd {
    // `Child::kill` sends `SIGKILL`. No exit a child reaches on its own can
    // carry a signal at all, and nothing else here signals this child, so
    // the signal is a fingerprint the kill alone can leave.
    #[cfg(unix)]
    let killed = std::os::unix::process::ExitStatusExt::signal(status) == Some(libc::SIGKILL);
    // `Child::kill` is `TerminateProcess(handle, 1)`, so the fingerprint
    // here is exit code 1. `measure_budget` asserts that this same command
    // in this same fixture exits 0 when nothing kills it, so 1 is not an
    // end any of these four commands reaches by itself.
    #[cfg(windows)]
    let killed = status.code() == Some(1);
    if killed {
        LaunchEnd::Killed
    } else if status.success() {
        LaunchEnd::Completed
    } else {
        LaunchEnd::Failed(status.code())
    }
}

/// One Git child the sampler launched: what it ran, which rung of the delay
/// ladder its kill was **aimed at**, when a kill actually **fired** at it,
/// and how it ended.
///
/// `after` and `fired` are two different things, and `PR5-R5-001` is the
/// difference. `after` is the caller's parameter: it is recorded whatever
/// the sampler does with it, so a ladder of `after`s is a ladder of
/// *intentions* and stays a perfect one after the wait that realizes it is
/// deleted. `fired` is the clock, read inside [`SampledChild::kill`]: it
/// exists only if a kill ran at this child, and it moves when the wait
/// before the kill does.
struct SampledLaunch {
    argv: Vec<String>,
    after: std::time::Duration,
    fired: Option<std::time::Duration>,
    end: LaunchEnd,
}

/// Every Git child the sampler actually launched, in order.
///
/// The independent observer of *launches and of their kills*.
/// `command_internal_sub_effects` freezes N per site and
/// `slice_contract.proof_tests[8]` names four commands, and both are claims
/// about what was spawned — while every assertion in the sampling test is
/// over `run.observed`, the list the loop **pushes to**, and over the
/// residues that list classifies into. Nothing counted a spawn. A run that
/// skipped one kill and pushed its observation anyway satisfied the length
/// assertion, the histogram total and the serialized `sampling_n` alike;
/// and a site that spawned another site's command left every count, class
/// and evidence record identical, because any Git child that leaves a
/// classifiable residue in the slot satisfies them all. Both were measured
/// surviving the whole suite.
///
/// Counting launches is only the first half, and `PR5-R4-001` is the
/// second: both live passages say the child is **killed**, and with
/// `child.kill()` deleted the sampler still spawned 4 × N children, still
/// classified a legal residue from each, still recovered and still wrote
/// the histogram — of *completion* residue, filed under the kill's name.
/// So each entry also carries how its child ended and at which rung of the
/// ladder the kill fired.
///
/// It has to be collected **here** and not at the call site: the edit that
/// drops a kill skips the call, so an observer beside the call would still
/// run and would count a launch that never happened.
///
/// Round 5 stopped one level short of that same rule, and `PR5-R5-001` and
/// `PR5-R5-002` are what it cost. Inside this function the *launch* is
/// observed and the *kill* was not: the parameter the kill was aimed at was
/// recorded beside the call, so deleting the wait left the record intact,
/// and the record was pushed after `wait()` for every child, so skipping the
/// kill for one of the four commands left the record intact too. The kill's
/// own record therefore has to be collected inside the kill, which is what
/// [`SampledChild`] is for.
static SAMPLED_LAUNCHES: std::sync::Mutex<Vec<SampledLaunch>> = std::sync::Mutex::new(Vec::new());

/// The Git child the sampler kills, wrapped so that **the kill records
/// itself**.
///
/// [`Self::kill`] is inherent, so `child.kill()` in `kill_git_child` is this
/// method and not [`Child::kill`], and the note that a kill ran is written
/// by the statement that runs it rather than beside it. Both of round 5's
/// surviving mutations are edits that stop a kill happening — one deletes
/// the wait before it, one skips it for a single command — and both walked
/// past records that were written whether the kill happened or not.
///
/// It is deliberately **blind to the command it is killing**: no argv
/// reaches it, so the per-command firing count in the sampling test cannot
/// be defeated inside this type without first giving it one.
///
/// [`Child::kill`]: std::process::Child::kill
struct SampledChild {
    child: std::process::Child,
    /// Started once [`Command::spawn`] has returned, so what [`Self::kill`]
    /// reads off it is time the child was left *running* rather than the
    /// cost of starting it — and so an unwaited kill reads as the ~0 it is.
    spawned: std::time::Instant,
    /// What the clock said when a kill fired at this child, or `None` if
    /// none ever did. Written only by [`Self::kill`].
    fired: Option<std::time::Duration>,
}

impl SampledChild {
    fn spawn(cwd: &Path, args: &[String]) -> Self {
        let child = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["-c", "core.fsmonitor=false"])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the sampled git child");
        Self {
            child,
            spawned: std::time::Instant::now(),
            fired: None,
        }
    }

    /// Kill the child, recording when the kill fired.
    ///
    /// The clock is read at the instant of the kill and stored *after* the
    /// kill has returned. Read at that instant, the value is the kill's
    /// actual timing rather than the timing it was asked for; stored after
    /// the call, the record cannot outlive the call's removal, because
    /// deleting `self.child.kill()` leaves `outcome` unbound and the module
    /// stops compiling.
    ///
    /// What that still leaves reachable is a fake that keeps the call and
    /// throws its effect away. The kill floor at the end of the sampling
    /// test is what covers that: it is over wait statuses, which nothing
    /// but a real kill produces.
    fn kill(&mut self) -> std::io::Result<()> {
        let fired = self.spawned.elapsed();
        let outcome = self.child.kill();
        self.fired = Some(fired);
        outcome
    }

    fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait()
    }
}

fn kill_git_child(cwd: &Path, args: &[String], after: std::time::Duration) {
    let mut child = SampledChild::spawn(cwd, args);
    std::thread::sleep(after);
    let _ = child.kill();
    // Reaped rather than discarded: this status is the whole observation.
    let status = child.wait().expect("reap the sampled git child");
    SAMPLED_LAUNCHES
        .lock()
        .expect("the launch log")
        .push(SampledLaunch {
            argv: args.to_vec(),
            after,
            fired: child.fired,
            end: launch_end(&status),
        });
}

/// The tabled recovery for whatever the sample left: forced removal of the
/// worktree and its intent, which is the before-phase action every
/// `Internal` residue routes to and is idempotent for the other two.
fn recover_sample(fixture: &Fixture, slot: &Slot) -> bool {
    fixture
        .manager
        .remove_worktree(&mut NoHooks, slot)
        .expect("forced removal converges");
    fixture
        .manager
        .remove_intent(&mut NoHooks, slot)
        .expect("intent removal converges");
    let path = fixture.manager.slot_path(slot);
    !path.exists()
        && !fixture
            .manager
            .worktree_records()
            .expect("records")
            .iter()
            .any(|record| canonical_prefix(&record.path).ok() == canonical_prefix(&path).ok())
}

// -----------------------------------------------------------------------
// ST-07 for this lane: every site, both phases
// -----------------------------------------------------------------------

/// `fault_injection_registry.completeness_rule`: "every site x hook phase …
/// is observed executed at least once by the suite … an unobserved site,
/// phase, point, or mode fails".
///
/// Restricted to the four groups this lane owns, and derived from their
/// `ALL` slices, so a group that gains a variant fails this until a funnel
/// executes it.
#[test]
fn every_site_this_lane_owns_executes_both_hook_phases() {
    let fixture = Fixture::created("grand-tour");
    let (mut hooks, shared) = harness();
    let manager = &fixture.manager;
    let integration = "refs/heads/upstroke/run-1";
    let candidates = "refs/upstroke/runs/run-1/candidates/kalpha/1";
    let pin = "refs/upstroke/runs/run-1/candidate-prepared/kalpha/1";
    let prepared = "refs/upstroke/runs/run-1/prepared/1";

    // The execution root already exists (Fixture::created), so run the site
    // again: it is idempotent and this is the tour's first observation.
    manager
        .create_execution_root(&mut hooks)
        .expect("Worktree.CreateExecutionRoot");
    manager
        .create_ref_zero_old(
            &mut hooks,
            RefSite::CreateIntegration,
            integration,
            &fixture.head,
        )
        .expect("Ref.CreateIntegration");

    // A task worktree, its capture, and its snapshot.
    let task = fixture.task("alpha", 1);
    manager
        .write_intent(&mut hooks, &task)
        .expect("Worktree.WriteIntent");
    let task_path = manager
        .add_worktree(&mut hooks, &task, &fixture.head)
        .expect("Worktree.Add");
    manager
        .verify_worktree(&mut hooks, &task, &Quiescence::AtBase(fixture.head.clone()))
        .expect("Worktree.Verify")
        .expect("quiescent");
    fs::write(task_path.join("worker.txt"), "worker\n").expect("worker edit");
    manager
        .candidate_stage(&mut hooks, &task)
        .expect("Object.CandidateStage");
    let tree = manager
        .candidate_write_tree(&mut hooks, &task)
        .expect("Object.CandidateWriteTree");

    let snapshot = manager
        .add_snapshot(
            &mut hooks,
            &SnapshotName::gates(1, 1),
            &SnapshotInput::Tree {
                tree: tree.clone(),
                parent: fixture.head.clone(),
            },
        )
        .expect("Object.SnapshotCommitTree + Snapshot.WriteIntent + Snapshot.Add");
    manager
        .remove_snapshot(&mut hooks, &snapshot)
        .expect("Snapshot.Remove + Snapshot.RemoveIntent");

    let candidate = manager
        .candidate_commit_tree(&mut hooks, &tree, &fixture.head, "candidate")
        .expect("Object.CandidateCommitTree");
    manager
        .create_ref_zero_old(&mut hooks, RefSite::PinCandidatePrepared, pin, &candidate)
        .expect("Ref.PinCandidatePrepared");
    manager
        .create_ref_zero_old(
            &mut hooks,
            RefSite::CreateCandidates,
            candidates,
            &candidate,
        )
        .expect("Ref.CreateCandidates");
    manager
        .delete_ref_expected_old(&mut hooks, RefSite::DeleteCandidatePin, pin, &candidate)
        .expect("Ref.DeleteCandidatePin");
    manager
        .remove_worktree(&mut hooks, &task)
        .expect("Worktree.Remove");
    manager
        .remove_intent(&mut hooks, &task)
        .expect("Worktree.RemoveIntent");

    // A repair worktree, for the last Object site.
    let repair = fixture.task("repair", 1);
    manager.write_intent(&mut hooks, &repair).expect("intent");
    manager
        .add_worktree(&mut hooks, &repair, &fixture.head)
        .expect("worktree");
    manager
        .repair_materialize(&mut hooks, &repair, &fixture.side)
        .expect("Object.RepairMaterialize");
    manager
        .remove_worktree(&mut hooks, &repair)
        .expect("remove");
    manager.remove_intent(&mut hooks, &repair).expect("intent");

    // The stale integration transaction: staging, cherry-pick, pin, CAS.
    let staging = Slot::Staging { sequence: 1 };
    manager
        .write_intent(&mut hooks, &staging)
        .expect("Worktree.WriteStagingIntent");
    manager
        .add_worktree(&mut hooks, &staging, &fixture.head)
        .expect("Worktree.AddStaging");
    let proposal = manager
        .proposal_cherry_pick(&mut hooks, &staging, &fixture.side)
        .expect("Object.ProposalCherryPick");
    manager
        .create_ref_zero_old(&mut hooks, RefSite::PinPrepared, prepared, &proposal)
        .expect("Ref.PinPrepared");
    manager
        .compare_and_swap_ref(
            &mut hooks,
            RefSite::CompareAndSwapIntegration,
            integration,
            &fixture.head,
            &proposal,
        )
        .expect("Ref.CompareAndSwapIntegration");
    manager
        .delete_ref_expected_old(&mut hooks, RefSite::DeletePreparedPin, prepared, &proposal)
        .expect("Ref.DeletePreparedPin");
    manager
        .remove_worktree(&mut hooks, &staging)
        .expect("Worktree.RemoveStaging");
    manager
        .remove_intent(&mut hooks, &staging)
        .expect("Worktree.RemoveStagingIntent");

    // The exact-base fast sequence: it creates no staging worktree,
    // cherry-picks nothing, and takes no prepared pin. The absence is
    // proved *inside* a sequence that demonstrably happened.
    shared
        .lock()
        .expect("harness")
        .begin_fast_sequence("exact-base-fast");
    let fast_task = fixture.task("fast", 1);
    manager
        .write_intent(&mut hooks, &fast_task)
        .expect("intent");
    let fast_path = manager
        .add_worktree(&mut hooks, &fast_task, &proposal)
        .expect("worktree");
    fs::write(fast_path.join("fast.txt"), "fast\n").expect("edit");
    manager
        .candidate_stage(&mut hooks, &fast_task)
        .expect("stage");
    let fast_tree = manager
        .candidate_write_tree(&mut hooks, &fast_task)
        .expect("write-tree");
    let fast_commit = manager
        .candidate_commit_tree(&mut hooks, &fast_tree, &proposal, "fast candidate")
        .expect("commit-tree");
    manager
        .compare_and_swap_ref(
            &mut hooks,
            RefSite::CompareAndSwapIntegration,
            integration,
            &proposal,
            &fast_commit,
        )
        .expect("the fast publication is a CAS of the candidate commit itself");
    manager
        .remove_worktree(&mut hooks, &fast_task)
        .expect("remove");
    manager
        .remove_intent(&mut hooks, &fast_task)
        .expect("intent");
    shared.lock().expect("harness").end_fast_sequence();

    manager
        .delete_ref_expected_old(
            &mut hooks,
            RefSite::DeleteCandidatesRef,
            candidates,
            &candidate,
        )
        .expect("Ref.DeleteCandidatesRef");
    manager
        .remove_execution_root(&mut hooks)
        .expect("Worktree.RemoveExecutionRoot");

    // The bijection, over the enums rather than over a list.
    let harness = shared.lock().expect("harness");
    let sites = lane_sites();
    assert_eq!(
        sites.len(),
        WorktreeSite::ALL.len()
            + SnapshotSite::ALL.len()
            + RefSite::ALL.len()
            + ObjectSite::ALL.len(),
        "the lane's site count comes from the frozen enums"
    );
    assert_eq!(sites.len(), 29, "eleven + four + eight + six");
    let mut missing: Vec<String> = Vec::new();
    for site in &sites {
        for phase in HookPhase::PHASES {
            if !harness.observed(*site, *phase) {
                missing.push(format!("{site} {phase}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "every site of this lane executes both hook phases; unobserved: {missing:?}"
    );

    // The no-execution record, per sequence and not per process.
    let sequence = harness
        .fast_sequence("exact-base-fast")
        .expect("the suite exercised a fast sequence");
    for absent in [
        EffectSiteId::Worktree(WorktreeSite::AddStaging),
        EffectSiteId::Object(ObjectSite::ProposalCherryPick),
        EffectSiteId::Ref(RefSite::PinPrepared),
    ] {
        assert!(
            !sequence.ran(absent),
            "`{absent}` must not execute for a fast sequence"
        );
    }
    assert!(
        !sequence.touched().is_empty(),
        "and the absence has to be proved inside a sequence that really ran"
    );
    assert!(
        harness.touched(EffectSiteId::Object(ObjectSite::ProposalCherryPick)),
        "…while the suite as a whole did exercise the stale path, so the absence is a \
             statement about the trace and not about the process"
    );
}
