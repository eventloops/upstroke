## 19. Failure handling

| Failure | Detection | Handling |
|---|---|---|
| Agent binary missing / probe failure | pre-flight | refuse to start |
| Agent spawn error | engine | halt run (environment, not task) |
| Agent output stream read fails rather than ends, or the host refuses its reader thread | process funnel | the tree is terminated and the invocation returns the error, as an agent spawn error; a transcript cut short by a failed read is never handed over as complete, and an interrupted read is retried a bounded number of times |
| Agent non-zero / timeout | adapter | attempt failure; feedback = stderr/transcript tail |
| Rate-limited | adapter signal | pool marked exhausted; task deferred to reset or demoted per strategy (never below min) |
| Gate failure | gate runner | attempt failure; feedback = log tail |
| Review failure | verdict | attempt failure; feedback = required_changes |
| Chain exhausted | router | `Unblock` question to human (top rung); declined/CI → task Failed, dependents Blocked |
| Question parked, frontier non-empty | scheduler | continue independent tasks |
| Runnable frontier empty | scheduler | hard block (interactive) / end run reporting parked tasks (CI) |
| Budget or pool budget exceeded | ledger | stop scheduling; run ends `BudgetExceeded` |
| Merge conflict or code-attributed stale integration rejection (v0.2) | merge queue | publish nothing; atomically append rejection plus its replayable frozen Fix task, respecting hard pins/ceilings and the lineage-wide `max_merge_repairs`; infrastructure keeps its ordinary policy |
| Engine crash / power loss | — | `upstroke resume` replays the event log |
