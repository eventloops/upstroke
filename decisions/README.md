# decisions/

Dated decision records: verdict first, the reasoning that earned it, measured vs.
assumed named explicitly, rejected options recorded with why.

The contract that keeps this folder safe:

- **DESIGN.md remains the only living authority for product design.**
  Records here are history, not spec. When a record's outcome changes the spec,
  DESIGN.md gets the compressed edit at the time of the decision, citing the record.
- **One decision per file**, named `YYYY-MM-DD-<slug>.md`. Do not accumulate
  addenda about unrelated decisions in one file. This is not filing tidiness: an
  append-only ledger in a single file conflicts on every concurrent branch, and
  it did — two branches open on 2026-08-11 both appended an "Addendum D" and an
  "Addendum E" with unrelated content, turning a documentation merge into manual
  reconciliation. Separate files merge without touching each other.
- **Records are immutable once landed.** Corrections and follow-ups are dated
  sections appended to *their own* record, never silent edits. A record whose
  conclusion is later overturned says so and links forward; it does not get
  rewritten to look right.
- **Design documents do not live here.** A proposal reaches this folder only
  as a decision record citing the proposal and its critiques as inputs.
  Proposals filed before 2026-08-27 are in [`proposals/`](../proposals/README.md);
  new ones are filed privately ([2026-08-27](2026-08-27-proposals-private.md)),
  and a record may cite one without reproducing it. (Convention since
  2026-08-13; before that, drafts stayed outside the repo entirely.)
- **Cross-link freely.** A decision that constrains another should say so in both
  directions.

When design work runs through upstroke itself, council ledgers land as run
artifacts (§15); records promoted here are the durable subset.

## Index

- [2026-08-11 — multi-model design council](2026-08-11-design-council.md): adopt
  the council manual-first, ≤3 family seats, critique-heavy; machinery deferred.
- [2026-08-11 — self-hosting v0.2](2026-08-11-self-hosting-v02.md): v0.2
  development runs through upstroke; the claim is auditable from commit tags.
- [2026-08-11 — gate config across a resume](2026-08-11-resume-gate-config.md):
  resume runs the gates the record carries, warning on config drift; verified live.
- [2026-08-11 — Codex reasoning effort](2026-08-11-codex-reasoning-effort.md):
  every Codex review had run at `low`; effort is now a routing axis, verified live.
- [2026-08-11 — decision export schema](2026-08-11-export-decisions-schema.md):
  local schema-2 JSONL/CSV projection, one row per recorded worker attempt.
- [2026-08-12 — v0.2 merge queue and execution topology](2026-08-12-merge-queue-execution-topology.md):
  schema-4 immutable candidates, exact-tree verification, crash-safe CAS
  integration, bounded human-gated repair tasks, and the shared worktree/runner
  boundary.
- [2026-08-20 — the automated review gate](2026-08-20-automated-review-gate.md):
  single reviewer every head, three-model panel once on the merge candidate; S9's
  remit moves to it. Stage 1 (comment-only) authorised; auto-merge is not, and the
  reviewer's credential separation is advisory, not enforced.
- [2026-08-20 — what invalidates a frontier review](2026-08-20-review-invalidation-scope.md):
  reviews bind to the exact tree modulo an explicit exempt set — exactly
  `reviews/FINDINGS.md` to start; ancestor + exempt-only diff re-attests via owner
  dispatch, computed on the trusted side. Ends ledger edits discarding max-effort
  reviews of unchanged code.
- [2026-08-21 — slices land as pull requests into their integration branch](2026-08-21-stacked-slice-prs.md):
  slice PRs into `codex/parallelism-design` get CI, policy, and a single-reviewer
  review of each head; attestation stays master-only and happens once on #18's
  merge candidate. Merge commits only — a rewrite orphans ledger rows.
