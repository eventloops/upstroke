# 2026-08-30 — run-directory deletion authority is token-carried, and there are two tokens

**Verdict.** Recursive deletion of a run-scoped directory tree stays token-carried,
and the token classes are now **exactly two**:

| token | authorises | minted by | build |
|---|---|---|---|
| `rundir::PrivateHalfProof` | one run's private half | `rundir::prove_private_half_ownership` | every build |
| `rundir::scratch_tree::ScratchTreeOwnership` | one test-minted scratch tree | `rundir::scratch_tree::acquire` | `cfg(test)` only |

`PrivateHalfProof` is unchanged: same constructor, same twelve conjuncts, same
fail-closed conjunct 12, same by-value spend in `remove_private_husk`. The
second token is added, `cfg(test)`-only, and is not an engine effect: it takes
no `RunDirSite`, adds no row to `effects/effect_sites.json`, and is censused
nowhere.

The P5b deletion boundary is **rescoped in wording, unmoved in force**: it
governs *run-lifecycle* paths — every path that reaches a run directory as a run
directory — and the sole thing outside them is the test build's scratch funnel,
which cannot name a run's private half at all.

## The problem this decides

`src/rundir.rs`'s test region built its scratch directories like this:

```rust
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("upstroke-rundir-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}
```

Three properties, and each one is a hazard:

1. **The root is predictable.** A tag and a pid. Two worktrees on one build box,
   or a pid reused across a reboot, land on the same path.
2. **It pre-cleans.** A collision is resolved by *deleting whatever is there*,
   before establishing any claim on it. The holder of that tree is not consulted
   and does not find out.
3. **The pre-clean is discarded.** `let _ =` means a removal that failed is
   indistinguishable from one that succeeded, so a fixture can be built on top of
   another run's residue and the disagreement surfaces somewhere else entirely.

The engine's own deletions have had an answer to this since PR5 —
`resource_accounting.completeness_rule`: "a private-half deletion outside the
proof-token funnel fails to compile" — and the test region, which is where the
proof's own fixtures are built, had none.

## Why not reuse `PrivateHalfProof`

Because it answers a different question, and making it answer this one costs a
live production guarantee.

`prove_private_half_ownership` mints its token from a marker, a reciprocal owner
record, a locator chain with no reparse point on it, and — conjunct 12 —
`<private>/committed.json` **proved absent**, where only `NotFound` is proof. A
test fixture that publishes a commit record is exactly the shape that proof is
required to refuse, and several tests exist to check that it does. So routing
fixture cleanup through that token needs one of:

* a second constructor, or public fields — which is what six compile-fail
  fixtures in `rundir::tests::build_refusals` exist to prevent; or
* a weakened conjunct 12 — the boundary itself; or
* a `Path::exists`-style absence oracle in place of the fail-closed stat, which
  reads "the filesystem declined to answer" as "the record is not there".

All three are rejected. The second token costs none of them.

## The authority model

**Acquisition creates what it binds.** `acquire(parent, tag)` builds
`<parent>/upstroke-scratch-<tag>-<ULID>` and creates it with a **non-recursive,
exclusive** `fs::create_dir`. "Previously nonexistent" is therefore decided by
the kernel's own exclusive create — not by a stat this code could lose a race to
— and the token binds the exact `PathBuf` that call created.

**Refusals refuse.** `AlreadyExists` is `Occupied`; every other error is
`Undecidable`. Neither deletes, moves or truncates anything, and neither retries
under another name. There is no pre-clean on any path.

**The name is not predictable.** The tag is for a human reading a leftover tree;
the ULID is what makes two holders unable to collide. A tag alone reproduces
hazard 1 above.

**Reclamation names only the token's root.** `remove_scratch_tree(token)` takes
the token **by value** and removes `token.path()`. There is no path-taking
variant, so a reclaim cannot be aimed at an ancestor, a sibling, or anything the
acquisition did not create.

**Only `NotFound` is success.** A reclaim succeeds on `Ok` and on `NotFound` —
the one absence a filesystem proves — and returns `ScratchReclaimFailure` for
every other answer, **carrying the token back** so the tree still has an owner
and the caller still has a handle.

