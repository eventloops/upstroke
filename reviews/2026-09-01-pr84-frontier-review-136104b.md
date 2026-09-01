# PR #84 — the infra relocation: frontier review record, post-merge

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED**, one P2 finding |
| **Reviewed SHA** | `136104bc1de1f9eaa49d634994a6a50c0169ad33` — the merged head |
| Pull request | eventloops/upstroke#84, into `master`; merged by the owner 2026-09-01T12:09:45Z |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 84`, 90-minute per-pass limit |
| CI at the reviewed SHA | green, `upstroke-pr-policy` included |

**Sequencing, disclosed honestly.** This review was launched pre-merge under
the CI-gated protocol, but the owner merged the head while it was still
running — the owner's merge is the attestation and preceded this verdict. The
review therefore lands as post-merge evidence, and its one finding is
dispositioned by the dated erratum appended to
`decisions/2026-09-01-infra-private.md` in the same pull request that lands
this record.

**Iteration history at earlier heads** (each verdict at its exact head):
`0fbb2c2` — five findings (no decision record; crate delta denied; `~/bin`
overbroad; relocation unevidenced; counts wrong), repaired by `f70c226`.
`f70c226` — five findings (a ledger row cited a blob absent at its SHA,
caught by the live validator; the crate-history repair inverted the
publication timeline; intake staged, not landed; twenty for twenty-one
paths; record overclaimed its reference cleanup), repaired by `68a50ec`.
`68a50ec` — two findings (the base had fallen behind master when #82 merged
mid-cycle; the recovery claim pointed at `7923a912`, whose tree carried
twelve files under the old names), repaired by `e329eea` plus the master
merge that produced `136104b`. `136104b` — the one finding below. Each
round's repairs are recorded in the pull request's finding ledger.

## The verdict, verbatim

> One blocking finding:
>
> - **P2 — The recovery claim is still false at the exact commit graph.** The decision record says "this change's parent" contains all 18 files and every public head from `458d928` through that parent contains the exact tree. But head `136104b` has ordered parents `e329eea` and `fff6abd`; `e329eea:infra` is absent. Only the PR-base parent `fff6abd:infra` has tree `1293e4a7…`. Public head `26c6e6c`, chronologically between the stated endpoints, also has no `infra/`.
>
>   Concrete failure: private archive is lost → operator follows "this change's parent" using ordinary `HEAD^1` or chooses another asserted-good public head → no `setup.sh` or guest templates exist → rebuild fails. The PR body's stronger "recoverable at any head" claim and INFRA-011's `fixed` disposition are therefore unsupported. Name `fff6abd…` explicitly and restrict the range to first-parent `master` snapshots, or provide one canonical recovery commit/command.
>
> I verified that the 21-path diff and counts are correct, the private intake is on its default branch with the claimed tree ID, the package claim is true at `fff6abd`, and no Rust panic/error/path rule is implicated.
>
> VERDICT: CHANGES_REQUIRED
