# `src/effects/tests/classification.rs`

Extended notes for [`src/effects/tests/classification.rs`](../../../../src/effects/tests/classification.rs).

The code is the authority for what it does. These notes started as the module's source prose.
Each code fragment in a heading is an exact source substring. When a heading names an enclosing
item before `›`, find that item first, then the following fragment within it.

## Module

`(3) Wrapper classification`: the four checks that hold
`effects/wrappers.toml` against the tree it classifies.

The domain derivation, both directions of the effectful/denied
correspondence, the funnel rows, and the `libc::` sweep. All four *read* --
`effects/wrappers.toml`, `clippy.toml`, and the modules those two name --
and none of them writes anything or starts a process.

Everything they read with stays where it was. The schemas and readers
(`ModuleClassification`, `wrappers`, `denylist`, `scanned_sources`,
`repo_root`) are `super`'s, and the production scanners
(`externally_reachable_fns`, `production_region`,
`blank_comments_and_strings`) are `crate::effects`'. This file consumes
them; it re-derives none of them.

**No name here is a test name.** The four `#[test]` wrappers stay in `super`
under the harness names the contract and CI know, and the four functions
below are deliberately named otherwise -- so `--list` over the test binary is
unchanged and nothing nests under `effects::tests::classification`.

### Why the bodies sit inside a `cfg(test)` module

A file reached by a plain `mod` declaration is inside every whole-tree
census's domain. That is the constraint `policy.rs` records, and the one
that kept the effectful build helpers out of it. The inline module closes it
here for both of the repository's source cutters at once:
[`crate::effects::production_region`] truncates at the first `#[cfg(test)]`
and [`crate::effects::production_code`] excises the item that attribute
attaches to, so the four bodies are outside both regions and this file reads
as the test logic it is.

It does so **without moving the whole-file module census**.
`census_domain::declared_whole_file_test_modules` derives a skip only from a
**terminated** declaration -- `mod name;` -- and an inline module with a
body opens a scope the scan reads declarations *inside* rather than naming a
file of its own. So
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
still resolves `cfg::WHOLE_FILE_TEST_MODULES` and no pinned test is renamed.
Measured, not argued, and re-measured when W1 grew the set: declared the
other way, this file joins that test's named set as a seventh
`["agent/proc/test_support/readiness.rs", "effects/tests/classification.rs",
"engine/topology/scaffold.rs", "events/log/premove.rs",
"rundir/scratch_tree.rs", "runner/container/fake.rs",
"workspace_manager/fixture.rs"]` against its expected six, and the
comparison below it resolves one module more than
`WHOLE_FILE_TEST_MODULES` lists.

That terminated form is deliberately not spelled out here, for the reason
`policy.rs` gives: one written inside a comment is the exact shape that once
derived a phantom skip and removed a real file from every census below it,
and the blanking that now defeats it is not a reason to write another.

The neighbour that makes the shape legible is
`src/runner/container/census/tests.rs`, whose bare `this_file_is_test_only`
marker module closes the *region* half only -- `production_code` excises the
marker and then scans that file in full -- and what keeps it out of the
whole-tree censuses is a real declaration one level up. This file can have
neither, so it wraps the bodies rather than marking above them.

The `#![deny]` below deliberately stays **above** the cut. Blanking takes
the prose, so that attribute is all three whole-tree walks' per-file "this
region is empty" control has left to count here -- and a region that
collapses to nothing is exactly what that control exists to catch.

The three effect denials are **restored** rather than inherited. `super`
allows them because it drives a compiler over fixtures it creates; nothing
in this file does, so the allowance has no business reaching it. Measured
rather than believed: one probe -- a `println!`, a `std::fs::write` and a
`std::process::Command` -- is refused three times here and emits no
`disallowed_*` at all from the identical lines in `tests.rs`, so the `deny`
is load-bearing and not a restatement of an ambient rule. That is also what
keeps this module out of `effects/allowlist.toml`: an allowance is what that
file records, and this module takes none.

## `pub(super) mod checks` › `pub(in crate::effects::tests) fn reachable_fns_are_classified() {`

Every externally reachable `fn` of a legacy or shared module is classified.

The domain is **derived from the modules**, not listed: a `pub fn` added to
one of them fails this test until somebody decides what it is. That is the
only half of `mechanism` (3) a test can hold — the classification itself is
a review — and it is the half that omission attacks.

## `pub(in crate::effects::tests) fn reachable_fns_are_classified() {` › `let classified: Vec<&str> = module`

A row may carry its receiver (`Workspace::branch_exists`) so the
denied path can name it; the domain is over bare fn names.

## `pub(super) mod checks` › `pub(in crate::effects::tests) fn effectful_wrappers_are_denied() {`

"effectful wrappers are added to the disallowed list themselves".

## `pub(in crate::effects::tests) fn effectful_wrappers_are_denied() {` › `let path = format!("{}::{name}", module.crate_path);`

`Type::method` is recorded as written, so an inherent method keeps
its receiver in the path clippy has to resolve.

## `pub(in crate::effects::tests) fn effectful_wrappers_are_denied() {` › `let classified: BTreeSet<String> = record`

The other direction: every crate-internal denial is a row somebody
classified. A `upstroke::…` entry nobody classified is a denial with no
review behind it.

## `pub(super) mod checks` › `pub(in crate::effects::tests) fn funnel_rows_name_a_site() {`

A row classified `funnel` really does name a site.

## `pub(in crate::effects::tests) fn funnel_rows_name_a_site()` › `let path = format!("{}::{name}", module.crate_path);`

A funnel is not a wrapper: it must not also be denied.

## `pub(super) mod checks` › `pub(in crate::effects::tests) fn libc_items_are_classified_and_denied() {`

Every `libc::` item the tree names is classified effect or not-an-effect, and
every one classified an effect is denied.

`claim_scope` makes exhaustiveness "the disallowed list is complete for the
**primitives the crate uses**", so the list is derived from the tree rather
than transcribed from the sentence's `fork/kill/setpgid/setsid/flock/fcntl/
exec*` — which is six names out of the twenty-four this crate actually calls.

## `pub(in crate::effects::tests) fn libc_items_are_classified_and_denied() {` › `let effects: BTreeSet<&str> = record.libc.effect.iter().map(String::as_str).collect();`

The other direction, or a reclassification would be free: moving an item
from `effect` to `not_an_effect` would leave its denial in place with
nothing behind it, and the first assertion could not tell.
