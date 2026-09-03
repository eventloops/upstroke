# PR #104 — refactor: inline the plan fixture corpus and retire fixtures/: frontier review record

| field | value |
|---|---|
| **Binding verdict** | **CHANGES_REQUIRED**, at the head this pull request merges |
| **Reviewed SHA** | `5d9a1eb023b3e61daa15c40b229e12ff68aad005` |
| Passes | 4 , each recorded below as it was returned |
| Pull request | eventloops/upstroke#104, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 104` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

This change went round 4 times. Every pass is recorded, in order, with
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
