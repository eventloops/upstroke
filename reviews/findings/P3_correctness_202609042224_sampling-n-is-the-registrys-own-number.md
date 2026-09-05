---
id: SWEEP-BIJECTION-005
severity: P3
disposition: deferred
category: correctness
pr: 146
reviewed_sha: 943ae61dc61c579a3b03744c8994a1ce81a9acf8
location: src/topology/effects/bijection.rs:450
provenance: pre_existing
first_bad:
guard: the registry.json gate PR10 builds, and whatever design section fixes N per site
---

## Failure sequence

A recovery-proven entry declares `sampling: { n: 1, histogram: { none: 1, internal: 0, after: 0 },
unclassified: 0, recovered: true }` for a site whose kill sampling is supposed to be frozen at a
much larger N. `check_evidence` reads `n` out of the entry it is checking and asks four things
of it: that it is not zero, that no sample was unclassifiable, that the histogram plus the
unclassified count equal `n`, and that everything recovered. All four hold. The entry is accepted
as recovery-proven evidence on its own authority, and ST-07's kill-sampling requirement is
discharged by a document that named its own bar.

The second half is worse and is not reachable from inside the crate at all. "Frozen" is a property
*across runs* — that this run's N is the N the registry fixed and not a smaller one someone wrote
after a red run. A check over one document cannot observe a property of two, so no arrangement of
this file can hold the freeze. The same limit sits one level up: `Evidence::Executed` names its
test as free text, and nothing in the crate checks that a test of that name exists or that it ran.

What is missing is an authority outside the document for the numbers the document asserts, and
the module said nothing about the gap. (An earlier version of this file said the check was not
wrong about anything it claims. Pass 1 on `ffe26ca` disproved that: the accounting sum saturated,
so `n = u32::MAX` with a histogram one sample over passed. That is fixed at `aa52b41` and is a
`fixed` row of PR #146's ledger, not this finding — this finding is about the authority for `n`,
which the fix does not and cannot supply.)

## Why this is deferred as beyond reach, not as out of scope

Deferring for scope promises that a later session picks the work up. This one has nowhere in the
crate to pick it up from: fixing it needs a per-site frozen N held somewhere the registry cannot
edit, which is a design decision and a gate that does not exist yet (`DESIGN.md` §21 puts the
registry.json gate in PR10). **If a later pass labels this P1 or P2, the disposition becomes
escalate-to-owner rather than still-deferred** — there is no session that can honestly hold it open
under a blocking label.

## What the change that takes this up should do

Give the frozen N a home outside the entry that cites it — a per-site constant beside
`EffectSiteId::residue_elements`, or a design section the registry.json gate reads — and make
`check_evidence` compare the record's `n` against it rather than only against itself. The gate
that reads registry.json across runs is where the freeze itself can be checked; a single-document
check cannot, and the module doc now says so rather than leaving a reader to assume otherwise.
