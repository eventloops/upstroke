## 13. Documentation and observability

New or changed public items have rustdoc stating their contract, with `# Errors`, `# Panics` and
`# Safety` sections where they apply. Doctests are documentation here: `cargo test --all-targets`
does not run them, so executable evidence lives in a unit or integration test that a named CI
command runs.

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

Enforced by: the two print lints; review for rustdoc and same-change documentation.
