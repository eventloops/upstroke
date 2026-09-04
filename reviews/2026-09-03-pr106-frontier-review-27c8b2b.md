# PR #106 — refactor: split topology::effects into per-concern child modules: frontier review record

| field | value |
|---|---|
| **Binding verdict** | **CHANGES_REQUIRED** at `27c8b2b`. **This record does NOT bind to the head this pull request merges.** A `CHANGES_REQUIRED` pass is followed by a repair, so the current head is later than the reviewed SHA below; see the delta note for what changed and whether a further pass exists |
| **Reviewed SHA** | `27c8b2bcdbd262a7bdde4054d111a589185074f1` |
| Passes | 6 , each recorded below as it was returned |
| Pull request | eventloops/upstroke#106, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 106` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

This change went round 6 times. Every pass is recorded, in order, with
its verdict as returned — a completed `CHANGES_REQUIRED` pass is not
folded into the pass that followed it.

**The final pass recorded here returned `CHANGES_REQUIRED`, so no reviewer has
seen the tree at the current head.** The reviewed SHA is `27c8b2b`; any
commit after it carries the repair for the findings below and has not
itself been reviewed. This record is evidence of what the reviewer said,
not an attestation of the merge head.

## Pass 1 — `2277f47` — CHANGES_REQUIRED

Reviewed SHA `2277f479dbb1aef0b1740f4b18cedddfbd37475e`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr106-2277f47.md` on the review box).

> Reviewed the exact clean head `2277f479dbb1aef0b1740f4b18cedddfbd37475e`; the supplied diff matches the repository diff.
>
> ## Findings
>
> 1. **The widened oracle does not scan the Rust module domain it claims to scan.**
>
>    [source_oracles.rs:126](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:126) enumerates immediate `*.rs` directory entries and excludes only the literal stem `tests`. That produces both false negatives and false positives:
>
>    - False negative: add a production `mod twelfth;` implemented using the standard `src/topology/effects/twelfth/mod.rs` layout, with a wildcard `row()` mapping. The directory entry is a directory, so the filter ignores it. The existing seven flat children and twelve mappings keep both lower bounds green, and the wildcard is missed. The repository’s own module resolver explicitly recognizes both `name.rs` and `name/mod.rs` at [cfg.rs:640](/srv/worktrees/w2-m2/src/effects/tests/cfg.rs:640).
>    - False positive: declare `#[cfg(test)] mod row_cases;` and put a wildcard-bearing test helper in `row_cases.rs`. The `cfg` is on the declaration, not inside the child file, so `production_region` cannot remove it. Because its stem is not exactly `tests`, the production census scans and rejects test-only code.
>
>    Thus the implementation neither includes every production child nor excludes every non-production file. The planted mutation in `vocab.rs` proves only that one already-included flat file is scanned; changing the extension filter to match nothing proves only that the count floor fires. Neither measures domain membership. This contradicts the claims at [pr.md:258](/tmp/tmp.LhO6geMvLN/pr.md:258) and silently widens the scanner to undeclared/test files.
>
>    The file set should be derived from the root’s module declarations, including their `cfg`, and resolve both supported out-of-line layouts.
>
> 2. **The “pure move” breaks eleven intra-doc links.**
>
>    Running `upstroke-build cargo rustdoc --lib -- --crate-version review-2277f47` reports newly unresolved links in the moved public documentation:
>
>    - `FaultRegistry::insert` at [harness.rs:24](/srv/worktrees/w2-m2/src/topology/effects/harness.rs:24)
>    - `BeforeState` at [registry.rs:131](/srv/worktrees/w2-m2/src/topology/effects/registry.rs:131)
>    - Six links involving `EffectSiteId` or `ExpectedResidue` starting at [residue_authority.rs:229](/srv/worktrees/w2-m2/src/topology/effects/residue_authority.rs:229)
>    - `Platform::required_on` at [sites.rs:1421](/srv/worktrees/w2-m2/src/topology/effects/sites.rs:1421)
>    - `EventSite` and `EffectSiteId::observable_orders` at [vocab.rs:335](/srv/worktrees/w2-m2/src/topology/effects/vocab.rs:335)
>
>    These names resolved when all items occupied one module. After the split, they are unimported sibling/root items in their defining modules. The generated API documentation consequently loses those links. Byte-identical documentation is not semantically identical when its name-resolution scope changes, so “Nothing else changes” is unsupported. Use crate-qualified public paths and add a documentation build to validation.
>
> 3. **Several PR-body claims are factually incorrect.**
>
>    - [pr.md:22](/tmp/tmp.LhO6geMvLN/pr.md:22) says 15 `BijectionFailure` directions, but the enum has 16 variants, ending with `InvalidEntry` at [bijection.rs:162](/srv/worktrees/w2-m2/src/topology/effects/bijection.rs:162).
>    - [pr.md:134](/tmp/tmp.LhO6geMvLN/pr.md:134) says five `pub use` statements; there are seven at [effects.rs:65](/srv/worktrees/w2-m2/src/topology/effects.rs:65).
>    - [pr.md:130](/tmp/tmp.LhO6geMvLN/pr.md:130) says twelve retained regions and twelve separators. The listed ranges form eleven retained chunks separated by ten omitted blank lines. The byte hashes themselves check out.
>    - [pr.md:370](/tmp/tmp.LhO6geMvLN/pr.md:370) says rollback is reverting “the single commit,” but this exact change contains two commits. Reverting only head `2277f47` leaves the split in place and merely restores the narrower census. A future squash commit could be reverted singly, but none is identified here.
>
> No decision record was modified, no `anyhow` or non-`std::path` path handling was introduced, and no newly authored production `unwrap`/`expect` was found. The census’s undeclared/test-file inclusion does violate the prohibition on silently widening the change’s scope.
>
> ## Out of scope
>
> - [bijection.rs:372](/srv/worktrees/w2-m2/src/topology/effects/bijection.rs:372) contains a production `.expect`, contrary to the literal panic rule. It existed identically at base `effects.rs:5547` and this change only moved it; the split does not make `hook_phase()` return `None`. It would panic if `required` later admitted `Residue` or `NoExecution`, but no current path does so.
> - [source_oracles.rs:156](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:156) recognizes only `_ =>` and `_=>`. A legal `_ /* fallback */ =>` arm is blanked to multiple spaces and escapes detection while the count remains green. This weakness existed identically before the split and was neither introduced nor activated by it.
>
> VERDICT: CHANGES_REQUIRED
## Pass 2 — `c639ac9` — CHANGES_REQUIRED

