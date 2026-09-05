---
id: SWEEP-ANNOTATION-003
severity: P3
disposition: deferred     # the site is sections.rs (row 67)
category: correctness
pr: 169
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/plan/markdown/sections.rs:83
provenance: pre_existing
first_bad:
guard: the module doc of `src/plan/markdown/annotation.rs` states that an inline opener without a closer is text; `the_marker_is_exact_and_leading`
---

## Failure sequence

An author writes the inline heading form and wraps it:

```
## Title <!-- upstroke: id=a
kind=fix -->
Body
```

Measured on pulldown-cmark 0.13.4: an inline `<!--` with no `-->` on the heading line is not
HTML, so the heading's events are `Text("Title ")`, `Text("<")`, `Text("!-- upstroke: id=a")`,
and the next line opens a paragraph beginning `kind=fix -->`. `split_sections` sees no
`InlineHtml`, the section's title becomes `Title <!-- upstroke: id=a`, its slug becomes the id,
`kind=fix -->` is the first line of the body, and nothing warns. The accumulator in
`annotation.rs` never sees any of it, so the unterminated warning this pull request added
cannot fire here.

## What the change that takes this up should do

In `split_sections`, when a heading's collected text contains `<!--` (or the marker
`upstroke:`) after the walk, warn that the heading holds an unclosed comment and that no
annotation applies; the section's title should probably be cut at the opener. A heading is one
line, so the fix is a text check on `HeadingScan::title`, not a reassembly.
