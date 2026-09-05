# `src/effects/tests/cfg.rs`

Extended notes for [`src/effects/tests/cfg.rs`](../../../../src/effects/tests/cfg.rs).

The code is the authority for what it does. These notes started as the module's source prose.
Each code fragment in a heading is an exact source substring. When a heading names an enclosing
item before `›`, find that item first, then the following fragment within it.

## Module

The cfg census: every `cfg` occurrence in the tree, and the runners that
actually compile it.

The predecessor collected `target_os = "..."` names wherever they appeared
at a code position and read each name as a platform demanding its own Clippy
runner. This module decides predicates instead, against the valuations
`super::ci_model`'s [`CI_TARGETS`] say CI sets — completely, so an unmodelled
name is a hard failure rather than an optimistic guess, and per invocation,
because `--all-targets` compiles the library twice and merging the two would
make `all(test, not(test))` look reachable.

Three distinctions carry the weight, and each is a claim an earlier version
got wrong:

  * **Not every cfg gates.** `cfg!(P)` is an expression and
    `#[cfg_attr(P, attr)]` conditions an attribute; the code around either is
    compiled everywhere. [`CfgForm`] keeps the three apart, and only
    [`CfgForm::Gate`] is a platform demand.
  * **An item's predicate is not the attribute written on it.** Stacked
    `#[cfg]`s conjoin, and so does every enclosing guard — the module block
    it sits in, and, for a whole-file module, the `#[cfg(test)] mod name;`
    that declares the file. [`CfgSite::written`] and [`CfgSite::rendered`]
    are both kept so the difference is visible.
  * **Position and text come from different views.** Nesting and brace depth
    read the blanked source, where a `cfg(` in prose or in a string literal
    is spaces; the predicate text reads the raw span, because blanking erases
    the platform name along with the quotes.

The `#[test]` wrappers that drive this stay in `super`, together with the
join against the workflow contract: this module is the census, not the
harness, and every name in it is deliberately not a test name.

The three effect denials are **restored** here rather than inherited.
`super`'s module-level allowance exists because that file drives
`clippy-driver` over fixtures it has to create; this module reads the tree
it is handed and writes nothing, so the allowance has no business reaching
it.

## `pub(super) enum CfgPred {`

A cfg predicate, parsed.

## `impl CfgPred` › `pub(super) fn render(&self) -> String {`

Canonical text, so two spellings of one predicate are one row.

## `impl CfgPred` › `fn conjunction(mut parts: Vec<CfgPred>) -> CfgPred {`

The conjunction of `parts`, without an `all(...)` around a single one.

## `const MODELLED_FLAGS: [&str; 3] = ["test", "unix", "windows"];`

The bare cfg flags this census models. Anything else is a hard failure.

The list is short because the tree is: `test`, `unix` and `windows` are every
bare flag any `#[cfg]` in `src/` and `examples/` names. Keeping it exactly
that short is the point -- a census that guesses at `debug_assertions` or
`miri` would be asserting what CI sets rather than reading it, and the right
answer to a new flag is to decide it here, once, in front of a reviewer.

## `const MODELLED_KEYS: [&str; 7] = [`

The `key = "value"` cfg keys this census models. Same rule.

## `struct Valuation {`

One compilation's **complete** cfg valuation.

Complete is the load-bearing word. A name this valuation does not carry is
not "unknown": rustc leaves it unset, so `cfg(name)` is **false**. That is
only sound while the set of names is closed, which is what [`MODELLED_FLAGS`]
and [`MODELLED_KEYS`] close and what [`holds`] refuses to guess past.

## `struct Valuation` › `invocation: String,`

What the invocation is, for a failure message that can be acted on.

## `fn ci_valuations() -> Vec<Valuation> {`

Every compilation CI performs, as a valuation.

Two per runner, because `cargo clippy --all-targets` and `cargo test
--all-targets` each compile the library twice -- once as a library, once as a
test harness with `test` set. They are kept apart rather than merged: merging
would set `test` and `not(test)` in one valuation and make `all(test,
not(test))` look reachable.

## `fn holds(pred: &CfgPred, valuation: &Valuation) -> Result<bool, String> {`

Whether `pred` holds under `valuation`, or the name that made it undecidable.

