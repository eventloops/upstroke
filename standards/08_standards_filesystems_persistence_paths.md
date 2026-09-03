## 8. Filesystems, persistence, and paths

- Paths are `Path`, `PathBuf`, `OsStr` or `OsString`: never string concatenation, never assumed
  UTF-8. A lossy display string is for diagnostics only, never identity. `CommandSpec.program`
  carries a bare CLI name today and is a `String`; this standard governs it the moment a path-valued
  input exists.
- Say what each write guarantees — replaceable, create-once, append-only, atomic, durable — because
  they are separate guarantees. A successful rename is not durability.
- Publish multi-step output through a unique staging path in the destination filesystem, never a
  fixed temporary name that concurrent writers can collide on.
- Exclusivity uses an atomic primitive (`create_new`, a lock, or compare-and-swap); check-then-write
  is not exclusive.
- Cleanup removes only what this operation can prove it owns, never inferred from a shared filename.
  Recursive deletion of a run-scoped tree is token-carried: `rundir::PrivateHalfProof` in every
  build, and the `cfg(test)`-only scratch-tree token.
- On-disk data is untrusted, including data written by an older or interrupted version: validate
  schema, bounds and invariants before constructing domain state. A persisted or inter-process
  representation is an explicit schema; do not serialize an internal struct for convenience. Serde
  defaults, aliases, unknown-field handling and enum tagging are compatibility decisions with tests.
- Containment checks account for `..`, absolute paths, symlinks and reparse points, and platform
  prefixes as the boundary requires; lexical normalization alone proves nothing about the
  filesystem.

For the event log, `DESIGN.md` §4 is absolute: every transition is an event and state is rebuilt by
replay. Never update shadow state and then emit. Schema changes preserve or deliberately migrate
supported historical runs, and an interrupted or truncated tail has defined recovery.

Enforced by: behavioural tests and platform CI; the effect denylist for raw filesystem calls outside
the funnel; review for atomicity, durability and ownership.
