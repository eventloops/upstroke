# Rust coding standards

This document is the normative implementation standard for Rust code in upstroke. It applies to
production code, tests, examples, build support, and code-generation inputs. It is deliberately
project-specific: official Rust guidance is the foundation, while upstroke's product invariants,
failure modes, and supported platforms determine the stricter rules.

Last reconciled with the sources listed below: 2026-08-28.

## 1. Authority and scope

The documents govern different questions:

- [`DESIGN.md`](DESIGN.md), especially §4, governs product behaviour and architecture.
- This document governs implementation quality and Rust engineering practice.
- [`MAINTAINING.md`](MAINTAINING.md) governs integration, review, release, and emergency process.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) is the contributor entry point and licence agreement.
- Dated records in `decisions/` explain why a decision was made; proposals do not bind the
  implementation unless the design adopts them.

When these disagree, do not choose silently. Product requirements in `DESIGN.md` take precedence
over an implementation preference, but the conflicting documents must be reconciled in the same
change. CI configuration describes automated enforcement; it does not weaken a requirement here
merely because that requirement needs human review.

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** express requirement
strength. A `SHOULD` deviation needs a concrete reason in the code or pull request. A `MUST`
deviation needs an explicit, reviewed change to this standard or to the controlling design—not an
ad hoc exception.

Existing code is not precedent against this standard. A change MUST leave the code it materially
touches compliant. Unrelated debt should be recorded rather than hidden inside an unreviewable
scope expansion, unless it creates an immediate correctness or security risk; deferring such a
risk requires an explicit owner and rationale.

### Known conflicts at adoption

`DESIGN.md:222` freezes `CommandSpec.program` as `String`, while §8 requires paths to use
OS-native path types. This conflict is unresolved and is tracked as the open owner question
`PR4-PROGRAM-PATH-NOT-UNICODE` in the parallelism workstream's `reviews/FINDINGS.md`. Its decision
venue is workstream W4, in the G2 pass over PR3's layer. Until that ruling, the frozen design
governs this one field; the exception is recorded rather than treated as precedent for other paths.

## 2. Automated baseline

Every change MUST pass the same commands as CI, from the repository root:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo +1.85.0 check --locked --all-targets --all-features
bash .github/scripts/test-release-record.sh
bash .github/scripts/test-pr-policy.sh
bash .github/scripts/test-pr-ledger-evidence.sh
bash .github/scripts/test-docs-consistency.sh
```

Run all eight commands from the repository root. `CLAUDE.md`'s Gates section records the known
root-invocation trap and the extra `jq` prerequisite for the release-record fixture.

These eight commands run on one operating system, which bounds what they can establish about a
`cfg` region compiled only for another target. Parsing and formatting still reach an inactive
inline block — `cfg` stripping happens after parsing, and rustfmt formats disabled source — so a
syntax error or a formatting violation inside one fails the baseline on any host. Nothing past
that does: a stripped block is never type-checked, its lint attributes are never evaluated, and
its behaviour is never run. CI carries the other platforms; §11 states which leg evidences which
kind of platform-gated claim.

The project uses Rust edition 2024 and has an MSRV of 1.85.0. Code and dependencies MUST remain
compatible with both the MSRV and the current stable toolchain. A green baseline is necessary,
not sufficient: the compiler, rustfmt, and Clippy cannot establish the behavioural rules below.

Lint policy:

- Fix warnings at their cause. Do not use crate- or module-wide `allow(warnings)`.
- A lint suppression MUST be as narrow as practical and explain why the flagged construction is
  correct. Test-only generated code is the usual exception to the explanation requirement.
- Lint levels are repository policy and are set only in `Cargo.toml`'s `[lints]` tables — one
  diffable authority. A crate-root `#![allow]` or `#![deny]` does not change policy.
- Prefer `#[expect(lint, reason = "…")]` to `#[allow]`. An expectation that stops firing becomes a
  warning, so a suppression that outlives its cause removes itself from review instead of
  surviving unnoticed. That self-retirement needs a leg that both compiles the annotated region
  and promotes warnings to errors. §11 governs which legs those are for platform-gated code; an
  expectation in a region no such leg compiles does not operate at all — it suppresses nothing and
  cannot become the warning that retires it.
- Add targeted lints only after the repository is clean under them and the lint is available on
  the MSRV. Do not enable Clippy's `pedantic`, `nursery`, or `restriction` groups wholesale;
  those groups intentionally contain contextual, experimental, or mutually incompatible lints.
- Do not rewrite clear code merely to satisfy a stylistic lint. Configure or narrowly suppress
  the lint, with rationale, when its premise does not hold.
- The `clippy.toml` denylist is effect-funnel enforcement rather than style. Every
  denied path MUST resolve under a Clippy CI leg that compiles the platform where the symbol
  exists. An unresolved denial enforces nothing; Clippy reports it only as a bare configuration
  warning, and `-D warnings` does not promote that warning.
- A denylist MUST have a resolution census run by a named gate. The census links the probe against
  every dependency needed to resolve its paths, enumerates the declared platform exceptions, and
  injects a misspelled control that it must detect. These requirements attach to `disallowed-*`
  entries, and they are active: `clippy.toml` carries 102 denied paths — 95 methods, 3 types
  and 4 macros — beside the `allow-*-in-tests` booleans, and the census is
  `effects::tests` in `src/effects/tests.rs`, which resolves every denied path
  against a linked probe and injects a misspelled control it must detect.

## 3. Rust-native design principles

These principles are the project's Rust-native counterpart to applying SOLID mechanically. They
preserve SOLID's useful goals while using ownership, algebraic data types, traits, and modules as
Rust intends.

