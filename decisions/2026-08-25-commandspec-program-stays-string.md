# 2026-08-25 — `CommandSpec.program` stays `String`

**Verdict: no change.** `DESIGN.md:222` keeps `program: String`, and
`PR4-PROGRAM-PATH-NOT-UNICODE` is **closed as not reproducible in production**.

The repair the standing ledger row proposed — widening `program` to `OsString`, scheduled to
a workstream of the G2 pass — is **withdrawn**. It rested on a premise about production that
the tree does not support.

## The premise that failed

The finding, and this record's first verdict, both rested on one sentence: *a resolved
agent-binary path that is not valid Unicode is refused today, and the only cause is the
struct's type.* The refusal is real —
[`Invocation::spec`](../src/agent/bin.rs) returns `Refused` when `self.path.to_str()`
is `None`. What is not real is a production path to it.

- **`Invocation::at` is `#[cfg(test)]`**, and its own doc says so in as many words:
  *"Production's only constructor is `Invocation::named`, whose argument is a bare CLI
  name."* Its two call sites are both inside test modules.
- **`Invocation::named` takes a `&str`.** A bare name is valid Unicode by construction, so
  `to_str()` cannot return `None` for an invocation production built.
- **The runner says the same thing about the field.** `runner/mod.rs` states that
  *"A `String` was always wide enough — `DESIGN.md:222` freezes `program: String`, and a
  name fits where a path may not"*, and names `PR4-ADAPTER-RESOLVES-ON-THE-HOST` as the
  entry with `Invocation::named` as the repair.
- **A non-Unicode installation directory is handled natively and never reaches the field.**
  `runner/host.rs` splits `PATH` with `std::env::split_paths` over an `OsStr` and joins the
  bare name to each entry, producing a `PathBuf`. Nothing writes that resolution back into
  `CommandSpec.program`.

So a CLI installed under a non-UTF-8 directory is found and executed **today**. The refusal
fires only for an `Invocation` carrying a non-Unicode path, and only a test can construct
one.

## What the conflict amounts to on the production route

`CODING_STANDARDS.md` §1 records a conflict between two live passages: `DESIGN.md:222`
freezing `program: String`, and §8 requiring OS-native types for paths. **On every route
production takes, the field carries a bare CLI name** — which §8 does not govern and which a
`String` represents exactly — so the conflict has no reachable instance today.

**That is not a finding that the field cannot hold a path, and §1's retirement must not say
it is.** The boundary is path-capable by contract: `src/runner/host.rs:828` turns on whether
`program` is *"a name for this boundary to resolve, rather than a location to use as given"*
and hands a location to `Command` byte for byte, and the retained test at
`src/agent/bin.rs:496` asserts `/usr/local/bin/claude` in `fine.program`. §8 therefore
governs this field the moment a path-valued input exists. What closes the finding is that no
production constructor supplies one; making the name-only reading a property of the *type*
would be a `DESIGN.md` change, and `DESIGN.md:222` says nothing of the kind.

## Consequences

- **`DESIGN.md:222` is unchanged**, and no compressed edit is owed at any workstream.
- **W4's widening mandate is withdrawn**, together with the three items that depended on it:
  the type change, the `DESIGN.md` edit, and the replacement of
  `a_program_path_a_string_cannot_carry_is_refused_by_name`.
- **That test stays, and its subject is now correctly described.** It exercises a state
  production cannot construct, which is what a defensive refusal is for: if a path-valued
  constructor is ever added, the boundary fails closed rather than spawning a lossily
  converted path that names something else. Deleting it would remove the guard that makes
  adding such a constructor safe.
- **§1's "Known conflicts at adoption" block is owed a retirement**, no longer as cargo on a
  spec edit that is not happening. It retires on its own motion, by its own pull request to
  `master`, recording that the conflict has no reachable instance on the production route and
  that the boundary's path-capable contract is what a future path-valued input would meet.

  **That block is now inaccurate, where a ruling of 2026-08-28 found it merely incomplete.**
  It says the conflict "is unresolved", names `PR4-PROGRAM-PATH-NOT-UNICODE` "the open owner
  question", and gives W4 as the decision venue. None of the three is true after this record.
  The earlier ruling was right about the document as it then stood; this is a supersession by
  later evidence rather than a reversal, and it raises the retirement from tidying to a
  correctness repair.
- **`PR4-PROGRAM-PATH-NOT-UNICODE` closes** as not reproducible in production. The standing
  ledger carries the superseding disposition; this record carries the reasoning.

## Rejected

**Widening `program` to `OsString` anyway**, on the ground that it costs little and removes
a representational limit. Rejected: it changes a frozen `DESIGN.md` line, mandates a
schema-adjacent edit and a test deletion, and buys no production behaviour — the record's
own "no speculative widening" scope forbids exactly that trade.

**Re-grounding the widening on a CLI named by path.** Rejected on evidence: no production
constructor takes a path. Making that route real would mean adding a path-valued adapter or
configuration input, which is a new feature and outside anything this record or that
workstream was chartered for.

**Re-grounding it on host-side resolution**, as an earlier revision of this record did.
Rejected: that reinstates `PR4-ADAPTER-RESOLVES-ON-THE-HOST`, contradicts `DESIGN.md:117`'s
data-only `CommandSpec`, and breaks the container case outright — a CLI that exists only
inside the image cannot be resolved on the coordinator.

## Measured vs assumed

**Measured**, by reading the tree at this pull request's head: `Invocation::at`'s
`#[cfg(test)]` attribute and its doc sentence; that its call sites are in test modules;
`Invocation::named`'s `&str` parameter; `Invocation::spec`'s refusal condition;
`runner/mod.rs`'s "a `String` was always wide enough"; and `runner/host.rs` splitting `PATH`
as an `OsStr`.

**Assumed**: nothing. The withdrawn repair's central claim was assumed and is what failed.

**Not measured, and deliberately**: whether any future adapter will want a path-valued
program. If one does, it arrives with its own decision, and the refusal this record
preserves is what makes that arrival safe.
