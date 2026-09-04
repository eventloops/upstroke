# Internal module notes

Extended notes for the crate, one file per module, mirroring `src/`.

This directory holds the **expository** material that used to sit in source comments: module
orientation, historical rationale, worked examples, and the records of what an earlier shape got
wrong. It exists because that material is valuable and dense enough to fill a reader's attention —
human or agent — before they reach the code. Moving it here makes loading it a choice.

It is **not** a second source of truth. `DESIGN.md` is the living authority for product design and
`CODING_STANDARDS.md` for implementation; a note here that disagrees with either is a defect in the
note. The code is the authority for what the code does.

## Layout

One file per Rust module, at the source path with `src/` replaced by `docs/internals/` and `.rs`
replaced by `.md`:

| Source | Notes |
|---|---|
| `src/runner/host.rs` | `docs/internals/runner/host.md` |
| `src/runner/host/naming.rs` | `docs/internals/runner/host/naming.md` |
| `src/effects.rs` | `docs/internals/effects.md` |

A module with children gets a file and a sibling directory, exactly as the source does. A module
with no note has no file; absence means nothing was moved, not that nothing is known.

## Getting from a note back to the code

Every file opens with a link to the module it describes. Every section is headed by the item it
belongs to, spelled as it is in the source, so the section heading is the grep string that finds
the code.

## Getting from the code to a note

Where prose was removed, the source keeps a marker on one line:

```rust
//! Extended notes: `docs/internals/runner/host.md`
```

and, for a note about one item rather than the module:

```rust
    // Extended notes: `docs/internals/runner/host.md#hostrunnerresolved`
```

The path is repo-root-relative so it means the same thing from any file, and it is plain text
rather than a Markdown link so that `rustdoc` does not try to resolve it as an intra-doc link.
`grep -rn 'docs/internals/' src/` is the full inventory of markers.

## What lives here and what stays in the source

The dividing question is **adjacency**: *if the code beneath this comment changed, would this
comment have to change?*

**Yes — it stays in the source.** Reasoning a standard requires at its site: why a lock is held or
a value cloned (§6), why a `?` or a refusal is deliberate (§7), a `SAFETY:` obligation (§11), a
concurrency protocol (§10). The `# Errors`, `# Panics` and `# Safety` sections of a public item
(§13). The one-sentence statement of what an item is and what it guarantees. A platform constraint
that explains the line under it. The citation of an invariant that governs the adjacent code —
`INV-18`, `DESIGN.md:612` — even when the argument behind it moves here.

**No — it moves here.** Module-level orientation essays. Historical rationale: what an earlier
shape was, what a review round found, what a past pull request got wrong. Worked examples and
enumerated alternatives. Records of evidence that is not a test — "these four forms were each
planted and each rejected". Explanation repeated across modules that is better stated once.

A comment that is doing both is split: the load-bearing sentence stays, the essay moves, and the
marker joins them.
