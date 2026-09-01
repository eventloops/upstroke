# PR #43 — frontier review record, `2b9e94b`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, four findings** |
| **Reviewed SHA** | `2b9e94b0011b81ebaf53c79fcb354753ddfdb07d` |
| Pull request | eventloops/upstroke#43, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 43` |
| Inputs | exact base-to-head diff, repository tree, prior review records, preserved CI logs, and pull-request body |

## Disposition

All four findings were accepted in the single triage. The macOS record now names the
SIGCONT guard fallback as another writer and requires the writer and its reason to be
established. The Windows record now preserves the log's exact separators and columns and
states its matching normalization. The pull-request body excludes the integration merge
from rollback instructions. Both records replace the expired PR #42 sequencing promise
with the serialized ledger lease and consolidated-sweep boundary. The shared ledger itself
is not edited by this branch.

## Review, verbatim

1. **P1 — the macOS matching rule is still incomplete.** The matching rule omitted the
   SIGCONT guard fallback as another route that can write SIGTERM and produce the same
   exit status without a reaper-cleanup failure.
2. **P2 — the Windows fingerprint does not byte-match its own log.** The record omitted
   Windows separators and assertion columns, and did not disclose matching normalization.
3. **P2 — the rollback instruction became unsafe at this exact head.** A first-parent
   range now includes the integration merge and could remove base work.
4. **P2 — the deferred-ledger promise has already failed.** Both records promised rows
   once PR #42 landed, but that condition had passed without those rows.

VERDICT: CHANGES_REQUIRED
