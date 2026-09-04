# `src/runner/host.rs` — the host runner, `host-v1`

Extended notes for [`src/runner/host.rs`](../../../src/runner/host.rs). The code is the authority;
this file holds the expository material moved out of it. Section headings are the item names as
they are spelled in the source.

## Module overview

Everything DESIGN.md:118 gives a runner — "cwd, mounts, environment, supervision, and timeout" —
for the boundary that is this machine. It wraps the process funnel in `crate::agent::proc` rather
than reimplementing it, so "process supervision, timeout, output capture, adapter parsing
unchanged" is true by construction and not by parity alone.

Three things live here that are not in the funnel:

* **Environment composition** (DESIGN.md:258-264). The Upstroke environment is the base; the runner
  supplies the reserved keys; `CommandSpec.env` is an overlay applied last and refused pre-flight if
  it names a reserved key.
* **The `RunnerPreflight` shell probe** (INV-23). The recorded shell executing `exit 0` **through
  the Runner**, role `probe(shell)`, non-slotted, a registered invocation. Availability cannot be
  established by inspection; only a spawn establishes it.
* **The write-command startup step** (INV-18). On Windows the coordinator joins its ambient
  kill-on-close Job Object before any spawn, so every host child is a member at creation; a failure
  refuses the write command with a diagnostic.

### Allowlist placement

The **funnel section** of `effects/allowlist.toml`, which carries this module's review clause —
effects only inside site-taking APIs, no writable handle returned.
`decisions.effect_site_inventory.mechanism` (2).

## `HostRunner::resolved`

The field is the `PR6-LANED-001` repair: what this runner has already decided a program name is.

DESIGN.md:612: "Probes run through that same runner, **or pre-flight could certify a host
CLI/version different from the one the attempt executes**." Running the probe through the runner is
necessary and is not sufficient: a bare name re-searched at every spawn can find a *different file*
the second time, because `PATH` is a search order over a filesystem that moves. `PATH=A:B` with the
same name in both, `A/cli` removed after pre-flight, and the attempt silently runs `B/cli` — one
program string, two executables, and `Caps.version` certifying the one that did not run.

So the answer is remembered, and **where** it is remembered is the whole point.
`PR4-ADAPTER-RESOLVES-ON-THE-HOST` removed a process-wide `OnceLock` in each adapter, which handed
one boundary's answer to the next; putting that back would reintroduce it. This is per `HostRunner`
— per *boundary* — so two runners in one process still each get their own answer, and identity is
stable across the one thing that has to agree: a run's pre-flight and its attempts. Production
constructs exactly one of these per run (`engine::run_harness`) or per resume
(`engine::resume_harness`) and borrows it as `&dyn Runner` for pre-flight and every attempt, so
per-instance *is* per-run; `production_reaches_a_spawn_through_one_host_runner_per_run` is what
keeps that true.

Keyed on the **question**, not on the name: the program string together with the composed `PATH` and
`PATHEXT` that answer it. Not on the whole composed environment, and that is load-bearing rather
than an optimisation — `host-v1` supplies credential locations *role-scoped*
(`supplies_credentials`), so a probe's environment and its attempt's environment differ by design,
and a memo keyed on the environment would miss on exactly the pair DESIGN.md:612 requires to agree.
The three fields that decide the answer are the three the key carries.

### `HostRunner::hooks`

Held for the whole of one `run`, so one `HostRunner` supervises one process at a time. That is not a
limitation today — `Runner::run` is synchronous until PR11 and the substrate is sequential — but
PR11's concurrent scheduler will need an observer per invocation rather than per runner, and this is
where that shows up.

## `HostRunner::with_environment`

It does **not** clear `HostRunner::resolved`, and that is a decision rather than an omission. The
memo is keyed on the question — the program name with the composed `PATH` and `PATHEXT` — so a new
environment that resolves a name differently asks a different question and misses, and one that
resolves it identically is entitled to the same answer. A clear here would therefore be a line no
fixture could ever see fail, which is the shape this project treats as debt: the key is the
mechanism, and `a_resolution_question_is_the_program_and_the_environment_that_answers_it` is what
holds it.