Reviewed SHA `c639ac9926031b054ad868dc687571dab29fbacb`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr106-c639ac9.md` on the review box).

> ## Findings
>
> 1. **P1 — The new regression witness is not connected to the census it claims to guard.**  
>    [source_oracles.rs:277](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:277) tests declaration parsing, candidate construction, and `production_module_files` independently. Its only real `<name>/mod.rs` case calls `sole_present` directly for `lib.rs → topology/mod.rs`; the actual walker is exercised only on `topology::effects`, whose seven production children are all flat `.rs` files. Nothing proves that the census at [source_oracles.rs:228](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:228) continues to consume that walker.
>
>    Concrete failure sequence:
>
>    1. Restore the census’s previous root-plus-immediate-`*.rs` enumeration, while leaving `production_module_files` and its new witness intact.
>    2. Every test remains green at this head: the old enumeration and the new walker both return the root plus the same seven flat children.
>    3. Add the body’s `twelfth/mod.rs` production module and add it to the witness’s explicit expected-domain list, as that test requires.
>    4. The witness passes because its independent walker sees `twelfth/mod.rs`; the actual census still skips the directory. Its eight-file and twelve-mapping floors remain green, so the wildcard is undetected.
>
>    The repository already warns against precisely this disconnected proof at [effects.rs:1394](/srv/worktrees/w2-m2/src/effects.rs:1394): testing what a domain helper returns says nothing about whether a census calls it. The manually planted scenario supports the implementation currently present, but the PR’s stronger claim that the added test is a regression guard is unsupported.
>
> 2. **P2 — The claim that documentation degradation is ruled out is false.** Rustdoc’s broken-link count checks name resolution, not whether prose and governance records still describe the tree.
>
>    Concrete stale statements activated by this split include:
>
>    - [effects/allowlist.toml:142](/srv/worktrees/w2-m2/effects/allowlist.toml:142) says the exact `src/topology/effects.rs` row contains “The site enums, their const accessors and `effect_sites()`”; those now live in `sites.rs` and `export.rs`. No new allowance is needed, but this path-specific review record is now factually stale, contradicting the claim that the mutex file needed no edit.
>    - [source_oracles.rs:31](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:31) says `cargo test -- --list` is unchanged, while this diff adds the wrapper at [tests.rs:2893](/srv/worktrees/w2-m2/src/effects/tests.rs:2893), and the PR body itself reports the one-entry increase.
>    - [source_oracles.rs:216](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:216) still says `effects.rs` is frozen and that this slice does not edit it, immediately before explaining the split that edits it.
>    - Other live present-tense location claims likewise still place `ContainerSite` and containment-point definitions in the old file, for example [container.rs:6](/srv/worktrees/w2-m2/src/runner/container.rs:6) and [host/tests.rs:2949](/srv/worktrees/w2-m2/src/runner/host/tests.rs:2949).
>
>    These statements were accurate, or at least not contradicted by this layout, before the split; the diff activates the inconsistency. Dated decision records should remain immutable, but current source documentation and governance artifacts need reconciliation.
>
> I found no undisclosed production-body edit: reversing the eleven documented link-target substitutions restores the moved bodies, and the private child modules plus explicit re-exports preserve the 59 public root names. I also found no newly introduced `anyhow` use, non-`std::path` path handling, or new production panic.
>
> ## Out of scope
>
> - [bijection.rs:372](/srv/worktrees/w2-m2/src/topology/effects/bijection.rs:372) contains a production `.expect`, contrary to the stated panic rule. It is byte-identical to base `effects.rs:5547`; the split merely relocates it and does not make `hook_phase()` return `None`. The concrete latent failure is extending `required` to admit `Residue` or `NoExecution`, after which `hook_phase()` returns `None` and the bijection check panics. Because this defect existed identically before and is not activated here, it does not affect the verdict.
>
> - [source_oracles.rs:249](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:249) recognizes only `_ =>` and `_=>`. Writing `_ /* fallback */ =>` causes comment blanking to leave spaces between `_` and `=>`, so the wildcard survives while `offenders` remains empty and the mapping floor stays green. This matcher is unchanged from the base; only its file domain changed, so the blind spot is not introduced or activated by this PR.
>
> VERDICT: CHANGES_REQUIRED
## Pass 3 — `eecf2fb` — CHANGES_REQUIRED

Reviewed SHA `eecf2fb62d46870376ccb32e39ae0aa7cc930e13`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr106-eecf2fb.md` on the review box).

