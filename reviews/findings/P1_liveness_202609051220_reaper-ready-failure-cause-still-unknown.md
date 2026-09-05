---
id: PR125-CLOSE-MACOS-READY-RED-CAUSE-UNKNOWN
severity: P1
disposition: deferred
category: liveness
pr: 154
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/agent/proc.rs:2461
provenance: undetermined
first_bad: PR125-CLOSE-MACOS-READY-RED-CAUSE-UNKNOWN
guard: preserve native failed-attempt evidence; PR 167 owns the coordinated startup diagnostic, and a demonstrated mechanism must select any repair
---

## Failure sequence

The parent starts the Unix cleanup reaper -> no READY is accepted -> the launch fails. The
retained macOS logs eventually collect exit status 1. They do not identify the child-side exit
site or timestamp that exit relative to the parent's deadline. Parent elapsed time does not
establish scheduling load as the cause.

This continues the existing P1 in `reviews/FINDINGS.md` section 49 and its PR #136 addendum.
Its stable ID, severity and unresolved disposition are unchanged. The historical addendum's
claim that status 1 proves exit before the deadline is too strong. Keep the closed historical
record intact and apply this precision correction to new evidence.

Selected September 5 failures, each with a 2-second budget and descriptor ceiling 10240:

| PR and head | Run, attempt, macOS job | Parent elapsed | Suite result |
|---|---|---|---|
| #154 d7e0c5d20d34220a27743616a56e8ff0de48095a | 33937147686, 1, 101227170458 | 2.001039541s | 1927 passed, 1 failed, 37 ignored |
| #145 84c21e01676c9e38c79321984441da86d220ce7e | 33936295713, 1, 101224794295 | 2.002052959s | 1917 passed, 1 failed, 37 ignored |
| #152 605e1fc23f6d2bd135cf94be8c9f39d6742397ea | 33957924675, 2, 101285329610 | 2.00181475s | 1916 passed, 2 failed, 37 ignored |
| #164 396a28b3d47f672a836eb79018e80b32e4198719 | 33939387531, 1, 101233664288 | 2.001712s | 1917 passed, 1 failed, 37 ignored |
| #167 10bca2056f104692d1ef1085e343422c9537651f | 33963296494, 1, 101298898354 | 1.999808667s | 1945 passed, 1 failed, 37 ignored |
| #167 10bca2056f104692d1ef1085e343422c9537651f | 33963296494, 2, 101300707489 | 2.001045542s | 1945 passed, 1 failed, 37 ignored |
| #152 4975a0a7dea973dd4112f3c0f2cc9165282ed581 | 33967513991, 1, 101310118921 | 2.001029208s | 1951 passed, 2 failed, 37 ignored |
| #149 8a5daaff9c99ffb61a8e28437c1072a1fda94a00 | 33968085235, 1, 101311639429 | 2.000738584s and 2.001296416s | 1945 passed, 2 failed, 37 ignored |

The #154 steward preserved and read the failed-attempt logs and metadata under its role
directory. Fresh retrieval of #154's old job ID returned later successful output, so the
preserved original is the failed-attempt evidence. #164 subsequently passed at a different
candidate, 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6, run 33963184374, attempt 1. That is not a
same-head recurrence rate. The selected failures above provide no population denominator.
The second failure at #152's earlier head is a separate commondir refusal. At its later head,
the other failure concerns budget-stop/index-lock handling and has separate provenance. The
later reaper occurrence ran at synthetic checkout b6191634c72d283780127c2174e727c5df7ec976.

The two #149 diagnostics occurred in one job, in
`an_exhausted_pool_and_a_silent_operator_still_terminate` and
`the_view_carries_the_exact_detached_head_and_index_of_the_worktree`. They are not two attempts.
Its sole unchanged retry, attempt 2, macOS job 101313849111, passed both tests with 1947 library
and 8 binary tests passing, 37 ignored. Both attempts used checkout
84a90396ecd6fca4d5e6a251fc76868f7a4eb5d3. This pair demonstrates outcome variation at one
checkout, without identifying a cause or establishing a population rate.

The steward's retained later logs have these SHA256 hashes:

| Evidence | SHA256 |
|---|---|
| #152 later failed attempt | 41890ecf7c3f540ba11fcd975eec94f0588a212ce34f03751810db1b4d3f7a19 |
| #149 failed attempt | 2e611d78ce7e0fd6b8f2f4db6e0ef138bda13f4340f51babc7242a77128fe09a |
| #149 passing retry | 8c8ab419b0ddb0fdb7c357ccb5f9a29ece422c703da7d5f7b202c0424a9dc3e4 |

Native job references are [#152's failure](https://github.com/eventloops/upstroke/actions/runs/33967513991/job/101310118921)
and [#149's failed attempt](https://github.com/eventloops/upstroke/actions/runs/33968085235/job/101311639429).

PR #154's drain workers start after reaper initialization. Its drain and helper-fixture repairs
do not change production startup and do not resolve this incident. #167 is the agreed carrier
for startup-specific stage/errno evidence and deterministic injected-stage tests. Diagnostic
instrumentation alone is not a repair verdict. Its existing failure teardown includes an
unbounded wait already recorded under PR125-CLOSE-UNBOUNDED-KILL-AND-WAIT-AT-FIVE-SITES.
The reported local diagnostic commit 2362ff1c6dd704eab243f7050c59d82576b12805 had local baseline
evidence, but this record claims no native result or resolving merged ancestry for it.

## What the change that takes this up should do

Preserve the child's fixed stage/errno and cleanup-lease index where available, and distinguish
the parent's timeout, EOF, invalid acknowledgement and read/poll failure. Failed observation
must stay unknown. Preserve post-fork safety and existing lifecycle boundaries. Obtain native
evidence for a specific mechanism before selecting a repair. Neither a larger budget nor
another blind retry follows from these logs. This record grants no exemption from required
green CI or the owner's P1 repair rule; landing disposition remains with the steward.
