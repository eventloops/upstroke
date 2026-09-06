# `src/config/parse.rs`

Extended notes for [`src/config/parse.rs`](../../../src/config/parse.rs).

## Module

The `upstroke.toml` section readers (DESIGN.md §17).

One function per section — `[runner]`, `[routing.effort]`, `[budgets]`,
`[[gates]]`, `[engine]`, `[interaction]` — each taking the section's raw
`toml::Value` and returning the typed shape the parent's `Config` is built
from. The parent owns the ladder that calls them and the types they return;
this module owns only the readings.

**The error-versus-warning split is a rule, not a per-section taste.** A key
whose typo would silently delete a control the operator asked for is a hard
error; a key whose typo only degrades what the run can say about itself is a
warning that names the key. Each function's own documentation records which
side of that line its keys sit on and why, because the reason is what a
later reader needs in order to place a new key correctly. **No section may
sit on neither side.** A key that is neither consumed, refused nor named is
a key that vanished, and a vanished key is indistinguishable from one that
was never written: `[[gates]]` and `[interaction]` sat there until 2026-09-04
(`RawGate` and `RawInteraction` now refuse what they do not read). **And an
unknown key is never a warning**, in any section: a key typo cannot be told
from a deleted key, and every section here holds at least one control —
`[engine]` warned until 2026-09-05, and `on_task_failur = "continue"` was
a halted run the operator asked to continue, with a footnote.

**A resume reads `[[gates]]` by what its log records** ([`EngineLimits`]):
with a gate record, today's section is compared with the record and never
refused over (`design/15`); without one, it settles the run's gates and is
read as a fresh run reads it. The `[engine]` ceilings warn on either resume.
Nothing else reads differently on a resume.

**A parse failure never becomes a default, with one stated exception.**
Every `unwrap_or`, `map_or` and `unwrap_or_else` in this module but one
folds an *absent* key into the default the design gives it; a key that was
written and could not be read is refused by the section's `try_into` before
any fold is reached. The one exception is `[engine] shell`, whose
`unwrap_or_else` folds a written value the parser did not recognise into the
platform default, and warns by name — see [`parse_engine`].

Nothing here reads a path, starts a process, or writes anything: every input
arrives as an already-parsed `toml::Value`, and the reading of the bytes it
came from belongs to `super::read`.

