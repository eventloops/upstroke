# `src/runner/container/env.rs`

Extended notes for [`src/runner/container/env.rs`](../../../../src/runner/container/env.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

`container-v1`'s environment contract, and the boundary layout the
environment names.

DESIGN.md:258-264, quoted in full because every clause is separately
droppable:

> `CommandSpec.env` **overlays a runner-owned base rather than replacing
> it**. The host runner starts from the Upstroke environment and **the
> container runner from the image environment**; each supplies role-scoped
> `HOME`, `PATH`, and credential locations. Adapter overrides may select
> profiles or CLI behavior but **may not conflict with runner-reserved
> keys**. **Probe and execution compose the same base, mounts, reserved
> values, and overlay**, so pre-flight certifies the environment that will
> actually spend.

#### Why this is a second implementation and not a call into `host-v1`

`decisions.tests_acceptance.parity` is "host and container runners produce
identical adapter parsing and **environment composition**". A container
runner that delegated composition to [`crate::runner::host::HostEnvironment`]
would make that clause true by construction and therefore unmeasurable — the
parity test would compare a function with itself, which is the project's own
"a function may not be its own oracle" applied to a whole seam. So the
contract is implemented twice and
`exec::tests::host_and_container_compose_the_same_environment_for_every_role`
is the cross-check, over an **explicit shared base** so the two really do
differ in nothing but the runner.

What is *not* re-derived is the **reserved-key enumeration**:
[`crate::runner::host::reserved_keys`] is a `pub fn` and is the one list, so
"which keys are reserved" is a thing another module reads rather than a
literal buried in a match arm here. Two runners disagreeing about the
reserved set would be a difference no parity test could interpret.

#### What role-scoping means at a boundary with its own filesystem

The host supplies a credential *location* from its own base and relies on
the agent's permission surface for everything else. A container supplies the
location **and** either mounts that agent's credential volume there or does
not — so for a role that takes no credentials the directory the variable
names is simply not there. That is the mechanically-perfect half
DESIGN.md:610 claims a container buys, and it is why
[`supplies_credential_location`] and the mount plan in
[`super::exec::ContainerRunner`] are asserted to be **the same predicate**
rather than two rules that happen to agree.

## `#![deny(`

`PR6-LANEF-004`: the Container funnel's module-level allow is an INNER
attribute, and a Rust lint level is scoped by the MODULE TREE rather than by
the file, so every out-of-line child of `runner::container` inherited it --
measured, a `ContainerRuntime::start` planted in a child module passed
`cargo clippy --all-targets --all-features -- -D warnings`. Re-denying here
is what makes `decisions.effect_site_inventory.mechanism` (1)'s BUILD error
true of a lane's module, which is the leg the source census cannot supply.
Enforced for every file in this directory by `runner::container::tests::
every_child_module_of_the_container_funnel_states_its_own_lint_level`.

## `pub struct BoundaryLayout {`

---------------------------------------------------------------------------
The boundary's own layout
---------------------------------------------------------------------------

## `pub struct BoundaryLayout {`

Where a container sees the things it is given.

A value rather than a set of constants, because both the environment
(`HOME`, credential locations) and the mount plan
([`super::exec::ContainerRunner`]) have to name the same paths, and two
literals that must agree are two literals that drift. The mount target and
the variable that points at it come from one place.

## `impl BoundaryLayout` › `pub const DEFAULT_WORKSPACE: &'static str = "/upstroke/workspace";`

Where the role's one worktree is mounted.

DESIGN.md:400: "A container receives **only its role's one worktree
mount**". One path, so a second worktree would need a second target and
there is nowhere to put it.

## `impl BoundaryLayout` › `pub const DEFAULT_CREDENTIALS: &'static str = "/upstroke/credentials";`

Under which each agent's credential volume is mounted.

## `impl BoundaryLayout` › `pub const DEFAULT_GIT_VIEW: &'static str = "/upstroke/gitview";`

Where the disposable Git view is mounted.

A root of its own rather than `<workspace>/.git`, and that is forced by
the runtime rather than chosen. Measured against `docker` 29.7.2: a bind
mount whose source is a **directory** and whose target is an existing
**file** fails the container's `runc create` outright —
`not a directory: Are you trying to mount a directory onto a file` — and
a linked worktree's `.git` is exactly such a file. So the view is
mounted here and `<workspace>/.git` receives a one-line **file** mount
pointing at it, which is the shape a linked worktree already has and is
therefore an overlay rather than a redirection: a tool that opens
`<workspace>/.git` finds the disposable view, not the real repository,
with no environment variable involved.

## `impl BoundaryLayout` › `pub const DEFAULT_GIT_OBJECTS: &'static str = "/upstroke/gitobjects";`

Where the repository's object store is mounted, **read-only**.

Beside the view rather than inside it, so the view's own `objects/`
stays writable: every object a gate creates lands in the disposable half
and the borrowed store is one the kernel will not let the container
write. Mounting the borrowed store *over* `<view>/objects` would make
`git add`, `git stash` and `git write-tree` fail hard inside every
container.

## `impl BoundaryLayout` › `pub const DEFAULT_SCRATCH: &'static str = "/tmp";`

The ephemeral scratch surface, and the working directory of a role that
has no worktree.

`/tmp` because that is the directory a POSIX shell, `git` and every CLI
this engine drives already write temporaries to, and because
`CreateSpec::read_only_root` closes the container's own layer: without a
declared writable scratch mount a gate could not run `sh` at all. It is
a `Mount::Tmpfs` and therefore has **no host source**, so "gate write
outside mount fails" stays a statement about a mount list every entry of
which is either the role's own or unreachable from the coordinator.

## `impl BoundaryLayout` › `pub fn new() -> Self {`

The default layout.

## `impl BoundaryLayout` › `pub fn with_roots(`

A layout with explicit roots, so a grid can vary them.

## `impl BoundaryLayout` › `pub fn workspace(&self) -> &str {`

The role's worktree.

## `impl BoundaryLayout` › `pub fn git_view(&self) -> &str {`

The disposable Git view.

## `impl BoundaryLayout` › `pub fn git_objects(&self) -> &str {`

The borrowed, read-only object store.

## `impl BoundaryLayout` › `pub fn scratch(&self) -> &str {`

The ephemeral scratch surface, and the working directory of a role that
has no worktree.

## `impl BoundaryLayout` › `pub fn git_pointer(&self) -> String {`

`<workspace>/.git` — where a Git-dependent tool looks, and what the
view overlays.

DESIGN.md:612: "Because a linked worktree's `.git` points back into the
real repository, the container **overlays** a disposable role-scoped Git
view". *Overlays* — at the place the tools look.

## `impl BoundaryLayout` › `pub fn credentials(&self, agent: &AgentId) -> String {`

Where this agent's credential volume is mounted.

## `impl BoundaryLayout` › `pub fn credential_root(&self) -> &str {`

The credential root itself.

## `pub const fn supplies_credential_location(role: &ExecutionRole) -> bool {`

---------------------------------------------------------------------------
Role scoping
---------------------------------------------------------------------------

## `pub const fn supplies_credential_location(role: &ExecutionRole) -> bool {`

Whether `container-v1` tells this role where an agent's credentials live —
and, equivalently, whether it mounts that agent's credential volume.

Transcribed from the same two sentences `host-v1` reads, not from
`host-v1`:

* INV-18: "every agent CLI invocation **incl. agent probes** acquires its
  atomic {agent, pool?} pair while gates **and the shell probe** register
  without slots" — the split between the processes that execute an agent CLI
  and the ones that do not.
* DESIGN.md:260: "each supplies **role-scoped** … credential locations".

A gate is repository-controlled code — the thing DESIGN.md:610 says a
container exists to confine — and the shell probe is a shell running
`exit 0`. Neither runs an agent CLI, so neither is handed an agent's
credentials, whatever agent the request happens to name.

Exhaustive with no wildcard: a role added later has to be classified here
rather than defaulting into the side that hands out credentials.

## `pub const CONTAINER_KEY_CASE: KeyCase = KeyCase::Sensitive;`

---------------------------------------------------------------------------
The environment
---------------------------------------------------------------------------

## `pub const CONTAINER_KEY_CASE: KeyCase = KeyCase::Sensitive;`

The name rule the **container** boundary obeys.

`KeyCase::Sensitive` unconditionally, and that is a decision rather than an
oversight: `host-v1` takes [`KeyCase::current`], the *coordinator's* rule,
because its boundary is the coordinator's own process environment. A
container's boundary is the image, and DESIGN.md:610's Windows paragraph
puts the repository "WSL-side" — the boundary is Linux even when the
coordinator is not. A container runner that took `KeyCase::current()` would,
on a Windows coordinator, treat `Path` and `PATH` as one variable inside a
boundary where they are two, and would refuse an overlay key the boundary
does not reserve.

## `pub const CONTAINER_PATH_SEPARATOR: char = ':';`

The separator between `PATH` components at the container boundary.

`:` unconditionally, for the reason [`CONTAINER_KEY_CASE`] is
`KeyCase::Sensitive`: the boundary is the image, DESIGN.md:610 puts the
repository "WSL-side", and the image is Linux even when the coordinator is
not. A `cfg!(windows)` here would split the container's `PATH` on `;` on a
Windows coordinator and find one enormous component that happens to be
absolute.

## `pub fn cwd_dependent_path_components(value: &str) -> Vec<String> {`

The `PATH` components that make a bare program name resolve **relative to
the working directory**.

Two shapes, and the second is the one that is missed: a component that is
literally `.`, and a component that is **empty** — `PATH=/usr/bin:` and
`PATH=:/usr/bin` and `PATH=/a::/b` all name the current directory, which is
POSIX and is what a trailing separator produces by accident. Anything that
does not begin with `/` is relative, so the test is one rule and the two
shapes are consequences of it.

#### Why this is a refusal and not a filter

DESIGN.md:612: "Probes run through that same runner, or pre-flight could
certify a host CLI/version different from the one the attempt executes."
A probe has no worktree and an attempt has one, so their working directories
necessarily differ — that difference is *designed*, and it cannot be
removed. What must not differ is which binary a name resolves to. With a
relative component in `PATH` the repository's own worktree is on the
executable search path, so repository-controlled content decides which
`claude` an Implement invocation runs while pre-flight certified another
one (`PR6-CORRECTNESS-006`).

Dropping the offending components would also make resolution
cwd-independent, and would do it by silently changing what the operator's
image asked for. Refusing says so. `pr_sequence[7]`'s own idiom is a refusal
before any effect, and `plan` performs none.

## `pub struct RoleScope<'a> {`

What one request's environment is composed for.

A struct rather than four parameters because the four travel together and a
call site that got one of them wrong — a gate carrying a bound agent, say —
is the shape `runner::worker_request` and its siblings exist to prevent.

## `pub struct RoleScope<'a>` › `pub role: &'a ExecutionRole,`

Which seat this process occupies.

## `pub struct RoleScope<'a>` › `pub agent: Option<&'a AgentId>,`

The agent whose credential volume this process uses, if any.

## `pub struct RoleScope<'a>` › `pub volumes: &'a BTreeMap<String, String>,`

The run's recorded per-agent credential volume names
(`RunnerPolicy.credential_volumes`). A volume this map does not name
cannot be mounted, so its location is not supplied either.

## `pub struct RoleScope<'a>` › `pub layout: &'a BoundaryLayout,`

Where the boundary puts things.

## `pub struct ContainerEnvironment {`

`container-v1`'s environment contract.

Holds its base explicitly, exactly as [`crate::runner::host::HostEnvironment`]
does and for the same reason: a test composes against a base it wrote rather
than against whatever the machine happens to carry.

## `impl ContainerEnvironment` › `pub fn from_image(base: Vec<(String, String)>) -> Self {`

The image environment, as the base.

"the container runner [starts] from the image environment"
(DESIGN.md:259).

## `impl ContainerEnvironment` › `pub fn inherited() -> Self {`

An empty base: the runtime applies the image environment itself.

`docker create --env K=V` **overlays** the image's own environment
rather than replacing it, so a runner that names no key still executes
against the image environment — which is precisely the base
DESIGN.md:259 gives this runner. This constructor is the honest spelling
of "the base is the image's, and this runner did not read it": the
composed vector then names only the keys the runner owns.

**The residual is stated rather than hidden.** A container runtime
cannot *unset* a variable the image sets, so an image whose environment
carries an agent's credential-location variable hands it to every role,
including the ones [`supplies_credential_location`] refuses. What that
cannot do is hand over the credentials themselves: the volume is either
mounted at that path or it is not, and for a gate it is not. The mount
is the boundary; the variable is a pointer.

**[`Self::compose`] now refuses every environment built from this base**
(`PR6-CORRECTNESS-006`), and the constructor survives only so that
refusal has a subject. An empty base supplies no `PATH`, so the image's
own decides which binary a bare program name resolves to; if that `PATH`
carries a relative component — `.`, or the empty component a trailing
`:` produces — a probe and the attempt it certifies resolve different
binaries, because a probe has no worktree and an attempt has one. The
honest reading of DESIGN.md:259-260 is that the runner *reads* the image
environment and supplies `PATH` from it; a caller that has performed
that read-only inspection passes it here through [`Self::from_image`].

## `impl ContainerEnvironment` › `pub fn with_base(base: Vec<(String, String)>, case: KeyCase) -> Self {`

An explicit base and an explicit name rule, for grids that must cover
both rules.

## `impl ContainerEnvironment` › `pub fn base(&self) -> &[(String, String)] {`

The base this runner composes from.

## `impl ContainerEnvironment` › `pub const fn case(&self) -> KeyCase {`

The name rule in force.

## `impl ContainerEnvironment` › `pub fn reserved_values(&self, scope: &RoleScope<'_>) -> Vec<(String, String)> {`

The reserved values the runner supplies for this request.

The same two rules `host-v1` applies, resolved for a boundary that has
its own filesystem:

* `PATH`, `HOME` and `USERPROFILE` are supplied to **every** role at the
  boundary's own value — from the base, which for this runner is the
  image environment. Not per-role: DESIGN.md:263's "probe and execution
  compose the same base … so pre-flight certifies the environment that
  will actually spend" forbids a `HOME` that differs between
  `probe(<agent>)` and `implement`, and a `PATH` that differed between
  `probe(shell)` and `gate` would certify a different program from the
  one that runs. A reserved key the base does not carry is **not**
  supplied — setting an absent variable to the empty string is a
  different environment from not setting it.
* The **credential location is role-scoped**, and its value is the
  boundary's own: the mount target of that agent's credential volume,
  never a coordinator-host path. A host path here would name nothing
  inside the image, which is the container half of
  `PR4-ADAPTER-RESOLVES-ON-THE-HOST` applied to the environment.

## `pub fn reserved_values(&self, scope: &RoleScope<'_>) -> Vec…` › `if scope.volumes.contains_key(agent.as_str()) {`

A location is supplied only when a volume is recorded for
that agent: the run's `RunnerPolicy.credential_volumes` is
what says which volumes exist, and pointing a CLI at a
directory nothing mounts is worse than saying nothing.

## `impl ContainerEnvironment` › `pub fn withheld_credential_locations(&self, scope: &RoleScope<'_>) -> Vec<(String, String…`

The credential-location keys this scope is **not** given, each with the
value that says so at the boundary.

#### Why an empty value and not an absent key (`PR6-CORRECTNESS-007`)

`docker create --env K=V` **overlays** the image's environment; it does
not replace it. So a key the composed vector simply *omits* is not a key
the container lacks — it is a key whose value the **image** chooses.
Measured on `docker` 29.7.2 against an image declaring
`ENV CODEX_HOME=/image/codex`: a container created with no `CODEX_HOME`
in its spec runs with `CODEX_HOME=/image/codex`. Every role therefore
received every credential location the image happened to carry, and
DESIGN.md:258-262's "each supplies **role-scoped** … credential
locations" was false of a gate, a reviewer and the shell probe — the
three roles [`supplies_credential_location`] refuses by name.

The runtime has no "unset" for an image variable, and the two remaining
spellings are not equivalent: bare `--env K` **passes the client's own
value through** when the coordinator has one, which turns an image leak
into a *host* leak. `K=` is the spelling that names the key and gives it
nothing.

**Unconditional, not conditional on the base carrying the key.** The
base is a read of the recorded image, and a rule that only neutralises
what that read happened to return is a rule whose correctness depends on
the read being complete. Stating "this role has no location for this
agent" for all three keys costs three environment entries and depends on
nothing.

This is a pointer and never a token — the credentials are the *volume*,
and for these roles it is not mounted (`ContainerRunner::mounts`). The
mount is the boundary; this closes the gap between the boundary and what
the process is told about it.

## `impl ContainerEnvironment` › `pub fn compose(`

Base, then reserved values, then overlay — DESIGN.md:263's own order
("the same base, mounts, reserved values, and overlay").

The base's own copies of the **reserved** keys are dropped before the
runner supplies them, for the reason `host-v1` states: cloning the base
and upserting would leave every credential location the image happens to
carry in a gate's environment, and would make this step
output-equivalent to deleting it, because [`Self::reserved_values`]
reads its values back out of the same base.

### Errors

[`UpstrokeError::Refused`] naming the key when the overlay names a
reserved one — refused by **key**, not by value, exactly as `host-v1`
refuses it: an overlay permitted to restate `PATH` today because the
value happens to match is an overlay that breaks silently the day the
runner's value changes.

[`UpstrokeError::Refused`] also when the composed environment does not
supply an absolute-only `PATH` — see [`Self::certify_path`].

## `impl ContainerEnvironment` › `for (key, value) in self.withheld_credential_locations(scope) {`

The keys this role is *not* given, named explicitly rather than
omitted — see `withheld_credential_locations`. After the supplied
ones and before the overlay, so the two sets cannot collide (a key in
one is by construction not in the other) and the overlay's own
reserved-key refusal still governs.

## `impl ContainerEnvironment` › `pub fn certify_path(&self, composed: &[(String, String)]) -> Result<(), UpstrokeError> {`

Refuse an environment under which a bare program name would resolve
against the working directory.

Two refusals, and each is separately droppable:

* the composed environment names **no** `PATH`, so the image's own
  decides resolution and this runner cannot say what it is. That is what
  [`Self::inherited`] produces, and it was the production default — a
  runner that supplied none of the three values DESIGN.md:260 says it
  supplies;
* the composed `PATH` carries a working-directory-relative component, so
  the same name resolves to different binaries in a probe (which has no
  worktree) and in the attempt it certifies (which has one).

Composed rather than base, because the reserved-key step is what puts
`PATH` there and a check on the base would pass an implementation that
dropped it.

### Errors

[`UpstrokeError::Refused`], naming the offending components.

## `impl ContainerEnvironment` › `pub fn preflight(&self, overlay: &[(String, String)]) -> Result<(), UpstrokeError> {`

The reserved-key refusal on its own, so a caller can certify an overlay
without building an environment.

### Errors

[`UpstrokeError::Refused`] naming the offending key and the reserved key
it collides with.

## `mod tests {`

-- test-only declarations ----------------------------------------------
At the BOTTOM: `effects::production_region` cuts a source at its first
`#[cfg(test)]`, so a test module above would remove everything below it from
every source census (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).

## `mod tests` › `const VOLUMES: &[(&str, &str)] = &[`

The three shipped adapters, and a volume name per adapter. Every value
distinct, so a swap between two of them is visible.

## `mod tests` › `fn image_base() -> Vec<(String, String)> {`

An image environment with a value for every key any test reads, each
distinct.

## `mod tests` › `fn binding(role: &ExecutionRole) -> Option<AgentId> {`

The agent a role binds in production: `runner::worker_request` and its
siblings decide this, and a grid that let the binding ride along with
the role would be varying two fields and calling it one.

## `mod tests` › `fn the_reserved_key_enumeration_is_the_hosts_and_not_a_second_list() {`

The reserved set is **one** enumeration, and it is `host-v1`'s.

Second field held constant: the role is `Implement` throughout, so what
varies is only which key is offered. The expected list is written from
DESIGN.md:260 ("role-scoped `HOME`, `PATH`, and credential locations")
and `src/capacity.rs:36-37`'s naming of the three vendor variables — not
read back from `reserved_keys()`, which is the function this pins.

## `fn the_reserved_key_enumeration_is_the_hosts_and_not_a_seco…` › `let environment = ContainerEnvironment::from_image(image_base());`

And the container runner refuses every one of them, by key, so the
two boundaries cannot disagree about what "reserved" means. A second
list here would be a difference no parity test could interpret.

## `fn the_reserved_key_enumeration_is_the_hosts_and_not_a_seco…` › `environment`

The control: a key neither runner reserves composes.

## `mod tests` › `fn an_overlay_naming_a_reserved_key_is_refused_by_key_across_every_role() {`

Every role refuses every reserved key, and refuses it **by key** rather
than by value.

Second field held constant: the base and the volume set, so what varies
is the (role, key) pair. Thirty refusals plus five controls, counted.

## `fn an_overlay_naming_a_reserved_key_is_refused_by_key_acros…` › `let value = if key == "PATH" {`

The value is exactly what the runner itself would supply for
`PATH`, so a refusal that compared values would let this one
through.

## `mod tests` › `fn the_overlay_overlays_the_base_rather_than_replacing_it() {`

"overlays a runner-owned base rather than replacing it" — three
fixtures, because the sentence has three separately droppable halves.

## `fn the_overlay_overlays_the_base_rather_than_replacing_it()` › `("UPSTROKE_NEW".to_owned(), "landed".to_owned()),`

(b) an overlay key the base does not carry lands

## `fn the_overlay_overlays_the_base_rather_than_replacing_it()` › `("LANG".to_owned(), "en_GB.UTF-8".to_owned()),`

(c) a collision between base and overlay resolves to the
overlay

## `fn the_overlay_overlays_the_base_rather_than_replacing_it()` › `assert_eq!(`

(a) a base key with no overlay survives

## `fn the_overlay_overlays_the_base_rather_than_replacing_it()` › `let mut keys: Vec<&str> = composed.iter().map(|(key, _)| key.as_str()).collect();`

One entry per key: an overlay that appended rather than upserted
would leave the child with whichever the runtime read last.

## `fn the_overlay_overlays_the_base_rather_than_replacing_it()` › `assert_eq!(`

And the base really did carry a different value for the collided key,
so (c) is a statement about resolution rather than about equality.

## `mod tests` › `fn the_credential_location_is_role_scoped_and_names_the_boundarys_own_path() {`

The credential location is role-scoped, and its value is the
**boundary's** path.

The grid is {5 roles} × {volume recorded, volume absent}, and the second
field is the one that matters: a rule keyed only on the role would
supply a location for a volume the record does not name, and a rule
keyed only on the record would hand a gate an agent's credentials.

**"Withheld" is `KEY=` and not an absent key** (`PR6-CORRECTNESS-007`).
A composed vector that merely omits the key leaves the value to the
image, because `docker create --env` overlays rather than replaces — so
every cell here asserts on the *pair* (named, value) and not on presence
alone. All three credential-location keys are named in every one of the
ten cells; only the value moves.

## `fn the_credential_location_is_role_scoped_and_names_the_bou…` › `for (_, key) in CREDENTIAL_LOCATIONS {`

Every credential-location key is named, whatever the role: an
unnamed key is a key the image decides.

## `fn the_credential_location_is_role_scoped_and_names_the_bou…` › `assert_eq!(supplied, 3, "three of the ten cells supply a location");`

Implement, Review and probe(claude-code) with a recorded volume.

## `fn the_credential_location_is_role_scoped_and_names_the_bou…` › `let claude = AgentId::new("claude-code");`

The cell the production request builders cannot reach: a role that
takes no credentials, carrying an agent anyway. `host-v1`'s own
`reserved_values` names this shape — "neither is told where an
agent's credentials live, **whatever agent the request happens to
name**" — and a grid built from `binding(role)` alone never asks it,
because that function gives a gate and a shell probe `None`.

## `fn the_credential_location_is_role_scoped_and_names_the_bou…` › `let per_agent: std::collections::BTreeSet<String> = VOLUMES`

Distinct-value count over the agents, so a layout that returned one
path for every agent is visible.

## `mod tests` › `fn the_container_boundary_is_case_sensitive_whatever_the_coordinator_is() {`

The container boundary is case-sensitive whatever the coordinator is.

Second field held constant: the role and the base; what varies is the
name rule, and it is varied over `KeyCase::ALL` rather than through
`cfg!(windows)` — a rule written as a `cfg!` is a rule whose other arm
no test on this machine can reach.

## `fn the_container_boundary_is_case_sensitive_whatever_the_co…` › `assert_eq!(`

And the production constructor picks the sensitive rule, on every
platform this crate builds for.

## `mod tests` › `fn an_image_credential_variable_does_not_survive_into_a_role_that_takes_none() {`

An image credential variable does not survive into a role that takes
none — and "does not survive" means the composed vector **overrides**
it, not that the vector is silent about it.

#### The weaker true statement this test used to prove

`PR6-CORRECTNESS-007`. It asserted `value(&composed, "CODEX_HOME") ==
None` for a gate: true, and a claim about *this vector*, not about the
container. `docker create --env K=V` **overlays** the image's
environment — measured on `docker` 29.7.2 against an image declaring
`ENV CODEX_HOME=/image/codex`, a container whose spec names no
`CODEX_HOME` runs with `CODEX_HOME=/image/codex`. So the omission the
old assertion proved was exactly the mechanism by which the image's
value reached the role, and DESIGN.md:258-262's "each supplies
**role-scoped** … credential locations" was false of every role
`supplies_credential_location` refuses.

The grid is now **{image sets the key} × {role receives it}**, all four
cells, because the withholding is unconditional: a rule that neutralised
only what the base happened to carry would be correct exactly as far as
the image-environment read was complete.

`GH_CONFIG_DIR` is the control — an image variable that is *not* a
credential location — so "the withheld keys were overridden" is
distinguishable from "the image environment was wiped".

## `fn an_image_credential_variable_does_not_survive_into_a_rol…` › `let composed = environment`

(a) A role that takes none: the key is **named**, with nothing.

## `fn an_image_credential_variable_does_not_survive_into_a_rol…` › `assert_eq!(`

The control: the image's non-credential variable is carried
through **unchanged**, so what happened to `CODEX_HOME` is a
targeted override and not a wiped environment.

## `fn an_image_credential_variable_does_not_survive_into_a_rol…` › `let composed = environment`

(b) A role that takes one: the boundary's own path, never the
image's.

## `fn an_image_credential_variable_does_not_survive_into_a_rol…` › `let gate = scope(&ExecutionRole::Gate, Some(&codex), &volumes, &layout);`

And the withheld set is exactly "every credential location this scope
is not given" — an independent recomputation from
`CREDENTIAL_LOCATIONS`, not a read-back of the function under test.

## `mod tests` › `fn the_boundary_layout_derives_every_path_from_its_own_root() {`

Four in-container roots, and every derived path follows its own.

Two literals that must agree are two literals that drift, so the mount
target and the variable that points at it come from one value. Varying
every root moves every derived path, which is what says they are
derived; the five roots are asserted **pairwise distinct** so a layout
that collapsed two of them onto one path is visible.

## `fn the_boundary_layout_derives_every_path_from_its_own_root…` › `assert!(layout.git_pointer().starts_with(layout.workspace()));`

`git_pointer` is where a tool looks, so it is inside the workspace;
the view and the borrowed store are not, because a directory cannot
be bind-mounted onto a file and a read-only object mount over the
view's own `objects/` would make every write-side Git call fail.

## `fn the_boundary_layout_derives_every_path_from_its_own_root…` › `let distinct: std::collections::BTreeSet<&String> = before.iter().collect();`

Five distinct targets: a layout that mounted two things at one path
would hide one of them.

## `mod tests` › `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not_absolute() {`

A `PATH` component is cwd-dependent exactly when it is not absolute, and
the classification is compared against a table written here.

`PR6-CORRECTNESS-006`. The expected answers come from POSIX's own rule —
"a null pathname in `PATH` is a legacy spelling of the current working
directory" — and from what `.` means, not from calling the function and
recording what it said. Both hostile shapes are covered separately
because they are separately droppable: an implementation that tested
`component == "."` passes every `.` row and lets `PATH=/usr/bin:`
through, and one that tested `is_empty()` does the converse.

Second field held constant: the separator, which is `:` in every row —
what varies is only the shape of one component.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `("/usr/local/bin:/usr/bin:/bin", &[]),`

Absolute-only: nothing is relative.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `("/usr/local/bin:.:/usr/bin", &["."]),`

An explicit current directory.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `("/usr/bin:", &[""]),`

The empty component, in each of the three places a trailing,
leading or doubled separator puts it.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `("bin:/usr/bin", &["bin"]),`

A bare relative directory name, which is neither `.` nor empty.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `(".:/usr/bin:", &[".", ""]),`

Several at once, reported in order.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `("C:\\Windows", &["C", "\\Windows"]),`

A Windows-shaped value at a Linux boundary is one relative
component, not two absolute ones: `CONTAINER_PATH_SEPARATOR` is
`:` whatever the coordinator is, so this must refuse rather than
parse.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `let volumes = volumes();`

And the refusal is the composition step's, for every role: a value
that classifies as hostile must actually stop an invocation, and a
classification nobody consults would pass the loop above.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `let silent = ContainerEnvironment::inherited();`

No PATH at all is the other refusal, and it is the one
production had: `inherited()` composes an empty base.

## `fn a_path_component_is_cwd_dependent_exactly_when_it_is_not…` › `let good = ContainerEnvironment::from_image(image_base());`

The control: absolute-only composes, so the two refusals above
are about the value and not about composition being broken.

## `mod tests` › `fn credential_scoping_follows_inv18s_split_not_the_predicate() {`

The role rule is exhaustive over the five roles, and it is the packet's
split rather than a predicate that happens to agree with one.

The expected pairs are transcribed from INV-18 ("every agent CLI
invocation **incl. agent probes** … while gates **and the shell probe**
register without slots"), not computed from the function under test.

## `fn credential_scoping_follows_inv18s_split_not_the_predicat…` › `assert_eq!(`

And it is the same split as the slot rule, which is the sentence
both come from.
