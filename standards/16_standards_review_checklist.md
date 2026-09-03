## 16. Review checklist

A reviewer and an author should be able to answer yes to each applicable item:

- [ ] Preserves every `DESIGN.md` §4 invariant and fits the §21 build order.
- [ ] Invalid states are rejected at the boundary or excluded by types.
- [ ] Ownership, side effects and state-transition authority are unambiguous; no `Rc`, `Arc`,
      `Mutex` or `clone()` without a stated reason (§6).
- [ ] Absence, failure, retry, cancellation and terminal outcomes stay distinguishable; every `?`
      propagates an error the caller can act on as it is (§7).
- [ ] No production panic path handles data, environment, persistence, process or scheduling.
- [ ] Filesystem publication and concurrent arbitration use atomic primitives.
- [ ] External commands check both process and protocol outcomes and clean up descendants.
- [ ] Platform assumptions are isolated, tested natively, and name the CI leg that evaluates them.
- [ ] Untrusted input is bounded and validated before it gains authority; secrets stay redacted.
- [ ] Tests force the important failure or interleaving and depend on no ambient machine state;
      readiness signals follow their state; every wait is bounded.
- [ ] An intermittent failure carries a rate, provenance, fingerprint, owner and consequence.
- [ ] Source instruments scan their whole claimed domain and their injected controls fail.
- [ ] Behaviour, persisted formats, events and documentation change together.
- [ ] New abstraction and dependencies have a demonstrated purpose.
- [ ] Lint levels change only in `[lints]`; new suppressions are `#[expect]` with a reason.
- [ ] Ambient time, environment and randomness stay inside the funnel modules.
- [ ] All eight §2 commands pass from the repository root.

A finding that cites a standard names the section and says whether a mechanism or review enforces
it. A §6 or §7 finding in an unswept file is in scope only under the activation rule in
`standards/SWEEP.md`.