| Goal | upstroke rule |
|---|---|
| One reason to change | Keep each type, function, and module responsible for one coherent policy or operation. Split mixed policy/effect code, not files by arbitrary line count. |
| Safe extension | Model a real extension axis with an enum, generic, or trait. Do not add indirection for hypothetical implementations. |
| Substitutability | A trait's implementations MUST obey one documented behavioural contract, including error, cancellation, and side-effect semantics. |
| Small interfaces | Expose the least capability a caller needs. Prefer focused traits and private fields to broad service objects. |
| Dependency direction | Keep policy dependent on values and narrow capabilities; place filesystem, process, clock, and platform effects at explicit boundaries. |

The following rules cut across every section:

1. **Make invalid states unrepresentable.** Validate at boundaries, then carry validated types.
2. **Treat ownership as architecture.** Ownership communicates lifetime, mutation authority, and
   which task or component is responsible for cleanup.
3. **Treat errors as part of the API.** Callers must be able to distinguish every outcome that
   changes their next action.
4. **Keep effects at explicit boundaries.** Pure decision logic should not discover its own
   filesystem, process, clock, environment, or network dependencies.
5. **Prefer narrow capabilities to framework-shaped interfaces.** A trait earns its place through
   a real behavioural boundary, not because every concrete type is expected to have an interface.
6. **Use one authoritative state-transition path.** In particular, events—not side state—drive
   run state and replay.
7. **Make correctness independent of scheduling.** Concurrent results must follow a defined
   protocol, not timing luck.
8. **Make abstraction pay rent.** Abstract to protect an invariant, express a genuine family of
   behaviour, or remove proven duplication. Duplication is often cheaper than a false unification.
   That is not permission to implement one design clause twice: every implementation claiming to
   satisfy the same clause MUST be counted, and two is a finding even when both appear correct.
   Once one authority is chosen, a source census with an injected positive control MUST pin it
   as the only production implementation.

### Ambient authority

Wall-clock time, monotonic time, environment variables, and randomness are effects under rule 4.
Production reads of them live in a deliberately small set of boundary modules, and decision logic
receives values or injected capabilities instead of asking the machine. A change that adds a read
site outside the existing set MUST say why the funnel cannot serve it; the `clippy.toml` denylist
pins the funnel, with its platform legs and census in place.

Deadlines, timeouts, and elapsed measurements use `Instant`. `SystemTime` appears only where a
recorded timestamp or a minted identifier needs wall-clock meaning. The two are never compared,
interchanged, or converted into one another.

## 4. Formatting, naming, and readability

`rustfmt` is the formatting authority. Run it rather than hand-aligning code or debating local
style. A `#[rustfmt::skip]` or generated-file exclusion MUST be limited to syntax that rustfmt
cannot represent usefully and MUST say why.

No `rustfmt.toml` or `.rustfmt.toml` exists at adoption, so default rustfmt is the authority.
Adding either file changes this standard and MUST update this document in the same change.

Names MUST follow the Rust API Guidelines and standard casing:

- types and traits use `UpperCamelCase`; functions, variables, and modules use `snake_case`;
  constants use `SCREAMING_SNAKE_CASE`;
- conversions use `as_`, `to_`, and `into_` according to borrowing, cost, and ownership;
- simple accessors use the field or concept name rather than a `get_` prefix;
- predicates read as predicates (`is_`, `has_`, `can_`, or `should_`);
- units and representation MUST be explicit in a type or, where a primitive is unavoidable, in
  the name (`timeout_ms`, not `timeout`). Prefer `Duration` and domain newtypes to unit suffixes.

Code SHOULD read in domain terms. Avoid compressed names outside small conventional scopes, dense
iterator chains that hide control flow, and clever expressions that make errors or mutation hard
to see. Extract a named operation when the name explains policy; do not extract one-line wrappers
that merely force the reader to jump elsewhere.

Comments explain **why**, an invariant, a safety argument, or a non-obvious platform constraint.
They do not narrate syntax. Stale comments are defects and MUST be updated or removed with the
code they describe.

## 5. Types, modules, and APIs

### Representing state

- Parse and validate untrusted or weakly typed input once. Internal code SHOULD consume validated
  enums, newtypes, and structs rather than rechecking strings and integers.
- Use an enum when alternatives have different meaning or behaviour. Do not use string tags or a
  collection of booleans to encode a state machine.
- Avoid boolean parameters whose call sites do not explain themselves. Use a named enum or options
  struct when the value selects behaviour.
- Keep fields private when construction or mutation must preserve an invariant. Provide validated
  constructors and operations; do not expose a representation and ask callers to behave.
- Use `Option<T>` for legitimate absence and `Result<T, E>` for failure. Use
  `Result<Option<T>, E>` when both are possible; never collapse failure into absence.
- Do not encode domain identifiers, paths, timestamps, durations, or capacity quantities as
  interchangeable strings or numbers when a dedicated type can prevent a mix-up.
- Use checked conversions for narrowing or signedness changes. An `as` cast is acceptable only
  when it is lossless by type or a nearby invariant proves the range. Arithmetic MUST choose
  checked, saturating, wrapping, or failing behaviour deliberately; floating-point input MUST
  reject non-finite values before it is ordered, budgeted, or persisted. Release builds keep
  `overflow-checks = true`, so an overflow nobody chose fails loudly instead of wrapping.
- Avoid wildcard arms over closed domain enums when a new variant should force a decision at each
  call site. Wildcards remain appropriate for intentionally open external inputs.

### API shape and visibility