**What a refusal carries.** Every error is [`UpstrokeError::Config`] with
the file's path and a message that names the section and the key, and the
value written when the value is what is wrong (`[engine] \`max_parallel = 4\``,
`[runner] \`kind = "vm"\``, `[[gates]] entry 2 (\`test\`)`); an unknown key is
named without its value, since the key is the mistake. That is enough to
find the line by eye; a byte offset is not carried, because `toml::Value`
has already dropped the span by the time a section reaches here.
**This child states its own lint level and inherits nothing.** A Rust lint
level is scoped by the module tree rather than by the file, so an out-of-line
child of `src/config.rs` inherits that file's inner
`#![allow(clippy::disallowed_methods)]` unless it says otherwise --
`PR6-LANEF-004`, and the mistake two W1 pull requests then made
independently (#100 and #102). Nothing here reaches a governed primitive, so
all three governed lints are DENIED and this module takes no
`effects/allowlist.toml` row: a row records an allowance, and this module
takes none.

The three are not equally load-bearing, and which is which is worth stating.
`src/config.rs` allows `clippy::disallowed_methods` and that lint alone, so
the first line below is the one that restores a level the parent removed
outright: without it, a denied method here raises no diagnostic at all. The
other two raise this module from clippy's default `warn` to `deny`, so a
denied type or macro fails here on its own rather than only under CI's
`-D warnings`. All three are written out because what decides the first one
is a property of the parent's attribute rather than of this file, and a
parent's attribute can widen without this file changing.

## `fn config_error(repo_path: &Path, message: String) -> UpstrokeError {`

The one error shape every reader here returns: [`UpstrokeError::Config`]
for the file at `repo_path`.

A function rather than a closure per reader, so a refusal is built the same
way at every site. The path copy is the refusal's own: an error value is
returned past the borrow it was built under, so it owns what it names (§6).

## `pub(super) fn read_runner(`

`[runner]` as written, with no policy applied.

Every key is an **error** when wrong, and an unknown key is an error too —
the section exists to decide whether gate code executes confined, and a
typo that leaves it on the host is the one mistake it must not absorb. The
only defaults here are for keys that are *absent*: no `kind` is the host
runner, no `credential_volumes` or `mounts` is none of either, and a mount
with no `read_only` is read-only. A key that is present and unreadable is
refused by the `try_into` before any of them is reached.

### Errors

[`UpstrokeError::Config`], naming the key: the section is not a table of
the accepted keys; a key outside [`RUNNER_KEYS`]; a `kind` other than
`host` or `container`; `image`, `credential_volumes` or `mounts` under the
host runner; a container with no `image`, or a blank one; a blank agent id
or volume name; a mount with a blank `target` or an empty `source`.

## `if !runner.unknown.is_empty() {`

An error. A typo here — `knid = "container"`, `iamge = "..."` — leaves
the run executing on the **host** while the operator believes gate code
is confined, which is the one thing this section exists to decide. Same
rule as every other section, for the same reason: silently ignoring a
key is silently deleting a control. Every unknown key is named, so one
load reports the whole section.

## `let kind = match runner.kind.as_deref() {`

Spelled here and pinned to `RunnerKind`'s own wire spelling by
`tests::the_runner_kind_words_are_the_wire_spelling`, so the config and
the record cannot drift apart without a test saying so.

## `let stray: Vec<&str> = [`

The config-side twin of `RunnerRecordDefect::HostWithContainerFields`,
which PR3 already refuses on the recorded side. An operator who set
`kind = "host"` under an image line has described two boundaries and
gets one; accepting it silently is how a run executes unconfined
while its config reads as if it did not.

## `let selected = match runner.kind {`

The message quotes what the file says. `kind = "host"` was
written in one case and not in the other, and a refusal that
quotes a line the operator never wrote sends them looking for
it.

## `let credential_volumes = runner.credential_volumes.unwrap_or_default();`

Absent is none, not a failure: an unreadable map was refused above.

## `for mount in runner.mounts.unwrap_or_default() {`

Absent is none, not a failure, as for the volumes above.

## `read_only: mount.read_only.unwrap_or(true),`

Writable is a thing you say. See `RunnerMount`. An absent key,
not a failed reading: a `read_only` that is not a boolean was
refused by the `try_into` above.

## `pub(super) fn refuse_legacy_container_selection(`

`expected_failures_refusals[0]`: "`[runner] kind = container` under a
schema-1..3 fresh run **or** resume -> config error before any effect".

#### Why this is structural and not stylistic

`production_effect`: "the legacy engine's preflight probes precede any run
identity or lock and can own no container intent". R26's own rule is that
"no container ever lacks a race-free owner or a durable boundary identity",
and the schema-1..3 engine has nothing to give one: it probes before it has a
run id, a `run.lock` or a recorded runner. So refusing **late** — after a
probe, after a lock, after any effect — is not a weaker version of this
refusal, it is a different and broken one: the container it would refuse
already exists and belongs to nobody.

#### Where "before any effect" is bought

Here, by position. Both write commands run
`preflight::validate_inputs` — which is `config::load_captured` — as their
first statement, before `Workspace::open`, before `WorktreeLock::acquire_in`
and before `RunPaths::create`: `coordinator.rs`'s comment on that line is
"every read-only refusal precedes every lock", and `resume.rs` marks the
line after it "the first effect of the command".
`runner::container::resolve::tests::legacy_container_selection_refused_before_effects`
drives both commands and asserts the tree afterwards.

#### Every reading refuses, and that is the whole of today's answer

[`EngineLimits`] distinguishes a run being created from a sequential run's
resume — in two flavours since 2026-09-05, by whether the log records its
gates, which changes how `[[gates]]` is read and nothing here — and
`expected_failures_refusals[0]` names a fresh run **and** a resume. No
reading accepts a container selection in this build: `EngineLimits::Fresh`
means "a run being created now", and every run this binary creates is
schema-3. `PR12 config acceptance for fresh schema-4 runs only` (INV-23's
`enforced_by`) is where a fresh schema-4 run learns to accept it, and that
is a new reading rather than a relaxation of this one.

### Errors

[`UpstrokeError::Config`] when `selection` is a container selection.

## `pub(super) fn parse_role_effort(`

Parse one role's explicit effort at config load. All three providers reject
an unknown value after process launch, so accepting a typo here would burn an
attempt for a routing policy the operator never actually selected.

### Errors

[`UpstrokeError::Config`] naming the role and the value when `raw` is not
one of [`Effort::KNOWN`]. An absent value is `Ok(None)`: no policy.

## `pub(super) fn parse_budgets(`

`[budgets]` (§17). A ceiling that is zero, negative, or not a number is a
hard error: every one of those readings would either stop the run before it
began or be ignored, and which of the two happened must not be a surprise.
An unknown key is a hard error too, by `Budgets`' own derive.

### Errors

[`UpstrokeError::Config`]: the section is not a table of the two optional
numbers, or a ceiling fails [`check_budget`] — non-positive, or not finite,
which TOML can write as `inf` and `nan`.

## `const MAX_GATE_TIMEOUT_SECS: u64 = u64::MAX / 1000;`

The largest `timeout_secs` the run record can carry back exactly.

`run_started` records each gate's timeout as `timeout_ms`, a `u64` written
by `crate::util::duration_millis`, which **saturates** at `u64::MAX`
milliseconds rather than failing. A larger value would load, be recorded
smaller, and be resumed at the recorded value with a drift warning against
a file nobody edited — the opposite of `design/15`'s record-and-resume-
exactly. So the reader refuses what the record cannot hold; the tests pin
this bound to the serialiser's own arithmetic.

## `pub(super) fn parse_gates(`

`[[gates]]` parsing with actionable shape errors: a `[gates]` table, a
wrong-typed field, or `timeout_secs = 0` all name what was expected.

**Every key is an error when wrong, and an unknown key is an error on a
fresh run.** An entry has three keys and the only optional one,
`timeout_secs`, decides when a running gate is killed and reported as
failed: `timeout_sec = 3600` on a gate that needs an hour is a gate that
fails at the 600 s default, and the ladder then spends attempts repairing
code that passes. That is a control deleted by a typo, which is the module's
rule for an error, not a warning.

**Two entries may not share a `name`, on a fresh run.** The name is what a
gate's log file is written under (`<task>-<attempt>-<name>.log`, in
`gates::run_all`) and what its failure report carries, so a second gate with
the first's name replaces the first's log in the same attempt and reports a
failure the operator cannot attribute. Names are compared **without regard
to ASCII case**: two of the three CI platforms keep their logs on a
case-insensitive filesystem (NTFS, APFS as shipped), where `check` and
`Check` are one file, and a config must not behave differently per platform.
ASCII case and no more, because `util::filename_component` maps every
non-ASCII character to `-` before the name reaches the filesystem, so ASCII
case is the only folding the filesystem can apply to what is written; the
collisions that mapping itself creates (`lint fast` and `lint-fast`) are the
log writer's, `SWEEP-CONFIG-PARSE-011`.

