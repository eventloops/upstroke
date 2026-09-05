---
id: PR154-DRAIN-CAPTURE-TAKEN-AT-GRACE-NOT-SURFACED
severity: P3
disposition: deferred
category: docs-contract
pr: 154
reviewed_sha: d3f0d18b595c143423e9d42d9462b0fa95f749e7
location: src/agent/proc.rs:111
provenance: pre_existing
first_bad:
guard: preserve the documented partial-output agent policy; any caller that requires complete output must use a byte-preserving capture and reject non-ended or limited captures, or receive an explicitly reviewed public API change
---

## Failure sequence

An escaped descendant retains an output writer after the direct child exits -> the post-exit
grace expires while the reader has no published EOF -> the agent API returns the captured
stdout/stderr prefix, without a public completeness flag. A caller of `ProcessOutput` cannot
infer EOF from those strings. This behavior predates PR #154 and is the
explicit agent-output policy in DESIGN §19, rather than an implicit promise of complete output.

This record originally accompanied pass 2's publication finding and proposed adding a public
field as its only remedy. The 2026-09-05 repair retains its stable ID and deferred history while
making the limitation explicit on `ProcessOutput`, `run_with_timeout_at`, and DESIGN §19. The
steward reaffirmed the existing policy after reviewing the current source and design. No current
agent caller requiring EOF has been demonstrated by this record. That is not a rejection of a
future concrete caller defect, and a valid P2 or reproducible witness still requires repair.

Internally, `Drain::collect_bytes` now preserves exact bytes and exposes `limited` and `ended`.
The ordinary text collector uses that same lifecycle and performs its existing lossy decoding.
Cancellation is distinct from EOF, including the reproduced race where a released reader
finishes before the supervisor takes its capture.
The joined nonblocking worker now reports a returned poll failure even if cancellation races
its publication. That repaired failure race is separate from the remaining lack of a public
EOF flag.

## What the change that takes this up should do

For a caller that needs complete binary output, consume the internal raw capture and reject
non-ended or limited output before constructing a successful result. PR #145 owns its Git
consumer and raw exit status; PR #154 supplies only the drain boundary and does not depend on
that unmerged caller. If an agent adapter requires EOF, document that concrete requirement and
review its error handling or a public `ProcessOutput` extension together. Adding a public field
requires a compatibility assessment, but compatibility is not grounds to leave a demonstrated
correctness defect unfixed.
