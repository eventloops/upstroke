## 14. Security and trust boundaries

Candidate repositories, plans, model output, external CLI output, configuration, environment
variables, and persisted run data cross trust boundaries. Code MUST validate them before granting
filesystem, process, git, capacity, or state-transition authority.

- Validation belongs at the effect boundary as well as any CLI/UI boundary; a caller cannot confer
  authority merely by constructing an internal-looking string.
- Bound input size, recursion, collection growth, output capture, concurrency, and retry work before
  allocating or spawning from untrusted values.
- Represent secrets so ordinary `Debug`, error, event, and serialization paths redact or omit them.
  Never place credentials in command-line arguments when a safer supported channel exists.
- Preserve least privilege between coordinator, worker, gate, and reviewer roles. A read-only role
  must not receive a write-capable handle and be trusted simply not to use it.
- Do not describe filtering, path checks, containers, or adapter deny rules as a sandbox unless the
  enforced OS boundary supports that claim. Security documentation MUST state residual authority.
- Security-sensitive comparisons and decisions MUST fail closed on malformed, contradictory, or
  unavailable evidence; availability fallbacks must not silently grant more authority.