**Guards reclaim on both exits.** `acquire` returns a `ScratchTree` guard whose
`Drop` reclaims on the normal return *and* on an unwind. A test that reclaimed at
the end of its body leaks on exactly the runs that matter — the failing ones —
and a suite that leaks a directory per failing fixture is how a build box runs
out of inodes with free space on the disk. A reclaim failure **panics** on the
non-unwinding path and is **suppressed, with a report on stderr, while already
unwinding**: a panic raised during an unwind aborts the process and would replace
the failing assertion's diagnosis with nothing. The result is matched in both
arms; `let _ =` and `.ok()` are prohibited, because they read the same on the
suppressed path and would make a leak silent on *every* path.

**Disarming obliges a fallback.** `ScratchTree::disarm` hands the token over and
consumes the guard. A witness that disarms in order to watch a reclaim fail must
`ScratchTree::rearm` **before it asserts anything**: between the two, a failing
assertion unwinds past a tree nothing will remove.

## Why deleting a scratch tree needs no proof about its contents

Because `acquire` creates the root, **nothing beneath a token root predates the
token**. Every byte under it was written after the token was minted, by whoever
holds the token. Exclusivity and the safety of removing the whole subtree are
therefore structural properties of the acquisition, not claims about the
contents — which is why the funnel does not stat for `committed.json` and must
not. A commit record inside a token root is a record the holder published
seconds ago; it is a fixture, not a run's deletion boundary.

## Completeness, in its two-token form

The rule the PR5 packet states as "a private-half deletion outside the
proof-token funnel fails to compile" is restated as:

> Every recursive deletion of a run-scoped tree is reachable only through a
> token. There are exactly two token classes. Neither can be forged, cloned,
> defaulted or spent twice, and neither can name a path its own minting did not
> bind. `PrivateHalfProof` authorises one run's private half and refuses on
> every `RetainReason`, `committed.json` included; `ScratchTreeOwnership`
> authorises one tree that the token's own acquisition created, exists only
> under `cfg(test)`, and can never name a run's private half — because it can
> only name a root that did not exist before it.

## The P5b boundary, rescoped in wording and unmoved in force

`RunDir.PublishCommitRecord`'s identity said "no path — creator or census —
deletes the private half". It now says **no run-lifecycle path**, with
run-lifecycle meaning every path that reaches a run directory as a run
directory: the whole of `engine::topology::create`, the startup census, and
`remove_private_husk`. The sole exception is the test build's scratch funnel,
which is on none of them.

