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

The export reads only the named, non-live run's event log and `plan.normalized.json`: it makes no HTTP request, branch switch, lock acquisition, or write. JSONL is the default; CSV has the same logical rows, with nested review passes and path hints represented as quoted JSON cells. See `decisions/2026-08-11-export-decisions-schema.md` for schema 2, legacy unknowns, and the measured/derived boundary.

`--dry-run` executes everything except agents: parse, route, and print task → kind → chain (with source of each decision: config/annotation/override) → gates → pool + strategy effect, at zero spend. It exists from day one; it is both the config-iteration loop and the sales demo.
