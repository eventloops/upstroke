# Internal module notes

Extended notes for the crate, one file per module, mirroring `src/`.

This directory holds a module's **whole prose**: its API contracts, module orientation, historical
rationale, worked examples, and the records of what an earlier shape got wrong. It exists because
that material is valuable and dense enough to fill a reader's attention — human or agent — before
they reach the code. Moving it here makes loading it a choice.

Where a module has a notes file, its source keeps one header pointer and the
site-required exceptions listed below. Other rustdoc and inline comments move to
the notes. `CODING_STANDARDS.md` §13 is the rule.

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

Every file opens with a visible backlink, using its own module and relative path:

```markdown
# `src/runner/host.rs`

Extended notes for [`src/runner/host.rs`](../../../src/runner/host.rs).
```

The opening paragraph may follow an H1 and blank lines. The H1 may include a
description, and ordinary prose may follow the backlink on the same line.
The generated wording is an example; a `[Source](relative/path.rs)` link also
satisfies the same contract.
The recognized link label is plain text or a single code span closed inside its
brackets. The opening is a paragraph; lists, blockquotes, and code blocks do not
supply it, even when their source contains link-like text.

The repository-relative link works in a checkout and on GitHub. A separate
`Source on GitHub` link points to the module's GitHub page for readers on
upstroke.rs, whose published `/docs` tree does not contain `src/`.

Code fragments in section headings are spelled as in source. Search each
backticked fragment separately; the enclosing item distinguishes repeated lines.

## Getting from the code to a note

One marker, in the module header, and nothing else:

```rust
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
banners. By default a notes file is a faithful dump in source order: `## Module` for the `//!`
block, then one section per comment headed by the line of code it sat above, with its enclosing
item — `` ## `impl HostRunner` › `pub fn new() -> Self {` `` — so the heading is the grep string
that finds the code. The pilot's `runner/host.md` is curated further, an *Item contracts* table
first and the rationale after, and any file may be reorganised or reworded like other
documentation; the source-order dump is the floor, not the ceiling.
Censuses that read prose were pointed at the notes or re-expressed rather than kept in the source:
`runner::container::tests` reads the orphan-window documentation from
`docs/internals/runner/container.md`; `events::log::tests` lifts its `compile_fail` build-refusal
fixtures from `docs/internals/events/log.md`; and the two comment-strip floors in `agent::tests`
and `runner::host::tests` plant a control line and assert it is removed instead of counting live
comments. No prose stays in the source for a census's sake either: the readiness module's
per-site allowance statement, which `effects::tests` and `runner::container::tests` require on a
line of its own, moved with the rest, and those two censuses now read it from
`docs/internals/agent/proc/test_support/readiness.md` and from that module's row in
`effects/allowlist.toml`. A comment can also be holding a lint at bay — `clippy::collapsible_if`
fired once when a comment between two `if`s was removed — and the code, not a comment, is the fix.


Four things do not move, because they are not this standard's to place:

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

`.github/scripts/test-internals-notes.sh` runs in CI's `lint` job (the Ubuntu one, with the other
five Bash gates) and holds the two trees to each other in both directions:

- every marker is spelled exactly `//! Extended notes: \`docs/internals/<module>.md\`` — no
  anchor, no prose, no other comment form — names the notes file that its own module's path
  derives, and that file exists;
- every notes file mirrors a live module, and that module carries exactly one marker, so a module
  that loses its marker is caught from the notes side;
- every notes file opens with the Markdown backlink shown above, optionally
  following an H1 and blank lines, and its relative target resolves to its module;
- a module carries at most one marker, above its first line of code.

An absent `docs/internals/` is a failure, never "nothing to check": with markers in `src/` it is a
deleted notes tree, and with none it is a gate measuring nothing. Each check has been broken on
purpose and watched fail.

The gate checks that opening format rather than parsing arbitrary Markdown.
Hidden links, plain paths, code examples, and images do not satisfy it. Its
isolated fixtures exercise valid depths and CRLF, malformed backlinks, missing
files, and misplaced or duplicate markers.

The gate does not check section headings, arbitrary source prose, or whether a
note remains true. Those are review duties under §13, including its site-required
exceptions. §4's rule that a stale comment is a defect also applies to notes.
