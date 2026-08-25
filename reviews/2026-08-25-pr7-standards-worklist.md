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
