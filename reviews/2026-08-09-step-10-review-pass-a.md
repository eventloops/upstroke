# Review: Step 10, Pass A — discovery and the pools file

- **Date:** 2026-08-09
- **Scope:** commit `35558b2`, files `src/connect.rs`, `src/agent/mod.rs`,
  `src/agent/claude.rs`, `src/agent/copilot.rs`, `src/config.rs`,
  `src/catalog.rs` — everything that decides what `~/.upstroke/pools.toml` says,
  kept with the parser that reads it back
- **Level:** max — 10 finder angles, then the three load-bearing claims driven
  by throwaway tests rather than by argument
- **Result:** 15 findings — 2 normal, 5 low, 8 nits — **all fixed** in the
  follow-up commit.
- **Question the pass was asked:** *does what `connect` writes describe the
  machine truthfully?*

The through-line: **the honesty machinery works and its own plumbing was the
dishonest part.** `AuthState`'s third state, the "kind below is a default"
caveat, the roster-provenance header, the refusal to clobber — every mechanism
built to stop this file overstating what it knows held up. What did not hold was
the layer underneath: a file that kept saying *NOT signed in* after the operator
logged in, a preference order silently replaced by an alphabet, a budget key
accepted and ignored, and a `--force` that deleted the one setting the refusal
it overrides exists to protect.

## Findings

| # | Finding | Severity | Verdict | Fix |
|---|---------|----------|---------|-----|
| 1 | **Pool order was alphabetical, not file order.** `RawPools.pools` is a `BTreeMap`, so `Config.pools` came out sorted by name — while its own doc and `capacity::pool_for` both promise "table order as preference … moving a pool up the file promotes it". That order is the *only* mechanism an operator has for choosing between two accounts on one vendor, which is exactly what §13's `profile` seam exists for | normal | CONFIRMED (test: `[pools.work]` written first, `[pools.personal]` second, loaded as `["personal", "work"]`) | `BTreeMap<String, toml::Spanned<toml::Value>>`, re-sorted by span offset. Exact, and no new dependency — `Spanned` is already in `toml` |
| 2 | **`connect` reported "unchanged" after a login, leaving the file saying NOT signed in.** Auth state is rendered only as a comment, and `settings_of` strips comments before comparing — so the one thing that had changed was the one thing invisible to the comparison | normal | CONFIRMED (test: second connect with `Authenticated` returns `Wrote::Unchanged`; file still reads `NOT signed in`) | Two comparisons, because two questions are being asked. *May* this be replaced turns on settings; *should* it be rewritten turns on everything except the header's timestamp. See #9 for the other half of this trade |
| 3 | **`[budgets] pool_fraction = 0.5` was accepted in total silence.** No `deny_unknown_fields`, no leftover-key sweep. §13 names per-pool budgets, so that is the key an operator reading the design reaches for first — and they would believe a pool was capped while nothing capped it | low | CONFIRMED (test: loads with zero warnings and zero errors) | `deny_unknown_fields` on `Budgets`. `AskBefore` already had it; `RawPool` already warns by name |
| 4 | An explicit **`--pools <typo>` silently yielded no pools**, so `upstroke capacity` answered "no pools connected. Run `upstroke connect`" — sending the operator to regenerate a file that was fine while their flag was wrong. `read_repo_config`, twenty lines above, has modelled the right rule for `--config` since step 1 | low | CONFIRMED | The same `required` distinction: explicit and absent is an error, discovered and absent is the normal fresh case |
| 5 | **`--force` silently discarded `profile`, `monthly_allowance` and `endpoint`** — none of which `connect` can discover — while the refusal message recommended it. `profile` is the whole point of D2's seam and `monthly_allowance` is the only thing that makes a self-metered estimate possible at all | low | CONFIRMED | The operator's keys are read before anything is written and carried onto the matching pool; `render_pool` emits them. Asserted in the clobber test |
| 6 | Pool-shape classification matched **substrings, api-set first**, so a description carrying both an api-ish and a subscription-ish word came out `ApiKey`, and `pro` matched inside `provider`. Worse asymmetrically: a confident *wrong* shape suppresses the "kind below is a default" comment, so the caveat vanishes exactly when it is most needed | low | PLAUSIBLE | `classify_shape`: whole tokens against two named sets, and **both-or-neither ⇒ `None`**. Ambiguity now says so by saying nothing |
| 7 | **Copilot was probed twice per `connect` and per `capacity`** — `discover()` called `self.probe()` after the caller already had. Four subprocesses where two would do, each carrying a 15s timeout | low | CONFIRMED | `discover(&Caps)`. Discovery always runs beside a probe, so taking its result is the honest signature |
| 8 | `missing_from` compared slugs **exactly and case-sensitively**. GitHub writes display names (`GPT-5.3-Codex`) beside the slugs `--model` takes, so a listing that used the former would report the entire roster missing and advise an upgrade that cannot help — a guard crying wolf on its first real firing | nit | PLAUSIBLE | Normalized comparison, plus a zero-overlap guard: no overlap at all is a format mismatch, not a stale catalog |
| 9 | `settings_of` stripped only **whole-line** comments, but `render_pool` writes a trailing one on `reserve` — so tidying the single line the tool decorates itself produced a spurious clobber refusal | nit | CONFIRMED | `strip_comment` handles trailing comments, tracking quotes so a `#` inside a value is not one |
| 10 | **Two vocabularies for one `AuthState`**: a terse `Display` and `connect::describe_auth`, so `capacity` said "not authenticated" and `connect` said "NOT signed in — log in with…". The rule the enum exists to enforce was enforced in one place and observed in the other | nit | CONFIRMED | One `Display`, carrying the operator-facing wording; `describe_auth` deleted |
| 11 | `parse_interaction`'s `warnings` parameter went dead when the `ask_before` warning became a hard error, and its doc still claimed the function warns about notifier ids — which it never did | nit | CONFIRMED | Parameter dropped; doc says what the function actually decides |
| 12 | A **blank pool name** was accepted. `pool_option` maps `""` to `None`, so such a pool matched for routing while recording no attribution — the ledger would print "no pool is connected" with a pool plainly connected | nit | PLAUSIBLE | Rejected at load, like a blank `[[gates]]` `name` |
| 13 | `parse_pool` reached straight into `agent::by_id`, where the sibling guard `check_pin_adapters` takes an injected predicate *and documents why* — the engine resolves adapters through a `Harness`, not the global registry | nit | CONFIRMED | `load_with(…, has_adapter, …)`; `load` delegates with the builtin registry |
| 14 | Duplicate ids in `run_with` would render `[pools.<name>]` twice — a file `connect` writes and `config::load` then refuses, TOML rejecting duplicate keys | nit | PLAUSIBLE | Deduped. The builtin registry has no duplicates, but `run_with` is the public seam |
| 15 | Every Claude Code user got a pool named **`claude-max`** — a plan name discovery never established, in the one file whose purpose is to describe their actual subscriptions, from a module that marks its other defaults as defaults. It also put a per-agent alias table in `connect` | nit | PLAUSIBLE | The pool is named for its agent. Renaming is the operator's call, and the file is hand-editable so they can make it |

