# PR #104 — refactor: inline the plan fixture corpus and retire fixtures/: frontier review record

| field | value |
|---|---|
| **Binding verdict** | **CHANGES_REQUIRED**, at the head this pull request merges |
| **Reviewed SHA** | `ae59f2df9be438e73072e0eac115e2ff101fb98c` |
| Passes | 8 , each recorded below as it was returned |
| Pull request | eventloops/upstroke#104, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 104` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

This change went round 8 times. Every pass is recorded, in order, with
its verdict as returned — a completed `CHANGES_REQUIRED` pass is not
folded into the pass that followed it.

## Pass 1 — `ca630af` — CHANGES_REQUIRED

Reviewed SHA `ca630afb747da54ae03483fd1c34a7aa04dba026`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr104-ca630af.md` on the review box).

> ## Findings
>
> 1. **The new corpus scratch directory violates the test-ownership rules.** `src/validate.rs:426-437` uses the predictable path `upstroke-validate-corpus-<pid>`, accepts an existing directory through `create_dir_all`, stores only a `PathBuf`, and never cleans it up. `OnceLock` serializes threads within one process; it provides neither filesystem exclusivity nor RAII cleanup. This violates `CODING_STANDARDS.md:495-498` and its §8 exclusivity requirements. The allowlist permits the calls for lint purposes; it does not waive those requirements.
>
>    Concrete failure: a previous run leaves the directory; after PID reuse, `create_dir_all` accepts it. If `sample-plan.md` is now a directory, the suite panics; if it is a symlink, `fs::write` follows it and truncates the target. Two PID namespaces sharing a temp mount can also use the same PID concurrently, allowing one process to read while the other rewrites. The review host already contains four such directories belonging to dead PIDs, proving the ordinary success path leaks them.
>
> 2. **The package claim is false.** `pr.md:254-255` says “The four plans stop shipping to crates.io.” They remain verbatim in `src/plan/mod.rs:76-148`, and Cargo packages source text without removing `#[cfg(test)]` items. Publishing this head therefore still ships all four plans; only the four standalone `fixtures/*.md` paths disappear. `cargo package --list` supports the narrower path-removal claim, not absence of the plans or their bytes.
>
> 3. **The suite is not an independent byte-identity check as claimed at `pr.md:297-305`.** No cited test pins an expected fixture hash. For example, replace the U+2014 in `STEPS_PLAN`’s pre-list prose with `-`: `Plan.source.hash` changes, but the parsed task list does not. The validate rendering does not expose the hash, and `the_registry_round_trips_the_frozen_plan_byte_for_byte` clones `plan.source` into the comparison value (`src/topology/registry.rs:3102-3107`), so the changed hash appears identically on both sides. Every cited test remains green. The external SHA-256 comparison is valid—I independently reproduced all four matches—but it is the sole byte-identity oracle, not one of two independent checks.
>
> 4. **The rollback instruction targets a nonexistent single-commit change.** `pr.md:23-25` correctly says the PR has two commits, while `pr.md:318-321` says to revert “the single commit.” Reverting only `ca630af` restores a comment saying fixtures stay while they remain deleted. Reverting only `1bde191` restores the files and readers but not the complete base state. Exact rollback requires reverting both commits, normally newest first, or staging the two reverts into one rollback commit.
>
> 5. **The newly added corpus rustdoc contains factual errors.** `src/plan/mod.rs:56-67` says there were two test regions, although this diff changes three: markdown, validate, and topology. It also claims copies could drift, but at the base all three readers opened the same four repository files; there were no duplicate text corpora. Finally, removing a fixture’s final newline changes its source hash but does not remove its last task: `split_sections` extends the final section to `raw.len()`, and the Markdown parser closes a heading/list item at EOF. The statement that the newline distinguishes “a last task and none” is therefore false.
>
> I confirmed that the supplied patch exactly matches `1cbdccd..ca630af`, all four literal hashes match the deleted blobs, all fifteen stated call sites were repointed, production Rust regions are unchanged, and the diff introduces no production `unwrap`/`expect`, `anyhow`, non-`std::path` path handling, or decision-record edits.
>
> ## Out of scope
>
> - `src/validate.rs:367-407` and the pre-existing scratch setups beginning at lines 570, 620, 668, 714, 770, 783, 807, 835, and 848 already use PID-derived temporary directories without RAII cleanup; `scratch_root` additionally pre-deletes a predictable path. Those statements existed at `1cbdccd` and this change neither introduces nor activates them. Their failure sequence is the same stale-path/PID-reuse leak and collision described above; the host currently contains hundreds of leaked directories for several of those patterns. They do not drive this verdict—the newly introduced `corpus()` instance does.
>
> VERDICT: CHANGES_REQUIRED
## Pass 2 — `4506028` — CHANGES_REQUIRED

