# Internal module notes

Extended notes for the crate, one file per module, mirroring `src/`.

This directory holds a module's **whole prose**: its API contracts, module orientation, historical
rationale, worked examples, and the records of what an earlier shape got wrong. It exists because
that material is valuable and dense enough to fill a reader's attention — human or agent — before
they reach the code. Moving it here makes loading it a choice.

Where a module has a notes file, its source carries **no rustdoc and no inline comments**, only a
single pointer in the module header. The code is expected to read for itself; the notes are the
backup for the context the code cannot carry. `CODING_STANDARDS.md` §13 is the rule.

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

One marker, in the module header, and nothing else:

```rust
//! The host runner: `host-v1`.
//!
//! Extended notes: `docs/internals/runner/host.md`
```

The path is repo-root-relative so it means the same thing from any file, and it is plain text
rather than a Markdown link so that `rustdoc` does not try to resolve it as an intra-doc link.
`grep -rn 'Extended notes:' src/` is the full inventory — one line per module that has notes.

There are no per-item markers. A reader who opens the notes reads the file; a reader who opens the
code reads the code. Splitting the pointer across every item costs tokens on every read to save a
scroll on a few, and `.github/scripts/test-internals-notes.sh` refuses a second marker for that
reason.

## What moves

Everything. A module that gets a notes file gets *all* of its prose moved: the module essay, every
item's rustdoc including its `# Errors` and `# Panics` sections, every inline comment, the section
banners. The notes file opens with an *Item contracts* section so the API documentation is still
one place, and carries the rationale below it.

Three things do not move, because they are not this standard's to place:

- a `SAFETY:` obligation (§11), which belongs against the `unsafe` block it discharges;
- a concurrency protocol (§10), where the type cannot carry it;
- an `#[expect(...)]` reason string, which is an attribute rather than a comment;
- the **allowlist-placement marker** above a module-level `#![allow(...)]` of a governed lint.
  `src/effects/`'s census reads that comment: it requires the words `effects/allowlist.toml` and
  the section name (`funnel section`, or `LEGACY-EFFECT`) directly above the attribute, so the
  comment is governance machinery rather than prose and deleting it fails
  `every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist`. Keep the marker to those
  required words and move the explanation.

Where one of these has to stay, it stays, and the notes file says so.

## Keeping them honest

`.github/scripts/test-internals-notes.sh` runs in CI's `lint` job. It checks that every marker
names a notes file that exists and an anchor some heading generates; that every notes file mirrors
a live module and links back to it resolvably; that a module with notes carries exactly one marker,
in its header; and that the marker spelling is uniform. It refuses to pass with zero markers or
zero notes files, so it cannot go quietly inert.

What it cannot check is whether a note is still *true*. That is a review duty, and §4's rule that a
stale comment is a defect applies to a stale note in the same way.
