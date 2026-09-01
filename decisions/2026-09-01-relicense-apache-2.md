# 2026-09-01 — relicense to Apache-2.0, with a NOTICE file

**Verdict: upstroke is licensed under the Apache License 2.0 from this change forward.**
`AGPL-3.0-only` and the public commercial-licence offer are retired together. Releases up to and
including `0.1.0` were published under the AGPL and remain so — published tags and crates.io
packages are immutable, and no attempt is made to touch them.

## The reasoning that earned it

The AGPL-plus-commercial position was a monetisation-first bet: the licence deters exactly the
users who might pay, and every deterred user is a visible lead. At zero market share that bet
inverts. The users the project needs first are individual developers, and in this category their
first trial is typically against an employer's repository — where blanket AGPL policy bans block
the trial regardless of the fact that mere use imposes no obligations. The funnel tax lands at
its narrowest point.

Three facts settled the direction, rather than the timing being left to "wait for evidence":

- **The evidence is invisible.** An adopter who bounces off the licence badge files no issue.
  There is no observable trigger for a later flip; waiting is deciding.
- **The flip is worth the most before distribution, not after.** A launch impression happens
  once. Relicensing after adoption stalls forfeits the window the relicensing was for.
- **Loosening is the cheap direction.** AGPL→Apache by the sole copyright holder costs nothing
  and angers no one; Apache→AGPL later, with users and contributors on board, is the direction
  that forks projects. The option being exercised here could not have been exercised more
  cheaply at any later date.

What is given up is named, not waved away: the strong payer under AGPL was never the
internal-use enterprise (internal use triggers nothing; such a buyer pays only for policy
comfort) but the **embedder** — a platform shipping upstroke inside its own product, which the
AGPL would have forced to the table. Apache-2.0 admits that buyer free of charge. The project
chooses to be the default plumbing rather than to hold the toll, and moves its moat from the
code to the mark: Apache-2.0 §6 grants no trademark rights, and the NOTICE file added with this
record is the mechanism (§4(d)) by which any distributed derivative work of a Work that
includes a NOTICE must reproduce its attribution notices — within a bundled NOTICE file,
within its source or documentation, or within a display the work generates — a condition on
distribution, not on private use. The deeper commercial analysis stays in the private strategy record
([2026-08-22](2026-08-22-strategy-record-private.md)); this record retires its public licensing
mechanism only.

## Consequences

- `LICENSE` carries the canonical Apache-2.0 text; `NOTICE` is added (name, copyright,
  repository URL — deliberately minimal). `Cargo.toml` metadata, the `DESIGN.md` header line,
  the README licence section, and the `src/lib.rs` / `src/main.rs` headers all move together in
  this record's pull request; crates.io metadata updates at the next publish.
- The README licence badge switches from the crates.io-derived badge to the GitHub-derived one:
  the crates badge reports the last *published* licence and would misreport the repository until
  the next release.
- **The CLA stays**, with clause 2 reworded and its rationale rewritten honestly: its value is
  relicensing agility and clean provenance, no longer a dual-licence business. Replacing it
  with a DCO is a separate decision this record deliberately does not make.
- The commercial-licence sentences leave the README and the two source headers. Anyone who
  wants proprietary use now simply has it.
- **Owed follow-up, separately because the release workflow is high-blast-radius:** the release
  archives ship the bare binary today. Under any licence they should carry `LICENSE`, `NOTICE`,
  and generated third-party attributions for the statically linked crates (`cargo-about` or
  equivalent), added inside the existing three assets so the release contract's exact-asset-set
  check is untouched. **The next release is gated on that follow-up**, and the gate lives where
  release authority lives: `MAINTAINING.md`'s release contract carries it as this record's
  compressed edit — decisions are history, not living authority. The workflow does not yet
  inspect archive contents, so until the follow-up lands the gate is owner-verified at tag
  time, like the immutability readback the contract already requires. (Deferring the workflow
  edit while gating on it is deliberate: the release path is high-blast-radius and deserves its
  own reviewed pull request.)

## Rejected

- **Staying AGPL and watching for licence-driven losses.** Rejected on the invisible-evidence
  and launch-window grounds above.
- **MPL-2.0 as a middle ground.** For a standalone binary, file-level copyleft buys almost
  nothing the Apache conditions don't, while keeping a scarier badge. The middle ground is all
  cost.
- **BUSL or another source-available delay licence.** Off corporate allowlists entirely — worse
  than AGPL for the stated goal, and it would surrender the "official CLIs only, nothing to
  hide" trust posture.
- **Dropping the CLA now that the copyleft rationale is gone.** Deferred, not decided: the CLA
  still buys relicensing agility and explicit provenance, and removing it deserves its own
  record if it happens.

## Measured vs assumed

**Measured**, by reading the tree and history at this record's head: every commit author
identity is the owner (four personal emails) or one of the engine's own local run identities;
the tree vendors no third-party code (the many "vendor" hits in `src/` are the product's
cross-vendor review language); no inbound NOTICE obligations exist to propagate; the release
workflow packages exactly the binary and nothing else into each archive.

**Attested rather than measured: chain of title.** Author identities and the absence of
vendored trees are evidence, not proof — they establish neither line-level origin nor that
nothing was ever committed under an owner or engine identity with someone else's rights
attached. No stronger measurement exists in a git history. The owner's merge of this record is
the attestation that the whole is theirs to relicense, on the same footing as every other
owner's-merge attestation in this repository.

**Assumed, and named as the bet:** the adoption benefit itself — that allowlist-frictionless
licensing materially widens the top of the funnel for a tool whose first trial happens at work.
That is market behaviour, unmeasurable from this repository. If the bet is wrong, nothing about
it is recoverable by relicensing back; the cost was accepted with eyes open.