**Which reading, and why the parser cannot choose it alone.** Under
[`EngineLimits::Fresh`] today's section governs the run, and every shape
above refuses. Under [`EngineLimits::SequentialResumeWithRecordedGates`] the run's log
records its gates and `design/15` is explicit that they are taken from the
record, not re-derived, and **not refused over**: today's section is read
only to be compared with them, so nothing here refuses — every shape,
including a zero timeout, a blank field, an entry that is not a table and a
section that is not an array, is a warning naming the recorded gates as
what runs, and the list carries what could be read so the comparison can
still say what moved (an unreadable entry is skipped; an unreadable section
is `Ok(None)`). Under [`EngineLimits::SequentialResume`] the
log has no gate record, this resume settles the run's gates from today's
file and records them, so the section governs and is read exactly as a
fresh run reads it. Which of the two resumes applies is a fact about the
log, not about the config, which is why `EngineLimits::for_resume` takes
it from `events::recorded_gates` and this function only asks the reading.
An earlier version keyed the downgrade off "a resume" alone and was wrong
both ways: it promised the record would run when a legacy log had none,
and it kept refusing a run that had one. A later draft put the compare-only
reading on `SequentialResume` itself, which silently changed what a caller
passing that public variant directly had always got; the reading lives on
the variant that names it, and `SequentialResume` governs as it always did.