- Default to private. Use `pub(crate)` for crate-internal collaboration and `pub` only for a
  supported external contract.
- A function SHOULD perform one coherent operation at one abstraction level. Many parameters,
  especially repeated booleans or optional values, indicate that a request/configuration type may
  better express the contract.
- Borrow inputs (`&str`, `&Path`, slices, references) when the callee does not retain ownership.
  Accept or return owned values when data is stored, transferred to another task, or deliberately
  detached from the input lifetime.
- Return useful values rather than mutating caller-provided output parameters.
- Implement common traits (`Debug`, `Clone`, `Eq`, `Hash`, iterators, and conversions) when their
  semantics are honest and useful. `From` is infallible; validation belongs in `TryFrom` or a
  named fallible constructor.
- New or changed public APIs MUST be assessed for SemVer impact — the crate is published, so this
  is a live contract. `cargo semver-checks` becomes the named mechanism when a release-workflow
  leg runs it; the assessment is review-only until then. Public types SHOULD remain open to
  compatible evolution only when that flexibility is deliberate; do not add `#[non_exhaustive]`
  reflexively.

### Traits and abstraction

Introduce a trait when at least one of these is true: multiple implementations exist or are part
of the accepted design; callers are generic over a meaningful behavioural contract; runtime
heterogeneity is required; or the trait isolates a real effect boundary. Do not introduce a trait
solely to mirror an object-oriented class/interface pair.

Traits MUST be cohesive, object-safe only when trait objects are needed, and explicit about
observable semantics. Prefer generics for static composition and trait objects for genuine runtime
selection. Avoid public blanket implementations that foreclose future implementations.

Before creating a shared abstraction, compare the full invariants—not just similar syntax. If two
callers differ in atomicity, ownership, error handling, ordering, or durability, they are not yet
the same operation.

## 6. Ownership, mutation, and resources

- Each mutable resource MUST have an identifiable owner. Mutation should occur through the
  narrowest API that can preserve its invariants.
- Prefer immutable values and explicit state transitions. Interior mutability, global state, and
  shared mutable ownership require a concrete lifecycle or concurrency reason.
- Do not add `clone()` merely to silence a borrow-checker problem. A clone is appropriate when an
  owned snapshot, task transfer, or small independent value is the intended semantics; expensive
  or non-obvious clones SHOULD be evident at the boundary or explained.
- Use RAII for files, locks, temporary directories, child processes, and other resources. Cleanup
  MUST also occur on early return, error, panic unwinding, and cancellation to the extent the
  platform permits.
- Scope guards and resource-owning types are preferable to paired `start`/`finish` calls whose
  second half can be skipped.
- Do not leak references or guards across layers merely to avoid defining ownership. Return an
  owned result or a purpose-built guard whose lifetime expresses the actual protocol.

## 7. Errors and panics

Library modules MUST return typed errors, normally derived with `thiserror`. `anyhow` is limited to
the binary/application edge, where the program adds user-facing operational context and decides
how to report or exit.

Error types and handling MUST follow these rules:

- Define variants around decisions a caller can make, not one variant for every line that can
  fail. Preserve the source error where it helps diagnosis.
- Error `Display` text starts lowercase, carries no trailing period, and does not repeat its
  source's message: report chains join fragments with `": "`.
- Add operation, path, task, run, or adapter context at the layer that knows it. Do not include
  secrets, tokens, full sensitive prompts, or other values that must not enter logs.
- Inspect structured error kinds. Only an actual not-found condition becomes absence; permission,
  corruption, malformed data, and transient I/O remain errors.
- Do not discard an error through `.ok()`, `let _ =`, `unwrap_or_default()`, or a catch-all match
  unless the operation is explicitly best-effort. Best-effort behaviour MUST define its
  observability and have a test for the failure path.
- Retries MUST be bounded, classify retryable failures, respect cancellation, and avoid repeating
  a non-idempotent effect without a protocol that makes it safe.

Production code MUST NOT call `.unwrap()` or `.expect()`. It also MUST NOT use `panic!`, `todo!`,
`unimplemented!`, potentially panicking indexing, or assertions to handle input, configuration,
persisted state, I/O, subprocess behaviour, or scheduling outcomes. Prefer exhaustive matching
and types that make an impossible branch impossible.

The panic policy is machine-enforced: `[lints]` denies `clippy::unwrap_used`, `expect_used`,
`panic`, `todo`, `unimplemented`, and `dbg_macro` on every target the Clippy leg compiles.
`.unwrap()` has no test allowance; the other lints carve tests out through `clippy.toml`'s
`allow-*-in-tests` booleans.

An assertion, an `unreachable!`, or an `.expect()` under `#[expect(clippy::expect_used, reason)]`
for a true internal invariant is exceptional: the invariant and proof MUST be local and
documented, termination must be the intended response to a program defect, and tests MUST cover
the surrounding boundary. Tests fail their own setup with `.expect(` and a message naming the
failed premise; `.unwrap()` stays denied in tests too.

The panic strategy stays `unwind` in every profile: §6's cleanup guarantees depend on RAII running
during unwinding, so `panic = "abort"` is a change to this standard, not a build setting.

## 8. Filesystems, persistence, and paths

- Represent paths with `Path`, `PathBuf`, `OsStr`, or `OsString`. Do not construct paths by string
  concatenation or assume UTF-8. A lossy display string is for diagnostics only, never identity.
- Define whether each write is replaceable, create-once, append-only, atomic, and/or durable.
  These are separate guarantees. A successful rename is not automatically a durability guarantee.
- Publish multi-step output through a unique staging path in the destination filesystem. Do not
  use a fixed temporary name where concurrent writers can collide or delete each other's work.