Reviewed SHA `450602838009c9e2877e532b65dc58bbe6f86efa`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr104-4506028.md` on the review box).

> ## Findings
>
> 1. **P2 — The cleanup repair is not fully RAII and can still leak directories.** In [`Corpus::new`](/srv/worktrees/w1-e-fixtures/src/validate.rs:451), the directory is created and four fallible writes occur before `Self { dir }` constructs the guard. Concrete sequence: the temp filesystem fills after one write; a later `fs::write` panics; no `Corpus` exists to drop; the partial directory remains. Separately, [`Drop`](/srv/worktrees/w1-e-fixtures/src/validate.rs:475) discards every removal failure, including on the normal, non-panicking path. On Windows, a process holding a plan file without delete sharing makes `remove_dir_all` fail; the test still reports success and leaves the tree behind. Repetition can exhaust temp-space inodes.
>
>    This contradicts the body’s claims that the directory “goes away,” that the repair “reclaims it,” and that the prior finding is fixed. The successful `/tmp` measurement exercises neither failure path. It also violates [`CODING_STANDARDS.md` §6–§7](/srv/worktrees/w1-e-fixtures/CODING_STANDARDS.md:287): cleanup must survive setup errors and unwinding, while best-effort errors require observability and a failure-path test. The body explicitly admits no regression test was added, contrary to [§12](/srv/worktrees/w1-e-fixtures/CODING_STANDARDS.md:481).
>
> 2. **P2 — The new helper introduces an uncontrolled wall-clock dependency.** [`SystemTime::now().duration_since(UNIX_EPOCH).expect(...)`](/srv/worktrees/w1-e-fixtures/src/validate.rs:453) makes nine tests depend on ambient time. If a VM or host clock is set before 1970, every affected test panics before testing validation behavior. This contradicts the ledger’s assertion that the guard is deterministic and violates §12’s requirement to inject or control clocks. The diff provides no reason the existing identifier/scratch funnel cannot serve this use.
>
> 3. **P3 — The body and new rustdoc overstate `Plan.source.hash`.** [`content_hash`](/srv/worktrees/w1-e-fixtures/src/ir.rs:448) deliberately ignores every carriage-return byte. Thus changing `"# Plan\n"` to `"# Plan\r\n"` changes bytes while preserving `Plan.source.hash`; the existing CRLF test asserts exactly that. Consequently, “Every byte is significant,” “a plan’s text is its identity,” and “a shifted byte changes `Plan.source.hash`” are false as written. This pre-existing normalization is in scope because the diff introduces the contradictory rustdoc and the PR body introduces the stronger risk claim. The external SHA-256 evidence still establishes that the four literals at this head are byte-identical.
>
> 4. **P3 — Nothing prevents the promised second-copy scenario.** [`src/plan/mod.rs:60`](/srv/worktrees/w1-e-fixtures/src/plan/mod.rs:60) and `pr.md:34` claim centralization means “a second copy cannot appear later.” The diff adds no census, visibility constraint, or other enforcement. A contributor can add another literal in any test module, have it compile, then let the copies drift without a failure. The accurate claim is that current unit-test consumers now have one shared source, not that duplication is impossible.
>
> I independently confirmed that the supplied diff exactly matches `1cbdccd..450602838009c9e2877e532b65dc58bbe6f86efa`, all four embedded literals match their deleted blobs by SHA-256, all fifteen stated sites were repointed, and no production `unwrap`/`expect`, `anyhow`, non-`std::path` path handling, or DESIGN.md conflict was introduced.
>
> ## Out of scope
>
> - The PID-only scratch paths already present in [`src/validate.rs:370`](/srv/worktrees/w1-e-fixtures/src/validate.rs:370) and the other existing scratch setups can adopt stale directories, follow stale symlinks, or pre-delete paths without proving ownership. For example, PID reuse can make `create_dir_all` accept a stale root containing `upstroke.toml`, changing a test’s configuration. Those operations are byte-identical before and after this change, and the new corpus helper does not activate them; the PR body also records them as C-006. They therefore do not affect this verdict.
>
> VERDICT: CHANGES_REQUIRED
## Pass 3 — `dbdce08` — CHANGES_REQUIRED

Reviewed SHA `dbdce0893f105ba83db5abd4a556b459f1df566f`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr104-dbdce08.md` on the review box).

