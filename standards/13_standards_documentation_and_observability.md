## 13. Documentation and observability

New or changed public items have their contract written down, with `# Errors`, `# Panics` and
`# Safety` where they apply. Rustdoc beside the item is the default place, and for most of the tree
it is the only one. Doctests are documentation here: `cargo test --all-targets` does not run them,
so executable evidence lives in a unit or integration test that a named CI command runs.

A module with an internal notes file writes them there instead. `docs/internals/` mirrors `src/`,
one notes file per module; where a module has one, that file carries the whole of the module's
prose — contracts, rationale, history, worked examples — and the source carries a single
`Extended notes:` pointer in its module header and no other comment. The obligation is that the
contract is written and can be found, not where it sits. This exists because the prose is dense
enough to fill a reader's attention before they reach the code, and a reader who wants the code
should not have to pay for the prose first; `docs/internals/README.md` states which material
belongs there and which stays at its site. Reasoning another standard requires *at* a site — a
`SAFETY:` obligation (§11), a concurrency protocol (§10) — is that standard's to place, and a
module with a notes file is not excused from it.

Process output is product surface. `println!` and `eprintln!` are denied (`clippy::print_stdout`,
`print_stderr`) outside the named output modules — the CLI binary and the terminal interaction
module — each of which carries a crate-level `#![expect]` stating its contract; examples carry the
same expectation.

A change to behaviour, configuration, events, persisted data, CLI output or a supported platform
updates the design (`design/`) and any user documentation in the same pull request. A code comment
or review note is never the only record of a contract.

Events and diagnostics make decisions reconstructable: they identify the operation and stable domain
ids, preserve causal errors, and distinguish retryable, parked, cancelled and terminal outcomes. They
never expose secrets. Logs are diagnostic evidence, not a second source of state.

Enforced by: the two print lints; `.github/scripts/test-internals-notes.sh` for the pointer and
the notes file it names; review for the contracts themselves and same-change documentation.
