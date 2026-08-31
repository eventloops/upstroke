# 2026-08-31 — the inertness premise is behavioural, and the visibility wording is retired

*Adopted by the owner on 2026-08-31 as ruling 1, **ratified as amended**. The
base text is the adjudication draft prepared by `promotion_decisions_fable`
(Fable 5), sha256 first-16 `fa8333798e231eee`; the owner's binding amendments 1a
and 1b are applied below and marked. Every source citation in this record was
re-verified against the candidate head before it was committed.*

**Verdict.** The G2 checkpoint's inertness condition
(`2026-08-25-checkpoint-merges.md`: "schema-4 machinery engaged only by
explicit schema choice") is **behavioural, is satisfied at the candidate head
`50ed8c86ec60164011bfd393066c4c3696d3865b`, and is the only form of the
premise this project states from now on**. The visibility form — "the
schema-4 surface is `pub(crate)`, so the released library cannot create state
its own resume refuses" — is retired as false at this head and unachievable
at any head, and the PR #18 body rewrite replaces it. No code changes. The
panel's confirmation of inertness on the candidate remains owed and is
unaffected.

## Why

- **The visibility claim is false at the head.** `src/lib.rs:49` is
  `pub mod topology;` with all nine submodules `pub mod`. What PR7 actually
  narrowed to `pub(crate)` is `engine::topology` — the coordinator
  (`src/engine/mod.rs:61`), whose zero-non-test-caller state is
  compile-checked (`src/engine/assembly.rs`, 328 dead items reported when
  narrowed). The PR body conflated the coordinator with the surface.
- **The public surface includes a schema-4 writer, not vocabulary alone.**
  A downstream crate can durably write a schema-4 `run_started` using only
  public API: construct `RunStarted4 { schema: TOPOLOGY_SCHEMA, … }` (all
  fields `pub`, nothing `non_exhaustive`), check it with
  `TopologyLine::round_trip` (`src/events/log.rs:1242`), open the topology
  funnel with `EventLog::open(EventSite::OpenLog, …)` (`:466`), and commit it
  with `append_topology(site_for(&body), …)` (`:796`, `:1064`). No
  write-side activation check exists; `TOPOLOGY_ACTIVATION` gates reading
  only. The resulting log is state the same binary's resume refuses by name.
- **The claimed guarantee is unachievable by visibility.** The legacy funnel
  already accepts any `pub u32` schema (`RunStarted.schema`,
  `src/events/mod.rs:315`), and plain `std::fs` binds no downstream crate.
  Log bytes are untrusted input, and the code has always treated them so —
  the `schema.rs` grid crosses hostile schemas up to `u32::MAX` against
  hostile ceilings.
- **The behavioural condition holds, verified.** Production's only
  `run_started` mint stamps schema 3 (`src/engine/coordinator.rs:164`); no
  CLI arm reaches the topology coordinator; the read ceiling is 3 by const
  assertion evaluated in the ordinary build (`src/topology/schema.rs:98-101` — four assertions, the first of which pins `TOPOLOGY_ACTIVATION` to `Inactive`);
  no upgrade path enters schema 4 (`check_upgrade_transition`); refusal is
  explicit and named (`SchemaRefusal::TopologyLogUnreadable`); and writing
  schema-4 state through the library takes three separate explicit topology
  choices. Nothing engages by default, by accident, or by upgrade — which is
  the condition, working.
- **The guarantee the project states is "cannot get it read", not "cannot
  create".** Refusing loudly, precisely, and without misfolding is the
  property that is built, tested, and const-pinned; it is also the only one
  that can be true.

## Conditions

- The PR #18 body rewrite carries the corrected sentence and not the retired
  one. Corrected form: "What holds `production_effect` at none is that
  `engine::topology` — the driver — is `pub(crate)` with a compile-checked
  absence of non-test callers, while inertness itself is behavioural: no
  shipped command writes schema 4, the read ceiling is 3 by const assertion,
  and a schema-4 log — however it was produced, including through the public
  `append_topology` funnel or by hand — is refused explicitly by name."
- **Amendment 1b (binding), applied.** The gate report's sentence "A library
  consumer can name the schema-4 vocabulary" was understated: a consumer can
  **write** schema-4 durable state through the checked funnel. The corrected
  sentence is now in `reviews/2026-08-31-g2-gate-report.md`, and that report
  carries a revision note recording the change the same way it recorded the
  serialized-run revision. The same corrected sentence goes into the PR #18
  body rewrite. The report's refusal of the `pub(crate)` wording stands.
- **Amendment 1a (binding), applied.** The public schema-4 write path is
  carried as a row in the ledger, not left as prose here:
  `SCHEMA4-PUBLIC-WRITE-PATH-UNGATED` in `reviews/FINDINGS.md` §37, owner
  *project owner*, venue *the PR12 activation slice*, shrinking when that slice
  lands or when a visibility narrowing is scheduled. The panel is to find this
  triaged in the ledger rather than discover it.
- The panel is pointed at this record so its inertness confirmation reviews
  the behavioural claims, not the retired visibility claim.

## Rejected

- **Narrowing `src/topology/` (and the event funnel) to `pub(crate)`.**
  `EventSite` sits in the public signatures of the log API, at least four
  frozen `compile_fail` doctests depend on the public paths and are pinned to
  their failure reasons, the lib build would newly report the whole topology
  tree dead at `-D warnings`, and any such commit is a new candidate head
  that re-runs the suite, the eight gate artifacts, and the 66-unit coverage
  map — to purchase a guarantee `std::fs` refutes. If the owner wants the
  narrowing regardless, it is post-G2 managed debt for the PR12 activation
  slice, not a repair of this candidate.
- **A write-side inactivity guard in `append_topology`.** Strengthens a
  guarantee beyond PR7's frozen packet; same venue as above if wanted, and
  the legacy funnel's unvalidated schema field would still need its own
  treatment for the guard to mean anything.
- **Leaving the body sentence.** A false statement about the tree in the
  promotion PR of record.

## Cross-references

- `2026-08-25-checkpoint-merges.md` — the controlling condition; unchanged.
- `2026-08-31-g2-checkpoint-promotion.md` — obligation 6 and the addendum
  this record formalizes.
- `reviews/2026-08-31-g2-gate-report.md` — "Inert by default" §3–§4, the
  structural proofs and the report's own refusal of the false wording.
- `reviews/FINDINGS.md` §37 — the carried row amendment 1a requires.
- `2026-08-31-panel-seats.md` — the panel that confirms this premise on the candidate.