### Errors

Under `Fresh` and `SequentialResume`, [`UpstrokeError::Config`]
naming the entry by position: `gates` is not an array; an entry is not a
table; `name` or `cmd` is missing, named beside any unknown key the entry
carries; a blank `name` or `cmd`, naming which; a key outside those three
on an entry that has both; `timeout_secs = 0`, or more than
[`MAX_GATE_TIMEOUT_SECS`]; a `name` an earlier entry has, compared without
regard to ASCII case. Under
`SequentialResumeWithRecordedGates`, never: each of those is a warning in `warnings`. On
every reading `Ok(None)` is an absent section and `Ok(Some(vec![]))` an
explicitly empty one — the parent's `Config::gates` says what each means —
with one exception the warning names: under `SequentialResumeWithRecordedGates` a section
that is not a list is also `Ok(None)`, there being no other shape for "no
list", so the engine derives defaults to compare with the record and the
warning says any reported difference is against those, not the file
(`SWEEP-CONFIG-PARSE-026`).

## `let reading = match limits {`

The one exhaustive decision: which of the three readings this is.

## `warnings.push(format!(`

Compared only, and nothing in the section can be compared. `None`
is the only shape `Config::gates` has for "no list", and downstream
it means "derive from the repository", so the comparison the engine
then reports is against derived defaults rather than the file — the
warning says so, and `SWEEP-CONFIG-PARSE-026` records the typed
state that would let the comparison be skipped instead.

## `continue;`

Compared only, and this entry cannot be built: skipped, and
the comparison says the record has a gate today's file lacks.

## `let (name, cmd) = match (g.name, g.cmd) {`

A required key that is absent is named beside any key the entry has
that nothing reads: `nmae = "check"` is a missing `name` and an
unknown `nmae`, and the operator is told both, since the second is
almost always the first misspelt. Serde's own "missing field" would
have named only the first.

## `continue;`

Compared only, and this entry cannot be built: skipped.

## `timeout: g`

An absent key, not a failed reading: a `timeout_secs` that is
not a whole number was refused (or announced and skipped) above.
Compared only, a zero written today is carried as zero so the
comparison can name it; it never runs, the record does.

## `#[derive(Clone, Copy)]`

What today's `[[gates]]` section is *for* on this load — the one question
[`parse_gates`] asks of [`EngineLimits`]. See its doc for which reading
maps to which.

## `Governs,`

The gates this run executes come from this section: refuse a shape
the engine cannot act on.

## `ComparedOnly,`

The gates this run executes come from its record; this section is
read only to say what moved. Nothing refuses.

## `fn refuse_or_announce(`

A `[[gates]]` shape the engine cannot act on: refused where the section
governs the run, announced where it is only compared with the recorded
gates (`design/15`: taken from the record and not refused over). See
[`parse_gates`].

## ``const ENGINE_KEYS: &str = "`shell`, `on_task_failure`, `max_parallel`, `max_merge_repairs`, \``

Every accepted `[engine]` key, written out, for the same reason as
[`RUNNER_KEYS`]: the refusal names this list, so a key that stops being
read is a key that stops being offered.

## `pub(super) fn parse_engine(`

`[engine]` (§17).

