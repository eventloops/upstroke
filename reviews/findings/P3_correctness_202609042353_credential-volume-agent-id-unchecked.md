---
id: SWEEP-CONFIG-PARSE-010
severity: P3
disposition: deferred     # the map is recorded but no runner mounts it per agent in this build
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/config/parse.rs:137
provenance: pre_existing
first_bad:
guard: the change that first mounts `credential_volumes` per agent (the container funnel, `src/runner/container.rs`); `parse_runner` in `src/config.rs` is where the adapter registry would be threaded in
---

## Failure sequence

`[runner]` contains `credential_volumes = { claude-cod = "creds-cc" }` -> `read_runner`
checks only that the agent id and the volume name are non-blank -> `resolve_container`
checks that the volume `creds-cc` exists, which it does -> the record carries a credential
volume for an agent id no adapter has and none for `claude-code` -> when a runner mounts
volumes per agent, the `claude-code` attempt runs without its credentials and fails to
authenticate inside the container; the failure is loud but misattributed to the agent, and
the ladder escalates it.

Today the map is recorded (INV-23) and compared on resume, and no runner consults it by
agent id, so the wrong key changes no run. `[pools]` already answers the same question for
its `agent` key (warn, and mark the pool unusable, because §17's own example ships an agent
this build lacks); the credential map has no such precedent yet.

## What the change that takes this up should do

Thread `has_adapter` -- already injected into `load_captured_with` for `[pools]` -- into
`parse_runner` and `read_runner`, and decide by the `[pools]` precedent whether an agent id
with no adapter warns or errors; a volume is a control, so the default position is an error
naming the id and the adapters this build has, with the `[pools]` argument (a documented
example naming an absent agent) checked against `design/17` before choosing the softer
reading. Test both through `read_runner` with an injected registry.