There is no third answer. An unmodelled name returns `Err` and fails the
census, which is the difference between this and the version it replaces:
that one returned `Unknown` and the caller counted `Unknown` as coverage, so
a predicate nobody could decide was reported as compiled by every runner.

## `pub(super) fn compiled_by(pred: &CfgPred) -> Result<BTreeSet<&'static str>, String> {`

The runners that **actually compile** a body guarded by `pred`.

A runner is in the set when some invocation it performs makes the predicate
true. Not "might" -- the predecessor's `might` is what let an undecidable
predicate claim three platforms.

## `struct CfgReader<'a> {`

A recursive-descent reader for the cfg predicate grammar.

Hand-written on purpose. The alternative is a Rust parser crate, and the only
dependency this crate was authorised to add is the YAML one; the grammar
`cfg` accepts is small enough that reading it exactly costs less than
carrying `syn`, and every form it accepts is exercised below.

## `impl<'a> CfgReader<'a>` › `fn skip_space(&mut self) {`

Whitespace and comments alike.

The reader skips comments itself rather than being handed a
comment-blanked view, because the repository's comment blanker *deletes*
comment bytes instead of replacing them -- it does not preserve
positions, and every span here is a byte range. `#[cfg(all(\n // why\n
unix))]` is legal Rust and reads correctly through this.

## `impl<'a> CfgReader<'a>` › `fn value(&mut self) -> Result<String, String> {`

A cfg value: any Rust string literal, raw or escaped.

`#[cfg(target_os = r"linux")]` and `#[cfg(target_os = "li\x6eux")]` are
both valid Rust naming the same platform, and a reader that handles only
`"..."` with a backslash passed through verbatim decodes the second to a
different platform than rustc does. Neither form appears in this tree
today, which is exactly why the control fixture carries both: a lexical
gap that nothing exercises is a gap nobody notices.

## `impl<'a> CfgReader<'a>` › `fn escape(&mut self, out: &mut String) -> Result<(), String> {`

One escape sequence, already past its backslash.

## `fn escape(&mut self, out: &mut String) -> Result<(), String>` › `b'x' => {`

`\x41`: exactly two hex digits, and only ASCII in a string.

## `fn escape(&mut self, out: &mut String) -> Result<(), String>` › `b'u' => {`

`\u{1F600}`: one to six hex digits in braces.

## `fn escape(&mut self, out: &mut String) -> Result<(), String>` › `b'\n' => {`

A backslash before a newline eats the following whitespace.

## `impl<'a> CfgReader<'a>` › `fn raw_value(&mut self) -> Result<String, String> {`

`r"..."`, `r#"..."#`, `r##"..."##` — no escapes inside.

## `pub(super) fn parse_cfg(inside: &str, attribute_form: bool) -> Result<CfgPred, String> {`

Parse the inside of a `cfg(...)`, or of a `cfg_attr(...)` up to its first
comma.

## `pub(super) enum CfgForm {`

What a `cfg` occurrence does to the code around it.

Only one of the three gates, and conflating them is the defect this
distinction repairs: a census that counts all three demands a Clippy runner
for platforms whose bodies are compiled everywhere.

## `pub(super) enum CfgForm` › `Gate,`

`#[cfg(P)]` and `#![cfg(P)]`. The item exists only where `P` holds, so
this is the only form whose predicate is a platform demand.

## `pub(super) enum CfgForm` › `Attribute,`

`#[cfg_attr(P, attr)]`. The **attribute** is conditional; the item is
compiled everywhere. `#[cfg_attr(not(windows), allow(dead_code))]` in
this tree does not make its function Windows-only.

## `pub(super) enum CfgForm` › `Macro,`

`cfg!(P)`. A compile-time boolean *expression*: both arms of the `if`
around it are compiled and type-checked on every platform.

## `pub(super) struct CfgSite {`

One `cfg` occurrence, with the predicate that actually decides it.

## `pub(super) struct CfgSite` › `pub(super) written: String,`

The predicate as written on this occurrence.

## `pub(super) struct CfgSite` › `pub(super) rendered: String,`

The predicate that decides whether the item is compiled: `written`
conjoined with every stacked attribute, every enclosing guard, and the
file's own guard when it is a whole-file module. Equal to `written` for
the non-gating forms, which decide nothing.

## `fn balanced(bytes: &[u8], at: usize, open: u8, close: u8) -> Option<usize> {`

