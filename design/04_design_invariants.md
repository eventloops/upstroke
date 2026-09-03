## 4. Invariants

1. **Agents edit files; the engine owns git.** Agents are instructed never to commit. The engine creates branches, stages, commits, and (v0.2) merges.
2. **The engine never speaks HTTP.** All model interaction happens inside agent subprocesses.
3. **Ground truth is the diff, not the transcript.** Gates check, reviewers judge, and feedback quotes `git diff` captured by the engine.
4. **Every state transition is an event.** State is derived by replaying `events.jsonl`. Resume = replay + continue.
5. **Official CLIs only.** No ToS-violating proxies, ever — the trust wedge is part of the product.
6. **Questions never stop the runnable frontier.** A question parks exactly the tasks it affects; the run hard-blocks only when nothing remains runnable.
7. **Capacity is estimated conservatively.** Safety margins on every pool, a reserve floor for the user's own interactive use, and rate-limit signals treated as ground truth over any estimate.
