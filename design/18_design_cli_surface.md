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

`connect` prints an agent summary with its auth state and pool, or its reason for being skipped.
The summary marks a pool whose `kind` is a default. Discovery notes follow on indented continuation
lines; each physical note line is prefixed separately and control characters become spaces, so a
note cannot create an unprefixed warning record. A skipped agent's error is reported once. Warnings
follow the agent summaries, then `wrote <path>`, `unchanged: <path>`, or the refusal with the proposed
file and what `--force` keeps. §17 describes that file and its rewrite rules.

`--dry-run` executes everything except agents: parse, route, and print task → kind → chain (with source of each decision: config/annotation/override) → gates → pool + strategy effect, at zero spend. It exists from day one; it is both the config-iteration loop and the sales demo.

`status` prints the settled view: the report and its ledger (§13), then a `state:` line wherever
the run is still owned or stopped without finishing, the `upstroke answer` command for each open
question, and the transcripts path. Report and ledger rows, warnings, question contexts,
resume and answer commands, and paths sanitize each assembled terminal line. Newlines, carriage
returns and tabs in recorded fields become spaces; other control characters become visible
escapes. The renderer adds layout newlines after sanitation, including the separate resume
command. Ledger widths measure sanitized cell text. Persisted JSON and event values stay intact.
`status --follow` first replays the run's history from the
beginning and then prints each event as it arrives, **one line per event**: the record's own time
of day with the zone the record wrote (`14:03:07Z`; a reader in UTC+2 sees the engine's clock,
not theirs), two spaces, then the body. Abbreviation requires a valid Gregorian date, hours
`00..23`, minutes and seconds `00..59`, an optional fraction with at least one digit, and a
`Z` or signed `HH:MM` offset with hours `00..23` and minutes `00..59`. Lowercase `t` and `z`
are accepted too. Other timestamps stay whole. Leap-second values retain their date because
this renderer does not check the historical leap-second schedule. The body names the task
where the event has one, the reason
where the record carries one, and the decision the engine made with it. A failed attempt says
which ladder move follows and whether the task was parked on a question, and each half of that
settlement renders on its own, so no pairing is dropped. A terminal failure says whether the run
halts or continues, the policy frozen with it. A declined question is a decline, never an answer,
and the line reports the halt policy frozen with the decline. Task failure has its own later
event, which a log that stopped between the two does not yet carry. A question no channel could
reach a person with is unanswered. An attempt is "passed" only on the record's own claim of success — no failure and
every review pass passed — and a review that rejected the code or reached no verdict is named by
pass and model beside the failure's reason, on the record the engine writes (a `review failed:`
failure with the pass's outcome) as much as on one carrying the outcome alone. A resume says how
many uncommitted paths it discarded; a deferral wait says how long it waited as seconds with
exactly three decimal places, preserving every recorded millisecond, and which round. Public
`status::describe` truncates any submillisecond remainder supplied directly by a caller.
A terminal task failure carries the transition's own reason and kind whether it stands alone
or rides on the attempt that caused it. A
finished run names its outcome in words and, when halted, the task it halted at — or that the
record names none. Every field on the line is on-disk data, so the
line is one line by construction: a newline, carriage return or tab inside a quoted reason becomes
one space and any other control character is written as its `\u{..}` escape rather than reaching
the terminal. The set of events a line can describe is closed — a variant the binary does not know
is a build error, never a line that renders as nothing. The line is a contract with an operator,
not a debug aid, and a change to it is a change to this section.
