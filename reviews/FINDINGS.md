# Standing finding ledger

Every review finding across every slice, with its disposition and whether it has recurred.
Accumulative and append-only. This file is **an input to every review**, not a record written
after one.

Per-PR ledgers in pull-request bodies stay as they are — `validate-pr-body.sh` enforces them and
they bind that PR's merge. This file is their union, in the repository, so a reviewer can read what
has already been settled before spending effort re-deriving it.

## Why this exists

Reviewers re-raise settled matters because nothing tells them a matter is settled. On slice PR3, six
concurrent lenses returned 196 findings and two independent skeptics killed 114 of them — a 58%
noise rate, some of it re-litigating questions already answered in a previous slice's ledger, which
lived in a pull-request body no reviewer was given.

## The authority rule

**The implementer holds the disposition.** A reviewer may not overturn one.

A reviewer *may* **append a challenge** to a settled entry, and should when it has something the
original disposition did not consider. A challenge is only admissible with **new evidence**:

- a **concrete failure sequence** the disposition did not address, and
- a **surviving mutation** — a specific edit the current suite would not catch.

A restatement of the original finding is not a challenge and should not be filed. Neither is a
preference: where the design is frozen, an equally valid alternative is not a defect.

Challenges go in §3. The implementer adjudicates them and either revises the disposition — appending
a new row, never editing the old one — or records why the challenge fails. **The middle ground is
the implementer's call.** That is deliberate: the implementer has read the frozen packet for that
slice and carries the consequence of getting it wrong.


## The boundary rule — for every review, not just gates

**A boundary you would have drawn elsewhere is not a defect when the design is frozen.**

Every fix draws a boundary somewhere, and a boundary can always be measured against *some* sentence.
A reviewer who does not separate "the packet forbids this" from "I would have drawn the line
elsewhere" will generate findings indefinitely, because each repair creates fresh boundaries to
object to. On PR3 that loop ran for three consecutive rounds.

**The test is a single question: can you quote a *live* packet passage that the current behaviour
fails to satisfy?**

- **Yes → a defect**, even if the implementer drew the boundary deliberately and documented it.
  `PR3-ST14-006` is the worked example: round 5 asserted the deferred-state legal transition only
  *below* the trace ceiling and said so in a comment, but `decisions.bounded_census.coverage_assertions`
  says **every** state with a Deferred task has at least one legal next transition. No exception
  exists in the sentence, so the finding stands.
- **No → not a defect.** `PR3-ST07-014`'s general half is the worked example the other way: the
  reviewer asked for a cumulative durable prefix per site and phase, but
  `fault_injection_registry.structure` keys entries by `EffectSiteId × phase × order × injection
  mode` and nothing else. A cumulative prefix is not a function of that key. The repair declined it,
  gave that reason, and made the boundary an executable test. Correct.

**"Live" is load-bearing.** The packet carries fourteen generations of disposition history inline and
superseded rationale reads exactly like specification. `*_verification_dispositions`,
`finding_dispositions[].rationale` and `v4_`..`v15_` keys are history; `decisions.*`, `invariants`
and `transaction_fault_matrix` are live.

**And say which you found.** A documented, counted, bounded boundary is not a concealed gap. Round
5's ceiling skip carried its rationale in a comment, counted the skipped states, and asserted
`deferred_states > at_ceiling` so the skip could not grow silently. The finding was still right — but
"narrower than required" and "hidden defect" are different things, and reporting the first as the
second misdirects the repair.

## Recurrence

The schema already tracks it and it must be used:

- `Provenance: fix_regression` means this finding is a *regression of a previous fix*.
- `First bad / prior ID` names the earlier finding it recurs from.
- `Regression or documented guard` names the test that now prevents it.

A finding whose `First bad / prior ID` is populated has happened before. Two occurrences of the same
class is a signal about the method, not about the slice — `PR1-ORDER-001-ABA` is the worked example:
a sound finding whose *fix* had a hole, caught only by a later independent pass.

## 1. Settled — do not re-raise without new evidence

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| PR3-LIMITS-SCHEDULING | P3 | ae9e9da+ / src/topology/events.rs:429 | TopologyLimits omits max_per_agent and max_per_pool -> claimed to break the durable trace contract | introduced_by_feature | correctness | — | rejected: live decisions.resource_accounting names both "process-lifetime ephemeral scheduler state", and ephemeral state does not belong in a durable event; canonical_trace_projection says what a comparison ignores, not what the log contains | rejected |
| PR3-PATHSET-WIRE-KEY | P3 | ae9e9da+ / src/topology/paths.rs:194 | a serde alias lets an alternate PathSet payload key deserialize, and no test rejects it | introduced_by_feature | compatibility | — | rejected as stated: the packet never names PathSet or freezes its payload key, so the demanded refusal has no live basis. The underlying gap — encoding compared only against itself — is repaired under the wire-pinning class | rejected |
| PR3-SATISFIES-ORDER | P3 | ae9e9da+ / src/topology/fold.rs:2306 | satisfies compared as a sorted set would accept a reordered closure | undetermined | correctness | — | rejected: no live packet passage requires satisfies to be ordered, and decisions.repairs says nothing about it. Production is stricter than required (Vec equality is order-sensitive); the authoritative fixture closure is single-element so no test can distinguish a relaxation. Forward note, not a defect | rejected |