> Changes are required. I verified the clean worktree is at the requested SHA and that `pr.diff` exactly matches `git diff 1cbdccd..eecf2fb` (SHA-256 `e6b9eace…93599`).
>
> ## In-scope findings
>
> 1. **P1 — The pass-2 witness still does not prove the census consumes the walker.**
>
>    The witness merely counts one lexical occurrence of `production_module_files(` and bans the literal `read_dir` ([source_oracles.rs:442](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:442)); its counter performs no name-resolution or data-flow analysis ([effects.rs:1275](/srv/worktrees/w2-m2/src/effects.rs:1275)).
>
>    Concrete failure sequence:
>
>    1. Keep `let _walker_domain = production_module_files(&root);`.
>    2. Populate `sources` independently from a hard-coded list of the current eight files.
>    3. The witness still sees exactly one call and no `read_dir`; parts 1–3 continue testing the unused walker.
>    4. Add declared `twelfth/mod.rs`, update the witness’s expected domain, and put a wildcard `row()` there.
>    5. The witness sees the ninth file, but the census scans its stale eight-file list. Its count floor remains green and the wildcard is undetected.
>
>    Thus the PR’s claim that the witness “fails when the census stops consuming” the walker ([pr.md:42](/tmp/tmp.xfZGTG8ZTp/pr.md:42)) is unsupported. The current census does consume it; the claimed regression protection does not enforce that fact.
>
> 2. **P2 — The new production walk activates a known `cfg_attr` classification defect.**
>
>    The new code treats every declaration for which `test_only` is false as production ([source_oracles.rs:149](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:149)). However, the reused scanner explicitly ignores `cfg_attr` that applies `cfg(test)` ([effects.rs:1915](/srv/worktrees/w2-m2/src/effects.rs:1915)).
>
>    Concrete failure sequence:
>
>    1. Declare `#[cfg_attr(all(), cfg(test))] mod row_cases;`.
>    2. Put a test-only helper containing a wildcard `fn row(...)` in `row_cases.rs`.
>    3. Rustc applies `cfg(test)`, so the child is never production.
>    4. The scanner reads it as unconditional; the new walker includes it.
>    5. The production-row census rejects legitimate test-only code.
>
>    Before this diff, that census read only `src/topology/effects.rs`, so this scanner weakness could not affect it. The diff newly activates it; under the owner’s scope rule, it is in scope. The body acknowledges the exact failure but incorrectly defers it as merely pre-existing ([pr.md:695](/tmp/tmp.xfZGTG8ZTp/pr.md:695)). It also makes “derived from declarations, with their `cfg`” stronger than the implementation supports.
>
> 3. **P2 — The PR body does not accurately describe the exact head.**
>
>    - It says the only touched files outside `src/topology/effects*` are the two effects-test files ([pr.md:55](/tmp/tmp.xfZGTG8ZTp/pr.md:55)); the diff additionally changes one engine file and three runner files.
>    - It asserts the runner-file count is zero ([pr.md:153](/tmp/tmp.xfZGTG8ZTp/pr.md:153)); the exact diff count is three. Those edits are prose-only, but the factual validation claim is still false.
>    - It says both runs at “this head” used `c639ac9`, calling that the head being pushed ([pr.md:236](/tmp/tmp.xfZGTG8ZTp/pr.md:236)), despite the reviewed head and earlier passing transcript being `eecf2fb`.
>    - It says no mutex-file edit was needed, while later admitting the split made the current allowlist review clause false. That clause still says the root file contains the enums and `effect_sites()` ([allowlist.toml:142](/srv/worktrees/w2-m2/effects/allowlist.toml:142)). This staleness was introduced by the split; needing mutex coordination does not make it out of scope.
>
> I found no introduced `anyhow`, non-`std::path` path handling, or new production `unwrap`/`expect` in materially changed code. The private child declarations and explicit re-exports otherwise preserve the visible module API.
>
> ## Out of scope
>
> - [bijection.rs:372](/srv/worktrees/w2-m2/src/topology/effects/bijection.rs:372) contains a production `.expect`, contrary to the no-panics rule. It is byte-identical to base `effects.rs:5547`, and this diff does not change the phases entering it. Future sequence: admit `Residue` or `NoExecution` into `required`; `hook_phase()` returns `None`; the checker panics rather than returning a failure. Pre-existing and not activated here.
>
> - [source_oracles.rs:298](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:298) recognizes only `_ =>` and `_=>`; `_ /* fallback */ =>` survives as spaced tokens after comment blanking and escapes detection. The same detector existed at base and no moved mapping uses that form, so the split does not activate it.
>
> - [run.rs:642](/srv/worktrees/w2-m2/src/engine/topology/run.rs:642) says its command finds a helper in `topology/effects.rs`, while the result is in `topology/effects/tests.rs`. The line is unchanged from the base and was already stale before this split.
>
> VERDICT: CHANGES_REQUIRED
## Pass 4 — `6d8cdda` — CHANGES_REQUIRED

