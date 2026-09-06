---
id: PR5-R2-WIN-NON-SURROGATE-REPARSE
severity: P2
disposition: deferred
category: security-trust
pr: 5
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR6/PR7 implementer (the slice that next owns Windows containment)
---

## Failure sequence

`PR5-WORKSPACE-006`. `validate_execution_root_chain`'s Windows arm checks the raw `FILE_ATTRIBUTE_REPARSE_POINT` attribute as well as `FileType::is_symlink()`, and only the raw check covers the **non-surrogate** tags — dedup, placeholder, LX symlink, appexec. Every fixture builds its reparse point with `cmd /C mklink /J`, and Rust's `is_symlink()` answers true for `IO_REPARSE_TAG_MOUNT_POINT` because a junction is a name-surrogate tag, so omitting the attribute check is behaviour-neutral for the only shape any fixture constructs. Measured twice: the mutation SURVIVED both the pre-repair and the post-repair guest runs, with both junction tests running and passing

## What the change that takes this up should do

Owner, as the ledger records it: PR6/PR7 implementer (the slice that next owns Windows containment).

**Carried because the distinguishing fixture cannot be built by the guest's test user.** Two of the four non-surrogate tags need a privilege it lacks (dedup and placeholder are filesystem-feature reparse points, not user-creatable), and the other two need WSL or an app-execution alias installed on the runner. A fixture that faked the attribute would be testing the fixture. What holds today is the surrogate half, on both platforms, by `a_junction_below_the_private_root_refuses_the_execution_root` and `a_managed_base_or_private_root_that_is_itself_a_link_refuses_before_any_effect`. The live passage is `slice_contract.expected_failures_refusals[0]` — "symlink/junction on the chain" — which names exactly the shape that *is* covered

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
