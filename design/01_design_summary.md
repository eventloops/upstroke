## 1. Summary

`upstroke` is a headless orchestration engine for AI coding agents. A frontier model and the user design a piece of work together in an interactive session; `upstroke` then executes that plan unattended — normalizing it into a dependency graph of typed tasks, dispatching each task to an existing coding-agent CLI (Claude Code, GitHub Copilot CLI, Aider) with a **model chosen per task**, verifying every result through objective gates and strong-model review, escalating failures up an explicit model chain, and **scheduling all of it against the user's actual subscription capacity** so that prepaid frontier-tier quota never expires unused.

It never edits a file, never implements an agentic loop, and never calls a model API. It is the conductor, not an instrument — and it treats your Claude Max windows, Copilot credits, API dollars, and local models as one portfolio to be spent optimally.

When it gets stuck at 2am it doesn't stop the run: it parks only the blocked branch, keeps everything else moving, and pings you as the top rung of the escalation chain.
