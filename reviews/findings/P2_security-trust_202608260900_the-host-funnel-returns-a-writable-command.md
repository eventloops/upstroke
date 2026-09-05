---
id: PR5D-FUNNEL-RETURNS-A-COMMAND
severity: P2
disposition: deferred
category: security-trust
pr: 5
reviewed_sha: 
location: src/runner/host.rs
provenance: pre_existing
first_bad: 
guard: the slice that owns `src/runner/**` (PR6/PR7 implementer)
---

## Failure sequence

`runner::host::build_command` is `pub(crate)` and returns a `std::process::Command` to the
rest of the crate — a writable handle. `decisions.effect_site_inventory.mechanism` (2) requires
every funnel module to perform effects only inside site-taking APIs and never to return writable
handles, and `src/runner/host.rs` is named in that list. A caller holding the returned `Command`
can add arguments, change the environment or spawn it without passing any funnel site, so the
guarantee the mechanism states does not hold for this module.

## What the change that takes this up should do

Move spawn construction inside the funnel so no writable handle crosses the boundary.
`agent::proc` and `agent::bin` both consume the `Command` the funnel hands out, so this is an
architectural change and not a line-level one, and `src/runner/**` is frozen under the 2026-08-20
owner ruling — which is why PR5 could not make it. The interim mitigation stands and should be
kept until then: `upstroke::runner::host::build_command` is on the denylist, so every caller has to
be allowlisted, and `src/agent/bin.rs` sits in the enumerated legacy section where the debt is
visible rather than convenient.

Recorded in `reviews/FINDINGS.md` §8. It was missed by both full-ledger audits (§35's 52 and §38's 26), so it is carried here on its own row text.
