# `src/runner/host/tests.rs`

Extended notes for [`src/runner/host/tests.rs`](../../../../src/runner/host/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `#![allow(`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
carries this file's own review clause. This is the extracted test module of
the Process funnel in `src/runner/host.rs` -- the region that lived inline
there, under that file's own inner allow of these same three lints, moved out
unchanged. The set of calls permitted here is unchanged by the move: the
fixtures build and tear down scratch trees, mark programs executable, and
spawn real child processes, exactly as they did inline. What moved is where
the permission is stated, not what it permits.

`PR6-LANEF-004`: it states that level **of its own** rather than inheriting
one. A lint level is scoped by the MODULE TREE and not by the file, so an
out-of-line child of a funnel is covered by the parent's allow unless it says
otherwise, and the funnel's child-module census requires each child to say so.
All three are needed here and each was measured at extraction; the counts are
in the review clause.
`decisions.effect_site_inventory.mechanism` (2).

## `use std::time::Duration;`

The split moved `SHELL_PROBE_TIMEOUT` -- the parent's one use of `Duration` --
into `super::probe`, so the parent's import list no longer carries the type
and `use super::*` no longer supplies it here. Named directly rather than
kept as a re-export the parent does not need. No test is renamed, no
assertion changes and no body moves; this line is the whole of what the
extraction owes this file.

## `fn os(value: &str) -> OsString {`

-----------------------------------------------------------------------
helpers
-----------------------------------------------------------------------

## `fn gate_invocation() -> InvocationId {`

A gate identity, for the tests whose subject is not the identity.

## `fn shell_probe_invocation() -> InvocationId {`

The pre-flight shell probe's identity: the packet's third form, target
`Shell`.

## `fn worker_invocation() -> InvocationId {`

One attempt's worker identity — the packet's first form, role `worker`.

## `fn review_invocation() -> InvocationId {`

One review pass's identity — the same form, role `review_pass0`.

## `fn fixture_agent() -> AgentId {`

The agent a worker or a reviewer is bound to.

`ExecutionRole::all` names the same adapter for its agent-probe target,
so the grid's three slotted roles all carry a real, shipped id — which
is what `host-v1` looks up a credential location by.

## `fn synthetic_base() -> Vec<(OsString, OsString)> {`

A base written by the test rather than read from the machine, so a
composition fixture asserts the same thing on every developer box.

## `fn native() -> ShellKind {`

The shell every platform has, invoked exactly as `gates` invokes it.

## `fn new_resolves_the_same_record_as_resolve_host() {`

-----------------------------------------------------------------------
policy
-----------------------------------------------------------------------

## `fn environment_composition_fixtures() {`

-----------------------------------------------------------------------
environment composition fixtures  (proof test: "environment composition
fixtures")
-----------------------------------------------------------------------

## `fn environment_composition_fixtures()` › `struct Fixture {`

Every fixture is (role, agent, overlay, what the composition must
say). The expected values are written from DESIGN.md:258-264, not
read back out of `compose`.

## `fn environment_composition_fixtures()` › `("CLAUDE_CONFIG_DIR", None),`

The base carries all three; a gate is repository code
and receives none of them.

## `fn environment_composition_fixtures()` › `("CODEX_HOME", None),`

A claude-code worker is not told where codex's or
copilot's credentials live.

## `fn environment_composition_fixtures()` › `("PATH", Some("/usr/bin:/bin")),`

A reviewer runs an agent CLI, so it is supplied the
unscoped values too.

## `fn environment_composition_fixtures()` › `("CLAUDE_CONFIG_DIR", None),`

The host knows no credential location for this agent, so
it supplies none — and the base's three do not leak in
its place.

## `fn environment_composition_fixtures()` › `let roles: BTreeSet<String> = fixtures.iter().map(|f| f.role.label()).collect();`

Fixture hostility as distinct-value counts, not as a comment.

## `fn environment_composition_fixtures()` › `let mut seen: Vec<&OsString> = Vec::new();`

One variable, one entry: a duplicated key is an environment
whose meaning depends on which end the child's runtime reads.

## `fn a_reserved_key_the_base_does_not_carry_is_not_supplied()` › `let environment =`

"set but empty" and "unset" are different environments, and CLIs
read them differently.

## `fn a_reserved_key_in_the_overlay_is_a_preflight_error() {`

The refusal is `expected_failures_refusals[0]`: "reserved env conflict
-> pre-flight error".

## `fn a_reserved_key_in_the_overlay_is_a_preflight_error()` › `let expected_reserved = [`

Written out from DESIGN.md §8's runner contract for role-scoped HOME, PATH,
and credential locations. The capacity module's credential-profile
documentation, `src/capacity.rs` and its notes pointer where present, names
`COPILOT_HOME` and `CLAUDE_CONFIG_DIR`. The third adapter fixture names
`CODEX_HOME`. The expected table does not read `reserved_keys()`.

## `fn a_reserved_key_is_refused_at_every_position_in_the_overlay() {`

Refused **wherever it sits in the overlay**.

Every reserved-key fixture in this suite hands `compose` a one-pair
overlay, so the conflicting pair is always `overlay.first()` — and a
scan that stopped after the first pair is indistinguishable from a full
one given those inputs. An adapter's overlay is not one pair:
`invariants_introduced[0]` says "reserved keys refused pre-flight",
which is a claim quantified over the whole vector.

The grid is the positions a conflict can occupy in an overlay of four:
first, two interior, and last.

## `fn a_reserved_key_is_refused_at_every_position_in_the_overlay() {` › `let harmless: Vec<(String, String)> = HARMLESS`

The control: four harmless pairs compose, so a refusal below is
the reserved key and not the shape of the overlay.

## `fn a_reserved_key_is_refused_at_every_position_in_the_overlay() {` › `assert_eq!(refusals, 6 * 4 * KeyCase::ALL.len());`

Six keys x four positions x two key cases, counted so a grid that
quietly stopped traversing fails here rather than passing smaller.

## `fn an_overlay_that_restates_a_reserved_key_with_the_runners_own_value_is_still_refused() {`

Refused by **key**, whatever the value — including the runner's own.

`invariants_introduced[0]` is "reserved keys refused pre-flight" and
`expected_failures_refusals[0]` is "reserved env conflict -> pre-flight
error naming the key". Neither says "unless the adapter agrees with the
runner today": an overlay allowed to restate `PATH` because the value
happens to match is an overlay that has taken ownership of the key, and
it breaks silently the day the runner's value changes. Every other
reserved-key fixture supplies a *different* value (`/tmp/hijacked`,
`/nowhere`, `C:\hijack`), so equality is the one case they cannot see.

## `fn windows_treats_reserved_keys_case_insensitively_and_unix_does_not() {` › `let overlay = vec![("Path".to_owned(), "C:\\hijack".to_owned())];`

The platform axis is a value here, not a `cfg!`. Both arms are
reached on every host, so a Linux cell proves the Windows rule.

## `fn windows_treats_reserved_keys_case_insensitively_and_unix_does_not() {` › `let composed = insensitive`

And the same-key rule is what decides an upsert, not only a refusal.

## `fn probe_and_execution_compose_the_same_environment() {`

DESIGN.md:263: "Probe and execution compose the same base, mounts,
reserved values, and overlay, so pre-flight certifies the environment
that will actually spend."

## `fn environment_dump_helper() {`

Prints this process's whole environment between two markers.

## `fn dumped_environment(runner: &HostRunner, request: &RunnerRequest) -> Vec<String> {`

One run of the dump helper, as the environment its child actually
carried.

## `fn a_probe_child_and_an_execution_child_of_one_adapter_carry_the_same_environment() {`

A probe child and an execution child of one adapter, held side by side.

DESIGN.md:263 — "Probe and execution compose the same base, mounts,
reserved values, and overlay, so pre-flight certifies the environment
that will actually spend." Every probe fixture in this suite probes one
agent in isolation and asserts only that the probe succeeded, and the
one composition comparison compares two *maps* rather than two children
— so the probe's base and the probe's overlay could each be replaced on
the way to the process and nothing would notice.

Two children, one adapter, one runner, at the same time. The probe half
is built by production (`agent::probe_request`), not by this test, so a
substitution made there is inside the comparison. The two sentinels
exist because equality between two empty environments is also equality:
one comes only from the base, the other only from the overlay, and both
must arrive.

## `fn a_probe_child_and_an_execution_child_of_one_adapter_carry_the_same_environment() {` › `let execution = crate::runner::worker_request(`

The worker half comes from production's builder, so the request this
compares against the probe is the request an attempt sends: the
bound agent and the worker identity, not a hand-written pair.

## `fn every_credential_supplied_role_composes_one_environment_per_binding() {`

The same claim, over **every** role production binds to an agent and
**every** shipped binding — in the children, not in a map.

DESIGN.md:258-264: "each supplies role-scoped `HOME`, `PATH`, and
credential locations", and "Probe and execution compose the **same**
base, mounts, reserved values, and overlay, so pre-flight certifies the
environment that will actually spend."

The test above holds one pair — `Probe(Agent(claude-code))` against
`Implement` — and `supplies_credentials` names **three** roles. So a
forwarding site that dropped the binding for `Review` alone left every
child-level comparison green: direct `compose` tests bypass the
forwarding site entirely, the actual-child parity compares Probe with
Implement, the credential-child test compares Gate with Implement, and
the Review cells of the role grid never look at their environment
(`PR5-CORRECTNESS-009`). The domain is therefore taken from
`ExecutionRole::all()` and filtered by `supplies_credentials`, so a role
added later is covered or fails here.

Three sentinels rather than equality alone, because two identical
*absences* are also equal: one value that can only have come from the
base, one that can only have come from the overlay, and the credential
location, which can only have come from the **binding** — which is the
one the finding is about, and the one a stripped binding removes.

## `fn every_credential_supplied_role_composes_one_environment_per_binding() {` › `for (_, key) in CREDENTIAL_LOCATIONS {`

A credential location already in the base, which is the failure
sequence's own starting state: composition strips reserved keys, and
only the agent binding can put the value back.

## `fn every_credential_supplied_role_composes_one_environment_per_binding() {` › `assert!(`

The binding's own contribution. `key` is a reserved key, so a
child carrying it carries a value composition put there —
and a role whose binding was dropped on the way to `compose`
carries nothing under this name at all.

## `fn the_reserved_values_every_role_gets_are_the_host_boundarys_own() {`

What `host-v1` supplies for `HOME`, `PATH` and `USERPROFILE`, asserted
from the passages that decide it.

DESIGN.md:260 names three things role-scoped — `HOME`, `PATH`, and
credential locations — and `host-v1` scopes one of them. That is a
boundary, so it is asserted here from the sentences that draw it and
not as a count of distinct values. The difference is the whole point of
this test's shape: the form it replaces asserted "`PATH` took exactly
**one** value across the five roles", a claim no passage makes, which
would fail the container runner and which made *implementing*
DESIGN.md:260's plainest reading a test failure. A repair round that
encodes a narrower boundary as the expected result is the shape this
project fears most, and a count cannot tell the two apart.

Three claims, one per passage:

* **Every role is supplied every one of the three keys the base
  carries.** DESIGN.md:260 says the runner *supplies* them; a role
  handed none at all satisfies any count of distinct values.
* **The roles the packet pairs are supplied identical reserved sets.**
  DESIGN.md:263 ("probe and execution compose the same base, mounts,
  reserved values, and overlay") over `probe(<agent>)`, `implement`,
  `review`; `decisions/2026-08-12-…:331-333` ("gate-shell/program
  availability is checked inside the same boundary") over
  `probe(shell)` and `gate`.
* **The value is the host boundary's own.** `decisions/2026-08-12-…:321`
  — "the host base starts from the Upstroke process environment". The
  expected values are read out of the base vector *this fixture wrote*,
  never out of [`HostEnvironment::lookup`]: `reserved_values` calls
  `lookup` too, and a function used as its own oracle asserts nothing.

What this test could get wrong: the two groups could stop covering the
five roles — a role dropped from both would be unasserted while every
remaining assertion passed — so their union is compared against
`ExecutionRole::all()` as a set, and not merely counted.

## `fn the_reserved_values_every_role_gets_are_the_host_boundarys_own() {` › `let agent = AgentId::new(claude::ADAPTER_ID);`

The agent `ExecutionRole::all()` names, so the groups below can be
compared against it as sets rather than by length alone.

## `fn the_reserved_values_every_role_gets_are_the_host_boundarys_own() {` › `let from_the_boundary = ["PATH", "HOME", "USERPROFILE"];`

Written here, not read from RESERVED_ALWAYS: the set is the claim.

## `fn the_reserved_values_every_role_gets_are_the_host_boundarys_own() {` › `let in_the_base = |key: &str| -> OsString {`

The independent oracle: the base this fixture wrote.

## `fn the_reserved_values_every_role_gets_are_the_host_boundarys_own() {` › `let certified: &[(&str, Vec<ExecutionRole>)] = &[`

The roles each passage holds together, and the passage, so a failure
names the sentence that was broken.

## `fn credential_locations_are_role_scoped() {`

The half `host-v1` **does** scope: credential locations.

The expected split is written from what each role executes — a worker, a
review and an agent probe run an agent CLI; a gate is repository code
and the shell probe is a shell — and not computed from
`supplies_credentials` or from `is_slotted`.

## `fn compose_gives_a_child_the_credential_location_of_its_own_agent_and_no_other() {`

The same split, asserted about the **environment the child receives**
rather than about the list the runner assembled.

`reserved_values` is what the runner *supplies*; `compose` is what the
process *gets*, and until this test the two were only related by
inspection: `compose` cloned the whole Upstroke base first, so a
`CODEX_HOME` in the coordinator's own environment reached a gate
whatever `supplies_credentials` said. DESIGN.md:258-264 scopes the
credential location by role, and the role a variable is scoped to is
the one the process runs under.

The grid crosses every role with every agent — including the one the
host knows no location for — under both name rules, and the expected
value of each of the three credential keys is written from the rule
("the bound agent's location, and only for a role that runs an agent
CLI"), not read from `supplies_credentials` or `reserved_values`.

## `fn compose_gives_a_child_the_credential_location_of_its_own_agent_and_no_other() {` › `let runs_an_agent_cli = |role: &ExecutionRole| match role {`

Written from what each role executes, exactly as
`credential_locations_are_role_scoped` writes it.

## `fn compose_gives_a_child_the_credential_location_of_its_own_agent_and_no_other() {` › `for (key, expected) in [`

And the keys that come from the host boundary itself are
still there, for every role: DESIGN.md:262 names HOME
and PATH beside the credential locations, and `host-v1`
supplies the boundary's own value of each to all five
roles (see
`the_reserved_values_every_role_gets_are_the_host_boundarys_own`
for the passages that decide that).

## `fn compose_gives_a_child_the_credential_location_of_its_own_agent_and_no_other() {` › `for (key, expected) in [("LANG", "C.UTF-8"), ("UPSTROKE_RUN", "01ABCDEF")] {`

Nothing else was dropped on the way through.

## `fn compose_gives_a_child_the_credential_location_of_its_own_agent_and_no_other() {` › `assert_eq!(supplied_count + denied_count, 2 * 5 * 5 * 3);`

Hostility as counts: 2 rules x 5 roles x 5 agents x 3 keys = 150
decisions, and both answers really occur.

## `const BASE_WITNESS: &[(&str, &str)] = &[`

The composed base **is** the Upstroke process environment, entry for
entry.

DESIGN.md:258-259: "`CommandSpec.env` overlays a runner-owned base
rather than replacing it. The host runner starts from the Upstroke
environment". `run` calls `env_clear()` before installing the composed
block, so anything `from_process` fails to collect is not merely
unscoped — it is *gone* from every child, and nothing else in the suite
would notice. That includes the entries only one platform has: Windows'
`=C:`-style per-drive working directories are yielded by `vars_os`
(Rust 1.85 keeps keys beginning with `=`), and this asserts they
survive collection on the platform that has them rather than assuming
it.
The variables the subprocess witness below inherits, and their values.

Values are deliberately awkward — a non-ASCII character, an `=`, and
the encoding's own separator — so a collector that split or filtered on
any of them loses the entry.

The second name is not decoration. `PR4-CORRECTNESS-006`'s surviving
mutation drops exactly that key inside `from_process`, and a filter
keyed on a name **no test process carries** removes nothing: it is
inert, and no oracle can observe an inert edit. Setting the name here is
what makes it a variable that exists and can therefore be lost. The
entry-for-entry equality below is the general statement; this is the
named guard for the one mutation that motivated it.

## `fn base_witness_helper() {`

The child half of [`the_base_of_a_process_environment_is_the_process_environment`].

A subprocess rather than `set_var`: the suite is multi-threaded and
mutating this process's environment while another test reads it is the
race `std::env::set_var` is `unsafe` for. The same shape
`proc::tests::sigchld_reaper_host_helper` uses, for the same reason.

## `fn base_witness_helper()` › `let expected: Vec<(OsString, OsString)> = std::env::vars_os().collect();`

The general statement, made where the environment is *known* to
carry awkward entries rather than wherever the machine left it: any
entry `from_process` drops, filters or reorders shows up here,
whatever it is named.

## `fn base_witness_helper()` › `let composed = base`

And each injected entry reaches a composed child environment, for a
role that is handed nothing else.

## `fn the_base_of_a_process_environment_is_the_process_environment() {`

The composed base **is** the Upstroke process environment, entry for
entry.

DESIGN.md:258-259: "`CommandSpec.env` overlays a runner-owned base
rather than replacing it. The host runner starts from the Upstroke
environment". `run` calls `env_clear()` before installing the composed
block, so anything `from_process` fails to collect is not merely
unscoped — it is *gone* from every child, and until this test nothing
in the suite would notice: every composition fixture supplies its own
base through `with_base`.

Two halves, because one alone is not enough. The equality is over
whatever this machine happens to carry — on Windows that includes the
`=C:`-style per-drive working directories, which Rust 1.85's `vars_os`
deliberately yields. The subprocess adds an entry chosen to be
awkward, so the equality is not satisfied merely by an environment with
nothing interesting in it.

## `fn the_base_of_a_process_environment_is_the_process_environment() {` › `"runner::host::tests::base_witness_helper",`

The **full path**. `--exact` matches the whole test name, so a
bare `base_witness_helper` filters all 932 tests out and the
child exits 0 having run nothing — a subprocess witness that
witnesses nothing, and a green one.

## `fn the_base_of_a_process_environment_is_the_process_environment() {` › `assert!(`

Assert the **count**, never a bare `ok`: `ok. 0 passed` is what a
filter that matched nothing prints, and it is indistinguishable from
success at the exit code.

## `fn the_credential_location_is_the_bound_agents_and_no_others_value() {` › `for (agent, key, expected) in [`

An independent table maps each adapter id to its config-directory variable.
The capacity module's credential-profile documentation, `src/capacity.rs`
and its notes pointer where present, supplies `COPILOT_HOME` and
`CLAUDE_CONFIG_DIR`. The Codex fixture separately names `CODEX_HOME`.

## `struct ParityFixture {`

-----------------------------------------------------------------------
supervision parity  (proof test: "supervision parity tests")
-----------------------------------------------------------------------

## `struct ParityFixture` › `floor: Option<Duration>,`

The shortest this child can possibly have taken, as a fact about the
child rather than about the machine that runs it: a sleeper cannot
finish before it has slept, and a child killed for exceeding its
timeout ran at least that long. `None` where the fixture finishes as
fast as the machine can run it and no floor is stateable.

This is the lower half of the duration pin. The upper half is wall
clock the test measures around each call, and neither is the
runner's own arithmetic.

## `fn parity_fixtures() -> Vec<ParityFixture>` › `script: "echo problem 1>&2",`

`1>&2` redirects the same way in `cmd` and in `sh`.

## `fn parity_fixtures() -> Vec<ParityFixture>` › `name: "a quoted argument survives the spec",`

The regression test for `build_command`'s `cmd.exe` rule:
std would re-quote this tail as `\"quoted arg\"`, which
cmd.exe does not un-escape, so the child would print
something else entirely. On Unix nothing special happens and
the fixture still has to agree.

## `fn parity_fixtures() -> Vec<ParityFixture>` › `name: "a sleeping child is measured, not the timeout",`

The fixture that pins duration **from below** without naming
the timeout. Every other fixture finishes as fast as the
machine can run it, so a duration reported too *large* is
caught by the elapsed bound while one reported too *small* is
caught by nothing: `> 0` admits a nanosecond.

A sleeping child gives the test a floor it can state in
advance and that holds on any machine — a loaded one only
makes the interval wider, never narrower. One second nominal,
asserted at half, because `ping -n 2` is "two pings a second
apart" rather than a sleep of exactly a second.

## `fn parity_fixtures() -> Vec<ParityFixture>` › `"ping -n 2 127.0.0.1 > NUL"`

`cmd` has no `sleep`; this is the shape the timeout
fixture below already relies on.

## `fn parity_fixtures() -> Vec<ParityFixture>` › `floor: Some(Duration::from_millis(400)),`

The timeout, written again rather than read from the field
beside it: a child killed for exceeding its timeout ran at
least that long, and the two values are the same number for
that reason rather than by construction.

## `fn supervision_parity_tests()` › `let direct_started = std::time::Instant::now();`

Wall clock around the call, measured by the test. It is an upper
bound on any honest `duration` *by construction*: the funnel
starts its own clock after this instant and stops it before the
drain, so `duration <= elapsed` holds however slow or loaded the
machine is. See the duration assertions below.

## `fn supervision_parity_tests()` › `assert_eq!(actual.stdout, expected.stdout, "{}: stdout", fixture.name);`

Byte for byte, including the line endings. Both sides run the
same script through the same recorded shell on the same machine,
so there is nothing legitimate for a normalization to absorb —
and normalizing *both* sides is how a runner that rewrote CRLF to
LF would have gone unnoticed on the only platform that produces
CRLF at all. `invariants_preserved[0]` says output capture is
unchanged, and rewriting it is a change.

## `fn supervision_parity_tests()` › `for (what, measured, bound) in [`

Duration is part of the record an attempt keeps
(`Outcome.duration`), so "supervision unchanged" includes
measuring **this child** rather than reporting some other true
fact. Not compared to the other supervisor's duration for
equality: two runs of one script take two times, and asserting
they match would be asserting the machine is idle.

Pinned from both sides instead, against two oracles that are not
the runner's own arithmetic:

* above, wall clock the *test* measured around the call — an
  upper bound by construction; and
* `fixture.floor`, the child's own behaviour, where the fixture
  has one to state.

Positivity alone is what let `.map(|mut output| { output.duration
= request.timeout; output })` survive a whole review round
(`PR4-CONF-013`): every ordinary fixture has a 30s timeout and
reports `> 0` under it, and the timeout fixture reports exactly
its timeout, which its own floor admits. The elapsed bound is
what that mutation cannot satisfy — a child that echoes and exits
has not taken thirty seconds.

## `fn supervision_parity_tests()` › `assert!(codes.len() >= 3, "distinct exit codes: {codes:?}");`

Hostility as counts: a parity suite whose fixtures all produce the
same output proves that two functions agree about nothing.

## `fn supervision_parity_tests()` › `assert_eq!(`

And the duration pin is counted rather than hoped for: two fixtures
state a floor, one from a sleep and one from a timeout kill. A grid
that lost them would still assert `> 0` on every row and read green.

## `const TRANSPARENT_STDOUT: &[&str] = &[`

-----------------------------------------------------------------------
output transparency  (`invariants_preserved[0]`: "output capture …
unchanged")
-----------------------------------------------------------------------

## `const TRANSPARENT_STDOUT: &[&str] = &[`

What the transparency shim prints on stdout: JSON Lines, because that is
the shape whose *first* lines carry the meaning — a Codex transcript's
`thread.started` (the session) and its `item.completed` (the verdict)
both precede the final `turn.completed`.

## `const TRANSPARENT_STDERR: &[&str] = &["tracing line one", "tracing line two"];`

And on stderr, which is captured by the same funnel and is where a Codex
run puts its tracing log.

## `fn transparency_shim(dir: &Path, name: &str) -> String {`

A launcher that ignores its arguments and prints
[`TRANSPARENT_STDOUT`] and [`TRANSPARENT_STDERR`].

Not [`forwarding_shim`]: that one forwards to this test binary, whose
`libtest` output is what it is. This child's output is chosen by the
test, so "what the child produced" and "what the runner returned" are
two things that can be compared.

Every payload byte is `echo`-safe in both dialects — JSON carries none
of `&`, `<`, `>`, `|`, `^`, and a `"` is printed literally by `cmd`'s
`echo`.

**The redirection goes first on Windows.** `cmd`'s `echo` prints
*everything* between the command and the redirection operator, so
`echo foo 1>&2` emits `foo` followed by a **trailing space** — measured
on the guest, where the first run of this grid failed with
`["tracing line one "]` against `["tracing line one"]`. `1>&2 echo foo`
has no such gap. A test that trimmed instead would have stopped being
able to see a runner that trimmed.

## `fn captured_lines(stream: &str) -> Vec<String> {`

A captured stream as lines, with the platform's terminator folded away.

## `fn transparency_run(agent: &str, resume: Option<&str>) -> crate::agent::TaskRun {`

One `TaskRun`, so each adapter's own `build_args` can be asked what
production's argument vector for it is.

## `fn the_runner_returns_the_childs_whole_output_for_every_production_request_shape() {`

`HostRunner::run` hands back **the child's whole output**, for every
request shape production sends.

`invariants_preserved[0]` is "process supervision, timeout, **output
capture**, adapter parsing unchanged", and `PR5-CORRECTNESS-012` is a
runner that keeps only the last stdout line when the role is
`Implement`/`Review`, the agent is `codex` and the first argument is
`exec` — after which a successful review loses its session and its
verdict and is re-asked, and can end as `ReviewFailed`.

Three axes, varied independently, because a suppression can key on any
of them and the existing grids hold two of them fixed:

* **role** — built by production's own builder, never by this fixture;
* **agent binding** — all three shipped ids, not one;
* **the argument vector** — each adapter's real one, from its own `pub
  fn build_args`, so `exec`, `-p` and the bare-prompt form all appear.
  *Every* existing grid in this file sends `["--exact", NO_SUCH_TEST]`,
  which is why an `args[0]`-keyed edit had nowhere to fail. That is
  `PR4-CONF-006`'s class one field further over, and this is the field.

The resumed shape is carried too: `codex exec resume <id>` moves the
subcommand's position, so a check on `args[0]` and one on "is this an
exec" are different predicates.

## `fn the_runner_returns_the_childs_whole_output_for_every_production_request_shape() {` › `for adapter in crate::agent::ADAPTERS {`

The probe role too: pre-flight reads a CLI's own answer, and a
truncated one is a capability read wrong rather than lost work.

## `fn the_runner_returns_the_childs_whole_output_for_every_production_request_shape() {` › `let script = if cfg!(windows) {`

And the unbound role, whose program is the recorded shell rather than
a located CLI — the other half of the program-shape partition.

## `fn the_runner_returns_the_childs_whole_output_for_every_production_request_shape() {` › `TRANSPARENT_STDERR`

Redirection first, for the trailing-space reason on
`transparency_shim`.

## `fn the_runner_returns_the_childs_whole_output_for_every_production_request_shape() {` › `assert_eq!(`

Hostility as counts, not prose.

## `fn build_args_for(id: &str, resume: Option<&str>) -> Vec<String> {`

Each adapter's production argument vector, from the adapter itself.

## `fn assert_transparent(cell: &str, output: &ProcessOutput) {`

Every line the child wrote came back, in order, on the stream it was
written to.

## `fn assert_transparent(cell: &str, output: &ProcessOutput)` › `assert!(`

Named separately, because "the last line survived" is the assertion a
truncating runner would still pass.

## `fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {`

The bounded capture allowance is the same one on both public funnel
entries, and it is the real one.

`invariants_preserved[0]`: "process supervision, timeout, output
capture and adapter parsing unchanged". `HostRunner` reaches the funnel
only through `run_with_timeout_hooked`, so a limit passed there and
nowhere else is a limit no parity fixture sees: every existing
output-limit test calls the private `run_with_timeout_and_limit` with a
64 KiB test value. This one drives the *real* constant through both
public entries with a child that never stops writing.

The expected bound is written here (16 MiB per stream) rather than read
from `proc`, so raising the constant to `usize::MAX` — or to anything
else — fails here rather than agreeing with itself.

## `fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {` › `const EXPECTED_LIMIT: usize = 16 * 1024 * 1024;`

`proc::OUTPUT_LIMIT_BYTES`, transcribed. Per stream.

## `fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {` › `const HELPER_BUDGET: usize = 64 * 1024 * 1024;`

How much the helper writes before exiting, as a decimal byte count.

Four times the allowance, and **finite**. A funnel that bounds
correctly kills the child while it is blocked on a full pipe, far
short of this, so the passing case is unchanged. A funnel that does
not bound captures 64 MiB and the child exits 0 — which fails
`output_limited` below by name, instead of running the parent out
of memory and taking the whole test binary with it. A budget set
*below* `EXPECTED_LIMIT` would fail this test's own
`output_limited` assertion, so it cannot drift silently.

## `fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {` › `let mut direct = Command::new(&exe);`

(a) The direct entry point, which is what the legacy engine used
before this slice.

## `fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {` › `let workspace = scratch("output-limit");`

(b) The same child through the Runner.

## `fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {` › `env: vec![(`

Not a reserved key: the helper reads it to decide it is the
helper rather than an ordinary test run, and to size its
output.

## `fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {` › `assert!(`

And it really did have to *reach* the allowance: a limit of zero
would also satisfy the bound above.

## `fn the_runner_executes_in_the_requested_workspace()` › `let template = shell.command("echo here > marker.txt");`

A relative redirect target resolves against the child's cwd in both
shells, which is the point of the fixture.

## `fn a_gate_child_is_told_where_no_agents_credentials_live() {`

The role scoping is in the child's own environment block, not only in
the vector the runner assembled.

A gate is repository-controlled code — "the one thing on the host that
no agent permission surface bounds" — and DESIGN.md:262 scopes the
credential location by role. This runs a real process under a base that
carries all three locations and asks the child what it received.

## `fn a_gate_child_is_told_where_no_agents_credentials_live()` › `let runner = HostRunner::new().with_environment(HostEnvironment::with_base(`

A base that carries every credential location, so "absent from the
child" cannot be an artefact of this machine.

## `fn a_gate_child_is_told_where_no_agents_credentials_live()` › `.filter(|(name, _)| !KeyCase::current().same_key(name, OsStr::new("PATH")))`

The shell must still be findable, so the real PATH wins over
the synthetic one.

## `fn a_gate_child_is_told_where_no_agents_credentials_live()` › `for value in [`

Windows `echo` prints the literal `%VAR%` when the variable is unset;
`sh` prints nothing. Both are "not set to a credential directory",
and neither is the value the base carries.

## `fn a_gate_child_is_told_where_no_agents_credentials_live()` › `let worker = runner`

The same base, the role that runs codex's CLI: its own location
arrives and the other two still do not. Without this half the
assertion above would be satisfied by supplying nothing at all.

## `struct StubRunner(Box<dyn Fn() -> Result<ProcessOutput, UpstrokeError> + Send + Sync>);`

-----------------------------------------------------------------------
the shell probe
-----------------------------------------------------------------------

## `struct StubRunner(Box<dyn Fn() -> Result<ProcessOutput, UpstrokeError> + Send + Sync>);`

A runner that never spawns, so the probe's *classification* of a
failure can be tested on a machine where every `ShellKind` happens to
be installed.

## `fn a_shell_probe_that_did_not_exit_zero_is_a_preflight_error_however_it_failed() {`

Every way a `ProcessOutput` can say the probe did not succeed.

`expected_failures_refusals[3]` is "a failing shell probe -> returned
pre-flight error to the caller", and the funnel has three independent
ways to report one: the exit code, `timed_out`, and `output_limited`.
Two of them can arrive **with** `code: Some(0)` — the limit is observed
during the final drain, and a signal-killed child reports `code: None`
with `timed_out: false` — so a probe that reads only one field
certifies a shell that did not run `exit 0`.

The grid is written from those three fields rather than from
`run_shell_probe`'s branches, so a field it stops reading fails here.

## `fn a_shell_probe_that_did_not_exit_zero_is_a_preflight_error_however_it_failed() {` › `name: "killed, with no exit code and no timeout",`

Killed by a signal, or by anything else that leaves no exit
code: `None` is not `Some(0)` and must not be read as one.

## `fn a_shell_probe_that_did_not_exit_zero_is_a_preflight_error_however_it_failed() {` › `name: "output-limited after exiting zero",`

The bounded-output contract terminated the owned tree; the
funnel can still report the code the leader exited with,
because the limit is observed during the final drain.

## `const MISSING_SHELL: ShellKind = ShellKind::Pwsh;`

The recorded shell the missing-shell case probes for.

`pwsh` rather than one of the other four because it is the one
[`ShellKind`] that is **not** reachable from any directory the child's
program search consults once `PATH` has been replaced: `cmd` is in the
Windows system directory always, `powershell` is in a subdirectory of it
that is on every Windows `PATH`, `sh` and `bash` are `/bin` programs on
Unix and Git-for-Windows programs on the runners. PowerShell 7 installs
outside all of those on every platform — and the helper asserts that,
rather than assuming it.

## `const MISSING_SHELL_MARKER: &str = "UPSTROKE_MISSING_SHELL_PROBE";`

Set by the parent test on the helper it spawns, so the helper is inert
when `cargo test -- --ignored` runs it directly.

## `const MISSING_SHELL_OK: &str = "<<MISSING-SHELL-REFUSED";`

Printed by the helper after it has asserted, so the parent can tell "the
helper ran and refused" from "the helper never ran".

## `fn windows_program_search_dirs() -> Vec<PathBuf> {`

The directories a child's program search consults **besides** `PATH`,
on the platform that has any.

std resolves a bare Windows program name against the child `PATH`, the
application directory, the system directory, the Windows directory and
then the **parent's** `PATH` (`library/std/src/sys/.../windows/process.rs`,
`search_paths`). The helper controls both `PATH`s by construction; these
three it can only *check*, which is what makes the check the premise.

## `fn shell_probe_missing_shell_helper() {`

The missing-shell half of
[`host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing`],
in a child process because one of the two `PATH`s that decide the answer
belongs to the **process**, not to the request.

The previous version of this case hid `pwsh.exe` by composing a base
environment with `PATH` removed and ran in-process. Windows CI proved
that oracle invalid — `CreateProcess` also searches the *parent's*
`PATH`, so an emptied child `PATH` hides nothing the runner itself can
see, and the guest passed only because that machine has no `pwsh` at
all. A process cannot rewrite its own `PATH` for one test without racing
every other test in the binary, so the absence is constructed where it
can be: in a child whose entire `PATH` is one directory this suite
created and asserts is empty.

Everything here is a **premise check followed by the claim**. If the
construction ever stops constructing the absence — a `PATH` that is not
the empty directory, a directory that is not empty, a `pwsh.exe` that
has appeared in one of the three directories the search reaches
regardless — this fails on the premise and says which one, rather than
passing for the wrong reason.

## `fn shell_probe_missing_shell_helper()` › `let path = std::env::var_os("PATH").expect("the parent supplies a PATH");`

Premise 1: this process's `PATH` is exactly one directory, and that
directory is empty. On Unix this is the whole search — `execvp`
consults `PATH` and nothing else, and an *absent* `PATH` would be
worse than a controlled one, because then `execvp` falls back to the
confstr default `/bin:/usr/bin`, where the CI image really does ship
`/usr/bin/pwsh`.

## `fn shell_probe_missing_shell_helper()` › `for dir in windows_program_search_dirs() {`

Premise 2: the directories Windows searches whatever `PATH` says do
not hold this shell either.

## `fn shell_probe_missing_shell_helper()` › `let workspace = scratch("missing-shell");`

Premise 3: the workspace **exists**. The three conditions the
contract's proof test composes are an existing workspace, an absent
shell and `HostRunner::shell_probe`; a probe that failed because its
directory was missing would prove the first of them false and test
something else. `HostRunner::run` hands every child an absolute
`current_dir`, so this is the difference between "the shell is not
there" and "the directory is not there".

## `fn shell_probe_missing_shell_helper()` › `let error = HostRunner::new()`

The claim. `HostRunner::new()` composes from *this* process's
environment, so the child inherits the same one-empty-directory
`PATH` — production's own composition, not a substituted one.

## `fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {`

`decisions.pr_sequence[5].slice_contract.proof_tests[8]`, by name.

The contract names one test for the whole shell probe and it composes
three things: the **recorded shell** succeeding, a shell that is
**missing** failing, and both going through
[`HostRunner::shell_probe`] — the `RunnerPreflight` entry point — rather
than through the free [`run_shell_probe`] or through `Runner::run`.
Decomposing it into separately-tested layers loses exactly that
composition: with the missing-shell case gone, a `shell_probe` body of

```text
match run_shell_probe(self, shell, workspace.to_path_buf(), invocation) {
    Err(error) if workspace.exists() && error.to_string().contains("os error 2") => Ok(()),
    outcome => outcome,
}
```

survives every remaining case — (a) succeeds, (c) has no workspace, (d)
does not use the method, and (e) does not spawn.

## `fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {` › `runner`

(a) The recorded shell, actually spawned. `gates::shell_available`
is a PATH check; this is a spawn, which is the only thing that
establishes availability (packet finding F-43 / V14-VERIFY-004).

## `fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {` › `let empty_path_dir = scratch("empty-path");`

(b) A recorded shell that is **missing**, with the workspace that
(a) just used still in place, through the same method — the
contract's own composition. It runs in a child because the absence
has to be constructed out of both `PATH`s and one of them is the
process's; see [`shell_probe_missing_shell_helper`], which holds the
assertions.

## `fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {` › `let absent = workspace.join(format!("absent-{}", crate::ulid::ulid()));`

(c) and (d): the other two ways the host can fail to complete a
probe, neither of which is a claim about what happens to be
installed. Both fail because of something this test constructs and
then checks, so they fail identically on a machine with every shell
in existence installed.

## `fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {` › `let absent = workspace.join(format!("absent-{}", crate::ulid::ulid()));`

(c) The recorded shell, asked to run in a directory that is not
there. `HostRunner::run` gives every child an absolute
`current_dir`, and starting a process in a directory that does not
exist is refused by the kernel everywhere this crate runs: `chdir`
answers `ENOENT` on Unix — whether std reaches it through `fork` or
through `posix_spawn_file_actions_addchdir_np` — and
`CreateProcessW` fails with `ERROR_DIRECTORY` on Windows. Both
surface as `Err` from `Command::spawn`, which is the same production
path an absent shell binary takes, and the caller must be handed a
pre-flight error naming the shell rather than an `Ok`.

## `fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {` › `let mut request = shell_probe_request(native(), workspace.clone(), shell_probe_invocation());`

(d) The missing-program fault at the `Runner::run` layer, expressed
so that no installed program can satisfy it: an absolute path inside
the directory (c) just established does not exist. It contains a path
separator, so it is looked up verbatim — neither `execvp`'s `PATH`
walk nor std's Windows search of the system directories and the
parent `PATH` is consulted for a name like this one, and there is
nothing at the name. (b) is the same fault one layer up, through the
method and against the recorded shell, which is what the contract
names; this one pins the layer beneath it.

## `fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {` › `let refusing = StubRunner(Box::new(|| Ok(stub_output(Some(127), false))));`

(e) A shell that runs and refuses, and one that hangs. Both are
pre-flight errors and neither is a `ProcessOutput` the caller has to
interpret.

## `fn the_shell_probe_spells_every_shell_the_way_gates_do() {`

The probe spells every shell exactly as `gates::ShellGate` does, so it
certifies the invocation the gates will use.

## `const WINDOWS_POINTS: &[SubEffectPoint] = &[`

-----------------------------------------------------------------------
containment sub-effect hook points (ST-07 subset)
-----------------------------------------------------------------------

## `const WINDOWS_POINTS: &[SubEffectPoint] = &[`

The eight points and the host each belongs to, transcribed from
`decisions.effect_site_inventory.containment_sub_effects`:

> Windows: Spawn.AmbientJobJoined …, Spawn.CreatedSuspended …,
> Spawn.PrivateJobAssigned, Spawn.Resumed … ; Unix: Spawn.ReaperStarted
> …, Spawn.PreExecPgidAndRegister …, Spawn.Exec, Spawn.Registered

## `fn observed_points(runner_hooks: HarnessHooks) -> BTreeSet<SubEffectPoint> {`

Run one trivial command through the runner and report which containment
points its funnel reached.

## `struct OrderedHooks(Arc<Mutex<Vec<SubEffectPoint>>>);`

Records the points a spawn reaches **in the order it reaches them**.

## `fn point_order() -> Vec<SubEffectPoint> {`

The containment points of one spawn, in order.

## `fn the_containment_points_of_a_spawn_fire_in_the_packets_order() {`

The points fire in the packet's own order, and each label sits at the
coordinate it names.

`containment_sub_effects` lists them as a sequence — "Windows:
Spawn.AmbientJobJoined …, Spawn.CreatedSuspended …,
Spawn.PrivateJobAssigned, Spawn.Resumed …; Unix: Spawn.ReaperStarted …,
Spawn.PreExecPgidAndRegister …, Spawn.Exec, Spawn.Registered" — and the
order is the whole content of two of them: `PrivateJobAssigned`
promises the child is *still suspended* and `Resumed` promises it is
not. A set-valued observation cannot tell those two apart, so swapping
the two labels in production would leave the ST-07 evidence claiming a
pre-resume coordinate for a hook that fires after the child can execute.

The expected sequence is transcribed from that sentence, not read back
from the funnel.

## `fn the_containment_points_of_a_spawn_fire_in_the_packets_order() {` › `vec![`

`AmbientJobJoined` is a write-command startup step rather than a
per-spawn one, so a spawn reaches the other three.

## `fn the_containment_points_of_a_spawn_fire_in_the_packets_order() {` › `assert_eq!(`

Each point once: a point consulted twice at one spawn would make the
sequence above ambiguous about which coordinate it names.

## `fn windows_containment_points_execute_and_unix_points_do_not() {` › `for point in &[`

`AmbientJobJoined` is a write-command startup step, not a per-spawn
one, so it is not reached by running a command; it is exercised by
`windows_ambient_job_unavailable_refuses_before_effects` and by the
coordinator-death tests.

## `fn the_two_points_whose_operation_is_not_parent_side_are_named_and_counted() {`

Where each containment point's **operation** happens, and where its
**injection** is controlled. Transcribed from
`decisions.effect_site_inventory.containment_sub_effects`, which
annotates the coordinate of exactly one point ("Spawn.PreExecPgidAndRegister
(in the child before exec)") and one other as parent-side
("Spawn.Registered (parent-side registration)").

Two of the eight perform their operation outside the parent, and both
are injected in the parent. That is not a coordinate the implementation
chose for convenience:

* their only declared mode is `Kill`, a **coordinator** death, and a
  coordinator is the parent — a kill inside the forked child would end
  the fork, not the coordinator, and the packet's claim for these points
  ("a coordinator kill after any of these leaves a group the reaper
  settles while holding R28") needs a group that exists;
* an observer cannot run between `fork` and `exec` at all: only
  async-signal-safe calls are permitted there and every real observer
  locks and allocates.

The packet contemplates both: "ST-07 evidence executes each point on its
platform (these are parent-side **or** pre-exec points the harness
controls)", and `InjectionMode` is documented as "how a fault is
introduced at a **parent-side** sub-effect point". The boundary is
counted here so it cannot grow: a third point that stops executing at
its own coordinate fails this test.

## `enum Coordinate` › `Parent,`

In the coordinator process.

## `enum Coordinate` › `ForkedChildBeforeExec,`

In the forked child, before `exec`.

## `enum Coordinate` › `Child,`

In the child, at `exec` itself.

## `fn the_two_points_whose_operation_is_not_parent_side_are_named_and_counted() {` › `let table: &[(SubEffectPoint, Coordinate, Coordinate)] = &[`

(point, where the operation happens, where the injection is controlled)

## `fn the_two_points_whose_operation_is_not_parent_side_are_named_and_counted() {` › `for point in &elsewhere {`

The reason, asserted rather than described: both declare `Kill`
alone, and `Kill` is a coordinator death. The expected modes are the
packet's own split — "Kill is universal … error-return is narrower …
Windows' AmbientJobJoined has one".

## `fn containment_points() -> Vec<SubEffectPoint> {`

-----------------------------------------------------------------------
the containment points, observed at runtime on **every** role
-----------------------------------------------------------------------

## `fn containment_points() -> Vec<SubEffectPoint> {`

**Every** containment point this host declares, read out of the frozen
inventory rather than transcribed from it.

`SPAWN_SITE.sub_effects()` is `Process.Spawn`'s own list and
`SubEffectPoint::platform()` is the point's own host, both in the frozen
`topology::effects` inventory — `sites.rs` and `vocab.rs` respectively since
that module was split into per-concern children — so a point added to the
site later is in this domain the moment it exists. That is not tidiness: the hand-written
Windows list this replaced named `CreatedSuspended`, `PrivateJobAssigned`
and `Resumed` and silently omitted `AmbientJobJoined`, so the kill grid
iterated three of the four points the platform has and six guest runs
reported covering a point none of them had executed (`PR5-RD-002`). A
domain that can omit a point is a domain whose coverage claim is a
coincidence.

## `fn per_spawn_points() -> Vec<SubEffectPoint> {`

The containment points one **spawn** reaches on this platform: every
point of [`containment_points`] that is not a startup step.

`AmbientJobJoined` is a write-command *startup* step rather than a
per-spawn one — it is reached by [`HostRunner::start_write_command`],
not by running a command. It is role-free by construction (the ambient
job is a property of the process, established once before anything can
spawn) and is witnessed for error-return by
`windows_ambient_job_unavailable_refuses_before_effects` and for kill by
`a_kill_armed_at_any_containment_point_actually_kills`, which iterates
the *whole* domain rather than this subset.

## `const STARTUP_POINTS: &[SubEffectPoint] = &[SubEffectPoint::AmbientJobJoined];`

The containment points a write command reaches at **startup**, before it
has spawned anything.

## `fn the_startup_and_per_spawn_domains_partition_this_platforms_points() {`

The two domains partition the platform's points, and neither is empty.

The partition is the property that makes `per_spawn_points` safe to
derive by subtraction: a point added to `Process.Spawn` later lands in
one of the two by construction, and cannot land in neither. Asserted
against `containment_points`, which is itself derived from the frozen
inventory, so the only way to lose a point from both is to delete it
from the `topology::effects` inventory (`vocab.rs` holds the points since
that module was split).

## `fn the_startup_and_per_spawn_domains_partition_this_platforms_points() {` › `let expected: &[SubEffectPoint] = if cfg!(windows) {`

Derived, not transcribed: the domain agrees with the packet's own
platform split for this host.

## `fn shell_command() -> CommandSpec {`

The child of a role whose production program is **the recorded shell**:
the gate (`gates::ShellGate::check` sends `ShellKind::spec`) and the
shell probe ([`shell_probe_request`] sends the same). Built by
[`ShellKind::spec`] rather than round-tripped through a `Command`, so
the program string is the one [`shell_probe_request`] itself carries.

No overlay and no stdin, because that is what production's two shell
specs carry: `gates::ShellKind::spec` writes `env: Vec::new(), stdin:
Vec::new()` and `gates` asserts it ("a gate carries no overlay and no
stdin").

## `const NO_SUCH_TEST: &str = "upstroke_pr4_role_grid_matches_no_test";`

A filter that matches no test in this binary, so the child below runs
nothing and exits 0.

## `fn agent_cli_command(stdin: &[u8]) -> CommandSpec {`

The child of a role whose production program is **an agent CLI**: the
worker, the reviewer and the agent probe, all of which execute the
located binary `bin::Invocation::spec` names and never a shell.

This test binary is that program. It is the one executable this suite
knows exists on both platforms at an absolute path, which is the shape
`bin::locate` produces, and with a filter that matches nothing it runs
no test and exits 0.

**Why the grid may not run one child for every role.** `HostRunner::run`
chooses the observer it hands the funnel, and a grid whose every child
was the recorded shell left

```text
let selected = if is_a_shell(&request.command.program) { hooks } else { &mut NoHooks };
```

green while every real worker, reviewer and agent probe — the three
roles that execute a CLI — ran with no containment hooks and no fault
injection. Same defect, same shape, one field over from the stdin one
below.

## `fn this_test_binary() -> String {`

This test binary's own path, as the `String` a [`CommandSpec`] carries.

## `fn agent_cli_command_at(program: &str, stdin: &[u8]) -> CommandSpec {`

[`agent_cli_command`] against an arbitrary launcher for this binary, so
the *program shape* can vary while everything else stays production's.

## `struct ProgramShape {`

One shape a production `CommandSpec.program` can take on this platform.

## `struct ProgramShape` › `what: &'static str,`

What production produces it.

## `struct ProgramShape` › `reports: bool,`

Whether the child ends up being this test binary, so its `libtest`
report is readable on stdout. False for the recorded shell, which
answers `exit 0` and prints nothing.

## `fn forwarding_shim(dir: &Path, name: &str) -> String {`

Write a launcher into `dir` that forwards its arguments and its stdin to
this test binary, and return its absolute path.

The two spellings are the two an installer actually produces: on Windows
npm writes a `.cmd` (or `.bat`) batch shim beside the package, and on
Unix it writes an extensionless script with a shebang. Neither is a
native executable, and `CreateProcessW`/`execve` reach both only through
an interpreter — which is precisely why a runner that treats them as a
different kind of program is a defect this suite has to be able to see.

## `fn forwarding_shim(dir: &Path, name: &str) -> String` › `format!("@echo off\r\n\"{exe}\" %*\r\n")`

`@echo off` so the batch text itself does not reach stdout, and
the target quoted because its own path may contain a space.

## `const A_DIRECTORY_WITH_A_SPACE: &str = "John Smith";`

A directory name with a space in it, which is what makes the path it
contains one std must quote. `bin.rs`'s own fixture is the production
value this transcribes: `C:\Users\John Smith\npm\copilot.cmd`.

## `fn program_shapes(root: &Path, stdin: &[u8]) -> Vec<ProgramShape> {`

Every **program shape** production can hand the runner on this platform,
materialised under `root`.

The list is derived from what actually reaches `CommandSpec.program` in
this crate, not from intuition. There are two producers —
`bin::Invocation::spec`, which carries the absolute path
`agent::bin::locate` resolved, and `gates::ShellKind::spec`, which
carries the recorded shell's **bare name** — and the first of them can
carry three different kinds of file, because `locate` accepts whatever
the installation is: a native executable, a batch shim, or (on Unix) a
shebang script. `bin::locate`'s own candidate list names `.cmd`
explicitly, and npm-installed agent CLIs on Windows *are*
`claude.cmd`, `codex.cmd`, `copilot.cmd`.

Two axes, varied independently, because a suppression can key on either:
the **kind of file** (native / batch / script / bare name) and whether
the **path needs quoting** (a directory with a space in it).

## `const WORKER_STDIN: &str = "## Task one\n\nthe materialized worker prompt, delivered on \`

What production writes to a **worker's** stdin: the materialized task
prompt, which `engine::attempt::run_attempt` puts on the spec with
`.stdin(cx.adapter.stdin_payload(&task_run).as_bytes().to_vec())`.

Non-empty, and that is the whole point of it being here — see
[`agent_cli_command`] and the grid below.

## `const REVIEW_STDIN: &str = "review the candidate diff and answer with the structured \`

What production writes to a **reviewer's** stdin, from the same seam at
`review::run_review`. A different payload from the worker's, so the grid
carries two distinct non-empty ones rather than one repeated.

## `const AGENT_PROBE_TIMEOUT: Duration = Duration::from_secs(60);`

Every adapter's `PROBE_TIMEOUT`, transcribed: `claude.rs`, `copilot.rs`
and `codex.rs` each declare `const PROBE_TIMEOUT: Duration =
Duration::from_secs(60)`, private to their own module.

## `fn production_request(`

The request **production** sends for `role`, from production's own
builder — five roles, five builders, and this fixture writes none of
them:

* `Probe(Shell)` — [`shell_probe_request`], which is what
  [`run_shell_probe`] (and therefore [`HostRunner::shell_probe`], the
  `RunnerPreflight` entry point ordered at P4 by
  `decisions.workspace_candidates.run_creation`) builds;
* `Probe(Agent)` — [`crate::agent::probe_request`], the builder every
  adapter's `probe` calls;
* `Implement` — [`crate::runner::worker_request`], which
  `engine::attempt::run_attempt` calls;
* `Gate` — [`crate::runner::gate_request`], which `gates::ShellGate::
  check` calls;
* `Review` — [`crate::runner::review_request`], which
  `review::run_review` calls.

This is the repair for a real hole, not tidiness. The three in-attempt
roles used to be hand-built here with `agent: None` and a *gate*
identity, while production sends `agent: Some(<the adapter>)` and a
worker/review identity. A `HostRunner::run` that selected [`NoHooks`]
for `role in {Implement, Review}` **and** `agent.is_some()` — the
production shape, and only it — therefore ran every real worker and
reviewer with no containment hooks and no fault injection while the
whole suite stayed green.

**The builders are only half of it.** A builder fixes the role, the
binding and the identity; everything it is *handed* — the program, the
arguments, the overlay, the stdin payload and the timeout — is still
this fixture's choice, and each of those is a field `HostRunner::run`
can key an observer selection on. So each is given production's own
value for that role rather than one convenient constant shared by all
five:

* **program and args** — [`agent_cli_command`] for the three roles that
  execute a located CLI, [`shell_command`] for the two that execute the
  recorded shell;
* **stdin** — the adapter's prompt for the worker and the reviewer
  (`AgentAdapter::stdin_payload`, delivered at
  `engine::attempt::run_attempt` and `review::run_review`), empty for
  the gate and both probes, which is what their specs carry;
* **env** — empty for all five, because that *is* production's only
  value: `ShellKind::spec` and `bin::Invocation::spec` are the only two
  spec constructors this crate has and both write `env: Vec::new()`,
  and no call site adds an overlay entry (asserted by
  `runner::tests::every_production_command_spec_payload_is_classified`);
* **timeout** — each role's own production default, five distinct
  values.

## `agent_cli_command(b""),`

A probe asks a CLI about itself: an agent binary, and no
prompt on stdin.

## `fn run_in_role(`

Start one child in `role`, through the entry point production uses for
that role, and wait for it.

`Probe(Shell)` goes through [`HostRunner::shell_probe`] rather than
through `run` directly, because that entry point is what pre-flight
calls and it adds the probe's own refusals; it builds its request with
[`shell_probe_request`], which is the value [`production_request`]
returns for that role. Every other role is [`production_request`]
executed through `run`, which is exactly how production sends it.

## `struct RoleWitness {`

Everything one spawn's containment observers can see, for one role.

`point` delegates to [`HarnessHooks`] — the production wiring onto
PR3's [`HookHarness`] — so the evidence lands where ST-07 reads it, and
records the order beside it, because a set cannot tell
`PrivateJobAssigned` (the child is still suspended) from `Resumed` (it
is not). `child_created` is the funnel's other observation, and on Unix
it asks the kernel whether the containment *operation* happened for
this role rather than only its hook — as a `GroupObservation`, so the
look carries whether the child had already exited, what `getpgid`
answered and, on macOS, what the exited record answered. The grid's
children are short-lived and the look comes after `spawn` returns, so
the exited case is ordinary here, and it is the case the standing macOS
red was (`W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT`).

## `fn group_leadership(observations: &[proc::GroupObservation]) -> (Vec<bool>, String) {`

The decisions and the rendered records side by side, so an assertion
compares the former and prints the latter.

## `impl RoleWitness` › `fn handle(&self) -> Self {`

A second handle on the same recordings, for the runner to own.

## `fn the_role_grid_sends_the_shapes_production_sends() {`

The grid's fixtures vary every independently meaningful field
independently, and the counts say so.

The standing guard in `reviews/FINDINGS.md`: "fixtures must vary every
independently meaningful field independently; assert hostility as
distinct-value **counts**, not prose." A grid that moved two fields
together would prove only that *some* combination reaches the funnel.

**The field list is taken from the type, not from intuition**, because
three repairs in a row swept the fields their author thought of and the
next confirmation found the one nobody listed. [`RunnerRequest`] has six
fields and its `command` has four, so this asserts all nine that can
vary: role, agent binding, invocation identity, workspace, timeout, and
the spec's program, args, env and stdin.

Two of them used to be constants here (`agent: None`, a gate identity),
which is precisely the shape production never sends. Three more were:
every request carried `stdin: Vec::new()` while production's worker and
reviewer always carry a prompt, every request ran the recorded shell
while production's three agent-bound roles always run a CLI, and every
request carried `SHELL_PROBE_TIMEOUT` while production gives each role
its own. Each of those left a one-line observer suppression in
`HostRunner::run` — keyed on `stdin.is_empty()`, on the program, or on
the timeout — passing this whole file.

Nothing asserted below is read back out of the fixture: the bindings are
R3's own predicate, the payload split is `AgentAdapter::stdin_payload`'s
rule, the program split is "a bound process runs its agent's CLI", and
the timeouts are production's own public constants.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `let labels: BTreeSet<String> = requests.iter().map(|r| r.role.label()).collect();`

Field 1: the role. Five distinct values, one per member.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `let bound = requests.iter().filter(|r| r.agent.is_some()).count();`

Field 2: the agent binding. Three bound, two not — and *which* three
is R3's rule, not this fixture's.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `let ids: BTreeSet<String> = requests.iter().map(|r| r.invocation.render()).collect();`

Field 3: the identity. Five distinct renderings, and the form
follows the role: the two probes carry probe identities, the three
in-attempt roles carry attempt identities with three distinct role
tokens.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `let paired: BTreeSet<(String, bool, String)> = requests`

And the pairing, so no two fields can be swapped without notice.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `let carrying_a_payload: BTreeSet<String> = requests`

Field 4: the stdin payload. `AgentAdapter::stdin_payload` is
delivered by `engine::attempt::run_attempt` and `review::run_review`
and by nothing else, so exactly the worker and the reviewer carry
bytes; a gate's spec carries none (`gates::ShellKind::spec`, and
`gates` asserts it), and neither probe does.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `let shell_program = native().spec(SHELL_PROBE_COMMAND).program;`

Field 5: the program (and with it the args). Production's two
spec constructors are `gates::ShellKind::spec` — the recorded shell —
and `bin::Invocation::spec` — a located agent binary at an absolute
path; which one a role gets is decided by whether it runs an agent
CLI.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `for request in &requests {`

The program and the binding do move together, and that is
production's rule rather than this fixture's shortcut — a bound
process is one that executes its agent's CLI, an unbound one executes
the recorded shell. Asserted, so a fixture that broke the
correspondence would have to say why.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `assert_ne!(`

The payload split is *not* the binding split, so the two cannot be
mistaken for one field: the agent probe is bound and runs a CLI and
still carries no prompt.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `let timeouts: BTreeSet<(String, Duration)> = requests`

Field 6: the timeout. Each role's own production default — five
constants, five distinct values, none of them this fixture's.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `assert!(`

Field 7: the overlay. Empty everywhere, because that is production's
only value — both spec constructors write `env: Vec::new()` and no
call site adds an entry
(`runner::tests::every_production_command_spec_payload_is_classified`
is the tripwire for a call site that starts to). Stated as an
assertion rather than left to silence: the day production carries an
overlay, this row has to become a varying dimension like the four
above, and this is what says so.

## `fn the_role_grid_sends_the_shapes_production_sends()` › `let workspaces: BTreeSet<&Path> = requests.iter().map(|r| r.workspace.as_path()).collect();`

Field 8: the workspace. Two distinct values, and which is which is
production's: a probe has no workspace of its own and runs in the
coordinator's directory (`agent::probe_workspace`), everything else
runs in the run's worktree.

## `fn every_production_invocation_identity_reaches_the_containment_points() {`

The **identity** dimension, over the whole space production builds —
not the five the role grid happens to carry.

The role grid carries one identity per role, so five, and each is the
first of its kind: `worker`, `gate0`, `review_pass0`, and the two probes
at ordinal 0. Production builds more than that, and every one of them is
a value `HostRunner::run` could key an observer selection on exactly the
way `PR4-CONF-006`'s mutation keyed on an empty stdin. Enumerated from
`AttemptRole`'s own variants and from the call sites that build them:

* `AttemptRole::ReviewReask(n)` — `review::run_review`'s re-ask, a
  second reviewer process inside one pass and the one attempt role the
  five-role grid has no slot for;
* non-zero role indices — `engine::attempt` numbers gates by position
  (`Gate(index)`) and review passes by pass (`ReviewPass(pass)`), so the
  grid's `0` is the first of several, not the only one;
* non-zero probe ordinals — each adapter fixes one per pre-flight step
  (`claude::probe_ordinal`: version 0, help 1, auth status 2).

Two shapes are deliberately absent because production does not build
them. `InvocationId::Sequence` has no production call site in this slice
— integration transactions are a later PR — and the `Attempt` form's own
`ordinal` is always 0, because `engine::attempt::AttemptCx::invocation`
says why: "nothing inside one attempt runs a given role twice".

## `fn every_production_invocation_identity_reaches_the_containment_points() {` › `let grid: BTreeSet<String> = ExecutionRole::all()`

Every identity here renders differently from every identity the role
grid sends, or this test would be re-proving the grid.

## `fn every_shipped_agent_binding_reaches_the_containment_points() {`

The **agent binding** dimension, over every shipped adapter rather than
the one the role grid names.

`ExecutionRole::all` binds `claude-code` to its agent-probe target and
`fixture_agent` binds the same id to the worker and the reviewer, so the
whole containment grid runs on one of the three ids this crate ships.
`agent` is a field of every request and `host-v1` already branches on it
(`HostEnvironment::compose` gives each agent its own credential
location), so `if request.agent == Some(AgentId::new("copilot")) {
NoHooks }` is a suppression that runs one third of every real run
unobserved and leaves the five-role grid green.

The roster is [`CREDENTIAL_LOCATIONS`]' own, not a list written here, so
a fourth adapter has to appear in this test the moment `host-v1` learns
where its credentials live.

## `fn every_production_program_shape_reaches_the_containment_points() {`

The **program shape** dimension, over every shape production can hand
the runner rather than the one the role grid happens to carry.

Every agent role in the five-role grid runs `std::env::current_exe()` —
a native `.exe` on Windows — and the only `.cmd` this suite ever
executes is `agent::bin::tests::a_batch_shim_runs_and_receives_its_argument`,
which calls `build_command(&spec).output()` and so bypasses `HostRunner`
and its hooks entirely. That left

```text
let mut no_hooks = NoHooks;
… if request.command.program.to_ascii_lowercase().ends_with(".cmd") {
       &mut no_hooks as &mut dyn SpawnHooks
   } else { &mut **hooks }
```

green across the whole suite while **every real Windows agent CLI** ran
with no containment observation and no fault injection — because
npm-installed agent CLIs on Windows are exactly `claude.cmd`,
`codex.cmd` and `copilot.cmd`. That is the production shape, not an
exotic one, and repair round 6 named this mutation in its own report and
neither repaired it nor carried it to `reviews/FINDINGS.md`.

Same two claims as every other axis in this file — the points are
reached, and the observer's answer is honoured — so a shape that is
merely *observed* and not *injectable* fails here too.

## `fn every_production_program_shape_reaches_the_containment_points() {` › `assert_eq!(`

The axes, as counts, so the list cannot shrink in silence.

## `fn every_production_program_shape_reaches_the_containment_points() {` › `let mut injections = 0_usize;`

And the observer's answer is honoured for every shape, at every
point — observation and injection are two claims.

## `fn the_cli_roles_of_the_grid_run_a_shim_shaped_program_through_the_funnel() {`

And the shim shape through **every role that runs a CLI**, built by that
role's own production builder.

[`every_production_program_shape_reaches_the_containment_points`] varies
the shape against one role, the way
[`every_shipped_agent_binding_reaches_the_containment_points`] varies the
binding against one role. This is the other half: the role grid itself
carrying a batch-shim program, so a suppression keyed on the *pair*
— a `.cmd` in the reviewer's hands, say — has nowhere left to be green.

Three roles and not five: `gate` and `probe(shell)` run the recorded
shell by production's own rule, which
`the_role_grid_sends_the_shapes_production_sends` asserts, and a fixture
that handed a gate an agent shim would be asserting something production
never does.

## `fn the_cli_roles_of_the_grid_run_a_shim_shaped_program_through_the_funnel() {` › `std::fs::create_dir_all(&request.workspace).expect("the request's workspace");`

A probe chooses its own workspace, so the run has to happen where
the request says rather than where this test would prefer.

## `fn the_grids_agent_cli_child_runs_no_test_and_exits_zero() {`

The grid's non-shell child really does run nothing.

What the round-6 repair could get wrong: [`agent_cli_command`] executes
**this test binary**, so a filter that ever matched something would have
the role grid running tests inside its own fixtures — silently, three
times per grid, with whatever those tests do to the filesystem — and a
filter that stopped exiting 0 would fail the grid for a reason that has
nothing to do with containment. Neither is visible from the grid itself,
which only reads the exit code.

So the child's own report is read: `libtest` prints the count it ran,
and it must be none.

## `fn every_role_reaches_the_containment_points_of_this_platform() {`

Every role's spawn is observed at the containment points — not only the
gate's.

`scope`: "HostRunner wraps proc supervision … through the process
funnel with containment sub-effect hook points" and "**probes**,
workers, gates, reviews go through the Runner"; `gating`: "process
funnel sites recorded"; `proof_tests[3]`: "containment sub-effect hook
tests (ST-07 subset)".

Until this fixture existed every runtime containment observer in this
file built its request with `ExecutionRole::Gate`, so a `run` that
passed [`NoHooks`] for any *other* role emitted no hook evidence at all
and the whole suite stayed green. The probe roles are what that costs
most: `decisions.workspace_candidates.run_creation` orders P4
`RunnerPreflight` before P6 `run_started`, so their spawns are the
prefixes ST-07 evidence over `Process.Spawn` is read as covering.

Three observations per role, not one — the points reached, the order
they were reached in, and (on Unix) the kernel's answer that the
containment *operation* ran for this role's child. A funnel that fired
the hooks for a probe while skipping the operation would pass the first
and fail the third.

`runner::tests::the_spawn_site_files_every_role_under_one_context_and_the_count_says_which`
does **not** discharge this: counting that two roles fall outside the
site's declared context proves the mismatch exists; it does not prove
the hooks execute on those roles. A counted admission is not runtime
proof. This is the runtime proof, and it is asserted for all five roles
rather than for the two the count names, because a suppression keyed on
any single role is the same defect.

## `fn every_role_reaches_the_containment_points_of_this_platform() {` › `assert_eq!(`

The same points, in the packet's order and each exactly once —
so a role whose funnel reached them in another order, or twice,
fails here too and not only the set above.

## `fn every_role_reaches_the_containment_points_of_this_platform() {` › `assert_eq!(`

The funnel's other observation, and the evidence a fault
injected at `child_created` would need.

## `fn every_role_reaches_the_containment_points_of_this_platform() {` › `assert_eq!(`

Unix: the containment *operation* — not only its hook — ran for
this role. The witness is the kernel.

## `const SPAWN_KILL_POINT: &str = "UPSTROKE_SPAWN_KILL_POINT";`

-----------------------------------------------------------------------
Kill mode, actually executed
-----------------------------------------------------------------------

## `const SPAWN_KILL_POINT: &str = "UPSTROKE_SPAWN_KILL_POINT";`

Which containment point the kill helper is to die at.

## `struct KillAtPoint(SubEffectPoint);`

A hook that kills the funnel at one named point and nowhere else.

## `impl SpawnHooks for KillAtPoint` › `fn point_mode(`

A point consulted at **two** coordinates is killed at the one the
kill mode belongs at, and not at the other one.

`Spawn.AmbientJobJoined` is that point: its error-return coordinate
is *before* the join and its kill coordinate is *after* it. The
inherited default answers `point()` to both, so a hook armed for a
kill would abort at the earlier, error-return coordinate — before
there is an ambient handle to close, which is the state the point's
kill claim says there is not. The grid would still see an abort and
would still pass, while witnessing a coordinate the packet does not
name. That is the same shape of false witness as the omitted point
itself (`PR5-RD-002`), one layer in.

## `fn spawn_funnel_kill_helper() {`

The child half of [`a_kill_armed_at_any_containment_point_actually_kills`].

A kill is `std::process::abort` for the reason [`proc::apply`] gives: the
claim under test is what a coordinator that dies **without running any
cleanup** does, and both `panic!` and `exit` run destructors — including
the one that closes the very job handle whose close-on-death is the
mechanism. So it needs a process of its own.

It **establishes containment first**, which is not decoration. On Windows
a kill at `CreatedSuspended` leaves a suspended stub by construction —
that is the state INV-18 exists for — and the only thing that reaps it is
the ambient job's handle closing when this process dies. A helper that
skipped the step would leak one suspended `cmd.exe` per point onto the
guest, **measured**: the first run of this grid left three of them and a
hung parent. On Unix the step is a no-op and the per-invocation reaper
settles the group instead.

**The startup step is where the arming happens for a startup point.**
This used to run `start_write_command(&mut NoHooks)` unconditionally and
only then install `KillAtPoint`, so `Spawn.AmbientJobJoined` — which is
reached by that call and by nothing later — could not receive a kill at
all, and six guest runs of a grid that claimed to cover it executed it
zero times (`PR5-RD-002`). A startup point is now armed *on the startup
call*, which is the only place it is consulted.

## `fn spawn_funnel_kill_helper()` › `let _ = start_write_command(&mut KillAtPoint(point));`

The point's kill coordinate is *inside* this call, after the real
ambient join. Reaching the line after it means the kill never
fired, and the parent reads a clean exit as exactly that.

## `fn spawn_funnel_kill_helper()` › `let _ = runner.run(&crate::runner::gate_request(`

Every point this platform declares is reached by an ordinary spawn,
which is what `every_role_reaches_the_containment_points_of_this_
platform` establishes; the gate role is the cheapest of the five.

## `fn spawn_funnel_kill_helper()` › `std::process::exit(0);`

Reached only if the kill did not fire, which the parent detects as a
clean exit.

## `fn a_kill_armed_at_any_containment_point_actually_kills() {`

`Injection::Kill` **aborts**, at every containment point that declares
the mode on this platform.

`decisions.effect_site_inventory.scope` requires "every parent-side
sub-effect point observed **executed** at least once by the suite in
every injection mode the point supports", and every containment point
declares `Kill` (`SubEffectPoint::modes`). Nothing had ever let one
fire: the runtime reach tests arm nothing, and the fault grid injects
`Injection::Error` — deliberately, because an abort would take the test
binary with it. So `Injection::Kill => Ok(())` in `proc::apply` passed
the whole suite (`PR5-SEAMS-001`), and with it every ST-07 kill-mode
claim about `Process.Spawn`.

This is the sibling of `events::log::tests::a_kill_at_each_append_point_
leaves_the_shape_the_packet_tables`, and the same idiom: a subprocess
helper, and the child's death **checked** rather than assumed — not a
clean exit, no `panicked at` on stderr, and on Unix the signal is
`SIGABRT` and not some other way of dying.

The domain is [`containment_points()`] — **every** point this platform
declares, read out of `Process.Spawn`'s own `sub_effects()` and each
point's own `platform()`, so a point added later is covered by
construction rather than by someone remembering to add it here. It was a
hand-written three-element list, which omitted `AmbientJobJoined`
(`PR5-RD-002`): the grid, its helper and this doc comment all agreed
that the Windows ambient join was covered in kill mode, and it had never
once executed in that mode on the guest.

Both of the funnel's two appliers are inside the domain: on Unix all
four points go through `proc::apply`, and on Windows `AmbientJobJoined`
goes through `apply` while the three per-spawn points go through
`apply_io`. That is the second reason the omission mattered — with
`AmbientJobJoined` absent, no Windows run of this grid touched `apply`
at all.

**The helper's output goes to files, and this waits on the process
rather than on its pipes.** `Command::output()` returns when the pipe
write ends close, not when the child exits, and on Windows
`CreateProcessW` inherits handles — so the grandchild this helper leaves
**suspended by design** holds a duplicate of the pipe and `output()`
blocks for ever. Measured on the guest, where it hung the whole run.

## `fn a_kill_armed_at_any_containment_point_actually_kills()` › `assert_eq!(`

The count, so a domain that shrank would fail here rather than pass
vacuously — four on **both** platforms, which is what the frozen
inventory declares and what the omitted `AmbientJobJoined` made read
as three.

## `struct FailAt(SubEffectPoint);`

A hook that fails the funnel at one named point and nowhere else.

## `fn a_fault_armed_at_any_containment_point_stops_any_role() {`

The hooks are not merely *called* on every role — their answer is
honoured, so every role is fault-injectable at every containment point
this platform reaches.

Observation and injection are two claims, and the second is the one
ST-07 spends: a funnel that consulted the observer and then ignored
what it said would satisfy
`every_role_reaches_the_containment_points_of_this_platform` and inject
nothing. The armed point is named in the failure the caller receives,
so this also pins *which* coordinate refused — a funnel that collapsed
four points into one arming site would fail here.

`Injection::Error` rather than `Injection::Kill` because `Kill` aborts
the process: it is exercised by the Windows coordinator-death tests,
which need a subprocess to survive it. The packet gives only
`AmbientJobJoined` an error contract, so an `Error` at these points can
come only from a hand-written observer — which is exactly what a fault
injection is, and `apply`/`apply_io` surface it rather than dropping it.

## `fn the_pre_exec_containment_step_runs_in_the_forked_child() {`

The operation of `Spawn.PreExecPgidAndRegister` really did happen in the
forked child, and the funnel really did reach the four Unix points in
the packet's order.

The witness is the kernel, not this crate: `getpgid(pid) == pid` is true
exactly when the closure's `setpgid(0, 0)` ran, and it is asked at
`child_created` — the first instant the parent knows the pid.

## `fn a_containment_proof_exists_only_where_containment_was_established() {`

A containment proof exists exactly when containment was established,
and establishing it twice is establishing it once.

[`Contained`] is what the engine's write coordinator now requires, and
[`containment_establishments`] is what an entry-point census reads, so
both are only worth anything if a token cannot appear without the step
having run. The count is incremented by the token's own constructor, so
this is the assertion that the constructor is not reachable another way
— including through the refusal path, which returns before it.

Idempotence is not decoration either: the CLI establishes containment
at dispatch and then calls `engine::run`, which establishes it again.
The ambient job is a process-wide singleton (`join_ambient` memoises),
so the second call must be a no-op that still hands back a proof — and
on the platform where it does something, the process must still be a
member afterwards.

## `fn a_containment_proof_exists_only_where_containment_was_established() {` › `let _first = contain_write_command(&mut NoHooks).expect("containment establishes on this host");`

The tokens are held rather than discarded: two live proofs at once is
what a coordinator entered from an already-contained CLI has.

## `fn a_containment_proof_exists_only_where_containment_was_established() {` › `{`

A refused establishment mints nothing. Only Windows can refuse — on
Unix the step is the reaper and the isolated process group, and
there is nothing that can fail — so the negative half is asserted
where it exists.

## `struct RefuseAmbientJoin {`

-----------------------------------------------------------------------
Windows ambient job (INV-18)
-----------------------------------------------------------------------

## `struct RefuseAmbientJoin {`

An observer that refuses the ambient join, for
`windows_ambient_job_unavailable_refuses_before_effects`.

## `fn windows_ambient_job_unavailable_refuses_before_effects()` › `struct Reporting {`

The observer has to be readable after the run, and `with_hooks`
takes ownership, so it reports through a channel.

## `fn windows_ambient_job_unavailable_refuses_before_effects()` › `assert_eq!(`

"The simulated failure left no real ambient job behind" — asserted
on the *count* rather than on `ambient_job_established`, which is a
process-wide latch and no longer a valid oracle here: the library's
write coordinator establishes containment now
(`engine::tests::every_public_write_coordinator_entry_point_establishes_containment`),
so other tests in this binary legitimately join the real ambient job
and the latch may already be true when this test runs. The count is
per-thread and per-call, so it still says exactly what this test
means: *this* refusal established nothing.

## `fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {`

The production containment mint propagates a join refusal, and a
refused join mints nothing.

`invariants_introduced[3]`: "INV-18 host portion: … refusal before any
effect if the ambient job cannot be established", and
`expected_failures_refusals[1]`: "ambient job cannot be created or
joined (Windows) → write command refuses at startup with a diagnostic".

The subject is [`contain_write_command`] **itself** — the function
`engine::run_harness`, `engine::resume_harness` and `src/main.rs`'s
dispatch all reach, and the only place in the crate that mints a
[`Contained`]. Every other simulated ambient failure in this suite goes
through [`HostRunner::start_write_command`] or through a closure injected
at `engine::run_contained`, so

```text
let _join_outcome = proc::join_ambient_job(hooks);
Ok(Contained::new())
```

left the whole suite green while every facade run and every `upstroke run`
on Windows dispatched with **no ambient job** — and a coordinator killed
between `CreateProcessW` and private-job assignment then leaves a
suspended stub with no owner, which is the one thing the ambient job
exists to prevent.

Windows only, and that is the invariant rather than a limitation of the
test: on Unix [`proc::join_ambient_job`] returns `Ok` unconditionally and
does not consult the observer at all — deliberately, so a Linux cell
cannot record a Windows containment point as executed — so there is no
failure on that platform for anything to propagate. The Linux suite
cannot kill that mutation and does not claim to.

## `fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {` › `let mut refusing = RefuseAmbientJoin::default();`

The observer is borrowed rather than owned here, so it can simply be
read afterwards — `with_hooks` takes ownership and needs a channel,
this does not.

## `fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {` › `let message = error.to_string();`

The diagnostic reaches the caller. The three fragments are
`proc::AMBIENT_REFUSAL_PREFIX` and `proc::AMBIENT_REFUSAL_SIMULATED`,
named rather than matched whole: what the operator has to be told is
that it is the ambient job, which invariant it enforces, and that
nothing ran.

## `fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {` › `assert_eq!(`

No effect precedes it: the funnel reached the join's own coordinate
and nothing past it, and no child exists.

## `fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {` › `assert_eq!(`

And no proof was minted — which is the half the mutation above
breaks. The count is per-thread and per-call, so it says exactly that
*this* call established nothing, where `proc::ambient_job_established`
is a process-wide latch other tests in this binary legitimately set.

## `fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {` › `let _proof = contain_write_command(&mut NoHooks).expect("the real join succeeds on this host");`

The success direction, so the assertion above is about the refusal
and not about a function that never mints at all.

## `fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {` › `let mut refusing = RefuseAmbientJoin::default();`

`start_write_command` is the same step for the caller with nothing to
prove it to — `src/main.rs`'s dispatch, which is the CLI's whole
write side — and it has a body of its own to drop the refusal in.

## `const JOIN_RECORD: &str = "UPSTROKE_PR4_JOIN_RECORD";`

Where the join-ordering helper writes what it saw.

## `struct WitnessAmbientJoin {`

Records, at each consultation of `Spawn.AmbientJobJoined`, whether the
ambient job existed *at that instant*. The kernel is the oracle: the
singleton is set only by a successful `AssignProcessToJobObject`.

## `fn windows_the_ambient_join_is_observed_on_both_sides_of_the_join() {`

The point named `AmbientJobJoined` is observed on the side of the join
its own contract needs.

`containment_sub_effects` gives it both an error contract ("failure
refuses the write command", which stands in place of establishing the
job) and a kill claim ("a coordinator kill after any of these leaves no
host process — the ambient handle closes"), and those are opposite sides
of one operation. So the error-return coordinate must see no job and the
kill coordinate must see one.

In a subprocess because the ambient job is a process-wide singleton:
"not yet established" is observable once per process, and a test that
depended on being the first in its binary would be a test whose meaning
depended on test order.

## `const POISON_AMBIENT: &str = "UPSTROKE_POISON_AMBIENT";`

Set for the child that is to carry a **real** memoised ambient failure.

## `fn poisoned_ambient_helper() {`

The child half of
[`a_real_memoised_ambient_failure_refuses_the_write_command`].

It spends this process's one ambient cell, which is why it needs a
process of its own: `AMBIENT` is a `OnceLock` and the test binary's
other tests need it unspent.

## `fn poisoned_ambient_helper()` › `let error = proc::join_ambient_job(&mut proc::NoHooks)`

(1) The funnel's own entry point reports the remembered failure.

## `fn poisoned_ambient_helper()` › `let before = containment_establishments();`

(2) The production mint refuses, and mints nothing.

## `fn poisoned_ambient_helper()` › `start_write_command(&mut proc::NoHooks).expect_err("the CLI write path refuses too");`

(3) And the CLI's unit-returning entry, which is what `src/main.rs`
calls before any dispatch arm.

## `fn a_real_memoised_ambient_failure_refuses_the_write_command() {`

A **real** memoised ambient failure refuses the write command.

`crash_reconstruction`: "if the ambient job cannot be created or joined
the write command refuses at startup with a diagnostic before any
workspace effect (no degraded mode; deferred)", and
`expected_failures_refusals[1]` requires the same refusal.

`PR4-CONF-005` closed the *injected* half — an observer refusing at
`Spawn.AmbientJobJoined`, which fires strictly **before** the memo is
consulted. `PR5-CORRECTNESS-010` is the half beyond it: no test had ever
carried an actual memoised `Err` through `join_ambient`'s match, so
`Err(_) => Ok(())` there left `join_ambient_job` reporting success,
`contain_write_command` minting `Contained`, and `run`/`resume` taking
workspace effects with no ambient kill-on-close job.

Windows only, and that is the invariant rather than a gap:
`join_ambient_job` is a no-op on Unix and has no memo to poison. The
platform-independent half of the same claim — that a remembered failure
comes back as that failure — is `agent::proc::tests::
a_memoised_establishment_failure_reaches_every_later_caller`, which runs
everywhere.

## `const STUB_RECORD: &str = "UPSTROKE_PR4_STUB_RECORD";`

Where the coordinator helper writes the identity of the stub it created
before it dies.

## `fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {` › `crate::topology::effects::Injection::Kill`

The kill hook: after `CreateProcess` and before private-job
assignment. `apply_io` aborts, so no destructor runs and the
ambient handle is closed only by the kernel.

## `fn windows_ambient_coordinator_helper()` › `let _ = runner.run(&request);`

Aborts inside `run`, at `CreatedSuspended`.

## `fn one_crash_cycle(tag: &str) -> (u32, u64, bool) {`

Run one coordinator-death cycle and return the stub's identity and
whether it was an ambient-job member at creation.

## `fn naming_grid() -> Vec<ProgramNaming> {`

-----------------------------------------------------------------------
program resolution (PR6D-001)
-----------------------------------------------------------------------

## `fn naming_grid() -> Vec<ProgramNaming> {`

Both naming rules, exhaustively.

The `match` is what makes it exhaustive: a variant added later fails to
compile here rather than quietly leaving every grid below, which is the
failure `PR5-RD-002` recorded one level out — a hand-written domain that
omitted a point while six guest runs reported covering it.

## `fn composed(pairs: &[(&str, &OsStr)]) -> Vec<(OsString, OsString)> {`

A composed environment holding exactly the pairs given.

## `fn path_of(dirs: &[&Path]) -> OsString {`

`PATH` as an `OsString`, from directories.

## `fn program_file(dir: &Path, file_name: &str) -> PathBuf {`

An empty file the platform would accept as a program: on Unix the
execute bit is set, because `execvp` requires it and
[`ProgramNaming::Posix`] therefore checks it.

## `fn unexecutable_file(dir: &Path, file_name: &str) -> PathBuf {`

A file that exists and is **not** a program: no execute bit.

Unix only, because it is only there that a file's mode decides: Windows
carries no execute bit, so "exists but is not executable" is not a state
a fixture can construct on that platform at all.

## `const REAL_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.CPL";`

The `PATHEXT` a Windows machine ships, in its order.

## `fn shim_file_name(name: &str) -> String {`

The file name a bare `name` is installed under here: npm's two
spellings, a batch shim on Windows and an extensionless script on Unix.

## `fn marker_shim(dir: &Path, file_name: &str, marker: &str) -> PathBuf {`

A runnable shim that prints `marker` and its first argument.

The **file name is the caller's**, so the extension is a field a test
varies while the content is held constant, and the marker is the
caller's so "a shim ran" and "*this* shim ran" are different
observations.

All three executable fixture builders, `marker_shim`, `transparency_shim`,
and `forwarding_shim`, use `write_shim` to publish their scripts.

## `fn write_shim(path: &Path, script: &str) {`

On Unix, a supervised `/bin/sh` child writes the script with `printf '%s'` and
positional arguments. The harness never opens that inode for writing. A fork
in another harness thread therefore cannot inherit its writer, which closes
when the writer child exits. The 60-second supervision bound fails and cleans
up a stuck writer. Windows retains the batch-file writer.

This repairs `W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY` and the same writer pattern
in the transparency and forwarding fixtures. The finding is recorded in
`reviews/FINDINGS.md` §43. Closing the harness's original file descriptor, or
renaming that inode, would leave an inherited writer alive. Moving the write
into a separate process prevents that inheritance.

## `mod inherited_writer {`

The Linux test `marker_shims_do_not_leave_writers_in_another_threads_fork`
runs one isolated helper under a 180-second process-tree timeout. Its held
forks cannot inherit descriptors from other tests in the outer harness.

The regular-file control opens a writer, forks from another thread, closes
the original descriptor, and requires `ETXTBSY` from executing that same inode.
Execution succeeds after the forked holder exits.

The ownership witness runs the actual `marker_shim` against a FIFO whose
capacity is smaller than its script. `F_SETPIPE_SZ` rounds a request up to the
page size and reports the capacity it installed, so the 4096-byte request
installs 65536 on a 64 KiB-page kernel. `marker_exceeding` sizes the payload
from that reported capacity, plus `MARKER_MARGIN`, rather than from the
request. The margin is headroom over the capacity, not decoration: the oracle
drains one byte before the fork, so the script has to exceed the pipe by more
than the bytes drained for the writer to still be blocked when the fork
happens. A payload fixed at 16 KiB instead both fails an exact-equality check
on the return value and, without it, fits such a pipe whole: the writer would
finish before the fork and the witness would observe nothing. That is
`PR162-ASTRA-PAGE-SIZE`. `the_shim_script_exceeds_every_installed_pipe_capacity`
holds the sizing for each page size Linux supports without needing one of those
kernels to run on, and `shim_script` is the single spelling of the expected
script that both it and the drain oracle read. Reading the first byte proves
the writer is open; the undrained payload keeps it open while another thread
forks.
After draining exactly the expected script and joining the original writer,
the reader must see EOF while the forked holder is still alive. An in-process
writer leaves its descriptor in that holder and produces `WouldBlock` instead.
The holder's lifetime is checked immediately before and after the EOF read.
The path and marker include shell metacharacters to check literal transport.

Socket readiness and release establish the interleaving. Reads and writer
supervision are bounded, the held child is reaped on return or unwinding, and
the outer supervisor contains a wedged helper and its descendants. The
adjacent source comments retain the fork safety proof and lifetime protocol.
Linux CI executes the inheritance and `ETXTBSY` witness. The existing native
Linux and macOS marker tests execute the Unix writer; Windows tests retain
their existing batch-shim behavior.

## `fn environment_on_path(dirs: &[&Path], pathext: Option<&str>) -> HostEnvironment {`

This process's environment with `PATH` — and `PATHEXT` — replaced.

The rest of the process environment is kept rather than emptied, because
spawning a batch shim on Windows goes through `cmd.exe` and a child
stripped to two variables would be testing something else. Only the two
fields under test move.

## `fn named_request(program: &str, argument: &str, workspace: &Path) -> RunnerRequest {`

A gate request for `program` with one argument.

## `const NAME_TABLE: &[(&str, bool, bool)] = &[`

A program string, and whether each naming rule calls it a **name** (to
search for) rather than a **location** (to use as given).

Written here from the two platforms' own rules, not read from the code
under test. The four rows the two rules disagree on are the point: on
Unix a backslash and a colon are ordinary characters in a file name, and
a rule that ignored its `self` would agree with itself on every row that
only contains `/`.

## `("claude", true, true),`

(program, is a name under Posix, is a name under Windows)

## `fn a_program_that_names_a_location_is_used_as_given_and_only_a_name_is_searched() {`

A program that names a **location** is handed to `Command` as given, and
only a bare name is searched for.

This is the constraint the repair had to hold: "an absolute `program`
must spawn exactly as today". It is asserted by construction rather than
by a spawn — the environment carries no `PATH` at all, so a resolution
that searched would have nothing to find and would refuse, and every
location row instead comes back byte for byte.

The second field held constant is the environment (empty, for every
row); what varies is the program shape and the naming rule, and the four
rows on which the two rules must **disagree** are counted so that a
`is_bare_name` which ignored its platform reports it.

## `fn path_directory_order_decides_between_installations_and_pathext_only_within_one() {`

`PATH` order decides between installations; `PATHEXT` order decides only
**within** one directory.

The intersection this repair is most exposed to, and the one a shell and
`std::process::Command` answer differently: std appends `.exe` and only
`.exe`, so an earlier directory's `claude.cmd` is invisible to it and a
later directory's `claude.exe` wins. A shell — and now this runner —
takes the earlier directory. Both axes vary independently and the
resolved files are counted as distinct values, so a nesting that swapped
the loops collapses the count.

Driven under [`ProgramNaming::Windows`] on **both** platforms, which is
the whole reason that type exists: `PR6D-001` is a Windows rule, and a
Windows rule only the guest can execute is a rule that ships untested
six days out of seven.

## `fn path_directory_order_decides_between_installations_and_pathext_only_within_one() {` › `assert_eq!(`

`first/x.cmd`, `second/x.exe`, `both/x.com`, `both/x.cmd`,
`both/x.exe` — one per way the two axes can decide, and a grid that
reaches fewer has an axis that is not varying the answer.

## `fn a_windows_name_is_pathext_and_never_the_extensionless_file() {`

What a Windows name may be is `PATHEXT`, and never the extensionless
file beside it.

Three things a resolution can get wrong here, each with its own row:
widening (an extensionless `claude` in a `PATH` directory shadowing the
real `claude.exe` — `CreateProcessW` appends `.exe` and `cmd.exe`
appends `PATHEXT`, and neither would run it); not reading `PATHEXT` at
all (a hard-coded list agrees with the default and diverges the moment
an operator sets one); and treating an unusable `PATHEXT` as "this
machine has no programs" rather than as a malformed variable.

Every row holds the directory and the file set constant and varies only
`PATHEXT` and the naming rule.

## `fn a_windows_name_is_pathext_and_never_the_extensionless_file() {` › `for absent in [None, Some(""), Some(";;;"), Some("exe"), Some(".")] {`

The default is the platform's, in the platform's order: `.COM` first.

## `fn a_windows_name_is_pathext_and_never_the_extensionless_file() {` › `assert_eq!(`

And it really is read, not assumed: a PATHEXT naming one extension
nobody ships picks that file over the `.exe` sitting beside it.

## `fn a_windows_name_is_pathext_and_never_the_extensionless_file() {` › `assert!(`

The widening that must not happen: `x` exists and is never chosen.

## `fn a_windows_name_is_pathext_and_never_the_extensionless_file() {` › `for pathext in [None, Some(REAL_PATHEXT), Some(".FOO")] {`

Unix has no extensions: the bare file is the only answer, and
`PATHEXT` changes nothing.

## `fn a_windows_name_is_pathext_and_never_the_extensionless_file() {` › `let value = OsString::from(".FOO");`

A name that carries an extension is tried verbatim first, under a
PATHEXT that would otherwise send it elsewhere.

## `fn a_windows_name_is_pathext_and_never_the_extensionless_file() {` › `let shadow = root.join("shadow");`

And a directory is not a program, on either rule.

## `fn a_candidate_without_the_execute_bit_is_skipped_the_way_execvp_skips_it() {`

A candidate without the execute bit is skipped, the way `execvp` skips
it.

The regression this guards is silent and platform-specific: `execvp`
walks past a non-executable file and finds the real installation further
along `PATH`, so a resolution that stopped at the first *existing* file
would refuse — or spawn `EACCES` — where the code it replaced ran. Two
directories, same name, and the answer must be the second.

## `fn a_candidate_without_the_execute_bit_is_skipped_the_way_execvp_skips_it() {` › `assert_eq!(`

Windows has no such bit, so the same file is a program there — the
two rules must not be the same rule.

## `fn a_name_that_matches_nothing_is_refused_and_an_empty_path_entry_is_never_searched() {`

A name that matches nothing is refused, naming the name and the
boundary — and an empty `PATH` entry is never searched.

Fail-closed is the choice the repair round is held to: the alternative
is handing the name to `Command` anyway and taking a `NotFound` that
names no boundary, which on Windows is the failure an operator could not
diagnose. The empty-entry rule is here because this is the site where it
would actually execute: an empty `PATH` entry means "the current
directory" to some shells, and this runner's current directory is the
workspace — repository content, under automation.

## `fn a_name_that_matches_nothing_is_refused_and_an_empty_path_entry_is_never_searched() {` › `let mixed = path_of(&[Path::new(""), &dir]);`

The empty entry is skipped rather than treated as "here", and a real
directory beside it is still searched: the count is the observable.

## `fn a_bare_name_that_only_pathext_resolves_runs_through_the_host_runner() {`

**`PR6D-001`.** A bare name that only `PATHEXT` resolves is executed by
the host runner — the npm-installed agent CLI, spawned the way
production now spawns it.

`PATHEXT` lists `.CMD`; `CreateProcessW` appends `.exe` and nothing
else, and neither does Rust's `Command`. So a `CommandSpec.program` of
`claude` with `claude.cmd` on `PATH` failed with `NotFound` on every
Windows host, for the probe, the worker, the gate, the review and the
re-ask. The suite could not see it: every `.cmd` fixture in this crate
used an **absolute** path, which is a different property, and the guest
has none of the three CLIs installed.

The platform fact is asserted **in this test**, not cited: the same
bare name is handed to `std::process::Command` under the same composed
environment first, and must fail with `NotFound`. Without that row the
claim below would pass on a platform where the bug never existed. Two
shims, two extensions, two markers and two arguments, counted as
distinct values so a fixture that ran one shim twice reports it; what is
held constant is the `PATH` directory, the runner and the composed
environment.

## `fn a_bare_name_that_only_pathext_resolves_runs_through_the_host_runner() {` › `let stem = format!("upstroke-d1-{}", crate::ulid::ulid());`

Unique per run, so nothing on this machine's real PATH, in the
application directory or in the system directories can satisfy it.

Both arguments are **benign**, for `bin.rs`'s own reason: `%~1`
strips the quotes the child received, so a `&` in the value would be
re-parsed by `cmd.exe` as a command separator *inside the shim* and
this case would be measuring batch re-parsing instead of resolution.
Argument escaping through a batch target is
`agent::bin::tests::arguments_reach_the_command_untouched`'s subject
and is unaffected by which file a name resolved to. Measured on the
guest: with `second & argument` the `.bat` shim exits 1 with
"'argument' is not recognized", and the `.cmd` shim beside it — same
resolution, benign argument — passes.

## `fn a_bare_name_that_only_pathext_resolves_runs_through_the_host_runner() {` › `let mut direct = Command::new(name);`

The platform fact, executed. `std` searches the child PATH, the
application directory, the system directories and the parent
PATH, appending `.exe` to each — so a `.cmd`/`.bat` on PATH is
invisible to it, and this is `PR6D-001` itself.

## `fn a_bare_name_that_only_pathext_resolves_runs_through_the_host_runner() {` › `let output = runner`

The claim: the same name, through the runner.

## `fn two_runners_in_one_process_resolve_one_name_against_their_own_environments() {`

Two runners in one process resolve one name against **their own**
environments.

The hazard the container runner introduces and this repair must not
reintroduce: a resolution remembered anywhere — a `OnceLock`, a field, a
process-wide cache — hands the first boundary's answer to the second,
and a value that is correct on first use and wrong on the second is
invisible to any test that constructs one runner.
`agent::built_program_tests` holds that for the adapters; this holds it
for the boundary, with real spawns.

Both orders, because "the first caller wins" is a property of order, and
the markers are counted as distinct values.

## `fn an_absolute_program_is_spawned_as_given_even_when_path_holds_that_name() {`

An absolute program is spawned as given, even when a `PATH` directory
holds a different file of the same name.

"An absolute `program` must spawn exactly as today" is the constraint
this repair was given, and a resolution that re-resolved one would be
invisible to every existing fixture: they all put the *only* copy of the
program at that path. Here there are two copies, so "used as given" and
"searched for" produce different output. The space in the directory name
is `bin.rs`'s own production shape, `C:\Users\John Smith\npm\`.

## `fn an_absolute_program_is_spawned_as_given_even_when_path_holds_that_name() {` › `let by_name = runner`

The control: the same runner, the same name without its directory,
reaches the other file — so the two really are distinguishable and
the row above is not passing by coincidence.

## `struct ResolutionWitness {`

Everything one spawn's containment observers can see about resolution.

## `struct ResolutionWitness` › `at_points: Arc<Mutex<Vec<(SubEffectPoint, u64)>>>,`

`program_resolutions()` as each containment point saw it, in order.

## `fn a_program_is_resolved_once_per_spawn_before_any_of_it_and_never_before_compose_refuses() {`

One program name is resolved **once per spawn**, **before** any of the
spawn, and **not at all** when the request is refused earlier.

Ordering is a set of independently droppable predicates and a suite that
proves only "the right file ran" holds none of them. The observable is
the sequence: [`program_resolutions`] read at every containment point,
which are the coordinates the funnel passes through between
`CreateProcess`/`fork` and the running child. A resolution that happened
twice shows a second increment; one that happened lazily at spawn time
shows a count that is still at the baseline when the first point fires;
one that happened before `compose` shows an increment on the request
`compose` refuses.

Both program shapes, because "once" must hold on both branches of the
resolution — a bare name that is searched for, and an absolute path that
is not — and the bare name is a `PATHEXT`-resolved batch shim, which is
the intersection {a name to resolve} x {a file `CreateProcessW` reaches
only through an interpreter} that no fixture in this crate had.

## `fn a_program_is_resolved_once_per_spawn_before_any_of_it_and_never_before_compose_refuses() {` › `let witness = ResolutionWitness::new();`

A request the environment refuses is refused **before** anything is
resolved: `compose` runs first, so a reserved-key overlay never
reaches the filesystem and never reaches a containment point.

## `fn a_program_is_resolved_once_per_spawn_before_any_of_it_and_never_before_compose_refuses() {` › `let witness = ResolutionWitness::new();`

And a name that resolves to nothing is refused **after** resolution
and **before** any of the spawn: the count moves, the points do not.

## `fn every_bare_program_this_crate_ships_goes_through_one_resolution_rule() {`

Every bare program this crate ships goes through **one** resolution
rule.

Two rules is how `PR6D-001` happened: the shells' bare names worked
because a shell is an `.exe`, so nothing noticed that the agent CLIs'
bare names — the same shape, one field over — did not. The names are
written here rather than read from `ShellKind::program` and the adapter
constants, and the exhaustive `match` below ties the written table to the
enum so a sixth shell fails to compile rather than silently leaving the
grid.

What is held constant is the directory, the shim content and the naming
rule; what varies is the name, and the resolved files are counted so a
rule that special-cased one name collapses the count.

## `fn every_bare_program_this_crate_ships_goes_through_one_resolution_rule() {` › `const NAMES: [&str; 8] = [`

The five shells `gates::ShellKind::spec` can put in a spec, and the
three agent CLIs `bin::Invocation::named` can.

## `fn a_resolved_cmd_keeps_the_raw_tail_rule() {`

Resolving `cmd` does not change `cmd.exe`'s raw-tail rule.

`build_command`'s one Windows rule is keyed on the program, and the
program the runner hands it is now the **resolved** one — an absolute
path where the spec carried a bare name. A gate whose command line
changed meaning depending on whether its shell had been resolved is not
"adapter parsing unchanged"; `gates::ShellKind::command` says why the
tail must reach the child un-re-quoted, and this is the half of that
rule which the repair could have broken.

**This has to spawn.** `Command::get_args` yields the same sequence for
`arg` and for `raw_arg`, so an assertion over the built `Command` cannot
tell the two apart — measured: the first version of this test was green
under the mutation it was written for. What distinguishes them is the
command line `CreateProcessW` receives, and the only oracle for that is
the child's own output: std escapes an embedded quote as `\"`, which
`cmd.exe` does not un-escape, so a re-quoted tail echoes `\"quoted`.

The resolved path is the one **this runner** resolves, not a transcribed
`C:\Windows\System32\cmd.exe`, so the case holds on a machine whose
shell lives elsewhere; that it differs from `cmd` is asserted first, or
the two spellings would be one.

## `fn a_resolved_cmd_keeps_the_raw_tail_rule()` › `let mut stdouts = BTreeSet::new();`

Both spellings, spawned. The quotes must arrive as the operator wrote
them, from the resolved path exactly as from the bare name.

## `fn a_resolved_cmd_keeps_the_raw_tail_rule()` › `let workspace = scratch("raw-tail");`

And through the production route, where the bare name is what the
gate ships and the runner does the resolving.

## `fn a_resolved_cmd_keeps_the_raw_tail_rule()` › `let shim = build_command_at(&spec, Path::new(r"C:\npm\claude.cmd"));`

The control: a program that is not `cmd` does not get the rule at
all, so the rows above are not true of everything. An npm shim named
`claude.cmd` has the file stem `claude`, and a rule keyed on the
extension rather than the stem would hand it a raw tail.

## `fn role_request_for(`

-----------------------------------------------------------------------
PR6-LANED-001: one boundary, one executable
-----------------------------------------------------------------------

## `fn role_request_for(`

The request **production** sends for `role`, carrying **this** program.

[`production_request`] fixes the program per role because there the role
is the subject; here the program is the subject and the role is what
varies, so each role's own production builder is handed the same spec.
`Probe(Shell)` returns `None` because its program is not a caller's to
choose — [`shell_probe_request`] writes the recorded shell — and the
`match` is exhaustive, so a role added later has to be classified here
rather than silently leaving the grid.

## `fn one_boundary_executes_one_file_for_a_name_across_a_probe_and_the_attempt() {`

**`PR6-LANED-001`.** One boundary executes **one** file for a name, even
when the filesystem moves between pre-flight and the attempt.

DESIGN.md:612 — "Probes run through that same runner, **or pre-flight
could certify a host CLI/version different from the one the attempt
executes**". Routing the probe through the runner is necessary and is
not sufficient, and this is the fixture that says so: `PATH=first:second`
with the same name in both directories, the probe certifies
`first/<name>`, `first/<name>` is then removed, and a runner that
re-searched per spawn hands the attempt `second/<name>` — a different
executable under an unchanged `CommandSpec.program`. A test asserting
that the two *program strings* agree passes throughout, which is why the
claim it supported was wrong.

**The control is the oracle**: a *fresh* runner over the same
environment, after the removal, does reach `second/<name>` — so the two
files are genuinely distinguishable, the removal genuinely changes the
answer, and the memoised runner's refusal is the memo rather than a
fixture that could not tell them apart.

The second field held constant is the environment — one `HostEnvironment`
for every row, composed the same way — and what varies is the role
(`Probe(Agent)` then `Implement`, which is the pair the passage names)
and the state of the filesystem between them.

**Fail-closed on purpose.** The attempt does not get `second/<name>`; it
gets a spawn failure naming the file pre-flight certified. An operator
reading it learns that the CLI moved under a running run, which is true,
instead of a `Caps.version` that quietly stopped describing anything.

## `fn one_boundary_executes_one_file_for_a_name_across_a_probe_and_the_attempt() {` › `std::fs::remove_file(&first).expect("remove the certified installation");`

The CLI moves under the run: the file pre-flight executed is gone,
and the other one — same name, same PATH, different executable — is
still there.

## `fn one_boundary_executes_one_file_for_a_name_across_a_probe_and_the_attempt() {` › `let fresh = HostRunner::new().with_environment(environment());`

The oracle. A boundary that had not decided yet reaches `second`, so
"resolve per spawn" really does change the answer here.

## `fn one_boundary_executes_one_file_for_a_name_across_a_probe_and_the_attempt() {` › `let outcome = runner.run(&attempt);`

The claim. The runner that certified `first` does not silently run
`second`.

*What* the failure looks like is the platform's, and the two differ:
on Unix the spawn of a vanished file is `ENOENT` and the runner
returns an error naming it, while on Windows `std` runs a `.cmd`
through `cmd.exe`, so the spawn succeeds and the interpreter exits
non-zero. The claim is neither of those spellings — it is that the
attempt did not run the *other* installation and did not report
success — so it is asserted over everything the boundary handed back,
whichever shape it came in.

The text is the child's own, not a `{:?}` of it: a debug rendering
escapes every backslash, and a Windows path searched for inside one
would never be found however right the runner was.

## `fn one_name_is_searched_once_for_a_boundary_and_asked_for_once_per_spawn() {`

One name is **searched once** for a whole run, and asked for once per
spawn.

The identity predicate and the ordering predicate are independently
droppable, so they have two counters: [`program_resolutions`] moves once
per spawn (D1's `a_program_is_resolved_once_per_spawn_…` holds that and
its position in the spawn) and [`program_searches`] moves once per
boundary. A memo that never hits satisfies the first and reopens
DESIGN.md:612; this is the fixture for the second.

**Across roles, and that is the point.** `host-v1` supplies credential
locations *role-scoped* ([`supplies_credentials`]), so a probe's
composed environment and a gate's differ — asserted here as a
distinct-value count before anything else, because a memo keyed on the
composed environment would miss on exactly the pre-flight/attempt pair
:612 requires to agree, and this test would then be reporting that four
spawns searched four times for a good reason. The key is the three
fields that decide the answer, not the environment.

`Probe(Shell)` is the one role absent: its program is the recorded
shell, not a caller's choice. [`role_request_for`] is exhaustive over
[`ExecutionRole`], so it is absent by classification rather than by
omission.

## `fn one_name_is_searched_once_for_a_boundary_and_asked_for_once_per_spawn() {` › `let on_path = environment_on_path(&[&bin], Some(REAL_PATHEXT));`

`host-v1` supplies a credential location only when the *base* carries
it, so the base has to carry one for the roles to differ at all —
this machine's own environment has no `CLAUDE_CONFIG_DIR` and every
role would otherwise compose the same thing.

## `fn one_name_is_searched_once_for_a_boundary_and_asked_for_once_per_spawn() {` › `let composed: BTreeSet<Vec<(OsString, OsString)>> = requests`

The premise: these roles really do compose different environments, so
"one answer for all of them" is a claim and not a tautology.

## `fn a_resolution_question_is_the_program_and_the_environment_that_answers_it() {`

What the memo is keyed on: the program **and** the environment that
answers for it.

The hazard the repair introduces. A memo keyed on the program name alone
is a new way to certify the wrong executable — the same defect one layer
in — and it is invisible to every fixture above, because in production
`PATH` is reserved and constant for a run. So this asks the boundary the
same name under two environments and requires two answers.

`program_for` directly rather than through `run`, because the only
composed value a caller can vary within one runner *through* `run` is
`PATHEXT` (an overlay may not name `PATH`, which is reserved), and
`PATHEXT` decides nothing on Unix. Both fields of the key are then
exercised on both platforms, which is the property D1's `ProgramNaming`
exists to preserve.

Held constant: the runner, the name, and the naming rule. Varied: the
composed `PATH`, and — on Windows, where it decides anything — the
composed `PATHEXT`. The resolved files are counted as distinct values.

## `fn a_resolution_question_is_the_program_and_the_environment_that_answers_it() {` › `{`

The other field of the key, where it decides anything.

## `fn a_refused_name_is_refused_identically_without_asking_the_filesystem_again() {`

A name this boundary refused stays refused, in the same words, without
asking the filesystem again.

The failure branch of the memo, and the one where fail-open would be
easy: not remembering a refusal means a run whose pre-flight could not
find `claude` silently finds one at the third attempt because something
installed it meanwhile — pre-flight certifying an absence the attempt
does not honour, which is DESIGN.md:612 with the polarity flipped.

**The control is the oracle.** After the CLI appears, a *fresh* boundary
does run it. So the second refusal is the memo holding, not a fixture in
which nothing changed.

The replayed error is required to be the first one **byte for byte**,
which is what makes storing the refusal as its message safe:
[`UpstrokeError::Refused`] displays as exactly its message, and if that
ever stops being true this row says so.

## `fn a_refused_name_is_refused_identically_without_asking_the_filesystem_again() {` › `marker_shim(&bin, &shim_file_name(&name), "LATE");`

The CLI appears under the run.

## `fn a_refused_name_is_refused_identically_without_asking_the_filesystem_again() {` › `let fresh = HostRunner::new().with_environment(environment());`

The oracle: it really is installed now.

## `fn production_reaches_a_spawn_through_one_host_runner_per_run() {`

Production reaches every spawn of a run through **one** `HostRunner`.

The memo is per boundary, so "the probe and the attempts agree" is only
true while the probe and the attempts share a runner. Nothing in the
type system says so: `run_harness_on` takes `&dyn Runner`, and an engine
that constructed one per attempt would leave the memo correct, the suite
green, and DESIGN.md:612 reopened. This is the census that fails first.

Structural rather than behavioural, for
`the_adapters_hold_no_process_wide_resolution_state`'s reason: a runner
constructed per attempt is indistinguishable from one constructed per run
in every observation except how many times the filesystem was asked, and
that observation belongs to a fixture that would have to drive a whole
engine run against a moving filesystem.

The expectation is written out — two construction sites, both in
`src/engine/mod.rs`, being `run_harness` and `resume_harness` — rather
than counted from the tree, because a count read from the tree grows
with it.

## `fn production_reaches_a_spawn_through_one_host_runner_per_run() {` › `const SITES: [(&str, usize); 6] = [`

Where a `HostRunner` is constructed in the engine's production code,
and how many times in each file.

## `fn production_reaches_a_spawn_through_one_host_runner_per_run() {` › `assert_eq!(`

The injected control contains comments, literals, a typed test function and one later production construction. It must add exactly one count alone and when appended to each of the six source files.

## `fn production_reaches_a_spawn_through_one_host_runner_per_run() {` › `let engine = crate::effects::production_code(include_str!("../../engine/mod.rs"));`

And the two are the run and the resume facade, each of which then
borrows that one runner for pre-flight and every attempt.

## `fn an_npm_style_installation_runs_by_bare_name_exactly_as_it_runs_by_path() {`

**`PR6-LANED-002` / refuted claim 3.** An **npm-style** installation of
each of the three agent CLIs runs by bare name exactly as it runs by
path.

The equivalence `agent::built_program_tests::the_host_runner_executes_a_
bare_program_name_as_it_executes_the_resolved_path` claims, over the
installation shape that one cannot express. That row uses `git` — a
native `.exe`, which `CreateProcessW` reaches from a bare name whether or
not this runner resolves anything — so it was green on Windows while
`PR6D-001` was live. The three agent CLIs are not installed that way:
`npm install -g` writes `claude.cmd`, `codex.cmd`, `copilot.cmd`, and a
`.cmd` is reachable only through `PATHEXT`. A fixture that cannot hold
the failing installation is a correlated fixture, whatever it asserts.

**All three names, because the behaviour that was dropped was not one
adapter's.** `PR6D-CODEX-STORE-ALIAS-WALK-DROPPED` records the deletion
as codex's Windows-Store-alias walk; the same deletion took `.cmd`/`.bat`
selection and `PATHEXT` away from `claude` and `copilot`, where a plain
npm install fails with no Store alias and no competing `PATH` entry in
sight. So the grid is the three names, each with its own marker, and the
markers are counted — a rule that special-cased one name collapses the
count.

The installation is `<name>.cmd` on Windows and an extensionless script
on Unix, with **no `.exe` beside it**, on a `PATH` this test wrote so
that nothing installed on the machine can satisfy it. Both spellings —
the bare name, and the absolute path of the file it must resolve to — go
through the production route, `HostRunner::run`, and must produce the
same output; the two program strings are asserted to differ first, or
this would compare a thing with itself.

It runs on **both** platforms. On Unix the property holds because
`execvp` would have satisfied it anyway, which is precisely why the
Windows arm needs a fixture that can fail rather than a `#[cfg(windows)]`
afterthought; D1's `a_bare_name_that_only_pathext_resolves_…` holds the
`NotFound` platform fact on the guest, and this holds the equivalence
everywhere.

What varies: the CLI name and the spelling of the program. Held constant:
the runner, the environment, the argument and the files on disk — so a
difference in output can only be a difference in which file ran.

## `fn an_npm_style_installation_runs_by_bare_name_exactly_as_it_runs_by_path() {` › `const CLIS: [&str; 3] = ["claude", "codex", "copilot"];`

The three names `bin::Invocation::named` ships, written here rather
than read from the adapters' private `CLI` constants;
`agent::built_program_tests::an_adapters_program_is_the_boundarys_…`
is what ties each adapter to its own name.

## `fn an_npm_style_installation_runs_by_bare_name_exactly_as_it_runs_by_path() {` › `let mut contents: Vec<String> = std::fs::read_dir(&bin)`

The installation really is the failing shape: one file per CLI, and
on Windows not one of them is an `.exe`.

## `const PATH_ENTRY_TABLE: &[(&str, bool, bool)] = &[`

-----------------------------------------------------------------------
PR6-LANED-003: the workspace is not a PATH directory
-----------------------------------------------------------------------

## `const PATH_ENTRY_TABLE: &[(&str, bool, bool)] = &[`

A `PATH` entry, and whether each platform calls it a location on its
own.

Written from the two platforms' rules rather than read from
`Path::is_absolute`, and then checked against it for the platform this
is running on — the Windows column on the guest, the Unix column here.
Every entry is free of both `PATH` separators, so one entry stays one
entry under `std::env::split_paths` on either platform.

## `("", false, false),`

(entry, is a location under Unix, is a location under Windows)

The empty entry: `PR6-LANED-003` itself. POSIX gives a null prefix
the meaning "the current directory".

## `("/usr/local/bin", true, false),`

Rooted on Unix; on Windows a leading separator is relative to the
*current drive*, so it is still a current-directory question.

## `(r"\\server\share\bin", false, true),`

A UNC share names a location on Windows and is an ordinary file name
on Unix.

## `fn every_path_entry_this_runner_searches_names_a_location_on_its_own() {`

Every `PATH` entry this runner searches names a location on its own.

**`PR6-LANED-003`**, as a rule rather than as one vector. A `PATH` entry
that is not absolute is resolved against *a* current directory, and this
boundary has two of them: the coordinator's, which is what
`ProgramNaming::is_program` inspects, and the workspace, which is what
the child actually runs in. So such an entry does not merely widen the
search — it lets the runner certify one file and execute another, and
the file it executes is repository content under automation
(DESIGN.md:398-402).

The written table is checked against `Path::is_absolute` first, so the
row below is a claim about `resolve_program` and not about `std`. What
varies is the entry; what is held constant is the program name, the
candidates and the composed environment.

## `fn every_path_entry_this_runner_searches_names_a_location_on_its_own() {` › `let bin = root.join("bin");`

And "searched" means searched: the one kind of entry that is not
skipped does find a program in it.

## `fn an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name() {`

**`PR6-LANED-003`.** An empty `PATH` entry never reaches the
workspace's own copy of a bare name.

The finding's vector, executed. A coordinator whose `PATH` holds an
empty segment — `:/usr/bin`, which is what a shell profile that appends
to an unset `PATH` produces — and a request workspace containing an
executable called `claude`. POSIX gives the null prefix the meaning "the
current directory", and this runner's current directory *is* the
workspace, so a bare name handed to `Command` runs repository content
with the coordinator's authority as the agent (DESIGN.md:398-402).

**The platform fact is executed, not cited**: every row spawns the same
bare name through `std::process::Command` under the same composed
environment and the same working directory first, and the outcome is
compared against a written expectation. Three of the four rows are ones
where the raw spawn reaches the workspace and the runner must not, and
that count is asserted — a fixture in which the two agree everywhere
proves nothing and says so.

Unix only, and deliberately: the empty-entry-means-here rule is POSIX's,
and the fixture's installations are shell scripts. The rule that closes
it is not Unix-only —
`every_path_entry_this_runner_searches_names_a_location_on_its_own`
executes it on both platforms.

What varies: where the empty entry sits, and whether a real installation
is on the `PATH` at all. Held constant: the workspace, its planted
executable, the name and the argument.

## `fn an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name() {` › `marker_shim(&workspace, &file, "WORKSPACE");`

Repository content, under automation: the workspace's own copy.

## `fn an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name() {` › `let rows: [(&str, String, &str, &str); 4] = [`

(what, PATH, what a raw spawn reaches, what the runner must reach)

## `fn an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name() {` › `let mut direct = Command::new(&name);`

The platform fact. `execvp` searches the child's PATH from the
child's working directory, so the empty entry is the workspace.

## `fn an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name() {` › `let witness = ResolutionWitness::new();`

The claim, through the boundary, with an observer so that a
refusal is a refusal *before* any spawn rather than a failed one.

## `fn a_relative_path_entry_is_refused_even_when_it_names_a_real_directory() {`

A relative `PATH` entry that really does name a directory is refused
rather than searched.

The empty entry of `PR6-LANED-003` is the degenerate case of this one,
and this is the case where the two current directories the boundary has
are visibly different: the entry resolves from the **coordinator's**
working directory — asserted here, so the row is hostile — while the
child would resolve it from the **workspace**. A runner that searched it
would hand `Command` a relative program and certify a file that is not
the one that runs.

The entry is built out of `..` back to the root and down again, so it is
genuinely relative and genuinely resolvable without anything being
written inside the repository. Unix only: the same construction on
Windows depends on the temporary directory and the working directory
sharing a drive, and a row that silently stops applying is worse than
one that is not there.

## `fn a_relative_path_entry_is_refused_even_when_it_names_a_real_directory() {` › `let mut workspace = root.clone();`

Deeper than the coordinator's own directory, so that the same
relative entry names a *different* place from each of them — which is
the whole hazard, and a workspace that happened to sit at the same
depth would hide it.

## `fn a_relative_path_entry_is_refused_even_when_it_names_a_real_directory() {` › `assert!(`

The row's own premise, both halves.

## `fn a_relative_path_entry_is_refused_even_when_it_names_a_real_directory() {` › `let reachable = HostRunner::new()`

The oracle: the same directory, named as a location, does run.

## `#![allow(`

Allowlist placement: the funnel section of `effects/allowlist.toml`, which
carries this module's review clause. `effect_site_inventory.mechanism` (2).

## `struct HeldFork {`

This guard owns one forked child's lifetime. Its socket releases the child
after observation; shutdown also releases it on failure. The child closes
the parent's socket endpoint before announcing readiness and exits after
release or its read timeout. Drop waits for that exact pid, never a group.

## `let waited = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };`

SAFETY: pid is this guard's unreaped child and status is writable.
WNOHANG observes whether it exited without waiting for release.

## `let result = unsafe { libc::waitpid(self.pid, &mut status, 0) };`

SAFETY: pid is our unreaped direct child; status is a live,
writable c_int. The socket's read timeout bounds the child.

## `unsafe {`

SAFETY: only the parent returns into Rust after fork. The child
uses only close/write/read/_exit, all async-signal-safe, with live
inherited fds and one-byte stack buffers. It never allocates,
unwinds, drops Rust owners, or touches inherited locks. Closing
parent_fd removes its copy of the release endpoint. The separate
child endpoint stays live until _exit closes all inherited fds.

## `let polled = unsafe { libc::poll(&mut ready, 1, 1000) };`

SAFETY: ready is one initialized pollfd and remains live for this
call; its descriptor is borrowed from reader. The wait is bounded.

## `assert_eq!(`

SAFETY: name is a live NUL-terminated path in our private scratch
directory, and 0600 grants access only to this test's user.

## `let installed = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_SETPIPE_SZ, 4096) };`

SAFETY: reader owns a live FIFO descriptor. F_SETPIPE_SZ takes an
integer size, and shrinking an empty pipe needs no extra privilege.

## `const CONTROL: &str = r##"`

STRIP-CONTROL goes through the same whole-file blanker and counter as
production. Its only production construction follows a test-only item,
so truncating at the first #[cfg(test)] also fails this control.

## `let engine = crate::effects::production_code(include_str!("../../engine/mod.rs"));`

And the two are the run and the resume facade, each of which then
borrows that one runner for pre-flight and every attempt.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {`

The census that holds this module's site-placed reasoning in step with the copy
kept here. `standards/11_standards_unsafe_and_platform_code.md` requires an
adjacent `SAFETY:` comment stating each obligation and why it holds, and
`standards/10_standards_concurrency.md` a written protocol for shared state and
its cleanup. Section 13 moves a module's prose into this file but says the
reasoning another standard requires *at* a site is that standard's to place, and
that a module with a notes file is not excused from it.

PR #157 moved the prose here and left only the opening line of each block behind,
separated from its own operation by a blank line, so a reader at the six `unsafe`
operations in `inherited_writer` met a sentence that stopped mid-clause -- "The
child", "a live,", "for this" -- and the `HeldFork` lifetime protocol had no site
copy at all. `PR157-ASTRA-SITE-SAFETY` is that finding. Nothing in the diff that
caused it touched an executable token, which is why nothing failed.

The census reads this module and this file through `include_str!` and holds them
equal in both directions: every operation carries an adjacent complete
obligation, the multiset of site obligations equals the multiset recorded here,
and the guard's protocol sits beside its type. Four separate mutations were run
against it -- adjacency lost, a block shortened but left adjacent, the protocol
deleted from the site, the notes copy edited away from the site copy -- and each
is detected by a different assertion.

It is deliberately outside the `#[cfg(target_os = "linux")]` module it censuses,
so all three Clippy and test legs evaluate it. `include_str!` embeds the
checked-out bytes, so a Windows checkout under `core.autocrlf` supplies `\r\n`;
each line is stripped of a trailing `\r` and every comparison is over
whitespace-separated words, which absorbs a rewrap of either copy as well.
Reading at compile time also keeps the census off the runtime effect surface
this module's allowlist row governs.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `const KEYWORD: &str = "unsafe";`

The census reads the file it lives in, and the keyword is spelled in full here
because the scan never sees this line: string literals are blanked before the
scan, so the constant, the fixtures the controls build, and every message that
names the keyword are prose to it. The first census spelled the needle in two
pieces to keep from finding itself and skipped only whole-line `//` comments,
so a literal or a block comment containing the word was read as an operation
with no obligation; `PR157-ASTRA-SITE-SAFETY-R2-001` is that finding, and the
blanked view is its repair.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `fn comment_body(line: &str) -> Option<&str> {`

The text of a `//` comment line, or `None` for code, a blank line, or the module
header. Returning `None` for a blank line is what catches the separator PR #157
left behind: a block is adjacent exactly when the line above the operation is a
comment.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `fn block_ending_at(lines: &[&str], above: usize) -> Option<String> {`

The whole comment block whose last line is `above`, normalised to one line of
words. Walking upward from the operation rather than downward from the comment is
what makes a truncated block visible: the remnant is still a comment, but it is
no longer the line above. It reads the source lines, not the blanked view, so
the obligation is compared as the reader sees it.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `const EXPRESSION_MACROS: [&str; 21] = [`

A keyword token is not an operation until the census knows what the text
around it is. The `assert_eq!(` above the `mkfifo` call is the reason this
list exists: the block inside it is a real operation, expanded as an
expression, and a census that skipped macro input would lose that site. These
are the standard macros whose input is expressions; the keyword inside one of
them is read as code. `INERT_MACROS` are the standard macros whose input is
never expanded as code -- `stringify!`, `concat!`, `include_str!` and their
kin -- and the keyword inside one of them is prose;
`PR157-ASTRA-SITE-SAFETY-RECOVERY-R3-001` is the finding that
`stringify!(unsafe)` was read as an operation. A macro on neither list is a
boundary the census does not cross: the keyword inside it is refused with the
macro's name, because the census cannot tell whether that macro expands its
input, and a guess in either direction is the defect this census exists to
refuse. An inert macro anywhere in the nest wins, since nothing inside it is
expanded; otherwise the first unknown macro in the nest refuses.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `fn keyword_tokens(code: &str) -> impl Iterator<Item = usize> + '_ {`

Every offset in the blanked view where the keyword is a token: an occurrence
with no identifier-continuing character on either side, and neither `#` nor
`'` before it. Edition 2024 admits a keyword as a raw identifier, so `r#unsafe`
is a binding, not an operation, and a splitter that treats `#` as a delimiter
reads it as one; `PR157-ASTRA-SITE-SAFETY-RECOVERY-R1-001` is that finding. No
other Rust token puts `#` directly before the word: an unsafe attribute opens
with `#[`, so it still counts. A `'` before it would be a lifetime or a label,
which Rust refuses to name after a keyword, so that shape cannot occur in a
module that compiles and is excluded rather than counted.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `fn macro_invocations(code: &str) -> Result<Vec<(&str, usize, usize)>, String> {`

Every macro invocation in the blanked view as its name and the offsets of its
token tree's delimiters: an identifier, `!`, optional whitespace and one of
`(`, `[`, `{`, closed by the matching delimiter counted over all three kinds,
which is reliable only because literals and comments are already blank.
`macro_rules! name { ... }` carries the defined name between the `!` and the
brace and is skipped over the same way. A `!` with no identifier before it --
negation, `!=`, the never type -- names no macro. A token tree that never
closes is a refusal, not a silent end of the scan.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `fn continues_an_identifier(character: char) -> bool {`

What may follow or precede the keyword inside one identifier, decided without
a Unicode table. Rust identifiers continue with `XID_Continue`, which
`char::is_alphanumeric` does not cover: U+0301 COMBINING ACUTE ACCENT continues
`unsafé` but is not alphanumeric, so a test on that predicate read the
identifier as the keyword; `PR157-ASTRA-SITE-SAFETY-RECOVERY-R2-001` is that
finding. In a module that compiles, the only characters that can stand
directly beside a token are ASCII punctuation, ASCII alphanumerics and `_`,
identifier characters, and Rust's whitespace, which is exactly Unicode
`Pattern_White_Space` -- the eleven characters of the constant above -- so the
rule is: ASCII alphanumerics and `_` continue an identifier, every other ASCII
character does not, and every non-ASCII character does unless it is one of
those eleven. That errs, if at all, toward not counting: a non-ASCII character
that is neither identifier nor whitespace is a lexer error, not a token
boundary, and the census does not run over text the compiler refused.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `fn site_obligations(source: &str) -> Result<Vec<String>, String> {`

The scan proper, over any source text, so the controls below can run it over a
fixture built from this module and observe a refusal as a value rather than a
panic. The operations are located in `crate::effects::blank_comments_and_strings`'
view of the text, which section 12 requires of a census of Rust structure: the
blanker keeps every position and every newline and replaces comments and
string, character, byte-string and raw-string literals with spaces, and carries
its own fixture proof in the effects suite, so this census does not write a
second one. The line count of the two views is asserted equal before they are
walked side by side; each keyword token of the blanked view is then placed in
the macro nest around it, and when it is code the obligation for its line is
read out of the original text at the same index, where the comment still is.
A keyword token in code position is always a construct in a module that
compiles -- a block, `fn`, `impl`, `trait`, `extern` or the attribute form --
so the census does not test the shape after it: a check no fixture of valid
Rust can reach would be a seam without a witness.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `let mut opens_at = index;`

The obligation sits above the statement, and a macro or call may open that
statement on an earlier line -- `assert_eq!(` above the `mkfifo` call. The walk
crosses those openers and nothing else: it reads the blanked view, where a
comment line or a line inside a literal is blank and so never ends with `(`,
and it never crosses a blank line or unrelated code, so "adjacent" keeps its
meaning.

## `fn every_site_obligation_is_complete_and_agrees_with_its_notes_copy() {` › `let lines_here = SOURCE.lines().count();`

The census's own controls, section 12's positive control included, each built by
appending to the real module so the domain is the whole file and not a
fixture shaped to pass. The keyword inside a string literal, a raw string
literal, a multi-line string literal, a one-line and a multi-line block comment
and a trailing `//` comment, as a raw identifier, as the start of a longer
identifier, followed by a combining mark or a precomposed letter that makes it
another identifier, and inside `stringify!` and `concat!` -- alone, nested in
`assert_eq!`, and as a whole block -- must leave the obligations exactly as
they were; an operation appended with nothing above it -- with U+2028 LINE
SEPARATOR, Rust whitespace that is not ASCII, between the keyword and its
block -- must be refused at its own line number, which is also the proof that
positions are preserved and that Rust's whitespace bounds the token; the same
operation inside `assert_eq!` must be refused the same way; the keyword inside
a macro on neither list must be refused with that macro's name; and an operation appended under a fresh
`SAFETY:` line must contribute exactly that line's text, read from the source
where the blanked view has already erased it.