## `HostRunner::program_for`

The `PR6-LANED-001` repair. `resolve_program` answers the question by searching a filesystem; this
decides *whether the question is asked*, and it is asked at most once per `ProgramQuestion` per
runner. See [`HostRunner::resolved`](#hostrunnerresolved) for why per-runner rather than per-spawn (a
filesystem that moves between pre-flight and the attempt would otherwise hand the attempt a
different executable under the same name) and why per-runner rather than process-wide (that is
`PR4-ADAPTER-RESOLVES-ON-THE-HOST`).

**A refusal is remembered too.** Fail-closed: a run whose pre-flight could not find `claude` on the
`PATH` it composes does not silently find it at the third attempt because something installed one
meanwhile. The stored value is the refusal's message, and `UpstrokeError::Refused` displays as
exactly its message, so the replayed error is the first one byte for byte —
`a_refused_name_is_refused_identically_without_asking_the_filesystem_again` is what holds that
rather than this sentence.

`program_resolutions` counts calls here — one per spawn, whether or not the filesystem was touched.
`program_searches` counts the ones that reached `resolve_program`. The two are separate because the
ordering predicate ("resolved once per spawn, before any of the spawn") and the identity predicate
("searched once per boundary") are different claims and a single counter could not hold both.

## `impl Runner for HostRunner` — `run`

### Where the program name is resolved

Which file the program *name* is, decided in `run` and nowhere else.

`CommandSpec::program` is a name, not a location, and DESIGN.md:118 gives this runner the
environment — so the boundary that executes a name is the boundary that says which file it is.
`PR4-ADAPTER-RESOLVES-ON-THE-HOST` states the shape in two clauses: "`CommandSpec.program` carries
the bare CLI name **and the runner resolves it against the environment it composes**". This is the
second clause.

**After `compose` and before anything is spawned**, both load-bearing. After, because the
environment this resolves against has to be the one the child will run under — a `PATH` the overlay
could not have named (it is reserved) but which a caller's `HostEnvironment` decides. Before,
because a name that reaches this boundary and resolves to nothing is a pre-flight refusal naming the
name, not a `NotFound` from a spawn.

**And once per boundary, not once per spawn** (`PR6-LANED-001`). `HostRunner::program_for` searches
the first time and remembers the answer for this runner, so a run's pre-flight and its attempts
execute the same file even if the filesystem moves under them — DESIGN.md:612, which running the
probe through the runner is necessary but not sufficient for.

### Why the composed environment is the whole environment

The composed environment *is* the environment: base, reserved values, overlay, and nothing arriving
by a route the record does not describe. "Probe and execution compose the same base, mounts,
reserved values, and overlay, so pre-flight certifies the environment that will actually spend"
(DESIGN.md:263).

## `build_command_at`

On Windows the tail after `cmd.exe`'s `/C` or `/K` is handed over raw. `gates::ShellKind::command`
explains why, and this is the other half of that rule: "std's Windows quoting escapes embedded
quotes as `\"` per `CommandLineToArgvW` rules, which cmd.exe does not un-escape; the /C tail must go
through raw_arg to survive intact." A runner that re-quoted the tail would hand every gate command
containing a quote to a different program than the one the operator wrote — silently, and only on
Windows. `invariants_preserved` says "adapter parsing unchanged", and a gate whose command line
changes meaning when it is routed through the Runner is not unchanged.

### Why the resolved program is a separate parameter

The split between `build_command_at` and the spec exists because the resolved program is a `Path`
and `CommandSpec::program` is a `String` (DESIGN.md:222): a `PATH` directory whose name is not valid
Unicode is legal on Unix, and writing the resolved path back into the spec would have to either
refuse it or rewrite it into a path that names nothing (`PR4-PROGRAM-PATH-NOT-UNICODE`). It never
becomes a `String`, so neither happens.

`cmd.exe`'s raw-tail rule is keyed on the program **that will execute** rather than on the spec's,
so it survives resolution: `cmd`, `cmd.exe` and `C:\Windows\System32\cmd.exe` all have the file stem
`cmd`, and a gate whose command line changes meaning depending on whether the runner resolved its
shell is not "adapter parsing unchanged".

