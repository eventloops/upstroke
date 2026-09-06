---
id: SWEEP-HOST-NAMING-003
severity: P3
disposition: deferred     # deferred | accepted-risk — why it is still here
category: correctness
pr: 185
reviewed_sha: b9c73630f0228036ad9c0baeb344e36ec589ca69
location: src/runner/host.rs:178
provenance: pre_existing   # pre_existing | introduced_by_feature | fix_regression | undetermined
first_bad:                # predates the #111 split (6a969a33); exact origin not derived
guard: row 45's sweep of src/runner/host.rs, or the change that next touches the resolution memo
---

## Failure sequence

`HostRunner::program_for` (`src/runner/host.rs:170-186`) memoises program resolution per
`(program, PATH, PATHEXT)`. It stores a failure as `Err(error.to_string())` and, on a memo hit,
rebuilds it as `UpstrokeError::Refused { message }`. Two consequences, both outside this sweep's
assigned file:

1. **The typed error is flattened on the second call.** Row 42's repair made `resolve_program`
   return `UpstrokeError::Filesystem { operation: "stat", path, source }` when a candidate could
   not be decided, so the `io::ErrorKind` survives for a caller that wants to tell a permission
   problem from a broken mount. The first call gets that variant; every memoised repeat gets
   `Refused` carrying the same rendered text and no `#[source]` chain. The same flattening has
   always applied to every variant this function can return, so it is not new, but the repair
   gives it something to lose that it did not have before.

2. **A transient failure is cached for the life of the runner.** An `EIO`, a timed-out network
   mount or a directory that is briefly unreadable is recorded once and returned for every later
   spawn of that program at that boundary, even after the condition clears. Before the repair this
   was invisible — such a candidate was silently folded into "not found" and cached as a refusal —
   so the repair does not introduce the caching, it makes the cached thing honest about what it is.
   §7's "Retries are bounded, classify retryable failures" is the standard this sits against: a
   memo that cannot distinguish a permanent answer from a transient one has classified nothing.

Neither is reachable through `src/runner/host/naming.rs`: the memo, its key, and its
`Err(String)` round-trip are all the parent's, and `resolve_program` is a pure function of the
values it is handed.

## What the change that takes this up should do

Decide what the memo is for. If it exists to avoid repeating a filesystem search, it should cache
only answers the filesystem actually gave — `Ok(path)` and a `NotFound`-only refusal — and let an
undetermined result fall through to a fresh search next time. If it must cache failures whole,
it should hold the `UpstrokeError` itself rather than its rendering, so the variant and its source
survive the hit. Either way, state the chosen rule in
`docs/internals/runner/host.md` beside the memo, and correct row 45's `standards/SWEEP.md` entry
to say which it is.