The index of the byte closing the group `open` at `at`.

## `pub(super) fn module_dir(path: &str) -> String {`

The directory a file's `mod name;` declarations resolve inside.

**The crate roots come from the manifest**, through the one inventory
`census_domain` resolves against. This used to read `matches!(stem, "mod" |
"lib" | "main")` — a second, lexical copy of the rule that census had
already stopped trusting, and the copy that was still wrong in this tree:
`examples/probe.rs` is an `example` target, so it is a crate root whose
children live in `examples/`, and the stem rule answered `examples/probe`.
An arbitrary `[[bin]] path` is the same error with more room in it.
`PR5D-VISIBILITY-CHECK-DUPLICATED` is the standing entry for a rule written
twice; this is the second copy retired rather than re-synchronised.

## `pub(super) fn cfg_regions(sources: &[(String, String)]) -> (Vec<CfgSite>, Vec<String>) {`

Every `cfg` occurrence in `sources`, and every one that could not be read.

Two passes, because a file's guard is written in another file. Pass one reads
the `#[cfg(P)] mod name;` declarations and resolves each to the file it
governs; pass two scans every file with the guard it inherited. The files
[`WHOLE_FILE_TEST_MODULES`] lists exist only under a `cfg(test)` module
declaration -- the population
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
resolves independently -- and a census that missed it would read every
predicate in them as unconditional.

**All but one of them, and the difference is deliberate.** This pass
reads `#[cfg(P)] mod name;` -- the attribute is on the declaration -- and
`agent/proc/test_support/readiness.rs` is declared with no attribute at all,
inside an inline `#[cfg(test)]` module. `census_domain` resolves that
ancestry and this pass does not, because the two answer different questions:
that one decides which files are test code, this one decides what predicate
each `cfg` occurrence is under. The floor below is `>=`, and readiness.rs
carries no `cfg` occurrence for this census to misattribute -- measured, and
asserted by the census test above staying green with the file in its domain.

Two views of each file, and which one answers which question is the part that
has cost this repository time before:

  * **position, nesting and brace depth** come from
    `blank_comments_and_strings`, where a `cfg(` inside prose or inside a
    string literal is spaces, and so is a brace. That is what keeps this
    census off its own explanatory comments -- an earlier version reported
    `freebsd` quoted from the paragraph beside it.
  * **the predicate text** is the raw span, because
    `blank_comments_and_strings` erases the platform name along with the
    quotes: reading the name from the blanked view is why the first version
    found only `windows`. [`CfgReader`] skips comments on its own, which is
    the part the comment blanker cannot do here -- it deletes comment bytes
    rather than blanking them, so it does not preserve the positions this
    scan is built on.

## `pub(super) fn cfg_regions(sources: &[(String, String)]) -> (Vec<CfgSite>, Vec<String>) {` › `let mut declared: Vec<(String, String, CfgPred)> = Vec::new();`

Pass one: which file each `#[cfg(P)] mod name;` governs.

## `pub(super) fn cfg_regions(sources: &[(String, String)]) -> (Vec<CfgSite>, Vec<String>) {` › `let mut guards: BTreeMap<String, CfgPred> = BTreeMap::new();`

A guarded file may itself declare a guarded module, so the guards compose.
Bounded rather than recursive, and the bound is checked: a cycle here
would otherwise be an infinite loop inside a test.

## `pub(super) fn cfg_regions(sources: &[(String, String)]) -> (Vec<CfgSite>, Vec<String>) {` › `let mut sites = Vec::new();`

Pass two: every occurrence, under the guard its file inherited.

## `fn scan_file(`

One file's occurrences.

`declarations` collects `#[cfg(P)] mod name;` for the caller's first pass;
`sites` and `unreadable` are the second pass's output. A pass wanting only
one of the two hands the other an empty vector it then discards.

## `let mut item_scopes: Vec<(usize, CfgPred)> = Vec::new();`

Active while the current depth is INSIDE the body the item opened.

## `let mut inner_scopes: Vec<(usize, CfgPred)> = Vec::new();`

`#![cfg(P)]`: active from the depth it was written at, downward.

## `let mut pending: Vec<(usize, CfgPred)> = Vec::new();`

The `#[cfg]`s stacked on the item being read, in source order.

## `if byte == b'#' {`

-- an attribute ---------------------------------------------------

