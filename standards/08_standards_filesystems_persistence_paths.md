## 8. Filesystems, persistence, and paths

- Represent paths with `Path`, `PathBuf`, `OsStr`, or `OsString`. Do not construct paths by string
  concatenation or assume UTF-8. A lossy display string is for diagnostics only, never identity.
- Define whether each write is replaceable, create-once, append-only, atomic, and/or durable.
  These are separate guarantees. A successful rename is not automatically a durability guarantee.
- Publish multi-step output through a unique staging path in the destination filesystem. Do not
  use a fixed temporary name where concurrent writers can collide or delete each other's work.
- Use an atomic primitive for exclusivity (`create_new`, a lock, or compare-and-swap as the design
  requires). A check followed by a write is not exclusive.
- Cleanup may remove only resources whose ownership this operation can prove. Never infer
  ownership from a shared filename alone.
- Treat on-disk data as untrusted, including data written by an older or interrupted version.
  Validate schema, bounds, and invariants before constructing domain state.
- Treat a persisted or inter-process representation as an explicit schema. Do not serialize an
  internal struct merely for convenience when private refactoring would then change stored data.
  Serde defaults, aliases, unknown fields, and enum tagging are compatibility decisions and need
  tests.
- Path containment checks MUST account for `..`, absolute paths, symlinks/reparse points, and
  platform-specific prefixes as appropriate to the security boundary. Lexical normalization alone
  does not prove filesystem containment.

For the event log, `DESIGN.md` §4 is absolute: every state transition is represented by an event,
and state is reconstructed by replay. Do not update shadow state and then emit an event. Event
schema changes MUST preserve or deliberately migrate supported historical runs, and interrupted or
truncated tails MUST have defined recovery behaviour.
