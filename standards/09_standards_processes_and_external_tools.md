## 9. Processes and external tools

Internally constructed commands use `std::process::Command` (or the runner abstraction) with
separate program and argument values; never concatenate values into shell text. Where the product
deliberately accepts user-authored shell commands — gates — that text stays opaque, and no untrusted
path, task field or model output is interpolated into it.

Every subprocess integration defines and tests:

- executable discovery and the error when it is absent;
- working directory and environment inheritance or removal;
- timeout, cancellation and descendant-process cleanup;
- exit-status **and** output interpretation, since neither alone is sufficient;
- stdout/stderr size and encoding behaviour, including malformed or adversarial output;
- secret redaction from commands, events, diagnostics and transcripts.

Stdout from an external tool is untrusted input even when the tool is official: parse it into a
typed result and reject contradictory or ambiguous success (`codex login status` prints "Not logged
in" and exits 0). Platform-specific process behaviour lives behind a shared semantic contract with
native tests per implementation.

The product invariants apply here: model interaction happens only inside official CLI subprocesses,
and the engine never adds an HTTP or model-API path.

Enforced by: tests and platform CI; the effect denylist for raw spawns outside the funnel; review.
