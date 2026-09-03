## 11. Unsafe and platform-specific code

Safe Rust first. Unsafe code is permitted only at an OS/FFI boundary or where a measured need cannot
be met safely. Every unsafe operation is confined to the smallest module and block; carries an
adjacent `SAFETY:` comment stating each obligation and why it holds; validates its pointer, length,
initialization, aliasing, ownership and lifetime assumptions; is wrapped in a safe API that cannot
be misused; uses explicit unsafe blocks inside an `unsafe fn` (`unsafe_op_in_unsafe_fn` is denied);
and has native tests on each platform where it is active. `clippy::undocumented_unsafe_blocks` turns
on in the commit that makes the tree clean under it. Miri and sanitizers are review-triggered until
a named gate exists.

Windows, macOS and Linux are supported targets, and Windows is first-class. Shared code uses
platform-neutral types and semantics, with `cfg` modules isolating genuine OS differences: no Unix
assumptions about paths, signals, permissions, executable suffixes, process trees, rename or locking
in shared code. Platform code is verified by running it natively, not by compiling it elsewhere.

**Evidence is platform-gated the way the code is.** A test, lint attribute or suppression inside a
`cfg` region counts only on a leg that compiles that region, and each kind of claim needs the leg
that evaluates it:

- A Clippy lint or `#[expect]` is evidenced only by a Clippy leg for that platform. CI runs one on
  each of the three — `lint`, `lint (windows)`, `lint (macos)` — beside `test` and `msrv` matrices
  that compile all three natively.
- A rustc lint is evaluated by any leg that compiles the region, but its `#[expect]` retires itself
  only where warnings are errors. `ci.yml` sets `RUSTFLAGS: -D warnings` at workflow scope, so
  today that is every leg; narrowing it to one job would silently remove the self-retirement
  guarantee.
- An `#[expect]` in a region no such leg compiles is inert in both directions: it suppresses nothing
  and cannot retire. Two effective `cfg` predicates are compiled by no runner — the
  unsupported-target else-arms — and `src/effects/tests.rs`'s `NO_CI_RUNNER_COMPILES` holds that
  set exact, so a newly uncovered region fails the census.

A change that adds platform-gated code, tests or annotations names the leg that evaluates them.

Enforced by: `unsafe_op_in_unsafe_fn` in `[lints]`; the three Clippy legs and native matrices; the
`cfg` census; review for safety proofs.