Every key here is now consumed or refused. Nothing is
read past: accepting `max_parallel = 4` and then running one attempt at a
time is the failure a config file exists to prevent — the operator believes
they bought four workers, the run costs and takes what one worker costs and
takes, and nothing anywhere says otherwise. That is the same silent-ignore
harm `second_opinion` and `[budgets] pool_fraction` each earned a refusal
for, and it is this section's own long-standing defect.

The three ceilings split from `max_parallel` on which reading is wrong.
`max_parallel` above 1 describes a run **this engine cannot perform**, so on
a fresh run it is a hard error — raised here, which is before a lock, a
workspace, or a run directory exists. `max_merge_repairs`, `max_per_agent`,
and `max_per_pool` bound a topology that arrives with the parallel engine; a
nondefault value is a true statement about a later run and a silent no-op in
this one, so it parses, is kept, and warns.

`limits` is what keeps that refusal from reaching a run it cannot help. See
[`EngineLimits`]: on a sequential run's resume every one of these keys is
about a future run, `max_parallel` included, so all four warn and the resume
continues on the ceiling it recorded.

**Which side each key sits on.** An unknown key **errors**, every one
named with the accepted set, on every reading. Until 2026-09-05 it warned,
on the reasoning that the keys here are ceilings and timeouts a typo merely
leaves at their defaults — but `on_task_failure` and `shell` sit in the
same table, and `on_task_failur = "continue"` under that reasoning was a
run that halted on its first failed task with a warning beside it: a
deleted control with a footnote. A key typo cannot be told from a deleted
key, so the table refuses, as every other section does; a misspelled key
governs no run, recorded or not, so the resume readings change nothing
here. An unknown `shell` **value** **warns** and takes the platform default
— the gate commands still run, under the shell the platform would have used
— and it is the one value in this module that degrades rather than refuses
(`SWEEP-CONFIG-PARSE-012`). `on_task_failure` **errors** on an unknown
value: a misspelling there decides whether a failed task stops the run. A
zero ceiling **errors** on every reading of `limits`: "no ceiling" and
"nothing may run" are two meanings, and a resume must not become a way
around the check.

### Errors

[`UpstrokeError::Config`]: the section is not a table of the six optional
keys; a key outside [`ENGINE_KEYS`]; `on_task_failure` is not `halt` or
`continue`; any ceiling is zero or not a whole number; `max_parallel`
above [`DEFAULT_MAX_PARALLEL`] on a fresh run.

## `let shell = match engine.shell {`

The one degrade-and-warn in the module; the reason is in the doc above.

## `let on_task_failure = match engine.on_task_failure {`

A misspelling here decides whether a failed task stops the run, so it
errors rather than warning: silently halting a run the user asked to
continue (or the reverse) is not a recoverable surprise.

## `let limit = |key: &str, configured: Option<u32>, default: u32| -> Result<u32, UpstrokeError> {`

Zero has two readings — "no ceiling" and "nothing may run" — and which one
happened must never be a surprise. The rule `attempts_per` and every
`timeout_secs` already follow. `None` is an absent key and takes the
default; a written value that is not a whole number never reaches here.

## `let max_parallel = match (limits, configured_parallel > DEFAULT_MAX_PARALLEL) {`

What this load's run will actually be allowed to do. It parts company
with what the file says in exactly one case — a sequential run's resume,
whose ceiling is a fact about the run and not about today's config — and
that case says so out loud below rather than carrying the file's number
into a Config field nothing may act on.

## `let max_per_agent = limit("max_per_agent", engine.max_per_agent, configured_parallel)?;`

Defaulted off what the file asked for rather than off the effective
ceiling: `max_parallel = 3` with neither companion written is one
statement about a future run, and splitting it into a refused 3 and two
inherited 1s would announce two edits the operator never made.

## `for (key, configured, default) in [`

Kept, and announced as inert. A warning rather than an error because the
value is not wrong — it is simply about a run this build cannot perform
yet, and erroring would refuse a config an operator wrote for the engine
they are waiting for.

