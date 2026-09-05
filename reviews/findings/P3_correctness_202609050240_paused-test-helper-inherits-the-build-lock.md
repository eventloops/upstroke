---
id: PR154-PROC-PAUSED-HELPER-INHERITS-DESCRIPTORS
severity: P3
disposition: deferred
category: correctness
pr: 154
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/agent/proc.rs:4657
provenance: pre_existing
first_bad:
guard: the sweep of `src/agent/proc/tests.rs` and the test support of `src/agent/proc.rs` (queue row 50); `spawn_group_anchor` at `src/agent/proc.rs:2709` is the crate's own correct handling of the same shape
---

## Failure sequence

**Established from the tree.** `sigchld_reaper_host_helper` (`src/agent/proc.rs:4643`, a
`#[ignore]`d subprocess helper the SIGCHLD reaper tests re-exec) forks a target at `:4657` that
calls `setpgid(0, 0)` and then `loop { libc::pause() }` at `:4661`, **without closing the
descriptors it inherited** -> every open file description of the test binary at the moment of
the fork — a build system's lock file among them, when the suite runs under a wrapper that holds
one — is held by a process that parks forever by design -> the test that abandons it is measuring
exactly that abandonment, so nothing in the suite reaps it, and the inherited descriptions stay
open until something outside the suite kills the process.

The crate names this class in two places and violates it here. `cleanup::take`
(`src/rundir.rs:1486`) takes its probe lock and releases it at once, with the reason at `:1509`:
"Do not retain the lock in the conductor: arbitrary forked children would inherit its open file
description and recreate the false-liveness window the primary fcntl lock deliberately avoids."
And the production anchor with the same shape as this helper, `spawn_group_anchor`
(`src/agent/proc.rs:2700`), calls `close_inherited_fds(&[], open_max)` at `:2709` before its own
`libc::pause()` at `:2713`. Those are the tree's only two `pause()` sites; exactly one closes what
it inherited, and it is the production one.

**Observed, second-hand and unconfirmed at this location.** On the build box on 2026-09-05 a
process sitting in `__do_sys_pause` held the `flock` on `/mnt/ramtarget/slot1` — 7.2G, the
largest slot in the `upstroke-build` pool — from 2026-09-04 09:22 for about seventeen hours with
nothing written to it, taking that slot out of the pool for every worktree on the box until it was
killed, after which a live build claimed the slot within seconds. The session that observed and
killed it attributed it to a test name that exists nowhere in the tree, so the attribution to this
helper is by mechanism (the only unclosed `pause()` site), not by observation. **What would
confirm the link**, for the next person who finds a slot held with nothing building:
`ls -l /proc/<pid>/fd` of the holder showing the slot's lock file, or its argv showing this test
binary with `sigchld_reaper_host_helper`; either settles it in one command.

It surfaced from a load investigation — the lock queue on the box — rather than from any review
of the file, which says where this class hides: a descriptor leak into a process that is meant to
be leaked is invisible to a reader of the test, because leaking the process is the behaviour under
test.

The operator-visible consequence is a build slot held indefinitely; on a shared box that is the
pool degrading, and the sweep's own gate throughput was affected on the day it was found.

## What the change that takes this up should do

One of two, and the finding names both without choosing: call `close_inherited_fds` (or set
`CLOEXEC` on what the helper inherits) in the forked target before it parks, the way
`spawn_group_anchor` does at `:2709`; or reap the target at the end of the test that abandons it,
so nothing outlives the binary. Either keeps the behaviour under test — a stopped, parked helper
the parent never waits on — while stopping it from carrying the harness's file descriptions.