| PR3-CENSUS-SKELETON-SCOPE | P2 | ae9e9da+ / src/topology/census.rs | 11 of 28 catalogue mutations survive against the ST-14 census skeleton | introduced_by_feature | correctness | — | **DISPOSITION OVERTURNED.** Confirmation 3 deferred all eleven wholesale to PR10. Confirmation 4 found that unsound and it is: the frozen contract's proof_tests names "ST-14 skeleton incl. **totality**, the pre-budget_exceeded prefix, the **deferred-state legal-transition assertion**, and the **fast relations**" — so nine of the eleven attack obligations PR3 itself owes. Repaired in round 5 | fixed |
| PR3-CENSUS-ATTRIBUTION | P3 | ae9e9da+ / src/topology/census.rs | repair round 2 attributed the accepted/refused movement (8423->7751, 254413->255085) to "the six new refusals" | undetermined | docs-contract | — | the numbers are credible but the attribution is FALSE: the census does not generate attempt_interrupted, merge_verification_unavailable or question_answered. Recorded so the wrong cause is not carried forward; the orchestrator had already written the attribution into its ledger as fact | rejected |

## 2. Open — carried deliberately, with an owner

| ID | What | Owner | Why it is open |
|---|---|---|---|
| PR3-ATTEMPT-SHAPE | Whether `AttemptSettlement` can represent the frozen atomic `attempt_finished` incl. the allowance decision | project owner | Turns on whether `finding_dispositions[].design_changes` and `transaction_fault_matrix` impose field requirements on event shapes. `decisions.tests_acceptance.seam_tests[14]` is live and names `attempt_finished{Retained, Retry{resume:true}}`. Forward constraint on PR7/PR11 |
| PR3-RUNNER-DIGEST | The packet contradicts itself: `decisions.task_registry.validation_at_fold` requires the container image digest "when Container"; `INV-23` has it "when reported" | project owner | A Container run whose runtime reports no manifest digest is legitimate under one reading and refused under the other. PR3 implemented INV-23 consistently across A1 and A2 and said so per refusal |
| PR3-REG-001-CONDITIONAL | `A3-REG-001` is equivalent *for the current inventory*, because every constructible site exposes zero or one observable order | PR4-PR10 implementer | It becomes live debt the moment any site exposes more than one observable order. Conditional debt, not closed |
| PR3-WINDOWS-UNVERIFIED | Whether the fault-seam platform axis behaves correctly *at runtime* on Windows | CI matrix / whoever reads PR #18's checks | **Narrowed 2026-08-18.** `cargo check --target x86_64-pc-windows-msvc --all-features --all-targets` passes on the canonical tree, so the whole crate **including every test** type-checks for Windows — the "will not compile there" risk is closed locally. What remains is runtime only: whether `Host::current()` resolves to Windows and the `FIXTURE_SHAPES` assertions hold there. Undetectable on Linux because `cfg!(windows)` is false and both sides of the pin move together. The PR's `test (windows-latest)` cell is the remaining evidence; **if it is red, that is a genuine finding, not noise** |
| PR3-BEFORE-PHASE-SCOPE | Before-phase rows name the site's own artifact, not the transaction's whole durable prefix — so `Worktree.Add/Before` is empty although R9 already holds the intent | PR7–PR10 implementer | Chosen deliberately by repair round 4, documented on the type and asserted as a test so it reads as a decision rather than an omission. The repair itself names it as the largest remaining place a finding could live, in either direction |
| PR3-COMMIT-AUTHORSHIP | PR3's commit will be authored `Cameron Lambert <cameronlambert84@gmail.com>` (the repo-local git config) while the five commits beneath it on `codex/parallelism-design` are `tactus <tactus@tactus.local>` | project owner | Cosmetic and unenforced: no CI gate checks authorship and CONTRIBUTING has no sign-off requirement. The repo already carries four identities in normal use (Cameron Lambert 72, tactus 46, t 46, GitHub noreply 14). Left as configured rather than silently changed; overriding is one `git -c` flag if preferred |
| PR3-CONTAINER-START-ROW | `Container.Start → Present` is the least obvious row in the semantics table | PR6/PR7 implementer | Flagged by repair round 4 as the row most worth a second opinion |
| PR3-FRAMEWORK-SILENT-1 | Non-releasing removals leave `rows: []` — the packet fixes the pruning case (R27) but says nothing about removals with no objects to release | PR7–PR10 implementer | Derived by applying the pruning reading: the row that accounted for what was removed no longer holds it. After stays distinguishable from Before by artifact (`Removed` vs `Nothing`) and by action |
| PR3-FRAMEWORK-SILENT-2 | Read-only sites' After phase leaves nothing | PR7–PR10 implementer | Derived from the packet's "performs no effect", not stated by it |
| PR3-FRAMEWORK-SILENT-3 | `Container.Stop` is `Referenced` (only `Remove` ends a container); `Lock.ProbeCleanupExclusive` is `Referenced` | PR7–PR10 implementer | R17 accounts for the hold while held and is process-local OS state the kernel releases at death |
| PR3-FRAMEWORK-SILENT-4 | `Event.OpenLog`'s `Create` and `TruncateTornTail`: kill → `NextOpenConverges`, error-return → `RefuseResumably` | PR7–PR10 implementer | The packet elaborates only `SyncPrefix`, giving one action in both modes; this table gives one action in both modes by the same shape |
| PR3-FRAMEWORK-SILENT-5 | Windows and Unix containment kills get distinct actions (`AmbientHandleTerminates`, `ReaperSettlesGroup`) though the packet's residue answer is "none" for both | PR7–PR10 implementer | The mechanisms the packet states are different, and a table that merged them would survive a swap |
| PR3-REPORT-DOUBLE-NAME | `RunDir.WriteReport` and the `Report` group both name `report.json`, so ST-07 will demand two hook executions for one write | project owner | Found by A3, implemented as written and reported |

