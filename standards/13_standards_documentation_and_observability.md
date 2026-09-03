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
