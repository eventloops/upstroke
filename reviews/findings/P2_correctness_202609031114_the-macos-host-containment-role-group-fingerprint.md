---
id: W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha:
location: 
provenance: pre_existing
first_bad:
guard: project owner / the slice that next opens the pre-exec containment path, once a controlled macOS environment can measure it
---

## Failure sequence

`runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` fails intermittently on macOS with *"`<role>`: the child did not lead its own process group, so the pre-exec containment step did not run for this role"*, and a `test result: FAILED. <n> passed; 1 failed` whose passed-count tracks whichever head it ran on. **Twelve sightings across six branches between 2026-09-01 and 2026-09-03**, every one confirmed by its own `... FAILED` line in its own job log rather than by a mention. **Three are on `master` itself**, and the two earliest — runs `33503020178` and `33535107935`, 2026-09-01 — are at `src/runner/host.rs:5574:13`, the pre-extraction location, so **the failure predates the W2 programme and the `W2-` prefix records when it was found, not when it began**. The rest sit at `src/runner/host/tests.rs:4220:9`, `:4227:9` or `:4229:9`, which is the same assertion relocated by successive splits

## What the change that takes this up should do

Owner, as the ledger records it: project owner / the slice that next opens the pre-exec containment path, once a controlled macOS environment can measure it.

**Open as an unexplained observation, not classified as a flake or regression.** Not diff-caused, on the cleanest counterfactual this programme has produced: `c30aca0`'s delta from `9a7fc22` is `reviews/`-only, `9a7fc22` was green (run `33776069960`, attempt 1) and `c30aca0` is red — the same tree with a markdown file added. Independently, #108 does not touch `runner::host` at all. **The failing role varies across three roles** — `probe(claude-code)` six times, `review` four, `implement` twice — **and one run settles what that means**: run `33777752620` is red on both attempts at the identical commit, naming `probe(claude-code)` then `review`. Direct evidence that any role can lose, consistent with a race in the pre-exec `setpgid` path rather than with anything specific to a role. Whether this is a face of `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` is **open**; the signatures differ and they are deliberately not merged on family resemblance, because that row's repair makes the question answer itself — if this shape stops recurring on heads carrying it, it was the same defect. **Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.** Full evidence: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