## `program_resolutions` and `program_searches`

`program_resolutions` is the same kind of observable as `containment_establishments`, and here for
the ordering rather than the count alone: resolution is specified to happen *once per spawn, before
any spawn*, and a suite that proves only that the right file ran holds neither half. A `SpawnHooks`
observer reading this at the first sub-effect point sees the resolution already done, and sees it
done once — see `tests::a_program_is_resolved_once_per_spawn_and_before_any_of_it`.

It is incremented by `HostRunner::program_for` on entry, so a spawn that took its answer from the
runner's memo and a spawn that searched for it are counted alike: this is "was the program decided
for this spawn, and when", not "did the filesystem move".

`program_searches` is its sibling and the observable of the `PR6-LANED-001` repair: `HostRunner`
resolves a name **once per boundary**, not once per spawn, so that a run's pre-flight and its
attempts execute the same file even when the filesystem moves between them (DESIGN.md:612). N spawns
of one name through one runner move `program_resolutions` by N and this by one.

It has to be a second counter rather than a reinterpretation of the first, because the two predicates
are independently droppable: a memo that never hits satisfies "once per spawn" and reopens :612, and
a resolution moved after the first containment point satisfies "once per boundary" and reopens the
ordering.

It is incremented by `resolve_program` on entry, so the count moves for a program that names a
location as well as for one that is searched for — the question asked is the same, and what differs
is the answer.

## `mod proof` — the containment proof and its sole mint

`Contained`'s field is private to **the `proof` module** rather than to `runner::host`, which is the
whole point: Rust privacy reaches a module and everything below it, so a field private to
`runner::host` is constructible from `runner::host::naming`, `::environment` and `::probe`. `proof`
has no children, so its siblings cannot reach the field and the only route to a value is
`contain_write_command`, which performs the join. The mint stays with the type as a local
implementation invariant of that module: a proof and the only code that may create it are read
together or not at all.

### `Contained`

The type exists so that "the ambient job is established before anything this run could spawn" is a
thing the compiler checks rather than a thing each new entry point is trusted to remember. **The
field is private to the `proof` module, and that module has no descendants** — so no sibling of
`runner::host`'s children, and not `runner::host` itself, can name it. The only values of it in the
crate are the ones `contain_write_command` returns after `proc::join_ambient_job` has succeeded, and
that is enforced by the compiler rather than by a census.

**The module boundary is the mechanism, and it is load-bearing.** Rust privacy is scoped to a module
*and everything below it*, so a private field in `runner::host` is reachable from every child of
`runner::host` — which the per-concern split made four modules rather than one. **Three spellings
construct one**, and only the third contains the `Contained(` needle a lexical census looks for:
`Contained { 0: () }`, `let c = Contained; c(())`, and the plain `Contained(())` — none of them
calling `Contained::new`, so none of them incrementing `containment_establishments`. Confining the
type to a module with nothing beneath it is what makes "a caller cannot forge one" true again.

**A runtime test cannot observe that**, because the property is that the offending code does not
compile: those three forms and `proof::Contained::new()` — four in all — were each planted in
`runner::host::naming` and each rejected, as `E0451`, `E0423`, `E0423` and `E0624`. The fourth is the
control that decides the shape: it is what a `pub(super) fn new` would have let through, minting a
token with no join performed while the counter agreed with it. The evidence is the repair round's,
not a `#[test]`'s.

`src/main.rs` has the same shape for the CLI's own dispatch (its `containment::Contained` proves
*classification*: a write command joined, a read-only one was not asked to). This is the library half
of that idea, for the callers that never go through `main.rs` at all — the frozen public
`engine::run/run_with/run_harness` and `resume/resume_with/resume_harness` facades, which a
downstream crate may call directly.

## `contain_write_command`

