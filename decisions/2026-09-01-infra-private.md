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

`Cargo.toml` never excluded `infra/`, so every published crate to date
packages the tree; from the first tag after this change, `cargo package` no
longer includes it. This is a deliberate consequence of the ruling, not an
oversight: the crate's contract is the engine, and the operator tree was
shipping in it by default, not by design.

## Evidence that this is relocation, not deletion

The private intake was verified against the public parent before this record
landed: the companion repository's staged `infra/` tree ID equals the public
parent's `infra/` tree ID `1293e4a71a6637d4d628aeeb5abc308ec7578a80` exactly —
same names, same modes (executable bits restored after a workstation copy
dropped them), same blobs. The owner merges the private intake before or
together with the public removal. And the standing caveat holds: relocation
does not remove the tree from public git history — it was introduced at
`7923a912` and remains retrievable from every public head that contained it;
the move is organizational and prospective.

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
  reconciliation, no public document cites an `infra/` path.)
