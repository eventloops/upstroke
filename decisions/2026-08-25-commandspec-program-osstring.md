# 2026-08-25 — `CommandSpec.program` widens to `OsString`

**Verdict — scheduled, not yet in force.** This record fixes the *direction*:
when workstream W4 of `proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md` opens
`CommandSpec`, `program` widens from `String` to `OsString`. It does **not** move
the spec today. `DESIGN.md:222` still reads `String` and still governs, and
`CODING_STANDARDS.md` §1's "Known conflicts at adoption" block — carried since
`6772b2a` — remains accurate and stays. Both change in one commit at W4, which
cites this record.

**Why this record does not carry a `DESIGN.md` edit.** `decisions/README.md`
requires the compressed edit at the time of the decision for a record whose
outcome changes the spec. That rule is satisfied by not claiming an outcome the
spec has not taken: `PR4-PROGRAM-PATH-NOT-UNICODE` stays **open** until W4 lands,
and this record is its scheduled venue and settled direction, not its closure. A
W4 implementer therefore has one authority, `DESIGN.md`, and one instruction,
this record's direction — never two live documents disagreeing about the type.

This addresses `PR4-PROGRAM-PATH-NOT-UNICODE` — open since 2026-08-20 as one
of the two accepted deviations the owner ruling of that date carried to G2.
Neither it nor the known-conflict block above is retired here; both are
retired at W4, in the same commit that moves the spec.

The scope is deliberately the one measured field. `args: Vec<String>` and the
`env` pairs are audited in W4 under the same standard (§8) and widened only
where a measured path can flow through them — abstraction pays rent; no
speculative widening.

## Why this way

Three grounds, in descending weight:

1. **It restores a frozen invariant instead of shrinking one.**
   `decisions.pr_sequence[5].slice_contract.invariants_preserved[1]` says,
   verbatim: *"legacy engine behavior unchanged (the legacy engine does not
   run the shell probe)"*. Pre-PR4 code carried the resolved `PathBuf`
   through `Command::new` unchanged and a non-Unicode installation ran;
   under the frozen `String` shape that invariant is unsatisfiable — the
   value cannot be represented, and PR4 could only choose which way to fail.
   Widening makes the invariant true again as written. The alternative is a
   permanent erratum narrowing it, where a mechanical repair exists.
2. **The normative standard already rules on the type.** `CODING_STANDARDS.md`
   §8, on master since `02b0752`: *"Represent paths with `Path`, `PathBuf`,
   `OsStr`, or `OsString`. Do not construct paths by string concatenation or
   assume UTF-8. A lossy display string is for diagnostics only, never
   identity."* The program is an OS path (or a PATH-resolved name — the same
   OS-string domain). Keeping `String` makes the standard's self-exception
   block permanent; a normative document with a standing exemption for the
   design it governs is corrosive in both directions.
3. **The refusal punishes a legitimate installation for a representational
   choice.** A Unix path is bytes; an agent CLI under a non-UTF-8 prefix is
   exotic but legal, ran before PR4, and fails today with a refusal whose
   only cause is the struct's type. PR4's record already rejected the other
   escape — `to_string_lossy` spawns a path that names nothing.

## Consequences, enumerated for W4

- Adapters hand the resolved `PathBuf` through; no lossy conversion enters
  identity. Diagnostics use `to_string_lossy`, documented as display-only
  (§8's own carve-out).
- The runner-policy canonical digests define the program's canonical bytes
  explicitly — OS-native (raw bytes on Unix, WTF-8 on Windows) — and the
  pinned samples (`HOST/CONTAINER_CANONICAL`, `SAMPLE_DIGEST`,
  `SAMPLE_CANONICAL_BYTES`) re-pin mechanically, the same class of edit the
  rename required. Cross-platform replay of a non-UTF-8 program was never
  meaningful — the path cannot exist on the other platform — so the
  per-platform encoding costs nothing real.
- `agent::bin::tests::a_program_path_a_string_cannot_carry_is_refused_by_name`
  — deliberately left unchanged by PR4 round 6 because changing it would
  have resolved an owner question inside a repair round — **will be** replaced
  at W4 by the test that the path is carried through, and the refusal it
  documents deleted with its cause. It is still in the tree
  (`src/agent/bin.rs`) and still correct, because the refusal is still the
  behaviour `DESIGN.md` specifies.
- `CODING_STANDARDS.md`'s Known Conflicts block **will be** removed in the same
  motion the widening lands (a one-line master docs edit citing this
  record). It stands until then.
- Packet side: W4 greps the packet for any `CommandSpec` shape mention as a
  completeness check; the errata batch
  (`upstroke-lab:packet/2026-08-25-g2-pass-errata.md`) records that none is
  expected.
- **The widening stops at spawn-time identity; the durable log keeps `String`.**
  Two live documents reason oppositely about one trade-off and W4 must not have
  to guess which governs. `src/topology/events.rs`'s `TaskDispatched::worktree_path`
  argues that *"a platform path type here would make a log written on one operating
  system a question on another"*, and this record argues three days later that
  *"cross-platform replay of a non-UTF-8 program was never meaningful — the path
  cannot exist on the other platform"*. Both are right about their own subject and
  neither generalises: the program is **ephemeral spawn-time identity**, consumed on
  the machine that resolved it, so per-platform encoding costs nothing; a recorded
  worktree path is **durable wire-facing identity**, read back by a later process and
  potentially on another platform, so it stays `String` under the reason its own
  field documents. W4 widens the first and leaves the second alone. Owner ruling,
  2026-08-28, on a standards-review finding that read the second as the same defect
  as the first: `CODING_STANDARDS.md` §8's path-representation bullet carries no
  requirement keyword where two sibling bullets in that section are MUST-tagged, so
  it is at most a SHOULD, and §1 admits a SHOULD deviation on a concrete reason in
  the code — which is on the field.

## Rejected

- **Keep `String`, keep the named refusal** (the shipped behaviour). Honest,
  and the right holding position while frozen — but as an end state it
  permanently narrows a preserved invariant and permanently exempts the
  design from its own implementation standard, both to avoid a mechanical
  change at the exact moment (the pass) built for such changes.
- **`to_string_lossy`.** Already rejected by the PR4 record: each invalid
  byte becomes U+FFFD and the runner spawns a path the operator never wrote,
  failing at `execvp`/`CreateProcess` with a worse diagnostic than the
  refusal it replaces.

## Measured vs assumed

Measured: the PR4 finding chain (`PR4-CONF-007` → the 2026-08-20 ruling
rows), the verbatim invariant text (read from the packet 2026-08-25), the
standards text on master, and the pre-PR4 carry-through behaviour as
recorded. Assumed until W4 checks it: that the packet nowhere freezes the
`CommandSpec` shape in its own text (if it does, a one-line erratum
accompanies the widening).
