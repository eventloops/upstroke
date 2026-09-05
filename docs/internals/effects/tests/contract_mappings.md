# `src/effects/tests/contract_mappings.rs`

Extended notes for [`src/effects/tests/contract_mappings.rs`](../../../../src/effects/tests/contract_mappings.rs).

The code is the authority for what it does. These notes started as the module's source prose.
Each code fragment in a heading is an exact source substring. When a heading names an enclosing
item before `›`, find that item first, then the following fragment within it.

## Module

The **contract mappings**: the three enumerations a frozen packet and its
reconciliation state, resolved against the tree that is supposed to satisfy
them.

Four transcribed tables and the one census that answers all four.
`T_CONTAINER_TESTS` is `transaction_fault_matrix` row
`T-CONTAINER`'s `test:` field; `PR6_REFUSALS`, `ST16_VARIANTS` and
`PR6_CLAUSES` are the three mappings the PR6 reconciliation said it had
supplied and had not. None of them is in this repository, so each is a
literal here for the reason `policy.rs` gives about
[`crate::effects::tests::policy::PACKET_PRIMITIVES`]: the no-self-oracle
rule requires the expected values to come from the document's own text
rather than from the tree they are checked against.

What resolves them is `defining_test_sites`, and it is deliberately one
predicate rather than two. Both bodies ask the same question of a name --
is it a `#[test]` function in real code, over source with comments and
string literals blanked -- and a second implementation of that question is
the shape `PR5D-VISIBILITY-CHECK-DUPLICATED` names: two hand-maintained
answers, one of which can break while the other stays green.

Everything they read with stays where it was. The tree reader
(`scanned_sources`) is `super`'s and the blanker is `crate::effects`'. This
file consumes them and re-derives neither.

**What is preserved here and not repaired.** `defining_test_sites` accepts a
`#[test]` anywhere in the 400 bytes preceding a signature, so a test
attribute far enough above a *different* function's signature is accepted;
the window is carried across byte-for-byte, because widening or narrowing it
changes what the two gates above accept and this is a move. The presence
boundary each gate states in its own words -- that a test with the right
name and a tautological body satisfies it completely -- is likewise the
docs' own, unedited.

**No name here is a test name.** The three `#[test]` wrappers stay in
`super` under the harness names the contract, CI and `--list` know, and the
three functions below are deliberately named otherwise -- so `--list` over
the test binary is unchanged and nothing nests under
`effects::tests::contract_mappings`.

**The R19 view-directory gate deliberately did not come with them**, and
that is what fixes this boundary. It is a mapping test by shape, but it
constructs a `ContainerName` to drive the mount side and the census side
against each other, and that identifier is one of the five needles
`runner::container::resolve::tests::no_module_outside_the_container_runner_writes_a_container_intent`
counts. That census scans the **whole** file rather than a production
region -- an inline `cfg(test)` module does not close it -- and it excludes
`src/effects/tests.rs` by exact path, with the exclusion's reason naming
that very test. A child holding it would need a second exclusion there,
which is a change to another slice's census rather than a consequence of
moving a declaration. So it stays with the harness: the same cut, for the
same reason, that left the effectful build helpers out of `policy.rs` and
the three regeneration writes out of `artifacts.rs`.

Measured rather than argued, for the three tables that did move: none of
them names `ContainerIntent`, `ContainerName`, `containers_dir`,
`CONTAINERS_DIR` or the funnel's `write_intent`, in code or in prose, so
that census's domain answers exactly what it answered before.

### Why the bodies sit inside a `cfg(test)` module

The reason `classification.rs` records. A file reached by a plain `mod`
declaration is inside every whole-tree census's domain, and the tables below
are dense with names those censuses read: nineteen plus twenty-eight test
identifiers, several of them container-substrate names. The inline module
closes it for both of the repository's source cutters at once --
[`crate::effects::production_region`] truncates at the first `#[cfg(test)]`
and [`crate::effects::production_code`] excises the item that attribute
attaches to -- so none of those names is in any census's region and this
file reads as the test logic it is.

It does so **without moving the whole-file module census**.
`census_domain::declared_whole_file_test_modules` derives a skip only from a
**terminated** declaration -- `mod name;` -- and an inline module with a
body opens a scope the scan reads declarations *inside* rather than naming a
file of its own. So
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
still resolves `cfg::WHOLE_FILE_TEST_MODULES` and no pinned test is renamed.

That terminated form is deliberately not spelled out here, for the reason
`policy.rs` gives: one written inside a comment is the exact shape that once
derived a phantom skip and removed a real file from every census below it,
and the blanking that now defeats it is not a reason to write another.

The `#![deny]` below deliberately stays **above** the cut. Blanking takes
the prose, so that attribute is all three whole-tree walks' per-file "this
region is empty" control has left to count here -- and a region that
collapses to nothing is exactly what that control exists to catch. It also
keeps `every_production_region_that_stops_early_stops_at_a_module` answering
what it answered: the first `#[cfg(test)]` in this file attaches to a
module, which is what that region is for.

