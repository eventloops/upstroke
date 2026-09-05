---
id: SWEEP-ANNOTATION-017
severity: P3
disposition: deferred     # the site is the parent, src/plan/markdown.rs (row 68)
category: correctness
pr: 169
reviewed_sha: 5e41ca5d8e4e084b8d32ea1266e381be21e20a21
location: src/plan/markdown.rs:67
provenance: pre_existing
first_bad:
guard: the `design/09` sentence limiting the warning claim to a plan that parses; the module doc of `src/plan/markdown/annotation.rs`
---

## Failure sequence

A checklist plan carries an annotation before its list and the list holds only plain bullets:

```
# Notes
<!-- upstroke: id=orphan -->
- just a thought
```

`checklist_drafts` warns "upstroke annotation outside any checklist item; ignored" into the
warning vector, then produces no draft because plain bullets are not tasks, and
`parse_with_warnings` returns `UpstrokeError::Parse` ("no tasks found") — dropping the vector.
`upstroke validate` prints the parse error alone. The author never learns that the annotation
they wrote bound to nothing; every warning gathered before the refusal is lost the same way.
Found by pass 1 on PR #169 (finding 5), which had claimed every warning reaches the author.

## What the change that takes this up should do

The session that takes row 68 (`src/plan/markdown.rs`) owns this: make the warnings gathered
before a no-tasks refusal reach the author, then extend the `design/09` sentence that now says
they reach the author "when the plan parses".

Carry the gathered warnings with the refusal: either an `UpstrokeError::Parse` variant (or a
field) that holds them so `validate` can print both, or a `Parsed` shape whose task list may be
empty and whose caller decides. The site is `parse_with_warnings` in `src/plan/markdown.rs`,
row 68 of the queue; `design/09` now says warnings reach the author when the plan parses, and
should say more once they survive a refusal.
