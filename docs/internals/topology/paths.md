# `src/topology/paths.rs`

Extended notes for [`src/topology/paths.rs`](../../../src/topology/paths.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The path vocabulary a schema-4 run leases and rejects by.

Two tasks may run in parallel exactly when the regions of the repository
they touch do not overlap, so "which paths" is a fact the log has to record
rather than recompute. It is recorded twice, deliberately: a **predicted**
region taken from the plan's path hints when a task is dispatched, and an
**actual** region taken from the diff when its candidate is prepared. The
prediction is what admission can know; the actual set is what the merge
queue is entitled to trust.

Both are [`PathSet`]s, and both can be [`PathSet::RepoWide`] — the answer
for a task that gave no usable hint, and the answer for a diff whose byte
paths did not decode. Repo-wide overlaps everything, so an unparsable
answer costs parallelism and never costs correctness. That asymmetry is the
whole reason the variant exists rather than an empty set or an error.

How the two are compared is the fold's business and arrives with it; what
is here is the frozen record itself, including the [`PathPolicy`] the run
resolved once and every later comparison must be read against.

## `pub struct PathPolicy {`

The comparison rules a run froze at pre-flight.

Versioned because path comparison is execution identity in the same sense
effort and reviewer bindings are: a run that admitted two tasks in parallel
under one case-folding rule must not have its later half admitted under
another because the machine changed.

## `pub struct PathPolicy` › `pub case_fold: bool,`

Whether two paths differing only in case name the same file. Resolved
from the repository's filesystem, not guessed per comparison.

## `pub struct PathPolicy` › `pub grammar: PathGrammar,`

The syntax the plan's hints are written in.

## `pub enum PathPolicyVersion {`

Which generation of the comparison rules a record was written under.

## `pub enum PathPolicyVersion` › `V1,`

Component-wise equal/ancestor/descendant overlap, literal prefix taken
before the first glob metacharacter, repo-wide for anything unsafe.

The derivation before 2026-09-06. It cut inside a component, so
`src/eng*` bounded `src/eng`, and it read a backslash as a separator,
so `src\foo` bounded `src/foo`. The spelling is kept so a log written
under it still decodes and can be refused by name rather than as a
serde error.

## `pub enum PathPolicyVersion` › `V2,`

Component-wise equal/ancestor/descendant overlap; the prefix is the
whole components before the first component that carries a glob
metacharacter; a hint with a `.` or `..` component, or with **any**
backslash, derives repo-wide.

`src/eng*` therefore bounds `src`, not `src/eng`, which
[`crate::topology::leases::paths_overlap`] does not match to the
`src/engine/mod.rs` the hint covers. A backslash bounds nothing
because the character has two readings and neither is safe to guess:
under the frozen [`PathGrammar::Globset`] it is an escape on Unix, so
`src/foo\?bar.rs` matches the single file `src/foo?bar.rs`, and it is
a separator on Windows, so the same hint names `src/foo/?bar.rs`. A
prefix that picks one reading is narrower than the hint under the
other, and a predicted region narrower than its hint is what admits
two owners of one file at once. The derivation refuses rather than
guesses.

This is the derivation this binary applies, and it applies no other:
a run's durable dispatch records carry the regions its own version
derived, so `check_run_started` refuses a run frozen under any
earlier version rather than replaying it under this one.

## `pub enum PathGrammar {`

The syntax a plan's path hints are interpreted in.

## `pub struct GitPath(pub String);`

One repository path as Git names it: forward slashes, relative to the repo
root, and never a filesystem path on the machine reading it.

Distinct from [`std::path::PathBuf`] on purpose. A recorded region has to
mean the same thing on the Windows machine that resumes the run as on the
Linux one that wrote it, and a platform path type would make that a
question about separators. Paths that did not decode are never stored: the
classification becomes [`PathSet::RepoWide`] instead, which is why this can
be a `String` without losing the byte-safe answer.

## `pub enum PathSet {`

A region of the repository.

## `pub enum PathSet` › `RepoWide,`

Everything. The classification for an absent, unsafe, unparsable, or
undecodable answer — and therefore the one that must never be produced
by accident, because it serializes every task against every other.

## `pub enum PathSet` › `Prefixes { paths: Vec<GitPath> },`

The literal prefixes a region is bounded by.

## `impl PathSet` › `pub fn is_repo_wide(&self) -> bool {`

Whether this region is the everything region.

## `impl PathSet` › `pub fn prefixes(&self) -> Option<&[GitPath]> {`

The prefixes bounding this region, or `None` when it is unbounded.

`Some(&[])` is a real and different answer from `None`: a task whose
diff touched nothing has an empty region that overlaps no bounded
region, while a task whose paths could not be read has an unbounded one
that overlaps everybody.

The two are not each other's complement, and the asymmetry is the point.
[`crate::topology::leases::regions_overlap`] answers `false` for the
empty region against any bounded one and `true` for it against
[`PathSet::RepoWide`]: `RepoWide` overlaps it, because the safe reading
of a region nobody could read is that it might be anywhere, and nothing
can be excluded from a region that might be anywhere. So an empty region
frees a task from every bounded holder and from none of the unread ones.
`docs/internals/topology/leases.md` states the same exception from the
comparison's own side, and
`regions_overlap_component_wise_and_repo_wide_overlaps_everything`
asserts both directions of it.

## `mod tests` › `fn hostile_prefixes() -> Vec<GitPath> {`

Hostile prefixes: deranged against sorted order, mixed case, padded,
multi-byte, and long enough that a truncating writer would show.

## `fn hostile_policy() -> PathPolicy` › `case_fold: true,`

Off-default: `bool::default()` is false, and a policy that lost
this field would still deserialize to the common case.

## `fn path_policy_round_trips_every_field_it_records()` › `assert!(json.contains(r#""version":"v2""#), "{json}");`

Named fields, not positional: a record whose keys were renamed would
still round-trip, and a resume reading a differently-named record
would fall back to a default it must never fall back to.

## `fn a_path_policy_refuses_an_unknown_field()` › `let json = r#"{"version":"v2","case_fold":true,"grammar":"globset","ordering":"lexical"}"…`

The policy is execution identity; a field this binary does not
understand means the record was written under rules it cannot apply.

## `mod tests` › `let intruders: [(&str, &str, &str); 3] = [`

A fixture that only ever *adds* an unknown key next to a complete
record is satisfied by an alias: the record is refused as a
duplicate, not as an unknown field. The replacement form is the one
that distinguishes them — with the real key removed, an aliased
spelling deserializes and the policy is silently accepted under a
name the frozen shape does not define.

Every hostile key is same-typed with the field it replaces, so a
type error cannot stand in for the refusal either.

## `mod tests` › `let mut replaced: serde_json::Value =`

(a) in place of the required field: the record is incomplete and
    the intruder is unknown, and both are refusals.

## `mod tests` › `let mut added: serde_json::Value =`

(b) in addition to it: still unknown, and refused for being so
    rather than for a field that is missing.

## `fn both_case_fold_values_survive_the_wire_exactly_as_writte…` › `let expectations = [`

`case_fold` is an independent boolean that decides whether two paths
differing only in case name the same file. Every fixture that sets
it to one value permits a writer that hard-codes that value: replay
would then turn a case-sensitive run into a case-folding one and
change every overlap decision the merge queue made.

The expected payloads are written out here rather than produced by
the serializer, so the assertion is about the frozen encoding rather
than about serde agreeing with itself.

## `fn both_case_fold_values_survive_the_wire_exactly_as_writte…` › `assert_eq!(policy.case_fold, case_fold);`

And the two encodings are different documents, so a serializer
that emitted a constant would collide here.

## `fn an_unsupported_policy_version_or_grammar_spelling_is_ref…` › `for version in ["v3", "V1", "v1 ", "", "v10", "v0"] {`

The frozen authority defines exactly two versions and one grammar. A
record declaring any other one was written under rules this binary
does not implement, and reading it as one of them would apply the
wrong comparison to every lease the run took.

## `fn an_unsupported_policy_version_or_grammar_spelling_is_ref…` › `assert_eq!(`

The canonical spellings, so the negatives above cannot be satisfied
by refusing everything -- including `v1`, whose spelling has to keep
decoding for the fold to refuse a v1 run by name rather than as a
malformed line.

## `fn a_path_policy_refuses_a_missing_field_rather_than_defaul…` › `for absent in ["version", "case_fold", "grammar"] {`

Each field removed in turn: `case_fold` is the dangerous one, since
a default would silently pick the case-sensitive comparison.

## `fn the_three_regions_are_distinguishable_on_the_wire()` › `let repo_wide = PathSet::RepoWide;`

Repo-wide, empty, and non-empty are three different answers and the
most damaging confusion is between the first two: an unbounded
region serialized as an empty one overlaps no bounded region and would
admit every task in parallel against every other.

## `fn prefixes_survive_in_the_order_and_bytes_they_were_record…` › `let bounded = PathSet::Prefixes {`

Not sorted, not trimmed, not normalized: the recorded region is
evidence about a past diff, and a writer that tidied it would make
two different diffs indistinguishable.

## `mod tests` › `const LONG_PREFIX_LITERAL: &str =`

The longest hostile prefix, written out as a literal rather than
produced by [`GitPath::from`]. An oracle built through the constructor
is truncated by exactly the mutation it is supposed to catch.

## `mod tests` › `const HOSTILE_REGION_JSON: &str = concat!(`

The canonical encoding of the hostile region, written by hand. Not
produced by the serializer, so it detects a change to the encoding
rather than agreeing with whatever the encoding currently is.

## `fn an_over_length_path_keeps_every_byte_it_was_given()` › `assert_eq!(LONG_PREFIX_LITERAL.len(), 88);`

The oracle is the literal above, not a second call to the
constructor: comparing `GitPath::from(x)` against `GitPath::from(x)`
normalizes both sides identically, so a constructor that truncated,
trimmed, or lower-cased its input would agree with itself and the
recorded region would silently name a different part of the tree.

## `fn an_over_length_path_keeps_every_byte_it_was_given()` › `assert_eq!(`

Through the wire too, against a hand-written payload.

## `fn an_over_length_path_keeps_every_byte_it_was_given()` › `let recorded = PathSet::Prefixes {`

And in place: index 3 of the hostile set is the long one, and the
earlier byte assertions in this module cover 0, 1 and 4 only.

## `fn every_region_encodes_to_the_payload_written_out_here_and…` › `let cases: [(PathSet, &str); 3] = [`

Round trips compare one serde implementation against itself, so a
symmetric rename of `region`, `paths`, `repo_wide` or `prefixes`
changes the durable format invisibly. These payloads are written by
hand, so any such change fails here in both directions.

## `fn the_notes_give_the_empty_region_the_repo_wide_exception_the_comparison_gives_it() {`

`ASTRA165-004`. The `prefixes` section said an empty region overlaps
nobody, and the wire section said an unbounded region written as an empty
one does; `regions_overlap` has never said either, because its
`(None, _) | (_, None)` arm answers `true` before it looks at a single
prefix. `docs/internals/topology/leases.md` carried the exception all
along, so the two files disagreed about the same function.

A text pin and only a text pin: it asserts what these two sections must
state and refuses each retired sentence by name. It does so only while
`regions_overlap` still answers `true` for the empty region against
`PathSet::RepoWide` and `false` for it against a bounded one, so the code
fact conditions the prose — a comparison that changed its answer fails
here with the reason rather than silently pinning a note that has become
wrong in the other direction. The behaviour itself stays where it is
asserted, in
`regions_overlap_component_wise_and_repo_wide_overlaps_everything`.

Matched on the prose, not on where its line breaks fall: a reflow must
not break the pin, only a changed claim.

## `fn a_git_path_is_transparent_on_the_wire()` › `let path = GitPath::from("src/Zebra/ÜBER.rs");`

A bare string, so a recorded region reads as one in `jq` and in the
file itself. A wrapper object here would change every recorded set.