> ## Findings
>
> 1. **P2 — The new corpus directory allocator is not hermetic across runs.** `src/validate.rs:466-473` derives the name solely from PID, thread ID, and a process-local counter; all reset after process exit. Sequence: a killed run leaves its first corpus directory, the PID is later reused—or two PID namespaces share `/tmp`—and the next single-threaded run generates the same name. `create_dir` returns `AlreadyExists`, and `.expect("corpus directory")` fails before validation. I reproduced this at the exact head by pre-creating the predicted path; `sample_plan_renders_expected_table` exited 101 with `AlreadyExists`. This violates `CODING_STANDARDS.md` §12’s deterministic, unique-temporary-directory requirement, whose sufficiency Appendix A marks review-only, and contradicts `pr.md:591`’s claim that “no path is predictable.” Exclusivity should remain, but `AlreadyExists` must select another candidate rather than fail.
>
> 2. **P3 — Exact-head evidence in the PR body is stale.** `pr.md:387-398` claims head lines 163..EOF hash to `96dcfe…`, the first `#[cfg(test)]` is line 74, and the corpus rustdoc is 20 lines. At `dbdce089…`, the first attribute is line 83 and the original tests begin at line 172. Lines 163..EOF hash to `6f774f…`; the claimed hash is obtained from the correct range, 172..EOF. The underlying splice is indeed pure, but the stated verification is false after `a082d3b` added nine rustdoc lines. The ledger is also stale at `pr.md:595`: it says `Plan.source.hash` supports the every-byte claim, directly contradicting the corrected `PR104-R2-003` row at line 599.
>
> 3. **P3 — The downstream package-risk claim is unsupported.** `pr.md:564-566` says no downstream consumer can have read the packaged fixture paths because they were test inputs. `#[cfg(test)]` controls Rust compilation; it does not restrict access to files in a `.crate` archive or `cargo vendor` output. Concrete sequence: a packaging or test script reads `vendor/upstroke-*/fixtures/sample-plan.md`; after updating to the first release containing this change, that path disappears. Removing the unsupported asset may be intentional, but the body cannot claim such consumers are impossible.
>
> I independently confirmed the exact diff, all four byte hashes, all 15 repointed sites, the four-entry package-list delta, unchanged production Rust prefixes, and the full suite result of 1809 library plus 8 binary tests passing.
>
> ## Out of scope
>
> - `src/validate.rs:369-407`: the pre-existing hermetic-root, pools-file, and `scratch_root` helpers use PID-based paths without RAII; `scratch_root` also pre-deletes a path it did not exclusively create. A stale or same-PID-namespaced directory can be adopted or deleted. Their naming and cleanup behavior is unchanged and not activated by this PR.
> - `src/engine/topology/prelock/tests.rs:201-206`: the existing `Scratch` precedent has the same cross-run PID/thread-name collision class. It is byte-identical across this change and is not called by it; copying that weakness into the new `Corpus` is covered by finding 1.
>
> VERDICT: CHANGES_REQUIRED
## Pass 4 — `5d9a1eb` — CHANGES_REQUIRED

