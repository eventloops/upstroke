## 15. Dependencies and features

A new dependency has a concrete benefit over the standard library or an existing dependency, and
the pull request weighs maintenance status, licence, security history, transitive cost, build
effect, MSRV and target support in proportion to risk.

- `Cargo.lock` stays committed; a dependency change updates only the entries it requires.
- Enable the smallest stable feature set that provides the behaviour: no broad default features,
  and no fragile hand-built substitute for a crate's supported setup.
- Feature flags are additive and compile in every combination CI claims; a feature never selects
  incompatible meanings for one API.
- Dependency types do not leak through a public API unless they are deliberately part of the
  contract.
- CI actions are pinned to a full commit SHA with the version in a comment; a tag is not a pin.
- The crate builds without a build script; introducing one, or a proc-macro dependency, names the
  new compile-time supply-chain surface in the pull request.
- No dependency introduces model-API or engine HTTP behaviour (`DESIGN.md` §4), even behind a
  feature.

`cargo deny` is the intended mechanism for the licence, advisory and source checks; until its
configuration lands they are review duties.

Enforced by: the locked MSRV check and all-feature builds; SHA pins and the rest by review.