Each companion is compared against the ceiling it was defaulted from —
the one the file asked for, not the effective one — so a companion the
operator never wrote is never announced: on a sequential resume,
`max_parallel = 3` alone is one warning naming `max_parallel`, not three
naming two keys the file does not contain. On a fresh run the two
ceilings are equal and the choice makes no difference.

## `pub(super) fn parse_interaction(`

`[interaction]` (§12).

Everything here is a hard error or nothing: `mode` and `ask_before` both
decide whether a human is ever asked, so a typo in either must not degrade
quietly. Notifier ids are the one soft setting, and they are validated by
`notifiers_for` at run time rather than here — which is why this function
takes no warning sink.

**An unknown key is a hard error** (`RawInteraction` refuses it), for the
reason the two hard keys give: a key typo cannot be told from a deleted
key, so `ask_befor = { frontier_escalation_over_usd = 5.0 }` is a spend
approval that no longer exists and `mod = "never"` is a CI run that will
stop to ask a person. Until 2026-09-04 both loaded without a word.

### Errors

[`UpstrokeError::Config`]: the section is not a table of the four optional
keys; a key outside them; an `ask_before` key outside
[`AskBefore::ACCEPTED`]; a `frontier_escalation_over_usd` that is negative
or not finite (`0.0` is a threshold: ask before any frontier escalation); a
`mode` other than `never`, `on_block` or `on_milestone`.

## `let ask_before = match interaction.ask_before {`

An unknown key inside `ask_before` errors rather than warning: the whole
point of the table is to stop the run and ask, so a misspelling that
silently drops the threshold spends the money the operator asked to be
consulted about. Same reasoning as `second_opinion`.

## `if !threshold.is_finite() || threshold < 0.0 {`

§5: a floating-point input rejects non-finite values before it is
budgeted. `nan` compares false with everything, so a NaN threshold
is one that never fires; `-inf` is one that always does.

## `notify: interaction.notify.unwrap_or_else(default_notify),`

Absent keys take §12's defaults; a written key that could not be
read was refused by the `try_into` above.

## `#[cfg(test)]`

The readers driven directly, each on a `toml::Value` built in the test, so
every assertion is about one section and no file is written. The parent's
suite drives the same readers through `load` and a scratch file; these pin
the refusals and diagnostics that suite did not, each named for the
sentence it proves. The mutation each was witnessed against is recorded in
the Validation section of the pull request that added it.

## `fn section(body: &str) -> toml::Value {`

A section body as the `toml::Value` the readers take.

## `fn refused<T>(result: Result<T, UpstrokeError>, what: &str) -> String {`

The message of a config refusal, or a panic naming what was expected.

## `for (typo, body) in [`

Each is the realistic typo of one accepted key, and each used to load
with that key's default in place: `mod = "never"` was a CI run that
would stop to ask a person, `ask_befor` a spend approval that no
longer existed.

## `let settings = parse_interaction(`

The control: the four accepted keys together are the shape every
refusal above is one letter away from.

## `let body = "[[gates]]\nname = \"test\"\ncmd = \"cargo test\"\ntimeout_sec = 3600\n";`

`timeout_sec = 3600` on a gate that needs an hour used to be a gate
killed at the 600 s default, with nothing said.

## `let mut warnings = Vec::new();`

A run whose log records its gates is resumed through the same
file: the section is compared only, so the key is named, the entry
is kept at the default the typo left it at, and the recorded gates
are named as what runs.

## `let mut warnings = Vec::new();`

A run whose log has no gate record settles them from this file, so
the file governs and the typo is refused exactly as for a fresh run:
the reviewer's slow gate is not killed at 600 s with a footnote.

## `let mut warnings = Vec::new();`

The control: the same entry with the key spelt right, and the value
reaches the gate under either reading.

## `let mut warnings = Vec::new();`

Two names that differ only in ASCII case are one log file on NTFS
and on APFS as shipped, so they are one name here on every platform.

## `let mut warnings = Vec::new();`