Reviewed SHA `5d9a1eb023b3e61daa15c40b229e12ff68aad005`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr104-5d9a1eb.md` on the review box).

> ## Findings
>
> 1. **The collision witness recreates the exact collision before exercising the fix.** [`src/validate.rs:732`](/srv/worktrees/w1-e-fixtures/src/validate.rs:732) directly creates the predictable `taken` directory and only removes it at line 763, without RAII. If a prior run dies after creation, the next reused PID/thread/index gets `AlreadyExists` at line 733 and never reaches `Corpus::of`. I reproduced this at the exact head in a PID namespace: pre-creating `/tmp/upstroke-validate-raii-taken-2-ThreadId(2)-0` made the new test fail at line 733. Thus PR104-R3-001’s claimed regression guard fails under the scenario it claims to cover and violates the unique-temporary-directory/RAII rule.
>
> 2. **`Corpus::drop` misclassifies both absence and genuine cleanup failure.** [`src/validate.rs:561`](/srv/worktrees/w1-e-fixtures/src/validate.rs:561) accepts only `Ok`, so `NotFound`—meaning the directory is already absent—panics on an otherwise successful exit. The test at line 780 deliberately removes the tree and passes by catching that false failure; it does not reproduce an unreclaimed directory or the claimed Windows held-handle case. Conversely, if `remove_dir_all` genuinely fails while another panic is unwinding, `std::thread::panicking()` makes the assertion succeed silently, leaving the tree with no report. Sequence: a plan file remains open on Windows, the test assertion panics, removal returns `PermissionDenied`, and `Drop` silently leaks the corpus. This conflicts with the project’s cleanup and best-effort observability requirements.
>
> 3. **The cleanup witnesses can report absence after discarding filesystem errors.** [`residue`](/srv/worktrees/w1-e-fixtures/src/validate.rs:586) uses `entry.ok()` and silently drops failed directory entries; the negative checks at lines 624 and 652 use `Path::exists`, which also collapses stat errors into `false`. A leaked corpus omitted by a `read_dir` error therefore makes `residue(tag).is_empty()` pass. The earlier live control does not protect later enumerations from errors. This directly violates the rule against discarding errors through `.ok()` and weakens the PR’s claimed cleanup evidence.
>
> 4. **The PR body still contains stale exact-head evidence.** [`pr.md:704`](/tmp/tmp.lvJBY1DbPP/pr.md:704) says the head uses an `AtomicUsize` and attributes that naming to `prelock::Scratch`. The head actually uses a thread-local `Cell<usize>` at [`src/validate.rs:430`](/srv/worktrees/w1-e-fixtures/src/validate.rs:430), while `prelock::Scratch` has no counter. This contradicts the body’s own pass-3 explanation that the atomic was replaced.
>
> I confirmed that the supplied patch exactly matches `1cbdccd..5d9a1eb`, all four embedded literals match the deleted blobs byte-for-byte, all fifteen consumers were repointed, and no production panic/`anyhow`/path or DESIGN.md violation was introduced.
>
> ## Out of scope
>
> - The pre-existing PID-named helpers at [`src/validate.rs:369`](/srv/worktrees/w1-e-fixtures/src/validate.rs:369) and [`src/validate.rs:404`](/srv/worktrees/w1-e-fixtures/src/validate.rs:404) still leak or pre-delete predictable trees. They are byte-identical across this change and not activated by it. A reused PID can make `scratch_root` delete another run’s tree or adopt residue after its discarded removal fails.
> - [`prelock::Scratch::drop`](/srv/worktrees/w1-e-fixtures/src/engine/topology/prelock/tests.rs:216) has the same `NotFound`/silent-unwind behavior, but this diff neither changes nor calls that implementation. Copying its pattern into the new `Corpus` is in scope; repairing the old instance is not.
>
> VERDICT: CHANGES_REQUIRED
## Pass 5 — `7ff163f` — CHANGES_REQUIRED

Reviewed SHA `7ff163fd51a19fe1fef00331b8058f62fc1a96a8`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr104-7ff163f.md` on the review box).