The three effect denials are **restored** rather than inherited. `super`
allows them because it drives a compiler over fixtures it creates; nothing
in this file does -- both bodies read the tree and write nothing -- so the
allowance has no business reaching it. That is also what keeps this module
out of `effects/allowlist.toml`: an allowance is what that file records, and
this module takes none.

## `pub(super) mod mappings` › `const T_CONTAINER_TESTS: [&str; 19] = [`

-----------------------------------------------------------------------
The T-CONTAINER mechanical checklist
-----------------------------------------------------------------------

## `pub(super) mod mappings` › `const T_CONTAINER_TESTS: [&str; 19] = [`

The nineteen tests `transaction_fault_matrix` row `T-CONTAINER` names in its
`test:` field, transcribed from the frozen packet.

**Transcribed, not read.** The packet is not in this repository, so the list
is a literal here the way
[`PACKET_PRIMITIVES`](crate::effects::tests::policy::PACKET_PRIMITIVES)
is — the no-self-oracle rule requires the expected values to come from the
packet's text rather than from the tree, and a literal is the only shape
that survives into CI.

Order is the packet's own. `windows_orphan_window_documented` is the last
entry and the packet writes it as `windows_orphan_window_documented (ST-16)`;
the trailing citation is not part of the identifier.

## `pub(super) mod mappings` › `fn defining_test_sites(name: &str) -> Vec<String> {`

Where `name` is defined as a `#[test]` function, over code with comments and
string literals blanked.

Blanked, because the failure this predicate exists to avoid is a name that
appears only in prose. Nine of the nineteen are quoted in a doc comment
somewhere in `src/runner/container/**` — `substituted_image_id_refused_
before_start` is named in `runtime.rs` and twice in `fake.rs` and is a
function in neither — so a `grep` for the bare string passes on a tree that
deleted the test and kept the sentence describing it.

## `fn defining_test_sites(name: &str) -> Vec<String>` › `let preceding = &code[index.saturating_sub(400)..index];`

`#[test]` sits above the signature, separated at most by the other
attributes a test carries (`#[cfg(...)]`, `#[should_panic]`) and by
the doc comment, which blanking has already turned into spaces.

## `pub(super) mod mappings` › `pub(in crate::effects::tests) fn every_fault_row_name_is_a_test_in_the_tree() {`

Every test `T-CONTAINER` names exists in this tree, as a test.

**The gate no gate was reading.** `phase9.sh` reads
`decisions.pr_sequence[N].slice_contract.proof_tests` and fails a slice that
deletes or renames one of its contract-named proof tests — the repair for
`PR4-CONTRACT-NAMED-PROOF-TEST-DELETED`. All **four** of PR6's `proof_tests`
are prose describing test families, so that gate parses zero identifiers out
of this slice and its zero-checked-is-a-failure rule fires without measuring
anything. The slice's actual mechanical checklist is somewhere else
entirely: `transaction_fault_matrix` row `T-CONTAINER`'s `test:` field, which
nothing in this repository read.

**This gate is orchestrator-added, not packet-required**, and says so rather
than implying otherwise. The packet enumerates the nineteen tests; it does
not require a meta-test that transcribes them. It is a control, kept because
a slice whose only mechanical checklist is unread is worse off without one.

### What this proves, and what it does not

**Proves:** each of the nineteen names is a `#[test]` function in real code
— not in a comment, not in a string literal, not merely a helper `fn` with
the right name. A rename, a deletion, or a demotion to a plain function
fails it by name, on every platform, because it is a source census rather
than a symbol census (two of the nineteen are behind `cfg(unix)` /
`cfg(windows)` and a symbol census would report each missing on the other
platform).

**Does not prove:** that any of them tests what its name claims. A test with
the right name and a tautological body satisfies this gate completely. That
is the boundary, stated here rather than left for a reviewer to find: this
is a **presence** gate over an enumeration nothing else reads, and the
evidence that the nineteen hold their clauses is the mutation witnessing in
the lanes' own reports, not this.

The second field it holds constant is the **body**; what varies is the
name and the file. The controls at the end vary the other way — one body
shape at a time, name held fixed — so the predicate is shown refusing a
comment, a string and a plain `fn`, and accepting a real test.

## `pub(in crate::effects::tests) fn every_fault_row_name_is_a_test_in_the_tree() {` › `let unique: BTreeSet<&str> = T_CONTAINER_TESTS.iter().copied().collect();`

The transcription itself is checked for the two ways a hand-written list
decays: a duplicate (which would let a missing name hide behind a present
one and keep the count at nineteen) and a name that is not an identifier.

## `pub(in crate::effects::tests) fn every_fault_row_name_is_a_test_in_the_tree() {` › `assert!(`

POSITIVE CONTROL. A census that can only say yes reports success from a
predicate that matched nothing -- `PR5-DOCKER-CENSUS-CANNOT-FAIL`, where a
needle that lived inside a string made the search unfalsifiable. Drive the
same predicate over a name that is not in the tree and require it to say
so, so a `defining_test_sites` that returned a constant fails here.

