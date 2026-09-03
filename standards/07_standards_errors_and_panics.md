## 7. Errors and panics

Library modules MUST return typed errors, normally derived with `thiserror`. `anyhow` is limited to
the binary/application edge, where the program adds user-facing operational context and decides
how to report or exit.

Error types and handling MUST follow these rules:

- Define variants around decisions a caller can make, not one variant for every line that can
  fail. Preserve the source error where it helps diagnosis.
- Error `Display` text starts lowercase, carries no trailing period, and does not repeat its
  source's message: report chains join fragments with `": "`.
- Add operation, path, task, run, or adapter context at the layer that knows it. Do not include
  secrets, tokens, full sensitive prompts, or other values that must not enter logs.
- Inspect structured error kinds. Only an actual not-found condition becomes absence; permission,
  corruption, malformed data, and transient I/O remain errors.
- Do not discard an error through `.ok()`, `let _ =`, `unwrap_or_default()`, or a catch-all match
  unless the operation is explicitly best-effort. Best-effort behaviour MUST define its
  observability and have a test for the failure path.
- Retries MUST be bounded, classify retryable failures, respect cancellation, and avoid repeating
  a non-idempotent effect without a protocol that makes it safe.

Production code MUST NOT call `.unwrap()` or `.expect()`. It also MUST NOT use `panic!`, `todo!`,
`unimplemented!`, potentially panicking indexing, or assertions to handle input, configuration,
persisted state, I/O, subprocess behaviour, or scheduling outcomes. Prefer exhaustive matching
and types that make an impossible branch impossible.

The panic policy is machine-enforced: `[lints]` denies `clippy::unwrap_used`, `expect_used`,
`panic`, `todo`, `unimplemented`, and `dbg_macro` on every target the Clippy leg compiles.
`.unwrap()` has no test allowance; the other lints carve tests out through `clippy.toml`'s
`allow-*-in-tests` booleans.

An assertion, an `unreachable!`, or an `.expect()` under `#[expect(clippy::expect_used, reason)]`
for a true internal invariant is exceptional: the invariant and proof MUST be local and
documented, termination must be the intended response to a program defect, and tests MUST cover
the surrounding boundary. Tests fail their own setup with `.expect(` and a message naming the
failed premise; `.unwrap()` stays denied in tests too.

The panic strategy stays `unwind` in every profile: §6's cleanup guarantees depend on RAII running
during unwinding, so `panic = "abort"` is a change to this standard, not a build setting.
