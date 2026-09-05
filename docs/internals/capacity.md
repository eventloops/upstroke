# `src/capacity.rs`

Extended notes for [`src/capacity.rs`](../../src/capacity.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The capacity engine (DESIGN.md §13) — **read-only in v0.1**.

Three things live here: the pool types `~/.upstroke/pools.toml` parses into,
the estimator that turns observations into a per-pool remaining figure, and
the fold that collects those observations from a run's event log.

§13's sequencing is the reason nothing routes on any of it yet: v0.1 ships
the estimator so `upstroke capacity` and the dry-run preview can show what each
strategy *would* do, and v0.2 wires it into the binder once the estimates
have been watched for a while. So `pool_for` fills `WorkerProfile.pool` for
**attribution only** — the binder still picks models from the catalog and
pins, exactly as it did before this module existed.

The estimator is a pure function over plain values ([`estimate`]), so every
rule in §13 is exercisable with no CLI installed and no file on disk. Only
collection touches the world, and even that is a fold over events someone
else read.

Three properties hold by construction and are pinned by tests:

1. **Never optimistic.** No observation means [`Remaining::Unknown`], never
   "full". A pool nobody has measured is not a pool with capacity.
2. **Conservative.** Effective remaining is
   `max(0, raw × (1 − safety_margin) − reserve)` — the margin covers usage
   on other machines that local parsing cannot see, and the reserve is
   headroom the engine leaves for the operator's own interactive work.
3. **Trust order is ranked and fixed.** `Signal > SelfMetered > Assumed >
   Unknown` ([`Confidence`]), and a lower-ranked source can never overwrite
   a higher one — a rate-limit signal is ground truth, and a self-metered
   guess must not talk it back up.

**v0.2 sketch — credential profiles.** §13 wants one vendor backing several
pools (two Claude Max accounts, say), selected per attempt "through the
provider's own profile mechanism, an environment variable on the subprocess
rather than a token the engine ever handles". That mechanism is real on both
vendors and is a config-*directory* variable: `COPILOT_HOME` (documented) and
`CLAUDE_CONFIG_DIR` (works, undocumented as of Aug 2026). The shape that
fits is upstroke-defined profile directories — `~/.upstroke/profiles/<name>` —
handed to the CLI through that variable, with login staying a user-driven
interactive step the engine never automates. "Does this CLI honour profile
selection" then becomes a `probe()` axis like any other, verified at
pre-flight instead of discovered mid-spend. v0.1 stops at [`Pool::profile`]:
the field is parsed, displayed, and attributed through, and **nothing sets
any environment variable**, because everything multi-profile actually buys
(per-profile attribution, asymmetric reserve, rebind-instead-of-wait) is
capacity-*driven* behaviour, which is v0.2 by sequencing.

## `pub enum PoolKind {`

§13's pool shapes. Which one a pool is decides which estimator rule applies,
so an unknown value is a config error rather than a warning.

## `pub enum PoolKind` › `SubscriptionWindow,`

Claude Max and friends: a rolling window plus a weekly cap.

## `pub enum PoolKind` › `Credits,`

Copilot on AI-credit billing: a monthly allowance plus pay-as-you-go.

## `pub enum PoolKind` › `RequestPool,`

Copilot on a legacy annual plan: premium requests × per-model multiplier.

## `pub enum PoolKind` › `ApiKey,`

Direct API billing — dollars, no reset, budget-only.

## `pub enum PoolKind` › `Unmetered,`

A local endpoint: hardware-bound rather than quota-bound.

## `impl PoolKind` › `pub const ACCEPTED: [&'static str; 5] = [`

Accepted spellings, named once so the parser and its error message
cannot disagree about what is legal.

## `pub enum Source {`

§13's estimation sources, in trust order. Listing one is a statement about
where a pool's numbers may come from — dropping `signals` by typo would
discard ground truth, which is why an unknown entry errors.

## `pub enum Source` › `ProviderEndpoint,`

(4) Provider usage endpoints — fragile, never load-bearing. Parsed in
v0.1, never read.

## `pub enum Source` › `LocalLogs,`

(3) ccusage-style parsing of the agent's own logs, which is what sees
the operator's interactive sessions. Parsed in v0.1, never read.

## `pub enum Source` › `SelfMetered,`

(2) Self-metering of everything this engine spawned.

## `pub enum Source` › `Signals,`

(1) Rate-limit signals from the CLIs — ground truth.

## `impl Source` › `pub fn read_in_v0_1(self) -> bool {`

Whether v0.1 actually reads this source. The two that are only parsed
get a note on the estimate rather than a pretend number.

## `pub enum Allowance {`

`monthly_allowance = "auto"` or a number of units.

`Auto` is honest rather than convenient: it means the size of the allowance
is not known to upstroke, which is different from it being zero and different
again from it being unlimited.

## `pub const DEFAULT_SAFETY_MARGIN: f64 = 0.15;`

§13's default margins, applied when the pools file is silent. Both are also
what `upstroke connect` writes, so a hand-edited file and a generated one mean
the same thing.

## `pub struct Pool {`

One `[pools.<name>]` entry (§17).

## `pub struct Pool` › `pub agent: String,`

Which agent drains it. A pool naming an agent this build has no adapter
for is kept and marked unusable rather than rejected — §17's own example
ships `[pools.local] agent = "aider"`.

## `pub struct Pool` › `pub window: Option<Duration>,`

Rolling window, e.g. `"5h"`.

## `pub struct Pool` › `pub profile: Option<String>,`

§13's credential-profile seam: an operator-writable config-directory
path identifying *which account* this pool draws from. Parsed, carried,
and displayed; **nothing acts on it in v0.1** (see the module docs).

## `pub struct Pool` › `pub usable: bool,`

No adapter in this build for [`Pool::agent`]. The pool is still listed —
it describes the operator's subscriptions, not this binary's features.

## `impl Pool` › `pub fn discovered(name: &str, kind: PoolKind, agent: &str, sources: Vec<Source>) -> Self {`

A pool as `connect` writes one: §13's defaults, nothing invented.

## `impl Pool` › `pub fn describe(&self) -> String {`

One line for a preview or a listing: what this pool is and whose it is.

## `pub fn pool_for<'a>(agent: &str, pools: &'a [Pool]) -> Option<&'a Pool> {`

Which pool an attempt on `agent` drains: the first matching entry, table
order as preference — the same convention `different_family_at` uses, so
moving a pool up the file promotes it.

**Attribution only.** Nothing routes on the answer in v0.1; it fills
`WorkerProfile.pool` so the ledger can say which subscription paid.

## `pub struct Spend {`

---------------------------------------------------------------------------
Observations
---------------------------------------------------------------------------

## `pub struct Spend {`

What one pool has drained *through this engine*, in the run being folded.

## `pub struct Spend` › `pub usd: Option<f64>,`

Reported api-equivalent dollars. `None` means nothing reported any —
which is not the same as nothing costing anything (§13: the Copilot
route reports no spend at all).

## `pub struct Spend` › `pub unpriced: u32,`

Attempts whose route reported no cost, so `usd` above is a floor.

## `pub struct Observations {`

Everything the estimator is allowed to look at.

Deliberately plain data: [`estimate`] cannot reach past this into a file, a
process, or a clock, which is what makes every §13 rule testable without a
CLI installed.

## `pub struct Observations` › `pub exhausted: BTreeMap<String, Option<String>>,`

§13 source 1 — pools a rate-limit signal marked exhausted, with the
reset time where the signal carried one. Ground truth.

## `pub struct Observations` › `pub self_spend: BTreeMap<String, Spend>,`

§13 source 2 — self-metering of what this engine spawned.

## `pub fn observe(events: &[Event]) -> Observations {`

Fold a run's events into observations.

A pure function over events someone else read, so the estimator's inputs are
derived by exactly the mechanism every other reader uses (§15: the log is
the source of truth). Attempts that name no pool contribute nothing — an
unattributed cost belongs to no subscription, and guessing which would be
worse than leaving the pool unmeasured.

## `fn retire_signals(exhausted: &mut BTreeMap<String, Option<String>>, record: &AttemptRecor…`

Withdraw the exhausted mark from any pool this attempt proves is serving
again.

Without this a rate limit is permanent: `exhausted` was only ever inserted
into, nothing emits a recovery event, and [`Confidence::Signal`] outranks
every other source **by design** — so the one thing that could correct the
record was the one thing forbidden from doing so. A pool that refused an
attempt at 10:00, came back at 10:05 and served the rest of the run still
read `exhausted [signal]` at midnight, on the same line that reported the
successful attempts it had served since.

Events arrive in order, so a later signal re-marks a pool this retired —
which is right: the pool went down again.

What counts as proof is deliberately narrow. A *completed* attempt reached
the model and got an answer, whatever the verdict on its code — a gate
failure says nothing about the subscription. A rate-limited one proves the
opposite. An interrupted one proves nothing at all: the engine died without
ever learning whether a reply was coming.

## `fn retire_signals(exhausted: &mut BTreeMap<String, Option<S…` › `for review in &record.reviews {`

A review pass that reached a verdict proves its own pool served, which on
a cross-vendor second opinion is a different subscription entirely.

## `fn retire_signals(exhausted: &mut BTreeMap<String, Option<S…` › `if let Some(failure) = &record.failure {`

The attempt settlement itself is the durable source of a rate-limit
signal. `pool_exhausted` remains useful detail, but a crash between the
two appends must not make replay forget which subscription refused work.

## `pub fn drain_of<'a>(`

The same fold, over attempt records rather than events — what the ledger
needs, and what a reader holding folded state rather than raw events has.

Shared with [`observe`] rather than written twice: the ledger's per-pool
column and the estimator's self-metered source must be the same number, or
one of them is wrong and nothing says which.

## `fn accumulate(drain: &mut BTreeMap<String, Spend>, record: …` › `if let Some(pool) = &record.pool {`

An attempt naming no pool contributes nothing: unattributed cost belongs
to no subscription, and guessing which would be worse than leaving the
pool unmeasured.

## `fn accumulate(drain: &mut BTreeMap<String, Spend>, record: …` › `for review in &record.reviews {`

A cross-vendor second opinion drains a different subscription than the
implementer it judged (§11.3), so each pass is attributed on its own.

## `pub enum Remaining {`

---------------------------------------------------------------------------
The estimate
---------------------------------------------------------------------------

## `pub enum Remaining {`

How much of a pool is left, after margins.

## `pub enum Remaining` › `Unknown,`

Nothing measured this pool. **Never rendered as "full"** — the whole
point of the variant is that "we do not know" and "there is plenty" are
different answers (§13, invariant 7).

## `pub enum Remaining` › `Exhausted,`

A rate-limit signal said so. Ground truth.

## `pub enum Remaining` › `AtMost(f64),`

An **upper bound** on the fraction still available, after
`safety_margin` and `reserve`, clamped to `0.0..=1.0`.

Deliberately not "the fraction remaining". Self-metering sees only what
this engine spawned in this repository, so `1 − draw/allowance` is what
is left *if nothing else drew on the pool* — and something else almost
always did: earlier runs, other repositories, and the operator's own
interactive sessions (§13's source 3, which v0.1 parses and does not
read). Every one of those can only reduce what is left, never increase
it, so the figure is sound as a ceiling and false as a measurement.
Rendered with `≤` for exactly that reason.

## `pub enum Remaining` › `Unmetered,`

Hardware-bound rather than quota-bound (§13's local pools).

## `pub enum Confidence {`

Where an estimate came from, ranked. §13's trust order made into a type, so
"a lower-ranked source can never overwrite a higher one" is enforced by
[`Ord`] rather than by every call site remembering it.

## `pub enum Confidence` › `Unknown,`

Nothing measured it.

## `pub enum Confidence` › `Assumed,`

Derived from the pool's declared shape rather than from anything
observed — e.g. a local endpoint that cannot run out.

## `pub enum Confidence` › `SelfMetered,`

Self-metering of what this engine spawned.

## `pub enum Confidence` › `Signal,`

A rate-limit signal from the CLI itself.

## `pub struct PoolEstimate {`

One pool's estimated state.

## `pub struct PoolEstimate` › `pub reset_at: Option<String>,`

When the signal said the window reopens, where it said so.

## `pub struct PoolEstimate` › `pub self_spend: Option<Spend>,`

What this engine has drawn from the pool in the run being folded.

## `pub struct PoolEstimate` › `pub notes: Vec<String>,`

Everything the estimate could not account for, said out loud.

## `impl PoolEstimate` › `pub fn describe(&self) -> String {`

One line: the pool, what is left, and how confident that is.

## `pub fn estimate(pools: &[Pool], obs: &Observations) -> Vec<PoolEstimate> {`

§13's estimator: pools plus observations in, one estimate per pool out.

Pure and total. Every branch is reachable from plain values, so the three
properties in the module docs are testable without a CLI, a repo, or a file.

## `fn estimate_one(pool: &Pool, obs: &Observations) -> PoolEst…` › `let mut remaining = Remaining::Unknown;`

Ranked candidates, strongest first. `take` refuses anything that does not
outrank what is already held, so the trust order cannot be inverted by
adding a rule below an existing one.

## `fn estimate_one(pool: &Pool, obs: &Observations) -> PoolEst…` › `if let Some(reset) = obs.exhausted.get(&pool.name) {`

(1) Signals — ground truth, and the only thing that can say "exhausted".

## `fn estimate_one(pool: &Pool, obs: &Observations) -> PoolEst…` › `if pool.kind == PoolKind::Unmetered {`

A pool that cannot run out is a fact about its shape, not a measurement.

## `fn estimate_one(pool: &Pool, obs: &Observations) -> PoolEst…` › `if let (Some(spend), Allowance::Units(allowance)) = (&self_spend, pool.monthly_allowance)…`

(2) Self-metering. It bounds what is left only when the allowance's size
is known: `spend / auto` is not a fraction of anything. Otherwise the
draw is reported beside an Unknown remaining rather than dressed up as
one — §13's conservatism is about never overstating what is left, and
"we measured some spend" is not a measurement of the ceiling.

## `fn estimate_one(pool: &Pool, obs: &Observations) -> PoolEst…` › `let unread: Vec<String> = pool`

The two sources v0.1 parses but does not read. Saying so is the point: a
pool that lists `local-logs` and gets an estimate anyway would read as
though interactive usage had been accounted for.

## `pub fn effective_remaining(raw: f64, pool: &Pool) -> f64 {`

§13's conservatism, in one place: `max(0, raw × (1 − safety_margin) −
reserve)`, clamped into `0.0..=1.0`.

The margin covers what local measurement cannot see (the same subscription
used from another machine); the reserve is headroom deliberately left for the
operator's own interactive work. Applied multiplicatively then additively
because they are different claims: the margin says the measurement may be
wrong, the reserve says some of what is left is not ours to spend.

## `pub fn parse_duration(raw: &str) -> Option<Duration> {`

---------------------------------------------------------------------------
Durations
---------------------------------------------------------------------------

## `pub fn parse_duration(raw: &str) -> Option<Duration> {`

Parse §17's window spellings: `"5h"`, `"30m"`, `"7d"`, `"90s"`.

Dependency-free, and deliberately narrow — a window is one number and one
unit, so anything else is a typo worth naming rather than a format worth
guessing at.

## `pub fn render_duration(duration: Duration) -> String {`

The inverse, in the largest unit that divides exactly — so a window read
from a file and written back out again is the same string.

## `pub fn strategy_preview(mode: &str, estimates: &[PoolEstimate]) -> Vec<String> {`

---------------------------------------------------------------------------
Strategy preview
---------------------------------------------------------------------------

## `pub fn strategy_preview(mode: &str, estimates: &[PoolEstimate]) -> Vec<String> {`

What each §13 strategy *would* do with these estimates — the read-only half
of the capacity engine, and the whole of what v0.1 ships.

Phrased as "would", every line of it, because none of it is wired to the
binder. A preview that reads as a description of what is about to happen
would be a lie by tense.

## `pub struct CapacityOptions {`

---------------------------------------------------------------------------
`upstroke capacity`
---------------------------------------------------------------------------

## `pub struct CapacityOptions {`

`upstroke capacity [--config <path>] [--pools <path>]` (§18).

## `pub struct CapacityOptions` › `pub repo_root: std::path::PathBuf,`

Where to look for runs to self-meter from. Outside a git repository
there simply are none, which the report says rather than erroring on.

## `pub struct CapacityReport` › `pub agents: Vec<AgentStatus>,`

Live probe + discovery per agent named by a pool. **This** is where
probing belongs: `capacity` is allowed to spawn the vendors' CLIs, and
`validate` is not (§18).

## `pub struct CapacityReport` › `pub run_id: Option<String>,`

The run the self-metered figures were folded from.

## `pub fn report(`

Collect everything `upstroke capacity` reports: pools from config, self-metered
spend from the latest run in this repo, and a live probe per agent.

## `let (observations, run_id) = match crate::rundir::latest_run(&opts.repo_root) {`

Self-metering needs a repository with runs in it. Pools are user-level
(§17), so a listing outside a repo is still worth having — it just cannot
say what has been drawn. Reporting that beats refusing to run.

## `let runner = crate::runner::host::HostRunner::new();`

`capacity` runs no run, so it has no run's Runner to borrow: it
makes its own host one. That is still the Runner seam rather than a
bare spawn — `invariants_introduced[0]` is "every CLI and gate
process executes through Runner" — but it is deliberately *not*
inside INV-18's ambient job, which is the coordinator's and which
this command is not (`main::the_commands_that_spawn_outside_a_run_
are_named_and_counted`).

## `let missing = crate::catalog::missing_from(&pool.agent, &discovery.models);`

D1's cross-check: where the CLI can actually list its models,
say so when the shipped catalog names one it does not offer.
Load-bearing rather than tidy — a stale frontier slug fails
every cross-vendor second opinion at runtime, on exactly the
paths §11.3 exists to protect.

## `Err(error) => agents.push(AgentStatus {`

A CLI that is not installed is a fact worth reporting, not a
reason to refuse the whole listing: an operator asking about
capacity on a machine missing one vendor still wants the other.

## `fn an_unmeasured_pool_is_unknown_never_full()` › `let estimates = estimate(&[pool("claude-max")], &Observations::default());`

Property 1. The trap this exists to stop: rendering "no observation"
as 100% would make a dry subscription look like the best pool to
route to, which is exactly backwards.

## `fn margins_apply_multiplicatively_then_subtract_the_reserve…` › `let pool = pool("claude-max");`

Property 2, on the arithmetic itself: 0.5 × 0.85 − 0.20 = 0.225.

## `fn margins_apply_multiplicatively_then_subtract_the_reserve…` › `assert_eq!(effective_remaining(0.1, &pool), 0.0);`

Never negative, never above one, and never NaN-propagating.

## `fn a_self_metered_estimate_is_conservative_end_to_end()` › `let Remaining::AtMost(left) = estimates[0].remaining else {`

Half the allowance spent, and the margins take it down from there —
it must never come out at the raw 50%.

## `fn a_self_metered_estimate_is_conservative_end_to_end()` › `assert!(`

And it is presented as the ceiling it is, not as a measurement: this
counts one run's draw against a *monthly* allowance, so every earlier
run and every interactive session is unseen.

## `fn a_pool_that_serves_again_stops_reading_as_exhausted()` › `use crate::events::{AttemptRecord, Event, EventBody, PoolExhausted};`

A signal is ground truth about the moment it was recorded, not
forever. Without retirement `Confidence::Signal` outranks every
source that could correct it, so one rate limit at 10:00 makes the
pool read as empty at midnight — on the same line that reports the
attempts it served in between.

## `fn a_pool_that_serves_again_stops_reading_as_exhausted()` › `let obs = observe(&[signal.clone(), record(None)]);`

Signal, then an attempt that completed: the pool is serving.

## `fn a_pool_that_serves_again_stops_reading_as_exhausted()` › `let obs = observe(&[signal.clone(), record(Some(FailureKind::GateFailed))]);`

A gate failure also proves the model answered — the verdict on the
code says nothing about the subscription.

## `fn a_pool_that_serves_again_stops_reading_as_exhausted()` › `for still_down in [FailureKind::RateLimited, FailureKind::Interrupted] {`

A second rate limit proves the opposite, and an interrupted attempt
proves nothing at all — the engine died without learning whether a
reply was coming.

## `fn a_pool_that_serves_again_stops_reading_as_exhausted()` › `let obs = observe(&[signal.clone(), record(None), signal]);`

And order is respected: recovery then a fresh outage stays down.

## `fn a_pool_that_serves_again_stops_reading_as_exhausted()` › `let reviewer_limited = Event::now(EventBody::AttemptFinished {`

A reviewer-side limit is attached to the failed review pool, while
the same settlement proves the worker and earlier reviewers served.

## `fn self_metering_cannot_talk_an_exhausted_pool_back_up()` › `let mut p = pool("claude-max");`

Property 3, in the form that matters: a signal said the pool is
empty, and a self-metered figure computed from a generous allowance
must not overwrite it. Getting this backwards would route work at a
pool the CLI has already refused.

## `fn self_metering_cannot_talk_an_exhausted_pool_back_up()` › `assert_eq!(`

The draw is still reported — the signal says the pool is empty, not
that this run drew nothing.

## `fn an_unknown_allowance_reports_the_draw_without_inventing_…` › `let mut obs = Observations::default();`

`spend / auto` is not a fraction of anything.

## `fn every_strategy_preview_says_it_changes_nothing()` › `for mode in ["conserve", "value-max", "deadline", "something-else"] {`

The read-only promise is the one line that must survive every mode:
a preview that reads as a description of what is about to happen
would be a lie by tense (§13's sequencing).
