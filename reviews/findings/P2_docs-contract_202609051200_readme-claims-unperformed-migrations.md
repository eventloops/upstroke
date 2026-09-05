---
id: ASTRA165-003
severity: P2
disposition: deferred
category: docs-contract
pr: 165
reviewed_sha: 9699c27396e6eff34be9a86bc634f042080cb280
location: docs/internals/README.md:67
provenance: introduced_by_feature
first_bad: —
guard: Compare the README claims with the candidate's actual notes files, test include paths, and comment-strip assertions
---

## Failure sequence

The README claims that container and event-log tests now read notes and that strip
checks use planted controls. The candidate lacks `docs/internals/runner/container.md`
and `docs/internals/events/log.md`. The container test still reads source at
`src/runner/container/tests.rs:888`; the build-refusal extractor still reads rustdoc
at `src/events/log/tests.rs:3679`. The strip checks still require `stripped > 100` at
`src/agent/mod.rs:1329` and `src/runner/host/tests.rs:6870`. A maintainer following the
README looks for absent inputs and is told the live-comment dependency is removed.

## What the change that takes this up should do

Correct the README to describe the current inputs and mark those migrations
pending. The review established the absence and source reads directly on the
reviewed SHA; green CI does not establish that the migrations described here ran.