## On the tests

Four tests were added and three existing ones changed.

**Findings #1, #2 and #3 were each verified by a throwaway test written before
the fix and watched to fail**, then rewritten as a permanent one. #1's is the
most useful of the three: every pools fixture in the suite happened to be in
alphabetical order already, which is why nothing had noticed, so the new test
deliberately writes `work` before `personal`.

The `--force` test now asserts both directions at once — the operator's
`profile` and `monthly_allowance` survive, *and* `weekly = true` is still
refreshed — because a fix that preserved everything would have been a fix that
stopped updating anything.

## Trade-offs taken deliberately

- **Comments are regenerated.** #2's fix means a note an operator adds to the
  pools file is written away by the next `connect`. That is the price of the
  file's discovery findings — auth state, notes, the default-kind caveat —
  being current, and it was the better half of the trade: their *settings* are
  still protected by the refusal, and their own keys now survive even `--force`.
  Recorded in the test rather than left to be rediscovered.
- **#15 renames pools for existing users.** Someone with a `claude-max` pool
  from the previous build gets a `Refused` on the next `connect` (the settings
  differ), and `--force` will not carry their `profile` across, because the
  carry is keyed by pool name and the name is what changed. Renaming the section
  by hand first is the migration. Worth the churn while the tool is pre-release
  and the alternative is asserting a subscription tier at every operator.

## Checked and clean

- **Invariant 2 held everywhere.** `discover` subprocesses the vendor's own CLI
  and parses stdout; nothing reads a credential file, handles a token, or opens
  a socket. The `--force` carry reads the pools file the operator wrote, which
  is upstroke's own artefact.
- The three-state `AuthState` is used correctly at every site: no path renders
  `Unknown` as "not connected".
- `parse_auth_status` degrades to `Unknown` on a timeout, on non-JSON, on
  `null`, on `{}` and on a missing `loggedIn` — checked against the real
  `claude auth status --json` on this machine, which returns
  `loggedIn:false, authMethod:"none", apiProvider:"firstParty"` and takes the
  honest default path.
- The catalog's D1 entries are correctly commented for confidence
  (`gpt-5.3-codex` verbatim, `gemini-3.1-pro` pattern-derived), and the
  family-prefix guard test still covers both.
