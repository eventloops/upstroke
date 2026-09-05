---
id: SWEEP-AMBIENT-012
severity: P3
disposition: deferred
category: correctness
pr: 147
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/agent/proc.rs:4199
provenance: pre_existing
first_bad:
guard: the sweep of `src/agent/proc.rs` (queue row 51), which owns `termination`'s `resolve_reaper_program` and `list_labeled_containers`; `find_program` in `src/util.rs` is the shared resolver and is also `DockerCli::available`'s
---

## Failure sequence

`set_container_reclaim_scope(Some(&scope))` with a bare program name resolves it through
`resolve_reaper_program` → `crate::util::find_program`, which walks `PATH` and returns the first
entry whose join with the name `is_file()` -> a **regular file that is not executable** — a
mode-0644 file named `docker` on `PATH` — satisfies that test, so arming succeeds and reports
nothing -> when the coordinator dies, the reaper's `list_labeled_containers` forks, `execv`s that
path, and the child `_exit(127)`s; the pipe carries no bytes; the reaper reads zero bytes as an
**empty listing**, which is the loop's "everything this selector names is gone" exit -> the
coordinator's labeled containers survive, and the reaper reports the same success it reports on a
clean machine. An inspection that could not be performed is presented as the negative answer, the
same class as `read_dir_names` folding a failed listing into `[]` (`SWEEP-CLASSIFY-009`) and the
`.ok()` fold of `SWEEP-AMBIENT-010`; §7's stricter treatment is triggered here exactly because the
inputs are filesystem state and the environment.

The inputs to arming are therefore wider than `PATH`: `find_program` joins a **relative** `PATH`
entry to the name and returns the result unchanged, so the working directory at fork time decides
what the reaper execs; and the filesystem's metadata at the moment of the probe decides which entry
wins. Neither is refused at the boundary with an error channel.

## Measured

At `7150ea9`, an uncommitted probe inside `termination`'s test module, run alone under `--exact`,
tree restored afterwards:

```
PROBE012 armed-with-0644=true resolved-0644=Some("/tmp/upstroke-probe-012-2938059/upstroke-probe-012-docker") resolved-via-relative-entry=Some("upstroke-probe-012-2938059/upstroke-probe-012-docker") relative=true
```

(a) a mode-0644 stub on an absolute `PATH` entry: `set_container_reclaim_scope` returned `Ok` and
`resolve_reaper_program` returned the stub's path; (b) the same stub reached through a relative
`PATH` entry from its parent directory: `resolve_reaper_program` returned a relative path. The
reaper-side half — `execv` on the 0644 file failing and the empty listing being read as "no
containers" — is read from `list_labeled_containers` and `resolve_reaper_program`'s own doc ("the
listing child `_exit(127)`s, the pipe carries no bytes, and the reaper reports exactly the same
success it reports on a clean machine"), not run here.

P3 because reaching it needs a `docker` that ran containers earlier in the run — `DockerCli`
invokes it through `Command`, which fails loudly on a non-executable file — and then stopped being
executable before the coordinator died; the same binary-changes-under-a-running-coordinator class
as `SWEEP-AMBIENT-010`.

## What the change that takes this up should do

At arm time, refuse a resolved program that is not an executable regular file (Unix: any execute
bit, or `access(X_OK)`), and canonicalise it to an absolute path before storing (which also closes
`SWEEP-AMBIENT-010`'s working-directory half). Whether that check belongs in `find_program` — where
`DockerCli::available` would inherit it — or only in `resolve_reaper_program` is row 51's call;
the reaper side cannot be repaired, since a fork-only child has no error channel, which is why the
arm-time check is the only place the refusal can live.
