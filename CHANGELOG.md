# Changelog

## Unreleased

- Retired the App-signed `upstroke-frontier-review` attestation gate: its two privileged workflows,
  four scripts and fixture tests, and the signing environment are gone, and the default-branch
  ruleset requires `upstroke-ci` and `upstroke-pr-policy` only. The review obligation is unchanged;
  the owner's merge is the attestation (`decisions/2026-08-23-retire-app-attestation.md`).
- Renamed project from `tactus` to `upstroke`. Binary, crate, env-var prefix (`UPSTROKE_*`),
  user directory (`~/.upstroke`) and run directory (`.upstroke/`) all change; no aliases. The
  transformation is `scripts/rename-tactus-to-upstroke.sh`.

## 0.1.0 — 2026-08-10

- The sequential conductor, end to end: plan ingestion, routing, the Claude Code and Copilot
  adapters, the engine with git ownership, gates, cross-family review, the verification ladder,
  the event log with resume and status, and the read-only capacity engine. `acceptance/RESULT.md`
  is the evidence.
