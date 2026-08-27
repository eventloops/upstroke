# PR7 — `CODING_STANDARDS.md` conformance work-list

**Not S5 findings.** This file collects conformance observations raised during PR7's
review rounds so the G2 pass can ingest them. It is empty until reviewers file into it.

## The rider reviewers are given, verbatim

> `CODING_STANDARDS.md` conformance findings do not block this merge. File each to the
> sweep work-list with the section it cites (or "no citable section"); only contract and
> correctness findings are S5 findings. The slice predates the standard; conformance is
> the G2 pass's remit.

## Why this file exists rather than the ledger

The PR's `Review finding ledger` is for findings that gate the merge — contract and
correctness — and its canonical row shape requires a severity, a provenance, a category
and a disposition. A conformance observation has none of those in the sense the ledger
means them: it is not a defect against the packet, and its disposition is "the pass will
decide", not "fixed" or "rejected".

## Salvage

Entries here are **salvaged by-hash against the merged head**, per the pass proposal's
W10.4. Each row therefore records the file and the exact content hash of the region it
cites, not a line number: PR7's own history has three occurrences of a line-anchored
reference going stale under `cargo fmt` alone (`reviews/FINDINGS.md` §4), and a work-list
that cannot be salvaged is a work-list that gets re-derived.

## Entries

| Observation | File | Region sha256 | Standards section cited | Raised by / round |
|---|---|---|---|---|
| _(none yet)_ | — | — | — | — |

## From S5 rounds 5 and 6 — the sweep's remit, not this merge's

Round 5's five lenses returned **7 `standards` observations** under the shield. Three were
things this session introduced and are repaired in-slice because they are mine; four are
routed here.

| observation | section cited |
|---|---|
| **A wrapped string literal without the `\`-continuation** leaves a run of eight or more spaces in the rendered message. Three new instances in the pre-clean witnesses were repaired; the class is **tree-wide and predates this slice** — `rundir.rs:5194`, `rundir.rs:5203`, `workspace_manager.rs:5536`, `workspace_manager.rs:7015`, `effects/tests.rs:1787`, `effects/tests.rs:1800` among them. A `grep -rnP '"[^"]*\s{8,}'` over `src/` is the whole sweep | no citable section |
| **`each_census_needle_covers_the_domain_its_doc_states` has a whole-tree name for a test over two helpers** local to `runner::tests`. The name promises more than the body | no citable section |
| **`the_frozen_pool_table_is_read_through_one_seam`'s headline is unscoped and its needle is one file.** The assertion message scopes itself; the name does not | no citable section |
| **The two ending-guard witnesses use opposite idioms for one rule**, and one's doc argues against the idiom its neighbour uses | no citable section |

The two that were repaired rather than routed, because a false sentence is not a style
preference: `emit.rs`'s repaired field doc opened "the two lines **above this one**", which
resolves only against the version it replaced; and `AttemptContext::start`'s historical note
sat inside its `# Errors` section, so rustdoc rendered provenance as part of the error
contract.
