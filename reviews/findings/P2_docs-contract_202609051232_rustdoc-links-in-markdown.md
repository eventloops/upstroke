---
status: owner attention required
id: PR161-ASTRA-RUSTDOC-LINKS
severity: P2
disposition: owner attention required
category: docs-contract
pr: 161
reviewed_sha: 976eae7b49e10b3560a96d2c28eb343c82cea016
location: docs/internals/effects/tests.md:65
provenance: introduced_by_feature
first_bad: 5a864e153c2b290014ed42866fbd9ac2b921e54f
guard: Convert Rustdoc references when the effects notes next receive a navigation update. PR #214 does the conversion and adds gate N5; it is parked, not merged, and this record stays open until it or a successor lands.
---

Deferred by owner authorization on 2026-09-05 under DOCS_FAST_TRACK.md and
STACK_STOP_RULE.md. This record preserves the finding without claiming a fix.

## Failure sequence

A reader opens the migrated notes as Markdown and follows the `census_domain` link at `docs/internals/effects/tests.md:65`. Its destination is `crate::effects::census_domain`, which requires Rustdoc's item resolver and does not name a repository document or web page. The same problem affects `PACKET_PRIMITIVES` at `docs/internals/effects/tests/contract_mappings.md:123`.

Shortcut references also remain in Rustdoc form without Markdown reference definitions. For example, the `normalize_lint` reference at `docs/internals/effects.md:160` renders as bracketed code, with no link. Moving these references out of Rustdoc removes their navigation behavior even though the code headings themselves can be found by searching the source.

The independent CommonMark rendering audit found two `crate::` link destinations and 145 unresolved bracketed code references across the ten effects notes. It rendered the examples as `<a href="crate::effects::census_domain">` and `[<code>normalize_lint</code>]`. The complete inventory and renderer output are retained with the independent review evidence.

## What the change that takes this up should do

Replace Rustdoc item links with relative Markdown links to the corresponding notes or source, using an anchor where useful. Render representative cross-file and same-file references and check the resulting destinations. Use plain code spans for references that are deliberately not links.

## Status, 2026-09-06: owner attention required

PR #214 on branch `codex/findings-133b25f7d89f` carries the whole conversion and
is **parked, not merged**. This record therefore stays open on master.

**What is done and independently confirmed.** Both `crate::` destinations and all
145 shortcut references in the ten effects notes are converted, and nine more
unresolvable destinations found elsewhere in the notes tree with them. Both
review passes state that the conversion fixes this finding and that its
before/after rendering evidence exercises the claimed failure. The nine-command
baseline is green at every pushed head and both required CI contexts are green at
`b63ab3815bbe619dea906cdb8e3d062499bc7739`.

**What blocks it.** Not the conversion: the gate the same change adds.
`PR214-N5-DESTINATION-DELIMITERS`, a P2 raised by review round 2 with a renderer-
confirmed witness, is unresolved. N5's inline-destination scanner counts
parentheses without first excluding a quoted title or an angle-bracket
destination, so `[`census_domain`](crate::effects::census_domain "(")` and
`[`census_domain`](<crate::effects::census_domain(>)` are both valid CommonMark
links carrying the retired destination, and both pass the gate. Round 1's two P2s
were fixed; this one arrived on the second and final pass, and a mandatory
witnessed defect cannot be deferred.

**What a successor should do.** Take the branch as it stands. The fix is small
and local to the destination scan in
`.github/scripts/validate-internals-notes.sh`: read `<...>` as a whole
destination before counting anything, skip a quoted title without counting its
parentheses, and refuse rather than silently drop a candidate that cannot be
classified. Add the two witnesses above as rejection fixtures with passing
counterparts. Everything else on the branch has been through two independent
reviews.

Evidence: `/home/ubuntu/findings-workflow/tasks/133b25f7d89f/`, with both
verdicts, the diff each read, the failing-before gate log against master, the
GitHub rendering transcript and every baseline.

## Owner attention required

Recorded 2026-09-06T17:11:13.747400+00:00. Workflow task 133b25f7d89f.

# Park: 133b25f7d89f / PR161-ASTRA-RUSTDOC-LINKS / PR #214

Status: **owner attention required**, 2026-09-06. Both review passes spent; the
second returned CHANGES_REQUIRED with a mandatory witnessed P2 that cannot be
deferred and cannot be repaired without a third pass.

## Exact state

| | |
|---|---|
| Branch | `codex/findings-133b25f7d89f`, preserved |
| Worktree | `/srv/worktrees/findings-workflow/tasks/133b25f7d89f`, preserved |
| PR | #214, draft, **not merged, do not merge** |
| Pinned review base | `7177050a6f6336ff0ca7d562f526e7c007263d34` (master tip fetched 2026-09-06, merged once at `1bbc8ed5`, clean and automatic, disjoint files, no conflict) |
| Round 1 reviewed head | `f795e911aeec525ef944d5c46bbdd6eb82cb92c1` |
| Round 2 reviewed head | `b63ab3815bbe619dea906cdb8e3d062499bc7739` |
| Park head | `17d2379ac5b1d9c605918947b2dae8f1ad5e06c3` |
| Reviewer | `gpt-6-astra`, high effort, official Codex CLI, read-only sandbox, 90-minute limit |
| Implementor | `claude-opus-5`, high effort |