Reviewed SHA `6d8cddab5f5ec5d5eef6cad48d54e107d33230c3`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr106-6d8cdda.md` on the review box).

> 1. **P1 — the census-consumption repair remains bypassable.** The witness in [source_oracles.rs](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:604) inspects only the census function body and rejects six literal reader names. A concrete surviving mutation is:
>
>    1. Add a sibling helper `scan_current_rows()` that manually reads/scans the current eight files.
>    2. Keep `ProductionModule::walk(...)` and `module.files()` in the census, but obtain `(scanned, offenders)` from that helper.
>    3. The extracted census body contains none of the six banned strings, while the positive control still finds `fs` in the witness itself.
>    4. Add declared child `effects/twelfth/mod.rs`, update the witness’s expected domain, and place a wildcard `row()` there.
>    5. The walker and domain witness see all nine files. The helper still scans eight, satisfying `scanned >= 8`, and reports no offender.
>
>    Both tests remain green while the wildcard is missed. An import alias such as `use std::fs as io` provides another bypass. Consequently, the claim in [pr.md](/tmp/tmp.yrnsS0Rl7b/pr.md:132) that part (5) catches the remaining bypass is false, and `PR106-CENSUS-CONSUMPTION` is not fixed.
>
> 2. **P2 — “0 narrowed” visibility is false.** At base, private `check_evidence` was defined in `topology::effects`, making it accessible to that module’s descendants. It is now private inside sibling child `topology::effects::bijection` at [bijection.rs](/srv/worktrees/w2-m2/src/topology/effects/bijection.rs:406), without `pub(super)` or a re-export. Parent and sibling modules can no longer call it. The public 59-name surface appears preserved, but the stronger claim in [pr.md](/tmp/tmp.yrnsS0Rl7b/pr.md:40) that no top-level item was narrowed is unsupported.
>
> 3. **P3 — the PR body miscounts its own ledger.** [pr.md](/tmp/tmp.yrnsS0Rl7b/pr.md:15) and again at line 288 say there are four repaired ledger rows, but the table contains five rows.
>
> I confirmed the supplied diff matches the requested head and found no introduced production `unwrap`/`expect`, `anyhow`, non-`std::path` path handling, or contradiction from an edited `DESIGN.md`; the disclosed production `expect` is moved from base. The surviving P1 is sufficient to require changes.
>
> VERDICT: CHANGES_REQUIRED
## Pass 5 — `6dc5987` — CHANGES_REQUIRED

Reviewed SHA `6dc5987ff926dc226d3c9802b1f744cee2465750`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr106-6dc5987.md` on the review box).

