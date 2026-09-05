# The finding ledger: one file per open finding

**Every finding gets its own file. One per finding — never one per pull request, and never one
per review pass.** A pass that returns six findings produces six files.

**When a finding is resolved, delete the file.** This directory is the outstanding work and
nothing else. An `ls` is the queue, worst first, with nothing to run and nothing to filter.

## Naming

```
<severity>_<category>_<UTC timestamp>_<brief-description>.md

P1_correctness_202609041730_git-in-the-fixture.md
P1_liveness_202609041732_unbounded-join-on-holder.md
P2_portability_202609041735_crlf-in-name-status.md
```

Severity leads so the directory sorts worst-first with no tooling: the P1s you have to fix are at
the top. Two findings from the same pass are two files, as above.

- **Severity** `P1`, `P2`, `P3`. It changes only when a reviewer reclassifies, which is rare and
  deliberate; when it does, `git mv` the file and history follows.
- **Category** from the closed vocabulary the gates already enforce, spelled exactly as in a
  ledger row: `correctness`, `crash-consistency`, `security-trust`, `portability`, `liveness`,
  `performance`, `compatibility`, `docs-contract`. `.github/scripts/test-pr-policy.sh` rejects
  anything else in a pull request body, so a different word here would put this directory and the
  gate out of step.
- **Timestamp** `YYYYMMDDHHMM`, UTC, when the finding was recorded. Keeps names unique and orders
  findings of equal severity by age, so the oldest P1 is the first line.
- **Brief description** a few words, hyphenated, readable at a glance.

**The stable identifier is not in the filename.** It lives in `id`, because that is what pull
request bodies, ledger rows and source comments cite, and a citation must not break when a finding
is reclassified. `grep -rl 'id: PR135-…' reviews/findings/` finds the file.

## The file

```markdown
---
id: PR135-FIXTURE-GIT-IN-THE-FIXTURE
severity: P1
disposition: deferred     # deferred | accepted-risk — why it is still here
category: correctness
pr: 135
reviewed_sha: af53b8f...   # full SHA the finding was found at
location: src/workspace_manager/fixture.rs:212
provenance: pre_existing   # pre_existing | introduced_by_feature | fix_regression | undetermined
first_bad:                # SHA or prior finding ID, when known
guard: <the sweep or change that should take this up>
---

## Failure sequence

Input or state, then the step that goes wrong, then the wrong result. Concrete, not abstract.

## What the change that takes this up should do
```

The fields are the columns of a body's ledger row, so a row and a file carry the same facts and
neither is transcribed from the other by hand.

## Deleting is safe, and where the permanent record lives

A deleted finding is not lost. Three things outlive it:

- **The pull request body's ledger table**, which lists every finding of that pull request with
  its disposition, including the ones fixed before merge. That table is validated by
  `validate-pr-body.sh` and `validate-pr-ledger-evidence.sh` and is the auditable record.
- **Git history.** `git log --diff-filter=D -- reviews/findings/` lists every finding ever closed,
  and `git show` recovers the file.
- **`reviews/FINDINGS.md`**, for everything up to 2026-09-04.

The one thing this costs is a grep. Checking whether a defect has recurred used to be a search of
one file; it is now a search of this directory for the open ones and `git log` for the closed. That
matters here — this project does record second sightings — so when you close a finding whose shape
looks likely to return, say so in the body's row rather than relying on the file being findable.

## What did not change

- **A pull request body still carries its own ledger table**, with the canonical nine-column
  header, listing every finding of that pull request. The body groups by pull request; this
  directory does not.
- **`reviews/FINDINGS.md` keeps its resolved history**, as the same ledger up to 2026-09-04. Its
  sections keep their numbers, because source comments and design sections cite them by number
  (`reviews/FINDINGS.md` §4, §19, §20 and others, in 25 places across 16 files). Nothing is
  renumbered, and its dated narrative and audit sections are left as their authors wrote them. What
  did move is its open rows: every one is a file here now, under the same `id`, so this directory
  is the whole queue and that file is the history behind it. It is closed to new sections; new
  findings come here.
- **The review-preservation rule still applies.** `MAINTAINING.md` lets a push confined to the
  ledger keep its frontier review; that exemption names this directory as well, so recording a
  finding, or deleting a resolved one, does not cost a pass.
