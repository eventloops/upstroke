## 23. Risks and kill criteria

- **Competitive risks, kill criteria, and positioning** are maintained in the strategy record outside this repository (moved 2026-08-22); the engineering risks stay here.
- **Estimator fragility:** provider usage endpoints break silently; hence signals-first trust order, read-only capacity in v0.1, and log-parse fallbacks.
- **Catalog staleness:** model rosters churn monthly; unknown models are never auto-selected, the catalog ships with releases, and pricing-derived priors bridge gaps.
- **Adapter churn:** Copilot's CLI has removed flags without deprecation; probing at pre-flight and per-version pinning are load-bearing, not nice-to-haves.

### 23.1 Deployment model and the enterprise path (recorded 2026-08-10)

The deployment model, the enterprise path, the positioning arguments and their kill
criteria moved to the strategy record outside this repository on 2026-08-22. The
engineering consequences other documents rely on are retained here, unchanged in substance:

- **Per-seat deployment.** upstroke runs on a developer's own machine, subprocessing a CLI
  signed in as *that developer* — through corporate SSO where there is one. There is no
  service account and no shared credential; a fleet of shared runners under a service
  account is not built without written terms saying it may be.
- **An org-shared pool cannot be estimated from one seat.** Every §13 source except provider
  endpoints is local to the seat, so against an org-level pool each instance estimates a
  shared resource from a fraction of the evidence. `Remaining::AtMost` stays correct but
  degrades toward vacuous; v0.2's answer is a pool flag — an org-shared pool returns
  `Unknown` with a note naming why — not a better estimator. Provider endpoints are the
  only org-level signal and remain a hint, never a floor.
- **Two features already serve a team without being built for it.** Repo-level
  `upstroke.toml` is policy distribution (a required second opinion on `src/auth/**` is
  committed to git and reviewable in a PR), and `reserve` reads as headroom for colleagues
  exactly as it reads as headroom for one's own interactive work.
- **The engine's record is the auditable account of what agents did**: engine-owned
  commits, the append-only event log, the engine-captured diff as ground truth, the
  reviewer's model family recorded per attempt, narrow permissions, and per-pool cost
  attribution — on any host, pre-commit. This is what the self-hosting record's "pen"
  refers to.
- **A refined story maps onto the IR nearly 1:1** (key → `id`, story → `implement`, bug →
  `fix`, spike → `design`, acceptance criteria → `acceptance`, component → `path_hints`,
  blocked-by → `depends_on`), so backlog import is translation, not authoring — an importer
  under §9's posture, never HTTP of our own. Writeback is a `Notifier` over the event log.
  Every `design_defect` is attributable to a story and aggregable per sprint: a badly
  refined story parks on a recorded question naming exactly what refinement failed to
  settle — a Definition of Ready with a failure signal. The importer is the
  highest-leverage unbuilt item; the near-term version is one developer hand-translating
  two stories in ten minutes.
- **Sequencing.** Prove the loop unattended (§21's acceptance run), use it on real work
  until it would survive a stranger's scrutiny, then build what real teams ask for. One
  cheap early check: confirm against real enterprise terms whether agent CLIs may run under
  anything other than a named seat.

### 23.2 What the first real runs measured (recorded 2026-08-10)

- **Review is charged per attempt, so attempt count dominates cost — and §13's `conserve` framing names the wrong lever.** Measured on one task, same base commit and same reviewer, with only `attempts_per` differing: escalating on the first failure cost **$2.73** over two attempts, while retrying on the cheap rung cost **$3.21** over three — *despite* the cheaper arm using the cheaper worker throughout. A frontier review costs the same whatever rung it judges, and it was 44–77% of spend across four runs, so one extra attempt costs more than one cheaper worker saves. "Route down aggressively, escalate only on failure" therefore optimises the smaller half of the bill and can lose money doing it; what reduces spend is **fewer attempts**, which often means starting *higher*. Two things keep this honest. The cheap rung does genuinely recover — §21(b)'s same-rung retry is real, and a retry succeeded here on the third attempt — so this is an argument about price, not capability. And the shape the data points at is inexpressible today: `attempts_per` is one `u32` per kind (`config.rs`), not per rung, so "one shot on the cheapest rung, a retry higher up" is a v0.2 config change rather than a settings tweak. **When cost has to come down _while the implementer is cheap_, the lever is the reviewer, not the worker** — a cheaper judge on early rungs — and that trade must be made deliberately, because on this evidence the reviewer is the half that earns its keep: it rejected an emission that built clean and passed all 722 tests but was not a compile-time constant, and so would have failed CS0133 in a consumer's build. No gate can catch that. **The scoping matters and the emphasis above is deliberate:** every run behind these numbers started at `small` and the ones that succeeded landed at `small` or `mid`, so nothing here measures a frontier *implementer*. The sentence beside it — a frontier review costs the same whatever rung it judges — is what says the ratio must invert: review is a roughly fixed cost per attempt, while implementation scales with tier and with how much agentic work the task takes. Review's 44–77% share is therefore a fact about cheap workers, not a law. Read as a general finding it would send someone optimising the wrong half of a frontier-implemented run, which is the regime the Codex adapter (§21) exists to make affordable, and the one this project still has no numbers for. That gap is now recordable rather than merely regrettable: `AttemptRecord.usage` carries the tokens a CLI reports even when it reports no dollars, because a run that did not record its usage can never be re-measured.
- **The routing dataset is better than §10 implies and the prize is smaller than it sounds — bound it before building anything.** §10 promises `export-decisions` "emits the dataset a learned router would train on" and v0.3 lists learned routing. Two corrections, pulling opposite ways. In its favour: **escalation yields paired observations** — `small failed → mid ok` is two models attempted against an identical task, treatment varying with the task held constant, produced free as a side effect of the ladder. That is a better structure than most off-policy settings ever get, and the label (passed every gate and an independent frontier reviewer) is objective and adversarially generated, which is rare in this domain. Only one direction is censored: when the cheap rung succeeds, nothing learns whether the expensive one would have, and buying those cells means occasionally double-running on purpose. Against it: **a perfect oracle is worth only the attempts it would have skipped, measured at 15–25% of spend** — real at scale, transformative for nobody — and the residual doubt is about *features*, not sample count, since the task that defeated both cheap attempts here read as trivial from its text and was hard for a reason living in the codebase's semantics rather than in anything a feature vector recovers. **The cheap test is to ask a frontier model to predict rung and cost against runs whose outcome is already known**; if it is calibrated, ship that as a `--dry-run` step and drop the learned policy entirely. One methodological finding stands behind all of the above and generalises past it: two runs of an identical configuration on one task produced two *different failure modes* — a review rejection and a parked question — so a single-run A/B comparison of agent behaviour is not evidence, however clean its numbers look. **Sharpened 2026-08-11**: the same reviewer, same model, same effort, passed `u8::try_from(v).unwrap_or(100)` on one run ("not the prohibited panicking `unwrap`") and rejected it on another ("still an unwrap-family shortcut") — one judge disagreeing with itself on one line, which puts the noise floor of review across pass and fail on identical input (`decisions/2026-08-11-codex-reasoning-effort.md`). The corollary is for plans, not judges: acceptance criteria naming a forbidden *idiom* invite that judgement call, where ones naming a forbidden *behaviour* ("must not panic on any input") can be checked.
