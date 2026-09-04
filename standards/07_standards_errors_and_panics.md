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
invariant may use an assertion, or `unreachable!` or `.expect()` under an `#[expect]` naming that
construct's own lint and carrying a `reason`, when the proof is local and documented and
termination is the intended response to a program defect. Tests fail their own setup with `.expect(`
and a message naming the failed premise.

**Indexing, slicing and `unreachable!` are denied on the same terms, and the lints are owed.**
`v[i]`, `&v[a..b]` and `unreachable!` all panic, and the paragraph above already governs the first
two whenever the index comes from input, configuration, persisted state, I/O, subprocess behaviour
or a scheduling outcome. `clippy::indexing_slicing` and `clippy::unreachable` are not in `[lints]`
today, so those are the constructs this standard governs that the build does not catch: prose where
the rest of the panic policy is mechanized. Both are owed a `[lints]` entry denying them, and they
take **different** treatment in tests, because Clippy offers different treatment. Indexing takes
`allow-indexing-slicing-in-tests` beside the three allowances already in `clippy.toml`: a test that
indexes a collection it has just built panics as its own failure, which is the carve-out `panic!`
and `.expect(` already have. `unreachable!` takes no allowance, because Clippy has none to take —
there is no `allow-unreachable-in-tests`, and `allow-panic-in-tests` does not suppress
`clippy::unreachable` (measured on clippy 0.1.97: the site in `src/workspace_manager/tests.rs` is
reported under this repository's current `clippy.toml`). So `unreachable!` is denied in tests as
well, like `.unwrap()`, and a test that needs one carries the per-site `#[expect]` with a `reason`
that this section already requires of every use. Prefer `get`, `get_mut`, `first`, `last`,
`split_at_checked` and pattern matching, each of which returns the absence rather than terminating
on it. This rule is transitional in the same way as §6's and the `?` rule above: it
binds the code a change adds or rewrites, under the activation rule `standards/SWEEP.md` states,
and the `[lints]` entries land in the pull request that retires that rule, when the tree can compile
under them.

The panic strategy stays `unwind` in every profile: §6's cleanup depends on RAII running during
unwinding, so `panic = "abort"` is a change to this standard, not a build setting.

Enforced by: `[lints]` on all three Clippy legs for `unwrap_used`, `expect_used`, `panic`, `todo`
and `unimplemented`; review for indexing, slicing, `unreachable!` and the rest, until those two
lints land. Even once they do, the lints are narrower than this section: `assert!`, `split_at`,
arithmetic overflow and a local macro that expands to `unreachable!` all still terminate and none
is caught, so what may panic stays a review question and only these two constructs stop being one.
