# The finding ledger: one file per finding

A finding that outlives its pull request is recorded here, as its own file. Nothing is appended
to a shared document any more.

## Why

`reviews/FINDINGS.md` is a single file that every pull request appends a section to. That made it
the repository's worst merge-conflict source: on 2026-09-04 alone it conflicted on #118, #119,
#120, #122, #126, #127, #128, #131 and #109, and sessions began reserving section numbers from
each other in advance to work around it. Two sessions recording unrelated findings have no reason
to touch the same bytes, and under this layout they do not.

## Where new findings go

One file per finding, in this directory:

```
reviews/findings/<UTC timestamp>_<category>_<ID>.md
        e.g.     202609041730_correctness_PR135-FIXTURE-GIT-IN-THE-FIXTURE.md
```

- **Timestamp** `YYYYMMDDHHMM`, UTC, when the finding was recorded. It orders the directory and
  keeps names unique.
- **Category** from the closed vocabulary the gates already enforce, exactly as spelled in a
  ledger row: `correctness`, `crash-consistency`, `security-trust`, `portability`, `liveness`,
  `performance`, `compatibility`, `docs-contract`. `.github/scripts/test-pr-policy.sh` rejects
  anything else in a pull request body, so a different word here would put this directory and the
  gate out of step.
- **ID** the finding's stable identifier, the same one used in the pull request body's ledger row
  — `PR3-LIMITS-SCHEDULING`, `PR4-CONF-001`, and so on. It is what other files cite, so it never
  changes.

**Severity and status are not in the filename.** They are the two fields that move: a P2 is
promoted on a later pass, a finding goes `deferred` then `fixed`. Putting either in the name would
mean renaming the file, and breaking every reference to it, every time the truth changed. They
live in the frontmatter, where an edit is a one-line diff.

## The file

```markdown
---
id: PR135-FIXTURE-GIT-IN-THE-FIXTURE
severity: P1
status: fixed            # deferred | fixed | accepted-risk | rejected
category: correctness
pr: 135
reviewed_sha: af53b8f...  # full SHA the finding was found at
location: src/workspace_manager/fixture.rs:212
provenance: pre_existing  # pre_existing | introduced_by_feature | fix_regression | undetermined
first_bad:               # SHA or prior finding ID, when known
guard: <the test, or the sweep that takes it up>
---

## Failure sequence

Input or state, then the step that goes wrong, then the wrong result. Concrete, not abstract.

## Disposition

What was done, or what the change that takes this up should do.
```

The frontmatter fields are the columns of a pull request body's ledger row, so a row and a file
carry the same facts and neither is derived from the other by hand.

## What did not change

- **A pull request body still carries its own ledger table**, with the canonical nine-column
  header. That is validated by `.github/scripts/validate-pr-body.sh` and
  `validate-pr-ledger-evidence.sh` and is unaffected by this layout.
- **`reviews/FINDINGS.md` stays exactly as it is**, as the historical ledger up to 2026-09-04. Its
  sections keep their numbers, because source comments and design sections cite them by number
  (`reviews/FINDINGS.md` §4, §19, §20 and others). Nothing is migrated and nothing is renumbered.
  It is closed to new sections; add new findings here instead.
- **The review-preservation rule still applies.** `MAINTAINING.md` lets a push confined to the
  ledger keep its frontier review; that exemption now covers this directory as well, so recording
  a finding does not cost a pass.