- Use an atomic primitive for exclusivity (`create_new`, a lock, or compare-and-swap as the design
  requires). A check followed by a write is not exclusive.
- Cleanup may remove only resources whose ownership this operation can prove. Never infer
  ownership from a shared filename alone.
- Treat on-disk data as untrusted, including data written by an older or interrupted version.
  Validate schema, bounds, and invariants before constructing domain state.
- Treat a persisted or inter-process representation as an explicit schema. Do not serialize an
  internal struct merely for convenience when private refactoring would then change stored data.
  Serde defaults, aliases, unknown fields, and enum tagging are compatibility decisions and need
  tests.
- Path containment checks MUST account for `..`, absolute paths, symlinks/reparse points, and
  platform-specific prefixes as appropriate to the security boundary. Lexical normalization alone
  does not prove filesystem containment.

For the event log, `DESIGN.md` §4 is absolute: every state transition is represented by an event,
and state is reconstructed by replay. Do not update shadow state and then emit an event. Event
schema changes MUST preserve or deliberately migrate supported historical runs, and interrupted or
truncated tails MUST have defined recovery behaviour.

## 9. Processes and external tools

Internally constructed commands MUST use `std::process::Command` (or the runner abstraction) with
separate program and argument values. Do not concatenate values into shell text. Where the product
contract deliberately accepts user-authored shell commands, such as gates, keep that text opaque;
never interpolate an untrusted path, task field, or model output into it.

Every subprocess integration MUST define and test:

- executable discovery and the error when it is absent;
- working directory and relevant environment inheritance or removal;
- timeout, cancellation, and descendant-process cleanup;
- exit-status interpretation **and** output interpretation—neither is universally sufficient;
- stdout/stderr size and encoding behaviour, including malformed or adversarial output;
- secret redaction from commands, events, diagnostics, and transcripts.

Stdout from an external tool is untrusted input even when the tool is official. Parse it into a
typed result and reject contradictory or ambiguous success. Platform-specific process behaviour
belongs behind a shared semantic contract, with native tests for each implementation.

The product invariants still apply: model interaction occurs only through official CLI
subprocesses, and the engine does not add an HTTP/model-API path.

## 10. Concurrency and async code

The codebase is synchronous at adoption. Requirements that specifically mention `.await` are N/A
until the v0.2 async scheduler lands; the ownership, arbitration, cancellation, ordering, and
bounded-work rules apply to current threads and subprocesses now.

Concurrent code MUST have a written protocol visible in types, API documentation, or an adjacent
comment. The protocol identifies the owner of shared state, the atomic/linearization point, valid
state transitions, winner and loser behaviour, and cleanup after failure or cancellation.

- Correctness MUST NOT depend on task start order, sleeps, or one worker usually finishing first.
- Do not use check-then-act against shared state. Use the atomic filesystem, lock, channel, or CAS
  operation that actually arbitrates the transition.
- Prefer message passing or single-owner coordination over shared mutable state. When a lock is
  necessary, keep its protected invariant and acquisition order explicit and its critical section
  small.
- Never hold a synchronous lock guard across `.await`. Blocking filesystem or process work MUST
  not occupy an async executor thread without an explicit blocking boundary.
- Bound queues, fan-out, retries, and spawned work. Backpressure and overload behaviour are part of
  the API, not tuning details.
- Join every spawned thread's `JoinHandle` and turn a worker panic into a defined outcome. A
  detached thread whose failure nobody observes is the worker that dies without a report. The
  same rule applies to task handles when the async scheduler lands.
- Cancellation MUST leave no published partial result, orphaned child process, leaked worktree, or
  permanently claimed capacity. If an operation is not cancellation-safe, its API MUST make that
  constraint explicit and shield the critical region.
- Externally observable ordering MUST be deterministic or explicitly documented as unordered.
- Prefer deriving `Send` and `Sync` from ordinary safe fields. Manual unsafe implementations
  require the same proof discipline as any other unsafe code.

Concurrency tests SHOULD force the disputed interleaving with barriers, channels, or controlled
fakes. A stress loop or a sleep may supplement such a test but cannot be its only oracle. When the
v0.2 scheduler lands, interleaving-sensitive protocols SHOULD also run under a model checker such
as Loom where its modelled primitives reach the code; until a named repository gate exists, that
is a triggered review requirement, exactly as with Miri in §11.

## 11. Unsafe and platform-specific code

Use safe Rust first. Unsafe code is permitted only where required by an OS/FFI boundary or where a
measured need cannot be met safely.

Every unsafe operation MUST:

1. be confined to the smallest practical module and block;
2. have an immediately adjacent `SAFETY:` comment stating every obligation and why it holds;
3. validate raw pointers, lengths, initialization, aliasing, ownership, and lifetime assumptions
   as applicable;
4. be wrapped in a safe API that cannot be called without satisfying the remaining preconditions;
5. use explicit unsafe blocks inside an `unsafe fn` (`unsafe_op_in_unsafe_fn` discipline);
6. have focused native tests on each supported platform where the code is active.

New unsafe code that Miri can reach SHOULD be exercised under Miri. Sanitizers SHOULD be used
where a configured platform leg can exercise the affected boundary. Until either tool has a named
repository gate, its use is a triggered review requirement rather than an automated compliance
claim.

`clippy::undocumented_unsafe_blocks` is the intended mechanism for item 2. The tree does not yet
satisfy it everywhere, so per the §2 ratchet it turns on in the commit that closes that gap —
production and tests alike.