What `src/main.rs` calls at the top of every write command, before any dispatch arm runs, and what
the engine's write coordinator calls before it touches anything. `crash_reconstruction`: "at process
start every write command creates one non-inheritable ambient Job Object with
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE and assigns the coordinator process itself to it … if the ambient
job cannot be created or joined the write command refuses at startup with a diagnostic before any
workspace effect".

A free function because the ambient job is a property of the *process*, not of a runner value: it is
established once at startup and held to process exit, and it must be established before anything that
could spawn exists. Idempotent for the same reason — `windows_job::join_ambient` memoises the
process's one answer — so a coordinator entered through the CLI, which has already joined,
re-establishes at no cost and gets its proof. `HostRunner::start_write_command` is the same step with
a runner's own observer attached, for the ST-07 evidence, and it calls **this** function — so there
is one join site and one mint in the crate, not two.

### Why the observer is a parameter

It is threaded one level further out than the funnel's, and for the reason that put a `join` closure
inside `proc::join_ambient_job_with` and a `contain` parameter on `engine::run_contained`: **no
machine here can make the real join fail on demand** — `windows_job::join_ambient` memoises the
process's one answer, so a binary that has ever joined can never observe a failure — and a step that
took no observer therefore had no failure path any test could drive.

That matters here more than anywhere else it is threaded. This is the function the frozen public
facades (`engine::run_harness`, `engine::resume_harness`) and `src/main.rs`'s dispatch all reach, and
it is the **only** place in the crate that mints a `Contained`; the `map` losing its short-circuit is
the whole of INV-18's host portion silently ceasing to hold, on the platform where the failure is
real. Production passes `crate::agent::proc::NoHooks` at every call site, exactly as it does to the
process funnel; the suite arms an observer that refuses at `Spawn.AmbientJobJoined` and watches the
refusal come back out with no proof minted (see
`tests::the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing`).

## `containment_establishments`

The same kind of observable as `proc::ambient_job_established`, which exists so INV-18's host portion
can be asserted rather than described, and narrower in the two ways that matter for a census of
*entry points*:

* it answers on every platform, where `ambient_job_established` can only answer on Windows — on Unix
  the step is a no-op, but *whether the step ran* is the same question on both platforms and is the
  one an entry point is accountable for;
* it counts calls on the calling thread instead of latching once per process, so it can say that
  *this* call established containment. A process-wide latch cannot: the second write coordinator in a
  process would find it already true whether or not it established anything.

It is incremented by `Contained::new`, so the count and the tokens cannot disagree.

## `start_write_command` (free function)

`contain_write_command` for a caller with nothing to prove it to. `src/main.rs` is that caller: it
mints its own dispatch-level token from this step's success, and the ordering it needs is between
*its* two statements. The engine facade takes the proof instead, because its ordering obligation is
against a function it calls.

It carries the observer through for the same reason `contain_write_command` takes one: this is the
entry point the CLI's whole write side depends on, and a body that dropped the refusal —
`let _ = contain_write_command(hooks); Ok(())` — would leave every `upstroke run` on Windows
dispatching with no ambient job and the suite green.

## `shell_probe_request`

A free function because both runners implement the same probe — INV-23: "the RunnerPreflight — one
non-slotted shell probe (the recorded shell executing `exit 0`) and one slotted probe per recorded
agent, each a registered invocation through the run's Runner". PR6's container runner executes this
identical request inside the recorded image.

The argument vector is taken from `ShellKind::command` rather than rebuilt, so the probe runs under
exactly the invocation a gate would — including `cmd.exe`'s `/C` and PowerShell's `-NoProfile
-NonInteractive`. A probe that spelled the shell differently from the gates would certify something
else.

## `RESERVED_ALWAYS`

DESIGN.md:260-262: "each supplies role-scoped `HOME`, `PATH`, and credential locations. Adapter
overrides may select profiles or CLI behavior but may not conflict with runner-reserved keys."

## `CREDENTIAL_LOCATIONS`

`src/capacity.rs:36-37` names two of the three as the vendors' own profile mechanism: "`COPILOT_HOME`
(documented) and `CLAUDE_CONFIG_DIR` (works, undocumented as of Aug 2026)"; `CODEX_HOME` is
codex-cli's equivalent.
