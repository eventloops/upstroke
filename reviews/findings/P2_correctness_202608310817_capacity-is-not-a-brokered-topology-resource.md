---
id: PR5-CAPACITY-NOT-A-TOPOLOGY-RESOURCE
severity: P2
disposition: deferred
category: correctness
pr: 5
reviewed_sha:
location: src/capacity.rs:376
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

Whether agent-model **capacity** — the provider window a worker or reviewer spends against — is a resource the parallelism topology *brokers*, or ambient state it discovers by failing. Today it is ambient. The three ceilings are parsed, validated and carried (`src/config.rs:439-447`), and two of them already say "acted on by the topology engine", but none of them is a *provider* budget: they bound how many attempts run at once, not how much window remains to spend. The only capacity feedback the engine has is retrospective — `capacity::retire_signals` marks a pool exhausted **after** an attempt came back `RateLimited` (`src/capacity.rs:376`), and the ladder then defers without spending an attempt (`ladder::rate_limits_defer_without_spending_an_attempt`). Nothing admits work *against* a budget, and no topology row models a permit

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Deferred to PR11 deliberately, not overlooked.** Three reasons, in ascending weight. (1) **The packet is frozen and the freeze is the method.** A capacity permit is a new row in a frozen contract. The owner ruling of 2026-08-20 held the line on `PR4-PROGRAM-PATH-NOT-UNICODE` and `PR4-DESIGN-ROLE-SCOPED-ENV` — two findings that violate *live passages* — rather than edit a frozen file. Amending the packet for a finding that violates no live passage, while those two stay accepted deviations, is the inconsistency a reviewer sees first. (2) **There is nothing yet to model it in.** PR11 is where the coordinator brokers concurrency; a permit is that same shape, so building one before the broker exists means inventing a second mechanism PR11 must then reconcile or discard. The ledger already places it there: `PR3-LIMITS-SCHEDULING`'s disposition rests on live `decisions.resource_accounting` naming `max_per_agent` and `max_per_pool` "process-lifetime ephemeral scheduler state" — a permit is that same kind of state, so it belongs in the scheduler PR11 builds and not in the frozen durable contract. (3) **The data to specify it does not exist.** DESIGN.md:656 (§23.2) records what the first real runs measured, and the capacity side of that is a single usage-limit event across five slices — not a distribution a fault row can be written against. **What is worth doing before PR11, and touches nothing frozen:** (a) make capacity exhaustion *distinguishable in the record at the launcher* — inside the engine `FailureKind::RateLimited` is classified and durable, but an agent invoked outside it that hits a provider limit and one that dies leave the same trace, which is why ruling limits out after the PR4 deaths took a transcript grep rather than a query; (b) carry provider identity as configuration rather than an ambient credential file — needed anyway for the cross-vendor reviewer, and the same seam `PR4-DESIGN-ROLE-SCOPED-ENV` names from the environment side (`CREDENTIAL_LOCATIONS`, DESIGN.md:260). Both produce the measurement (3) is missing, so PR11 can specify against evidence instead of intuition. Forward constraint on PR11, carried the way `PR3-ATTEMPT-SHAPE` is

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