## Reviews

**Round 1** — `f795e911`, CHANGES_REQUIRED, two P2, **both fixed** in `b63ab381`.
Verdict published verbatim and SHA-bound:
https://github.com/eventloops/upstroke/pull/214#issuecomment-5560709180

- `PR214-N5-WRAPPED-DESTINATION`: an inline destination wrapped onto the line
  after its `(` emitted no record, so a Rustdoc destination passed the new gate
  wrapped and failed it unwrapped.
- `PR214-N5-BLOCK-CLASSIFICATION`: four defects in the gate's CommonMark
  reading — a backtick info string taken for an opening fence and the rest of
  the file skipped, a closing-fence test that ignored the info string, backticks
  pairing across list items, an indented example read as navigation.

The repair did not patch the four. It removed the surface: N5 stopped
classifying blocks and pairing code spans across a file, and became a lexical
refusal with the notes README's three quotations listed by exact text and an
unused-row check on that list.

**Round 2** — `b63ab381`, CHANGES_REQUIRED, one P2, **unresolved**. Verdict
published verbatim and SHA-bound:
https://github.com/eventloops/upstroke/pull/214#issuecomment-5560790730

The reviewer confirmed the captured diff against HEAD, that the conversion fixes
the original finding, that the rendering evidence exercises the claimed failure,
and that the repaired validator rejects both of round 1's witnesses.

## The remaining blocker

`PR214-N5-DESTINATION-DELIMITERS`, P2, at
`.github/scripts/validate-internals-notes.sh:323` on `b63ab381`.

N5's inline-destination scan counts parentheses to find the end of `](…)`
without first excluding the two places CommonMark allows an unbalanced one. Both
of these are valid links carrying the retired Rustdoc destination, and both pass
the gate:

```markdown
[`census_domain`](crate::effects::census_domain "(")
[`census_domain`](<crate::effects::census_domain(>)
```

Removing the title makes the same input fail, which is the reviewer's control.
The witness is renderer-confirmed and executable.

**The repair, for a successor.** Small and local to that scan: read `<…>` as a
whole destination before counting anything, skip a quoted title without counting
its parentheses, and refuse rather than silently drop a candidate that cannot be
classified. Add both witnesses as rejection fixtures with passing counterparts
using a resolvable relative destination. Everything else on the branch has been
through two independent reviews.

## Why this is a park and not a repair

The workflow allows two passes per finding and both are spent. A mandatory
witnessed defect may not be deferred, and a gate change after review requires the
remaining pass, which does not exist. Merging on a third unreviewed repair is not
available.

## What is done, and stands

- Both `crate::` destinations and all 145 shortcut references in the ten effects
  notes converted: 20 to inline Markdown links, 125 to plain code spans.
- Nine more unresolvable destinations found elsewhere in the notes tree by
  grepping the class, three of which a `crate::` search does not find:
  `Self::close_and_wait`, `RunState::apply`, `std::time::Duration`. Eleven in
  total — eight became links, three became code spans.
- N5 added to `validate-internals-notes.sh` with 26 new isolated fixtures, 66
  cases in total.
- `docs/internals/README.md` gains a *Referring to an item* section stating the
  rule and the reasoning.
- `PR161-NOTES-SHORTCUT-REFERENCES-TREE` recorded, deferred: 1743 shortcut
  references across 111 other notes files.
- `PR160-NOTES-RUSTDOC-LINKS` updated, not deleted: its two cited destinations
  are converted, its shortcut-reference half is not.
- The original ticket restored and marked `owner attention required` with the
  same account.

## Gates and CI

- Nine-command baseline through `upstroke-build`: ALL 9 PASS at `17d2379a`,
  `b63ab381`, `f795e911`, `ab32164f`, `cea88149`, `e087f8e6`, `29e05c95` and
  `402612ad`. Logs in `~/eight-logs/` and copied to `evidence/`.
- Both required contexts green on `f795e911` (after one rerun of `test
  (winguest)`, a lost UNC share in a test this branch does not touch) and on
  `b63ab381`, which is what each review read.
- Two local FAIL 03 runs were
  `workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered`,
  the parked `PR172-SAMPLER-REFUSED-A-TORN-WORKTREE-LIST-RECORD`. Each passed in
  isolation and the rerun baseline was ALL 9 PASS. This branch touches no Rust
  source.

## Evidence

`/home/ubuntu/findings-workflow/tasks/133b25f7d89f/`:

- `review-1/` and `review-2/`: each verdict, the diff it read, its prompt and log.
- `evidence/README.md`: index.
- `evidence/gate-before-master.log`: the final gate against master
  `f25eeedc816336e4995c6575ac1b626197c8801d`, 156 violations, exit 1.
- `evidence/inventory-before.txt`, `evidence/github-rendering.txt`,
  `evidence/links-added.txt`, `evidence/commonmark-scan.py`, `evidence/baseline-*.log`.