A run whose log records its gates is resumed through the same
file: both entries are kept, so the record can be compared with
them, and the repeat is announced with the recorded gates named as
what runs. A run with no gate record refuses it as a fresh run does.

## `let mut warnings = Vec::new();`

The control: the same three commands under three names, kept in file
order, with nothing announced.

## `for (body, expected) in [`

A blank field refuses wherever the section governs the run, and the
refusal says which field, where it used to say only that one of the
two was blank. Where the section is only compared with a record it
is announced, with the same words, and the entry kept as written.

## `let zero = "[[gates]]\nname = \"test\"\ncmd = \"cargo test\"\ntimeout_secs = 0\n";`

The shapes a fresh run refuses that the tests above do not already
drive under the compare-only reading: a zero timeout (carried as
zero so the comparison can name it), an entry that is not a table
(skipped, since nothing can be built from it), and a section that
is not an array (nothing to compare: `None`). Each is announced
with the recorded gates named as what runs, and none refuses —
`design/15`'s "not refused over", shape by shape. The same three
refuse under the two readings where the section governs.

## `assert!(`

And the second warning disowns the comparison the engine will make
against derived defaults, so a "difference" reported after it is
not read as an edit to this file.

## `for body in [zero, not_a_table, not_an_array] {`

The control: where the section governs, each of the three refuses.

## `for limits in [`

`on_task_failur = "continue"` used to warn and leave `halt` in
place: a deleted control with a footnote. Every unknown key is
named, the accepted set is listed, and no reading softens it — a
misspelled key governs no run, recorded or not.

## `for limits in [`

The control: the key spelt right is consumed, on every reading.

## `let mut warnings = Vec::new();`

`max_parallel = 3` alone is one statement about a future run. The
two companions default to it, and a companion the operator never
wrote must not be announced as if the file contained it.

## `let mut warnings = Vec::new();`

A companion the file did write at a value other than its default is
announced beside it, so the rule tracks what was written and not the
key's presence alone.

## `for (word, expected) in [`

`RunnerKind` is PR3's wire kind, and the config and the record have
to be comparable. The words this reader accepts are pinned to the
words the type's own `Deserialize` accepts, so neither can move
without the other.

## `for word in ["Host", "CONTAINER", "container "] {`

And a spelling the wire refuses is refused here, so this reader is
not looser than the record it must compare against.

## `for value in ["-1.0", "-inf", "inf", "nan"] {`

A NaN threshold compares false with every spend and never fires; a
negative or `-inf` one fires before a dollar is spent. Neither is a
threshold the operator can have meant.

## `for (value, expected) in [("0.0", 0.0), ("5.0", 5.0), ("12", 12.0)] {`

Zero is a threshold — ask before any frontier escalation — and so is
any finite non-negative number.

## `for body in [`

TOML writes `inf` and `nan` as numbers, and neither is a ceiling: a
NaN ceiling is never reached and an infinite one never fires, which
is "unlimited" spelt as a limit. The parent's suite pins zero and a
negative; these are the other two arms of `check_budget`.

## `let typo = "[[gates]]\nnmae = \"check\"\ncmd = \"cargo check\"\n";`

`nmae = "check"` used to fail inside serde as "missing field `name`",
which names the field the operator did not write and not the one
they did. Both are named now, on the readings where the section
governs; where it is only compared, the entry cannot be built and
is skipped with the same words.

## `let mut warnings = Vec::new();`

Missing with nothing misspelt is named alone, so the sentence about
a misspelling is not printed where there is none to point at.

## `assert!(`

The bound is the serialiser's own arithmetic, asserted here rather
than restated: `duration_millis::serialize` writes
`u64::try_from(d.as_millis()).unwrap_or(u64::MAX)`, so the largest
whole second it carries exactly is the one whose millisecond count
still converts.

## `let mut warnings = Vec::new();`

Compared only: announced and carried as written, since it never runs.

## `let at = format!(`

The bound itself loads, on every reading, and reaches the gate.