Windows, macOS, and Linux are supported targets. Cross-platform code MUST use platform-neutral
types and semantics; `cfg` modules isolate genuine OS differences. Do not make Unix path, signal,
permission, executable-suffix, process-tree, rename, or locking assumptions in shared code.
Platform code is not verified by compiling it on another OS alone: behaviour needs native CI
coverage.

Evidence is platform-gated in the same way the code is. A test, a lint attribute, or a suppression
that covers a `cfg` region is evidenced only by a leg that compiles that region, and each kind of
claim needs the leg that evaluates it:

- A **Clippy** lint attribute is evidenced only by a Clippy leg for that platform. A native test
  leg and a native MSRV `check` leg do compile the region, but they do not run Clippy over it, so
  they pass while a Clippy leg for the same platform fails.
- A **rustc** lint attribute is not so limited: any leg that compiles the region evaluates it, so a
  denied rustc lint does fail a native `test` or `check` leg on that platform. Its expectation
  needs one thing more, because `unfulfilled_lint_expectations` is warn-by-default and so retires
  a suppression only where warnings are promoted to errors. `ci.yml` sets `RUSTFLAGS: -D warnings`
  at workflow scope, so today that is every leg — which means narrowing it to a single job would
  silently take the self-retirement guarantee with it.
- An `#[expect]` inside a region that no such leg compiles is inert in both directions. It
  suppresses nothing, and it cannot become the warning that retires it. It reads as enforcement
  and is not.
- Cross-compilation is evidence, not a native run, and it carries its own blind spot: a path that
  resolves for the host may not resolve for the target, which Clippy reports as a bare
  configuration warning that `-D warnings` does not promote.

A change that adds platform-gated code, tests, or annotations MUST name the leg that evaluates
them. Every supported target has one today: `ci.yml` runs a Clippy leg on each of the three —
`lint`, `lint (windows)` and `lint (macos)` — beside `test` and `msrv` matrices that compile all
three natively, and the `upstroke-ci` aggregate fails unless every one of them succeeds. Adding a
platform, or gating code to a target no leg covers, leaves a claim unevidenced; that is recorded
in Appendix A as an uncovered platform rather than left to be inferred from a green baseline.

## 12. Tests

Tests are executable evidence of a contract. New behaviour and bug fixes MUST include tests at the
lowest layer that can observe the real failure, plus a higher-level test when composition is the
risk.

A sufficient test set covers, as applicable:

- the ordinary success path;
- invalid, missing, malformed, and boundary input;
- each error category that changes caller behaviour;
- interrupted writes, partial external output, failed cleanup, retry, and resume;
- platform-specific semantics;
- concurrent winner/loser and cancellation paths;
- compatibility with persisted formats and public API contracts.

Tests MUST be deterministic and hermetic by default:

- use unique temporary directories with RAII cleanup;
- inject or control clocks, randomness, environment, capacity observations, and process responses;
- do not depend on network access, ambient credentials, user configuration, PATH contents, test
  execution order, or an installed vendor CLI unless the test is explicitly an integration test;
- do not use sleeping as synchronization; signal the state the test needs to observe, under the
  readiness rules below;
- do not silently return success when a prerequisite is absent. Either provide the dependency,
  classify the test outside the default suite, or fail with a useful diagnostic;
- state the reason in the attribute when a test is disabled (`#[ignore = "…"]`); a bare
  `#[ignore]` is a parking space, not a classification;
- assert externally meaningful state and side effects, not an implementation copied into the
  test as its own oracle.

### Readiness protocols

A readiness signal is a claim about state, and a waiter is entitled to assert everything the claim
names. The producer's ordering is what makes that claim true, so these rules bind the helper as
much as the test:

- A readiness signal MUST be published only after the state it announces is complete and
  observable by the waiter. Publish it last, not alongside the work it describes.
- **A file's existence is a readiness signal only if the file is published atomically.** Creation
  and content are otherwise separate events, so a waiter polling for a path that is created and
  then written can open it and read nothing. Two forms are sound: an empty marker created after
  the state it announces, where there is nothing to read; and a file staged elsewhere and moved
  into place by atomic rename under §8, where the name and its contents become visible together,
  so a waiter may await the path and then read it. What is unsound is a path created in place and
  written afterwards, because its existence is observable before the state it stands for.
- A partial record MUST NOT be readable as a whole one. A record delimited by a terminator — a
  newline on a pipe — is complete only once the terminator arrives; an unterminated final record
  is a truncated write and MUST fail rather than yield a short value.
- Keep the payload inside what the framing can carry. A path is not safely a line: an ancestor may
  contain the delimiter, or bytes that are not text at all. Send an identifier the receiver can
  rejoin to a root it already knows.
- The wait MUST be bounded, and the bound MUST bound a producer that has wedged rather than time
  one that is healthy. A deadline short enough to expire on a loaded runner has become the signal
  itself, which is the failure this rule exists to prevent. The fast path is a producer that fails
  and closes its channel; the bound is for the one that stays alive and silent.

### Flakes

A test that fails intermittently is a fact about the repository, and it is handled by measurement
rather than by adjective. "Occasionally" and "flaky under load" are not records.

When a test is observed failing without a change that explains it:

- **Measure a rate before naming it a flake.** Record a numerator over a denominator of observed
  runs — "one failure in 31 full-suite runs" — with the platform, the head, and the assertion
  that failed. One failure and no denominator is an observation, not a rate.