## 3. Challenges to settled entries

A reviewer appends here; the implementer adjudicates. New evidence only — a failure sequence the
disposition did not address, and a mutation the current suite would not catch.

*(none yet — but see §2 for the mechanism working in the other direction: the second
confirmation was asked a direct question about scope and answered it as a disposition, which is
now settled in §1.)*

## 4. Recurrence watch

Classes seen more than once. Two occurrences is a signal about the method.

| Class | Occurrences | Where | What it says |
|---|---|---|---|
| **A boundary drawn narrower than the packet's sentence** | 2 | `PR3-ST14-006` (round 5's trace-ceiling skip); `PR3-ST07-014` (round 4's site-artifact scope) | **Distinct from a fix that introduces a defect, and it should not be counted as one.** In both cases the round documented the boundary, gave a reason, and made it observable — round 5's skip carries its rationale in a comment, counts the skipped states, and asserts `deferred_states > at_ceiling` so the skip cannot grow silently. The finding is still real where a live packet sentence says otherwise (`coverage_assertions` says *every* state), but the failure mode is "narrower than required", not "concealed". **A reviewer must distinguish the two, or every fix generates a finding forever** — each fix draws a boundary and a boundary can always be measured against some sentence |
| **A fix that introduced a new defect** | **2** | `PR1-ORDER-001-ABA` (PR1); `PR3-ST07-011` and `-012` (PR3 round 3) | **The strongest argument for the independent final confirmation, and the reason round count is itself a risk.** PR1's was a fix *specification* with a hole. PR3's were fixes structurally right and wrong at the boundary: `semantics(Before)` returned empty rows, so the framework refused a packet-correct `[R9]` entry and accepted a false empty one — the exact inversion of its purpose. Guard adopted for round 4: for every change, state what the *new* code could get wrong and write the test that catches it |
| Tests satisfied by a correlated field rather than the named one | 11 (PR2) + 11 (PR3/A1) | PR2 registry tests; A1 fixtures | Fixtures must vary every independently meaningful field independently; assert hostility as distinct-value **counts**, not prose |
| A function used as its own expected-value oracle | 5 (PR3/A1) | `RunnerContract::kind`, `VerificationRecord::passed`, `GitPath::from` | Expected values come from the packet's text or an independent table, never from the function under test |
| A grid bounded short of its required domain | 8 (PR3/A1) | upgrade totality `to<=6`, reader selection, `is_topology_schema` | State what bounds each grid and why that bound is sound |
| Omitted packet-required fields | 7 (PR3/A1) | `RunStarted4.integration_ref`, `.execution_root` | **Mutation witnessing cannot detect omission.** Transcription slices need a reconciliation table against the packet's named enumerations |


## The hardening rule

**A finding that strengthens a guarantee beyond what the frozen packet requires is not a defect.**
It is recorded here as managed debt and scheduled, not repaired in the slice that surfaced it.

The test, applied per finding:

- **Defect** — a live `decisions.*`, `invariants` or `transaction_fault_matrix` passage says the
  current behaviour is *wrong*. There is a concrete failure sequence against the packet, and a
  mutation the suite does not catch. Repair it in-slice.
- **Hardening** — the current behaviour satisfies the packet, and the finding proposes a *stronger*
  property: a runtime check promoted to compile time, an invariant asserted from a second direction,
  a guarantee the packet never asked for. Record it here with an owner and a slice; do not repair it
  in the slice that surfaced it.

Two reasons this is the right cut. Round count is itself a risk — each repair round rewrites tests
and inverts assertions, and every rewrite is a chance to encode a defect as an expectation. And the
project already has the precedent: `ae9e9da` shipped naming PR2's remaining test-sufficiency debt in
its commit message rather than grinding a sixth round, and the handover records that as the right
call.

**Applies from PR3's third confirmation onward.** Authorised by the project owner, 2026-08-18.

| ID | Finding | Packet says current behaviour is wrong? | Disposition |
|---|---|---|---|
| *(filled as confirmations return)* | | | |

## 5. Fixed — recorded so recurrence is visible

A fixed finding is not a closed subject. It is recorded here with the guard that now prevents it, so
a later reviewer can tell a *new* defect from a *returning* one, and so a class that keeps coming
back is visible as a fact rather than a feeling.

| ID | Slice | What | Guard that now prevents it | Class |
|---|---|---|---|---|
| PR3-RUNSTARTED-FIELDS | PR3/A1 | `RunStarted4` omitted `integration_ref` and `execution_root`, both named by the packet in two independent passages | reconciliation of every event's fields against the packet's named lists | omitted packet-required field |
| PR3-STRICTNESS-RECURSION | PR3/A1 | `refusals[24]` not enforced recursively — 32 of 69 types carried `deny_unknown_fields`; `Answer4`, reachable from `question_answered`, did not | unknown-field injection at every reachable object path (384 paths) | recursive strictness |
| PR3-TOPOLOGY-PREDICATE | PR3/A1 | `is_topology_schema` compared with `>=`; `fold.rs:808` gates schema-4 admission on it, so a schema-5 run would be admitted | domain widened past the adjacent pair | bounded grid |
| PR3-UPGRADE-DOMAIN | PR3/A1 | upgrade-totality grid crossed destinations only to 6, so a guard bounded at 6 passed all 669 tests | grid extended past the implementation-chosen bound | bounded grid |
| PR3-SELF-ORACLE | PR3/A1 | completeness grid computed its expected contract/kind relation by calling `RunnerContract::kind()` — oracle and result moved together | expected values from the packet's text or an independent table | self-oracle |
| PR3-WIRE-PINNING | PR3/A1 | every serialization test consumed self-produced canonical JSON, so any symmetric rename survived | encoding pinned against independently written payloads | encoding compared to itself |
| PR3-FOLD-001..006 | PR3/A2 | six fold defects: blank committed lines skipped rather than refused; `max_defers` off-by-one; `binding_override` never checked against the frozen `HumanBinding`; `attempt_interrupted` leaving a generation open against `T-ATTEMPT`; `CandidatePrepared` unbound to the successful attempt; a second candidate silently overwriting the first | per-finding witnesses, each finding's own surviving mutation now dying | fold identity and refusal |
| PR3-BLOCKED-TRANSITIVE | PR3/repair2 | `blocked_tasks` walked the task list once in key order on "keys refer only backwards" — true for repairs, false for plan-ordered originals | fixed-point iteration; three-task chain witness | found while writing a witness for another finding |
| PR3-ST07-001..005 | PR3/A3 | five framework defects where the shipped implementation *was* a withheld catalogue mutation | each entry re-measured KILLED against the repaired tree | framework self-reference |

### What the recurrence data says so far

- **Fixture/oracle defects recur across slices** — 11 in PR2, 11 again in PR3/A1, in different code
  by different authors. That is a property of the method, and it is why hostility is now asserted as
  distinct-value **counts** and why a function may not be its own oracle.
- **Omission does not recur — it had never been looked for.** Mutation witnessing cannot detect a
  field that was never written, so no previous slice would have caught it either. The guard is a
  reconciliation table, not a better test.
- **Nine of PR3's fourteen second-round findings were predicted before the code existed** by withheld
  mutation catalogues. Authoring a catalogue and not measuring it is spend with no yield; measuring
  one and not triaging the survivors is worse.
