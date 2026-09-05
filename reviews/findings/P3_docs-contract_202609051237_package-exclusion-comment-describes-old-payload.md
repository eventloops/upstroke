---
id: PR156-SHARED-PACKAGING
severity: P3
disposition: deferred
category: docs-contract
pr: 158
reviewed_sha: 105c9e1509efe6cbfbe6d93e8d930c289056f041
location: Cargo.toml:23
provenance: introduced_by_feature
first_bad: 45ecb3d86c33bcdadc907d95fa5874e70418fd13
guard: "Compare the exclusion comment with the package listing and retained policy: website assets stay excluded while internal notes ship. The PR body already discloses this family's 16 packaged notes. Owner-authorized deferred as lower-severity documentation debt; retain one canonical record if unresolved. #156 owns the shared comment correction."
---

## Failure sequence

Internal notes enter the package while the exclusion comment describes docs as website material outside the library package -> a maintainer relies on the stale exclusion claim -> the published payload is misdescribed. The original report also found missing PR-body scope disclosure; that portion is now corrected.

## What the change that takes this up should do

Describe the specific website assets that remain excluded and the internal notes that ship. Compare the comment with the package listing without changing the approved packaging policy. #156 owns the shared correction; do not duplicate it in this family branch.

## Review history and evidence

The original gpt-5.6-sol/max report at 6180e0e6015cb7b267f6136b5065b465cb32381e supplied no severity. The lane provisionally classified PR158-PASS1-02 as P2/docs-contract and pre_existing relative to the family. The independent GPT-6 Astra/max report at 105c9e1509efe6cbfbe6d93e8d930c289056f041 names the same manifest-comment proposition ASTRA158-004 and classifies it P3/docs-contract. The packaging change introduced it within the declared-base diff at 45ecb3d86c33bcdadc907d95fa5874e70418fd13. The family's package additions are now disclosed in the live PR body, but the source comment remains unresolved. Canonical ID PR156-SHARED-PACKAGING names this proposition, with PR158-PASS1-02 and ASTRA158-004 preserved as aliases. The owner authorizes deferral of this unresolved documentation finding once individually recorded.

The review's successful wrapped package listing includes all 17 module notes and their README. Cargo.toml still describes docs as website material rather than library content, while its exclusions name only docs/index.html and docs/CNAME. The package-list.stdout.log is preserved with the independent audit; no new package command was run while preparing this record.

[Independent review of 105c9e1509efe6cbfbe6d93e8d930c289056f041](https://github.com/eventloops/upstroke/pull/158#issuecomment-5551707422).
