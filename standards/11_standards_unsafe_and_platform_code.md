## 11. Unsafe and platform-specific code

Use safe Rust first. Unsafe code is permitted only where required by an OS/FFI boundary or where a
measured need cannot be met safely.

Every unsafe operation MUST:

1. be confined to the smallest practical module and block;
2. have an immediately adjacent `SAFETY:` comment stating every obligation and why it holds;
3. validate raw pointers, lengths, initialization, aliasing, ownership, and lifetime assumptions
   as applicable;
4. be wrapped in a safe API that cannot be called without satisfying the remaining preconditions;
5. use explicit unsafe blocks inside an `unsafe fn` (`unsafe_op_in_unsafe_fn` discipline);
6. have focused native tests on each supported platform where the code is active.

New unsafe code that Miri can reach SHOULD be exercised under Miri. Sanitizers SHOULD be used
where a configured platform leg can exercise the affected boundary. Until either tool has a named
repository gate, its use is a triggered review requirement rather than an automated compliance
claim.

`clippy::undocumented_unsafe_blocks` is the intended mechanism for item 2. The tree does not yet
satisfy it everywhere, so per the §2 ratchet it turns on in the commit that closes that gap —
production and tests alike.

Windows, macOS, and Linux are supported targets. Cross-platform code MUST use platform-neutral
types and semantics; `cfg` modules isolate genuine OS differences. Do not make Unix path, signal,
permission, executable-suffix, process-tree, rename, or locking assumptions in shared code.
Platform code is not verified by compiling it on another OS alone: behaviour needs native CI
coverage.

Evidence is platform-gated in the same way the code is. A test, a lint attribute, or a suppression
that covers a `cfg` region is evidenced only by a leg that compiles that region, and each kind of
claim needs the leg that evaluates it:

- A **Clippy** lint attribute is evidenced only by a Clippy leg for that platform. A native test
  leg and a native MSRV `check` leg do compile the region, but they do not run Clippy over it, so
  they pass while a Clippy leg for the same platform fails.
- A **rustc** lint attribute is not so limited: any leg that compiles the region evaluates it, so a
  denied rustc lint does fail a native `test` or `check` leg on that platform. Its expectation
  needs one thing more, because `unfulfilled_lint_expectations` is warn-by-default and so retires
  a suppression only where warnings are promoted to errors. `ci.yml` sets `RUSTFLAGS: -D warnings`
  at workflow scope, so today that is every leg — which means narrowing it to a single job would
  silently take the self-retirement guarantee with it.
- An `#[expect]` inside a region that no such leg compiles is inert in both directions. It
  suppresses nothing, and it cannot become the warning that retires it. It reads as enforcement
  and is not.
- Cross-compilation is evidence, not a native run, and it carries its own blind spot: a path that
  resolves for the host may not resolve for the target, which Clippy reports as a bare
  configuration warning that `-D warnings` does not promote.

A change that adds platform-gated code, tests, or annotations MUST name the leg that evaluates
them. Every supported target has one today: `ci.yml` runs a Clippy leg on each of the three —
`lint`, `lint (windows)` and `lint (macos)` — beside `test` and `msrv` matrices that compile all
three natively, and the `upstroke-ci` aggregate fails unless every one of them succeeds. Adding a
platform, or gating code to a target no leg covers, leaves a claim unevidenced; that is recorded
in Appendix A as an uncovered platform rather than left to be inferred from a green baseline.
