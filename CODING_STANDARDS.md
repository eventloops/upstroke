# Rust coding standards

The normative implementation standard for Rust in upstroke: production code, tests, examples and
build support. Official Rust guidance is the foundation; the product's invariants, failure modes and
supported platforms set the stricter rules. Reworked and slimmed on 2026-09-03.

This file is the index. Each standard lives in `standards/`, one numbered section per file.
Section numbers are the API: code comments, `effects/allowlist.toml` and review records cite
`CODING_STANDARDS.md §N`, so a number is never reassigned. New standards append; a retired one
keeps its number and says so. Each file ends with the mechanism that enforces it, or `review`.

| § | Standard | File |
|---|---|---|
| 1 | Authority and scope | [standards/01_standards_authority_and_scope.md](standards/01_standards_authority_and_scope.md) |
| 2 | Automated baseline | [standards/02_standards_automated_baseline.md](standards/02_standards_automated_baseline.md) |
| 3 | Rust-native design principles | [standards/03_standards_design_principles.md](standards/03_standards_design_principles.md) |
| 4 | Formatting, naming, and readability | [standards/04_standards_formatting_naming_readability.md](standards/04_standards_formatting_naming_readability.md) |
| 5 | Types, modules, and APIs | [standards/05_standards_types_modules_apis.md](standards/05_standards_types_modules_apis.md) |
| 6 | Ownership, mutation, and resources | [standards/06_standards_ownership_mutation_resources.md](standards/06_standards_ownership_mutation_resources.md) |
| 7 | Errors and panics | [standards/07_standards_errors_and_panics.md](standards/07_standards_errors_and_panics.md) |
| 8 | Filesystems, persistence, and paths | [standards/08_standards_filesystems_persistence_paths.md](standards/08_standards_filesystems_persistence_paths.md) |
| 9 | Processes and external tools | [standards/09_standards_processes_and_external_tools.md](standards/09_standards_processes_and_external_tools.md) |
| 10 | Concurrency and async code | [standards/10_standards_concurrency.md](standards/10_standards_concurrency.md) |
| 11 | Unsafe and platform-specific code | [standards/11_standards_unsafe_and_platform_code.md](standards/11_standards_unsafe_and_platform_code.md) |
| 12 | Tests | [standards/12_standards_tests.md](standards/12_standards_tests.md) |
| 13 | Documentation and observability | [standards/13_standards_documentation_and_observability.md](standards/13_standards_documentation_and_observability.md) |
| 14 | Security and trust boundaries | [standards/14_standards_security_and_trust_boundaries.md](standards/14_standards_security_and_trust_boundaries.md) |
| 15 | Dependencies and features | [standards/15_standards_dependencies_and_features.md](standards/15_standards_dependencies_and_features.md) |
| 16 | Review checklist | [standards/16_standards_review_checklist.md](standards/16_standards_review_checklist.md) |
| 17 | Upstream references | [standards/17_standards_upstream_references.md](standards/17_standards_upstream_references.md) |

[standards/SWEEP.md](standards/SWEEP.md) tracks the file-by-file cleanup under the §6 and §7
rules tightened on 2026-09-03; until a file is listed there, those two sections bind only the code
a change adds or rewrites, under the activation rule that file states.

The hard requirements most changes meet head-on: edition 2024 with MSRV 1.85; no `.unwrap()` or
`.expect()` in production and no `.unwrap()` in tests (§7, lint-enforced); `anyhow` only at the
binary edge (§7); every path through `std::path` (§8); no shared ownership, locks, or non-trivial
clones without a stated reason (§6); every `?` deliberate (§7); ambient time, environment and randomness only in
the funnel modules (§3, denylist-enforced).
