## 9. Processes and external tools

Internally constructed commands MUST use `std::process::Command` (or the runner abstraction) with
separate program and argument values. Do not concatenate values into shell text. Where the product
contract deliberately accepts user-authored shell commands, such as gates, keep that text opaque;
never interpolate an untrusted path, task field, or model output into it.

Every subprocess integration MUST define and test:

- executable discovery and the error when it is absent;
- working directory and relevant environment inheritance or removal;
- timeout, cancellation, and descendant-process cleanup;
- exit-status interpretation **and** output interpretation—neither is universally sufficient;
- stdout/stderr size and encoding behaviour, including malformed or adversarial output;
- secret redaction from commands, events, diagnostics, and transcripts.

Stdout from an external tool is untrusted input even when the tool is official. Parse it into a
typed result and reject contradictory or ambiguous success. Platform-specific process behaviour
belongs behind a shared semantic contract, with native tests for each implementation.

The product invariants still apply: model interaction occurs only through official CLI
subprocesses, and the engine does not add an HTTP/model-API path.
