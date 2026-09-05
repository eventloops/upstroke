# `src/engine/topology/preflight/tests.rs`

Extended notes for [`src/engine/topology/preflight/tests.rs`](../../../../../src/engine/topology/preflight/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The `RunnerPreflight` implementation, on its own.

`recover/tests.rs` drives this through the whole recovery order; these are
the claims about the pre-flight itself — what it probes, in which order,
which of its processes take a slot pair, and that both ledgers balance on
the refusal path as well as the successful one.

## `struct Recording {`

---------------------------------------------------------------------------
Doubles
---------------------------------------------------------------------------

## `struct Recording {`

A runner that records every request and can fail one program.

## `struct Recording` › `refuses: bool,`

Refuse to run at all — the shape a spawn failure has, as distinct from
a process that ran and exited non-zero.

## `struct Stub(&'static str);`

An adapter that runs one probe process and reports what it printed.

## `fn a_preflight_is_the_shell_probe_then_one_probe_per_recorded_agent() {`

---------------------------------------------------------------------------
What a pre-flight is
---------------------------------------------------------------------------

## `fn a_preflight_is_the_shell_probe_then_one_probe_per_recorded_agent() {`

One non-slotted shell probe, then one slotted probe per recorded agent, in
the recorded order.

Every clause of INV-23's sentence is a separate assertion, because they fail
independently: the count, the order, the shell going first, and the slot
asymmetry. Two agents rather than one, so "in the recorded order" has
something to be wrong about.

## `fn every_probe_carries_the_identity_the_preflight_mints_for_it() {`

The shell probe carries the identity [`PreflightIdentities::shell`] mints,
and each agent probe the one [`PreflightIdentities::agent`] mints.

Asserted as an equality against those constructors rather than against a
literal: the identities are what a container name is derived from, and a
test that spelled them out by hand would agree with a wrong implementation
as easily as with a right one.

## `fn a_failing_shell_refuses_before_any_agent_is_probed() {`

A failing shell refuses **before any agent is probed**, and names the image
the probe ran inside.

The second half is not decoration: "a recorded shell or agent CLI that fails
**inside the recorded image**" is a different fact from the same CLI failing
on the host, and an operator told only the first has nothing to fix.

## `fn a_failing_agent_cli_refuses_naming_the_agent_and_releases_its_slot() {`

A failing agent CLI refuses naming the agent, and every slot it took is
released.

The slot half is the one a `Drop`-based release would get wrong and a test
asserting only the message would miss: a pre-flight that refused while still
holding the pair would make the run's next slotted invocation refuse against
its own assertion.

## `fn a_refused_spawn_cancels_its_registration_rather_than_completing_it() {`

A runner that refuses to start a process at all settles the invocation as
**cancelled**, not completed.

R3's two lifecycle rows are "released on cancel" and "released on complete
or cancel", so the two are told apart on purpose. A ledger that completed a
process which never ran would balance and be wrong.

## `fn an_agent_with_no_adapter_refuses_naming_it() {`

An agent the record names and no adapter provides refuses, naming it.

`run_started(4).probed_agents` is the allow-list every binding is drawn
from, so an agent in it that this build cannot probe is a run that cannot be
certified — not one that quietly certifies fewer agents than it recorded.

## `fn a_slotted_invocation_without_an_agent_binding_is_refused() {`

A slotted invocation with no agent binding is refused, and its registration
is cancelled rather than left running.

Reachable only through a hand-built request — [`crate::agent::probe_request`]
always sets `agent` — which is exactly why it is worth holding: the pair a
slotted invocation takes is `{agent, pool?}`, and there is no agent to name.

## `fn one_identity_cannot_be_registered_twice() {`

Two invocations sharing one identity are refused: that is aliasing, not a
duplicate completion.

## `fn a_gate_invocation_through_the_boundary_takes_no_slot() {`

A gate identity acquires no slot pair, so the pre-flight's boundary does not
take one for it either.

The rule lives in [`is_slotted`], which is a total function of the identity;
this is the assertion that the boundary reads it rather than carrying a
second opinion.

## `fn a_gate_invocation_through_the_boundary_takes_no_slot()` › `agent: None,`

A gate carries no agent binding; a boundary that slotted it would
refuse here for want of one, which is what makes this assertion
sharp rather than incidental.

## `fn the_debug_rendering_names_the_boundary_and_not_the_ledgers() {`

The `Debug` rendering names what the pre-flight is for without printing its
ledgers.

Asserted because the impl is hand-written: a derived one would print the
`Mutex`es, and a `RunPreflight` in a refusal message would then carry every
invocation identity the run has issued.