> I verified the clean checkout is at `6dc5987ff926dc226d3c9802b1f744cee2465750` and its base-to-head diff matches `pr.diff` byte-for-byte.
>
> ## Findings
>
> 1. **The claimed run-time domain proof remains forgeable.**
>
>    [`RowMappingScan`](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:401) and both fields are `pub(super)`, so a sibling helper can construct arbitrary `scanned` and `offenders` values. The equality at [source_oracles.rs:529](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:529) therefore compares the walk against self-reported metadata, not necessarily the files actually scanned. This also conflicts with the rule to keep invariant-preserving representations private at [CODING_STANDARDS.md:229](/srv/worktrees/w2-m2/CODING_STANDARDS.md:229).
>
>    Concrete bypass:
>
>    1. Add a ninth declared child containing a wildcard `row()` and update the witness’s expected list, as the PR’s own mutation does.
>    2. Add sibling `scan_current_rows(&ProductionModule)` which scans the stale eight files.
>    3. Have it construct `RowMappingScan` with `scanned` populated from `module.files()`, recording zero mappings for the unscanned ninth path, and `offenders` from only the stale eight.
>    4. Use that helper in the census.
>
>    `scanned == walked` passes, the existing mappings satisfy the floor, and `offenders` is empty. Witness part (5) calls the honest method but never checks its offenders; part (6) sees no reader in the census body. Both row tests remain green while the wildcard is missed. This directly contradicts the body’s claim that a stale helper must fail “whatever any function body says” at [pr.md:223](/tmp/tmp.GtrmUItnNb/pr.md:223).
>
> 2. **The new production-module walk fails open on macro-generated module declarations.**
>
>    The reused scanner explicitly admits item macros can expand to modules, but silently skips an invocation whenever the invocation’s input tokens contain no module-shaped sequence ([effects.rs:2191](/srv/worktrees/w2-m2/src/effects.rs:2191)). Its tests require `modify!(x)` to yield zero declarations ([tests.rs:3795](/srv/worktrees/w2-m2/src/effects/tests.rs:3795)).
>
>    Concrete sequence:
>
>    1. Import a function-like item macro whose empty invocation expands to `mod twelfth;`.
>    2. Invoke it in `effects.rs` and put a wildcard site `row()` in `twelfth.rs`.
>    3. The compiler declares and compiles the child, but `scan_module_declarations` inspects only the empty invocation and returns no declaration.
>    4. The walk remains at eight paths, so the hard-coded witness and `scanned == walked` both pass; the new file is never scanned.
>
>    The scanner weakness predates this PR, but this diff activates it by making that scanner the authority for the row census’s production domain—exactly the in-scope activation class described by the owner.
>
> 3. **“Reach narrowed: exactly 2” omits eight private fields.**
>
>    The body’s parser counts `struct` items but not their fields. The split moves these private fields from the parent module into private sibling children:
>
>    - Five `HookHarness` fields and two `FastSequence` fields at [harness.rs:122](/srv/worktrees/w2-m2/src/topology/effects/harness.rs:122).
>    - `FaultRegistry::entries` at [registry.rs:468](/srv/worktrees/w2-m2/src/topology/effects/registry.rs:468).
>
>    At base, `topology::effects` and any descendant such as `topology::effects::tests` could access those parent-private fields. At head, the root and sibling tests cannot access fields private to `harness` or `registry`. A field access in `effects::tests` therefore compiles at base and fails with private-field errors at head.
>
>    This does not break the public API, but it is eight additional reach reductions under the same privacy reasoning the body applies to `check_evidence` and `record`. Thus the unqualified “exactly 2” claim at [pr.md:95](/tmp/tmp.GtrmUItnNb/pr.md:95) is unsupported.
>
> 4. **The moved-comment sweep missed another changed referent.**
>
>    `ReportSite` still says “see this module’s worker report” at [sites.rs:1283](/srv/worktrees/w2-m2/src/topology/effects/sites.rs:1283). At base, “this module” denoted `topology::effects`; after the move it denotes the private `sites` child. This is the same locality change the PR says it corrected for `DurableEvent`, so the claim that the moved self-referential prose was swept is incomplete.
>
> The explicit re-exports contain no glob, the diff adds no `anyhow`, new path operations use `Path`/`PathBuf`, and neither `DESIGN.md` nor any immutable decision record is modified.
>
> ## Out of scope
>
> - [`src/topology/effects/bijection.rs:372`](/srv/worktrees/w2-m2/src/topology/effects/bijection.rs:372) contains a production `.expect()`, prohibited by the project rule. It is byte-identical to base `src/topology/effects.rs:5547`, and this diff does not change the construction of the phases reaching it. A future phase for which `hook_phase()` returns `None` would panic, but no new reachable sequence is introduced here.
>
> - [`src/effects/tests/source_oracles.rs:364`](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:364) recognizes only `_ =>` and `_=>`. `_ /* fallback */ =>` becomes `_              =>` after comment blanking and is missed. After updating the other exhaustive tables, a new enum variant can therefore compile behind that wildcard while this census remains green. The identical two-needle logic existed at base `source_oracles.rs:127`; the split neither introduces nor activates this lexical gap.
>
> VERDICT: CHANGES_REQUIRED
## Pass 6 — `27c8b2b` — CHANGES_REQUIRED

