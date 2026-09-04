---
id: PR139-REFUSAL-SEMANTICS-NOT-IN-DESIGN
severity: P2
disposition: deferred
category: docs-contract
pr: 139
reviewed_sha: cc202c81ab289ec74ee6ee527534bb543735a7f8
location: design/15_design_event_log_resume_run_layout.md:30
provenance: introduced_by_feature
first_bad:
guard: an owner change to `DESIGN.md` §15 stating the three refusal semantics below; until then the behaviour's only record is PR #139's body and the doc comments in `src/rundir/ownership.rs` and `src/rundir.rs`
---

## Failure sequence

`CODING_STANDARDS.md` §13 requires the design to change in the same pull request as the behaviour
it describes, and forbids a code comment being the sole record of a design decision. PR #139
changes what the startup census does with three classes of I/O answer, and `DESIGN.md` is the
owner's — no session edits it — so the change landed with its only durable record in a pull request
body and in doc comments. A reader of §15 alone now has an incomplete account of when a run
directory is reclaimed.

§15 states the reclaim preconditions positively ("the chain … carries no symlink or reparse point",
"every create, reclaim and delete revalidates all of that") and says nothing about what happens
when the filesystem *declines to answer* the questions those preconditions ask. That silence is
what let three folds each answer a failure with a deletion.

## What the change that takes this up should do

§15 has to state three semantics, and each is a sentence about evidence rather than about code:

1. **Listing.** A directory listing that did not complete is not an empty directory. Only a listing
   that completed establishes what a run directory holds, and no error kind is privileged —
   `NotFound` included, because a directory that is not there has not answered the question either,
   and because this crate has measured the Windows guest reporting `NotFound` for a stat beneath a
   file ancestor. The reclaiming shapes ("a bare directory, or one holding only a staged marker")
   therefore require a listing that answered.
2. **Stat.** Only `io::ErrorKind::NotFound` proves a path absent. Every other error is an answer
   the filesystem declined to give, and the census's reclaiming answers — the marker's recorded
   private target being gone, and `committed.json` being absent — require proof of absence rather
   than absence of proof. §15 already implies this for the commit record; it should say it for the
   target too.
3. **Canonicalisation, and path identity.** A canonicalisation that failed establishes no identity,
   so a conjunct that compares canonical paths refuses rather than falling back to the path it was
   handed. And identity is compared as a **path**, never as a lossy rendering of one: the records
   carry `String`s, so a run whose path is not valid UTF-8 is retained and reported rather than
   proven or reclaimed. That last clause is a real behaviour bound and belongs in the design
   alongside the run-layout rules, not only in a risk section.

The related and larger question — that `CreatingMarker.private_dir`, `OwnerRecord.public_dir` and
`run_started.private_dir` all carry a lossy `String`, so a non-UTF-8 path cannot round-trip at all
— is **not** this finding. It has a parked owner-level record in PR #39, which carries a DO NOT
MERGE caution until after v0.2. This finding is only about the design stating what the census does
today, which is refuse.
