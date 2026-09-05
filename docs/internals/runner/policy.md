# `src/runner/policy.rs`

Extended notes for [`src/runner/policy.rs`](../../../src/runner/policy.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Resolution, canonical serialization and the digest of the run's
[`RunnerPolicy`] (INV-23).

#### One record, not two

The wire type is PR3's [`crate::topology::events::RunnerPolicy`], shipped
for `run_started(4).runner`. This module deliberately does not define a
second one. INV-23's enforcement is an **exact-equality** check across four
copies of one record — the marker digest (P1), `owner.json.runner` (P3b),
`run_started(4).runner` (P6) and `run_resumed(4).runner` — and
`decisions.pr_sequence[5].scope` names the digest that binds them:
"canonical serialization and `runner_policy_sha256`". Two Rust definitions
of one record are two things that drift, and the fold's `difference()`
would compare a record against itself while the *other* definition moved.

So: `topology::events` owns the shape and the equality; this module owns
**resolution**, **canonicalisation** and **the digest** over that shape.

#### Why a hand-rolled encoding rather than `serde_json`

The digest goes into the P1 marker and into every container intent
(`decisions.pr_sequence[7].scope`: "owner run, run directory, incarnation,
repo key, invocation, `runner_policy_sha256`"), so it is compared across
processes and across binary versions. `serde_json` does not promise a
stable byte sequence for a map, and a field renamed on the wire would move
the digest silently. A length-prefixed encoding written out field by field
is injective by construction and can be written by hand in a test — which
is the only way to pin an encoding against something other than itself.

## `pub const CANONICAL_VERSION: &str = "upstroke.runner-policy.v1";`

The version tag the canonical encoding opens with.

Part of the digested bytes, so a future encoding change is a different
digest rather than the same digest over different bytes.

## `pub fn host_policy() -> RunnerPolicy {`

The host runner's resolved policy: `RunnerPolicy{kind: Host, policy:
host-v1, image: None, credential_volumes: None}`.

INV-23 requires resolution "by read-only inspection before the worktree
lock (… the runtime must already hold the image and the volumes must
exist)". For the host there is nothing to inspect: the boundary is this
process's own machine, there is no image and there are no credential
volumes — `image` and `credential_volumes` are `None` because a host
runner carrying either is [`RunnerRecordDefect::HostWithContainerFields`],
which PR3's `completeness()` already refuses. The inspection that can fail
is the container runner's, and that is PR6.

[`RunnerRecordDefect::HostWithContainerFields`]:
    crate::topology::events::RunnerRecordDefect::HostWithContainerFields

## `pub fn resolve_host() -> Result<RunnerPolicy, UpstrokeError> {`

Resolve the host runner's policy, refusing a record PR3 would call
incomplete.

The check is not decorative: this value is written into the marker, the
owner record and `run_started(4)`, and the fold refuses an incomplete one
on the way back in. Refusing here means a run cannot start with a record
its own resume would reject.

### Errors

[`UpstrokeError::Refused`] if the resolved record is not complete.

## `pub fn canonical_bytes(policy: &RunnerPolicy) -> Vec<u8> {`

The canonical bytes of `policy`.

Field order and shape, written out because a test has to be able to
reproduce them by hand:

```text
f("upstroke.runner-policy.v1")
f(kind)                  "host" | "container"
f(policy)                "host-v1" | "container-v1"
b(image.is_some)
    f(image.reference) f(image.id) b(image.digest.is_some) [f(image.digest)]
b(credential_volumes.is_some)
    n(len) [ f(agent) f(volume) ]*     in the map's own (sorted) order
```

where `f(s)` is `<byte-length>:<bytes>;`, `b(x)` is `f("1")` or `f("0")`,
and `n(x)` is `f(<decimal>)`. The same encoding
[`crate::topology::registry`] uses, for the same reason: a length prefix is
injective over values that may contain the delimiter.

## `pub fn runner_policy_sha256(policy: &RunnerPolicy) -> String {`

`sha256:<hex>` over [`canonical_bytes`].

The `sha256:<hex>` shape rather than a bare hex string, matching the
registry digest and the normalized plan digest, "so a log carries one shape
of digest rather than two" ([`crate::topology::registry`]).

## `const fn kind_tag(kind: RunnerKind) -> &'static str {`

The wire tag of a kind.

Written out rather than taken from serde, so the canonical encoding does
not move when a serde attribute does. That is exactly the drift the digest
exists to detect, and it must not be able to detect it by moving with it.

## `const fn contract_tag(contract: RunnerContract) -> &'static str {`

The wire tag of a contract version.

## `mod tests` › `fn the_host_policy_is_inv23s_host_record() {`

The host record, spelled out from INV-23's own field list rather than
from `host_policy()`.

## `mod tests` › `const HOST_CANONICAL: &[u8] = b"25:upstroke.runner-policy.v1;4:host;7:host-v1;1:0;1:0;";`

The bytes, written by hand from the field list in the module docs.

Not produced by `canonical_bytes` and not round-tripped: a suite that
consumes its own canonical output cannot see a symmetric rename, which
is `PR3-WIRE-PINNING`.

## `mod tests` › `fn the_canonical_encoding_separates_records_the_type_separates() {`

The encoding separates records the type separates.

INV-23 enforces **exact equality** between three copies of this record,
so an encoding that maps two distinguishable records onto one digest
makes a genuine mismatch invisible rather than loud. The digest fixtures
here are well-formed records crossed against each other, which catches a
field that stops being encoded — but not a pair deliberately collapsed.

Two pairs, because there are two places this encoding could collapse:
`Option<map>`'s absent/present-but-empty boundary, which no fixture
carried, and a host record carrying container-only fields, which nothing
pushed through `canonical_bytes` at all.

## `fn the_canonical_encoding_separates_records_the_type_separates() {` › `let absent = host_policy();`

Absent is not the same as present-and-empty. "No credential volumes
were configured" and "credential volumes were configured, and there
are none" are different records, and `Option` is how the type says so.

## `fn the_canonical_encoding_separates_records_the_type_separates() {` › `assert_eq!(`

Written by hand from the field list, like the two above it: version,
kind, contract, image-absent, volumes-present, zero entries.

## `fn the_canonical_encoding_separates_records_the_type_separates() {` › `let mut mislabelled = host_policy();`

A malformed record is encoded as what it is, not projected into the
record it ought to have been. `canonical_bytes` is INV-23's
comparison surface; silently normalising here would let a host runner
that had acquired an image agree with one that had not.

## `fn host_runner_declares_host_v1_policy_with_stable_digest()` › `let expected = format!("sha256:{:x}", Sha256::digest(HOST_CANONICAL));`

The expected digest is computed over the hand-written bytes, so the
oracle is the payload rather than the function under test.

## `fn host_runner_declares_host_v1_policy_with_stable_digest()` › `assert_eq!(`

Stable: the value a second incarnation resolves is the value the
first recorded, which is the whole of INV-23's equality check.

## `mod tests` › `fn the_digest_separates_every_field_of_the_record() {`

Every independently meaningful field, varied independently, with the
distinct-value counts asserted rather than described.

## `fn the_digest_separates_every_field_of_the_record()` › `let kinds: std::collections::BTreeSet<_> =`

Hostility as counts, not prose: every field the record has takes at
least two values across the set.

## `fn the_digest_separates_every_field_of_the_record()` › `let mut rebuilt = BTreeMap::new();`

And the same record digests the same however it was built: the
volume map is compared as a set, so insertion order may not move the
digest (PR3's own reason for making it a map).

## `mod tests` › `fn ascii_case_is_significant_in_every_string_field_of_the_record() {`

ASCII case is significant in **every** string field of the record.

PR3 compares `RunnerPolicy` records exactly — `difference()` reports
`ImageId` for `sha256:ab` against `SHA256:AB` — and INV-23 binds four
copies of one record through this digest: the P1 marker, the P3b owner
record, `run_started(4).runner`, and `run_resumed(4).runner`, the last
of which `validation_at_fold[14]` requires to equal the first "exactly
(kind, policy, image reference, id, digest, credential-volume set)". A
canonicalisation that folded case would let a marker attest a record
the fold calls different: the husk ownership proof would accept a
policy that is not the one it names.

Every other digest fixture is lowercase — including the delimiter one —
so a `to_ascii_lowercase()` anywhere in `field()` passes all of them.
This crosses each field with a case-distinct twin and asserts the
**count** of distinct digests.

## `fn ascii_case_is_significant_in_every_string_field_of_the_record() {` › `let volumes = upper_volume_agent`

A case-distinct *key*: `Codex` and `codex` are two entries of
the map, so this also proves the agent name is encoded and not
normalised on the way in.

## `fn ascii_case_is_significant_in_every_string_field_of_the_record() {` › `for (name, policy) in &fixtures[1..] {`

Every twin differs from the base in ASCII case alone, which is what
makes the digest counts below a statement about case.

## `fn ascii_case_is_significant_in_every_string_field_of_the_record() {` › `let upper_host = RunnerPolicy {`

And the bytes themselves carry the case, pinned against a payload
written by hand rather than against `canonical_bytes` output.

## `mod tests` › `fn field_writes_its_values_bytes_and_transforms_nothing() {`

`field` copies its value's **bytes**, and does nothing else to them.

The module docs give the grammar as `f(s) = <byte-length>:<bytes>;`, and
that is written out here rather than read from `field` — a length prefix
computed by the function under test would agree with any transformation
the function also applied.

This is the whole class in one assertion. `PR5-CORRECTNESS-004` is
`value.replace('_', "-")` and `PR5-SEAMS-002` is `value.trim()`; a
`to_ascii_lowercase`, a whitespace collapse, a Unicode normalisation and
a delimiter escape are the same defect wearing different clothes, and
every one of them changes these bytes. INV-23 makes this record
"execution identity", compared **exactly** across the P1 marker, the P3b
owner record and `run_started(4).runner`, so any two values the type
distinguishes must reach the digest distinguishable.

## `fn field_writes_its_values_bytes_and_transforms_nothing()` › `let mut wide = Vec::new();`

The length prefix counts **bytes**, not characters — a two-byte `é`
and a four-byte emoji say so, and a `chars().count()` would not.

## `mod tests` › `fn a_normalisable_difference_in_any_string_position_moves_the_digest() {`

Two records differing by a character a normaliser would collapse carry
two digests, in **every** string position of the record.

`PR5-CORRECTNESS-004` names one position (a credential volume's value)
and one pair (`creds_a` vs `creds-a`); `PR5-SEAMS-002` names the same
position and a whitespace pair. The class is neither: it is any
normalisation, anywhere a string reaches [`canonical_bytes`]. So the
pairs are crossed against every position the record has — image
reference, image id, image digest, credential-volume **key**, and
credential-volume **value** — and the counts are asserted rather than
described.

The failure this prevents is not a wrong answer but a *silent* one: the
marker's `runner_policy_sha256` and `owner.json`'s full record would
agree while carrying different execution identities, and
`prove_private_half_ownership`'s digest conjunct would mint deletion
authority for a private half belonging to a different runner.

## `fn a_normalisable_difference_in_any_string_position_moves_the_digest() {` › `const PAIRS: &[(&str, &str, &str)] = &[`

Pairs a normaliser would fold together. Each is *one* transformation
away from its twin, so a digest that cannot separate them names the
transformation that was applied.

## `fn a_normalisable_difference_in_any_string_position_moves_the_digest() {` › `let positions: &[(&str, fn(&mut RunnerPolicy, &str))] = &[`

Every string position of the record, by name, as a setter.

## `mod tests` › `fn the_container_fields_option_and_sequence_boundaries_are_injective() {`

The three boundaries a length-prefixed encoding of *these* fields can
still collapse, each pinned against bytes written by hand.

PR4's fixtures cross well-formed records against each other, which
catches a field that stops being encoded. They do not reach the three
places where the container half of the record has an `Option` or a
variable-length sequence, and those are where an encoding collapses:

1. **`digest: None` vs `digest: Some("")`.** "The manifest digest **when
   reported**" — a runtime that reported nothing and one that reported an
   empty string are different records, and PR3's `difference()` reports
   `ImageDigest` between them, so the digest INV-23 compares must too.
   `crate::runner::container::resolve` never *produces* `Some("")` — it
   collapses the two at the inspection seam, deliberately and in one
   place — but a record can carry one from a hand-edited `owner.json` or
   a future runtime, and the fold compares whatever it is given.
2. **An absent credential-volume map vs a present empty one.** PR4 pins
   this on a *host* record; a container record is where it actually
   occurs, because `completeness()` requires container volumes to be
   `Some` and an empty map is the real answer "no agent needs
   credentials".
3. **Concatenation coincidences.** `{"a": "bc"}` and `{"ab": "c"}` flatten
   to the same key/value sequence, as do a reference/id pair split at a
   different point. A delimiter-only encoding maps each pair onto one
   digest.

The expected bytes are written out from the module docs' field list, so
the oracle is the grammar and not `canonical_bytes`.

## `fn the_container_fields_option_and_sequence_boundaries_are_injective() {` › `let mut absent = container_fixture();`

-- 1. absent digest vs empty digest --------------------------------

## `fn the_container_fields_option_and_sequence_boundaries_are_injective() {` › `const DIGEST_ABSENT: &[u8] = b"25:upstroke.runner-policy.v1;9:container;12:container-v1;\`

version, kind, contract, image-present, reference, id, digest-absent,
volumes-present, 2, the two pairs.
Written out with the file's own line-continuation idiom, which strips
the newline *and* the leading indentation, so the literal is the
bytes and nothing else.

## `fn the_container_fields_option_and_sequence_boundaries_are_injective() {` › `const DIGEST_EMPTY: &[u8] = b"25:upstroke.runner-policy.v1;9:container;12:container-v1;\`

The same, with digest-present and a zero-length value.

## `fn the_container_fields_option_and_sequence_boundaries_are_injective() {` › `assert_ne!(DIGEST_ABSENT, DIGEST_EMPTY);`

The two literals differ, and by the one field: a copy-paste that made
them equal would make both assertions above vacuous.

## `fn the_container_fields_option_and_sequence_boundaries_are_injective() {` › `let mut no_volumes = container_fixture();`

-- 2. absent volume map vs empty one, on a container record ---------

## `fn the_container_fields_option_and_sequence_boundaries_are_injective() {` › `let volume_pair = |key: &str, value: &str| {`

-- 3. concatenation coincidences ------------------------------------
Every pair below flattens to the same character sequence and differs
only in where the boundaries fall. The counts are asserted rather
than described.

## `mod tests` › `fn completeness_covers_one_direction_of_the_host_container_field_split() {`

`HostWithContainerFields` is the only defect covering the host/container
field split, and it covers **one** direction.

The reconciliation obligation asks what PR3's `completeness()` does and
does not cover before anything is added to it. Executed rather than
asserted: the grid drives every combination of `image` and
`credential_volumes` presence against both kinds and records which
defect comes back.

What it covers: a **host** record carrying either container field. What
it does not, and what this slice therefore holds elsewhere: an image
record whose *digest* is `Some("")` (allowed, because "when reported"
makes an absent digest legitimate and there is no shape that
distinguishes a bad one), and a container record whose credential map
is empty (allowed, because "an empty map is a real answer"). Both are
then *encoding* obligations rather than *shape* ones, which is why
`the_container_fields_option_and_sequence_boundaries_are_injective`
exists.

## `fn completeness_covers_one_direction_of_the_host_container_field_split() {` › `for has_image in [false, true] {`

The host direction: any container field at all is refused, and by the
one defect.

## `fn completeness_covers_one_direction_of_the_host_container_field_split() {` › `assert_eq!(`

The container direction is not the mirror image: a missing field is
named by its own defect, and a *present but empty* one is accepted.

## `fn completeness_covers_one_direction_of_the_host_container_field_split() {` › `for digest in [None, Some(String::new()), Some("sha256:b".to_owned())] {`

And the digest is not a completeness question at all, in either
state — which is what makes it an encoding one.

## `mod tests` › `fn field_values_carrying_the_delimiters_do_not_collide() {`

A length-prefixed encoding is injective; a delimiter-only one is not.
