---
id: PR156-SHARED-N3
severity: P2
disposition: deferred
category: correctness
pr: 158
reviewed_sha: 105c9e1509efe6cbfbe6d93e8d930c289056f041
location: .github/scripts/test-internals-notes.sh:102
provenance: introduced_by_feature
first_bad: e711a5c227987ea3ea93fdef8bbd9c124478f75d
guard: "A future shared-gate repair should recognize usable Markdown backlinks and preserve positive controls plus the hidden-path, bare-parentheses, inline-code and fenced-code witnesses. Owner-authorized deferred under the 2026-09-05 limited-impact stop rule: the demonstrated consequence is missing internal navigation. Keep this P2/correctness finding visible; deferral does not claim a fix or waive required green CI and independent review."
---

## Failure sequence

A note contains a parenthesized source path outside a usable Markdown link, or a hidden source path accompanies a broken visible backlink -> N3 selects the relative .rs substring without checking Markdown context -> the gate accepts a note whose rendered backlink is unusable. The parser also rejects valid reference-style and angle-delimited Markdown links.

## What the change that takes this up should do

Recognize actual Markdown links outside code and other non-link contexts. Commit positive and negative fixtures for the supported syntax in the shared #156 carrier. Preserve coherent repairs already underway. This accepted limited-impact finding does not require another repair cycle or holding peer documentation lanes solely for its repair.

## Review history and evidence

The original gpt-5.6-sol/max report at 6180e0e6015cb7b267f6136b5065b465cb32381e supplied no severity. The lane provisionally classified PR158-PASS1-01 as P1/security-trust and pre_existing relative to the family. The independent GPT-6 Astra/max report at 105c9e1509efe6cbfbe6d93e8d930c289056f041 names the same proposition ASTRA158-001 and classifies it P2/correctness. It is introduced by the prerequisite within the declared-base PR diff. The historical CHANGES_REQUIRED treated executable gate logic as blocking. The owner's later limited-impact stop rule authorizes deferral of its demonstrated internal-navigation consequence; the steward confirmed that scope without changing severity. Canonical ID PR156-SHARED-N3 now names this proposition, with PR158-PASS1-01 and ASTRA158-001 preserved as aliases.

The published independent report preserves valid_inline exit 0 and no_backlink exit 1 as controls. bare_parentheses, inline_code_only and fenced_code_only incorrectly exit 0. Its wrong-visible-link-before-fenced-example fixture fails correctly, so that particular ordering is not claimed as a false acceptance. The earlier hidden-path witness remains a separate preserved occurrence. The independent gate-controls.json and isolated fixtures are beside the review artifact in the private audit directory.

[Independent review of 105c9e1509efe6cbfbe6d93e8d930c289056f041](https://github.com/eventloops/upstroke/pull/158#issuecomment-5551707422).
