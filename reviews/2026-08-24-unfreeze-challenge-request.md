# Adversarial review request — is a `src/topology/**` reader a breach of the 2026-08-20 ruling?

**Status: a request for critique, not a decision.** Written by the PR7 implementer, addressed to an
independent reviewer. It is deliberately written **as advocacy** — the strongest honest case its
author can make — so that a critic has something solid to attack.

**Attack the strongest form.** If a claim is badly phrased but its point survives a better phrasing,
repair the phrasing and attack that. A critique that wins on a sloppy sentence has told the owner
nothing.

**Verify rather than accept.** Every load-bearing claim below is reproducible from this repository
alone, at `slice/pr7`. Several are counts you can check in one command. If any is wrong, that is the
most useful thing you can report.

> The design packet (`tactus-parallel-design-neutral-v16.json`, 645 KB) is **not in this repository**
> — it is private. Claims 2, 4 and 5 need no access to it. Claim 3 is verifiable from
> `src/topology/events.rs` alone. Where the packet is quoted, the quote is reproduced inline and you
> should treat it as the implementer's transcription, flagging anything that looks self-serving.

---

## The mechanism this request uses

This project has a procedure for exactly this, and this request follows it rather than inventing one.
`reviews/FINDINGS.md`, "The authority rule":

> **The implementer holds the disposition.** A reviewer may not overturn one. A reviewer *may* append
> a challenge to a settled entry, and should when it has something the original disposition did not
> consider. A challenge is only admissible with **new evidence**: a **concrete failure sequence** the
> disposition did not address, and a **surviving mutation** — a specific edit the current suite would
> not catch. […] A restatement of the original finding is not a challenge. Neither is a preference:
> **where the design is frozen, an equally valid alternative is not a defect.**

So the bar is explicit, and the first thing worth your scrutiny is whether this request clears it or
is a preference wearing a challenge's clothes.

## The ruling being challenged

Not reachable as a commit from this branch. Its text is in `reviews/FINDINGS.md` §2, on the rows
`PR4-SPAWN-SITE-PROBE-CONTEXT` and `PR4-PROGRAM-PATH-NOT-UNICODE`:

> **OWNER RULING, 2026-08-20: the frozen files stay frozen.** PR4 does not change
> `src/topology/effects.rs` or `DESIGN.md:222`. This is an **accepted deviation**, not an open
> question and not a defect to be repaired in this slice: the repair requires editing a file an
> earlier slice froze, and **a slice may not quietly redesign what it implements**. **Revisit at G2**
> if it is raised repeatedly there. […] a reviewer may still append a challenge in §3, but only with
> evidence the ruling did not consider, and **'a live passage is violated' is not new evidence: that
> is the fact the ruling was made about.**

The two repairs it refused were a **new enum variant** in `effects.rs`, and **widening**
`CommandSpec.program` from `String` to `OsString`.

## What is being asked

Whether a slice may add a **public reader** to a file in `src/topology/**` — a method that delegates
to logic already present in that file, changes no behaviour, adds no variant, widens no type, and
deletes nothing.

---

# The argument

## Claim 1 — the ruling names two files, and neither is the one at issue

The ruling's own sentence is *"PR4 does not change `src/topology/effects.rs` or `DESIGN.md:222`."*
Read literally, it froze **two named things**.