Reviewed SHA `27c8b2bcdbd262a7bdde4054d111a589185074f1`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr106-27c8b2b.md` on the review box).

> ## Findings
>
> 1. **P1 — The production-domain census still fails open on a macro-hidden `#[path]` module.**  
>    [`scan_module_declarations`](/srv/worktrees/w2-m2/src/effects.rs:2252) discards a macro invocation whose invocation tokens are not module-shaped. The fallback reconciliation only searches the directory returned by `module_dir(root)` ([source_oracles.rs](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:454)).
>
>    Concrete sequence:
>
>    1. Define a parent macro expanding to `#[path = "effects_hidden.rs"] mod hidden;`.
>    2. Invoke it as `declare_hidden!();` in `effects.rs`; the invocation contains no visible `mod`.
>    3. Move `EventSite::row` into `src/topology/effects_hidden.rs` and implement its current behavior with `_ => ResourceRow::R21`. All existing `EventSite` variants already map to R21.
>    4. Rust compiles the production module, but the declaration scanner sees nothing. Reconciliation searches only `src/topology/effects/`, so it never encounters sibling `src/topology/effects_hidden.rs`.
>    5. The walked and scanned sets remain the expected eight files. Removing the visible mapping leaves eleven mappings, still above the `>= 8` floor. Both row-mapping tests remain green while the wildcard is missed.
>    6. A later `EventSite` variant consequently receives R21 without requiring an explicit arm.
>
>    This is the same pre-existing scanner weakness activated by this change’s new domain authority. The body and ledger overstate the repair by marking `PR106-MACRO-DECLARED-CHILD` fixed ([pr.md](/tmp/tmp.FJNsbais4G/pr.md:371)).
>
> 2. **P2 — The PR body still contradicts itself about reach reductions.**  
>    [pr.md:55](/tmp/tmp.FJNsbais4G/pr.md:55) claims “two reach reductions,” while line 104 and the table at lines 116–125 establish ten. The ledger then marks the previous “exactly 2” finding fixed. It is not fixed in the exact body under review.
>
> 3. **P2 — The claimed mechanical evidence is absent and its stated domain disagrees with its description.**  
>    [pr.md:39](/tmp/tmp.FJNsbais4G/pr.md:39) says every retained number carries a regenerating command, and line 91 says both visibility scripts are in the review record. The added [review record](/srv/worktrees/w2-m2/reviews/2026-09-03-pr106-frontier-review-6dc5987.md:1) contains neither script nor commands regenerating the 304/270/751/696/10 or 59-name measurements. Moreover, the body says the domain includes every `effects/*.rs`, including the pre-existing `tests.rs`, but later describes parsing only the root at base and root plus seven production children at head. Its reported `fn 5/5` count cannot include `tests.rs`, which alone contains dozens of functions.
>
> 4. **P3 — The newly added standing finding miscounts itself.**  
>    [reviews/FINDINGS.md:178](/srv/worktrees/w2-m2/reviews/FINDINGS.md:178) begins “Four instances,” enumerates `(1)` through `(6)`, and later says “Six instances.” This is a diff-introduced false governance record.
>
> I verified the checkout is exactly `27c8b2bcdbd262a7bdde4054d111a589185074f1`, the supplied diff matches `HEAD^2..HEAD` byte-for-byte, the 59 public top-level definitions match base, re-exports are named rather than globbed, and the prescribed row-mapping test command passes at the honest head. I found no newly introduced `anyhow`, non-`std::path` path handling, or production panic beyond the moved item below.
>
> ## Out of scope
>
> - [`src/topology/effects/bijection.rs:372`](/srv/worktrees/w2-m2/src/topology/effects/bijection.rs:372) contains a production `.expect()`, violating the literal panic rule. It is byte-identical to base `effects.rs:5547`; this diff only relocates it and does not make `required` contain `Residue` or `NoExecution`. If either entered that loop, `hook_phase()` would return `None` and panic. Pre-existing and not activated.
>
> - [`src/effects/tests/source_oracles.rs:392`](/srv/worktrees/w2-m2/src/effects/tests/source_oracles.rs:392) detects only `_ =>` and `_=>`. `_ /* fallback */ =>` becomes spaced tokens after comment blanking and escapes detection. The identical matcher existed at base, and no moved mapping activates the gap. This is distinct from finding 1, whose hidden file can contain the directly detectable `_ =>`.
>
> VERDICT: CHANGES_REQUIRED