> ## Findings
>
> 1. **The setup-cleanup witness is not hermetic across PID reuse.** [`residue`](/srv/worktrees/w1-e-fixtures/src/validate.rs:602) matches every temp entry sharing only the tag and PID, despite the allocator explicitly acknowledging that PIDs repeat across runs and namespaces.
>
>    I reproduced this at the exact head: before releasing the test process, I created its exact first candidate, `...-ThreadId(2)-0`. `Corpus::of` correctly stepped to index 1, but the assertion at [`src/validate.rs:849`](/srv/worktrees/w1-e-fixtures/src/validate.rs:849) failed because `residue` returned both the foreign index-0 directory and the current index-1 directory. A killed run followed by PID reuse produces the same sequence. Thus the new regression test turns the collision behavior it claims to tolerate into a false red suite and contradicts its claim that the PID excludes other processes. This violates the deterministic/hermetic-test requirement in [CODING_STANDARDS.md §12](https://github.com/eventloops/upstroke/blob/7ff163fd51a19fe1fef00331b8058f62fc1a96a8/CODING_STANDARDS.md#L479-L508).
>
> 2. **The unwind-reporting witness never observes the report.** [`reclaim`](/srv/worktrees/w1-e-fixtures/src/validate.rs:581) currently writes cleanup failures to stderr, but [`a_reclamation_that_fails_during_an_unwind_is_reported_without_a_second_panic`](/srv/worktrees/w1-e-fixtures/src/validate.rs:1053) checks only that the primary panic survives and that the directory remains for `Unwritable` to clean later.
>
>    Replace the panicking branch with silence while retaining the ordinary-exit panic: the cleanup error is discarded, yet this test still receives `PRIMARY`, sees the directory, restores permissions, removes it, and passes. Therefore the test and [`pr.md:481`](/tmp/tmp.ThxoZtSgFf/pr.md:481) do not support the “reported” claim or satisfy the required failure-observability guard. The repository’s existing reporter explicitly explains why an external child-process stderr oracle is necessary and implements one at [`src/rundir/scratch_tree.rs:370`](/srv/worktrees/w1-e-fixtures/src/rundir/scratch_tree.rs:370).
>
> The four embedded literals do match the deleted blobs byte-for-byte, all fifteen consumers are repointed, and I found no production panic/`anyhow`/path or DESIGN.md violation.
>
> ## Out of scope
>
> - [`src/validate.rs:369`](/srv/worktrees/w1-e-fixtures/src/validate.rs:369), [`src/validate.rs:405`](/srv/worktrees/w1-e-fixtures/src/validate.rs:405), and the PID-named scratch setups beginning at lines 1227, 1277, 1326, 1373, 1433, 1446, 1471, 1500, and 1514 still adopt or pre-delete predictable paths without RAII. Those operations are pre-existing and not activated by the fixture repointing. A killed run followed by PID reuse can supply stale configuration, while two PID namespaces sharing `/tmp` can make `scratch_root` delete another live run’s tree.
>
> - [`prelock::Scratch`](/srv/worktrees/w1-e-fixtures/src/engine/topology/prelock/tests.rs:193) still uses reproducible PID/thread names and suppresses genuine cleanup failures during unwinding. It is unchanged and not called by this diff. A killed run followed by PID/thread reuse can make its exclusive creation fail, and an unwind-time removal failure remains silent.
>
> VERDICT: CHANGES_REQUIRED
## Pass 6 — `c5c19db` — CHANGES_REQUIRED

Reviewed SHA `c5c19db299dddb4e6d784325f3d9a015b9034609`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr104-c5c19db.md` on the review box).

> ## Findings
>
> 1. **The new child-process oracle violates the native-path rule and fails on legal Unix paths.** At `src/validate.rs:1208-1212`, `current_exe()` returns a `PathBuf`, but `exe.display().to_string()` converts it lossily and uses the result as executable identity. I reproduced this by placing the exact-head test binary beneath a path containing byte `0xFF` and supplying an ASCII `argv[0]`. The test failed at line 1229 trying to spawn the nonexistent mangled path `bin-�/upstroke-tests`. This is introduced by `c5c19db` and directly violates the requirement that paths remain `std::path` values.
>
> 2. **The replacement for the cross-process false-red still cannot distinguish directory identity.** `CREATED` records pathnames permanently (`src/validate.rs:605-617`), and `residue` later stats whichever object currently occupies each name (`:620-643`). Sequence: process A creates and records candidate `P`, drops it, and removes `P`; before A calls `residue`, another same-PID namespace process creates its own directory at reproducible name `P`; A now reports that foreign directory as its residue. Thus both the rustdoc at line 630 and `pr.md:555` overclaim that the registry “cannot see another process at all,” and the false-red scenario remains under a different interleaving.
>
> 3. **The exact-head review attestation is unsupported and internally contradictory.** `pr.md:832-844` records only four passes ending at `5d9a1eb`, while the body itself describes a fifth pass and the requested head is `c5c19db`. The delta from the recorded SHA adds 635 lines and removes 58, including the defective spawn above. No owner diff-read is disclosed, despite the checked owner-verification assertion at `pr.md:860`. Worse, `reviews/2026-09-03-pr104-frontier-review-5d9a1eb.md:5` says its `5d9a1eb` verdict binds “at the head this pull request merges,” although the merge head moved twice afterward. The supplied evidence therefore does not support the checked final-head claim.
>
> ## Out of scope
>
> - `src/engine/topology/prelock/tests.rs:631-634` already performs the same lossy `current_exe()` conversion. It is byte-identical across this diff and is not called by the new test; it fails under the same non-UTF-8 executable-path sequence.
> - `src/validate.rs:370-408` retains predictable PID-derived scratch directories, stale-directory adoption, and unowned pre-deletion. Those operations are pre-existing and not activated by the signature change; PID reuse or shared PID namespaces can still make one run adopt or delete another run’s tree.
>
> VERDICT: CHANGES_REQUIRED
## Pass 7 — `77174ce` — CHANGES_REQUIRED

Reviewed SHA `77174ce777f3c63be126cf78c3c2fb4e5149effc`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr104-77174ce.md` on the review box).

