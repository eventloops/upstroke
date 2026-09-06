---
id: PR157-ASTRA-SITE-SAFETY-RECOVERY-R4-001
severity: P2
disposition: deferred
category: correctness
pr: 197
reviewed_sha: cbd42faa60df93807602a2e8100f7624193b553a
location: src/runner/host/tests.rs:6062
provenance: fix_regression
first_bad: cbd42faa60df93807602a2e8100f7624193b553a
guard: the PR #197 branch `codex/findings-2b802b011b29` if it is resumed, or the change that next opens `every_site_obligation_is_complete_and_agrees_with_its_notes_copy`; escalate if the census is merged with this open
---

`every_site_obligation_is_complete_and_agrees_with_its_notes_copy` classifies a keyword token by
the terminal name of the macros around it: inside a name on `INERT_MACROS` (`stringify`,
`concat`, `include_str` and their kin) the token is prose and is skipped; inside a name on
`EXPRESSION_MACROS` it is code; inside any other name it is refused. The lists are matched by
spelling alone. Rust lets a local `macro_rules!` definition, an import or an alias shadow a
built-in name, so a macro spelled `stringify` need not be the built-in and may expand its input.

## Failure sequence

At `cbd42faa60df93807602a2e8100f7624193b553a`, append to `src/runner/host/tests.rs` a local
forwarding macro named `stringify` and a test that calls
`stringify!(unsafe { std::ptr::read_volatile(&value) })`. The local definition shadows the
built-in, so the call expands and executes the unsafe expression, and no `SAFETY:` comment sits
above it. `macro_invocations` records the enclosing name `stringify`; `site_obligations` finds it
on `INERT_MACROS` and skips the token, returning the unchanged obligation set. A real unannotated
operation evades the census: a false negative, the dangerous direction, in code the pull request
adds. Reported by recovery review pass 4 of 4
(https://github.com/sourcemaps/upstroke/pull/197#issuecomment-5562382675); the controls and
mutation arms at that head exercise the built-in `stringify!` only, not macro resolution or
shadowing, so none protects this path.

## What the change that takes this up should do

Do not infer a macro's semantics from its terminal name. Either fail closed when an allowlisted
built-in name is shadowed, by refusing the module when a `macro_rules!` definition, a `use ... as`
or an import binds any name on either list (the census already finds `macro_rules!` token trees,
so the definition is visible to it), or replace the name-based classification with an explicitly
anchored census: each site declared by its exact statement line in the notes file under its
`SAFETY:` paragraph, every keyword token in code position required to be one of the declared
lines, and the domain boundary asserted as that equality. Add an executable control in the census
itself with a shadowed inert-macro name forwarding a real unannotated unsafe expression, and
require the census to reject it, while keeping the existing built-in `stringify!` control green.