The broader reading — that all of `src/topology/**` is frozen — appears in the tree as a later
**gloss**, in `reviews/FINDINGS.md` §11 ("PR5 — a frozen file changed, and why that is not a breach
of the ruling"), which opens: *"`src/topology/registry.rs` is modified by this slice (+56/−15).
`src/topology/**` is slice PR3's code, and the owner ruled on 2026-08-20 that the frozen files stay
frozen."*

**This is the first thing the advocacy wants tested, and it may dissolve the whole request.** If §11's
gloss is the operative rule, this is a challenge. If the ruling means what it says, `fold.rs` — the
only file PR7 touches — was never frozen by it, and the implementer has spent a slice avoiding a
constraint that does not exist. The advocacy's author suspects the latter and reports it against
their own interest.

## Claim 2 — the measured cost, across the largest slice in the project

```
git diff --stat 615597c -- src/topology/
```

Nine of ten files in `src/topology/` are **untouched** by PR7. The entire footprint is `fold.rs`,
**+652 / −0**:

| | lines |
|---|---|
| test code | 474 |
| doc comments and blanks in production | 127 |
| **production code** | **51** |
| deletions, anywhere in `src/topology/` | **0** |

Those 51 lines are **ten `pub fn` readers**, each a one-line delegation to an existing private
`RunState` predicate with a poison guard, plus **one** conjunct — `&& self.pipeline_reservable()` in
`integration_admissible` — which repaired a measured breach of `max_parallel` at width 1.

## Claim 3 — the vocabulary is already complete for the rest of v0.2

Check `src/topology/events.rs`: `MergePrepared`, `MergeRejected`, `TaskMerged`,
`MergeVerificationStarted`, `MergeVerificationInterrupted`, `MergeVerificationUnavailable`,
`TaskSpawned`, `CreatesLineage`, `InheritedLineage`, `WidensLineage`, `RunEnding` and `RunFinished`
are all present, with their effect sites in `effects.rs`.

PR8's merge queue and PR9's repairs therefore need **no new variants**. The category the ruling
actually forbade is not blocking any remaining slice.

## Claim 4 — the new evidence: refusing a reader produced a defect, with a surviving mutation

This is the claim that has to clear the authority rule's bar, and the one most worth attacking.

**The failure sequence.** `fold::predicted_region` (`src/topology/fold.rs`) derives a task's predicted
region from its plan hints — stripping glob metacharacters to a literal prefix, normalising
backslashes, trimming trailing separators, classifying an unbounded hint repo-wide. It is **private**,
and it is **load-bearing**: `dispatch_lease_check` calls it to decide whether a task is `ready` at
all, by computing the region and asking the lease table what that region overlaps.

PR7's dispatch branch needs the same region, to record in `task_dispatched`. Rather than expose the
existing derivation, a second one was written in the engine. It took the hints literally. For the
test fixture's own hint:

```
hint "src/alpha/*.rs"    the fold admits the dispatch on prefix   src/alpha
                         the log records                          src/alpha/*.rs
```

and the lease table keeps the **log's**. A prefix containing `*` overlaps nothing, so the predicted
lease protects nothing. For `**/mod.rs` the fold classifies repo-wide — admissible only when nothing
is held — while the literal version records a prefix that collides with no one, so the next task is
admitted against a lease that should have blocked it. Two tasks then edit the same files.

Invisible at `max_parallel = 1`, where one generation runs at a time. Live at the first width above
one, which is PR11 — by which time the dispatch that wrote it is many slices old.

**The surviving mutation.** It is not hypothetical: the second derivation **shipped green**, in
`199dc1d`, past the full suite and every gate.

```
git show 199dc1d:src/engine/topology/run.rs | grep -n "fn predicted_region" -A 12
```

Repaired in `84a3978` by adding the reader and deleting the engine's copy. The regression test
compares the recorded region against **the fold's answer**, not against a literal — a literal
expectation would agree with whichever derivation the test itself used.

**Why this is not "a live passage is violated".** The ruling pre-empts that argument by name, and
rightly. This is a different claim: the freeze did not protect the vocabulary, it produced a **second
authority** for a rule the vocabulary already owned. Duplication is this project's dominant defect
class — PR7's own review rounds found three implementations of one append-error protocol, two barrier
witnesses, two run-directory censuses, and two disagreeing implementations of one retry rule.

## Claim 5 — the rule proposed

> A slice may add a **public reader** to `src/topology/**` that delegates to existing logic and
> changes no behaviour. Anything that adds a variant, widens a type, or changes a value requires a
> decision record.

Applied retroactively it would have permitted every edit PR7 needed, forbidden both edits the owner
reverted during this slice, and prevented the defect in Claim 4.

---

# What is **not** claimed

- **Not** a "cleanup pass to make the design more concise." Nothing above supports that. Cleanup is
  unbounded, and the freeze's clearest value is making unbounded work impossible to start by accident.
- **Not** any category-C change. Nothing needs one.
- **Not** that the three findings carried in `reviews/FINDINGS.md` §2 for the post-v0.2 pass should be
  repaired now. That disposition stands.

---

# Objections already known

Listed so you do not spend effort rediscovering them. A critique that raises only these has added
nothing.

1. **Claim 1 may dissolve the request.** If the ruling froze two named files, `fold.rs` was never
   frozen, there is nothing to challenge, and the correct finding is that §11's gloss over-broadened
   a narrow ruling — a documentation defect, not a design one.
2. **The rule may fix nothing that judgement would not.** A reader was arguably always permissible.
   The Claim 4 defect came from an implementer being *over-cautious*; codifying category A may
   relabel a judgement call and hand future slices a category to argue into.
3. **Slippery slope, and not fallaciously.** Every category-C change can be introduced as "a reader
   that needs one more field". Who adjudicates, at what cost per pull request?
4. **A bright line is worth something a categorised rule is not.** "Do not touch these files" needs no
   interpretation; "readers are fine" needs one every time.
5. **One slice is one data point.** PR7 is the first to drive the vocabulary. PR8 and PR11 may want
   category C for reasons PR7 cannot see.
6. **Ten readers is a smell about the original design, not a licence.** If a consumer needs ten new
   public methods to use a type, the honest response may be that `TopologyFold`'s surface was drawn
   wrong — which is a redesign, i.e. the thing the ruling forbids.
7. **+652 lines on a frozen file is not nothing**, whatever the split. "Mostly tests and doc comments"
   is the argument every large diff makes.

---

# What to return

1. **A verdict on Claim 1 first**, because it may end the matter. Does the ruling freeze two files or
   a directory? Quote it against the advocacy if it deserves it.
2. **Verification of Claims 2–4**, naming any number you could not reproduce. Claim 4 is load-bearing:
   check that the two derivations really disagree and that the consequence is real rather than
   dramatised.
3. **Whether Claim 4 clears the authority rule's bar** — a concrete failure sequence the disposition
   did not address, *and* a surviving mutation — or is a preference in a challenge's clothes.
4. **The strongest objection not listed above**, if one exists.
5. **A recommendation with a condition**: accept, accept-with-a-narrower-rule, or reject — and if
   accept, what would have to become true to revisit it.
6. If the honest answer is *"the freeze is fine and the implementer should have used better
   judgement"*, say so. The advocacy's author considers it the single most likely way this argument
   is wrong.

---

## Postscript, 2026-08-24 — adjudicated

**This request is closed.** The Status line above says it awaits adjudication; it no longer does.

The project owner ruled on 2026-08-24, after an independent adversarial review of `3c09f6e`. The
ruling is recorded in `reviews/FINDINGS.md` **§3**, first entry, which is authoritative — this
postscript points at it and does not restate it.

In outline: PR7's `fold.rs` footprint is **accepted as a disclosed deviation** through `3362f65`; the
**standing rule proposed by Claim 5 is rejected**, and `frozen_rung_binding` is the last fold reader
outside a dedicated pass; a **freeze charter** replaces the ad-hoc reading, with the deferred work
scheduled as a **G2 PR3-layer pass** running after PR7 merges and before PR8.

Appended rather than rewritten, per this project's convention for settled records: the argument above
stands as it was filed, including the parts the adjudication did not accept, so that a later reader
can see what was argued and not merely what was decided.
