## 14. Security and trust boundaries

Candidate repositories, plans, model output, external CLI output, configuration, environment
variables and persisted run data all cross trust boundaries and are validated before they grant
filesystem, process, git, capacity or state-transition authority.

- Validation sits at the effect boundary as well as at any CLI or UI boundary; constructing an
  internal-looking string confers no authority.
- Input size, recursion, collection growth, output capture, concurrency and retry work are bounded
  before allocating or spawning from untrusted values.
- Secrets are represented so that ordinary `Debug`, error, event and serialization paths redact or
  omit them, and never go on a command line when a safer supported channel exists.
- Least privilege holds between coordinator, worker, gate and reviewer roles: a read-only role never
  receives a write-capable handle and is trusted not to use it.
- Filtering, path checks, containers and adapter deny rules are not called a sandbox unless an
  enforced OS boundary supports the claim; security documentation states residual authority.
- Security-sensitive comparisons fail closed on malformed, contradictory or unavailable evidence; an
  availability fallback never grants more authority.

Enforced by: behavioural tests and the effect denylist where cited; review otherwise.
