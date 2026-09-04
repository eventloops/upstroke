# reviews/

Dated **implementation reviews**: the verdict a frontier reviewer returned on a
pull request head, recorded verbatim. Scope is a commit or build step; result is
findings and their fixes, named `YYYY-MM-DD-<slug>.md`.

[`FINDINGS.md`](FINDINGS.md) is the standing finding ledger — every finding across every slice,
its disposition, and whether it has recurred. It is an **input to every review**: a reviewer reads
it before reviewing, does not re-raise a settled entry without new evidence, and appends a challenge
rather than overturning a disposition. The implementer holds the disposition and adjudicates
challenges.
