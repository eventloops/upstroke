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
