## 19. Failure handling

| Failure | Detection | Handling |
|---|---|---|
| Agent binary missing / probe failure | pre-flight | refuse to start |
| Agent spawn error | engine | halt run (environment, not task) |
| Agent output read fails, a reader panics, or the host refuses its thread | process funnel | settle the process tree and report the reader failure as an invocation error. Parent endpoints use nonblocking I/O and retry consecutive interruptions a bounded number of times. After the post-exit grace, release and join the reader before taking its capture. Classify a returned poll failure before cancellation; early supervisor exits retain secondary worker failures in the invocation error. Release does not mean EOF. The agent API retains bounded partial strings without a public completeness flag. The internal byte-preserving collector also reports the byte limit and observed EOF, for callers that require complete binary output |
| Agent stdin write fails, its feeder panics, or the host refuses the feeder thread | process funnel | settle the process tree and report the feeder failure as an invocation error. A broken pipe accepts the child's refusal of remaining input; other write failures, invalid write counts and excessive consecutive interruptions are errors. Feed stdin concurrently with output capture through an owned nonblocking endpoint. Collection and drop release, wake and join the feeder. Classify returned failures before cancellation and preserve secondary failures on early supervisor exits. After the post-exit grace, remaining input delivery is best-effort. Joining requires scheduling and finite local operations, without waiting for an escaped descendant to consume bytes or close its endpoint |
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
