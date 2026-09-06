---
id: SWEEP-EFFECTS-PARENT-001
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects.rs:338
provenance: pre_existing
first_bad:                # present since the effect-seam framework landed; exact origin not derived
guard: a change that takes `observable_orders`, its two consumers in `src/topology/effects/registry.rs` and `src/topology/effects/bijection.rs`, and `EffectSiteExport`'s wire field together, with the `design/25` and `design/26` sentences the field's shape appears in
---

## Failure sequence

`EffectSiteId::observable_orders` returns `&'static [ObservableOrder]`, a slice whose length is
zero or one and never two: its whole body is `match self.adjacent()` over three arms, two of which
return a one-element static and the third the empty one. Its own doc says so — "One order, not
two, wherever the design fixes which of the effect and the append is durable first ... A site with
no adjacency has no order at all, and its entry carries `None` rather than an arbitrary one" — and
`design/26_design_merge_queue_protocol.md` states it as "at most one observable order". §5 says
`Option<T>` is absence; the type here says "any number of orders", so the invariant is carried by
prose and by a `match` a reader has to go and read, and not by the signature.

The consequence is not hypothetical. `check_orders` in `src/topology/effects/bijection.rs:553`
records that the residue-class loop took `observable_orders()[0]` while the hook loop iterated the
slice, and that "the two agree at this head, because `observable_orders` answers one order or none
by construction, and agreeing by coincidence is not the same as agreeing". A slice of at most one
invites both spellings — an index that panics on the empty case and a loop that silently does
nothing on it — and the wide type is what makes the two look equally correct at the call site. An
`Option` admits neither: the empty case is a variant the caller must name.

The defect this file's sweep fixed elsewhere is the same class one level down. `semantics`'s
`EntryPhase::Residue { .. }` discarded a closed-domain field, and `EffectSiteId::name` and
`Display` were two statements of one shape. This is the third: one statement of a bound in a type
that does not carry it.

Not fixed in the sweep of `src/topology/effects.rs` because the change does not stay inside the
file. `validate_entry` (`src/topology/effects/registry.rs:542`, `orders.contains(&order)` and
`orders.is_empty()`) and `check_orders` (`src/topology/effects/bijection.rs:562`) are two swept
rows under concurrent sweep at this base, and `EffectSiteExport::observable_orders`
(`src/topology/effects/export.rs:65`) is a `Vec<ObservableOrder>` in the published
`effect_sites.json`, so the narrowing is a wire-format change — `"observable_orders": []` becomes
`"observable_orders": null` — with the `design/` sentences that describe the document to move with
it. That is past a sweep's own-file bound.

## What the change that takes this up should do

Narrow `EffectSiteId::observable_orders` to `Option<ObservableOrder>` and let each consumer name
the empty case:

* `validate_entry`'s three-arm `match (entry.phase, entry.order)` becomes
  `(_, order) => order == site.observable_orders()`, which is the same rule stated once instead of
  as a `contains`/`is_empty` pair.
* `check_orders` becomes a `match` on the `Option` with the `None` arm calling `check_evidence`
  with `None`, deleting the `is_empty`/loop asymmetry the comment at
  `src/topology/effects/bijection.rs:553` was written about.
* Decide `EffectSiteExport::observable_orders` deliberately: either keep the `Vec` at the wire
  boundary, with the conversion stated at the one site that builds it, or narrow it to
  `Option<ObservableOrder>` and say in `design/25_design_export_decisions_schema.md` and
  `design/26_design_merge_queue_protocol.md` that the field is a value or null and never a list.
  Keeping the `Vec` is defensible — a document field that may grow is not the same decision as an
  in-process accessor — but it should be a decision written down and not the accessor's shape
  leaking outward.

Carry a test that the `None` case is reached: `Event.AppendFirst` has `Adjacent::None` and is the
site that exercises it (`the_observable_orders_are_the_ones_the_adjacency_admits` already crosses
all three adjacency arms and counts each as non-empty, so it is the assertion to move rather than
to add).