- [2026-08-22 — the strategy layer lives outside the public repository](2026-08-22-strategy-record-private.md):
  competitive analysis, kill criteria, positioning and the commercial path are
  maintained privately; `DESIGN.md` keeps stubs plus the engineering consequences;
  promotion is demand-driven, by the pull request that first needs to cite a document.
- [2026-08-23 — retire the App-signed attestation gate](2026-08-23-retire-app-attestation.md):
  the `upstroke-frontier-review` App check, its two privileged workflows and the
  signing environment are retired; the ruleset requires `upstroke-ci` and
  `upstroke-pr-policy` only. The review obligation is unchanged and the owner's
  merge is the attestation; 2026-08-20 §5 remains the bar for any automated return.
- [2026-08-24 — the PR3-layer freeze: charter, adjudication, and the G2 pass](2026-08-24-pr3-layer-freeze-charter.md): three
  deviation classes charter what the freeze admits; PR7's `fold.rs` footprint is
  blessed as a disclosed deviation; the pass over the layer runs before PR8.
- [2026-08-25 — `CommandSpec.program` stays `String`](2026-08-25-commandspec-program-stays-string.md):
  closes `PR4-PROGRAM-PATH-NOT-UNICODE` as not reproducible in production. Every production
  route puts a bare CLI name in the field, so `DESIGN.md:222` is unchanged and the W4 widening
  is withdrawn; the boundary stays path-capable, and `CODING_STANDARDS.md` §1's known-conflict
  block retires on its own motion.
- [2026-08-25 — integration merges happen at attested checkpoints](2026-08-25-checkpoint-merges.md):
  `codex/parallelism-design` merges to `master` at the G2 checkpoint and again at
  v0.2 completion, rather than once at the end.
- [2026-08-26 — the retry brief survives a crash](2026-08-26-durable-retry-feedback.md):
  `FailureRecord` gains one additive `#[serde(default)] detail`, carrying §11.4's
  feedback onto the durable record; the schema-4 brief becomes a fold over it,
  derived once and called by both the live loop and a replay. `SCHEMA_VERSION`
  unmoved. Class C exception to the 2026-08-20 freeze, scoped to that field.
- [2026-08-31 — the G2 checkpoint promotion candidate](2026-08-31-g2-checkpoint-promotion.md):
  reconciles `2026-08-25-checkpoint-merges.md` obligation by obligation and leaves it
  controlling in full. The ledger audit and the recurrence review are discharged, and the
  serialized suite ran green at the committed evidence head; the panel and the six captured gate
  artifacts, plus macOS and Windows, remain owed. Inertness is
  verified by construction, no `0.2.0` tag is authorized, and rollback is
  `git revert -m 1 MERGE_OID` — deliberately a high bar, because re-promotion then
  requires reverting the revert. A fourth addendum records the step-1 collision
  ruling, includes artifact 7 in the owed captured set, and binds this record to
  correction by appended erratum.
- [2026-08-31 — the inertness premise is behavioural](2026-08-31-inertness-premise-behavioural.md):
  the G2 inertness condition is behavioural and holds at the pre-assembly baseline; the
  `pub(crate)` visibility form is retired as false and unachievable. A library consumer can
  WRITE schema-4 state through the checked funnel, carried as a ledger row for the PR12
  activation slice. No visibility change to the code is authorized.
- [2026-08-31 — the G2 panel's three seats](2026-08-31-panel-seats.md): one seat per family —
  `gpt-5.6-sol` at max, `claude-fable-5` at max explicitly pinned, `gemini-3.1-pro-high` via
  `agy` by absolute path — each with its invocation guard. No pre-authorized fallback: one
  repair, then wait; the panel never convenes partially.
- [2026-09-01 — the build-box tree lives outside the public repository](2026-09-01-infra-private.md):
  the 18-file `infra/` tree relocates to the private companion repository —
  operator tooling, not engine contract; the 2026-08-22 keep-public floor is
  untouched, the next publish can no longer package the tree (no published
  crate ever did), and the private intake landed on the companion default
  branch, verified byte-for-byte by tree ID, before removal.
