## 18. CLI surface

```
upstroke connect                     # discover installed agent CLIs, auth, plans; writes ~/.upstroke/pools.toml
upstroke design <brief>              # v0.3: interactive design phase (until then: Claude Code plan mode)
upstroke validate <plan>             # parse, task table, routing + capacity preview
upstroke run <plan> [--dry-run] [--budget <usd>] [--config <path>]
upstroke resume <run-id>
upstroke status [<run-id>] [--follow]
upstroke answer <question-id> [--option N | --text "..."]
upstroke capacity                    # all pools: remaining, resets, active strategy effect
upstroke export-decisions <run-id> [--format jsonl|csv] # landed 2026-08-12: local versioned attempt projection to stdout
```

The export reads only the named, non-live run's event log and `plan.normalized.json`: it makes no HTTP request, branch switch, lock acquisition, or write. JSONL is the default; CSV has the same logical rows, with nested review passes and path hints represented as quoted JSON cells. See §25 for schema 2, legacy unknowns, and the measured/derived boundary.

`--dry-run` executes everything except agents: parse, route, and print task → kind → chain (with source of each decision: config/annotation/override) → gates → pool + strategy effect, at zero spend. It exists from day one; it is both the config-iteration loop and the sales demo.

`status` prints the settled view: the report and its ledger (§13), then a `state:` line wherever
the run is still owned or stopped without finishing, the `upstroke answer` command for each open
question, and the transcripts path. `status --follow` first replays the run's history from the
beginning and then prints each event as it arrives, **one line per event**: the record's own time
of day with the zone the record wrote (`14:03:07Z`; a reader in UTC+2 sees the engine's clock,
not theirs), two spaces, then the body. The body names the task where the event has one, the reason
where the record carries one, and the decision the engine made with it. A failed attempt says
which ladder move follows and whether the task was parked on a question, and each half of that
settlement renders on its own, so no pairing is dropped. A terminal failure says whether the run
halts or continues, the policy frozen with it. A declined question is a decline, with its task's
failure and that same policy, never an answer; a question no channel could reach a person with is
unanswered. An attempt is "passed" by the one definition the fold promotes on — no failure and
every review pass passed — and otherwise the line names the pass that rejected it or reached no
verdict. A resume says how many uncommitted paths it discarded. A finished run names its outcome
in words and, when halted, the task it halted at. Every field on the line is on-disk data, so the
line is one line by construction: a newline, carriage return or tab inside a quoted reason becomes
one space and any other control character is written as its `\u{..}` escape rather than reaching
the terminal. The set of events a line can describe is closed — a variant the binary does not know
is a build error, never a line that renders as nothing. The line is a contract with an operator,
not a debug aid, and a change to it is a change to this section.
