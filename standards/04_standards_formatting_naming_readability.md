## 4. Formatting, naming, and readability

`rustfmt` is the formatting authority. Run it rather than hand-aligning code or debating local
style. A `#[rustfmt::skip]` or generated-file exclusion MUST be limited to syntax that rustfmt
cannot represent usefully and MUST say why.

No `rustfmt.toml` or `.rustfmt.toml` exists at adoption, so default rustfmt is the authority.
Adding either file changes this standard and MUST update this document in the same change.

Names MUST follow the Rust API Guidelines and standard casing:

- types and traits use `UpperCamelCase`; functions, variables, and modules use `snake_case`;
  constants use `SCREAMING_SNAKE_CASE`;
- conversions use `as_`, `to_`, and `into_` according to borrowing, cost, and ownership;
- simple accessors use the field or concept name rather than a `get_` prefix;
- predicates read as predicates (`is_`, `has_`, `can_`, or `should_`);
- units and representation MUST be explicit in a type or, where a primitive is unavoidable, in
  the name (`timeout_ms`, not `timeout`). Prefer `Duration` and domain newtypes to unit suffixes.

Code SHOULD read in domain terms. Avoid compressed names outside small conventional scopes, dense
iterator chains that hide control flow, and clever expressions that make errors or mutation hard
to see. Extract a named operation when the name explains policy; do not extract one-line wrappers
that merely force the reader to jump elsewhere.

Comments explain **why**, an invariant, a safety argument, or a non-obvious platform constraint.
They do not narrate syntax. Stale comments are defects and MUST be updated or removed with the
code they describe.