- **Establish provenance, and do not mistake nondeterminism for it.** Re-running the failed job at
  the same commit is cheap and worth doing, but one byte-identical tree producing both outcomes
  shows only that the failure is nondeterministic. It does not show where the nondeterminism came
  from: a change that introduces a race produces exactly that signature. Provenance needs one of
  two things — the same failure reproduced on a base or prior head that predates the change, or a
  causal argument, made from what the diff actually touches, that it cannot reach the behaviour.
  With neither, a newly observed intermittent failure is a candidate regression and is triaged as
  one. "It passed on re-run" MUST NOT be the reason a change merges.
- **Fingerprint an occurrence by platform and by the assertion or error it produced**, not by test
  name alone. One name can cover several causes, and one cause can surface under several names, so
  match on the failing assertion, together with the error code where the failure carries one — a
  panicked assertion has a message and a location and no code, and is fingerprinted by those.
- **Name an owner and state the consequence**, so a later red is triaged instead of re-diagnosed:
  which fingerprint, and that a failure matching it is this flake until proven otherwise. A red
  that does not match the recorded fingerprint is a regression until someone shows otherwise.
- **The classification is provisional, and a cause retires it.** A failure with an identified
  mechanism on a supported platform is a defect at whatever rate it occurs, triaged by repairing
  that mechanism rather than by re-running. A rate does not settle the category; it is what makes
  the category arguable.
- **Preserve the evidence a rate is made of.** A harness that overwrites one output path each run
  loses the auditable fingerprint and diagnostic provenance of every occurrence but the last,
  because the next run destroys the failure the previous one caught. A rate can still be measured
  if outcomes and diagnostics are retained separately — a numerator and denominator kept durably
  outside that path carry the arithmetic — but occurrences that can no longer be re-examined are
  not evidence that any of them matched the recorded fingerprint. Write per-run output: that is
  the preferred form, because it keeps each occurrence's outcome and diagnostic together. Quote
  the diagnostic — the message, and the error code when the failure has one — rather than the
  bare fact of a failure.

A red that recurs with no rate, no owner, and no stated consequence trains reviewers to discount
CI, which costs more than the test does.

### Instruments and censuses

Source-based enforcement is code and needs tests of its own:

- A census of Rust structure MUST blank comments and string, character, byte-string, and raw-string
  literals before counting. The blanker MUST preserve positions and output length. Every scan MUST
  assert a non-zero blanked-region count unless its declared domain intentionally contains none;
  the blanker's tests MUST prove this with a fixture containing removable regions. Prefer a parser
  or structural match to a substring search.
- Every census MUST assert the size and boundaries of the domain it claims and carry a positive
  control that injects one violation and observes the expected failure. A positive control inside
  a truncated domain does not prove that the whole named domain was scanned.
- Test-only items MUST follow every production item in a source file. A mid-file `#[cfg(test)]`
  boundary can silently truncate any instrument that defines production as the prefix before the
  first test item.
- After inserting an item, verify that the neighbouring doc comments and attributes still attach
  to their intended items. In particular, check `#[cfg]`, `#[test]`, lint attributes, derives, and
  rustdoc on both sides of the insertion.
- Derive a fixture's field list from the production type, not intuition. Vary every independently
  meaningful field independently and assert hostility with distinct-value counts or a complete
  written-out table; correlated values do not prove the field named by the test.
- Name each test as the sentence it proves, so a failure reads as the broken claim rather than as
  an implementation detail.

A periodic mutation pass (for example `cargo mutants`) over gates and instruments is the
systematic form of the injected-control discipline and SHOULD accompany changes to enforcement
code. It has no named repository gate at adoption.

A regression test MUST fail for the reported defect before the fix and pass after it. Prefer a
minimal reproducer that names the violated contract over a broad snapshot that happens to change.
Property tests and state-machine tests SHOULD be considered for parsers, replay, routing,
serialization, and concurrent protocols where example cases leave a large state space uncovered.
Parsers over untrusted input — the plan format, agent stdout — SHOULD additionally get fuzz
coverage once a fuzzing target exists in the repository; until then that is a triggered review
consideration.

Test code may trade some abstraction for clarity, but it must not weaken production invariants by
reaching through private state when the public behaviour can be exercised directly.

## 13. Documentation and observability

New or changed public items MUST have rustdoc that states their contract. Include `# Errors`,
`# Panics`, and `# Safety` sections when applicable. Examples SHOULD remain valid Rust and MAY be
doctests, but doctests are documentation in this repository: `cargo test --all-targets` excludes
them. Executable evidence MUST live in a unit, integration, or other run target executed by a
named CI command; a doctest alone does not satisfy a testing requirement.

Process output is part of the product surface. The `println!` and `eprintln!` macros are denied
(`clippy::print_stdout`, `print_stderr`) outside the named output modules — the CLI binary and the
terminal interaction module — each of which carries `#![expect]` stating its contract. Examples
print what they demonstrate and carry the same expectation.

A change to behaviour, configuration, events, persisted data, CLI output, or a supported platform
MUST update its user and design documentation in the same pull request. Do not leave a code comment
or review note as the only record of a new contract.

Events and diagnostics SHOULD make decisions reconstructable: identify the operation and stable
domain IDs, preserve causal errors, and distinguish retryable, parked, cancelled, and terminal
outcomes. They MUST NOT expose secrets. Logs are diagnostic evidence, not a second source of state.

## 14. Security and trust boundaries

Candidate repositories, plans, model output, external CLI output, configuration, environment
variables, and persisted run data cross trust boundaries. Code MUST validate them before granting
filesystem, process, git, capacity, or state-transition authority.

- Validation belongs at the effect boundary as well as any CLI/UI boundary; a caller cannot confer
  authority merely by constructing an internal-looking string.
- Bound input size, recursion, collection growth, output capture, concurrency, and retry work before
  allocating or spawning from untrusted values.
