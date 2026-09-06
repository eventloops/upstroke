---
id: PR161-NOTES-SHORTCUT-REFERENCES-TREE
severity: P2
disposition: deferred
category: docs-contract
pr: 214
reviewed_sha: f25eeedc816336e4995c6575ac1b626197c8801d
location: docs/internals/engine/topology/settle.md:22
provenance: introduced_by_feature
first_bad: undetermined; the notes moved subtree by subtree, and `git log --diff-filter=A -- docs/internals` names the commit that added each file
guard: N5(b) in `.github/scripts/validate-internals-notes.sh` refuses the form inside the converted domain. Widen that domain in the same change that converts a subtree, and the gate holds it.
---

Recorded, not fixed, by PR #214, the change that closed `PR161-ASTRA-RUSTDOC-LINKS`. That
finding's remedy was scoped to the ten effects notes, because those ten are what an
independent CommonMark rendering audit measured. The same form is in most of the rest
of the tree, and this record carries it rather than letting the scoped fix imply the
tree is clean.

## Failure sequence

A reader opens any of 111 notes files as Markdown and meets a rustdoc shortcut
reference — a code span in brackets, `[`rematerialize_question`]` at
`docs/internals/engine/topology/settle.md:22`. Rustdoc resolved it against the item
tree. CommonMark looks for a link reference definition, finds none in the file, and
renders `[<code>rematerialize_question</code>]`: brackets around code with nothing
behind them. `docs/` is the GitHub Pages source for upstroke.rs, so this is what
readers of the published notes get. There is no navigation to lose — there was none
after the migration — but the brackets promise one on every occurrence.

1743 occurrences across 111 files, `settle.md` holding 23 of them. The count is the
one N5(b) makes, so it is reproducible from the tree rather than transcribed: run the
same block scan the gate runs — fenced blocks dropped, code spans found first, one
leaf block at a time — over `docs/internals/**/*.md` outside the effects notes, and
count code spans whose immediately preceding and following characters are `[` and `]`
and which are not followed by `(`.

The ten effects notes held 145 more, all converted at the head that records this. The
eleven unresolvable link *destinations* in the tree — the other half of the same
migration — are converted too, and N5(a) holds every notes file to them. This record
is only the shortcut-reference half outside the effects notes.

## What the change that takes this up should do

This is large and mechanical, and it is not one change. Take a subtree at a time, in
whatever order the reader gets the most from, and in each:

- Convert each reference to an inline Markdown link when it names a module that has
  its own notes file and that file is not the one being edited, and to a plain code
  span otherwise. `docs/internals/README.md`'s "Referring to an item" states the rule
  and why the destination is the file rather than a heading anchor.
- Widen N5(b)'s domain in `.github/scripts/validate-internals-notes.sh` to the subtree
  in the same change, so the domain grows with the conversion rather than ahead of it,
  and add its fixture beside `shortcut_reference_below_effects_is_rejected`.
- Render the converted file and check the destinations it now claims. A link to a
  notes file that does not document the named item is worse than the code span it
  replaced.

An implementor who cannot scope one subtree inside this workflow's review budget
should park this rather than convert the tree in one pass.
