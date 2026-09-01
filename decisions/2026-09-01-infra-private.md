# 2026-09-01 — the build-box tree lives outside the public repository

**Verdict.** The 18-file `infra/` tree — provisioning, the operational tooling
sources, the AppArmor profile, and the two operations documents — moves to the
private companion repository. The owner rules that the tree is the live box's
operational-security narrative (hardening sequence, firewall and break-glass
topology, guest NAT identity, token-preflight design) and that its
reproducibility value serves the operator, not the public engine contract.
The 2026-09-01 external security review recommended privatizing the package
as a P1; an owner-side documentation triage, held outside the repository, had
argued a narrower split (operations report private, tooling public) — the
owner adopts the review's whole-package scope.

## Relationship to prior records, stated so nothing is silently overturned

- **2026-08-22 (the strategy record) is not overturned.** Its explicit
  keep-public floor — `decisions/`, `MAINTAINING.md`, the workflows and
  validators, `reviews/FINDINGS.md`, the technical sections of `DESIGN.md` —
  names no `infra/` path, and every floor item remains public. The trust
  model's executable surface stays where it was: the four gate scripts and
  validators live under `.github/scripts/`, not `infra/`.
- **`DESIGN.md` is unchanged, deliberately.** The tree is operator tooling for
  one machine; no design section describes it, and the engine neither reads
  nor ships behavior from it.
- **`CLAUDE.md` is reconciled in the same change** (§1 same-change rule): its
  build-box section drops the backticked `infra/README.md` reference — the C1
  gate's only `infra/` coupling — and states the public contract inline.

## Release-artifact consequence, disclosed

`Cargo.toml` never excluded `infra/`, so a publish from any tree containing
it would have packaged the tree by default — `cargo package --list` at this
change's parent contains all 18 files. No *published* crate ever did:
`upstroke 0.1.0` was published from `3d9cf38`, hours before the tree first
reached master at `458d928`, and contains zero `infra/` paths. The exposure
was prospective, not historical, and after this change it cannot occur: the
crate's contract is the engine, and the operator tree was set to ship in it
by default, not by design.

## Evidence that this is relocation, not deletion

The private intake landed on the companion repository's default branch, at
its commit `359953f54f6c1dd08b2d2d1b36b1a1635a72e26b` (the commit-naming form
the 2026-08-22 `Provenance:` convention already sanctions), before this
record merged; its `infra/` tree ID equals the public parent's `infra/` tree
ID `1293e4a71a6637d4d628aeeb5abc308ec7578a80` exactly — same names, same
modes (executable bits restored after a workstation copy dropped them), same
blobs. And the standing caveat holds: relocation does not remove the tree
from public git history — the directory's lineage began at `7923a912` (then
twelve files under the old names), and the exact 18-file tree removed here is
on every public head from `458d928` through this change's parent; the move is
organizational and prospective.

## Rejected

- **The narrower split (17 keep, operations report private).** The triage's
  tested verdict; overruled by the owner: the scripts themselves encode the
  hardening sequence, guest identity, and preflight design — the
  operational-security narrative is not confined to the report.
- **Deletion without a verified private intake.** A later hardware rebuild
  would have to mine public history for `setup.sh` and the guest templates;
  the tree-ID check above is the guard.
- **Keeping the tree public.** The reproducibility-for-strangers argument;
  rejected as serving no public contract the repository actually makes —
  the deployed box does not depend on the public copy.

## Cross-references

- [2026-08-22 — the strategy layer lives outside the public repository](2026-08-22-strategy-record-private.md)
  — the public/private split mechanism and the keep-public floor this record
  leaves intact. (The stub mechanism is not needed here: after the CLAUDE.md
  reconciliation, the C1-checked citation surface — `CLAUDE.md` and
  `CONTRIBUTING.md` — carries no `infra/` tree path; dated immutable records
  and this record's own mentions remain valid as history, which a stub cannot
  improve.)

## 2026-09-01 — erratum: the canonical recovery point

The post-merge frontier review of the relocating pull request's final head
(`136104b`, verdict CHANGES_REQUIRED, one finding — recorded in
`reviews/2026-09-01-pr84-frontier-review-136104b.md`) caught an imprecision
in the evidence above, corrected here by this folder's appended-erratum
mechanism; nothing above is rewritten.

- "This change's parent" is ambiguous at the merge head: `136104b` has two
  parents, and ordinary `HEAD^1` selects `e329eea` — the branch side, which
  carries no `infra/` at all. The commit meant is the pull request's base
  parent on `master`.
- "Every public head from `458d928`" overreached: branch-side commits
  reachable in history (`26c6e6c`, for one) carry no `infra/`.

The precise statements, each verified against the commit graph:

- **The canonical recovery point is
  `fff6abd97ff49117a7af5841765fcee4f4f058f6`** — `master` immediately before
  the removal merge. Its `infra/` tree is
  `1293e4a71a6637d4d628aeeb5abc308ec7578a80`, the exact 18-file tree, equal
  to the private intake's. One-command recovery, extracting to an explicit
  destination outside the working tree so nothing lands in the index for an
  accidental recommit:
  `git archive fff6abd9 infra | tar -x -C <destination>`.
  (An in-clone `git checkout fff6abd9 -- infra/` stages all eighteen files —
  measured — and is deliberately not the documented path.)
- The same exact tree is on every **first-parent `master` snapshot** from
  `458d928` through `fff6abd` inclusive — `458d928` introduced the tree in
  exactly this form, and no later first-parent commit through `fff6abd`
  touches `infra/` (verified: `git log --first-parent 458d928^..fff6abd --
  infra/` returns only `458d928` itself). Claims about heads outside
  master's first-parent line are withdrawn.
