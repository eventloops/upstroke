## 15. Dependencies and features

A new dependency MUST have a concrete benefit over the standard library or an existing dependency.
The pull request MUST consider maintenance status, licence compatibility, security history,
transitive cost, binary-size/compile-time effect, MSRV, and target support in proportion to risk.

- Keep `Cargo.lock` committed and update only entries required by the dependency change.
- Avoid broad default features. Enable the smallest stable feature set that provides the needed
  behaviour, without creating a fragile hand-built substitute for a crate's supported setup.
- Feature flags MUST be additive and compile in every supported combination that CI claims. Do not
  use a feature to select mutually incompatible meanings for the same API.
- Dependency types SHOULD not leak through a public API unless that dependency is intentionally
  part of the public contract.
- CI workflow actions MUST be pinned to a full commit SHA with the version named in a comment; a
  tag is a mutable reference, not a pin.
- The crate builds without a build script. Introducing one, or adding a proc-macro dependency,
  names the new compile-time supply-chain surface in this section's assessment.
- A dependency-policy gate (`cargo deny`: advisories, licences, bans, sources) is the intended
  mechanism for this section's review duties. Its requirements activate in the same change that
  introduces its configuration, carrying the positive control Appendix A requires.
- No dependency may introduce direct model-API or engine HTTP behaviour contrary to `DESIGN.md`
  §4, even behind an optional feature.