Conjunct 12 is **unchanged and still fail-closed**. `PrivateHalfProof` is
unchanged. The two restatements live in `src/topology/effects.rs`
(`RunDirSite::PublishCommitRecord`) and `src/engine/topology/create.rs`
(the module's deletion-boundary section).

## Why the refusals are written differently from `PrivateHalfProof`'s

`PrivateHalfProof`'s forge/clone/default/spend refusals are fixtures compiled
against this crate's **rlib**. That harness cannot reach a `cfg(test)` type:
measured, a fixture naming `upstroke::rundir::scratch_tree::ScratchTreeOwnership`
fails with `E0433`, "could not find `scratch_tree` in `rundir`" — an unresolved
path, which would go green for a token that *was* `Clone`. A refusal that passes
for the wrong reason enforces nothing.

So the refusals move into the crate, where the compiler decides them on every
build of the test target:

* a `fn(ScratchTreeOwnership) -> Result<…>` coercion pins the parameter to the
  owned token, so the call spends it. Measured: a `&ScratchTreeOwnership`
  parameter is `E0308`.
* a second `clone` and a second `default` on the token make the std impls
  ambiguous the moment either exists. Measured: `#[derive(Clone)]` and
  `#[derive(Default)]` each produce `E0034`, multiple applicable items in scope.
  `Copy` is caught by the first, because `Copy` requires `Clone`.

## Scope boundary

**In.** The token, its guard, its refusals, its funnel and its witnesses, all
inside `src/rundir.rs`; the two boundary restatements; the `src/rundir.rs` review
paragraph in `effects/allowlist.toml`; this record.

**Out, deliberately.** No production funnel is aimed at an ancestor or a
non-site artifact. No docs-only "tests may delete ancestors" carve-out is added
anywhere. No topology module gains a raw-delete allow. `PrivateHalfProof` is not
forged, cloned, defaulted or reached by a new constructor. Conjunct 12 is not
weakened, and no `Path::exists` absence oracle is introduced — every absence in
the new code is observed through the same fail-closed predicate conjunct 12
uses. The site inventory, its counts and `effects/effect_sites.json` are
untouched.

**Deferred.** Migrating the existing `rundir::tests::scratch` helper and the
other predictable-path scratch helpers in this crate onto the token. This record
establishes the authority; the call sites move under it in a later change, so
that the mechanism is reviewed on its own before a wide mechanical diff is
written against it.

## Witnesses

| # | claim | where |
|---|---|---|
| 1 | an occupied root refuses and the occupant keeps its bytes | `an_occupied_root_refuses_and_leaves_what_it_found` |
| 2 | an undecidable root refuses, and the refusal modifies nothing | `an_undecidable_root_refuses_without_claiming_it_was_occupied` |
| 3 | reclaiming a root that is not there succeeds | `reclaiming_a_root_that_is_not_there_succeeds` |
| 4 | a reclaim removes the token root and nothing outside it | `a_reclaim_removes_the_token_root_and_nothing_outside_it` |
| 5 | an injected reclaim failure returns the error and the token | `an_injected_reclaim_failure_returns_the_token_with_the_error` |
| 6 | a spent token cannot authorise a second deletion, by API construction | `a_spent_token_cannot_authorise_a_second_deletion` |
| 7 | a tree holding a published `committed.json` is reclaimed by the scratch token while the ownership proof retains it as `PossiblyCommitted` | `a_scratch_tree_holding_a_committed_record_is_reclaimed_while_the_proof_refuses_it` |
| 8 | the allow-placement scan and the effect censuses are unchanged and green | `effects::tests` |

Three more, for claims this record makes that the eight above do not reach:

| claim | where |
|---|---|
| the guard reclaims on an unwind as well as on a normal return | `a_guard_reclaims_on_an_unwind_as_well_as_on_a_normal_return` |
| a reclaim failure on the normal path is raised, and the panic names the tree | `a_reclaim_failure_on_the_normal_path_is_raised` |
| a reclaim failure while already unwinding is suppressed rather than aborting | `a_reclaim_failure_while_already_unwinding_is_suppressed` |

The last of the three is witnessed by the process still being alive to run its
assertions: measured, replacing the suppression with a raise produces `thread
caused non-unwinding panic. aborting.` and takes the whole test binary with it.
The middle one exists because of a **measured surviving mutation** — with the
`panic!` replaced by the `eprintln!` the suppressed arm uses, every other witness
here stayed green.

Every absence assertion in them goes through `scratch_tree::proves_absent`, which
is conjunct 12's own predicate: only `NotFound` is proof. `Path::exists` is not
used anywhere in the module.

Witness 2 promises exactly two things — the acquisition **refuses**, and the
refusal **modifies nothing** — and it reads the second from the parent's own
directory listing, which every platform answers the same way. It deliberately
makes no claim about how a stat *beneath* the refused root classifies: what a
filesystem reports for a path under a file ancestor is platform-dependent, and
asserting on it made the witness non-portable. That is
`PR77-WIN-UNDECIDABLE-STAT-ORACLE`, validated by the CI arbiter — the Windows
guest maps that stat to `NotFound`, so the assertion held on Linux and failed
there. The finding is witness-and-prose only: the acquisition refuses and
preserves the occupant on both platforms, and the production authority is
unchanged.

The predicate's classification is owned by
`a_commit_record_stat_that_is_not_not_found_is_not_proof_of_absence`, which
asserts it directly over every shape the stat can produce — `NotFound` as the
one proof of absence, `PermissionDenied`/`Other`/`InvalidInput`/`TimedOut` as
answers the filesystem declined to give, and a successful stat as presence — and
then wires the two reachable shapes through the whole ownership proof. A witness
about acquisition is the wrong place to re-derive it, and doing so is what
introduced a platform dependency the classification test does not have.

## Inputs

* `decisions.resource_accounting.completeness_rule` and
  `decisions.effect_site_inventory.identity` (the PR5 packet), live.
* [2026-08-30 — readiness lint placement](2026-08-30-readiness-lint-placement.md),
  for the module-tree lint-scope reading the allowlist paragraph cites: the
  scratch module is **inline**, so the file-scope allow recorded for
  `src/rundir.rs` is its level by the module tree, and it carries no attribute of
  its own.
