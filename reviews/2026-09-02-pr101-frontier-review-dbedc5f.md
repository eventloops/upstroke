# PR #101 — one source of truth for the whole-file test-module domain: frontier review record

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED on both passes, recorded as written.** Every finding of both passes is repaired in the head this record accompanies. The owner cut the third pass and authorized the merge on 2026-09-02 |
| **Reviewed SHAs** | pass 1 `5be58bc455483dc3fcff508a267fa10c3d938e99`, pass 2 `dbedc5f8f10e5e0dbd780fb2e685b3f1e4cbda11` |
| **Merged SHA** | `28e6814aa0c4ac01bc833b06d45812dc89face5c` — the pass-2 repairs, unreviewed by a third pass at the owner's direction |
| Pull request | eventloops/upstroke#101, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 101`, 90-minute per-pass limit |
| Findings | seven across two passes: four then three, each round narrower and disjoint from the last |
| CI at each reviewed SHA | green, `upstroke-pr-policy` included; the eight-command baseline ran on the build box at every head |

## Why this pull request exists

It is bookkeeping infrastructure for W1, and it exists because of a review
finding on another pull request. PR #97 extracted one inline test module into a
sibling file, which added one whole-file test module to the crate. Its review
observed that `WHOLE_FILE_TEST_MODULES = 18` stayed green — it is compared with
`>=` — while about twenty comments across eight files went on saying "eighteen"
and "fourteen" in English. A passing floor is not a true document, and
`CODING_STANDARDS.md` §1 requires conflicting documents to be reconciled in the
same change as the code.

That instruction to leave the constant alone was the coordinator's, not the
implementor's, and it was wrong. This pull request is the repair, and it is
separate from the five extraction packets because those packets would otherwise
each have had to edit the same twenty prose sites in the same eight shared
files, colliding with one another at every merge.

## What it does

One sorted collection of paths replaces both counts. Both populations are read
off it — the whole domain, and the subset a literal `#[cfg(test)] mod tests;`
declares — and the four modules not named `tests.rs` fall out as the
complement. About twenty comments stop stating a number.

A list rather than a count, and that is the argument the reviewer tested
hardest: a derivation that swapped one module for another at the same
cardinality satisfies every assertion a number can carry, and fails against a
list naming the file it gained and the file it lost. The four odd modules were
already named individually for exactly that reason; this applies the same
reasoning to the whole population.

## The verdicts, verbatim

Each blockquoted byte-for-byte from the reviewer's saved output on the review
box (`~/review-pr101-<head7>.md`).

### Pass 1 — `5be58bc455483dc3fcff508a267fa10c3d938e99`

> 1. **P1 — path identities violate the mandatory native-path rule.** [`WHOLE_FILE_TEST_MODULES`](/srv/worktrees/w1-p0-count/src/effects/tests/cfg.rs:1210) is `&[&str]`, while [`source_oracles.rs`](/srv/worktrees/w1-p0-count/src/effects/tests/source_oracles.rs:573) converts native paths with `to_string_lossy().replace('\\', "/")` and uses those strings for equality. This is identity logic, not diagnostics. `CODING_STANDARDS.md` §8 requires `Path`/`PathBuf`/`OsStr`/`OsString` and expressly forbids lossy strings for identity; the standard applies to tests. Distinct non-UTF-8 paths can collapse, and on Unix replacing a literal backslash can conflate a filename component with a separator.
>
> 2. **P1 — the promised exhaustive guard has a shared blind spot.** `whole_file_test_modules` calls `declared_whole_file_test_modules` directly at [`effects.rs:1349`](/srv/worktrees/w1-p0-count/src/effects.rs:1349); the second alleged derivation calls that same function at [`source_oracles.rs:652`](/srv/worktrees/w1-p0-count/src/effects/tests/source_oracles.rs:652); and `named` merely filters the first result at line 609. These are not three independent tree derivations.
>
>    Concrete bypass:
>
>    1. Add `#[cfg_attr(all(), cfg(test))] mod hidden_tests;` and its file.
>    2. Rust applies `cfg(test)`, making the file wholly test-only.
>    3. The scanner ignores every `cfg_attr` not containing `path` at [`effects.rs:2149`](/srv/worktrees/w1-p0-count/src/effects.rs:2149), so it classifies this declaration as unconditional and omits it.
>    4. Leave the list unchanged. Every new set assertion still sees the old population and passes.
>    5. A fixture call in that file remains in production censuses. It can mask deletion of a real production call—the exact failure these skip sets claim to prevent.
>
>    The reported list mutations establish assertion reachability, not derivation independence. Similarly, the “literal `#[cfg(test)] mod tests;`” assertion only checks the module name and inline ancestry, not literal syntax or guard.
>
> 3. **P2 — “every whole-file test module” is already false on this tree.** [`effects.rs:1889`](/srv/worktrees/w1-p0-count/src/effects.rs:1889) explicitly says `effects/tests/policy.rs` is compiled only under `cfg(test)` but deliberately excluded because the derivation does no transitive closure. That contradicts the PR Summary and [`cfg.rs:1165`](/srv/worktrees/w1-p0-count/src/effects/tests/cfg.rs:1165), which describe the list as every such file. If this is intentionally only the non-transitive census domain, it must be named and documented that way.
>
> 4. **P2 — the single-source claims contradict the exact diff.** [`cfg.rs:1182`](/srv/worktrees/w1-p0-count/src/effects/tests/cfg.rs:1182) says neither number is written anywhere, while [`recover/tests.rs:7727`](/srv/worktrees/w1-p0-count/src/engine/topology/recover/tests.rs:7727) retains `>= 18`. `pr.md` likewise says both “every count … is derived” and “No integer literal … survives,” then discloses that literal. Disclosure does not make the absolute claims true. The newly added comment also says “about twenty … across eight files,” while the PR’s own measurement says 37 across ten.
>
> The exact diff hash and requested SHA matched, the worktree was clean, and the registered oracle test passed. I found no added production `unwrap`/`expect`, `anyhow`, or undisclosed production logic.
>
> VERDICT: CHANGES_REQUIRED

