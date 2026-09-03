# PR #108 — refactor: split topology::fold into per-concern child modules: frontier review record

| field | value |
|---|---|
| **Binding verdict** | **CHANGES_REQUIRED** at `c42b3c1`. **This record does NOT bind to the head this pull request merges.** A `CHANGES_REQUIRED` pass is followed by a repair, so the current head is later than the reviewed SHA below; see the delta note for what changed and whether a further pass exists |
| **Reviewed SHA** | `c42b3c1505df4ab9e8a672100982d2ce2c477f5a` |
| Passes | 2 , each recorded below as it was returned |
| Pull request | eventloops/upstroke#108, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 108` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

This change went round 2 times. Every pass is recorded, in order, with
its verdict as returned — a completed `CHANGES_REQUIRED` pass is not
folded into the pass that followed it.

**The final pass recorded here returned `CHANGES_REQUIRED`, so no reviewer has
seen the tree at the current head.** The reviewed SHA is `c42b3c1`; any
commit after it carries the repair for the findings below and has not
itself been reviewed. This record is evidence of what the reviewer said,
not an attestation of the merge head.

## Pass 1 — `5b67179` — CHANGES_REQUIRED

Reviewed SHA `5b67179c14a692b823654846efbada1434288449`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr108-5b67179.md` on the review box).

> ## Findings
>
> 1. The rewritten `charge_allowance` census is fail-open below one directory level. [runner/mod.rs](https://github.com/eventloops/upstroke/blob/5b67179c14a692b823654846efbada1434288449/src/runner/mod.rs#L2339-L2366) scans `src/topology/fold/*.rs` but never descends into subdirectories, despite claiming the entire fold module’s production region. The existing `production_sources()` helper already performs a recursive, test-aware walk. This also conflicts with the census rule that a control inside a truncated domain does not prove the whole domain was scanned ([CODING_STANDARDS.md](https://github.com/eventloops/upstroke/blob/5b67179c14a692b823654846efbada1434288449/CODING_STANDARDS.md#L578-L590)).
>
>    Concrete failure sequence:
>
>    1. Add `mod debit;` to `fold/apply.rs`.
>    2. Add `fold/apply/debit.rs` containing a used helper that calls `self.charge_allowance(...)`.
>    3. Call that helper after the existing charge in `apply_settlement`.
>    4. Runtime now charges a retained failure twice, potentially exhausting its rung allowance early.
>    5. The census still reads only the direct files, sees the original two calls, and passes.
>
>    The body’s mutation in direct child `check_candidate.rs` cannot expose this truncation because it was planted inside the scanned boundary. This is in scope: the split activates the old single-file census problem, and this diff introduces the incomplete replacement traversal.
>
> 2. The PR body’s quantitative “proof” contains false, internally contradictory claims.
>
>    - [pr.md:494](/tmp/tmp.VgtsTn8ueC/pr.md:494) says 129 function blocks were conserved, while [pr.md:516](/tmp/tmp.VgtsTn8ueC/pr.md:516) says there are only 97 definitions: 79 children plus 18 root.
>    - At the exact head there are 129 `fn` declarations: 107 across the children and 22 in the root. One is the bodyless trait signature, so there are 128 bodies. [`predicates.rs`](https://github.com/eventloops/upstroke/blob/5b67179c14a692b823654846efbada1434288449/src/topology/fold/predicates.rs#L15-L317) alone contains the 28 declarations omitted from the claimed child count.
>    - [pr.md:138](/tmp/tmp.VgtsTn8ueC/pr.md:138) claims the set matching `^\s*pub(\(| )` remains 72. Literally applied, it is 72 before and 148 after because all 76 new `pub(super)` declarations match that expression. The narrower unrestricted-public expression `^\s*pub ` does remain 72, so I found no API widening—but the stated mechanical evidence is false.
>
> 3. The opening scope claim that children contain “impl blocks and free functions only” is false: [region.rs:24](https://github.com/eventloops/upstroke/blob/5b67179c14a692b823654846efbada1434288449/src/topology/fold/region.rs#L23-L30) defines `GitRefName`. Later sections disclose the trait, so this is inconsistent wording rather than hidden code scope.
>
> I otherwise found no semantic change in the moved fold logic: all 107 moved declarations mapped one-for-one and all 106 moved function bodies were token-identical. The 76 restricted visibilities preserve their former effective reach, and the diff introduces no forbidden production `unwrap`/`expect`, `anyhow`, or non-portable path manipulation.
>
> VERDICT: CHANGES_REQUIRED
## Pass 2 — `c42b3c1` — CHANGES_REQUIRED

Reviewed SHA `c42b3c1505df4ab9e8a672100982d2ce2c477f5a`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr108-c42b3c1.md` on the review box).

> ## Findings
>
> 1. **P1 — The repaired allowance census is still fail-open.** `src/runner/mod.rs:2384-2396` merely totals the exact spelling `self.charge_allowance(` across the subtree. It does not prove the diagnostic’s claim that each settlement applier calls it.
>
>    Concrete failure sequence:
>
>    - Remove the success charge from `apply_candidate_prepared` at `apply.rs:317`.
>    - Add a second identical charge in `apply_settlement` at `apply.rs:191`.
>    - Failures are charged twice; successes are not charged.
>    - The write/consult census is unchanged, and the aggregate still finds two calls, so the assertion passes.
>
>    Additionally, a third valid call written as `Self::charge_allowance(self, ...)` is invisible to the literal needle. The recorded grandchild mutation used the same matched spelling, so it proves traversal depth only—not the claimed settlement mapping or “any third charge” protection. [Diff](/tmp/tmp.RfrYbuyoIK/pr.diff:552)
>
> 2. **P2 — The census repair violates the path rule.** The new boundary logic represents repo paths as `String`, applies string `strip_prefix`/`starts_with`, and manually interprets `/`. The existing helper produces that value through lossy string conversion and separator replacement; this change now uses the display form as path identity. That directly conflicts with the requirement to use `Path`/`PathBuf` and avoid separator assumptions. [Diff](/tmp/tmp.RfrYbuyoIK/pr.diff:537)
>
> 3. **P2 — The PR body still contains unsupported evidence claims.**
>
>    - It says the steward record contains per-boundary hashes and a signature-reflow enumeration, but the committed 385-line record contains neither. [PR body](/tmp/tmp.RfrYbuyoIK/pr.md:96)
>    - The same comparison is described first as four changed comment lines and then as four changed characters; the diff is four replaced lines, not four characters. [PR body](/tmp/tmp.RfrYbuyoIK/pr.md:522)
>    - It says steps 2 and 3 were not requested by pass 1, then later calls all three changes “repairs pass 1 asked for.” [PR body](/tmp/tmp.RfrYbuyoIK/pr.md:573)
>
> I found no new production `unwrap`/`expect`, `anyhow`, decision-file edit, or semantic alteration in the mechanically moved fold bodies.
>
> VERDICT: CHANGES_REQUIRED
