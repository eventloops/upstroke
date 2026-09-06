---
id: SWEEP-SITES-READ-ONLY-ORDER
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/sites.rs:29
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects.rs`, queue row 28, which owns `observable_orders()`; the fix also moves `design/26_design_merge_queue_protocol.md`
---

## Failure sequence

Location as first recorded: src/topology/effects/sites.rs (`WorktreeSite::Verify`, `AnswerSite::Ingest` and `LockSite::ObserveCleanupHold` in `adjacent()`); src/topology/effects.rs (`EffectSiteId::observable_orders`)

A suite writes the fault-injection registry for `Answer.Ingest`, the answer funnel's read-only
ingestion (`rundir::ingest_answer`).

`check_bijection` calls `check_orders` for that site's before and after hook phases.
`check_orders` reads `EffectSiteId::observable_orders()`, which reads
`AnswerSite::Ingest.adjacent()` — `Adjacent::Before(DurableEvent::QuestionAnswered)` — and answers
`[ObservableOrder::EffectBeforeEvent]`. So the bijection fails unless the registry carries an
entry keyed `(Answer.Ingest, Before, EffectBeforeEvent)` and another at `(…, After, …)`.

`ObservableOrder::EffectBeforeEvent` is defined, at `src/topology/effects/residue_authority.rs`,
as "The effect is durable, the adjacent append is not." `Answer.Ingest` performs no effect: its
`is_read_only()` is `true`, its `after_effect()` is `AfterEffect::NoEffect`, and
`EffectSiteId::semantics(EntryPhase::After)` answers `rows: []` with
`ResidueArtifact::NoEffectPerformed` and `ResumeAction::RepeatObservation`. The entry the suite is
obliged to write therefore asserts a durable effect that the same authority, one function away,
says cannot exist.

Three of the four read-only sites are in this position, all measured at this head:

| site | `adjacent()` | `observable_orders()` |
|---|---|---|
| `Worktree.Verify` | `Before(AttemptStarted)` | `[EffectBeforeEvent]` |
| `Answer.Ingest` | `Before(QuestionAnswered)` | `[EffectBeforeEvent]` |
| `Lock.ObserveCleanupHold` | `Before(RunStarted)` | `[EffectBeforeEvent]` |
| `Event.ProvePrefixStable` | `None` | `[]` |

The fourth is `Adjacent::None` and takes no order at all, so the framework already has a shape that
fits a site with no effect — but it has it for a different stated reason. `EventSite::adjacent`'s
own doc comment says the whole Event group is `None` because "an append site *is* the durable
event", not because `ProvePrefixStable` observes rather than acts. Nothing in the tree says a
read-only site should have no order; the shape is a coincidence of the Event group.

The consequence is bounded, which is why this is P3 and not higher: no run is wrong and no
guarantee is lost. What is lost is that the order coordinate stops meaning what its own
documentation says at three of seventy sites, and a reader auditing an entry at
`(Worktree.Verify, After, EffectBeforeEvent)` has to know that "the effect is durable" is not to be
read there.

## What the change that takes this up should do

Decide which of the two readings the order axis has, and make one of them true everywhere:

1. **The order is about the effect.** Then `observable_orders()` answers `&[]` for a site whose
   `is_read_only()` is true, the same way it does for `Adjacent::None`, and the three sites above
   stop carrying an order. This is a behaviour change: it removes two required registry entries per
   site from the bijection's demands, it moves `effect_sites.json` (the generated inventory carries
   `read_only` and the orders side by side), and under §13 it lands in
   `design/26_design_merge_queue_protocol.md`, whose sentence today is "at most one observable
   order — which of the effect and its event append is durable first, fixed by the site's
   adjacency", with no read-only carve-out.

2. **The order is about when the site ran.** Then it is `ObservableOrder`'s two doc comments that
   are wrong for a read-only site, and they should say so: the coordinate is which of the site's
   execution and the adjacent append is on the durable side of the fault, which reads correctly for
   an observation as well as an effect. This is the cheaper repair and it keeps the adjacency
   information a read-only site genuinely carries — `Worktree.Verify` really does run before
   `attempt_started`, and the fault matrix's `T-RETRY` row needs that.

Reading 2 is the one this sweep would take on the evidence available to it, but the choice belongs
with `observable_orders()`, which is in `src/topology/effects.rs` and outside this sweep's
one-file bound; and the design sentence is the owner's. Whichever is chosen, the check that keeps
it honest is a test asserting the relation between `is_read_only()` and `observable_orders()` over
all seventy sites, which no test asserts today.

Not related to `PR3-REG-001-CONDITIONAL` or `PR4-REG-001-STILL-EQUIVALENT`, which are about a site
exposing *more than one* order; this is about a site with no effect exposing one at all. It is the
same corner as `PR3-FRAMEWORK-SILENT-2` ("read-only sites' After phase leaves nothing … derived
from the packet's 'performs no effect', not stated by it") one axis over: that one is about what
the after phase leaves, this one about the order it is keyed at, and a change that answers the
first has the second in front of it.
