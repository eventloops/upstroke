# 2026-08-25 — review of PR #32, `CODING_STANDARDS.md`, at `f3ffb2d`

**Reviewed head:** `f3ffb2d9d7ca37eb0f5024f5bb6afdb18bb8e58b`
(`codex/rust-coding-standards`, up to date with `origin/master`; four files:
`CODING_STANDARDS.md` +442, plus pointer edits to `CLAUDE.md`, `CONTRIBUTING.md`,
`README.md`). A new push invalidates this review.

**Verdict: adopt after revisions.** The document is sound, project-specific in the
right places, and much of it gives citable text to rules the ledger already paid
for. Four findings block the merge — each is the document violating its own §1
reconciliation rule or recreating a class `reviews/FINDINGS.md` names — and five
more are additions it needs before the G2 sweep can be run against it. Everything
below was verified against the tree, not assumed; commands are quoted per finding.

## Blocking — the document breaks its own rules or a ledger rule

**1. §8 outlaws `DESIGN.md:222` and the conflict is unnamed.** §8: *"Represent
paths with `Path`, `PathBuf`, `OsStr`, or `OsString`. Do not construct paths by
string concatenation or assume UTF-8."* `DESIGN.md:222` freezes
`struct CommandSpec { program: String, … }`, and the resulting refusal of
non-Unicode agent paths is an open owner question
(`PR4-PROGRAM-PATH-NOT-UNICODE`). §1's own rule: *"the conflicting documents must
be reconciled in the same change."* Landing the standard silent on this is the
standard's first violation of itself. **Fix:** a short "Known conflicts at
adoption" block naming the conflict, the owner question, and the venue (the G2
pass, W4). This is also a second independent argument for the `OsString`
resolution when the owner rules.

**2. The authority re-scope is half-done.** The PR edits `CLAUDE.md` to
*"`DESIGN.md` is the only living authority **for product design**"* — correct —
but `decisions/README.md` and `proposals/README.md` both still say *"DESIGN.md
remains the only living authority"* unqualified. Same-change reconciliation, per
§1. **Fix:** the same one-clause qualification in both READMEs, in this PR.

**3. §13 recommends doctests that CI never runs.** *"Examples SHOULD compile as
doctests"* — but CI's test legs run `cargo test --all-targets --all-features`,
and `--all-targets` **excludes doctests** (verified: no `--doc` invocation
anywhere in `.github/workflows/ci.yml`). The ledger already paid for exactly
this: `PR5-C-DOCTEST-FIXTURES-NEVER-RAN`, class *"the thing that was supposed to
prove it never ran"* — three `compile_fail` fixtures for a contract-named
refusal, executed by nothing, green everywhere. **Fix (either):** add
`cargo test --doc --all-features` to CI in this PR, or rewrite the sentence to
the ledger's own rule: doctests are documentation; executable evidence lives in
a run target that a named CI command executes.

**4. §11 makes Miri/sanitizers a MUST with no gate behind it.** Item 6 of the
unsafe checklist requires *"Miri or sanitizers where they can exercise the code
meaningfully"* — CI has no Miri or sanitizer leg, so this is a MUST nothing
runs: the ledger's *"an enforcement artifact no gate validates"* class (two
occurrences, both green while binding nothing). **Fix:** soften to SHOULD with
the trigger named (new unsafe code that Miri can reach), or add the leg in the
same change.

## Needed before the sweep relies on it

**5. §2's baseline is smaller than CI.** The lint job also runs the three bash
gates (`bash .github/scripts/test-*.sh`, from the repository root — the
from-root requirement is itself a documented trap). List them beside the four
cargo commands, or cite `CLAUDE.md` §Gates as the authoritative command list.

**6. §2 never names the repository's own denylist.** `clippy.toml` carries
`disallowed-methods` / `disallowed-types` / `disallowed-macros` — rustc-HIR
resolved, and load-bearing: it is the effect-funnel *enforcement*, not style.
Its failure mode is already measured: a denial whose path does not resolve on
the compiling platform enforces nothing and warns un-escalated
(`PR5D-UNRESOLVED-DENIAL-IS-A-WARNING`, `PR5-MACOS-CLIPPY-NEVER-RUN`). The
standard should state the two rules that keep it honest: every entry must
resolve on a platform CI compiles, and the resolution census with its injected
control is the guard.

**7. §3 rule 8 needs its counterweight.** *"Duplication is often cheaper than a
false unification"* is true and stays — but this project's dominant measured
defect class is duplication-drift (four instances in PR7 alone; the largest
root-cause bucket across slices). Add the ledger's mechanical guard beside it:
**for every clause of the design, count the implementations — two is a finding
regardless of whether both are right**, and where one authority is chosen, a
census pins it (the one-join-site/one-mint pattern).

**8. The instrument classes are missing, and they are this document's remit.**
The preamble promises project-specific rules born of measured failure modes;
the most-paid-for ones are absent:
- **Censuses and blankers** (five-occurrence class): strip comments *and
  strings* before counting and assert the strip removed something; prefer
  structure to substrings; a counting census over unblanked text lets deleted
  prose buy a real call; every census needs a positive control.
