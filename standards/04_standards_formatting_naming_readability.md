## 4. Formatting, naming, and readability

`rustfmt` with its default configuration is the formatting authority. There is no `rustfmt.toml`,
and adding one changes this standard. A `#[rustfmt::skip]` is limited to syntax rustfmt cannot
represent usefully and says why.

Names follow the Rust API Guidelines: `UpperCamelCase` types and traits; `snake_case` functions,
variables and modules; `SCREAMING_SNAKE_CASE` constants; `as_`/`to_`/`into_` by borrowing, cost and
ownership; accessors without `get_`; predicates that read as predicates (`is_`, `has_`, `can_`,
`should_`). Units are explicit in a type — prefer `Duration` and domain newtypes — or, where a
primitive is unavoidable, in the name (`timeout_ms`).

Code reads in domain terms. Avoid compressed names outside small conventional scopes, iterator
chains that hide control flow, and clever expressions that hide errors or mutation. Extract a named
operation when the name explains a policy; do not extract one-line wrappers that only force a jump.

Comments explain why, an invariant, a safety argument or a platform constraint; they do not narrate
syntax. A stale comment is a defect and changes with the code it describes.

Enforced by: `cargo fmt --check`; review for naming and readability.
