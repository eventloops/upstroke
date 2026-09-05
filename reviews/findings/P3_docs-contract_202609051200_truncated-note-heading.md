---
id: ASTRA165-005
severity: P3
disposition: deferred
category: docs-contract
pr: 165
reviewed_sha: 9699c27396e6eff34be9a86bc634f042080cb280
location: docs/internals/topology/fold/check_end.md:13
provenance: introduced_by_feature
first_bad: —
guard: Copy each advertised source-search fragment into a literal search of its corresponding Rust module and verify a match
---

## Failure sequence

The note heading truncates the function declaration's return type to `FoldE…`
even though the notes contract promises exact source search strings. Copying its
code fragment into `rg -n -F` exits 1 without a match. Searching the untruncated
function name exits 0 and finds the declaration at source line 7. A reader cannot
use the advertised fragment to navigate to its source.

## What the change that takes this up should do

Use complete item names or untruncated fragments in generated headings and refresh snippets affected by formatting.

The review retains the commands and exit codes in `heading-witness.json`, with the
broader inspection in `heading-search-analysis.json`. The same issue affects other
headings, and formatting invalidates snippets such as
`docs/internals/topology/census.md:135`.
