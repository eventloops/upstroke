---
id: SWEEP-EFFECTS-REGISTRY-DESIGN-TEXT-CHANNEL
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: design/26_design_merge_queue_protocol.md:507
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects.rs` (queue row 28), or the next change to DESIGN.md §26's "Diagnostics are typed" paragraph
---

## Failure sequence

`DESIGN.md` §26, "Diagnostics are typed", ends: "An entry the format refuses is
reported with the format's own error value embedded, and that value carries what
the entry wrote in the field the format refused — a residue detail, a resume
action, **a site or phase name as text** — so a hand-edited document's own text
can reach a reader through that one variant, quoted, never interpreted."

Two of those three are true. `RegistryError::WrongResidueDetail.found` and
`WrongResumeAction.found` are `String`s taken from the entry, and a hand-edited
`registry.json` reaches a reader through them verbatim. The site and phase names
are not: `RegistryEntry::site` is an `EffectSiteId` and `RegistryEntry::phase` is
an `EntryPhase`, both closed enums, so a document naming a site or a phase the
enums do not declare is refused by serde and never reaches `validate_entry` at
all (`the_wire_form_refuses_an_entry_naming_a_site_the_enums_do_not_declare`
pins exactly that). Every `site: String` and `phase: String` in `RegistryError`
is `entry.site.name()` or `entry.phase.to_string()` — the format's own rendering
of its own enum, not the document's words.

The consequence is a reader of the design who believes the site and phase fields
are a channel for a document's own text, and either relies on it for diagnosing
a hand-edited registry — where it carries nothing the enums did not already
authorise — or takes the `String` typing of those fields to be load-bearing when
what actually forces it is nothing.

## What the change that takes this up should do

Either narrow the design sentence to the two fields that carry a document's own
words, and say that the site and phase a refusal names are the enums' own
spellings because no other value can reach the refusal; or, if the site and
phase fields are wanted as typed values (which is what the same paragraph's
first sentence asks of `BijectionFailure`), retype them as `EffectSiteId` and
`EntryPhase`. The message text is unchanged by the retyping — both implement
`Display` and every `#[error]` format string uses `{site}` and `{phase}` — and
no test in the tree constructs or reads those fields (all 39 `RegistryError::`
sites in `src/topology/effects/tests.rs` match with `{ .. }`), so the change is
confined to the one file. Do the design sentence and the code in the same change,
per §13.