## `if !pending.is_empty() {`

-- the item those attributes belong to -----------------------------

## `if byte == b'{' {`

-- ordinary tokens --------------------------------------------------

## `if &blanked[start..end] == "cfg" {`

`cfg!(P)`, and ONLY with the `!`. A bare `cfg(` is an ordinary
call or a function named `cfg`, which is not an attribute and not
a macro; treating it as one is how a census invents a predicate
out of `fn cfg(bits: u32)`.

## `enum ItemShape {`

What the item starting at `at` is, as far as scoping cares.

## `enum ItemShape` › `Module { name: String, body: Option<usize> },`

`mod name { … }` or `mod name;`.

## `enum ItemShape` › `Block,`

Anything else with a braced body: a function, an `impl`, a `struct`, a
bare block. Its guard reaches everything inside it.

## `enum ItemShape` › `Flat,`

Anything that ends before a brace: a `use`, a `const`, a struct field, a
match arm. Nothing is nested under it.

## `fn item_shape(bytes: &[u8], at: usize) -> ItemShape {`

Read far enough to tell those three apart.

The scan stops at the first `;` or `,` outside any bracket, which is what
ends a flat item, and at the first `{` outside any bracket, which opens a
body. Brackets are tracked because `const X: [u8; 2]` puts a `;` inside one
and `fn f(a: u8, b: u8)` puts a `,` inside one.

## `pub(super) const NO_CI_RUNNER_COMPILES: [(&str, &str); 2] = [`

The effective predicates in this tree that no CI runner compiles, and why
each is deliberate.

An equality, not a filter. A new predicate that no runner compiles fails this
census until someone adds the platform's Clippy leg or writes the reason down
here, which is the check the predecessor could not make at all: it collected
`target_os` names, `not(any(unix, windows))` carries none, and five
production regions the denylist has never examined were invisible to it.

## `pub(super) const CFG_CENSUS_CONTROL: &str = r##"//! A control fixture. It is not compiled; it is scanned.`

The census's permanent positive control.

Injected into the **whole** scanned domain rather than parsed on its own:
`CODING_STANDARDS.md` §12 says a control inside a truncated domain does not
prove the domain was scanned, so the control rides along with every real file
and must still be found.

Every row of it is a thing a version of this census got wrong. The first four
are the standing ledger's: a predicate nothing compiles, one everything
compiles, a `target_os` binding that is not a predicate, and the same token
in prose and in a string literal. The rest are the review's: stacked
attributes, a guard on the module rather than the item, the two non-gating
forms, and the two literal shapes a `"..."`-only reader decodes wrongly.
`fn cfg` is there because an ordinary function may be called `cfg`, and a
scanner that reads any `cfg(` as an attribute invents a predicate from its
parameter list.

## `pub(super) const CONTROL_GATES: [&str; 7] = [`

The gate predicates the control fixture must produce, in source order.

## `pub(super) const CFG_ESCAPES: [(&str, &[&str], &str); 12] = [`

The predicate rows the standing ledger and the review name, with the runners
that actually compile each.

`(predicate, the runners that compile it, why the row is here)`.

## `pub(super) const CFG_GATE_FLOOR: usize = 350;`

The floor on the census's gate domain.

A count, because a scan that silently stops reading is a scan that reports
nothing uncovered. The tree carries several hundred gating attributes and the
number moves with ordinary edits, so this is a floor rather than a pin; the
boundary assertions beside it are what pin the shape.

## `pub(crate) static WHOLE_FILE_TEST_MODULES: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {`

The census domain: every file a test-only `mod …;` declaration names,
relative to `src/`, sorted.

Derived by
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`,
and pinned here because every predicate in those files is `all(test, …)`
rather than what it says: a census that resolved none of them would read
several hundred predicates as unconditional and never notice.

**A collection of paths, and the type says so.** Every reader compares these
against a `Path` the tree produced, and `CODING_STANDARDS.md` §8 keeps a path
out of `String`: a lossy rendering maps two distinct non-UTF-8 paths onto
one, and rewriting `\` to `/` turns a backslash -- a legal character in a
Unix file name -- into what reads as a separator. A `&[&str]` named as the
source of truth for path identities says "string" in its type however
carefully each use site converts, so what everything reads is a
`Vec<PathBuf>` and the conversion happens once, here. The `&'static str`
literals inside are the *written* form, because a path literal in source is
how a human writes one; nothing reads them without going through `PathBuf`.

