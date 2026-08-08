# Add rate limiting to the API

Claude Code plan-mode shape: numbered implementation steps, no headings per
task, no annotations — everything comes from heuristics.

1. Design the limiter interface and storage schema
2. Implement the token-bucket middleware
   - keep counters in `src/limit/bucket.rs`
3. Fix the flaky retry test that hits the limiter
4. Document the rate-limit headers