> ## Findings
>
> 1. **P2 — The PR activates the known unsafe scratch-directory defect at four new sites.** `src/validate.rs:723,746,756,795` newly route tests through `scratch_root`, which derives a predictable PID-only path, discards `remove_dir_all` errors, deletes without proving ownership, uses `create_dir_all`, and has no RAII cleanup (`src/validate.rs:403-407`).
>
>    This is not out of scope merely because the helper predates the PR: those four tests previously read `fixtures/*.md` directly. Commit `77174ce` is what makes the defective helper affect them.
>
>    I reproduced the harm with the exact-head test binary. I pre-created `$TMPDIR/upstroke-validate-sample-<test-pid>/foreign-sentinel`, then ran `sample_plan_renders_expected_table`. The test passed, but:
>
>    ```
>    sentinel=deleted
>    replacement_plan=present
>    ```
>
>    Thus the new test path silently deleted foreign content and leaked its replacement directory. If deletion instead fails, `create_dir_all` can adopt the existing directory and `write_plan` at lines 421-424 can follow a planted `sample-plan.md` symlink.
>
>    This violates `CODING_STANDARDS.md` §1, §6, §7, §8, and §12: existing code is explicitly not precedent; temporary directories require unique ownership and RAII; cleanup errors cannot be discarded; and deletion requires proven ownership. The body itself admits at `pr.md:675-686` that the four new roots leak and pre-delete paths they do not own, but incorrectly declares that in-scope activation “pre-existing.” The six earlier passes ended at `c5c19db`; none reviewed these call sites introduced by `77174ce`. Consequently, the ledger’s claim that the original corpus-directory defect is fixed because its code was deleted is also too strong—the final commit recreates the same failure class through another helper.
>
> 2. **P3 — The review evidence is internally stale and contradictory.** The newly added `reviews/2026-09-03-pr104-frontier-review-5d9a1eb.md:5-7` calls `5d9a1eb` “the head this pull request merges,” although the supplied head is `77174ce`. The body admits this record is stale and lacks passes 5–6 (`pr.md:935-939`), while elsewhere it simultaneously says the owner answered the escalation and created `77174ce` (`pr.md:623-633`) and that the PR remains parked pending that owner decision (`pr.md:921-925`). It also labels the reviewer history “four passes” at `pr.md:892-893` while describing six passes.
>
>    The unchecked exact-head box is appropriately honest; the contradictory affirmative statements and tracked “binding” record are not. An auditor following the durable record would find a verdict bound to an earlier head, before the 896-line final replacement that introduced finding 1.
>
> I independently confirmed that the supplied diff exactly matches `1cbdccd..77174ce`, all four constants match their deleted files byte-for-byte, the fifteen consumers were repointed, production Rust prefixes are unchanged, and no production panic/`anyhow`/path or DESIGN/decision-record violation was introduced.
>
> ## Out of scope
>
> - `src/validate.rs:367-407` and the pre-existing callers at lines 434, 453, and 490 already used PID-derived roots without RAII, including unowned pre-deletion. The other existing scratch setups around lines 557, 607, 656, 703, 766, 779, 805, 834, and 846 likewise already leaked or adopted predictable roots and already wrote files there. A stale same-PID path can therefore be deleted or adopted, but those original lifecycle defects were active before this PR. They do not drive the verdict; only the four newly added `scratch_root` calls do.
>
> - `src/engine/topology/prelock/tests.rs:199-228` derives `Scratch` names from PID and thread ID without collision retry. A killed run followed by PID/thread-ID reuse can leave `create_private_dir` failing with `AlreadyExists`; cleanup failure during unwinding is also suppressed. This code is unchanged and uncalled by the diff.
>
> VERDICT: CHANGES_REQUIRED
## Pass 8 — `ae59f2d` — CHANGES_REQUIRED

