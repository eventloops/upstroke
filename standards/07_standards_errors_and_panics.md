## 7. Errors and panics

Library modules return typed errors, normally derived with `thiserror`. `anyhow` is limited to the
binary edge, where the program adds operational context and decides how to report or exit.

- Variants follow decisions a caller can make, not one variant per failing line. Preserve the
  source error where it helps diagnosis.
- `Display` text starts lowercase, has no trailing period, and does not repeat its source; report
  chains join fragments with `": "`.
- Add operation, path, task, run or adapter context at the layer that knows it. Never include
  secrets, tokens or sensitive prompt text.
- Inspect structured error kinds: only an actual not-found becomes absence; permission, corruption,
  malformed data and transient I/O stay errors.
- Do not discard an error through `.ok()`, `let _ =`, `unwrap_or_default()` or a catch-all match
  unless the operation is explicitly best-effort, in which case its observability is defined and
  its failure path is tested.
- Retries are bounded, classify retryable failures, respect cancellation, and never repeat a
  non-idempotent effect without a protocol that makes it safe.

**`?` is propagation, not handling.** A bare `?` is allowed only when the caller can act on the
error exactly as it is: the error type already carries the operation and its context, and the
function has nothing to add that a reader of the failure would need. Otherwise decide at that point
— `map_err` into a variant that names the operation, or a `match` that chooses. A `?` on a raw
`io::Error`, a serde error, or another library's error across a module boundary is a finding. `?`
on an `Option` MUST NOT turn absence into failure silently. The aim is fewer `?` sites, each one
deliberate: a function that is a long chain of `?` has usually not decided what its errors mean.
This rule is transitional in the same way as §6's: it binds the code a change adds or rewrites,
under the activation rule `standards/SWEEP.md` states, and that file tracks the existing tree.

**Panics.** Production code MUST NOT call `.unwrap()` or `.expect()`, nor use `panic!`, `todo!`,
`unimplemented!`, panicking indexing or assertions to handle input, configuration, persisted state,
I/O, subprocess behaviour or scheduling outcomes. Prefer exhaustive matching and types that make the
impossible branch impossible. `[lints]` denies `clippy::unwrap_used`, `expect_used`, `panic`,
`todo`, `unimplemented` and `dbg_macro` on every target the Clippy legs compile; `.unwrap()` has no
test allowance, and the others are carved out for tests through `clippy.toml`. A true internal
invariant may use `unreachable!`, an assertion, or `.expect()` under
`#[expect(clippy::expect_used, reason = "…")]` when the proof is local and documented and
termination is the intended response to a program defect. Tests fail their own setup with `.expect(`
and a message naming the failed premise.

The panic strategy stays `unwind` in every profile: §6's cleanup depends on RAII running during
unwinding, so `panic = "abort"` is a change to this standard, not a build setting.

Enforced by: `[lints]` on all three Clippy legs for the panic policy; review for the rest.