- Represent secrets so ordinary `Debug`, error, event, and serialization paths redact or omit them.
  Never place credentials in command-line arguments when a safer supported channel exists.
- Preserve least privilege between coordinator, worker, gate, and reviewer roles. A read-only role
  must not receive a write-capable handle and be trusted simply not to use it.
- Do not describe filtering, path checks, containers, or adapter deny rules as a sandbox unless the
  enforced OS boundary supports that claim. Security documentation MUST state residual authority.
- Security-sensitive comparisons and decisions MUST fail closed on malformed, contradictory, or
  unavailable evidence; availability fallbacks must not silently grant more authority.

## 15. Dependencies and features

A new dependency MUST have a concrete benefit over the standard library or an existing dependency.
The pull request MUST consider maintenance status, licence compatibility, security history,
transitive cost, binary-size/compile-time effect, MSRV, and target support in proportion to risk.

- Keep `Cargo.lock` committed and update only entries required by the dependency change.
- Avoid broad default features. Enable the smallest stable feature set that provides the needed
  behaviour, without creating a fragile hand-built substitute for a crate's supported setup.
- Feature flags MUST be additive and compile in every supported combination that CI claims. Do not
  use a feature to select mutually incompatible meanings for the same API.
- Dependency types SHOULD not leak through a public API unless that dependency is intentionally
  part of the public contract.
- CI workflow actions MUST be pinned to a full commit SHA with the version named in a comment; a
  tag is a mutable reference, not a pin.
- The crate builds without a build script. Introducing one, or adding a proc-macro dependency,
  names the new compile-time supply-chain surface in this section's assessment.
- A dependency-policy gate (`cargo deny`: advisories, licences, bans, sources) is the intended
  mechanism for this section's review duties. Its requirements activate in the same change that
  introduces its configuration, carrying the positive control Appendix A requires.
- No dependency may introduce direct model-API or engine HTTP behaviour contrary to `DESIGN.md`
  §4, even behind an optional feature.

## 16. Review checklist

Reviewers and authors should be able to answer yes to each applicable item:

- [ ] The change preserves all `DESIGN.md` §4 invariants and follows the current build order.
- [ ] Invalid states are rejected at the boundary or excluded by types.
- [ ] Ownership, side effects, and state-transition authority are unambiguous.
- [ ] Absence, failure, retry, cancellation, and terminal outcomes remain distinguishable.
- [ ] No production panic path handles data, environment, persistence, process, or scheduling.
- [ ] Filesystem publication and concurrent arbitration use the required atomic semantics.
- [ ] External commands check both process and protocol outcomes and clean up descendants.
- [ ] Platform assumptions are isolated and tested natively, and platform-gated code, tests, and
      annotations name the leg that evaluates them.
- [ ] Untrusted input is bounded and validated before it gains authority; secrets stay redacted.
- [ ] Tests force the important failure/interleaving and do not depend on ambient machine state.
- [ ] Readiness signals follow their state, cannot be read partially, and every wait is bounded.
- [ ] An intermittent failure carries a measured rate, established provenance, a fingerprint, an
      owner, and a re-run-or-repair rule.
- [ ] Source instruments scan their complete claimed domain and their injected controls fail.
- [ ] Public behaviour, persisted formats, events, and documentation change together.
- [ ] New abstraction and dependencies have a demonstrated purpose and do not widen capability.
- [ ] Every cited standard maps to a named mechanism or is explicitly review-only.
- [ ] Lint-level changes live only in `[lints]`, and new suppressions are `#[expect]` with a reason.
- [ ] Ambient time, environment, and randomness stay inside the named funnel modules.
- [ ] All eight §2 baseline commands pass from the repository root.

## 17. Upstream references

These official sources inform this standard where project rules are silent. They are guidance,
not an unversioned way to change the repository's contract:

- [Rust Style Guide](https://doc.rust-lang.org/style-guide/) — canonical formatting principles;
  rustfmt is its executable implementation.
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html) — naming,
  interoperability, documentation, predictability, and API evolution.
