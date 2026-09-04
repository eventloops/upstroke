# `src/effects/tests.rs`

Extended notes for [`src/effects/tests.rs`](../../../src/effects/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The enforcement layer's tests: the allow-placement scan, the frozen legacy
section, the wrapper classification, the generated inventories, and the
build refusals whose *reason* is pinned.

Three rules this project pays for when it forgets them are load-bearing
here:

* **A function may not be its own oracle.** The denylist is checked against
  [`PACKET_PRIMITIVES`], transcribed from
  `decisions.effect_site_inventory.mechanism`'s own sentence, never against
  itself. The site inventory is checked against the enums.
* **Enumerations come from the types and the packet.** The site grid iterates
  `EffectSiteId::all()`; the classification domain is derived by parsing the
  modules, not by listing what came to mind.
* **A refusal is executed, not inferred.** Every "this is refused" claim here
  is driven with input that *does* the forbidden thing — a legacy list that
  grows, an entry that names a topology module, an allow below module level —
  because a refusal only ever measured against compliant input is a refusal
  nobody has seen fire.

## `#![allow(`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
carries this module's review clause -- effects only inside site-taking APIs,
no writable handle returned. `decisions.effect_site_inventory.mechanism` (2).

## `mod policy;`

The definitions these tests are checked against -- the two packet tables, the
host-conditional denials and the placement scan's prologue reader -- are
beside this file. The machinery that *drives* a compiler is not: it stays
here, because this file is a whole-file test module and `policy.rs` is not,
so a `Command::new(` moved there would enter two production censuses in
`src/runner/mod.rs` and have to be classified in them.

## `mod classification;`

The wrapper classification's four checks are beside this file too, and they
go one step further than `policy.rs`: their bodies sit inside a `cfg(test)`
module, so both source cutters read that file as test logic. An inline module
with a body is not the terminated declaration `census_domain` derives a skip
from, so the whole-file module census is untouched by it.

## `fn repo_root() -> PathBuf {`

---------------------------------------------------------------------------
Reading the tree and the artifacts
---------------------------------------------------------------------------

## `pub(in crate::effects) fn crate_roots() -> &'static CrateRoots {`

This package's target inventory, read once from `cargo metadata`.

**The acquisition lives here and the authority lives in
[`census_domain`](crate::effects::census_domain).** Reading a manifest means
starting a process, and `effects/allowlist.toml` records `allows = []` for
`src/effects.rs` on the strength of that file carrying no attribute and
reaching no denied primitive — "a stronger claim than any other entry in
this section makes", in the row's own words. This file is where the
machinery that drives a toolchain already lives, for the reason its prologue
gives: it is a whole-file test module, so a `Command::new(` in it is not in
any production census's domain. So the process start is here, the parse and
the resolution are beside the census that uses them, and neither half is
somewhere it would have to be argued for.

### Panics

When the inventory cannot be established. That is the fail-closed half of
`PR72-TARGETS-001`: which files Cargo compiles as crate roots decides which
file every `mod name;` in the tree resolves to, and a census that cannot
read the manifest must stop rather than fall back to a rule about file
stems — the rule this replaces, whose failures resolve to a real sibling
instead of announcing themselves.

## `pub(in crate::effects) fn crate_roots_of(`

The inventory of the package whose manifest sits in `manifest_dir`.

Separate from [`crate_roots`] and taking a directory, because a control that
only ever runs against this tree's own manifest cannot show what the reader
does with an arbitrary `[[bin]] path` — and an arbitrary `[[bin]] path` is
the whole of what the stem rule got wrong.

## `fn cargo_metadata_json(manifest: &Path) -> Result<String, InventoryRefusal> {`

`cargo metadata` for one manifest, as its stdout.

`--no-deps` because only this package's targets are wanted, `--offline`
because a census must not depend on a network, and the cargo the test binary
was built by rather than whichever one is first on `PATH`: the MSRV job runs
`cargo +1.85.0`, and an inventory read by a different toolchain than the one
compiling the tree is an inventory of something else.

## `fn scanned_sources() -> Vec<(String, String)> {`

Every `src/**/*.rs` and `examples/**/*.rs`, as `(repo-relative path, source)`.

`examples/**` is beyond the mechanism sentence's `src/**/*.rs` and is scanned
anyway: `cargo clippy --all-targets` compiles examples, so an ungoverned
example is a hole in the same wall. Scanning wider can only find more.

## `struct AllowlistEntry` › `expect_sites: usize,`

How many **per-site** `#[expect(…)]` attributes of the recorded lints the
file carries, or zero when its allowance is the module-level one.

`standards/02_standards_automated_baseline.md`. A per-site
expectation is narrower than a module-level allow and the compiler owns
its count in both directions; this is the reviewed number that count is
checked against, so an annotation appearing or vanishing has to pass
through a row a reviewer reads.

## `struct ClippyToml` › `allow_expect_in_tests: bool,`

The §7 panic-policy allowances (CODING_STANDARDS.md §7), which arrived
with master's lint mechanization. They configure clippy's own lints
rather than naming an effect primitive, so `all()` deliberately excludes
them. They are declared because `deny_unknown_fields` above is the
mechanism that turns an unclassified clippy.toml key into a failure --
the correct response to a new key is to classify it here, never to
relax the attribute -- and they are asserted by
`clippy_toml_turns_the_allowances_on_and_gives_unwrap_none` so a
field this file merely parses cannot drift unobserved.

## `fn clippy_toml_turns_the_allowances_on_and_gives_unwrap_none() {`

`clippy.toml` turns the three §7 allowances on, and gives `.unwrap()` none.

**It reads `clippy.toml`, not the standard**, and is named for that. An
earlier name claimed the allowances were "exactly what the standard
states", which this test cannot know: parsing §7's prose to compare would
be a text checker over an open-ended surface, and PR #25 is five review
rounds of evidence that those do not converge.

CODING_STANDARDS.md §7: tests fail their own setup with `.expect(` and a
message, use `panic!` in their own assertion helpers, and may print;
`.unwrap()` "is denied everywhere, tests included" because it carries no
diagnostic. A `false` here would silently re-deny a form 4,100 call sites
use, and an `unwrap` allowance appearing would silently permit one the
standard refuses -- so both directions are asserted rather than assumed.

## `struct ModuleClassification` › `crate_path: String,`

The path a denied entry would name this module by, or empty when the
module is not reachable from outside its parent (a private `mod`, or the
binary crate root).

## `fn the_readiness_expectations_are_per_site_and_both_records_say_so() {`

---------------------------------------------------------------------------
(2) The allow-placement scan
---------------------------------------------------------------------------

## `fn the_readiness_expectations_are_per_site_and_both_records_say_so() {`

`mechanism` (2), executed over the tree.

Four things, and the fourth is the one a scan usually leaves out: an
attribute's lint set must **equal** what the allowlist records, so a widening
is a failure rather than a silent extra.
**The readiness allowance is six per-site expectations, and every record
says the same six.**

`PR72-PLACEMENT-001`. The file used to open with a blanket
`#![allow(clippy::disallowed_methods)]`, and the census that guarded it —
`runner::container::tests::the_readiness_allowance_names_the_paths_it_is_\
written_against` — had to be the authority on which primitives the file
reaches, because nothing else was: it derives the denied set from
`clippy.toml` and compares it for equality, which is the only version of
that census worth having while a whole file is allowed.

It is not the authority any more. The lint is **denied** at file scope and
each of the six call sites carries its own
`#[expect(clippy::disallowed_methods, reason = …)]`, so under the
`-D warnings` the gate runs with, the compiler owns the count in both
directions: a seventh denied call is an error, and a site that stops
reaching a denied path is `unfulfilled_lint_expectations`. What is left for
a test is **documentation synchronisation** — that the file's prologue, the
six annotations and the `effects/allowlist.toml` row still say the same
thing — and that is all this does. The arithmetic census upstream keeps
its own job, which is now a second, independent reading of the same tree
rather than the only one.

Every needle here is contained in one line, deliberately: `PR72-WIN-EOL-003`
was two controls that searched for byte sequences spanning a line, which are
`\r\n` on the guest and hold on Unix and nowhere else. A needle that cannot
span a line ending cannot have that bug, so the records are written to keep
their phrases on one line rather than being folded back together here.

## `fn the_readiness_expectations_are_per_site_and_both_records…` › `const SPELLED: [&str; 8] = [`

The records are prose and spell the count as a word. The two are bound
rather than restated: changing `SITES` without changing the word fails
here instead of quietly searching for a phrase no record contains.

## `fn the_readiness_expectations_are_per_site_and_both_records…` › `for lint in USED_GOVERNED_LINTS {`

(1) **All three governed lints are denied at file scope, and none is
allowed there.** The deny is what makes an expectation a narrowing.

## `fn the_readiness_expectations_are_per_site_and_both_records…` › `let found = governed_allows(&source);`

(2) **Exactly six per-site expectations, each of them an `expect` of that
one lint, below module level, with a reason that names which site it is.**
The indices are asserted as a set: six annotations that all said "site 1
of 6" would satisfy a count and would mean the file had been copied
rather than read.

## `fn the_readiness_expectations_are_per_site_and_both_records…` › `let indices: BTreeSet<usize> = (1..=SITES)`

The reasons are read out of the source rather than out of the attribute
scan, because the scan blanks string literals — which is what keeps a
fixture in a doc comment invisible, and what means the reason's text has
to be read from the file itself.

## `fn the_readiness_expectations_are_per_site_and_both_records…` › `let list = allowlist();`

(3) **The row records the same lint and the same count**, and names the
decision that admits a per-site expectation at all.

## `fn the_readiness_expectations_are_per_site_and_both_records…` › `let phrase = format!("five distinct denied paths across {sites_in_words} sites");`

(4) **The prose in both records states the count, on one line each.**

## `fn the_readiness_expectations_are_per_site_and_both_records…` › `assert!(`

(5) **The decision exists.** A record cited by two files and absent from
the tree is a citation nobody can follow.

## `fn file_level_denies(source: &str, lint: &str) -> bool {`

Whether `source`'s file-module prologue **denies** the governed `lint`.

`deny` and `forbid` both are: each makes the lint a build error for the whole
module tree, which is what a per-site expectation has to be narrowing.
`bare` because a row records `clippy::disallowed_methods` and a prologue may
write either spelling; the reader normalises both.

## `fn every_allow_of_a_governed_lint_is_module_level_and_in_th…` › `if !allow.module_level`

**The one shape permitted below module level**, and every clause
of it is load-bearing. `decisions/2026-08-30-readiness-lint-\
placement.md` amends `mechanism` (2)'s "only as module-level
attributes" for a per-site `#[expect]` and nothing else: an
`expect` the compiler refuses when it goes unfulfilled, carrying
its own reason, in a file that DENIES the lint at module level so
the expectation narrows a denial instead of decorating an
inheritance, and counted in a row a reviewer read.

## `fn every_allow_of_a_governed_lint_is_module_level_and_in_th…` › `for (path, (entry, _)) in &recorded {`

A row recording per-site expectations for a file the scan never reached
is a row nothing checks. The count above only runs for files the scan
found an attribute in.

## `fn every_allow_of_a_governed_lint_is_module_level_and_in_th…` › `for (path, (entry, _)) in &recorded {`

A file listed with a non-empty `allows` and no attribute is a stale entry;
a scan that found nothing is a scan that proves nothing.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_level_and_sees_through_no_disgu…`

The scan refuses what it is for — driven with input that breaks each rule.

A placement scan only ever run against a compliant tree is a scan nobody has
seen refuse anything. Every case here is synthetic and every one asserts a
*different* discriminator, so a scan that collapsed to "returns true" would
fail on the counts rather than pass on the cases.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let on_a_function = "#[allow(clippy::disallowed_methods)]\nfn go() {}\n";`

(1) A function-level allow is found and is not module-level.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let on_a_statement = "fn go() {\n    #[allow(clippy::disallowed_methods)]\n    let _ = 1;…`

(2) A statement-level allow, likewise.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let on_a_module = "#[allow(clippy::disallowed_methods)]\nmod inner { }\n";`

(3) An outer allow on an inner `mod` IS module-level — the rule permits
    module-level attributes, not only file-level ones.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let inner = "//! doc\n#![allow(clippy::disallowed_types)]\nfn go() {}\n";`

(4) An inner attribute in the prologue is module-level.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let late = "fn go() {}\n#![allow(clippy::disallowed_types)]\n";`

(5) An inner attribute after an item is not in the prologue.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let expected = "#![expect(clippy::disallowed_macros)]\n";`

(6) `expect` counts too; the sentence says "allow/expect".

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `assert!(governed_allows("#![allow(clippy::too_many_arguments)]\n").is_empty());`

(7) An ungoverned lint is not reported at all.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let disguised = concat!(`

(8) THE DISGUISES. An attribute inside a comment or a string is not an
    attribute. `PR4-CENSUS-COMMENT-ORACLE` is in the ledger because a
    census counted a doc comment, and this module's own fixtures are
    attributes written inside string literals.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let blanked = blank_comments_and_strings(disguised);`

... and the blanking that makes that true actually ran.

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let mixed = format!("{disguised}#![allow(clippy::disallowed_macros)]\n");`

(9) A real attribute in a file that also carries disguised ones is still
    found — the blanking must not be a blunt "delete everything".

## `fn the_placement_scan_refuses_an_allow_that_is_not_module_l…` › `let mechanisms = 9;`

The hostility is a count over *mechanisms*, not over strings: nine cases,
and the placement answers partition 4 / 3 (module-level / not) with two
that report nothing at all.

## `fn the_three_blunt_governed_lints_are_used_by_nobody() {`

`clippy::style`, `clippy::all` and `warnings` are governed and unused.

Each would suppress far more than an effect denial — `warnings` would
suppress the whole gate. The count is asserted at zero rather than left to
habit, and the scanner is shown to *see* them so the zero is not a blind
spot.

## `fn the_three_blunt_governed_lints_are_used_by_nobody()` › `for probe in [`

The scanner sees them when they are there.

## `fn the_three_blunt_governed_lints_are_used_by_nobody()` › `let list = allowlist();`

And the three that ARE used are exactly the three recorded.

## `fn cargo_toml_declares_no_lint_table_that_could_allow_a_governed_lint() {`

`mechanism` (2) scans `Cargo.toml [lints]` too, so this is that half.

## `fn cargo_toml_declares_no_lint_table_that_could_allow_a_gov…` › `return;` (trailing)

No table at all is the strongest form of the answer.

## `fn the_legacy_section_is_frozen_and_may_only_shrink() {`

---------------------------------------------------------------------------
(2) The frozen legacy section
---------------------------------------------------------------------------

## `fn the_legacy_section_is_frozen_and_may_only_shrink() {`

The legacy section may only shrink, and the refusal is executed.

## `fn the_legacy_section_is_frozen_and_may_only_shrink()` › `let grown: Vec<&str> = current.iter().copied().chain(["src/catalog.rs"]).collect();`

Executed, not inferred: a list that DOES grow is refused, and shrinking is
allowed. Two directions, because a checker that refused everything would
pass the first assertion.

## `fn the_legacy_section_is_frozen_and_may_only_shrink()` › `let frozen: BTreeSet<&str> = FROZEN_LEGACY_ALLOWLIST.iter().copied().collect();`

And the frozen list is the tree's, not a second copy that drifted.

## `fn the_legacy_section_never_contains_a_topology_module() {`

"never contains a topology module (src/topology/**, src/runner/**,
src/workspace_manager.rs, src/engine/topology.rs)".

## `fn the_legacy_section_never_contains_a_topology_module()` › `let probes = [`

Executed: each of the banned shapes is refused on its own, so a check
that only knew about `src/topology/` would fail here.

## `fn the_legacy_section_never_contains_a_topology_module()` › `let sentence_shapes = [`

The gap the fifth shape closes, executed rather than described.

`topology_modules_among` matches with `str::starts_with`, and the packet
sentence names `src/engine/topology.rs` — a file. PR7 makes the schema-4
engine a directory, and `"src/engine/topology/create.rs"` does not start
with `"src/engine/topology.rs"`. Run the check with only the four shapes
the sentence names and it returns nothing for a submodule: the ban would
have stopped covering every file of the module it exists to cover, and
nothing would have said so.

A test that has never been seen red is not coverage. This is the red.

## `fn the_legacy_section_never_contains_a_topology_module()` › `let before_the_split = [`

And the gap the `src/workspace_manager/` shape closes, executed the same
way. The sentence names `src/workspace_manager.rs`, a file, and the
schema-4 workspace funnel grew a directory of production modules in W2 --
so every shape that predates the `m4-workspace` split covers the parent
and none of its children. Written out rather than derived from
`TOPOLOGY_MODULES` minus an element, because the claim is about what the
list looked like while the hole was open, and a derivation would follow
the list wherever it went next.

## `fn the_legacy_section_never_contains_a_topology_module()` › `let funnel: BTreeSet<&str> = list.funnel.iter().map(|e| e.path.as_str()).collect();`

The ban is on the LEGACY section alone: the same sentence puts
`src/runner/{host,invocation}.rs` and `src/workspace_manager.rs` in the
funnel section, and they are there.

## `fn every_allowlist_entry_carries_its_justification_and_names_a_real_file() {`

Every legacy entry carries the justification the packet asks for, and every
funnel entry carries its review clause.

## `fn every_allowlist_entry_carries_its_justification_and_name…` › `assert_eq!(absent, Vec::<&str>::new(), "the absent set moved");`

**Empty since PR6.** It held exactly one entry — `src/runner/container.rs`,
the file `FunnelGroup::Container.module()` names and PR5 did not have —
and PR6 adds that file, so the allowlist now describes the tree it is in
with nothing left over. A new entry appearing here would mean the
allowlist had started describing a tree that does not exist.

## `fn the_denylist_names_every_primitive_the_packet_enumerates() {`

---------------------------------------------------------------------------
(1) The denylist
---------------------------------------------------------------------------

## `fn the_denylist_names_every_primitive_the_packet_enumerates…` › `assert!(!denied.disallowed_methods.is_empty());`

The three lists exist and none is vacuous. An empty `disallowed-macros`
would satisfy "clippy.toml has three lists" and enforce nothing.

## `fn the_denylist_names_every_primitive_the_packet_enumerates…` › `for entry in denied.all() {`

Every entry says why. A denial without a reason is a denial the next
author deletes.

## `fn the_denylist_names_every_primitive_the_packet_enumerates…` › `const NAMES_A_CONTAINER_RUNTIME: &[(&str, &str)] = &[`

"docker invocation helpers". PR6 adds them, so this is no longer an
absence claim: exactly one production file may name a container runtime,
and it is the module `FunnelGroup::Container.module()` names.

**The predecessor of this block could not fail.** It searched
`blank_comments_and_strings(...)` for `"docker` — and that function blanks
string literals *including their quotes*, so the needle it looked for was
one the haystack could never contain. Measured at PR6, when a real
`const DOCKER_PROGRAM: &str = "docker"` landed in production and the
census stayed green. The comparison is against the **unblanked**
production region now, and the control below proves the needle is
findable.

The **set** of files is the claim, in the idiom of
`runner::tests::every_production_process_start_is_classified`: a new file
naming a container runtime is the finding, and every file in the set has
a reason.

## `fn the_denylist_names_every_primitive_the_packet_enumerates…` › `let production = blank_comments(&production_region(&source));`

Comments blanked and **strings kept**: the needle lives inside a
string literal, so the sibling blanker would remove the very bytes
this looks for. Comments are blanked because a doc comment quoting
the packet's "docker ps" is prose, and a census that counted it would
be the fifth `PR4-CENSUS-COMMENT-ORACLE`.

## `fn the_denylist_names_every_primitive_the_packet_enumerates…` › `for helper in [`

And the helpers themselves are denied by name, which is the packet's
actual requirement: the six effectful operations of the two seams the
Container sites are primitives of.

## `fn every_denied_path_this_host_can_resolve_does_resolve() {`

A denied path that does not resolve enforces nothing, and clippy says so with
a bare `warning:` that `-D warnings` does **not** escalate (measured on
clippy 0.1.97). This is the check that would otherwise not exist.

## `fn every_denied_path_this_host_can_resolve_does_resolve()` › `let denied_text = fs::read_to_string(repo_root().join(CLIPPY_TOML)).expect("clippy.toml");`

The repo's own denylist, with every `allow-invalid` stripped, so the
suppression cannot hide a typo from this test the way it hides the
platform-conditional entries from the gate.

## `fn every_denied_path_this_host_can_resolve_does_resolve()` › `let with_typo = format!("{stripped}\n[[extra]]\n",).replace("[[extra]]\n", "");`

The control: a typo IS detected. Without it, a probe that silently linted
nothing would report an empty set and pass.

## `fn unresolved_paths(dir: &Path, tag: &str) -> BTreeSet<String> {`

Run clippy over an empty probe with `dir`'s `clippy.toml` and collect the
paths it reports as unreachable.

## `fn extern_dependencies(deps: &Path) -> Vec<(String, PathBuf)> {`

Every dependency rlib beside the test executable, so the probe links the
crates whose paths the denylist names — `libc` above all, whose entries would
otherwise be silently unchecked.

## `fn every_platform_conditional_denial_names_something_real() {`

The platform-conditional denials name something this tree really calls.

`windows_sys::*` cannot be resolved from a Unix host at all — clippy ignores
a path whose crate is not linked, without even the unreachable-path notice —
so a typo there would be invisible on the only platform where the lint gate
runs. What *is* checkable from here is that every such path's item name
appears in this tree's own Windows source. A misspelling diverges from the
call site and fails.

**The residual, stated:** this proves the name is spelled the way the tree
spells it, not that `windows_sys` exports it at that module path. The
msvc-target clippy run is what proves the second half, and it is a gate
rather than a test.

## `fn every_platform_conditional_denial_names_something_real()` › `const PACKET_ONLY: &[&str] = &[`

`exec*` is the packet's own wildcard: the tree calls none of them
today and the sentence still requires them denied.

## `fn every_platform_conditional_denial_names_something_real()` › `let suppressed: BTreeSet<&str> = denied`

`allow-invalid` suppresses the unreachable-path notice, so it is also the
one way to hide a typo from `every_denied_path_this_host_can_resolve_does_
resolve`. It is therefore spent on exactly the paths that are a real
module on one supported platform and no module on the other, and the set
is written out rather than counted.

## `fn every_platform_conditional_denial_names_something_real()` › `"libc::pipe2",`

Real on Linux, no module on Darwin: `libc` does not define `pipe2`
for macOS. Added after CI's macOS job found it -- this project has
a Windows guest and no macOS host, which is `PR5-MACOS-CLIPPY-NEVER-
RUN`. The suppression is what keeps the `lint (macos)` job green;
`host_conditional_paths` still asserts the path is unresolved there,
because that test strips `allow-invalid` before it probes.

## `fn every_declared_effect_denial_refuses_for_the_reason_it_declares() {`

---------------------------------------------------------------------------
`proof_tests[4]` — the fixtures whose failure reason is pinned
---------------------------------------------------------------------------

## `fn every_declared_effect_denial_refuses_for_the_reason_it_declares() {`

`proof_tests[4]`: "injected renamed-import / re-export / function-value /
legacy-wrapper call fixtures fail the build".

A fixture asserting "this does not build" is green whether it failed for the
intended reason or a typo. Four things are asserted that a bare refusal
cannot give:

* a **positive control** compiles clean first, so a mis-wired `--extern` or a
  missing `clippy.toml` cannot make every fixture "refuse";
* each fixture emits **exactly** its declared lint and no other governed one;
* clippy's message names the **resolved** path — `std::fs::write`, not the
  alias the fixture wrote — which is the whole of `mechanism` (1)'s claim
  that resolution defeats renaming;
* the shapes are counted, so a deleted fixture is loud.

## `fn every_declared_effect_denial_refuses_for_the_reason_it_d…` › `let (ok, diagnostics) = lint_fixture(&scratch, "control", DENIAL_CONTROL);`

The control first. If this does not compile clean, nothing below means
anything -- `PR5-C-DOCTEST-FIXTURES-NEVER-RAN` is the ledger entry for
fixtures that were green having never executed.

## `fn every_declared_effect_denial_refuses_for_the_reason_it_d…` › `assert_eq!(shapes.len(), 7, "{shapes:?}");`

`mechanism` (1) names five resolution shapes -- "aliases, re-exports,
function values, method calls, and macro-expanded code" -- and
`proof_tests[4]` names four fixtures. The grid covers the union plus the
type list, which is seven, and all three lints fire.

## `fn lint_fixture(dir: &Path, tag: &str, body: &str) -> (bool, Vec<(String, String)>) {`

Compile `body` as its own crate under the repo's `clippy.toml`, and return
whether it compiled plus every clippy diagnostic it emitted.

## `fn clippy_driver() -> PathBuf {`

`clippy-driver`, from `PATH` or from the active toolchain's sysroot.

**Not** optional, and not skipped when missing: a build refusal whose only
evidence is a fixture nothing executes is `PR5-C-DOCTEST-FIXTURES-NEVER-RAN`,
and the rule adopted from it is to name the command that runs the fixture and
check that the command is one CI runs. `.github/workflows/ci.yml` installs
the clippy component in both the `test` and the `lint` job, and
[`the_workflow_that_runs_these_tests_installs_the_compiler_they_need`]
asserts it.

## `mod ci_model;`

---------------------------------------------------------------------------
The CI workflow, read as a document rather than as text
---------------------------------------------------------------------------

`BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE` in `reviews/FINDINGS.md` deferred
this section's repair for one reason -- it needs a YAML parser and the crate
had no `[dev-dependencies]` at all. The dependency is added, so the repair is
made: every claim below is an equality over a parsed mapping or an exact
scalar pin, and the two escapes the row enumerates are executed as mutations
that must be refused.

**The ruling this section used to carry is withdrawn.** An earlier round
argued from PR #25 that a checker over this surface cannot converge. PR #25's
retained half kept C1-C4 as equalities and exact pins and it is the withdrawn
half that compared prose across an open document set; the lesson supports a
structural equality here rather than licensing repeated `contains`.

## `mod ci_model;`

The shape itself, and the oracle that reads the document against it, are
implementation and live beside this file rather than in it. `ci_model` is the
single authority for what CI runs and on which runners; `workflow` turns a
parsed document into complaints and carries the mutations that prove each
complaint fires. The cfg census below reads `ci_model` too -- which is why
the constants are a module of their own and not a half of `workflow`.

What stays here is what this section *is*: the five tests, and, further down,
the join where the census and the workflow contract meet.

## `fn the_workflow_parser_rejects_duplicate_keys_and_reads_on_as_a_string() {`

The parser this oracle depends on has the two properties it was chosen for.

Executed rather than believed. A silent change in either -- a dependency
bump, a feature flag -- weakens every equality in this section, so it fails
here first.

## `fn the_workflow_parser_rejects_duplicate_keys_and_reads_on_…` › `let clean = "jobs:\n  lint:\n    runs-on: ubuntu-latest\n";`

The control: the same shape without the duplicate parses, so "refused"
below is not "refuses everything".

## `fn the_workflow_parser_rejects_duplicate_keys_and_reads_on_…` › `let doc = parse_workflow(&ci_workflow_text()).expect(CI_WORKFLOW);`

YAML 1.1 resolves the bare word `on` to the boolean `true`, which would
put the workflow's trigger block under a key no reader looks for. A 1.2
parser reads it as the string it is, and `field_names` renders a non-string
key rather than dropping it, so this would fail loudly either way.

## `fn the_workflow_shape_oracle_refuses_every_escape_the_ledger_names() {`

Every escape the ledger and this section's history name is refused.

The oracle is run against mutated documents because an oracle only ever run
on conforming input is one nobody has seen refuse anything -- the rule this
file states in its own header and the reason `PR5-C-DOCTEST-FIXTURES-NEVER-RAN`
exists.

## `fn the_workflow_shape_oracle_refuses_every_escape_the_ledge…` › `let doc = parse_workflow(&text).expect(CI_WORKFLOW);`

The negative control first: the real document has no complaint, so a
refusal below is the mutation and not a contract that refuses everything.

## `fn the_workflow_that_runs_these_tests_installs_the_compiler_they_need() {`

The command that executes the fixtures is one CI runs, on every platform.

`clippy-driver` is a test dependency of that job and `dtolnay/rust-toolchain`
installs the minimal profile, so the components list is part of the claim.
The predecessor asked whether the word `clippy` appeared on a `components:`
line of the comment-stripped text, and whether the file contained the test
command anywhere; both survive an `echo`, and the strip existed only because
the job's own comment spelled the needle.

## `fn the_self_hosted_windows_leg_runs_these_fixtures_on_the_pinned_labels() {`

The Windows suite's job runs these fixtures on the self-hosted labels, and
on nothing else the contract can read.

The claim the `test` job discharges with an install step -- that
`clippy-driver` is present for the fixtures -- is discharged here by the
golden image the runner boots, which this contract cannot read; the decision
record binds re-curation to it instead. What the contract *can* read is
pinned: the labels exactly, the suite step exactly -- the command and the
count that says it executed, see
[`the_self_hosted_leg_counts_the_tests_it_ran`] -- the platform-default
shell on every `run:` step, and a field set with no `if:` or
`continue-on-error:`. The refusals are executed in [`WORKFLOW_ESCAPES`],
every row named `MUT-TEST-WINDOWS-*` and both `MUT-WINDOWS-WITNESS-*`.

## `fn the_hosted_windows_leg_still_links_every_test_binary() {`

The Windows tree is code-generated and linked on GitHub's current stable,
not only type-checked.

The self-hosted leg executes the suite with the image's toolchain, which
moves only by re-curation; `cargo check` and Clippy stop before codegen. The
witness is a hosted `cargo build --all-targets`, pinned exactly once on
exactly one `windows-latest` job and riding the Windows Clippy gate so that
job's step and checkout pins cover it. It links the library and binaries as
shipped and as test harnesses, so a Windows-only codegen or link failure in
any of them on current stable cannot pass every hosted leg; what it cannot
see is a failure that needs a toolchain newer than current stable, which no
leg has. Its carrier's toolchain input is pinned to `stable` too: the action
is pinned by commit, and the input is what decides which compiler runs. The
refusals are executed in [`WORKFLOW_ESCAPES`], `MUT-WINDOWS-BUILD-WITNESS-*`,
`MUT-WITNESS-CHECKOUT-REF` and `MUT-GATE-TOOLCHAIN-DOWNGRADED`.

## `fn no_repository_file_overrides_what_ci_compiles_or_runs() {`

No file in the repository outranks what `ci.yml` says CI compiles and runs.

Every other assertion in this section reads `ci.yml` and concludes something
about what CI does. Two repository files make that inference false without
touching the workflow at all. A `rust-toolchain.toml` overrides the rustup
default the pinned toolchain action sets, so every bare `cargo` command runs
a compiler the workflow never names -- the current-stable witness included,
and the MSRV floor with it. A `.cargo/config.toml` can bind
`target.<triple>.runner`, which Cargo applies to `cargo test`: every Windows
harness builds and a wrapper reports success without executing one, on the
one platform whose tests no other leg runs.

Neither exists, and this is what keeps it that way. Absence rather than a
parse: adding either is a deliberate act, and the same change must decide
what this contract then reads. `CLAUDE.md` already states the convention for
the toolchain file; this makes it enforceable rather than remembered.

## `fn no_repository_file_overrides_what_ci_compiles_or_runs()` › `let manifest: toml::Value =`

Package selection, for the same reason. `--all-targets` applies to the
packages Cargo selected, and `workspace.default-members` chooses them.
TOML's parser, not a spelling. `[ workspace ]`, `[workspace] # note` and a
root `workspace.default-members = [...]` are one table to Cargo and three
different strings to a line scan, which is how the first two versions of
this check read and how each was shown a spelling it missed.

## `fn the_self_hosted_leg_counts_the_tests_it_ran() {`

The leg whose tests left GitHub's runners reports that they ran.

Every other assertion here reads `ci.yml` and concludes what CI was *asked*
to do. Cargo can be asked for this suite and execute none of it: a
`target.<triple>.runner` in a repository `.cargo/config.toml`, in
`$CARGO_HOME`, in a directory above the checkout or in the process
environment hands each compiled harness to a wrapper that exits zero, and a
root `[workspace]` whose `default-members` name another crate builds no
harness of this one. Three of those are written where nothing reading this
repository can see them, and Cargo is free to add a fourth route.

So this leg counts instead of enumerating: a suite that did not execute
reports no `test result: ok.` line, and a job that cannot reach the floor
fails. It is pinned like every other script, and the pin is what stops the
count being deleted or its floor lowered to a number nothing has to clear.

It is not a defence against a pull request, and nothing in this file is: an
edit to `ci.yml` deletes this step as easily as any other, and the decision
record says where the boundary actually is. It is a defence against the
machine, which is the input this change added. The guest is provisioned
outside the repository, so its Cargo home and its environment are not in any
diff, and this is the leg saying it ran what it says it ran.

## `fn the_msrv_leg_checks_the_floor_the_manifest_publishes_on_every_platform() {`

The MSRV leg checks the floor the manifest publishes, on every platform.

Four claims. Three were held by nothing at all before this test: that the leg
is enabled and unabsolved, that its command is the documented one *including*
`--locked`, and that its matrix is every supported runner. The fourth, the
toolchain, was held loosely -- `.github/scripts/test-docs-consistency.sh`'s C2
accepts `rust-version` "or a patch release of it" -- and is held exactly here.
It is derived from the manifest and quoted from it on failure, because a
literal `1.85.0` would make this its own oracle for the fact it exists to
hold.

The refusals are executed in [`WORKFLOW_ESCAPES`] -- every row named
`MUT-MSRV-*` -- so this test passing is not the claim that the contract
refuses nothing.

## `fn the_msrv_leg_checks_the_floor_the_manifest_publishes_on_…` › `assert_eq!(three_component("1.85"), "1.85.0");`

The derivation, with its controls, before anything is asserted with it.

## `fn the_msrv_leg_checks_the_floor_the_manifest_publishes_on_…` › `let installed: Vec<&str> = field(&doc, "jobs")`

The toolchain claim once more as a bare equality, so its failure names the
manifest and the workflow rather than only the complaint between them.

## `fn the_msrv_leg_checks_the_floor_the_manifest_publishes_on_…` › `let steps = field(&doc, "jobs")`

The order, as the indices themselves. `MUT-MSRV-CHECK-BEFORE-TOOLCHAIN`
executes the refusal; this is the positive control beside it, and it fails
with both positions named rather than with a complaint about them.

## `fn the_workflow_scope_rustflags_pin_refuses_weakening_and_every_override() {`

The workflow-scope `-D warnings` is pinned, and nothing narrows it.

The refusals are driven on synthetic documents as well as on mutations of the
real one, because on the real one this scan cannot be seen working *alone*:
every job and step of the live workflow that could carry an `env:` is already
covered by a field set, so `MUT-RUSTFLAGS-JOB-OVERRIDE` is refused twice
over. Those rows still bind to the code this scan emits and nothing else
emits, so they measure it; what they cannot show is it holding somewhere no
field set does. Each document below carries one job that no other check in
this section reaches, which is where that is shown.

The positive controls come first, in both halves: the real workflow satisfies
the contract, and so does the minimal conforming probe. Without them a
refusal below would be evidence of nothing.

## `fn the_workflow_scope_rustflags_pin_refuses_weakening_and_e…` › `fn probe(header: &str, job_body: &str) -> String {`

A workflow carrying one job the rest of this section does not model.

## `fn the_workflow_scope_rustflags_pin_refuses_weakening_and_e…` › `const PLAIN: &str = "    runs-on: ubuntu-latest\n    steps:\n      - run: cargo check\n";`

A job that binds nothing of its own.

## `fn the_workflow_scope_rustflags_pin_refuses_weakening_and_e…` › `const PINNED: &str = "env:\n  RUSTFLAGS: -D warnings\n";`

The pinned workflow-scope binding, written as the real document writes it.

## `fn the_workflow_scope_rustflags_pin_refuses_weakening_and_e…` › `for (shape, document) in [`

The other half of a scan that matches whole names case-insensitively: it
must not fire on names that merely resemble the guarded ones. Each of these
is a real thing a workflow could carry, and none of them is the warning
policy. Without this block the case-insensitive widening above could be
satisfied by a scan that refuses everything containing `rustflags`.

## `pub(crate) mod cfg;`

---------------------------------------------------------------------------
The cfg census: effective predicates, decided against real valuations
---------------------------------------------------------------------------

The predecessor collected `target_os = "..."` names wherever they appeared at
a code position and treated each name as a platform demanding its own Clippy
runner. `BRIDGE-CI-SHAPE-TEST-IS-A-SUBSTRING-ORACLE` records three ways that
misreads the tree, and all three are what a name-collector cannot see:

  * `not(any(target_os = "linux", target_os = "macos", target_os = "windows"))`
    reported all three platforms covered while **no** runner compiles the body;
  * `not(target_os = "freebsd")` would demand a FreeBSD runner for a body every
    runner compiles;
  * `let target_os = "android";` is code, passes a position gate, and was
    reported as a platform.

Three further corrections come from the review of that repair, and each is a
claim the first structural version got wrong rather than a refinement:

  * **Coverage is decided, never assumed.** The first version evaluated under
    three-valued logic and counted `Unknown` as coverage, so a predicate whose
    truth it could not decide was reported as compiled. Every valuation below
    is COMPLETE for the names this census models, an unmodelled name is a hard
    failure rather than an optimistic guess, and `test` is enumerated per
    invocation because `--all-targets` compiles the library twice.
  * **A `#[cfg]` is not the only cfg, and not every cfg gates.** `cfg!(P)` is a
    boolean expression: the code around it is compiled everywhere. `#[cfg_attr(
    P, attr)]` applies an attribute conditionally; the item is compiled
    everywhere. Counting either as a gated region invents platform demands the
    tree does not make.
  * **An item's predicate is not the attribute written on it.** Stacked
    `#[cfg]`s conjoin, and so does every enclosing guard -- the module block it
    sits in, and, for a whole-file module, the `mod name;` declaration that
    names the file -- whether the guard is written on that declaration or on
    an inline module enclosing it. The files `cfg::WHOLE_FILE_TEST_MODULES`
    lists are reached only that way.

## `pub(crate) mod cfg;`

The census is `cfg`, beside this file; the two tests below are what it answers
to. It decides predicates against `ci_model`'s targets -- the same table the
workflow contract above is checked against -- so "no runner compiles this
body" and "no job lints that platform" cannot drift apart.

`pub(crate)` for one item and one reader: `cfg::WHOLE_FILE_TEST_MODULES` is
the crate's only statement of the whole-file test-module population, and
`engine::topology::recover::tests` floors its skip count at that list's
length. Nothing else here is reachable from outside this directory.

## `fn the_cfg_census_evaluates_effective_predicates_against_the_valuations_ci_sets() {`

The cfg census reads effective predicates, decides them, and knows which
forms gate.

## `fn the_cfg_census_evaluates_effective_predicates_against_th…` › `let unmodelled = parse_cfg("feature = \"unshipped\"", false).expect("a parseable predicat…`

An unmodelled name is a hard failure, not an optimistic guess. This is the
review's fourth finding as a control: the version this replaces answered
`Unknown` here and the caller read `Unknown` as "every runner compiles it".

## `fn the_cfg_census_evaluates_effective_predicates_against_th…` › `let mut domain = scanned_sources();`

The control rides along with the whole domain, so finding it proves the
scan reaches injected content in the presence of every real file rather
than in a fixture read on its own.

## `fn the_cfg_census_evaluates_effective_predicates_against_th…` › `let by_form: BTreeMap<CfgForm, Vec<&str>> =`

The two non-gating forms are RECORDED and not counted. Recording them is
what makes their exclusion measurable: `target_os = "plan9"` and
`target_os = "haiku"` are compiled by no runner, so if either were read as
a gate the census below would report an uncovered predicate.

## `fn the_cfg_census_evaluates_effective_predicates_against_th…` › `let stacked = injected`

Two of the gates exist only because a guard was conjoined from somewhere
other than the attribute itself.

## `fn every_platform_this_crate_configures_for_has_a_clippy_gate_the_aggregate_requires() {`

Every platform this crate configures code for has a Clippy gate, and the
aggregate makes that gate required.

The domain is derived from the tree rather than listed here: a written-down
platform list is one nothing forces an author to extend, which is what the
previous repair of this test shipped. The two halves join at the target
tuple -- [`cfg_regions`] decides which runners compile each body, and the
workflow contract requires a gate job whose `runs-on:` is that runner.

**Why this is one test and not three.** `PR5D-MSVC-CLIPPY-NEVER-RUN` and
`PR5-MACOS-CLIPPY-NEVER-RUN` are the same defect on two platforms, found
apart, because the Windows repair was written as an instance rather than a
class. A derived domain makes the next platform's omission a failure here
rather than a third finding.

## `fn every_platform_this_crate_configures_for_has_a_clippy_ga…` › `assert!(`

A boundary, not a count: the tree carries nested, negated predicates and a
census that only reads flat ones would pass every other assertion here.

## `fn every_platform_this_crate_configures_for_has_a_clippy_ga…` › `let under_a_file_guard: BTreeSet<&str> = gates`

The whole-file guards are the other boundary. Every predicate in those
files is `all(test, …)`, and a census that resolved none of them would
read them all as unconditional.

## `fn every_platform_this_crate_configures_for_has_a_clippy_ga…` › `for target in &CI_TARGETS {`

Each leg is load-bearing, with a witness. A runner no body needs is a job
this contract would keep demanding for no reason; a body only one runner
compiles is why that runner's leg cannot be dropped.

## `fn crate_under_test() -> (PathBuf, PathBuf) {`

The crate's own rlib and the directory its dependencies are in.

The test binary lives beside them, so both are found from `current_exe`
rather than from a guessed target directory — `CARGO_TARGET_DIR` here is the
build wrapper's slot, not `target/`. The idiom is lane C's, from
`src/events/log/tests.rs`.

## `fn every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified() {`

---------------------------------------------------------------------------
(3) Wrapper classification
---------------------------------------------------------------------------

The four bodies are in `classification::checks`, beside this file. The names
here are the harness -- they are what the contract, CI and `--list` know --
and each one delegates and does nothing else. Every check reads
`effects/wrappers.toml` and `clippy.toml` against the tree they classify, so
the child is read-only and can be, and is, cut out as test logic by both
source cutters without joining the whole-file module census.

## `mod artifacts;`

---------------------------------------------------------------------------
`outputs` — the generated inventories
---------------------------------------------------------------------------

## `mod artifacts;`

What the artifacts *contain* and what the inventory *declares* are
definitions, and they live beside this file rather than in it: the CRLF
discipline every comparison is made under, the module a group's funnel bodies
are actually in, the sites no funnel names, the frozen sampling N, and the
two record generators. `artifacts` is the single authority for all six, and
the three Answer disagreements are its answer rather than this file's.

What stays here is what this section *is*: the six tests -- and the reason
the boundary is drawn exactly there is that three of them **regenerate**.
`fs::write` is a denied primitive, `artifacts` restores that denial, and an
allowance may live only in a file `effects/allowlist.toml` lists; a child
that regenerated an artifact would need an entry in it. That is a governance
claim about where an effect may live, not a mechanical consequence of moving
a declaration, so the writes stay with the harness -- the same cut, for the
same reason, that left the effectful build helpers out of `policy.rs`.

## `fn the_checked_in_effect_sites_json_is_what_the_enums_generate() {`

`outputs`: "effect_sites.json (from the enums) … generated from the enums by
a test and attached to gate reports".

## `fn the_checked_in_effect_sites_json_is_what_the_enums_gener…` › `assert_eq!(effect_sites().len(), EffectSiteId::all().len());`

It really is the whole inventory, not a corner of it.

## `fn the_checked_in_funnel_module_record_states_where_the_bodies_are() {`

The companion artifact states where the funnel bodies actually are
(`PR5-CONF-018`).

`effect_sites.json` ships `"module": "src/interaction.rs"` for
`Answer.Ingest`, `Answer.PublishRename` and `Answer.StageWrite`, and the
`AnswerSite::` literals are at `src/rundir.rs:899`, `:912` and `:934` and
nowhere else. Until this round the only thing reconciling the artifact with
the tree was a **test-side override** — [`funnel_module`] — so the artifact a
gate report carries said something false about this tree and nothing checked
in said otherwise. Measured: deleting that override makes the three Answer
sites join the "no funnel names them" set, which is the finding.

The two axes are the *inventory's claim* and the *tree's answer*. Every
existing test holds one constant and reads the other — the census searches
the file the override names, the artifact test compares the file the enums
name — so the pair was never written down together. Here they are written
down together, for every site rather than for the three that disagree, so a
fourth disagreement appearing later is a change to this file rather than a
silence.

## `fn the_checked_in_funnel_module_record_states_where_the_bod…` › `["Answer.StageWrite", "Answer.PublishRename", "Answer.Ingest"],`

In `EffectSiteId::all()` order, which is the frozen enum's, so a site
moving within the inventory is a change here too.

## `fn every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent() {`

Every module the inventory names is in the funnel section, and every site has
a funnel that names it — or is recorded absent with the reason.

This is where an omission would live. `effect_sites.json` is generated from
the enums so it cannot omit a *site*; what it can do is name a module that
implements none of them, which reads identically to a module that implements
all of them.

## `fn every_site_the_inventory_declares_has_a_funnel_that_name…` › `let mut sources: BTreeMap<String, String> = BTreeMap::new();`

Per site: does a funnel name it?

Two mechanisms, because the three lanes built two and a grid that knew
one would report the other's whole group as unimplemented:

  * the variant literal — `RunDirSite::PublishMarker` inside the funnel
    body, which is lane B's shape (one `pub fn` per site, site fixed);
  * the site as a **parameter** — `fn create_ref_zero_old(site: RefSite,
    …)`, which is lane A's and lane C's, and is the shape `identity`
    literally describes ("every effectful funnel API takes its group's
    site by value").

Recorded per group by `funnel_mechanism` so a group that stopped doing
either is loud rather than silently "still covered by the other".

## `fn every_site_the_inventory_declares_has_a_funnel_that_name…` › `let distinct: BTreeSet<&str> = mechanisms.values().copied().collect();`

Both mechanisms are in use. If one disappeared, every group would have to
be re-measured against the other rather than inheriting a pass.

## `fn every_site_the_inventory_declares_has_a_funnel_that_name…` › `let expected: BTreeSet<String> = SITES_WITHOUT_A_FUNNEL`

The expected set, written out rather than counted, because *which* sites
have no funnel is the finding and a count would hide a swap.

## `fn no_production_api_exports_a_writable_process_command() {`

No production module may return a writable process handle through public or
crate-visible API, directly or behind a function pointer.

This is structural over signatures, not a builder-name denylist. Renaming
`build_command`, adding a second builder, or returning `fn() -> Command`
therefore cannot make the finding disappear. Private construction inside a
funnel and APIs that *consume* a Command remain permitted.

## `fn the_checked_in_residue_class_record_is_what_the_enums_generate() {`

`outputs`: "the residue-class evidence record (per element: constructed,
classified, recovered; per site: sampling N and observed-class histogram)".

## `fn the_checked_in_residue_class_record_is_what_the_enums_ge…` › `let harness = fs::read_to_string(repo_root().join("src/workspace_manager/tests.rs"))`

The sampling N the record freezes is the N the harness runs.
`command_internal_sub_effects` says "N frozen per site in the registry";
`src/topology/registry.rs` is PR3's and frozen, and carries no N, so the
record carries it and this is the cross-check that keeps the two equal.

## `fn every_file_durability_barrier_in_a_funnel_module_goes_through_one_call() {`

The durability barrier is reached through **one** call each, and the syscall
is inside it (`PR5-CONF-012`).

`proof_tests[9]` makes the durability ledger a *named proof*: "the sync
ledger shows the synced length equal to the file length after open". The
ledger entry is written beside the syscall by the same function, so it
certifies itself: `let outcome = file.sync_all();` → `let outcome:
io::Result<()> = Ok(());` survived the whole suite, with the fsync gone and
every trace assertion still green. `sync_file_recorded`'s own doc conceded
the residual in as many words, and the same shape held in
`src/workspace_manager.rs` and for the Event sync records.

Nothing on a machine that does not lose power can see *inside* `fsync`. What
can be seen is two things either side of it, and the repair is to make both
checkable rather than one:

* **the syscall is there** — this census, which reads the source and fails if
  the call leaves the one function that is allowed to make it;
* **the seam was reached as often as the ledger claims** —
  `rundir::tests::the_durability_ledger_counts_barriers_that_were_actually_
  performed`, which crosses the ledger's entries against
  `util::barriers_performed()`.

Neither alone is enough, and that is the point: a census cannot tell whether
the line ran, and a counter cannot tell whether the line still contains the
syscall.

`src/events/log/premove.rs` is excluded by name. It is `git show
ff0490a:src/events.rs` kept verbatim as the independent oracle for
byte-identical legacy behaviour, and its whole value is that it is unchanged.

## `fn every_file_durability_barrier_in_a_funnel_module_goes_th…` › `const BARRIERS: &[(&str, &str, usize)] = &[`

The two functions that may name the primitive, and how many times each.

## `fn every_file_durability_barrier_in_a_funnel_module_goes_th…` › `let util = artifact_content(`

Line endings normalized before any structural search: the guest checks this
tree out with CRLF, and `find("\n}\n")` does not match `\r\n}\r\n`. Measured —
this census passed on Linux and panicked "the function ends" on Windows
Server 2025, which is the platform half of the same lesson the rest of this
round is about. `artifact_content` exists for exactly this reason.

## `fn every_file_durability_barrier_in_a_funnel_module_goes_th…` › `const FUNNELS: &[&str] = &[`

And nowhere else in the funnel modules, so a caller cannot quietly grow a
second barrier the counter and this census both miss.

## `fn every_file_durability_barrier_in_a_funnel_module_goes_th…` › `"src/runner/container.rs",`

PR6's Container funnel writes the intent record durably and reaches
the barrier through `util::fsync_file`/`util::fsync_dir` like every
other funnel, so it belongs in the "and nowhere else" half.

## `fn every_file_durability_barrier_in_a_funnel_module_goes_th…` › `let log = fs::read_to_string(repo_root().join("src/events/log.rs")).expect("src/events/lo…`

The Event funnel's own primitive is `sync_data`, a different call with its
own census next door, and it is named here so this test's silence about it
is a decision rather than an oversight.

## `mod source_oracles;`

---------------------------------------------------------------------------
"no topology production callers", and the source oracles under it
---------------------------------------------------------------------------

## `mod source_oracles;`

The twelve bodies are in `source_oracles::oracles`, beside this file: the two
site censuses here and the witness for the first one's domain, and, in the
T-CONTAINER section further down, the five
that hold the two production regions and the whole-file module derivation.
The names in this file are the harness -- they are what the contract, CI,
`effects/wrappers.toml`, `reviews/FINDINGS.md` and `--list` know -- and each
one delegates and does nothing else.

The boundary is drawn at "reads the tree, writes nothing". All twelve do
exactly that, so the child restores the three effect denials `super` allows
and takes no allowlist entry. The needles they carry -- a funnel table, a
`RunnerRequest {` in prose, the container-runtime literal -- are the reason
the bodies sit inside a `cfg(test)` module there rather than at file level:
both source cutters then read them as test logic, and the census that counts
files naming a container runtime keeps the set it has.

## `mod contract_mappings;`

---------------------------------------------------------------------------
The T-CONTAINER mechanical checklist
---------------------------------------------------------------------------

## `mod contract_mappings;`

The nineteen-name transcription, the presence predicate they share and both
bodies are in `contract_mappings::mappings`, beside this file, with the three
R3b enumerations below. The names here are the harness -- they are what the
contract, CI and `--list` know -- and each one delegates and does nothing
else.

The boundary is drawn at "resolves a transcribed enumeration against the
tree, and writes nothing". Both do exactly that, so the child restores the
three effect denials `super` allows and takes no allowlist entry.

`the_view_directory_has_one_definition_in_the_tree` below is a mapping test
by shape and deliberately did NOT follow them. It constructs a
`ContainerName` to drive the mount side against the census side, and that is
one of the five needles `runner::container::resolve::tests::
no_module_outside_the_container_runner_writes_a_container_intent` counts over
the WHOLE file -- not over a production region, so an inline `cfg(test)`
module does not close it. That census excludes this file by exact path and
its exclusion names this very test as the reason; a child holding it would
need a second exclusion there, which is a change to another slice's census
rather than a consequence of moving a declaration. The same cut, for the same
reason, that left the effectful build helpers out of `policy.rs`.

## `fn the_view_directory_has_one_definition_in_the_tree() {`

The R19 view directory has **one** definition in this tree.

`PR6E-005`. `src/runner/container/exec.rs` mounts the disposable Git view and
`src/runner/container/census.rs` finds it again after a coordinator death.
They were written in different lanes and each had its own definition of
`<R>/views/<container-name>` — lane A's `join("views")` literal and lane C's
`VIEWS_DIR` const — with nothing crossing them. Measured on the merged tree:
`VIEWS_DIR = "views-mutated"` passed **all 1324 tests**, because lane C's
fixtures plant orphan views through `view_path` itself and lane A's assert
its own literal. A divergence leaves every orphan view unreclaimed after a
crash, against `resource_accounting` R19's `NoRunFinished` ("pruned at the
next write-command start after the owning container is observed terminated")
and ST-16's closing clause "ledgers R19/R26 balance".

`exec::view_dir` now delegates to `census::view_path`, so the two cannot
disagree. This is the guard against a **third** definition: the segment is
declared once, by one const, and a second production site that joins a
`"views"` literal fails here by name.

The class is `PR5D-VISIBILITY-CHECK-DUPLICATED` — a hand-maintained value
kept in two places, where breaking one copy left the suite green because the
other still answered.

## `fn the_view_directory_has_one_definition_in_the_tree()` › `let container: Vec<(String, String)> = scanned_sources()`

The domain is the container substrate's PRODUCTION modules. Test modules
are excluded by name rather than by `production_region`, deliberately:
`src/runner/container/tests.rs` is a whole-file `#[cfg(test)] mod tests;`
with no inline marker, so `production_region` returns all 3 000 lines of
it as production and a fixture asserting the path it expects would read as
a second declaration. That inconsistency is `PR6E-006` and is a finding of
its own; this test does not depend on it being repaired.

## `fn the_view_directory_has_one_definition_in_the_tree()` › `assert_eq!(`

CONTROL, and it is the one that stops this going vacuous: name the modules
the scan must be looking at. A filter that matched nothing, or a rename
that moved a half of the seam out of the scanned set, fails here rather
than reporting one clean site — `PR5-DOCKER-CENSUS-CANNOT-FAIL`.

## `fn the_view_directory_has_one_definition_in_the_tree()` › `sites.push(path.clone());`

The property is "one site, and it is the census's". The LINE is
incidental: pinning it made this test fail when repair C1's merge
shifted census.rs by four lines, which is a true statement about
line numbers and says nothing about the seam. Assert the path;
carry the line into the message, where a human wants it.

## `fn the_view_directory_has_one_definition_in_the_tree()` › `let (_, census) = container`

And the scan can see a declaration at all: a blanker that erased the code
would report zero sites, which reads as "one definition" only because the
expected list happens to be short.

## `fn the_view_directory_has_one_definition_in_the_tree()` › `let root = Path::new("/private/root");`

And the two halves really do answer the same thing, driven rather than
read: the mount side and the census side, same inputs, same path.

## `fn the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names() {`

The five source-oracle bodies that close this section are in
`source_oracles::oracles` with the other six. They belong to that file and
stand here because this is where the harness names are: the whole-file module
derivation the four censuses skip by, and the two production regions every
prohibition census counts over.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_than_text_after_an_attribute() {`

---------------------------------------------------------------------------
The module resolver reads structure, and refuses what it cannot read
---------------------------------------------------------------------------

## `fn the_module_scan_reads_ancestry_and_visibility_rather_than_text_after_an_attribute() {`

**These two bodies are here rather than beside the other instrument controls
in `source_oracles.rs`, and the reason is that file's own rule.** It is
reached by a plain `mod` declaration, so it sits inside every whole-tree
census's domain, and it therefore refuses to spell out a terminated
`#[cfg(test)] mod name;` even inside a string literal -- one written in a
comment is the exact shape that once derived a phantom skip and removed a
real production file from every census below it. A scanner whose whole
subject is that form cannot be driven under that rule. This file is itself a
whole-file test module -- `effects.rs` declares it `#[cfg(test)] mod tests;`
-- so no census reads it and the fixtures below cost nothing.

Every positive case carries the mutation that makes it negative, in the same
assertion pair: the guard deleted, the ancestry flattened, the qualifier
removed. A scan that answered "test-only" unconditionally passes the
positives and fails on every one of the negatives beside them.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_than_text_after_an_attribute() {`

The scan reads a file's **module structure**: inline ancestry, visibility
qualifiers, and the predicates that compose down the tree.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let plain = only("#[cfg(test)]\nmod tests;\n");`

(1) The plain form the text rule found, still found.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for written in [`

(2) **Visibility qualifiers.** The text rule read `mod ` immediately
after the attribute, so every one of these was invisible to it and the
file it named stayed inside every census's domain. Four spellings,
because `pub(in path)` carries a `::` and `pub(crate)` carries a paren,
and a scan that stepped over one shape and not the others would pass on
whichever the tree happens to use today.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `assert!(!only("pub(crate) mod helpers;\n").test_only);`

And the qualifier is not what makes it test-only: removed guard, same
qualifier, decided the other way.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let inherited =`

(3) **Inline ancestry**, which is the shape `agent/proc.rs` uses and the
one no text rule reaches at all: the declaration carries no attribute.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let ungated = only("pub(crate) mod test_support {\n    pub(crate) mod readiness;\n}\n");`

The mutation: the same file with the ancestor's guard deleted. The
declaration is byte-identical and the answer flips, which is what says
the ancestry is being read rather than the name.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let deep =`

(4) **Nested inline modules**, with the guard on the middle one — so
neither "the outermost" nor "the declaration's own" is the rule.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let both = scan("#[cfg(test)]\nmod inner {\n    mod under;\n}\nmod beside;\n");`

(5) **The scope closes.** A declaration written after the guarded block
ends does not inherit it, which is the whole of what brace depth is for.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let after_a_function = scan("#[cfg(test)]\nfn helper() {}\nmod plain;\n");`

(6) **An attribute belongs to the item it precedes.** A `#[cfg(test)]`
on a function does not carry to the next `mod`, and a brace-bodied module
is not a declaration of a file at all.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for (written, expected) in [`

(7) **The predicate is decided, never assumed.** `any` is the case that
matters: a Unix build with `test` off compiles the file, so the census
must keep it. Deciding it "test-only" would remove a production file from
every census below, silently, which is the failure direction this whole
derivation is shaped against.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for written in ["test", "all(test, unix)", "not(any(not(test), unix))"] {`

(8) The entailment itself, driven on predicates rather than on sources.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for prose in [`

(9) **Comments and string literals are not code.** `PR4-CENSUS-COMMENT-
ORACLE` is the standing entry, and this derivation is the one it was
filed against: a `//` line carrying a declaration once derived a skip for
a real production module and removed it from every census below.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let after_a_brace_char = only("const C: char = '{';\n#[cfg(test)]\nmod real;\n");`

A char literal holding a brace must not move the depth the ancestry is
measured in — `PR7-R2C-CHAR-LITERAL-DESYNC`'s class, one instrument over.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `assert!(scan("fn models() {}\nstruct modest;\n").is_empty());`

(10) **The word, not a prefix of one.** `models` is not `mod els`.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let past_a_macro = only("thread_local! {\n    static X: u8 = 0;\n}\n#[cfg(test)]\nmod rea…`

(11) **A macro body is discarded, not walked.** Its tokens are only
*shaped* like items, so anything read out of one is invented. The
discard has to be verified from both sides: nothing inside is derived,
and everything outside still is.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let after_attributed_macro = only("#[cfg(test)]\nlazy! [ a, b ]\nmod plain;\n");`

Delimiters inside a macro body do not move the depth the ancestry is
measured in, and an attribute above a macro belongs to the macro.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let past_a_negation = only("fn f() { let _ = a != b; }\n#[cfg(test)]\nmod real;\n");`

`a != b` is not a macro: the token after `!` opens nothing.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for tokens in [`

**The discard is load-bearing, not decoration.** `mod` is an ordinary
token inside a macro, and a matcher may capture it — but a scanner
reading the body as *items* sees a `mod` with no name after it and
refuses the whole file. So these are legal Rust that a body-walking scan
cannot read, and the discard is what makes them silent.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let beside_a_macro = only(`

And a file holding both still derives exactly the real one.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for spaced in [`

(12) **A spaced or commented `!` is still a macro**, and the discard has
to survive the widening: these bodies hold nothing module-shaped, so they
are dropped in silence and the declaration after them is unaffected.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `"macro_rules ! m {\n    (mod $n:ident) => {\n        ()\n    };\n}\n#[cfg(test)]\nmod rea…`

Discriminating: a matcher capturing the `mod` keyword. Recognised as
a macro it is discarded; missed because the `!` is not the very next
byte, its body is walked and the bare `mod` refuses the whole file.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let inside_a_negated_block = only(`

**`if !condition { … }` is not an invocation of `if`.** Allowing a gap
before the `!` is what makes that shape reachable -- identifier, `!`,
identifier, delimiter -- and reading it as a macro would skip the whole
block. Only `macro_rules` carries a name between its `!` and its body, so
the block below is walked as a block: the declaration inside it is still
derived, with the ancestry it actually has.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let inside_a_negated_block = only(`

And the block a negated condition guards is **walked**, not skipped. An
empty block cannot tell the two apart — skipping a balanced group and
walking it leave the same depth — so the discriminating shape is a
declaration inside it. Read as a macro body this is module-shaped and the
whole file is refused; read as a block it is the declaration it is.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for negated_group in [`

(13) **A negated grouped expression is not a macro body.** `if !(…)` is
identifier, `!`, delimited group — the same three tokens as `foo!(…)` —
and a block expression inside the group may legally declare a module.
Read as a macro the group is module-shaped and the whole file is refused;
read as the negation it is, the module is the module it is. A keyword
cannot be a macro's path segment, which is what separates them.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let through_a_negated_group = only(`

And the same shape with a real out-of-line declaration inside the group,
so the walk is shown to reach it rather than merely not to refuse.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for (written, expected) in [`

(14) **Raw identifiers are one token, and their name may be a keyword.**
`mod r#type;` declares a module called `type` and resolves to `type.rs`;
a reader that stopped at the `#` saw `mod r` with no terminator after it
and refused the file.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `assert!(scan("struct r#mod;\nfn f() { let raw = 1; }\n").is_empty());`

A raw `r#mod` is an identifier named `mod`, not the keyword, so it opens
nothing; and `raw` is an ordinary identifier that merely starts with `r`.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let raw_binding = "fn f() { let r#mod = 1; }\n#[cfg(test)]\nmod tests;\n";`

**The token the fallback steps over is the whole token.**
`PR72-RESOLVER-003`. Everything above reads `word`s; the "anything else"
arm advanced by identifier *bytes*, and `r#mod` is not a run of them. It
consumed the `r`, met the `#`, stepped over it as a byte that opens no
attribute, and then read `mod …` — the **inside of a token** — as though
it stood at item position.

Measured, both shapes refuse: valid Rust, and the scan will not answer
for the file. `let r#mod = 1;` becomes a `mod` item whose name is `=`,
and `use std::r#mod as tests;` becomes `mod as` with no terminator after
it. A refusal here is not a small failure — `declared_whole_file_test_
modules` panics on it, so every census that skips test modules stops on a
tree that compiles. Whether a given rescan refuses or instead *invents* a
declaration is decided by the byte after the embedded name, and neither
outcome is one this scan may have; the repair is that the inside of a
token is never read as one.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let raw_in_a_use = "#[cfg(test)]\nmod harness {\n    use std::r#mod as tests;\n}\n";`

The second shape, inside a `#[cfg(test)]` module so that anything the
rescan derived would be test-only — a skip, for a file the crate never
declared. It declares no module at all.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for source in [`

And the token boundary itself, in both spellings and both directions: a
raw identifier is one token, an ordinary identifier that merely begins
with `r` is another, and a bare `r` is a third.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `let past_a_raw_macro = only("r#if! { let _ = 1; }\n#[cfg(test)]\nmod real;\n");`

(15) **A raw macro name is still a macro.** `r#if!(…)` is a macro called
`if`, so the keyword rule above must read the raw spelling as an
identifier rather than as the keyword it names.

## `fn the_module_scan_reads_ancestry_and_visibility_rather_tha…` › `for fixture in [`

(16) **CRLF.** The guest checks this tree out with CRLF, and every
structural answer above has to be the same there. Driven by converting
each fixture rather than by trusting that nothing here reads a line.

## `fn is_the_literal_mod_tests_form(name: &str, inline_path: &[String], guard: &str) -> bool…`

Whether a declaration is the literal `#[cfg(test)] mod tests;` form: that
name, at its parent's own top level, under **that** guard rather than one
that merely implies it.

Read by `the_whole_file_modules_are_read_from_the_declarations`, which
compares the files these resolve to against the `tests.rs` half of
`cfg::WHOLE_FILE_TEST_MODULES` — the half a `file_stem == "tests"` census
finds — and driven over synthetic input by
`a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal_mod_tests_form`.

**The guard has to *be* `test`, and that is the repair.** Membership used to
be an empty `inline_path` and `name == "tests"`, which never looked at the
guard at all — so `#[cfg(all(test, unix))] mod tests;` counted as the plain
form: same name, same file stem, still test-only, every comparison green,
while rustc compiles no such module on Windows and a census skipping by file
name goes on skipping a file that is not there. A repository whose
first-class target is Windows would have lost a whole test module on it with
the Linux suite green — the exact failure this census family exists to
catch. PR #101's reviewer found it and supplied the reproduction.

**The equality is predicate identity, not a text approximation.** `guard` is
`Predicate::render`'s output, and `Predicate::Test` is the only predicate
that renders as the bare `test`: `Other` is constructed for an atom whose
name is not `test`, or for a `name = "value"` form, and every combinator
renders with its own parentheses.

A guard written equivalently but not identically — `all(test)` — is refused
here too, and that direction is deliberate. It fails loudly, naming the
file, where admitting it means deciding equivalence for a rule whose whole
job is to say which files a *file-name* census may skip; a loud failure
costs a sentence in the slice that writes one, and the other direction costs
a platform.

A narrowed declaration is still test-only and still belongs in the domain
list. `cfg::WHOLE_FILE_TEST_MODULES`' doc comment says what happens then and
why the resulting disagreement is the signal.

Takes the three fields rather than a declaration, so the scan's own
`ScannedDeclaration` and the resolved `TestModuleDeclaration` are decided by
this one rule instead of by two copies of it
(`PR5D-VISIBILITY-CHECK-DUPLICATED`).

## `fn a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal_mod_tests_form() {`

A narrowed guard is still a whole-file test module and is **not** the
literal `#[cfg(test)] mod tests;` form.

The reproduction PR #101's reviewer supplied, driven over synthetic input
rather than by a real narrowed declaration under `src/`: writing one there
would make the tree the fixture and would cost that module its Windows
compilation for as long as it stood.

The mutation is one field wide. Every input the membership rule used to read
is identical between the positive and the negative below — the name, the
empty inline path, the resolved file stem, and `test_only` — so a rule that
does not read the guard cannot tell them apart, and nothing else in this
crate would have said so.

## `fn a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal…` › `let plain = only("#[cfg(test)]\nmod tests;\n");`

The positive: the form the `tests.rs` half of the census domain is a list
of, and the one a `file_stem == "tests"` census may skip.

## `fn a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal…` › `for narrowed in [`

The negatives, written both ways a narrowing reaches the declaration: one
attribute carrying a conjunction, and two attributes conjoined.

## `fn a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal…` › `let inherited = only("#[cfg(test)]\nmod test_support {\n    pub(crate) mod readiness;\n}\…`

The other two ways out of the subset, so the guard is not the only thing
this rule reads: the inline ancestry `readiness.rs` is reached through,
and a declared name that is not `tests`. Both carry the bare `test` guard,
so each isolates one condition.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolve() {`

The resolver **refuses** every shape it cannot resolve, rather than guessing.

Both wrong answers are silent. A missing skip leaves a test file inside a
census's domain, where a fixture reads as a production offender and someone
looks; a spurious one removes a real production file from every census
below and nothing says so. So the derivation refuses instead of choosing,
and every refusal below is driven — none of them is reachable from this
tree, which is exactly why they would otherwise be code nobody has watched
work.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert_eq!(`

(1) Malformed input the scan cannot tokenise.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `for unreadable in [`

(2) A predicate the entailment grammar cannot read. Unresolved is
refused, not treated as "not test" — because "not test" is the answer
that keeps a file in a census's domain, and a scan that cannot read a
guard does not know which direction is safe.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `for unreadable in [`

The parser's own refusals, driven directly.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `for pathed in [`

(3) `#[path]`, which is the one construct that can point a declaration
outside its own directory — and therefore the one that could build the
cycle asserted against below. Refused rather than resolved, in both the
direct and the `cfg_attr` forms.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert!(`

And it is refused **because it reaches a module**: the same attribute on
something else is not this derivation's business, and refusing it would
be a scan that fails on files it has no claim about.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `for shaped in [`

(3b) **A macro body holding a module-shaped sequence.** A macro invoked
at item position can expand to a module, and nothing here can tell which
does — so a body whose tokens *could* be one is refused rather than
either walked (which invents a declaration for a file the macro never
names) or silently dropped (which loses a real one). Every delimiter,
and the `macro_rules!` definition form, which carries a name between the
`!` and its body.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `"macro_rules! r#mod {\n    () => {\n        mod x;\n    };\n}\n",`

Raw identifiers, on both halves: a macro defined with a keyword for a
name, and a module-shaped body whose module is named with one.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `for spaced in [`

**The `!` need not touch the name.** Whitespace and comments between a
macro's name and its `!` are valid Rust, and `#[rustfmt::skip]` keeps
whatever spelling a file was written with -- so a guard keyed on the very
next byte missed exactly the macros somebody had spaced out. Every
spelling below is a real one a formatter would otherwise close up.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `for ordinary in [`

And a macro body with nothing module-shaped in it is discarded in
silence, which is what stops the refusal from being a tax on every
`vec!`, `assert!` and `format!` in the tree.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert!(matches!(`

(4) An inner `#![cfg(…)]` gates the module it is written in, which this
derivation does not model. There are none in this tree; one arriving
fails loudly rather than being read as ungated.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert!(matches!(`

(5) Duplicates, and the control that says the check is per parent module
rather than per file — two modules may each declare an `x`.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `let roots = crate::effects::tests::crate_roots();`

(6) **Candidate paths, the flattening mutation, and the crate roots.**
The inline path is part of the directory. A resolver that dropped it
looks in `agent/proc/readiness.rs`, which does not exist — so the failure
is a zero-candidate refusal if you are lucky, and the wrong file if a
module of that name is ever added beside it.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert_eq!(`

**A crate root owns its directory; an ordinary module does not**, and
which files are roots is read from this package's manifest rather than
from their names — `PR72-TARGETS-001`. `mod.rs` is the first case
wherever it sits; everything else is the first case exactly when the
manifest names it.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert!(`

**The live instance the stem rule got wrong in this tree.**
`examples/probe.rs` is an `example` target, so it is a crate root and its
out-of-line children live in `examples/` — and `scanned_sources` walks
`examples/**`, so this is inside a census's domain rather than
hypothetical. A stem rule answers `examples/probe/`, which is a directory
Cargo does not compile out of.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert_eq!(`

**The competing production sibling, decided rather than refused.** A
nested `src/a/lib.rs` this manifest never names is the ordinary module
`a::lib`, so `mod tests;` in it resolves to `src/a/lib/tests.rs`. Reading
it as a crate root points at `src/a/tests.rs` — a *different file*, a
sibling that may well be production, which the derivation would then
remove from every census as though `a/lib.rs` had declared it, and with
no `src/a/lib/tests.rs` present that wrong reading resolves rather than
refusing. The old derivation could not tell the two apart and refused
both; the manifest tells them apart.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert_eq!(`

The sibling the wrong reading would have claimed, named so the two
readings are visible side by side rather than asserted apart.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `let elsewhere = std::env::temp_dir().join("upstroke-not-this-package/src/lib.rs");`

**Outside the package is refused, not resolved.** An inventory is a
statement about one package; a file that is not inside it is one the
inventory says nothing about, and answering anyway would be the guess
this repair removed.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `let pair = named("src/a.rs", &[], "b").expect("an ordinary module");`

(7) **Zero and two candidates.** Two is `x.rs` and `x/mod.rs` both
present — a competing `mod.rs` that Rust itself refuses to compile and
that a resolver taking the first match would silently pick a side in.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `let base = Path::new("src/agent");`

(8) **Path escape.** A candidate must descend from the declaring file's
directory through plain components. This holds by construction while
`#[path]` is refused, and the two are one control with two halves.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `let edge = |from: &str, to: &str| (PathBuf::from(from), PathBuf::from(to));`

(9) **Cycles.** The derivation reads every guard from the file above, so
a cycle means a guard attributed to a file that does not inherit it. Not
reachable while directory-derived candidates descend, which is the reason
to drive it here rather than a reason to leave it unchecked.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `let branching = vec![`

**A branching graph, with the cycle on the second edge out of a node.**
The first version walked `edges.iter().find(…)` — one outgoing edge per
node — so it followed `a -> b`, found `b` a leaf, and reported the whole
graph acyclic while `a -> c -> a` sat beside it. Every node here has an
outgoing edge, so a detector that merely *terminates* still passes; what
separates them is which edges get walked.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `assert_eq!(`

The same shape without the back edge is a tree, so the branching itself
is not what the detector is reacting to.

## `fn the_module_resolver_refuses_every_shape_it_cannot_resolv…` › `let deferred = vec![`

A cycle reachable only through a node whose *first* edge leads away from
it: the depth-first walk has to come back and take the second.

## `fn a_census_handed_a_source_root_the_manifest_does_not_describe_is_refused() {`

A census handed a source root the manifest does not describe is **refused**.

The other half of `PR72-TARGETS-001`'s fail-closed side. `source_root` is the
caller's claim about where the crate's sources live and the inventory is the
manifest's; when no target sits under it the two are about different trees,
and every module directory the census then resolves is resolved against an
inventory that says nothing about the files in hand. Driven, because no
caller in this tree passes such a root and an arm nobody has watched refuse
is an arm nobody has watched.

## `fn the_cfg_census_resolves_module_directories_through_the_target_inventory() {`

The cfg census resolves a `mod name;` through the **same** target inventory.

`PR72-TARGETS-001`, second half. `cfg::module_dir` was a second copy of the
rule `census_domain` had already stopped trusting — `matches!(stem, "mod" |
"lib" | "main")` — and it was the copy that was still wrong on this tree
rather than only on a hypothetical manifest: `examples/probe.rs` is an
`example` target, `scanned_sources` walks `examples/**`, and the stem rule
puts that file's children in `examples/probe/`. `PR5D-VISIBILITY-CHECK-\
DUPLICATED` is the standing entry for a rule written twice; this is the
second copy retired, and this is the control that says so, because the tree
declares no `mod` inside `examples/probe.rs` today and a census that only
ran over the tree would not notice either reading.

## `fn the_cfg_census_resolves_module_directories_through_the_t…` › `("src/lib.rs", "src"),`

A crate root owns its own directory. All three of this package's
targets, so the answer is read from the manifest rather than from two
stems that happen to agree with it.

## `fn the_cfg_census_resolves_module_directories_through_the_t…` › `("src/engine/mod.rs", "src/engine"),`

`mod.rs` is a crate root's shape wherever it sits.

## `fn the_cfg_census_resolves_module_directories_through_the_t…` › `("src/effects.rs", "src/effects"),`

And an ordinary module owns a directory named after it — including
one whose stem is `lib`, which the retired rule read as a root.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_bin_path_is_one() {`

**The crate roots come from the manifest, and an arbitrary `[[bin]] path` is
one of them.**

`PR72-TARGETS-001`. Which files Cargo compiles as crate roots decides which
file every out-of-line `mod name;` in the tree resolves to, and the previous
derivation decided it from the file's stem: `lib.rs`/`main.rs` at the source
root was a root, the same stem deeper was refused, anything else was an
ordinary module. A manifest may name **any** path as a target, so the third
arm is a guess — and it is the arm that fails silently, because reading a
root as an ordinary module points its children one directory too deep, at a
sibling that may well exist.

Driven against a manifest built for it rather than against this tree, for
the reason a refusal is always driven here: this package's targets are
`src/lib.rs`, `src/main.rs` and `examples/probe.rs`, so nothing in it
exercises an arbitrary bin path at all. The **exact inventory** is asserted,
not a membership test, and the stem rule this replaces is written out beside
it and shown to disagree — a control that both readings pass is a control
that measures neither.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `fn by_stem(file: &Path) -> PathBuf {`

The rule this replaces, written out so the disagreement is measured
rather than asserted. `mod` is common ground; the roots are not.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `for (file, owns, stem_says) in [`

Each of the three cases, and the stem rule's answer beside it.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `("src/tools/odd.rs", "src/tools", "src/tools/odd"),`

An arbitrary bin path is a crate root: its children live beside it.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `("src/deep/nest/main.rs", "src/deep/nest", "src/deep/nest"),`

So is a `main.rs` that is not at the source root, because this
manifest says so — the case the old derivation refused outright.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `("src/a/lib.rs", "src/a/lib", "src/a"),`

And a `lib.rs` the manifest never names is an ordinary module, which
the old derivation also refused.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `let disagreements = ["src/tools/odd.rs", "src/a/lib.rs"]`

Two of the three disagree, and the two that do are the ones no rule about
file names can get right. The third is common ground and is here so the
comparison is not silently vacuous.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `let missing = scratch.join("no-such-package");`

**Fail closed.** Every refusal is driven, because none is reachable from
this tree and an unreachable arm is one nobody has watched work.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `"{\"packages\":[{\"manifest_path\":\"/somewhere/else/Cargo.toml\",\"targets\":[{\"src_pat…`

A package that is not this one. Falling back to \"the first package\"
here is the fail-open shape: an inventory for somebody else's targets
reads as an inventory, and every module directory below is resolved
against it.

## `fn the_crate_roots_come_from_the_manifest_and_an_arbitrary_…` › `let live = crate::effects::tests::crate_roots();`

And the real package's inventory is the one the census resolves against,
read through the same reader.

## `fn the_file_level_lint_reader_is_a_census_instrument_and_not_a_shipped_api() {`

The file-module-level lint reader is a **census instrument**, not a shipped
API.

`PR72-API-001`. It arrived as a `pub fn` in this file's production region,
which is a public surface added so that a test could call it: the binary
consults it nowhere, and `effects/allowlist.toml` records `allows = []` for
this file precisely because everything above the `#[cfg(test)]` cut is meant
to be the parsers and the frozen lists and nothing else. It is
`#[cfg(test)] pub(crate)` now, in a module at the bottom.

Asserted over the region rather than by eye, and in both directions: the
name is absent from the production region and present in the file, so a
typo in the needle fails the second half instead of passing the first.

## `fn the_file_level_lint_reader_is_a_census_instrument_and_no…` › `fn absent_from_production(source: &str) -> Vec<String> {`

The two claims, over one spelling of the file.

**Structural, and therefore line-ending-blind.** The first draft
searched for the literal `"#[cfg(test)]\npub(crate) mod lint_levels {"`,
which is a search for a spelling of a newline: the guest checks this
tree out with CRLF and that needle is `\r\n` there, so the assertion
held on Unix and on nothing else. What actually has to be true is that
the item is **removed by** [`crate::effects::production_code`], which is
what `#[cfg(test)]` means to every census in this crate — and that is
read from the region, not from a byte sequence spanning a line.

## `fn the_file_level_lint_reader_is_a_census_instrument_and_no…` › `let crlf = source.replace('\n', "\r\n");`

The same file with the line endings the Windows guest gives it.

## `fn the_file_level_lint_reader_is_a_census_instrument_and_no…` › `assert!(`

The visibility is narrow as well as gated, and that fits on one line in
either spelling.

## `fn the_file_level_lint_reader_is_a_census_instrument_and_no…` › `for prologue in [`

The instrument still answers where it is used, so narrowing it did not
narrow it out of existence — under both spellings of a line ending.

## `fn the_file_level_lint_reader_answers_what_rustc_does() {`

**The file-level lint reader answers what rustc does**, on a table rustc
decides.

`PR72-LEVELS-001`. The reader returned at the *first* attribute naming the
lint, and a prologue is ordered: `#![deny(L)] #![allow(L)]` is a file where
`L` is allowed, and the reader called it a denial. Two censuses turn on that
answer — `every_allow_of_a_governed_lint_is_module_level_and_in_the_allowlist`
here and `runner::container::tests::every_child_module_of_the_container_
funnel_states_its_own_lint_level` — and the wrong answer is the reassuring
one: a module reported as having closed `PR6-LANEF-004` by a prologue whose
next line reopens it.

**No lexical restatement is accepted as authority.** The table below does not
say what each prologue means. Each row is compiled by `clippy-driver` under
this repository's own `clippy.toml`, against a body that reaches
`std::fs::write` — a denied path — and the *observed* diagnostics are the
verdict. The reader is asked the same question and its answer is turned into
a prediction of what the compiler must have emitted; the two are compared.
The only sentence written by hand is the bridge between a level and its
observable, and every arm of that bridge is exercised by a row, so a bridge
that was wrong could not stay green.

The rows include the two shapes that are the whole reason for the repair —
`deny` then `allow`, which must be **allow**, and `forbid` then `allow`,
which is `E0453` and not a level at all — and the decoys the blanking exists
for.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `const BODY: &str = "pub fn go(p: &std::path::Path) { let _ = std::fs::write(p, \"x\"); }\…`

A body that reaches a denied path exactly once, so a `disallowed_methods`
diagnostic is produced by every level that does not suppress one.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `fn compile(dir: &Path, tag: &str, source: &str) -> (bool, Vec<(String, String)>) {`

Compile one prologue and return whether it built, plus every diagnostic
that carries a code, as `(level, code)`.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `fn predict(resolution: Resolution) -> (bool, Vec<&'static str>, bool) {`

What the compiler must have done, if the reader's answer is right.

`(the crate builds, the levels at which the lint fired, E0453 present)`.
The one hand-written sentence in this test, and every arm of it is
reached by a row below.

## `fn predict(resolution: Resolution) -> (bool, Vec<&'static s…` › `return (false, Vec::new(), true);`

Not a level: the prologue is rejected and the lint never runs.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `let table: &[(&str, &str)] = &[`

Every row is a prologue. Nothing here says what it means.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `(`

The qualified and bare spellings are one lint, and the order still
decides. `normalize_lint` is the bridge, and rustc accepts the bare
name (with a rename warning of its own, which this ignores).

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `(`

The decoys the blanking exists for: a level in prose and a level in a
string literal govern nothing, and an outer attribute on an item is
not the file module's.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `assert_eq!(`

The same prologue with the line endings the Windows guest gives it.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `assert!(`

**The table is not vacuous.** Four distinct compiler behaviours are
reached — clean, warned, errored, and rejected outright — so a reader
that collapsed to one answer could not pass.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `let deny_then_allow = format!(`

And the two claims the repair is named for, stated as values now that the
compiler has confirmed the reader on every row above.

## `fn the_file_level_lint_reader_answers_what_rustc_does()` › `let mut restated = Vec::new();`

**No file in this tree states one governed lint twice at file-module
level**, so the ordering above changes no answer today. That is the point
of measuring it: the repair is about what the reader does when one
arrives, and this says none has, rather than leaving it to be believed.

## `fn every_pr6_refusal_st16_variant_and_invariant_clause_names_a_test_or_an_owner() {`

---------------------------------------------------------------------------
R3b: the enumerations the reconciliation promised and did not supply
---------------------------------------------------------------------------

## `fn every_pr6_refusal_st16_variant_and_invariant_clause_names_a_test_or_an_owner() {`

The three enumerations this section supplies -- the nine refusals with their
ordering predicates, the twelve ST-16 variants and the twelve clauses -- and
the body that holds them are in `contract_mappings::mappings` with the
T-CONTAINER transcription above. They are resolved by the same
`defining_test_sites` census and belong beside it; the name below is the
harness and delegates.
