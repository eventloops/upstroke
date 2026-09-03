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
