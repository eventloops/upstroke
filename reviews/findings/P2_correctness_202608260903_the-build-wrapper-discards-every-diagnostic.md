---
id: PR5D-TOOLBOX-DISCARDS-CLIPPY-OUTPUT
severity: P2
disposition: deferred
category: correctness
pr: 5
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: the project owner — the build box's tooling, not the tree
---

## Failure sequence

`~/bin/upstroke-build` silently discards the stderr of every command it runs. Line 85,
`exec {slotfd}>"$lock" 2>/dev/null`, is an `exec` with only redirections, which rebinds the
*wrapper's own* stderr permanently, and the exec'd `cargo` inherits the null redirect. So
`upstroke-build cargo clippy … > log 2>&1` produces an empty log: the exit code survives and every
diagnostic is lost, and the same holds for `cargo +1.85.0 check` and for `cargo test` compile
errors. The evidence is already on disk — `pr5/gates-merged/clippy.log`, `fmt.log` and `msrv.log`
are all zero bytes. A gate whose result is trustworthy and whose evidence is empty cannot be
audited after the fact.

## What the change that takes this up should do

Scope the redirection to the `exec` alone rather than letting it rebind the wrapper's stderr.
The fix is one character's worth of scope and it has to be applied on the build box, because the
file is outside the repository — which is why no pull request can close this row. Until then the
workaround is what was used throughout: `--message-format=json`, which puts diagnostics on stdout,
or running `CARGO_TARGET_DIR=<slot> cargo …` directly.

Recorded in `reviews/FINDINGS.md` §8. This is a build-infrastructure defect, not a defect in the tree; `correctness` is the closest word in the closed category vocabulary and the classification is this migration's judgement.