### Pass 2 — `dbedc5f8f10e5e0dbd780fb2e685b3f1e4cbda11`

> Changes are required at exact head `dbedc5f8f`; the supplied diff hash matches the repository object.
>
> 1. P1 — The new source of truth violates the path-type rule. `src/effects/tests/cfg.rs:1227` stores filesystem paths as `&[&str]`. These are identities, not diagnostics, so converting them to `Path` later does not satisfy the requirement to represent paths with `std::path` types. Use a typed path collection/accessor, such as a test-only `LazyLock<Vec<PathBuf>>`.
>
> 2. P1 — The claimed literal-declaration oracle ignores the declaration’s guard. At `source_oracles.rs:721-729`, `literal` filters only `inline_path.is_empty()` and `name == "tests"`, despite `guard` being available. Concrete failure:
>
>    - Change `#[cfg(test)] mod tests;` to `#[cfg(all(test, unix))] mod tests;`.
>    - `entails_test` still classifies it as test-only.
>    - Its path and file stem are unchanged, so every new comparison passes.
>    - Linux validation still runs it, while Windows silently loses the entire test module.
>
>    Therefore the assertion message and PR claim that literal `#[cfg(test)] mod tests;` declarations are pinned are stronger than the code.
>
> 3. P2 — Sortedness, which the merge-conflict rationale depends on, is not enforced. `expected` immediately sorts `WHOLE_FILE_TEST_MODULES`, erasing its written order. Swapping two current entries—or appending a new valid module at the end—leaves every set comparison and length floor green while the authoritative list is no longer sorted. An assertion must validate the original list order before normalization.
>
> The added `.expect` is test-only, no `anyhow` or decision-record mutation was introduced, and the visibility widening is disclosed and remains behind `cfg(test)`.
>
> VERDICT: CHANGES_REQUIRED

## Disposition

Every finding of both passes is repaired in the merged head. By pass:

**Pass 1 (`5be58bc`), four findings.** Path identity went through
`to_string_lossy().replace('\\', "/")`, which `CODING_STANDARDS.md` §8 forbids
for identity and which collapses distinct non-UTF-8 paths. The claim that three
independent derivations agreed was false: `whole_file_test_modules` calls
`declared_whole_file_test_modules`, and the `named` set filters the first one's
result, so there is one resolver, one view over it and a filename rule. "Every
whole-file test module" was already false, because the derivation does no
transitive closure and `effects/tests/policy.rs` is deliberately outside it. And
three statements contradicted the diff, including a surviving `>= 18`.

The first two were the coordinator's instructions rather than the implementor's
work: the list-of-strings shape was specified without thinking about path
identity on a first-class Windows target, and the independence argument was
explicitly asked for. Both are recorded that way here because a review record
that assigns an error to the wrong party is worth less than no record.

**Pass 2 (`dbedc5f`), three findings.** Storage as `&[&str]` still typed the
source of truth as strings; it is now a `LazyLock<Vec<PathBuf>>`, with the
literals kept as the written form because that is how a human writes a path in
source. Sortedness, which the merge-conflict argument depends on, was asserted
nowhere; it is now checked inside the initializer — on the literals, so what is
checked is the text a reader diffs — with `>=` so a duplicated entry fails too.

The third is the one that mattered. The literal-`#[cfg(test)] mod tests;`
membership rule read the module's name and inline ancestry and never its guard,
though the guard was in the same struct. So narrowing a declaration to
`#[cfg(all(test, unix))]` left the path, the stem and every comparison
unchanged while Windows silently lost that entire test module and Linux
validation stayed green — on a repository whose first-class target is Windows,
and inside the census family whose whole purpose is to notice a domain quietly
shrinking. Membership now requires the guard to *be* `test`, through a helper
shared with a fixture that drives the reviewer's own reproduction over
synthetic input. A narrowed declaration is still in the domain list and is no
longer in the literal subset, and the assertion message now says what such a
failure means.

## The third pass, cut

Two passes ran. The owner cut the third on 2026-09-02, after the pass-2 repairs
were verified by the coordinator and the eight-command baseline passed at the
merged head, and authorized the merge. So the head that landed carries repairs
no frontier pass has read.

That is a deliberate, recorded departure from `MAINTAINING.md` step 5, and it
is stated here rather than implied. What it rests on: the pass-2 findings were
narrower than pass 1's and disjoint from them; each round's repairs were
verified against the tree by the coordinator rather than accepted on the
implementor's report; and this pull request changes test and census machinery
only, with no production code, no workflow, no gate script and no validator in
its diff.

## What this record does not claim

The domain is every file a **direct** test-only `mod …;` declaration names.
There is no transitive closure, so a file declared from inside another
whole-file test module is outside it — `effects/tests/policy.rs` is the live
example, named in the list's own doc comment.

Pass 2 also surfaced a declaration form the shared resolver cannot see at all:
`#[cfg_attr(all(), cfg(test))] mod hidden_tests;` is applied by rustc as
`cfg(test)`, but the scanner treats a `cfg_attr` as significant only when it
contains `path`, so such a file is classified unconditional and omitted from
every census that reads this domain. That gap predates this change, is recorded
as a stated limit in the list's doc comment, and is not repaired here: widening
the scanner changes what every census scans and is its own change with its own
review.
