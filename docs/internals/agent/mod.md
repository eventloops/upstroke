# `src/agent/mod.rs`

Extended notes for [`src/agent/mod.rs`](../../../src/agent/mod.rs).

[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/agent/mod.rs).

The code defines current behavior. These notes preserve contracts and implementation
history. Search each backticked heading fragment separately in the source.

References below to `decisions.*` and `INV-18` use retired v0.2 planning identifiers.
They record implementation history and do not add current requirements.
[DESIGN.md](https://github.com/eventloops/upstroke/blob/master/DESIGN.md#retired-records)
is the living design authority.

## Module

Agent adapters (DESIGN.md §8, §16): turn a `TaskRun` into a subprocess of
an official agent CLI and parse what came back. Adapters never edit files,
never commit, and never speak HTTP — they only build commands and read
process output. One file per agent.

## `pub enum AuthState {`

Whether the vendor's CLI says it is signed in.

Three states, not two. "Could not tell" must never render as "not
connected": `upstroke connect` writes a file an operator then trusts, and a
confident *wrong* "you are not logged in" sends them to re-authenticate an
account that was fine.

## `impl std::fmt::Display for AuthState {`

One rendering, used by `connect` and `capacity` alike.

There were two: a terse `Display` here and a fuller `describe_auth` in
`connect`, so the same fact read as "not authenticated" from one command and
"NOT signed in — log in with the vendor's own CLI before running" from the
other, and an operator comparing them could not tell whether they described
the same thing. The rule this enum exists to enforce — "could not tell"
never renders as "not connected" — was then enforced in one place and merely
observed in the other.

## `pub struct Discovery {`

What one agent's CLI could be got to say about itself, without the network
and without touching a credential (invariants 2 and 5).

## `pub struct Discovery` › `pub models: Vec<String>,`

The models the CLI itself advertises.

Empty on Claude Code and Copilot today: as of Aug 2026 neither offers
non-interactive model enumeration. Codex exposes its local roster via
`debug models`; its adapter validates model × effort support at probe
and reports the slugs here. The seam lets every real listing be
cross-checked against the shipped catalog; [`Caps::model_list`] is the
gate.

## `pub struct Discovery` › `pub shape: Option<PoolKind>,`

§13's pool-kind hint, read from whatever the CLI says about the account
it is signed into. `None` means it said nothing conclusive, and the
caller picks a documented default rather than guessing.

## `pub struct Discovery` › `pub notes: Vec<String>,`

Everything the operator should know about how this was worked out —
including what could not be.

## `impl Discovery` › `pub fn unknown() -> Self {`

What an adapter that does not implement discovery reports: nothing,
said out loud.

## `pub struct Caps {`

Capabilities discovered by `probe()` at pre-flight (§14). Copilot's CLI
has shipped breaking flag removals, so capability probing is load-bearing,
not decorative.

## `pub struct Caps` › `pub version: String,`

Version string as reported by the binary, best-effort.

## `pub struct TaskRun {`

Everything an adapter needs to build one attempt's subprocess. The engine
materializes the prompt (§14: body + acceptance + artifacts + conventions
brief) — adapters never re-derive it.

## `pub struct TaskRun` › `pub prompt: String,`

Fully materialized prompt, delivered on stdin.

## `pub struct TaskRun` › `pub workspace: PathBuf,`

Working directory for the subprocess (the workspace repo root).

## `pub struct TaskRun` › `pub gate_cmds: Vec<String>,`

The gate commands this profile may run, and nothing else (§20). Empty
for reviewers, which run nothing at all.

Carried on the run rather than only handed to
[`AgentAdapter::materialize_permissions`] because not every agent has a
settings file to put them in: Copilot's permission surface is argv, so
its `build` needs them at command-construction time.

## `pub struct TaskRun` › `pub resume_session: Option<String>,`

Same-rung retry: resume this session with feedback instead of starting
fresh (§11.4).

## `pub struct TaskRun` › `pub settings_path: Option<PathBuf>,`

Per-run permission settings file, materialized by the engine from
[`claude::permission_settings`]-style generators (§20).

## `pub fn probe_workspace() -> PathBuf {`

Where a pre-flight process runs.

The coordinator's own working directory, which is exactly what a probe
inherited before probes went through the Runner — a probe asks a CLI about
itself and has no workspace of its own. Absolute rather than `"."` because
`runner::host::HostRunner::run` clears the environment, and on Windows the
`=X:` drive-relative variables go with it; every process it starts is given
an absolute directory so none of them can be resolving a drive-relative
path.

## `pub fn probe_request(`

One pre-flight process of `agent`, as a [`RunnerRequest`].

`decisions.pr_sequence[5].scope`: "**probes**, workers, gates, reviews go
through the Runner", and INV-18 accounts an agent probe the way it accounts
an attempt — "every agent CLI invocation **incl. agent probes** acquires
its atomic {agent, pool?} pair" — so the role is `probe(<agent>)`, it is
slotted, and `agent` is set so `host-v1` supplies that agent's credential
location (a probe that could not see the credential directory would certify
a CLI in a state the attempt never runs in).

`ordinal` is **which of this adapter's pre-flight processes this is**. A
pre-flight that runs `--version` and then `--help` runs two processes, and
"unique per process" is the packet's property, so each adapter fixes a
named ordinal per step rather than counting: a counter would renumber every
later step the first time an earlier one was skipped (codex's binary
resolution caches, so its second call skips one), and the identities of one
machine's pre-flight would stop being a function of the pre-flight.

### Errors

[`UpstrokeError::Refused`] when the adapter id cannot appear in an invocation
identity — see [`InvocationId::probe`]. Every shipped id is `[a-z-]`.

## `pub trait AgentAdapter: Send + Sync {`

DESIGN.md §8 `AgentAdapter`.

## `pub trait AgentAdapter: Send + Sync` › `fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError>;`

Locate the binary and report version + capabilities. Ran at pre-flight;
a missing binary is a refusal to start, not a task failure (§19).

Takes the runner because DESIGN.md:209 does — `probe(&self, runner:
&dyn Runner)`, annotated "probes the boundary that will execute" — and
DESIGN.md:612 says why: "Probes run through that same runner, or
pre-flight could certify a host CLI/version different from the one the
attempt executes."

### Errors

A missing or unusable binary, a CLI that has dropped a required flag,
or a runner refusal.

## `pub trait AgentAdapter: Send + Sync` › `fn build(&self, run: &TaskRun) -> Result<CommandSpec, UpstrokeError>;`

Turn one attempt into a **data-only** [`CommandSpec`].

DESIGN.md:117: an adapter "does not decide where the process runs". A
`build` that returned a live `std::process::Command` could carry a cwd,
an environment, or a spawn past the runner, and PR6's container runner
would inherit the hole — so what comes back is a value with a program,
arguments, an environment **overlay** and stdin bytes, and nothing that
names a machine.

### Errors

A refusal to run this profile at all (§19/§20), or a binary that cannot
be located.

## `pub trait AgentAdapter: Send + Sync` › `fn parse(&self, out: &ProcessOutput) -> Result<Outcome, UpstrokeError>;`

Read one attempt's process output as an [`Outcome`].

### Errors

Output this adapter cannot interpret at all.

## `pub trait AgentAdapter: Send + Sync` › `fn discover`

§13's `upstroke connect`: ask this agent's CLI about the account behind
it — signed in or not, what shape its quota is, which models it offers.

Subprocesses the vendor's own CLI and parses what came back. No HTTP, no
token ever handled, no credential file read: a vendor CLI talking to its
own vendor is the design (invariant 2), the same posture §9 sets for
plan importers.

Takes the `Caps` the caller already probed rather than re-probing:
discovery always runs beside a probe (a CLI that cannot report its own
version is in no state to be asked about its account), and an adapter
that called `probe()` again spawned `--version` and `--help` a second
time — four subprocesses where two would do, each carrying the probe
timeout.

The default reports nothing rather than being required, so an adapter
cannot silently claim discovery it does not do — [`Discovery::unknown`]
is an honest "could not tell", and every consumer treats it as one.

### Errors

Whatever asking this CLI about its account failed with. The default
never fails: it asks nothing.

## `pub trait AgentAdapter: Send + Sync` › `fn stdin_payload<'a>(&self, run: &'a TaskRun) -> &'a str {`

What to write to the child's stdin. Delivery is the adapter's call:
CLIs that take the prompt as an argument instead return empty here.

## `pub trait AgentAdapter: Send + Sync` › `fn materialize_permissions(`

Materialize this agent's permission surface (§20) into `dir`, returning
the file the command should reference. Claude Code writes a settings
JSON; Copilot will encode permissions as argv flags and write nothing.

## `pub trait AdapterSource {`

Where a caller finds agent adapters. Injectable so the engine, `connect`
and `capacity` are all fully testable without any real agent CLI on the
machine.

Lives here rather than in `engine` because resolving an adapter id has
nothing to do with running a plan: `capacity` documents itself as a pure
estimator over plain values, and `connect` executes nothing at all, yet both
had to import the execution engine for this two-line trait.

## `pub static ADAPTERS: &[&dyn AgentAdapter] = &[`

Registry in routing order; ids match `WorkerProfile.agent`.

## `pub(crate) fn missing_effort_levels(help: &str) -> Vec<Effort> {`

Shared effort levels the help entry for `--effort` actually advertises.

Looking only for the flag proves too little: several CLI versions exposed
`--effort` with a narrower enum. The option's own wrapped help block is
parsed so unrelated words elsewhere in `--help` cannot masquerade as a
supported value.

## `pub(crate) fn advertises_flag(help: &str, flag: &str) -> bool {`

Whether help advertises `flag` as a whole option token.

Short flags need this instead of substring search: `-p` occurs inside
`--permission-mode`, and `-s` inside several unrelated long options.

## `pub fn looks_rate_limited(text: &str) -> bool {`

Rate-limit signals are ground truth for the capacity engine (§13), so both
adapters read from one vocabulary rather than two that drift apart.

Phrases cover the subscription-window wording Claude Code prints ("5-hour
limit reached", "Weekly limit reached"), Copilot's credit and premium-request
wording (§13's two billing shapes), and API-level errors underneath either.

Only ever consulted for a FAILED attempt: a successful task *about* rate
limiting ("added backoff for 429 responses") must never be read as the pool
being exhausted, or verified work gets rolled back.

## `mod tests` › `fn a_successful_answer_from(id: &str) -> String {`

A stdout every adapter would call a **success**, written in that
adapter's own answer shape.

Load-bearing rather than convenient: it is what makes the supervision
grid below hostile. With a failure payload, dropping the supervision
checks would still report `AgentError` and the cells would pass for the
wrong reason.

## `mod tests` › `fn every_adapter_maps_every_supervision_result_the_same_way() {`

**Every** adapter maps **every** supervision result the same way.

`invariants_preserved[0]` is "process supervision, timeout, output
capture, **adapter parsing** unchanged", and the supervisor's two flags
are inputs to parsing, not to it alone: `output_limited` means the tree
was terminated with the transcript truncated, and `timed_out` means it
was terminated for exceeding its wall clock. A truncated transcript
authorizes nothing (`PR5-CORRECTNESS-013`) and a timeout is a distinct
ladder input from a generic agent failure (`PR5-CORRECTNESS-014`).

The domain is `ADAPTERS`, from the type, and the expectations are
literals — a table, not a re-derivation of the branch order. That is the
point: the two rows where an exit code of 0 meets a set flag are exactly
the rows a re-derivation would get wrong in the same direction the code
would.

Claude and Copilot had direct flag-to-status tests; Codex's flag
fixtures exercised its strict-config *preflight validators*, so no test
had ever parsed an output-limited or timed-out Codex execution. That is
the "guarantee proved for the variant that was looked at" class, and the
guard for it is a domain taken from the type.

## `fn every_adapter_maps_every_supervision_result_the_same_way` › `type SupervisionCell = (`

One supervision shape: name, exit code, `timed_out`,
`output_limited`, the status every adapter must report, and a
substring the detail must carry.

## `fn every_adapter_maps_every_supervision_result_the_same_way` › `assert_eq!(outcome.duration, out.duration, "{cell}: duration");`

The duration is the supervisor's, on every route.

## `fn every_adapter_maps_every_supervision_result_the_same_way` › `assert_ne!(OutcomeStatus::Timeout, OutcomeStatus::AgentError);`

A `Timeout` really is a different answer from an `AgentError`, which
is what makes cells 4 and 5 worth having: the ladder acts on it.

## `mod tests` › `fn an_agent_probe_request_is_slotted_names_its_agent_and_carries_the_probe_identity() {`

What an agent probe *is*, against the two passages that say so.

INV-18: "every agent CLI invocation **incl. agent probes** acquires its
atomic {agent, pool?} pair while gates and the shell probe register
without slots" — so it is slotted and it names its agent.
`decisions.admission_and_leases.permits.invocation_identity`: the third
form is "(probe, target: Agent(name) | Shell, ordinal) at pre-flight",
and the shell probe is the *other* target — so an agent probe's
identity names the agent, never `shell`.

The expected values are written from those sentences, not read back
from the request under test.

## `fn an_agent_probe_request_is_slotted_names_its_agent_and_carries_the_probe_identity` › `assert_ne!(request.invocation.render(), "p.shell.o0");`

The role the request carries and the target its identity carries are
the same agent, and neither is the shell probe's. A request whose
identity said `shell` would be a non-slotted process wearing a
slotted role.

## `fn an_agent_probe_request_is_slotted_names_its_agent_and_carries_the_probe_identity` › `let ids: std::collections::BTreeSet<String> = ADAPTERS`

Every shipped adapter id can be one, and the three are three
distinct identities at the same ordinal — the target is a field, not
decoration.

## `fn an_agent_probe_request_is_slotted_names_its_agent_and_carries_the_probe_identity` › `let ordinals: std::collections::BTreeSet<String> = (0..4)`

And one adapter's successive pre-flight processes are successive
identities, which is what makes "unique per process" hold for a
probe that runs `--version` and then `--help`.

## `mod tests` › `fn a_probe_request_refuses_an_agent_id_that_would_not_survive_a_container_name() {`

An id that could not survive a container name is refused rather than
carried. `decisions.pr_sequence[7].scope` puts an invocation id inside
`<R>/containers/<name>.intent`, and `.` is the identity's own field
separator.

## `mod built_program_tests` › `struct Boundary {`

A boundary with an agent CLI installation the test invents.

**This is the test's oracle, and it is deliberately not this machine's
filesystem.** The property being measured — *which* environment an
adapter's program was resolved against — cannot be measured with a
predicate over the same filesystem production consults: `is_file()` is
true of a host-resolved path whether the resolution was right or wrong,
so the oracle blesses either answer (`PR4-CONF-012`). A boundary the
test invents has an installation the test knows about and the host does
not, so "the boundary decided" and "the coordinator host decided" become
different observations.

It refuses any program that is not the one it has, the way a real
boundary would, and records every request so a boundary that was
**never asked** is distinguishable from one that answered. It also
reports a **version of its own**, because `Caps.version` certifying the
host's CLI while the attempt runs the image's is DESIGN.md:612's
sentence and a distinct failure from either of the other two.

## `struct Boundary` › `installed: String,`

The only program string this boundary will execute.

## `struct Boundary` › `version: String,`

What `--version` prints here. Unforgeable by the host: no machine
has an agent CLI at this version.

## `mod built_program_tests` › `fn expected_name(id: &str) -> &'static str {`

The bare name each adapter's CLI is installed under, **written here**
rather than read from the adapter's own constant: a name compared only
against the code that produced it proves nothing.

## `mod built_program_tests` › `fn scripted_help(cli: &str) -> &'static str {`

The help text each adapter's `--help` contract demands, written here
from the flags each adapter requires rather than captured from a CLI.

## `mod built_program_tests` › `fn scripted_answer(cli: &str, args: &[String], version: &str) -> (i32, String, String) {`

A boundary that can satisfy a whole pre-flight, answering **by
argument** and never by program.

Keying an answer on the program string would make the fixture agree
with whatever the adapter sent, which is the self-oracle this whole
file exists to avoid. The one place the program is consulted is
[`Boundary::run`]'s presence check, which is the boundary's own
question — "do I have this?" — and the thing under test.

## `fn scripted_answer` › `if joined.contains("upstroke_probe_deliberately_unknown") {`

Codex's six strict-config parser probes. The two assignments are
transcribed here rather than read from that adapter's private
constants, so a renamed probe key fails this fixture loudly instead
of silently agreeing with itself.

## `mod built_program_tests` › `fn an_adapters_program_is_the_boundarys_and_the_coordinator_host_is_never_asked() {`

An adapter's program is the **boundary's**, and the coordinator host is
never asked what it has.

This replaces `an_adapters_program_is_the_coordinator_hosts_and_the_
boundary_supplies_none`, which pinned PR4's behaviour deliberately and
which a correct PR6 must fail by name. The property that **moved
across** is the old test's claim 2 — "what pre-flight sends and what the
attempt would send are one program", DESIGN.md:612 in the only form PR4
could hold it. It is now held by
[`the_program_preflight_certifies_is_the_program_the_attempt_would_run`]
in both call orders, because the ordering PR4 needed was a property of
the process-wide resolution cache and that cache is gone. The old
claims 1 and 3 are **inverted** here: the boundary is asked, and a CLI
the coordinator host cannot resolve is no longer refused before it is.

`PR4-ADAPTER-RESOLVES-ON-THE-HOST`'s three separate failures, and where
each dies:

1. *the normal container case is refused before the runtime is asked* —
   every adapter certifies against a boundary the coordinator host does
   not share, and
   [`a_cli_this_host_does_not_have_still_certifies_at_the_boundary`]
   witnesses the absent-on-the-host half on a machine that really lacks
   one;
2. *every spec carries a path that names nothing at the boundary* — the
   program every request carries is one path component, not absolute,
   and equal to the name written in [`expected_name`]; and
3. *`Caps.version` certifies the host's CLI while the attempt runs the
   image's* — the version returned is the boundary's invented `9.9.9`,
   which no installation on any machine reports, and
   [`two_boundaries_in_one_process_each_certify_their_own_cli`] holds it
   across two boundaries.

The second field this holds constant is **what this machine has
installed**: the assertions above are true whichever branch a machine
takes, and both branches are counted so a machine cannot make the test
mean less by having more.

## `fn an_adapters_program_is_the_boundarys_and_the_coordinator_host_is_never_asked` › `match crate::util::find_program(name) {`

The discrimination against the old behaviour, on either kind of
machine. Where this host has the CLI, the old code would have
sent that absolute path and this boundary would have refused it;
where it does not, the old code refused before asking at all.

## `mod built_program_tests` › `fn a_cli_this_host_does_not_have_still_certifies_at_the_boundary() {`

A CLI this coordinator host does not have certifies anyway, because the
boundary has it.

`PR4-ADAPTER-RESOLVES-ON-THE-HOST` failure (1), witnessed rather than
argued: DESIGN.md:612's normal container case is "an image with
version-pinned CLIs", and until PR6 that image's CLI was refused at
pre-flight because *this* machine had no such file.

The premise is asserted rather than hoped for: with all three CLIs
installed here the absence half is unobservable, and a silent skip would
measure nothing while looking green. Both machines this slice is
measured on satisfy it — this box has `claude` and `codex` and no
`copilot`, and the Windows guest has none of the three.

## `mod built_program_tests` › `fn a_cli_this_host_does_not_have_is_refused_by_name_and_by_boundary() {`

The other side of that: when the boundary **is** this host and this host
does not have the CLI, the operator is told so, by name and by boundary.

The fail-closed half of `PR6D-001`'s repair, end to end. The runner's own
refusal is asserted in `runner::host::tests`; this is the sentence the
operator actually reads, which is [`bin::boundary_refused`] wrapping it,
and the two had never been composed. What it must not be is a bare
`NotFound`: before the runner resolved names, an npm-installed CLI on
Windows failed with "program not found" naming no boundary and no
remedy, which is the failure nobody could diagnose from a log.

The premise — that this machine lacks at least one of the three — is
asserted rather than hoped for, the same way its sibling above asserts
it, so a machine with all three installed reports that it cannot observe
this instead of passing vacuously. Both measured machines satisfy it:
this box has no `copilot`, the Windows guest has none of the three.

## `fn a_cli_this_host_does_not_have_is_refused_by_name_and_by_boundary` › `assert!(`

And it is the **resolution's** refusal, not a spawn's. This is
the fail-closed clause itself: a runner that handed the name to
`Command` anyway would produce a `NotFound` here, which names no
boundary and is what an operator cannot act on.

## `fn a_cli_this_host_does_not_have_is_refused_by_name_and_by_boundary` › `assert_eq!(`

One distinct sentence per absent CLI: a refusal that named the first
adapter for all of them would collapse this to one.

## `mod built_program_tests` › `fn the_program_preflight_certifies_is_the_program_the_attempt_would_run() {`

Pre-flight certifies the program the attempt would run — in **both call
orders**.

This is the property that moved here from the test this replaced.
There, `build` had to run **first**, and its comment said why: each
adapter memoised its resolution in a process-wide `OnceLock`, so a probe
reaching an unfilled cache wrote the fixture's answer into it and
changed what every sibling test in the binary resolved. The ordering was
load-bearing because the answer was *state*. It is now a function of its
argument, so the order cannot matter — and asserting both orders is what
says so.

The second field held constant is the adapter; what varies is the order,
and the two orders must agree exactly.

## `mod built_program_tests` › `fn two_boundaries_in_one_process_each_certify_their_own_cli() {`

Two boundaries in one process each certify **their own** CLI.

The hazard this exists for: with two runners in one process — which is
exactly what the container runner introduces — a resolution cached on
nothing hands one boundary's answer to the other, and a value that is
correct on first use and wrong on the second is invisible to any test
that constructs one runner. The repair is that there is no cache; this
is what says so, and it is what fails if one comes back.

Two independently varying fields, and their intersection is the test:
**which boundary** (two, reporting different versions) x **in which
order** (both). Three distinct versions are asserted as distinct-value
counts rather than described, so a fixture that lost a version reports
it instead of agreeing with itself.

## `mod built_program_tests` › `fn the_adapters_hold_no_process_wide_resolution_state() {`

No adapter holds process-wide resolution state, and this is the answer
to "what is the cache keyed on".

**Nothing is cached, so there is no key.** Each adapter used to memoise
its resolved binary in a `static RESOLVED: OnceLock<Option<Invocation>>`
— a cell keyed on nothing, which with two runners in one process hands
one boundary's answer to the other. A resolution that is correct on
first use and wrong on the second is the hardest shape in this slice
precisely because it is invisible to any test that constructs one
runner, and it stays invisible to a *behavioural* test even with two,
because a cache of a constant is indistinguishable from no cache.

So the claim is structural and the census is the only thing that can
make it: an adapter that starts remembering an answer fails here on the
line that declares the cell, before any behaviour depends on it.

Comments are stripped, and the strip is asserted to have removed
something — `PR4-CENSUS-COMMENT-ORACLE` is a census over a file format
that has comments, and this file's own prose names every pattern below.

## `fn the_adapters_hold_no_process_wide_resolution_state()` › `const HOLDERS: [&str; 4] = ["static ", "thread_local!", "OnceLock", "LazyLock"];`

Every way an adapter could hold a value across calls. Written out
rather than derived: a list read from the tree would shrink with it.

`static ` rather than a list of cell types, and that is deliberate.
Every adapter is a unit struct with no state of its own, so the only
way one remembers anything between calls is a module-level item —
and naming `OnceLock` alone would miss the `static X: Mutex<_>` that
a `const fn` constructor makes just as easy. `OnceLock` and
`LazyLock` are named as well so a `let`-bound or field-held cell is
caught too.

## `fn the_adapters_hold_no_process_wide_resolution_state()` › `assert!(`

The control: the pattern really does match when it is present, so an
empty result means absence rather than a broken search.

## `mod built_program_tests` › `fn the_host_runner_executes_a_bare_program_name_as_it_executes_the_resolved_path() {`

The host runner executes a bare program name as it executes the path
the adapters used to resolve.

The other direction of this repair, and the one every existing test and
the whole v0.1 product depends on: a change that resolves correctly for
a container boundary and quietly changes what the **host** runner runs is
a defect. The suite cannot show this on its own — it was written against
the old shape and largely still passes — so this spawns the same program
twice through the real [`crate::runner::host::HostRunner`], once named
and once at the absolute path `util::find_program` picks (which is the
resolution the adapters performed), and requires the two to agree.

`git` rather than an agent CLI because every machine that can build this
repository has it, so the observation is not a property of what happens
to be installed. The two program strings are asserted to actually differ
first; if they did not, this would compare a thing with itself.

**What this row cannot express, and where that lives instead.** `git` is
a native `.exe`, and on Windows `CreateProcessW` appends `.exe` to a bare
name, so the bare spelling reaches it whether or not this runner resolves
anything — which is exactly the installation shape `PR6D-001` did *not*
break. As a witness for the Windows case this row is therefore
correlated: it was green while the defect was live. It is kept as the
control it can honestly be — a real program on this machine's real
`PATH`, which the repair must not have changed — and the axis it cannot
vary, an npm-style `.cmd`-only installation reachable only through
`PATHEXT`, is
`runner::host::tests::an_npm_style_installation_runs_by_bare_name_exactly_as_it_runs_by_path`.
That test lives beside the resolution because writing a shim needs the
effect allowance `effects/allowlist.toml` gives `src/runner/host.rs` and
does not give this module.

## `mod probe_identity_tests` › `fn each_agent_probe_request_names_its_own_agent_in_every_field() {`

Each agent probe names **its own** agent, in every field that names one.

`invariants_introduced[1]` — "RunnerRequest carries a typed
InvocationId (… probes included; the probe role carries target
`Agent(name)` | `Shell`)". Every probe fixture in this suite probes one
agent, so a `probe_request` that filled the target with the first
configured adapter's name would agree with itself on every one of them.
Two independently named probes, and each request checked against the
name it was asked for rather than against the other request.

Both iteration orders, because "the first configured agent" is a
property of order: a fixture that only ever built them in one order
would pass for the agent that happened to be first.

## `fn each_agent_probe_request_names_its_own_agent_in_every_field` › `const NAMES: [&str; 3] = ["claude-code", "codex", "copilot"];`

Written here, not read from the adapter registry: the names are the
expected values.

## `fn each_agent_probe_request_names_its_own_agent_in_every_field` › `assert_eq!(roles.len(), 3, "{roles:?}");`

Hostility as counts: three names in, three distinct values out of
each field that carries one.

## `mod probe_identity_tests` › `fn use_sites(source: &str) -> Vec<String> {`

The file minus its `#[cfg(test)] mod tests { … }` block and minus the
`mod probe_ordinal { … }` declaration, so what is left is the
production code that *uses* an ordinal.

## `fn use_sites(source: &str) -> Vec<String>` › `if trimmed.contains("probe_ordinal::")`

Prose mentions an ordinal too, and a comment starts no
process.

## `mod probe_identity_tests` › `fn every_probe_call_site_passes_its_own_ordinal_constant() {`

Every ordinal an adapter's pre-flight passes to the Runner, **read
from the call sites** rather than from the table beside them.

`decisions.admission_and_leases.permits.invocation_identity`: an
invocation identity is "unique **per process**", and "every
RunnerRequest carries it". Each adapter's
`every_preflight_process_has_its_own_ordinal` builds its set from the
`probe_ordinal::ALL` array, so what it asserts is that a *table* has
distinct entries — which stays true when a call site passes another
entry's constant (codex's `debug models` step passing `VERSION`) or an
arithmetic expression over one (`HELP.saturating_sub(1)`). Two
processes then carry `p.agent-<name>.o0` and the ledger cannot tell
them apart, which is exactly what "unique per process" forbids.

This asserts the property one step later, at the point of use: each
declared constant is used **once**, every one is used, and the only
non-bare uses are the one block codex documents — a base plus an index,
for its six strict-config parser probes.

Codex had a second such block until PR6, one process per PATH
candidate, because it resolved its binary by spawning each candidate on
the coordinator host. `PR4-ADAPTER-RESOLVES-ON-THE-HOST` removed the
resolution, so `RESOLUTION_BASE` and its call site are gone and the
counts below drop by one.

## `struct Adapter` › `declared: &'static [&'static str],`

The constants the module declares, written out here from the
steps the adapter performs rather than read from the module.

## `struct Adapter` › `block_parts: &'static [&'static str],`

Constants that may appear in an expression, with the block they
open. Everything else must reach the Runner as a bare
`probe_ordinal::NAME` argument.

## `struct Adapter` › `call_sites: usize,`

How many processes this adapter starts through
`probe_request`, counting each variable-length block as one
call site.

## `fn every_probe_call_site_passes_its_own_ordinal_constant()` › `block_parts: &["CONFIG_BASE", "CONFIG_PER_SURFACE"],`

The one variable-length step: six strict-config parser
probes (two surfaces x three assignments). `probe_ordinal`
documents it as a block precisely because it cannot be one
constant.

## `fn every_probe_call_site_passes_its_own_ordinal_constant()` › `let collisions: Vec<(&String, &usize)> =`

No constant is used twice: that is the collision this exists to
catch, and it is what a table-only test cannot see.

## `fn every_probe_call_site_passes_its_own_ordinal_constant()` › `let declared: BTreeSet<&str> = adapter.declared.iter().copied().collect();`

And every declared constant is used: an ordinal declared and
never passed is a step whose identity came from somewhere else.

## `fn every_probe_call_site_passes_its_own_ordinal_constant()` › `let calls = adapter`

The number of processes, counted from the call sites rather
than from the table.

## `fn every_probe_call_site_passes_its_own_ordinal_constant()` › `assert_eq!(total_sites, 12, "the adapters' probe call sites moved");`

Hostility as a count: 3 + 2 + 7 across the three adapters, written
from what each pre-flight does. Codex was 8 until PR6 removed the
per-PATH-candidate resolution spawn
(`PR4-ADAPTER-RESOLVES-ON-THE-HOST`).
