---
id: SWEEP-RENDER-013
severity: P3
disposition: deferred
category: correctness
pr: 166
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/status.rs:41
provenance: pre_existing
first_bad:
guard: a RunStatus API change in src/status.rs deriving one liveness reading; production load currently constructs coherent flags
---

## Failure sequence

A caller can manually construct public RunStatus fields with `running: true, held: false` -> render reports a running process holding the run despite the supplied held flag. Production `load` computes running from held and the finish state, so its values are coherent. No supported production path or regression witness for an inconsistent load result has been established. The field-privacy guidance in standards section 5 motivates an API improvement; it is not an explicit MUST that this renderer change breaches.

## What the change that takes this up should do

Have RunStatus derive liveness once from the held and finished facts, or expose a constructor/private representation that preserves the relationship. Move report and render readers together and assess compatibility for public field users. This remains a deferred P3 API design issue, not a claim that an operator currently reaches an inconsistent production value.