**Not every file this crate compiles only under `cfg(test)`, and the gap is
deliberate.** `census_domain::declared_whole_file_test_modules` does not
close over the file graph, so a declaration inside a file that is itself in
this list derives nothing: `effects/tests.rs` is listed and declares `mod
policy;`, which makes rustc compile `effects/tests/policy.rs` under
`cfg(test)` too, and `policy.rs` is deliberately absent here. Adding it
would widen what every census scans, which is a change to the measurement
and not a correction to this list. That derivation's own doc comment states
the closure it declines and the declaration form its scan cannot see.

**A list rather than a count, because a count does not say *which* files.**
A derivation that swapped one module for another -- same cardinality,
different set -- satisfies every assertion a number can carry, and fails
here naming the file it gained and the file it lost. The modules not called
`tests.rs` were already named individually for exactly that reason;
this is that argument applied to the whole population rather than to the
part of it a file-name rule misses.

**Both populations are read off this list, so neither number is written
anywhere.** The whole of it is the domain above. The subset a literal
`#[cfg(test)] mod tests;` declares is the entries whose file stem is
`tests`: declared under that exact name at their parent's own top level,
with the attribute on the declaration and its effective predicate the bare
`test` atom, so each is a `tests.rs` and the file-name rule `file_stem ==
"tests"` finds it. The rest
-- `scaffold`, `premove`, `fake`, `fixture`, `scratch_tree` and `readiness`
-- differ by **how each
file is reached**, which is the distinction a census gets wrong, and they
are the ones it is most likely to trip over, since a scaffold, a fake and a
readiness protocol exist to name what production names.

**A narrowed guard would stay in this list and leave that subset, and the
disagreement is the signal.** `#[cfg(all(test, unix))] mod tests;` compiles
a whole test file on Unix and no file at all on Windows. It is still
test-only, so the derivation keeps it and it belongs in this domain; it is
not the form `#[cfg(test)] mod tests;`, so it is not in the subset above --
while its file stem is still `tests`, which is the half of this list that
subset is compared against. The two disagree, and the oracle fails naming
the file. That is the decision rather than an oversight: a census skipping
by file name treats such a module as present everywhere, so Windows would
lose it in silence, and the slice that writes one has to say what every
census should do about a module that exists on only some platforms. There
is no such declaration in this tree; PR #101's reviewer supplied the
reproduction and
`a_narrowed_cfg_guard_is_test_only_but_is_not_the_literal_mod_tests_form`
drives it over synthetic input, so no later change can lose the distinction.

**A slice that adds a whole-file test module adds its path here, in sorted
position, in the same commit.** That is the whole edit: both counts follow,
and so does the named-individually set. The entries cluster by directory, so
slices landing in different directories insert far apart in this list. That
argument depends on the written order actually being sorted, and the
initializer asserts it **as written**, before anything normalises it --
every comparison against this list sorts what it reads, so an entry appended
at the end would otherwise satisfy all of them while the claim quietly
stopped being true.

Where it is compared with `>=`, the length is a floor. One entry --
`readiness.rs`, reached through an inline ancestor rather than through an
attribute on its own declaration -- is outside [`cfg_regions`]' grammar and
carries no `cfg` occurrence, so the two derivations agree on the number
without that census having to resolve the ancestry that produces it.

**This is the only place either population is written, and every assertion
about them reads it.** The two counts were stated as English words 37 times
across ten files, and written as an integer literal in five more places, so
one slice adding one whole-file test module falsified every one of them at
once while the `>=` floor stayed green -- and a passing floor is not the
same as a true document. PR #97's review found that, and
the prose now names this constant or describes the population without
counting it.

`pub(crate)` rather than `pub(super)` for one reader outside this directory:
`engine::topology::recover::tests` floors its skip count at `.len()`, which
is the only form of that floor that is not satisfied by the derivation
having gone inert.

## `let written = [`

The written order, which is the sorted one the paragraph above argues
for. Sortedness is checked here rather than in a test so that no reader
of the list can bypass it, and on the literals rather than on the
`PathBuf`s so that what is checked is the text a human reads and diffs.
`>=` rather than `>`, so a path written twice fails here as well: the
oracle compares a `Vec` and not a set for the same reason.