- [Clippy documentation](https://doc.rust-lang.org/stable/clippy/) and
  [lint-group policy](https://doc.rust-lang.org/stable/clippy/usage.html) — automated diagnostics
  and why allow-by-default groups are selected deliberately.
- [Cargo SemVer compatibility](https://doc.rust-lang.org/cargo/reference/semver.html) — what Cargo
  and the Rust project generally treat as compatible public API evolution.
- [The Rust Reference: unsafe](https://doc.rust-lang.org/reference/unsafe-keyword.html) and the
  [Rustonomicon](https://doc.rust-lang.org/nomicon/working-with-unsafe.html) — unsafe obligations
  and safe-abstraction boundaries.
- [The Rust Book: recoverable errors](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
  and [fearless concurrency](https://doc.rust-lang.org/book/ch16-00-concurrency.html) — the language's
  error and ownership model.

## Appendix A. Enforcement map

A rule is automated only on the paths and platforms a named mechanism actually examines. Compiler
assistance is not the same as enforcement, and a green gate is not evidence for review-only rules.

| Rule area | Mechanism | Enforcement status |
|---|---|---|
| §1 authority, precedence, and known conflicts | Pull-request review; `test-docs-consistency.sh` for the document relationships it explicitly enumerates | Review-only unless the named Bash fixture contains the claim |
| §2 formatting | `cargo fmt --check` / rustfmt | Automated on all formatted Rust inputs; default configuration at adoption |
| §2 compiler and ordinary lint baseline | `cargo clippy --all-targets --all-features -- -D warnings` | Automated for code compiled by the lint job |
| §2 effect denylist | `clippy.toml` disallowed methods, types, and macros; denylist resolution census with an injected typo control | **Active.** 102 denied paths, the census in `src/effects/tests.rs`, and Clippy legs on all three platforms — `lint`, `lint (windows)` and `lint (macos)`. A path this host cannot resolve is caught by the census, not by `-D warnings` |
| §§3–6 design, types, APIs, ownership, and resources | rustc ownership/type checking and targeted Clippy lints where applicable; tests; pull-request review | Partly automated; semantic and abstraction rules are review-only, and the ambient-authority funnel is review-enforced until denylist entries pin it |
| §7 errors and panics | `[lints]`-denied `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, and `dbg_macro`; type checking; tests | Panic policy automated on all three platforms' Clippy legs, with `#[expect]` marking each documented §7 exception — the platform-coverage row below names the legs; before `lint (macos)` landed, `#[cfg(target_os = "macos")]` code was compiled by no Clippy job at all; context quality and error taxonomy stay review-only |
| §§8–9 filesystem, persistence, and processes | behavioural tests; platform CI; the active effect denylist | Partly automated; atomicity, durability, ownership, parsing, and protocol completeness require review |
| §10 concurrency and async code | rustc `Send`/`Sync` and borrowing checks; deterministic concurrency tests | Partly automated; protocol, cancellation safety, bounds, and ordering require review. `.await`-specific rows are N/A until async production code lands |
| §11 unsafe and platform code | rustc unsafe checks, `unsafe_op_in_unsafe_fn` denied via `[lints]`, native tests; Miri or sanitizer only when a named leg exists; `undocumented_unsafe_blocks` pending its ratchet commit | Partly automated; safety proofs and tool triggers require review |
| §11 platform coverage of lint claims | A Clippy leg per supported platform — `lint`, `lint (windows)` and `lint (macos)` — beside the `test` and `msrv` matrices, which compile all three natively and, under the workflow-wide `RUSTFLAGS: -D warnings`, evidence rustc lints and their expectations on all three but never Clippy's; `upstroke-ci` aggregates every leg | **Covered on all three supported targets.** A Clippy lint or expectation in a `#[cfg(windows)]` or `#[cfg(target_os = "macos")]` region is evaluated on its own platform: `lint (windows)` evaluates the three `#[expect(clippy::expect_used)]` annotations in `src/agent/proc.rs`'s `windows_job`, and `lint (macos)` is what holds that file's `#[cfg(target_os = "macos")]` regions — the macOS arm of `create_cloexec_pipe` among them — to the denied panic lints. Each of the two added legs repaired a real hole: before them those regions were compiled by the `test` and `msrv` matrices but by no Clippy job, so the annotations suppressed nothing and could not self-retire, and a green baseline did not reveal it. Two effective `cfg` predicates are compiled by no CI runner, and `src/effects/tests.rs`'s `NO_CI_RUNNER_COMPILES` records exactly those two: `not(any(unix, windows))` and `all(unix, not(any(target_os = "linux", target_os = "macos")))`. Both are deliberate unsupported-target regions — else-arms that keep the crate compiling on a target family this project does not ship — so their bodies stay outside the denylist's and the panic lints' reach on every job CI runs. That is disclosure rather than coverage, and it is held exact: the structural `cfg` census asserts that the set of effective predicates no runner compiles equals this recorded set, so a newly uncovered region fails the census instead of passing unnoticed. A region gated to a target no Clippy leg covers is uncovered and is recorded as such here |
| §12 tests, instruments, and censuses | `cargo test --all-targets --all-features`; each instrument's positive control and named test or Bash gate | Execution is automated; sufficiency, independence, complete domains, oracle quality, `#[ignore]` reasons, and the periodic mutation pass require review |
| §12 readiness protocols and flake records | Pull-request review; the standing finding ledger `reviews/FINDINGS.md` for a carried flake's rate, owner, and consequence, and a dated `reviews/` record for a single unexplained failure | Review-only, and not automatable as stated. No gate detects a readiness signal published before the state it announces — the test passes whenever the race is won — and no single CI attempt can establish a failure rate, which is measured across repeated runs of one head |
| §13 documentation | `test-docs-consistency.sh` for its enumerated contracts; rustdoc/compiler where a compiled example target exists; `print_stdout`/`print_stderr` deny output outside the named modules | Otherwise review-only; doctests are not run by the baseline |
| §14 security and trust boundaries | behavioural/security tests; the active effect denylist; pull-request review | Review-only where no named test or denial is cited |
| §15 dependencies and features | locked MSRV check, all-feature compiler/test gates, and dependency diff review; SHA-pinned actions (review-verified); `cargo deny` once its configuration lands | Compatibility is partly automated; maintenance, licence, security, and capability assessment are review-only |
| Contribution, PR, and release policy | `test-pr-policy.sh`, `test-pr-ledger-evidence.sh`, and `test-release-record.sh` | Automated by the lint job |

Application rules:

- A review finding that cites this standard MUST identify the applicable map row and either the
  named enforcement mechanism or `review-only`. Absence of a mechanism never means compliant.
- An enforcement artifact MUST be exercised by a named command that CI runs and MUST carry a
  positive control proving that the command detects a violation in the claimed domain.
- A lint is enabled in the same commit that makes the complete claimed tree clean under it. This
  ratchet applies equally to a new lint, a widened lint scope, and a newly compiled platform.
- A mechanism enforces only the files, targets, features, and platforms it actually evaluates.
  Cross-compilation, an MSRV `check`, and a native test leg do not substitute for a platform Clippy
  leg when the claim is a rustc-HIR-resolved denial.