Reviewed SHA `ae59f2df9be438e73072e0eac115e2ff101fb98c`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr104-ae59f2d.md` on the review box).

> ## Findings
>
> 1. **P2 — New plan writes can silently overwrite files outside their scratch roots.** `src/validate.rs:422-424` uses `fs::write` in five pre-existing PID-named roots (`:656`, `:713`, `:753`, `:882`, `:903`) that are adopted with `create_dir_all`. This is in scope: the roots predate the PR, but writing `sample-plan.md` into them does not.
>
>    Concrete reproduction at the exact head: I pre-created `upstroke-validate-effort-<pid>/sample-plan.md` as a symlink to a victim file, then ran `the_preview_echoes_resolved_role_tier_pin_and_disabled_review_effort`. The test passed, while the victim changed from `DO NOT OVERWRITE\n` to the 646-byte sample plan. At the base, that test read `fixtures/sample-plan.md` and never touched this symlink. Thus the diff activates the weakness. The body’s classification of these six read sites as entirely pre-existing C-006 debt (`pr.md:52-57`, `76-87`, `963-967`) is wrong.
>
> 2. **P2 — `PlanDir` is exclusive but not unique or hermetic across executions.** `src/validate.rs:442-450` derives its candidate solely from tag, PID, and a process-local counter. PID and counter reset, so residue from a killed run or a shared PID namespace can reproduce the name. `create_dir` then fails instead of selecting another candidate.
>
>    I pre-created the exact first candidate with a sentinel and ran `sample_plan_renders_expected_table` from the exact-head binary. It exited 101 at line 449 with `AlreadyExists`; the sentinel survived. Loud failure prevents adoption, but it does not satisfy the requirement for unique, hermetic temporary directories. This recreates the failure class already recorded as `PR104-R3-001` and contradicts `pr.md:780`’s claim that none of the five defects remains.
>
> 3. **P2 — The new cleanup contract has no committed regression tests.** `PlanDir` claims ordinary cleanup, unwind cleanup, and reporting of cleanup failures, but the body explicitly says the witness suite was not rebuilt (`pr.md:752-755`). The four consumer tests assert validation output only; none observes that the directory disappeared or drives either failure arm. A mutation making `Drop` remove a nonexistent sibling would leave every added consumer test green while leaking every plan directory. Manual scratch mutations in the PR body do not satisfy `CODING_STANDARDS.md` §12’s requirement that new behavior and bug fixes include failure-path tests.
>
> 4. **P3 — The review evidence is stale and internally inconsistent.** `reviews/2026-09-03-pr104-frontier-review-5d9a1eb.md:5-7` calls `5d9a1eb` “the head this pull request merges,” although the supplied head is `ae59f2d`. The body’s current evidence names `c5c19db`, four commits behind the requested head (`pr.md:983`), says “four passes” despite describing seven, and promises a final record carrying six. It also says two boxes are unchecked while three are. The body admits the record is stale; that admission does not make the exact-head evidence complete.
>
> I confirmed that `pr.diff` exactly matches `1cbdccd..ae59f2d`, all four embedded literals match the deleted blobs’ SHA-256 values, all fifteen consumers were repointed, production Rust regions are unchanged, and the diff introduces no production panic/`anyhow`, non-`std::path` handling, DESIGN.md change, or decision-record edit.
>
> ## Out of scope
>
> - `src/validate.rs:369-409` and the unchanged scratch setups already use predictable PID-derived paths, adopt existing directories, leak them, and—in `scratch_root`—pre-delete without proving ownership. For example, PID reuse can make an unchanged `scratch_root` caller delete a foreign sentinel tree. These behaviors existed and were exercised before this PR. They do not drive the verdict; only the newly added `sample-plan.md` writes through those roots are finding 1.
>
> VERDICT: CHANGES_REQUIRED
