# S5 closing compliance sweep — `d17bcf2` and `4247255`

Mechanical, no lenses. Rule: **every prose claim in these two commits is test-borne or
carries the sha it was measured at.** Fix by stamping or moving; nothing else.

## What was extracted and how

    git log -1 --format=%B <sha>                          # the message
    git show <sha> --format= --unified=0 | grep '^+'      # the prose it adds

then every `[0-9a-f]{7,40}` token, every `file.(rs|md):[0-9]+` citation, and every numeric
claim, checked one at a time.

## Hex tokens — 24 distinct

| kind | count | verdict |
|---|---|---|
| git commits, ancestors of HEAD | 12 | resolve, reachable |
| git commit, **not** an ancestor | 1 | `e1e6841`, and it is the *subject* of a finding rather than a citation — stamped with where it was observed and with the note that an unreachable object may be collected |
| sha256 blob hashes | 11 | **all 11 re-derive** |

    d17bcf2's six staged-blob hashes, against d17bcf2        6/6 MATCH
    4247255's FINDINGS.md hash, against 4247255              1/1 MATCH
    the four documented-mismatch hashes, against their refs  4/4 MATCH

## `file:line` citations — 5 distinct

| citation | claim | before | after |
|---|---|---|---|
| `census/tests.rs:4618` | the "refused instead of converging" panic site | true at HEAD, unstamped | stamped `4247255`, and the **item named** beside the line |
| `recover.rs:802` | where `BarrierHeld::fold`'s doc sat | historical, unstamped | stamped `80a141b` |
| `run.rs:637` | where the `#[must_use]` warning fired | historical, unstamped | stamped `bb68cf6` |
| `run.rs:615` (×2) | the anchor that pointed into its own correction | historical, unstamped | stamped `9b6fef1`, both places |
| `recover/tests.rs:5488` (×3) | the anchor nineteen lines moved | one unstamped in `run/tests.rs`; two inside §19 | stamped `c01a844`; the §19 pair carry §19's section-level *"Re-verified at `cca1276`"* |

## Numeric claims

| claim | disposition |
|---|---|
| **seventeen** whole-file test modules, three not called `tests.rs` | **test-borne** — `the_declared_whole_file_test_modules_are_seventeen_and_three_are_not_called_tests`, which names all three rather than counting |
| **ten** compound assignment operators | **test-borne** — `each_census_needle_covers_the_domain_its_doc_states` drives every one |
| **eleven** distinct defects | the table enumerating them **is** the evidence |
| **50 / 112 / 95** and the per-lens splits | counted from five lens reports that live outside the repository → now named **unverifiable by construction**, the disposition §21 already gives round 5's |
| **nine** citations of `e1e6841` | stamped `8e48dd1` |
| **five** call sites of `whole_file_test_modules` | stamped `d17bcf2` |
| **11** occurrences of the doc-re-targeting class | already *derived at `51cfc01`* — the fix that prompted this rule |
| `RACING_ACCESS_ATTEMPTS = 64`, **2 of 4** full-suite runs, **0 of 3** isolated | stamped *"Measured at `d17bcf2`"*, and the constant is cited by name |
| gate blocks (`1704 + 8 passed, 0 failed`, `rc=0`) | the commit that carries them **is** the stamp |

## Result

**Nine stampings, no moves, no repairs.** Nothing in either commit needed a claim retracted
or a test written: every claim was either already test-borne, already carried a sha, or
was true-and-unstamped and now carries one.

Gates at the swept head: `cargo fmt --check` clean · `clippy -D warnings` rc=0 ·
`cargo test --all-targets --all-features` **1704 + 8 passed, 0 failed, 32 ignored** ·
`cargo +1.85.0 check --locked` rc=0 · all three bash gates PASS.