- **`#[cfg(test)]` placement**: test-only items sit below every production item
  in a file; a mid-file test item has silently truncated classification domains
  twice (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`, `PR7-WRAPPERS-EMPTY-DOMAIN`).
- **Insertion between a doc comment (or attribute) and its item**: this class
  detached rustdoc twice in one file this slice and once produced a duplicated
  `#[test]` attribute that only the Windows leg caught. Rule: after inserting an
  item, verify the neighbours' docs and attributes still attach.
- **Fixture independence**: derive a fixture's field list from the type, vary
  every independently meaningful field, assert hostility as distinct-value
  counts — the correlated-fixture class stands at 11+11+2 occurrences.
- **Test names as claims**: the house style (a test name is the sentence it
  proves) is nowhere written down and should be.

**9. No enforcement mapping.** Mark each rule *enforced-by* (rustfmt / a named
clippy lint / `clippy.toml` / a bash gate) or *review-only* — the sweep needs to
know which findings a gate will hold and which need a reviewer. Ratchet rule:
a lint turns on in the same commit that makes the tree clean under it.

## Notes, non-blocking

- **§10's async rules are currently unexercised** — the source is synchronous;
  every `async`/`tokio` mention in `src/` is a comment, and two of them say the
  Tokio scheduler arrives later in v0.2. The rules are timely, not dormant;
  checklist items should read N/A until that lands rather than implying present
  compliance.
- **SemVer bullet**: the crate is unpublished; the "supported external contract"
  qualifier already carries the right scope — fine as written.
- **No rustfmt config file exists**, so "rustfmt is the formatting authority"
  means *default* rustfmt; adding a config later is a change to this standard.

## What the document already does right, named so the sweep can link rows to it

§12's unique-tempdir/RAII rule is the citable passage
`PR7-SCRATCH-FIXTURE-LEAK` was waiting for (the shared `scratch` helper's
delete-at-creation pattern is now non-compliant by document, not by taste).
§5's wildcard-arm rule matches the `no_site_enums_row_mapping_has_a_wildcard_arm`
census. §10's check-then-act ban and §8's staging-path/atomic-primitive rules
match the publication-atomicity work in PR5. §14's "do not call it a sandbox"
matches the host-runner honesty rows. §12's "not an implementation copied into
the test as its own oracle" is the self-oracle rule the fold's order-axis defect
survived without. The authority split in §1 (design vs implementation vs
lifecycle) is clean and the pointer-file edits implement it correctly — finding
2 is the only remainder.

## Verification record

- `git rev-parse origin/codex/rust-coding-standards` → `f3ffb2d…`;
  `merge-base --is-ancestor origin/master` → up to date.
- `Select-String ci.yml 'cargo test|--doc'` → one form, `--all-targets`, no
  doctest leg.
- `clippy.toml` present, `disallowed-methods` confirmed; no `rustfmt.toml` /
  `.rustfmt.toml`.
- `async fn|\.await|tokio` over `src/` → five matches, all comments.

---

## 2026-08-25 — re-review of the revision, at `6772b2a`

**Reviewed head:** `6772b2a7970a8baa4624befb7daf71a6c48fcc5c`
(`docs: reconcile coding standards review`; revision delta is three files,
`CODING_STANDARDS.md` +112/−12 and the two READMEs — the pointer files are
unchanged since `f3ffb2d`). CI and `upstroke-pr-policy` green on this head.

**Verdict: ADOPT. All nine findings are addressed faithfully; no new findings.**

Per finding: (1) the "Known conflicts at adoption" block names `DESIGN.md:222`,
`PR4-PROGRAM-PATH-NOT-UNICODE`, and the W4/G2 venue, and correctly rules that
the frozen design governs the field until the owner does — recorded, not
precedent. (2) Both READMEs now carry the "for product design" qualification.
(3) §13 rewritten to the ledger's rule — doctests are documentation; executable
evidence lives in a run target a named CI command executes — after a local
probe found **zero doctests** (`cargo test --doc` → 0 passed, 0 failed), which
makes "no CI leg" the correct branch of the brief's fork. (4) §11's Miri/
sanitizer item is now a triggered SHOULD with the honest boundary stated
("a triggered review requirement rather than an automated compliance claim").
(5) §2 lists all eight baseline commands; parity with CI verified — master's
lint job runs exactly those four `test-*.sh` gates. (6) The denylist rules are
stated **conditionally** and activate "in the same change that introduces"
a `clippy.toml` — the right treatment, see the corrections below. (7) §3.8
carries the count-the-implementations counterweight and the pinning census.
(8) The "Instruments and censuses" section covers all six classes and sharpens
two of them (position/length-preserving blankers; "a positive control inside a
truncated domain does not prove the whole named domain was scanned").
(9) Appendix A maps every rule area to a mechanism or review-only, with the
ratchet extended to widened scopes and newly compiled platforms, and the
HIR-resolution caveat for platform legs. Section numbering is stable; the
checklist gained the instrument and mapping items.

**Two errors in the original brief, found by the implementer and confirmed
here:** the brief's "facts already verified" were checked in a `slice/pr7`
working tree, not against `master` — (a) `clippy.toml` exists only on the
parallelism branch, not on `master` or this PR's branch, so the brief asked the
document to describe a file its tree does not have (the conditional-activation
treatment adopted is the correct resolution); (b) `master` runs **four** bash
gates, not three — the "three gates, from the repository root" claim came from
the parallelism branch's stale `CLAUDE.md`, while master's already says four
and notes most gates self-locate. Verification against the wrong tree is
exactly the class this ledger warns reviewers about; recorded here against the
reviewer's interest.

**Non-blocking notes carried forward:** §2's uniform "from the repository
root" is a conservative superset of master's current gate behaviour — fine.
The next routine merge of `master` into `codex/parallelism-design` will hit a
`CLAUDE.md` reconcile (the branch still says "Three bash gates"; master's
wording plus this PR's authority edits win).