## `pub(in crate::effects::tests) fn every_fault_row_name_is_a_test_in_the_tree() {` › `let (_, container) = scanned_sources()`

And it must be reading a tree. `scanned_sources` asserts its own walk
found files; this asserts the *blanking* left code behind, because a
blanker that erased everything would make every name absent and the
failure would read as nineteen deleted tests.

## `pub(super) mod mappings` › `pub(in crate::effects::tests) fn the_presence_predicate_refuses_a_non_test_shape() {`

The presence predicate refuses every shape that is not a test.

Separated from the gate above so a failure says which half broke: the tree,
or the thing that reads it. Each source varies exactly one property against
the accepted shape and holds the name fixed.

## `pub(in crate::effects::tests) fn the_presence_predicate_refuses_a_non_test_shape() {` › `let accepted = format!("#[test]\nfn {name}() {{ assert!(true); }}\n");`

Accepted: a real test.

## `pub(in crate::effects::tests) fn the_presence_predicate_refuses_a_non_test_shape() {` › `for (label, source) in [`

Refused, one property changed at a time.

## `pub(super) mod mappings` › `const PR6_REFUSALS: [(&str, &str, &str); 9] = [`

-----------------------------------------------------------------------
R3b: the enumerations the reconciliation promised and did not supply
-----------------------------------------------------------------------

## `pub(super) mod mappings` › `const PR6_REFUSALS: [(&str, &str, &str); 9] = [`

The nine `expected_failures_refusals`, each with the **ordering predicate**
it carries and the test that holds it.

`PR6-ENUM-011`. The reconciliation document states that the nine refusals
and the twelve ST-16 variants "are mapped" and never supplies the mappings,
so a clause with neither a named test nor an owned deferral was
indistinguishable from one with both. A promise in a markdown file is not
something a build can read; this is.

`(clause, ordering predicate, test)`. The ordering is written out because it
is the **independently droppable** half: a refusal test that proves only
*that* it refused holds none of "before any effect", "before any lock or
effect", "before any spawn", "before start", "before any recovery event", or
"by construction".

## `pub(super) mod mappings` › `const ST16_VARIANTS: [(char, &str, &str); 12] = [`

The twelve ST-16 variants (a)–(l), each mapped to the test that drives it.

`PR6-ENUM-011`. `T_CONTAINER_TESTS` is the packet's `test:` field and is a
*presence* list; this is the **variant** enumeration, which is a different
axis — several variants share a named test and one variant is carried by a
test the `test:` field does not name.

## `pub(super) mod mappings` › `const PR6_CLAUSES: [(&str, &str); 12] = [`

The clauses of `invariants_introduced` and of ST-20 that this slice owns,
each with a test **or** an owned deferral.

`PR6-ENUM-011`. The reconciliation decomposed neither, so descendant
containment, resumed-epoch attribution and report/status attribution had
neither a named test nor an owner. A deferral is written as
`defer:<slice>` and is as much an answer as a test name — what is not an
answer is silence.

## `pub(super) mod mappings` › `pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {`

Every enumeration the reconciliation promised is supplied here, and every
entry either names a test that exists or defers to a named slice.

`PR6-ENUM-011`. Three separate claims, each of which the document made and
none of which anything read:

1. the **nine** refusals are mapped — and to an *ordering predicate* as well
   as to a test, because the ordering is the droppable half;
2. the **twelve** ST-16 variants (a)–(l) are mapped;
3. `invariants_introduced` and the prose `proof_tests` are decomposed into
   clauses, each with a test **or an owned deferral**.

A name that is not a `#[test]` in this tree fails here, through the same
[`defining_test_sites`] census `T_CONTAINER_TESTS` uses — so this cannot be
satisfied by prose, by a helper function with the right name, or by a string
in a comment.

**What this does not prove**, stated for the same reason the gate above
states it: that the named test holds the clause. This is a *mapping* gate.
The evidence that the clauses hold is the mutation witnessing recorded in
the repair reports.

## `pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {` › `assert_eq!(PR6_REFUSALS.len(), 9, "the contract states nine refusals");`

(1) The nine refusals, with distinct clauses and distinct orderings.

## `pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {` › `assert_eq!(ST16_VARIANTS.len(), 12);`

(2) The twelve ST-16 variants, (a)-(l), each present exactly once.

## `pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {` › `let deferred: Vec<&str> = PR6_CLAUSES`

(3) The clause decomposition, with deferrals owned by a named slice.

## `pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {` › `let named: Vec<&str> = PR6_REFUSALS`

Every name that is not a deferral is a `#[test]` in this tree.

## `pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {` › `for (letter, _, test) in &ST16_VARIANTS {`

And the ST-16 mapping is consistent with the packet's own `test:` field:
every variant's test that appears there appears under the same name.

## `pub(in crate::effects::tests) fn every_promised_mapping_names_a_test_or_an_owner() {` › `assert!(`

A variant carried by a test the `test:` field does not name is
allowed and must be visible, not silent.
