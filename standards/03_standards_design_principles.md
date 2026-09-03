## 3. Rust-native design principles

Use ownership, enums, traits and modules the way Rust intends rather than applying SOLID
mechanically:

- **One reason to change.** A type, function or module owns one coherent policy or operation.
  Split mixed policy/effect code; do not split files by line count alone.
- **Extend along a real axis.** An enum, generic or trait models an extension point that exists.
  No indirection for hypothetical implementations.
- **Substitutability.** A trait's implementations obey one documented contract, including error,
  cancellation and side-effect semantics.
- **Small interfaces.** Expose the least capability a caller needs.
- **Dependency direction.** Policy depends on values and narrow capabilities; filesystem, process,
  clock, environment and platform effects sit at explicit boundaries.

Cross-cutting rules:

1. Make invalid states unrepresentable: validate at the boundary, then carry validated types.
2. Ownership is architecture: it says who may mutate, how long a value lives and who cleans up.
3. Errors are API: a caller must be able to tell apart every outcome that changes its next action.
4. Effects stay at boundaries: decision logic does not discover its own filesystem, process, clock,
   environment or network.
5. Events, not side state, drive run state and replay; there is one transition path.
6. Correctness never depends on scheduling luck.
7. Abstraction pays rent: abstract to protect an invariant, express a real family of behaviour, or
   remove proven duplication. Duplication is often cheaper than a false unification. That is not
   licence to implement one design clause twice: one production implementation per clause, pinned
   by a source census with an injected control where the clause matters.

**Ambient authority.** Wall-clock time, monotonic time, environment variables and randomness are
effects. Production reads of them live in a small set of funnel modules pinned by the `clippy.toml`
denylist; decision logic receives values or injected capabilities. A new read site outside the
funnel says why the funnel cannot serve it. Deadlines and elapsed measurements use `Instant`;
`SystemTime` appears only where a recorded timestamp or minted identifier needs wall-clock meaning;
the two are never compared or converted.

Enforced by: the denylist and effects census for the funnel; review for the rest.
