---
id: PR149-ULID-SEED-COLLIDES-ACROSS-CONCURRENT-PROCESSES
severity: P3
disposition: deferred
category: correctness
pr: 149
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/ulid.rs:31
provenance: pre_existing
first_bad:
guard: the next change to `src/ulid.rs`, or a run-directory uniqueness check at `src/engine/coordinator.rs`; recorded and not fixed here on the coordinator's direction of 2026-09-05, because PR #149's subject is the settle kill tests; a later pass labelling this P1 escalates to the owner rather than re-defers
---

## Failure sequence

`src/ulid.rs:1-4` states the contract: "ULID generation (§15: `run-id = ULID`) … Uniqueness
against ourselves is the requirement — nothing cryptographic." Two concurrent upstroke processes
on one machine are "ourselves", and the seed breaks it to arithmetic:

```rust
let mut seed = now_ms ^ (u64::from(pid) << 32) ^ nonce.rotate_left(17);
```

Split into words. The low word must match, and `rotate_left(17)` of a nonce below 2^15 lands in
the low word, so two processes at low nonces agree only at the same nonce and then need the same
pid, which live processes never share. A nonce at or above 2^15 lands in the high word instead:
`32768.rotate_left(17) == 1 << 32`, so pid 100 at nonce 32768 and pid 101 at nonce 0 seed
identically. In general the seeds are equal when the nonces agree modulo 2^15 and their XOR
shifted right by fifteen equals the pids' XOR — `nonce_a ≡ nonce_b (mod 2^15)` and
`pid_a ^ pid_b == (nonce_a ^ nonce_b) >> 15` — which needs one of the two at a nonce of at least
2^15 (the nonce is the per-process count of ULIDs minted so far) and both drawing in the same
millisecond. `(100, 65536)` against `(102, 0)` and `(100, 32769)` against `(101, 1)` are further
examples, not an enumeration; a mirror of the construction confirms them. One full run of the lib test harness mints 6,017 ULIDs and ends at nonce 6,016, which is why
the settle tests were the symptom and not the population: the harness alone cannot get there. The eighty random bits are then
identical, the timestamp is identical, and the ULID is identical.

The production call sites at `323beb0`:

- `src/engine/coordinator.rs:121`, `let run_id = ulid::ulid();` — §15's canonical run identity.
- `src/engine/topology/seams.rs:330` and `:334` — `IncarnationId`.
- `src/interaction.rs:101` — `QuestionId`.
- `src/agent/codex.rs:486`, `src/workspace.rs:1402`, `:1445`, `:1483`, and
  `src/workspace_manager.rs:3471`.

Each checked against its file's `#[cfg(test)]` boundary; `src/agent/proc.rs:4572` and `:5133`,
listed at first, are inside `mod tests` and are not production.

**What currently prevents the harm, and that it is incidental.** Two conductors cannot both be
minting a run id for one worktree: `coordinator.rs` takes `WorktreeLock::acquire_in` before
`let run_id = ulid::ulid()`, and run directories are repo-scoped, so an identical run id minted by
a process on another repository names a different directory. No shared run directory or event log
follows from the collision as the code stands; that protection is the lock's and the layout's, not
the id's, and a caller that minted before locking, or a consumer that keyed on the id alone
(`IncarnationId`, `QuestionId`, the staging names), would not have it. The finding is that the
module does not meet its own stated requirement by construction; the impact is what a consumer
makes of it.

The realistic shape is not two fresh runs racing. It is a long-lived conductor that has minted
tens of thousands of question and incarnation ids — nonce past 2^15 — and a second process
minting at a low nonce, in the same millisecond, with pids in the XOR relation the nonces dictate;
two harnesses cannot supply it, since one harness ends at nonce 6,016 and counters do not combine
across processes. On a box running a dozen concurrent sessions, pids differ in the low bits all the
time; a single-session workflow never has two processes minting ULIDs in one millisecond with
adjacent pids, which is why this survived until 2026-09-05. Whoever fixes it should test against
that population, not a single process.

The module's "nothing cryptographic" caveat is not the issue: there is no adversary here. The
stated same-machine uniqueness requirement fails to arithmetic.

Found by PR #149's pass 4 (`gpt-5.6-sol`, on `8a24eba`) as the unarranged path to an `Occupied`
refusal in the settle kill tests' scratch fixture, whose names are these ULIDs; the fixture now
draws again on a refusal (`SCRATCH_DRAWS`), which the production sites do not.

## What the change that takes this up should do

Named without choosing: a wider or non-XOR-combined seed — the nonce and the pid must not be able
to cancel — or a uniqueness check at the run-directory boundary, where the exclusive create that
`rundir` already performs would turn a shared run id into a refusal instead of a shared log. Add
the seed-equal pair above as an exact-string test of `ulid_from_parts`, which the module's
observation seam already makes possible.
