# `src/effects.rs`

Extended notes for [`src/effects.rs`](../../src/effects.rs).

The code is the authority for what it does. These notes started as the module's source prose.
Each code fragment in a heading is an exact source substring. When a heading names an enclosing
item before `›`, find that item first, then the following fragment within it.

## Module

The compile-time enforcement layer: the effect denylist, the allowlist, the
wrapper classification, and the generated inventories.

The retired `decisions.effect_site_inventory.mechanism` packet described
this module in the four parts below. They record its original rationale.
[DESIGN.md](../../DESIGN.md) is the living authority for product design;
the implementation rules are in [standards §2](../../standards/02_standards_automated_baseline.md)
for lint placement and the effects census, [§3](../../standards/03_standards_design_principles.md)
for effect boundaries, and [§15](../../standards/15_standards_dependencies_and_features.md)
for dependency review. Packet quotations here preserve history and do not
override those rules.

1. **The denylist is rustc-resolved, not lexical.** `clippy.toml`'s
   `disallowed-methods` / `disallowed-types` / `disallowed-macros` name every
   effect primitive the crate can reach, and "aliases, re-exports, function
   values, method calls, and macro-expanded code in this crate resolve to the
   same DefId". [`tests::every_declared_effect_denial_refuses_for_the_reason_it_declares`](effects/tests.md)
   compiles one fixture per shape and asserts the lint each emits, because
   that sentence is a claim about a toolchain and not a law of nature.
2. **An allow of a governed lint lives only where the allowlist says.**
   Module-level, in a file listed in `effects/allowlist.toml`, whose legacy
   section is frozen, may only shrink, and never contains a topology module.
3. **Wrapper classification.** Every externally reachable `fn` of a legacy or
   shared module is classified; the effectful ones join the denylist, "so a
   topology module cannot reach an effect through a legacy wrapper".
4. **Dependency review.** A new dependency performing filesystem, process,
   lock or container effects has its API added to the denylist or is confined
   to a funnel module.

### This module performs no effect

The non-test portion contains parsers, classifiers and frozen lists that
compute from supplied values. Reading `clippy.toml`, writing
`effect_sites.json` and compiling fixtures all happen in the test region.
The retired packet's `outputs` key also described inventory generation
by a test. That is why this file is
in the funnel section of the allowlist while claiming something stronger than
any other entry there.

### Reading the historical references

References to `decisions.*`, `mechanism` and `outputs` below name keys in
the retired packet. They explain provenance; the current design and
standards remain authoritative. `*_verification_dispositions`,
`finding_dispositions[].rationale` and the `v4_`..`v15_` keys belong to the
packet's disposition history and are not reproduced here.

## `use std::collections::BTreeSet;`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, and
the entry there records `allows = []`. This module carries no attribute at
all, which is the strongest form of the claim above: it reaches no denied
primitive, and the one `std::process::Command` its text contains is inside
`DENIAL_FIXTURES`, a string constant compiled elsewhere in order to be
refused. `decisions.effect_site_inventory.mechanism` (2).

## `pub const CLIPPY_TOML: &str = "clippy.toml";`

---------------------------------------------------------------------------
The artifacts, by the names `outputs` gives them
---------------------------------------------------------------------------

## `pub const CLIPPY_TOML: &str = "clippy.toml";`

`effect_site_inventory.outputs`: "clippy.toml".

## `pub const ALLOWLIST_TOML: &str = "effects/allowlist.toml";`

`effect_site_inventory.outputs`: "effects/allowlist.toml".

## `pub const WRAPPERS_TOML: &str = "effects/wrappers.toml";`

`effect_site_inventory.outputs`: "the wrapper classification".

## `pub const EFFECT_SITES_JSON: &str = "effect_sites.json";`

`effect_site_inventory.outputs`: "effect_sites.json (from the enums)".

## `pub const RESIDUE_CLASSES_JSON: &str = "effects/residue-classes.json";`

`effect_site_inventory.outputs`: "the residue-class evidence record (per
element: constructed, classified, recovered; per site: sampling N and
observed-class histogram)".

The *declarations* half. The histogram half is `RESIDUE_HISTOGRAM_JSON`,
and the split is forced rather than chosen — see there.

## `pub const RESIDUE_HISTOGRAM_JSON: &str = "effects/residue-histogram.json";`

The **observed-class histogram** half of the same record (`PR5-CONF-004`).

`outputs` requires, per site, "sampling N **and observed-class histogram**".
`RESIDUE_CLASSES_JSON` is generated from the frozen enums and compared
byte-for-byte, which it must be — and a histogram is machine-varying by
construction, since which class a kill sample lands in is a race between the
kill and Git. A count cannot be byte-pinned and a byte-pinned file cannot
carry one, so the histogram is emitted to this path on every run of
`workspace_manager::tests::sampled_git_child_kills_every_residue_classified_
and_recovered`, which then reads it back. Not checked in: its contents are a
property of the machine that produced them, and a stale copy of somebody
else's numbers would be worse than no copy.

## `pub const FUNNEL_MODULES_JSON: &str = "effects/funnel-modules.json";`

Where each site's funnel **bodies** actually are, where that is not what
`EFFECT_SITES_JSON`'s `module` column says (`PR5-CONF-018`).

`effect_sites.json` is generated from the frozen enums, so its `module`
column is `EffectSiteId::module()` — PR3's answer, and the packet's:
`mechanism` (2) places "the answer funnels in `src/interaction.rs`". PR5's
lane B put the three Answer funnel bodies in `src/rundir.rs` and left
`interaction::{write_question, write_answer, read_answer}` as delegations,
so for `Answer.Ingest`, `Answer.PublishRename` and `Answer.StageWrite` the
checked-in artifact states something that is not true of this tree — and the
artifact is attached to gate reports, where a reader has no way to know.

The generator is `src/topology/effects.rs`, frozen under the owner ruling of
2026-08-20, so the column cannot be corrected in place and the bodies are not
moved: `AnswerSite`'s three funnels close over `rundir`'s private `funnel`
and `RunDirHooks`, and relocating them to satisfy a column would be a slice
redesigning what it implements. What ships instead is this companion, which
carries the tree's own answer beside the inventory's for **every** site, so
the pair is true where either alone is not. Derived, compared byte-for-byte,
and regenerated by the same `REGENERATE` switch, so it cannot drift.

## `pub const REGENERATE: &str = "UPSTROKE_REGENERATE_EFFECT_ARTIFACTS";`

The environment variable that turns the generating tests into writers.

A generated artifact that is only ever *compared* rots into a chore nobody
can discharge; one that is only ever *written* proves nothing. Both, keyed on
this, is the ordinary resolution.

## `pub const GOVERNED_LINTS: &[&str] = &[`

---------------------------------------------------------------------------
(2) The governed lints and where an allow of one may live
---------------------------------------------------------------------------

## `pub const GOVERNED_LINTS: &[&str] = &[`

The six lints `mechanism` (2) governs, as bare names.

> "permits allow/expect of disallowed_methods, disallowed_types,
> disallowed_macros, clippy::style, clippy::all, or warnings only as
> module-level attributes in files listed in effects/allowlist.toml"

Bare, because an attribute may write either `disallowed_methods` or
`clippy::disallowed_methods` and the sentence names them both ways in one
breath. `normalize_lint` is the bridge.

## `pub const USED_GOVERNED_LINTS: &[&str] = &[`

The three governed lints this slice actually uses, fully qualified.

`clippy::style`, `clippy::all` and `warnings` are governed and **unused**:
each would suppress far more than an effect denial, and
[`tests::the_three_blunt_governed_lints_are_used_by_nobody`](effects/tests.md) asserts the
count is zero rather than leaving it to habit.

## `pub fn normalize_lint(entry: &str) -> Option<&'static str> {`

The bare lint name an attribute entry refers to, if it is governed.

`clippy::disallowed_methods` and `disallowed_methods` are the same lint;
`clippy::too_many_arguments` is not governed and answers `None`.

## `pub struct GovernedAllow {`

One `allow`/`expect` of a governed lint, as the scan found it.

## `pub struct GovernedAllow` › `pub line: usize,`

1-based line of the attribute's `#`.

## `pub struct GovernedAllow` › `pub inner: bool,`

Whether it is an inner attribute (`#![…]`).

## `pub struct GovernedAllow` › `pub module_level: bool,`

Whether it is module-level: an inner attribute in the file's prologue, or
an outer attribute on a `mod` item.

## `pub struct GovernedAllow` › `pub lints: Vec<String>,`

The governed lints it names, normalized, in source order.

## `pub struct GovernedAllow` › `pub written: Vec<String>,`

Every lint it names, as written — so a widening is visible.

## `pub struct GovernedAllow` › `pub keywords: Vec<&'static str>,`

Which attribute keywords the governed lints were found under: `allow`,
`expect`, or both if one attribute writes both.

The two are not the same permission and the placement rule now
distinguishes them. `allow` is unconditional and says nothing when the
thing it permits stops happening; `expect` is refused by the compiler
when it goes unfulfilled, which is what makes a per-site one a count the
build owns rather than a claim a reviewer has to re-check.

## `pub struct GovernedAllow` › `pub reasoned: bool,`

Whether it carries a `reason = "…"`.

## `pub fn blank_comments(source: &str) -> String {`

`source` with every comment and string literal replaced by spaces of the same
length, newlines preserved.

The scan has to be blind to text that only *looks* like an attribute.
`PR4-CENSUS-COMMENT-ORACLE` is in the standing ledger because a source census
counted a doc comment; this module is worse placed than most, since its own
build-refusal fixtures are `#[allow(clippy::disallowed_methods)]` written
inside doc comments and string literals. Blanking rather than deleting keeps
every byte offset — and therefore every line number — exact.

Raw strings (`r"…"`, `r#"…"#`), byte strings, char literals and escapes are
handled; a `'a` lifetime is not a char literal and is left alone.
Comments blanked, **string literals kept**.

The other half of `blank_comments_and_strings`, and a separate function
because a census whose needle lives *inside* a string cannot use that one:
it blanks a literal including its quotes, so a search for `"docker` in its
output looks for a byte sequence the haystack can no longer contain. That is
not hypothetical — it is what the `mechanism` (1) "docker invocation
helpers" census did until PR6, which is why it stayed green when a real
`const DOCKER_PROGRAM: &str = "docker"` landed in production.

**One implementation, one caller shape.** `PR5D-VISIBILITY-CHECK-DUPLICATED`
is the standing entry for a parser written twice in this tree, so this lives
here beside its sibling rather than in each census that wants it.

Line comments, block comments (nested), char literals, escapes and **raw
strings** (`r"…"`, `r#"…"#`, `b"…"`, `br#"…"#`) are all handled: this
function tokenises exactly as `blank_comments_and_strings` does and differs
only in keeping a literal's bytes instead of blanking them. Byte offsets are
not preserved; line breaks are.

#### Why raw strings are modelled, and the direction the old limit had wrong

This used to track only `"` and document the omission as safe: "the failure
mode is a needle this function does *not* find, which makes a census that
uses it report something missing — **loud** — rather than accept something
extra." **That is backwards for a census over an expected set, which is what
every caller here is** (`PR6-LANEF-005`).

`r#"x" //"#` closed the literal at the second `"`, so the `//` that followed
began a line comment and **the rest of that line was deleted** — including a
real `"docker"` literal after it. `every_declared_effect_denial_names_a_real_path`'s
"docker invocation helpers" block asserts that the set of files naming a
container runtime is exactly a table of four; a fifth file whose literal was
erased is *absent from the computed set*, the sets compare equal, and the
census is **green with an extra Docker-naming file present**. A missed needle
is a false negative, and a false negative in a set comparison is fail-open,
not loud. The reviewer built that mutation and measured it.

So the residual is now the same as its sibling's: an unterminated literal
runs to end of input, which is a file that does not compile.

## `pub fn blank_comments(source: &str) -> String` › `while i < bytes.len() && bytes[i] != b'\n' {`

The newline itself is left for the outer loop, so line numbers
survive.

## `pub fn blank_comments(source: &str) -> String` › `match literal_end(bytes, i) {`

`r"…"`, `r#"…"#`, `b"…"`, `br#"…"#` — and an identifier that
merely begins with one of these letters, which is why the
preceding byte is checked and why a non-literal falls through
to a single push.

## `pub fn blank_comments(source: &str) -> String` › `match char_literal_end(bytes, i) {`

`'"'` is the one that matters here: without this arm it opens
a string. `char_literal_end` decides, so this and its
sibling cannot drift apart.

## `fn char_literal_end(bytes: &[u8], from: usize) -> Option<usize> {`

Where the char literal starting at `from` ends, exclusive, or `None` when
`from` does not start one.

A char literal is `'`, then either an escape (`\n`, `\\`, `\'`, `\u{1F600}`)
or **one UTF-8 scalar**, then `'`. The scalar is one to four bytes, and that
is the whole reason this is a scan rather than a lookahead.

#### The desync a fixed lookahead produces, and how far it reaches

Both blankers used to answer the question with two bytes: `'` is a char
literal when the byte at `+2` is a quote. `'é'` closes at `+3`, so it was
classified as **not** a literal, scanning resumed on its closing quote, and
that quote was then read as an *opening* one. From there the tokeniser is out
of phase: in `('é','{')` the pairing shifts by one and the `{` that is inside
a char literal survives into the blanked text as visible **code**.

One unbalanced brace is enough to take a file out of every census that
consults `production_code`. `matching` counts it, so
`configured_item_end`'s brace arm walks past the item's real `}`, finds no
balancing brace and gives up — and giving up used to mean "blank to end of
file".

Measured end to end, twice. On `src/agent/claude.rs`, with the pair inside
that file's `#[cfg(test)] mod tests` and a forged item appended below it, the
region measured **8525** non-whitespace bytes with the attack and 8525
without — a zero-byte delta no floor can see — and every source census was
green. Then gate-clean, because the first form is not: `cargo fmt` rewrites
`('é','{')` to `('é', '{')` and the space defuses it, and
`clippy::items_after_test_module` refuses an item placed below a file's own
`mod tests`. Both are avoidable. `stringify! { ('é','{') }` is left alone by
rustfmt (macro bodies in braces are), and a `#[cfg(test)]` module not named
`tests` is not what that lint looks for. With the probe inside
`src/runner/container/view.rs`'s `#[cfg(test)] pub(crate) mod fixtures` and a
forged `RunnerRequest {` builder above the file's real test module,
`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` both exit
0 and `runner::tests::every_production_runner_request_is_built_by_its_roles_\
builder` passes — while the identical forged builder **without** the probe
fails it by name.

The preconditions are already in this tree: `src/status.rs`, `src/util.rs`
(twice on one line) and `src/engine/tests.rs` all hold non-ASCII char
literals. Only the adjacency was missing.

`'a` is a lifetime and is not a char literal; nor is `'_`, nor the `'static`
in `&'static str`. All three are refused by one rule — the byte after the
scalar is not a quote — rather than by a list.

## `fn char_literal_end(bytes: &[u8], from: usize) -> Option<usize> {` › `at += 2;`

An escape. The longest Rust spells is `\u{10FFFF}`, which closes at
`from + 11`, so the window is bounded and a runaway scan over the rest
of the file cannot happen.

## `fn char_literal_end(bytes: &[u8], from: usize) -> Option<usize> {` › `let width = match *bytes.get(at)? {`

One UTF-8 scalar, whose width its lead byte states. A continuation or an
otherwise invalid lead cannot begin one, and `source` is a `&str`, so the
remaining ranges are unreachable rather than merely unhandled.

## `fn literal_end(bytes: &[u8], from: usize) -> Option<usize> {`

Where the string literal starting at `from` ends, or `None` when `from` does
not start one.

Accepts `"…"`, `b"…"`, `r"…"`, `r#"…"#` and `br##"…"##`. An unterminated
literal ends at end of input — a file that does not compile.

## `pub fn blank_comments_and_strings(source: &str) -> String` › `for (index, byte) in bytes.iter().enumerate() {`

Newlines survive so line numbers do.

## `pub fn blank_comments_and_strings(source: &str) -> String` › `let mut j = i;`

`r"…"`, `r#"…"#`, `b"…"`, `br#"…"#`

## `pub fn blank_comments_and_strings(source: &str) -> String` › `match char_literal_end(bytes, i) {`

`char_literal_end` decides, so this and its sibling in
`blank_comments` cannot drift apart.

## `pub fn production_region(source: &str) -> String {`

The production region: everything before the first `#[cfg(test)]` that is not
inside a comment or a string.

## `pub fn production_code(source: &str) -> String {`

The production **code** of `source`: comments and string literals blanked,
and every `#[cfg(test)]`-configured item removed.

`production_region` answers a different question and keeps its answer: it
*truncates* at the first `#[cfg(test)]`, which is what a **domain** question
wants (everything above the cut is certainly production) and what a
**prohibition** question must not have. Three failures a prohibition census
pays for with a truncating region, all three measured on this tree:

* A file that declares its tests as `#[cfg(test)] mod tests;` — the
  `tests.rs` entries of `effects::tests::cfg::WHOLE_FILE_TEST_MODULES` —
  puts every line **below** that declaration outside the region.
  The declaration is usually the last item, so the hole is normally empty;
  appending to the file fills it. Legal Rust, no comment trick, and it
  defeated the barrier census, the process-start census and the container
  token census at once.
* A `#[cfg(test)]` inside a block comment or a string literal truncates a
  region that a `//`-only strip cannot see. `PR4-CENSUS-COMMENT-ORACLE`,
  in the shape a `//`-only strip does not close.
* Counting over unblanked text counts prose. `src/agent/proc.rs` names
  `run_with_timeout` eight times, five in code and three in doc comments, so
  a real ninth entry point could be paid for by deleting two sentences.

So this returns the **whole file**, blanked, with each `#[cfg(test)]` item
blanked out in place. Newlines survive, so a byte offset still maps to the
line it came from.

The item's extent is found by delimiter matching over the blanked text — a
brace body ends at its matching `}` (and takes a trailing `;` with it, for
`use a::{b, c};`), anything else ends at the first `;` or `,` outside a
nested delimiter, except that a recognized function return type keeps its
commas until the function body or semicolon. A closing delimiter that would
leave the enclosing block ends the item too. Angle brackets are not matched: a
`#[cfg(test)] field: BTreeMap<K, V>,` ends at the comma inside the generics
and leaves `V>,` behind. That is the safe direction — a region that is too
**large** can only make a census match more, never less.

## `pub fn production_code(source: &str) -> String` › `while let Some(at) = bytes`

Searched over bytes rather than `str::find`, because a cut offset is not
guaranteed to be a char boundary and slicing one panics.

## `pub fn production_code(source: &str) -> String` › `let mut start = at + ATTR.len();`

Any further attributes stacked on the same item belong to it.

## `fn configured_item_end(bytes: &[u8], start: usize) -> usize {`

Where the item beginning at `start` ends, exclusive. See `production_code`.

**The two give-up paths return `start`, not `bytes.len()`.** Both are reached
only when the blanked text does not parse — an unbalanced brace, or an item
with no terminator before end of file — and neither is reachable from this
tree today (measured: zero occurrences over all 92 source files). What
decides the value is the *direction* they fail in. `bytes.len()` reads "the
item is the rest of the file" and blanks it, so a tokeniser that has lost
phase silently removes every production item below the attribute from every
census that consults this region — which is exactly what
`char_literal_end`'s desync used to buy. Returning `start` blanks the
attribute and nothing else, so the test module below it reads as production
and the censuses go **loud** instead. The larger region is always the safe
one here, for the same reason the doc above gives for not matching angle
brackets: it can only make a census match more, never less.

## `pub fn governed_allows(source: &str) -> Vec<GovernedAllow> {`

Every `allow`/`expect` of a governed lint in `source`, with where it sits.

Attributes are found in the blanked text and read out of the original, so a
fixture quoted in a doc comment is invisible and a real attribute is not.

## `fn matching(bytes: &[u8], open: usize, opener: u8, closer: u8) -> Option<usize> {`

The index of the bracket closing the one at `open`, or `None`.

## `fn is_module_level(blanked: &str, hash: usize, close: usize, inner: bool) -> bool {`

An inner attribute in the file's prologue, or an outer attribute on a `mod`.

"Module-level" is the whole of the placement rule, so it is decided here
rather than by eye: an `#![allow(…)]` before the first item governs the file
module; a `#[allow(…)] mod inner { … }` governs that module; an attribute on
a function, a statement or an expression governs neither and is what the rule
exists to refuse.

## `fn is_module_level(blanked: &str, hash: usize, close: usize, inner: bool) -> bool {` › `let mut prefix = &blanked[..hash];`

Nothing but whitespace and other attributes may precede it.

## `fn is_module_level(blanked: &str, hash: usize, close: usize, inner: bool) -> bool {` › `let mut rest = &blanked[close + 1..];`

Outer: skip further attributes and whitespace, then require `mod`.

## `pub const FROZEN_LEGACY_ALLOWLIST: &[&str] = &[`

---------------------------------------------------------------------------
(2) The frozen legacy section
---------------------------------------------------------------------------

## `pub const FROZEN_LEGACY_ALLOWLIST: &[&str] = &[`

The legacy section of `effects/allowlist.toml` as PR5 freezes it.

> "the legacy section may only shrink after PR5 (the test compares against
> the frozen list) and never contains a topology module"

Held here rather than only in the TOML because the TOML is the thing under
test: a frozen list that lived in the file it freezes would agree with any
edit to that file.

## `pub const TOPOLOGY_MODULES: &[&str] = &[`

The modules the legacy section may never contain, verbatim from `mechanism`.

> "never contains a topology module (src/topology/**, src/runner/**,
> src/workspace_manager.rs, src/engine/topology.rs)"

The ban is on the **legacy** section alone, which is why
`src/runner/{host,container,invocation}.rs` and `src/workspace_manager.rs`
are in the funnel section without contradiction — the same sentence lists
them there.

`src/engine/topology/` is the fifth entry and is **not** in the packet
sentence, which names `src/engine/topology.rs` only. It is here because
`topology_modules_among` matches with `str::starts_with`, and
`"src/engine/topology/create.rs"` does not start with
`"src/engine/topology.rs"` — the sentence's four shapes were written when
the schema-4 engine was one file, and PR7 makes it a directory. Without
this entry the ban silently stops covering every submodule of the module it
exists to cover. Widening a ban is not a relaxation of the packet, and
`the_legacy_section_never_contains_a_topology_module` executes the gap: it
asserts the four-entry list misses a submodule that the five-entry list
catches.

The `src/workspace_manager/` entry is **that paragraph again**, for the same
reason and with the same evidence. The sentence names
`src/workspace_manager.rs` — a file — and `"src/workspace_manager/residue.rs"`
does not start with it. That cost nothing while the directory held only
`fixture.rs` and `tests.rs`, both `#[cfg(test)]`; the `m4-workspace` split
puts eight **production** modules there, and without this entry the ban would
silently stop covering the funnel's own production code in the very commit
that created it. Restoring coverage a split removed is neutrality rather than
a widening of the packet, and the gap is executed below just as the
`src/engine/topology/` one is.

**What is reachable through the hole, stated rather than dressed up.** The
ban is on the legacy section alone, and
`the_legacy_section_is_frozen_and_may_only_shrink` pins that section by
length *and* by exact set equality, so it cannot grow at all. Reaching this
hole therefore means first editing a PR5-frozen production constant. It is
lost defence-in-depth, not a live escape — and it is closed here because
`m4-workspace` is the only split that opens it: `src/topology/` and
`src/runner/` are already prefixes, and `src/rundir.rs` and
`src/agent/proc.rs` are not in this list at all.

**Why this list takes a directory prefix and `CLASSIFIED_MODULES` does
not**, since one commit does both and the two answers look contradictory.
They are matched differently on purpose. Entries here are matched with
`str::starts_with`, so a prefix is the only form that covers a module tree,
and a ban that covers more is strictly better. Entries there are joined onto
the manifest root and **read as source files** by
`reachable_fns_are_classified`, so a directory would name nothing at all;
that list is a roll-call whose whole point is per-module review, which is why
its children are enrolled one path each (`C-002`).

## `pub fn legacy_growth<'a>(frozen: &[&str], current: &[&'a str]) -> Vec<&'a str> {`

Entries of `current` that the frozen list does not contain — i.e. growth.

A pure function over its inputs precisely so the refusal can be *executed*
against a list that does grow, rather than inferred from one that does not.

## `pub fn topology_modules_among<'a>(paths: &[&'a str]) -> Vec<&'a str> {`

Entries of `paths` that name a topology module.

## `pub const CLASSIFIED_MODULES: &[&str] = &[`

---------------------------------------------------------------------------
(3) Wrapper classification
---------------------------------------------------------------------------

## `pub const CLASSIFIED_MODULES: &[&str] = &[`

The modules whose externally reachable `fn`s `mechanism` (3) classifies.

> "at PR5 every pubfn of a legacy or shared module is classified effectful or
> effect-free by review"

**Legacy** is the frozen legacy section. **Shared** is the modules the slice
`scope` names — "shared primitives (locks, run-dir creation and marker,
answer staging/ingestion, util JSON write, the exact-snapshot primitive incl.
its ephemeral commit, the event-log writer) moved behind funnels with Shared
sites" — plus the process funnel, whose `Shared` sites PR4 landed.

`src/topology/effects.rs` and `src/effects.rs` are deliberately outside the
domain: both are in the allowlist's funnel section, and neither is legacy nor
shared. Between them they declare 208 + n functions that touch nothing, and
classifying them would bury the rows that matter.

## `"src/workspace_manager.rs",`

shared

## `"src/rundir/classify.rs",`

The five production children the `m3-rundir` split gave `src/rundir.rs`.
They are named **one path each**, which is the only form this list has:
`reachable_fns_are_classified` joins every entry onto the manifest root
and reads it as a source file, so a directory prefix would name nothing
and the entry above cannot be widened into one. `C-002` is the standing
finding that this roll-call is hand-maintained rather than derived, and it
is not this split's to repair. (`TOPOLOGY_MODULES` above is the list that
*does* match with `starts_with`, and it does not name `src/rundir.rs` at
all -- neither the ban it serves nor the prefix question reaches here.)

They are here because the split moved seventeen externally reachable
`fn`s out of `src/rundir.rs`, and a name that leaves the domain is a name
nobody has to classify any more. Listing the children keeps the whole of
the run-directory subsystem inside `mechanism` (3)'s classification with
the same names accounted for on the other side of the move; the funnels,
and every effect site, stayed in the parent.

## `"src/runner/host/environment.rs",`

The three production children the `m5-host` split gave
`src/runner/host.rs`: `environment.rs`, `naming.rs` and `probe.rs`, named
**one path each**. That is the only form this list has:
`reachable_fns_are_classified` joins every entry onto the manifest root
and reads it as a source file, so a directory prefix would name nothing,
and `"src/runner/host.rs"` is therefore left as an exact path rather than
widened into `"src/runner/host"`. `C-002` is the standing finding that
this roll-call is hand-maintained rather than derived, and it is not this
split's to repair. (`TOPOLOGY_MODULES` is the list that *does* match with
`starts_with`, and it already names `src/runner/`, so neither the ban it
serves nor the prefix question reaches here.)

They are named because the split moved ten externally reachable `fn`s out
of `src/runner/host.rs`, and a name that leaves the domain is a name
nobody has to classify any more. Naming the children keeps the whole of
the host boundary inside `mechanism` (3)'s classification with the same
names accounted for on the other side of the move; the funnel, both
`ProcessSite` values, the `Contained` mint and the reserved-key
vocabulary stayed in `src/runner/host.rs`.

## `"src/runner/container.rs",`

The third of `mechanism` (2)'s `src/runner/{host,container,invocation}.rs`,
added by PR6. It is here rather than only in the allowlist because it
denies six of its own paths — the "docker invocation helpers" the same
sentence enumerates — and `every_effectful_wrapper_is_on_the_disallowed_list`
requires a `upstroke::` denial to be a row somebody classified.

## `"src/runner/container/view.rs",`

The body of the Container funnel's R19 view, added by PR7's census
repair. It carries `#![allow(clippy::disallowed_methods)]` over its
production region and was the **only** non-test production module in the
tree in that position and absent from this list. The consequence was not
theoretical: with no row here, none of its `pub fn` needed classifying,
so `every_effectful_wrapper_is_on_the_disallowed_list` could never force
one onto the denylist — a module that may reach `fs` under its own allow,
and whose reachable surface nobody had to account for.

## `"src/engine/coordinator.rs",`

legacy

## `"src/agent/proc/ambient.rs",`

The three production children the `m6-proc` split gave
`src/agent/proc.rs`: `hooks.rs` (the observation and injection surface),
`ambient.rs` (the ambient Job Object and the reclaim scope) and `drain.rs`
(the pipe reader), named **one path each**. That is the only form this
list has: `reachable_fns_are_classified` joins every entry onto the
manifest root and reads it as a source file, so a directory prefix would
name nothing, and `"src/agent/proc.rs"` is therefore left as an exact path
rather than widened into `"src/agent/proc"`. `C-002` is the standing
finding that this roll-call is hand-maintained rather than derived, and it
is not this split's to repair.

They are named because the split moved externally reachable `fn`s out of
`src/agent/proc.rs` and made previously private ones `pub(super)` in the
children -- the same visibility a private item of `proc` had, and the
visibility `externally_reachable_fns` counts. Naming the children keeps
every one of those names accounted for on the other side of the move.
Stated as a property and not as a tally: this list merges as a sorted
union and a count beside it does not, so a number here would be wrong at
the next edit to any of these files rather than at a merge anyone reads.
The funnel entry point, both `ProcessSite` values, the no-degraded-mode
memo, `windows_job` and `termination` stayed in `src/agent/proc.rs`.

## `pub fn externally_reachable_fns(source: &str) -> Vec<String> {`

Every `fn` of `source`'s production region that is reachable from outside its
module.

Three shapes, because "pubfn" in the packet's sentence has three of them in
this tree and a classification that saw one would be complete against a
domain nobody drew:

* `pub fn` / `pub(crate) fn` / `pub(super) fn` items, free or in an inherent
  `impl`;
* every `fn` inside an `impl <Trait> for <Type>` block, which is reachable
  through the trait whatever its own visibility says;
* associated `fn`s of a public trait's default bodies, which are the same
  case.

Names are returned once each, sorted. Two `impl` blocks with a `new` apiece
are one row: the classification is of a *name in a module*, and a name that
is effectful in one impl is a name the denylist has to carry anyway.

**The third shape was documented and not implemented until repair round F1**
(`PR6-REACHABLE-FN-PARSER-MISSES-TRAIT-DEFAULTS`, refiled as
`PR6-LANEF-007`). The predicate was `visible || in_trait_impl`, and a default
body inside a `pub trait` declaration is neither: it carries no visibility of
its own and it is not in an `impl … for …` block. Lane F filed it as narrow
because no such body reached an effect; the reviewer **built one** —
`fn remove_without_a_site(&self, path: &Path) { let _ = fs::remove_file(path); }`
as a default method on the public `ContainerHooks` — and clippy, all 79
effects tests and all 38 container tests passed. A default body is the one
place in this tree where an effect could be added to a *classified* module
without appearing in its classification.

A trait method **declaration** (no body) is deliberately still excluded: it
performs nothing, and every implementation of it is reached by the
`impl … for …` shape above.

## `pub fn externally_reachable_fns(source: &str) -> Vec<String>` › `let mut t = 0;`

`pub trait X: Y { … }` — the bodies inside are reachable through the
trait, exactly as a trait impl's are.

## `pub fn externally_reachable_fns(source: &str) -> Vec<String>` › `let mut i = 0;`

`impl <something> for <something> {` — the `for` is what makes it a trait
impl; an inherent `impl Type {` has none before the brace.

## `pub fn externally_reachable_fns(source: &str) -> Vec<String>` › `let is_default_body = public_trait_spans`

A default body in a public trait, and only a default *body*:
`find_header_brace` answers `None` at the `;` of a declaration.

## `fn declares_visibility(prefix: &str) -> bool {`

Whether the text immediately before a `fn` declares it visible outside its
module — with the `pub const fn` / `pub unsafe fn` / `pub async fn`
modifiers stripped first.

**One copy, deliberately.** This was written twice — once for the bare case
and once inside the modifier-stripping fallback — and a mutation that broke
the `pub(crate)` arm of the first copy left the whole suite green, because
the second copy still caught it. Two hand-maintained lists of three strings
disagree eventually, and the one that disagreed silently would be this one.
Measured, mutation `the-parser-misses-pub-crate`.

## `fn find_header_brace(region: &str, from: usize) -> Option<usize> {`

The `{` that opens an `impl` block's body, skipping generics and where-clauses.

## `pub struct DenialFixture {`

---------------------------------------------------------------------------
The four build-failure refusals whose reason must be pinned
---------------------------------------------------------------------------

## `pub struct DenialFixture {`

One shape `mechanism` (1) claims rustc resolution defeats, as a fixture.

`proof_tests[4]`: "injected renamed-import / re-export / function-value /
legacy-wrapper call fixtures fail the build". A fixture asserting "this does
not build" is green whether it failed for the intended reason or a typo, so
each row carries the lint it must emit **and** the resolved path clippy must
name — and the harness runs a control that must compile first.

## `pub struct DenialFixture` › `pub shape: &'static str,`

What the shape is called in `proof_tests[4]`.

## `pub struct DenialFixture` › `pub source: &'static str,`

The fixture body, compiled as its own crate against this crate's rlib.

## `pub struct DenialFixture` › `pub lint: &'static str,`

The lint the fixture must emit, and nothing else.

## `pub struct DenialFixture` › `pub resolves_to: &'static str,`

The path clippy's message must name — the *resolved* one, which is the
whole claim: a renamed import reports as `std::fs::write`, not as `w`.

## `pub const DENIAL_FIXTURES: &[DenialFixture] = &[`

The fixture set. One row per shape `proof_tests[4]` names, plus the two the
mechanism sentence names that the proof test does not (a method call and a
macro), because "aliases, re-exports, function values, method calls, and
macro-expanded code" is five shapes and a grid short of its domain is the
class this project has recorded four times.

## `pub const DENIAL_CONTROL: &str = "pub fn go(p: &std::path::Path) -> bool {\n\`

A fixture that must compile clean, so a mis-wired invocation cannot make
every refusal above "pass".

`PR5-C-DOCTEST-FIXTURES-NEVER-RAN` is in the standing ledger because three
build-refusal fixtures were green having never executed. The control is the
difference between "the compiler refused this" and "the compiler could not
find a crate to refuse it against".

## `pub(crate) mod census_domain {`

-- test-only declarations ----------------------------------------------
At the BOTTOM, and a `mod` rather than a bare `fn`: `production_region` cuts a
file at its first `#[cfg(test)]`, and
`effects::tests::every_production_region_that_stops_early_stops_at_a_module`
pins by name the ten files whose cut lands on something that is not a module.
This file is not one of them and must not become one.

## `pub(crate) mod census_domain {`

The **domain** every whole-tree census draws, derived once.

`PR5D-VISIBILITY-CHECK-DUPLICATED`: a value two places both maintain by hand
disagree eventually, and the one that disagrees silently is the one that
decides what a census is allowed to see. This derivation was written twice —
`runner::tests::whole_file_test_module_declarations` and
`events::log::tests::declared_whole_file_test_modules`, identical by hand,
each deciding which files four whole-tree censuses skip. It lives beside
`production_code` now, which is the region those same censuses count over.

## `pub(crate) mod census_domain` › `pub(crate) fn production_calls(code: &str, name: &str, form: Call) -> usize {`

Calls to `name` in `code`: neither its definition, nor a longer identifier
that merely ends in it.

The second half is the one that was missing. A needle built as
`format!("{name}(")` is a plain substring search, so `expected_refs(` is
satisfied by every `refuse_unexpected_refs(` in the tree — and a census whose
entry is proved by a different function's call sites proves nothing about its
own. Measured on this tree: `workspace_manager.rs` carries four occurrences of
the substring `expected_refs(` and **zero** calls to `expected_refs` — one of
the four survives into `production_code`'s region, and it is the *definition
line* of `refuse_unexpected_refs`, which the "calls, not definitions" filter
does not see because the text before the match is `pub fn refuse_un`.

The boundary is "the byte before the match is not an identifier byte", which
keeps `crate::a::b::expected_refs(` — `:` is not one — and rejects
`unexpected_refs(`. Not a rename, which is how
`the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle`
closed the same class: that census's needle is a constant it could choose,
this one's is eleven names the packet chose.

## `pub(crate) fn production_calls(code: &str, name: &str, form: Call) -> usize {` › `.filter(|(at, _)| {`

Not the tail of a longer identifier.

## `pub(crate) fn production_calls(code: &str, name: &str, form: Call) -> usize {` › `.filter(|(at, _)| !code[..*at].trim_end().ends_with("fn"))`

Calls, not definitions.

## `pub(crate) fn production_calls(code: &str, name: &str, form: Call) -> usize {` › `.filter(|(at, _)| {`

And the form the clause is written in, which is what tells three
items of one name apart.

## `pub(crate) mod census_domain` › `pub(crate) enum Call {`

How a clause's function is **called** in production.

Not decoration. `settle_interrupted` names three unrelated production items
in this tree — `recover::settle_interrupted` (the `T-ATTEMPT` clause, a free
function), `AttemptContext::settle_interrupted` and
`events::RunState::settle_interrupted` (both methods, both called) — and a
census counting the bare name is satisfied by either of the other two.
Measured by S5 round 4: deleting step (d)'s only production call left the
census **and the entire suite** green, with `attempt_interrupted` appended
by no run. `reviews/FINDINGS.md` §4's "a refutation must name which item it
inspected" is the same rule; this is it applied to the instrument.

## `pub(crate) enum Call` › `Free,`

`name(…)` or `path::name(…)` — never `receiver.name(…)`.

## `pub(crate) enum Call` › `Method,`

`receiver.name(…)`.

## `pub(crate) mod census_domain` › `pub(crate) fn whole_file_test_modules(`

The **files** `declared_whole_file_test_modules` resolves to, as a set a
census can test membership in.

The resolution loop — assert exactly one of the two candidates exists,
collect it — was written out at each caller, and a third caller wrote a
different rule instead: `path.file_stem() == "tests"`. That covers the
files named `tests.rs` — the entries of
`effects::tests::cfg::WHOLE_FILE_TEST_MODULES` whose file stem is
`tests` — and **not** the six that are not: `scaffold`, `premove`,
`fake`, `fixture`, `scratch_tree` and `readiness`. The whole set, that
subset and the difference
between them are all read off that one list; the four the rule misses
are the ones a census is
most likely to trip over, because a scaffold, a fake and a readiness
protocol exist to *name* the things production names. Found by S5 round
5's `seams`, `attempt` and `settle` lenses independently; the
consolidation had been filed one commit earlier in
`reviews/FINDINGS.md` §20 as tidiness.

### Panics

When a declaration resolves to no file or to both candidates — a skip
path naming no file is a skip that has stopped meaning anything — when
two declarations resolve to one file, when the declaration graph is
cyclic, or when fewer than `floor` declarations are derived, which is
the control against a derivation that has silently stopped finding
anything.

## `pub(crate) mod census_domain` › `assert!(`

**The declaration graph is a forest.** Directory-derived candidates
descend, so a cycle is not reachable from this tree — which is the
reason to check rather than a reason not to: an unreachable path is
one nobody would notice becoming reachable. A `#[path]` attribute is
the one construct that could build one, and the scanner refuses those
rather than resolving them, so this assertion and that refusal are
one control with two halves.

## `pub(crate) mod census_domain` › `assert!(`

**The control that binds every caller**, and it belongs here rather
than at each of them.
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
asserts what this *returns*; it says nothing about whether a census
calls it, which is the defect `3a91626` repaired for two censuses and
this witness then reproduced one commit later (`R6-SETTLE-003`). A
caller cannot reach the set without passing through this line.

## `pub(crate) mod census_domain` › `pub(crate) fn declaration_cycle(edges: &[(PathBuf, PathBuf)]) -> Option<Vec<PathBuf>> {`

A cycle in `edges`, as the path that closes it, or `None`.

`edges` is (declaring file, declared file). The derivation treats that
relation as a forest — every guard is read from the file *above* — so a
cycle means the traversal would either not terminate or attribute a
guard to a file that does not inherit it.

Pure and separately driven, because the real tree cannot produce one: a
census control that is only ever exercised on input that satisfies it is
a control nobody has seen refuse anything.

## `pub(crate) fn declaration_cycle(edges: &[(PathBuf, PathBuf)]) -> Option<Vec<PathBuf>> {` › `enum Colour {`

Depth-first search state. `Grey` is "on the path being walked", and
reaching a `Grey` node is what a back edge *is*.

## `pub(crate) fn declaration_cycle(edges: &[(PathBuf, PathBuf)]) -> Option<Vec<PathBuf>> {` › `let mut adjacency: BTreeMap<&PathBuf, Vec<&PathBuf>> = BTreeMap::new();`

**The full adjacency, not the first edge out of each node.** The
first version followed `edges.iter().find(…)`, which walks one
outgoing edge per node — so a node with two children whose *second*
child closes the loop reported no cycle. `a -> b`, `a -> c`,
`c -> a` was the shape, and it read as acyclic.

## `pub(crate) fn declaration_cycle(edges: &[(PathBuf, PathBuf)]) -> Option<Vec<PathBuf>> {` › `let mut stack: Vec<(&PathBuf, usize)> = vec![(start, 0)];`

(node, how many of its outgoing edges have been taken). The stack
IS the current path, which is what makes the cycle reportable.

## `pub(crate) mod census_domain` › `pub(crate) fn sole_present<'a>(`

The one of `candidates` that `exists` accepts, or how many it accepted.

Zero is a declaration naming no file, which is a skip that has stopped
meaning anything. Two is `name.rs` and `name/mod.rs` both present, which
Rust itself refuses to compile and which a resolver that took the first
match would silently pick a side in. Both are refusals.

`exists` is a parameter rather than a `Path::is_file` call so the two
refusals can be driven: neither is reachable from this tree, and a
control that has only ever seen compliant input is a control nobody has
watched refuse anything. It is also what keeps this body free of an
effect — the funnel section of the allowlist records `allows = []` for
this file and that claim is stronger than any other entry there.

## `pub(crate) mod census_domain` › `pub(crate) enum CandidateRefusal {`

Why a declaration's candidate files cannot be named.

## `pub(crate) enum CandidateRefusal` › `OutsideThePackage {`

The declaring file is not inside the package the inventory was read
for, so that inventory does not say whether it is a crate root.

## `pub(crate) mod census_domain` › `pub(crate) enum InventoryRefusal {`

Why a package's target inventory could not be established.

Every variant is a **refusal to guess**. The resolution below turns on
which files Cargo compiles as crate roots, that is a fact only the
manifest holds, and the previous derivation held it as a rule about file
stems instead. A rule cannot be wrong quietly the way a stem test can:
when the authority is unavailable the census stops.

## `pub(crate) enum InventoryRefusal` › `NotRun {`

`cargo metadata` could not be started at all.

## `pub(crate) enum InventoryRefusal` › `Failed {`

It ran and exited non-zero.

## `pub(crate) enum InventoryRefusal` › `Unreadable {`

Its output is not the JSON document this reads.

## `pub(crate) enum InventoryRefusal` › `NoPackage {`

No package in the document has that manifest path.

## `pub(crate) enum InventoryRefusal` › `NoTargets {`

The package has no targets, so nothing is a crate root.

## `pub(crate) mod census_domain` › `pub(crate) struct CrateRoots {`

**The files Cargo compiles as crate roots**, read from the manifest via
`cargo metadata` rather than inferred from their names.

A crate root owns its own directory; every other file owns a directory
named after it. Which files are roots is a property of the *manifest*,
and the previous derivation decided it from the file's stem: `lib.rs` or
`main.rs` at the source root was a root, the same stem anywhere else was
refused, and anything else was an ordinary module. Both halves are wrong
against a manifest that says otherwise, and the second half is wrong
**silently**:

* `[[bin]] path = "src/tools/odd.rs"` is a crate root with an arbitrary
  name. The stem rule reads it as the ordinary module `tools::odd`, so a
  `mod helper;` inside it resolves to `src/tools/odd/helper.rs` when
  Cargo compiles `src/tools/helper.rs`. That is a **different file** —
  the same competing-sibling hazard the nested-`lib.rs` refusal was
  written for, arriving through the door that refusal left open, and it
  does not announce itself: with no `src/tools/odd/helper.rs` present the
  wrong reading resolves rather than refusing.
* `examples/probe.rs` is this tree's live instance. It is an `example`
  target — a crate root — and `effects::tests::scanned_sources` walks
  `examples/**`, so the stem rule already answers `examples/probe` for a
  directory Cargo calls `examples`.
* A nested `src/a/lib.rs` the manifest never names is the ordinary module
  `a::lib`, which is decidable rather than ambiguous once the manifest is
  read. The old refusal was the honest answer to not knowing; this is the
  answer.

Kinds are **not** filtered. `lib`, `bin`, `example`, `test`, `bench` and
`custom-build` are each a crate root of their own, and a census that
looked only at `lib`/`bin` would re-introduce the same class one kind at
a time.

## `impl CrateRoots` › `pub(crate) fn from_metadata_json(`

The inventory in one `cargo metadata --format-version 1` document,
for the package whose manifest is `manifest`.

Pure over the document, which is what makes every refusal below
drivable: the acquisition is a process start and lives in
[`crate::effects::tests`](effects/tests.md), where this crate's governance puts one.

## `impl CrateRoots` › `pub(crate) fn package_dir(&self) -> &std::path::Path {`

The directory the package's manifest sits in.

## `impl CrateRoots` › `pub(crate) fn roots(&self) -> impl Iterator<Item = &std::path::Path> {`

Every crate root, absolute, in sorted order.

## `impl CrateRoots` › `pub(crate) fn is_root(&self, path: &std::path::Path) -> bool {`

Whether `path` is one of them.

## `impl CrateRoots` › `pub(crate) fn is_root_relative(&self, relative: &str) -> bool {`

Whether the package-relative, `/`-separated `relative` is one.

The second caller reads the tree as repo-relative slash strings
rather than as paths, and one authority answering both is the point:
`PR5D-VISIBILITY-CHECK-DUPLICATED` is the standing entry for the rule
that got written twice, and the stem test *was* written twice — here
and in `effects::tests::cfg::module_dir`.

## `pub(crate) mod census_domain` › `pub(crate) fn module_directory(`

The directory an out-of-line child of `declared_in` lives in.

**A crate root owns its directory; an ordinary module owns a directory
named after it.** `mod.rs` is the first case wherever it sits — that is
what `mod.rs` means. Everything else is the first case exactly when the
manifest names it as a target's path, which is what `CrateRoots` reads
and what no rule about file names can answer.

Refused rather than decided when `declared_in` is not inside the package
the inventory was read for: an inventory is a statement about one
package, and a file outside it is one the inventory is silent on.

## `pub(crate) mod census_domain` › `pub(crate) fn candidates_for(`

The two files `mod <name>;` can name, given where it was written.

`declared_in` is the declaring file; `inline_path` is the inline modules
enclosing the declaration, outermost first. **The inline path is part of
the directory**, which is the half a resolver reading only the file name
gets wrong: `mod readiness;` inside `proc.rs`'s inline `test_support`
names `proc/test_support/readiness.rs`, and flattened to
`proc/readiness.rs` it names nothing — a zero-candidate refusal if you
are lucky and the wrong file if you are not.

`roots` is the package's target inventory, and `module_directory` is
why that is a parameter rather than a test on the file's stem.

## `pub(crate) mod census_domain` › `pub(crate) fn contained_in(base: &std::path::Path, candidate: &std::path::Path) -> bool {`

Whether `candidate` stays inside `base` through plain path components.

A module name is an identifier and a candidate is `base` joined with
identifiers, so this holds by construction — and is asserted anyway,
because the construction is what a `#[path = "../.."]` attribute would
change, and the failure it would cause is a census reading a file
outside the tree as declared inside it.

## `pub(crate) mod census_domain` › `pub(crate) struct TestModuleDeclaration {`

One out-of-line `mod <name>;` the crate declares as test-only.

## `pub(crate) struct TestModuleDeclaration` › `pub(crate) declared_in: PathBuf,`

The file the declaration is written in.

## `pub(crate) struct TestModuleDeclaration` › `pub(crate) name: String,`

The declared module's name.

## `pub(crate) struct TestModuleDeclaration` › `pub(crate) inline_path: Vec<String>,`

The **inline** modules enclosing the declaration, outermost first.
Empty when the declaration sits at the file's top level.

## `pub(crate) struct TestModuleDeclaration` › `pub(crate) guard: String,`

The effective `cfg` predicate, rendered — the conjunction of every
enclosing inline module's predicate and the declaration's own.

## `pub(crate) struct TestModuleDeclaration` › `pub(crate) candidates: [PathBuf; 2],`

`[<dir>/<name>.rs, <dir>/<name>/mod.rs]`, where `<dir>` is the
declaring file's module directory joined with `Self::inline_path`.

## `impl TestModuleDeclaration` › `fn render_guard(&self) -> String {`

The guard and the inline path it was read through, for a diagnostic.

## `pub(crate) mod census_domain` › `pub(crate) fn declared_whole_file_test_modules(`

Every out-of-line module declaration the crate compiles **only** under
`cfg(test)`, structurally resolved.

Such a file is test code end to end. A region function has nothing to
remove in one, so it would count the whole of it as production — a
fixture that names a census's needle would then read as a production
offender. The set is read out of the declarations rather than listed by
hand: it was `src/engine/tests.rs` alone until PR5 moved the Event funnel
into `src/events/log.rs` with two test modules of its own, and the census
failed on the first file the hand-maintained list did not know about.

**Read out of the blanked source, and every candidate returned rather
than assumed.** The split used to be over the raw text, so a `//` line
containing `#[cfg(test)] mod policy;` derived a skip for
`src/runner/policy.rs` and removed that file from every census below —
measured, with a `git push` planted in it that the census then did not
see. Over the whole tree the raw split derived 50 skip paths of which
**34 named no file at all**, and a skip path naming no file is a skip
that has stopped meaning anything, so `whole_file_test_modules` asserts
that exactly one of the two candidates exists.

### Structure, not a literal `#[cfg(test)] mod name;`

The predicate used to be exactly that string, and it had two holes a
**text** rule cannot close and a structural one closes together:

* **A visibility qualifier hid the declaration.** `#[cfg(test)]
  pub(crate) mod helpers;` was not matched, because the rule read `mod `
  immediately after the attribute. That direction was chosen as the safe
  one — failing to derive a skip leaves a test file in a census's domain,
  where a fixture reads as an offender and someone looks — and it is
  still the safe direction. It stopped being *necessary*: the scan below
  reads the item, so a qualifier is transparent rather than fatal.
* **An inline ancestor carried the guard.** `#[cfg(test)] mod
  test_support { … mod readiness; }` compiles `readiness.rs` only under
  `cfg(test)`, and the declaration inside carries no attribute at all.
  `src/agent/proc/test_support/readiness.rs` is that file; without the
  ancestry it is a whole test file with no `#[cfg(test)]` anywhere in it,
  which is precisely the shape every census here exists to skip.

So the scan walks each file's **module structure**: brace depth, the
inline modules open at each point, and the `cfg` predicates on each of
them. A declaration is test-only when the conjunction of its own
predicate and every enclosing inline module's predicate is false
wherever `test` is false — `entails_test`.

### What it deliberately does not do

**No transitive closure over files.** `src/effects/tests.rs` is itself a
whole-file test module and declares `mod policy;`, so Rust compiles
`src/effects/tests/policy.rs` only under `cfg(test)` too — and this
derivation does not say so. Every census in this crate reads
`super::production_code`, which removes `#[cfg(test)]` items from the
files it keeps, and those second-level files carry their own inline
`cfg(test)` modules and their own `#![deny]` prologues for exactly that
reason (`effects/tests/classification.rs` and its siblings say so at
length). Closing over the file graph would widen the skip set by a dozen
files whose contents no census has been measured against, which is a
change to what every census can see and not a bug fix. The measured
domain is the set
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
names — the literal `#[cfg(test)] mod tests;` declarations, plus
`scaffold`, `premove`, `fake`, `fixture`, `scratch_tree` and
`readiness` — and it is listed, path
by path, in `effects::tests::cfg::WHOLE_FILE_TEST_MODULES`. One more
arrives with the slice that adds it, and that slice adds its path to
that list: the population lives in one place, so adding a module is one
edit rather than a sweep over every comment that had restated its size.

**No `#[path]`.** A `#[path]` attribute on a module is refused rather
than resolved: it is the one construct that can point a declaration
outside its own directory, and there are none in this tree.

**No `cfg_attr` that applies a `cfg`, and this one is a hole rather
than a choice.** `scan_module_declarations` treats a `cfg_attr` as
significant only when it contains `path`, so `#[cfg_attr(all(),
cfg(test))] mod hidden_tests;` — which rustc applies as `#[cfg(test)]`
and compiles only under test — is read here as an unconditional
declaration, and the file it names stays in every census's domain as
production. There is no such declaration in this tree; the form is
stated because nothing in the crate would notice one. Measured by
writing one and reverting it: the module's own `#[test]` ran, so rustc
had applied the `cfg(test)`, while
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
stayed green with the file outside the population it resolves. It is
invisible to every reading of this derivation at once,
`whole_file_test_modules` included, and to the hand-maintained
`effects::tests::cfg::WHOLE_FILE_TEST_MODULES` that pins the result, so
no assertion over the population can be the thing that catches it. PR
#101's reviewer found it, it predates that change, and widening the
scan to decide `cfg_attr` predicates is its own change with its own
review.

### Panics

When a file cannot be read structurally at all — an attribute that never
closes, a brace that closes one too many, a `mod` with no name or no
terminator, a `cfg` predicate the entailment grammar cannot read, a
`#[path]`, or one name declared twice in one module. Every one of those
means the scan does not know what the file declares, and a scan that
does not know must not answer.

## `pub(crate) mod census_domain` › `assert!(`

**The inventory has to describe the tree being walked.** `source_root`
is the caller's claim about where the crate's sources live, and the
manifest's is the target paths; a `source_root` no target sits under
means the two are about different trees, and every answer below would
be resolved against an inventory that says nothing about the files in
hand. Fail closed, in the same breath as the acquisition itself.

## `pub(crate) mod census_domain` › `pub(crate) struct ScannedDeclaration {`

One `mod` declaration as the scan read it out of a file's structure.

## `pub(crate) struct ScannedDeclaration` › `pub(crate) name: String,`

The declared module's name.

## `pub(crate) struct ScannedDeclaration` › `pub(crate) inline_path: Vec<String>,`

The inline modules enclosing it, outermost first.

## `pub(crate) struct ScannedDeclaration` › `pub(crate) guard: String,`

The effective predicate, rendered.

## `pub(crate) struct ScannedDeclaration` › `pub(crate) test_only: bool,`

Whether that predicate is false wherever `test` is false.

## `pub(crate) mod census_domain` › `pub(crate) enum ScanRefusal {`

Why a file's structure could not be read, and where.

Every variant is a refusal rather than a guess. The direction is the one
`declared_whole_file_test_modules` argues for: a scan that cannot tell
what a file declares must not answer, because both wrong answers are
silent — a missing skip reports a fixture as an offender, and a spurious
one removes a production file from every census below.

## `pub(crate) enum ScanRefusal` › `UnclosedAttribute {`

`#[…` with no `]`.

## `pub(crate) enum ScanRefusal` › `UnbalancedBraces {`

A `}` with no `{`.

## `pub(crate) enum ScanRefusal` › `MalformedDeclaration {`

`mod` with no name, or a name followed by neither `;` nor `{`.

## `pub(crate) enum ScanRefusal` › `UnreadablePredicate {`

A `cfg` predicate the entailment grammar cannot read.

## `pub(crate) enum ScanRefusal` › `UnsupportedPathAttribute {`

`#[path = "…"]`, or a `cfg_attr` that could apply one.

## `pub(crate) enum ScanRefusal` › `UnsupportedInnerCfg {`

An inner `#![cfg(…)]`, which gates the module it is written in.

## `pub(crate) enum ScanRefusal` › `DuplicateDeclaration {`

One module name declared twice in one module.

## `pub(crate) enum ScanRefusal` › `ModuleShapedMacroBody {`

A macro body holding a module-shaped token sequence.

## `pub(crate) mod census_domain` › `pub(crate) fn scan_module_declarations(`

Every `mod` declaration in `source`, with the inline modules enclosing it
and the effective `cfg` predicate it inherits.

Pure over `&str`, which is what makes the refusals above drivable: the
tree satisfies every one of them, so the only way to see one is to hand
this a source that does not.

Comments and string literals are blanked first —
`super::blank_comments_and_strings`, which also handles raw strings,
byte strings and char literals — so a `mod` written in prose is spaces.
The predicate text is read from the **raw** span at the same offsets,
because blanking erases what is inside a string and `feature = "x"` would
otherwise arrive as `feature = "   "`.

## `pub(crate) mod census_domain` › `struct Scope {`

An inline `mod name { … }` that is open at the current position.

## `struct Scope` › `open_depth: usize,`

The brace depth *outside* the module's body.

## `pub(crate) mod census_domain` › `if byte == b'#' {`

-- an attribute, which belongs to whatever item comes next -----

## `pub(crate) mod census_domain` › `"path" => pending_path = true,`

`path` names the file directly; `cfg_attr` can apply one
conditionally. Both are refused where they could reach a
module, which is decided when the item is read.

## `pub(crate) mod census_domain` › `if let Some(invocation) = macro_at(bytes, i) {`

-- a macro, whose body is token trees and not items ------------

`mod x;` inside `macro_rules! m { () => { mod x; } }` is not a
declaration, and `#[cfg(test)] mod x;` inside one is not a
test-only declaration: the tokens are only *shaped* like an item
until something expands them. Walking into a macro body therefore
invents declarations, which is the direction that removes a real
production file from every census below.

A macro invoked at item position **can** expand to a module,
though, and this scan cannot tell which does. So the body is
discarded when it holds nothing module-shaped and refused when it
does: the discard is what stops the false positives, and the
refusal is what stops the discard from becoming a blind spot.
Measured on this tree: zero macro bodies hold one.

## `pub(crate) mod census_domain` › `pending.clear();`

Attributes stacked above a macro invocation belong to it.

## `pub(crate) mod census_domain` › `if let Some(shape) = module_at(bytes, i) {`

-- a `mod` item, with any visibility qualifier in front of it ---

## `pub(crate) mod census_domain` › `i = bytes[name_at..]`

Past the `;`.

## `pub(crate) mod census_domain` › `pending.clear();`

-- anything else: the attributes above it are not a module's ---

## `pub(crate) mod census_domain` › `i = word(bytes, i).end;`

**Past the whole token, `r#` included.** This advanced by
`is_ident_byte`, and a raw identifier is not a run of
identifier bytes: `r#mod` is `r`, a `#`, and `mod`. So the
scan consumed the `r`, met the `#`, stepped over it as a
non-attribute byte, and then read the *inside* of the token
as though it stood at item position. `let r#mod = 1;` — valid
Rust — became `mod = 1;`, a `mod` item with no name, and the
whole file was refused; `use std::r#mod as tests;` inside a
`#[cfg(test)]` module became `mod as;`, a test-only
declaration the crate never wrote, whose skip names a file
that does not exist. `word` is the token this scan reads
everywhere else, and the fallback reads it too now.

## `pub(crate) mod census_domain` › `struct MacroInvocation {`

A macro invocation or `macro_rules!` definition and its delimited body.

## `struct MacroInvocation` › `name: String,`

The macro's name, for the diagnostic.

## `struct MacroInvocation` › `open: usize,`

The index of the body's opening delimiter.

## `struct MacroInvocation` › `close: usize,`

The index of the matching closing delimiter.

## `pub(crate) mod census_domain` › `fn macro_at(bytes: &[u8], at: usize) -> Option<MacroInvocation> {`

`MacroInvocation` beginning at `at`, or `None`.

The shape is an identifier, `!`, an optional second identifier — that is
`macro_rules! name { … }`, the one form that has one — and a delimited
group. Requiring the group is what keeps `a != b` out: after that `!`
comes `=`, which opens nothing. Requiring the second identifier only
after `macro_rules` is what keeps `if !condition { … }` out, which
otherwise reads as an invocation of `if` whose body is the block.

## `fn macro_at(bytes: &[u8], at: usize) -> Option<MacroInvocation> {` › `if !name.raw && is_keyword(name.text) {`

**A keyword before a `!` is unary negation, not a macro name.**
`if !(cond)`, `while !(cond)`, `return !(x)` are identifier, `!`,
delimited group -- the same three tokens as `foo!(…)` -- so reading
them as macros skips the grouped expression, and a `mod` written
inside it (`if !({ mod local {} true })` is valid Rust) then reads as
a module-shaped macro body and refuses the whole file. A macro's path
segment cannot be a keyword unless it is written raw, and `r#if!(…)`
is a macro called `if`, so the test is on the plain spelling only.

## `fn macro_at(bytes: &[u8], at: usize) -> Option<MacroInvocation> {` › `let bang = whitespace(bytes, after_name);`

**Whitespace and comments may sit between the name and its `!`.**
`macro_rules ! m { … }` and `quote /* why */ ! { … }` are both valid
Rust, and `#[rustfmt::skip]` keeps either spelling in a real file —
so requiring the `!` to be the very next byte made the guard miss
exactly the macros somebody had gone out of their way to space out.
Comments are already spaces in the view this reads, so one skip
covers both.

## `fn macro_at(bytes: &[u8], at: usize) -> Option<MacroInvocation> {` › `if !name.raw && name.text == b"macro_rules" {`

`macro_rules! name { … }` is the **only** form carrying a name
between the `!` and the body, and reading one for every macro is
what would make `if !condition { … }` an invocation of `if` once the
gap above is allowed: identifier, `!`, identifier, delimiter — and
the whole block would be skipped. Keyed on the one name that has it.

## `fn macro_at(bytes: &[u8], at: usize) -> Option<MacroInvocation> {` › `let defined = word(bytes, cursor);`

The defined name may itself be raw -- `macro_rules! r#mod { … }`
is how a macro takes a keyword for a name.

## `pub(crate) mod census_domain` › `fn module_shaped_between(bytes: &[u8], from: usize, to: usize) -> Option<usize> {`

Where a module-shaped token sequence starts inside `from..to`, if any.

"Module-shaped" is the word `mod`, a name, and a `;` or `{` — the same
three tokens `module_at` reads, minus the visibility prefix, because
what matters here is only whether the body *could* expand to a module.

## `fn module_shaped_between(bytes: &[u8], from: usize, to: usize) -> Option<usize> {` › `let declared = word(bytes, name_at);`

The name may be raw: `mod r#type;` inside a macro body is
as module-shaped as `mod tests;` is.

## `pub(crate) mod census_domain` › `fn identifier(bytes: &[u8], from: usize) -> (usize, &[u8]) {`

The identifier at `from`, and where it ends. Empty when there is none.

## `pub(crate) mod census_domain` › `struct Word<'a> {`

One identifier token, raw or plain.

## `struct Word<'a>` › `end: usize,`

Where the token ends, `r#` included.

## `struct Word<'a>` › `raw: bool,`

Whether it was written `r#name`.

## `struct Word<'a>` › `text: &'a [u8],`

The name, without any `r#`.

## `pub(crate) mod census_domain` › `fn word(bytes: &[u8], from: usize) -> Word<'_> {`

The identifier token at `from`, reading `r#name` as one token.

**A raw identifier is one token and its name may be a keyword.** That is
the whole reason this exists: `mod r#type;` declares a module called
`type`, and a reader that stopped at the `#` saw `mod r` followed by
something that is not a terminator and refused the file. `raw` is an
ordinary identifier that merely begins with the same letter, so the
prefix counts only when a `#` and an identifier byte follow it.

## `pub(crate) mod census_domain` › `const KEYWORDS: &[&[u8]] = &[`

Rust's keywords, strict and reserved.

**A keyword cannot be a macro's path segment**, and that is the only
structural thing separating `if !(…)` from `foo!(…)`: both are an
identifier, a `!` and a delimited group. Written raw it can --
`r#if!(…)` is a macro named `if` -- which is why `Word` carries that
bit rather than only the text.

## `pub(crate) mod census_domain` › `fn is_keyword(text: &[u8]) -> bool {`

Whether `text` is a Rust keyword written plainly.

## `pub(crate) mod census_domain` › `fn whitespace(bytes: &[u8], from: usize) -> usize {`

The first non-whitespace index at or after `from`.

## `pub(crate) mod census_domain` › `struct ModuleShape {`

A `mod` item beginning at `at`, past any visibility qualifier.

## `struct ModuleShape` › `name_at: usize,`

Where the module's name starts.

## `struct ModuleShape` › `body: Option<usize>,`

The index of the body's `{`, or `None` for `mod name;`.

## `pub(crate) mod census_domain` › `fn module_at(bytes: &[u8], at: usize) -> Option<ModuleShape> {`

`ModuleShape` at `at`, or `None` when this is not a `mod` item.

`pub`, `pub(crate)`, `pub(super)` and `pub(in a::b)` are transparent:
they are read and stepped over rather than treated as the start of some
other item, which is the whole of what "visibility-qualified declaration"
costs a structural scan. A text rule keyed on `mod ` immediately after
the attribute could not do it, and that is the hole this closes.

## `fn module_at(bytes: &[u8], at: usize) -> Option<ModuleShape>` › `let mut token = word(bytes, at);`

Raw-aware throughout: `r#pub` and `r#mod` are identifiers named for
keywords, not the keywords, and neither opens a module item.

## `fn module_at(bytes: &[u8], at: usize) -> Option<ModuleShape>` › `let after_keyword = whitespace(bytes, token.end);`

`mod` and the name must be separated: `models` is not `mod els`.

## `fn module_at(bytes: &[u8], at: usize) -> Option<ModuleShape>` › `let declared = word(bytes, after_keyword);`

The declared name is the identifier without its `r#`: `mod r#type;`
names `type.rs`, the way rustc resolves it.

## `fn module_at(bytes: &[u8], at: usize) -> Option<ModuleShape>` › `_ => Some(ModuleShape {`

A name with neither terminator is malformed, and the caller
refuses it. Reported through an empty-bodied shape so the caller
sees the position rather than silently skipping the item.

## `pub(crate) mod census_domain` › `pub(crate) enum Predicate {`

A `cfg` predicate, reduced to the one question this module asks of it.

`effects::tests::cfg` models predicates *properly* — every `target_os`,
every CI valuation, which platform compiles which body — and answers a
different question with them. This decides one: is the predicate false
wherever `test` is false. So every atom that is not `test` collapses to
`Predicate::Other`, and the grammar below is the whole of what the
derivation reads. A predicate it cannot parse is a refusal, not a guess.

## `pub(crate) enum Predicate` › `Test,`

The `test` atom itself.

## `pub(crate) enum Predicate` › `Other(String),`

Any other atom: a bare name, or `key = "value"`.

## `pub(crate) enum Predicate` › `All(Vec<Predicate>),`

`all(…)`, and the conjunction an inline ancestry composes.

## `pub(crate) enum Predicate` › `Any(Vec<Predicate>),`

`any(…)`.

## `pub(crate) enum Predicate` › `Not(Box<Predicate>),`

`not(…)`.

## `impl Predicate` › `fn all(parts: Vec<Predicate>) -> Self {`

The conjunction of `parts`, flattened; the empty one is `All([])`,
which is true and entails nothing.

## `impl Predicate` › `pub(crate) fn render(&self) -> String {`

The predicate as it reads, for a diagnostic.

## `pub(crate) mod census_domain` › `pub(crate) fn entails_test(predicate: &Predicate) -> bool {`

Whether `predicate` is false wherever `test` is false.

Three-valued, with `test` bound to false and every other atom left
*unknown* — which is the only sound reading, because this module knows
nothing about platforms or features and must not pretend to. `all(test,
unix)` entails; `any(test, unix)` does not, because a Unix build without
`test` compiles it; `not(test)` does not.

## `pub(crate) mod census_domain` › `fn decide_without_test(predicate: &Predicate) -> Option<bool> {`

`predicate` with `test = false` and every other atom unknown.

## `fn decide_without_test(predicate: &Predicate) -> Option<bool> {` › `Predicate::All(parts) => {`

Short-circuiting, and the `None` arms are the point: one
undecidable conjunct does not make a conjunction undecidable if
another is already false, and one undecidable disjunct does not
make a disjunction undecidable if another is already true. The
empty forms answer as `cfg` does -- `all()` is true, `any()` is
false.

## `pub(crate) mod census_domain` › `pub(crate) fn parse_predicate(written: &str) -> Result<Predicate, String> {`

`written` as a `Predicate`, or why it cannot be read.

The grammar is `all(…)`, `any(…)`, `not(P)`, and an atom — a bare name
or `name = "value"`. Anything else is refused: an unknown combinator, an
unbalanced paren, `not` with other than one argument, an empty atom.

## `pub(crate) fn parse_predicate(written: &str) -> Result<Predicate, String> {` › `if name.is_empty() {`

An atom: `test`, `unix`, or `key = "value"`.

## `pub(crate) mod census_domain` › `fn split_arguments(text: &str) -> Result<Vec<&str>, String> {`

The comma-separated arguments of a parenthesised group starting at `(`.

## `pub(crate) mod lint_levels {`

The **file-module-level lint state** reader, for the governance censuses.

`#[cfg(test)]` and `pub(crate)`, and both halves are the point. This is a
census instrument, not a product API: nothing the binary does consults it,
and a `pub fn` here would have been a shipped surface added for a test to
call. It sits at the BOTTOM beside `census_domain` for the same reason
that module does — `production_region` cuts a file at its first
`#[cfg(test)]` and
`effects::tests::every_production_region_that_stops_early_stops_at_a_module`
pins the ten files whose cut lands on something that is not a module. This
file is not one of them and must not become one.

## `pub(crate) mod lint_levels` › `pub(crate) struct Resolution {`

How a file's prologue resolves for one lint: the level **in force**, and
whether rustc refuses the prologue outright.

## `pub(crate) struct Resolution` › `pub(crate) level: Option<&'static str>,`

The level governing the file module, or `None` when its prologue
states none and the lint is left at whatever it inherits.

## `pub(crate) struct Resolution` › `pub(crate) refused_downgrade: bool,`

A later attribute tried to weaken a `forbid`. rustc answers `E0453`
and the crate does not compile, so this is not a level at all — it is
the file failing to build, and a reader that folded it into a level
would report a governance state for a file that has none.

## `pub(crate) mod lint_levels` › `pub(crate) fn file_level_lint_resolution(source: &str, lint: &str) -> Resolution {`

`Resolution` for `lint` over `source`'s file-module prologue.

"File-module level" is the whole of the claim, and it is narrower than
"somewhere in the file". A lint level is scoped by the module tree, so
`#![deny(clippy::disallowed_types)]` in a file's prologue governs the file
and everything nested in it — while `#[deny(clippy::disallowed_types)]`
written on a single `fn` governs that function and says nothing whatever
about the file, which goes on inheriting whatever its ancestors allow.
A scan that accepts the second in place of the first reports a module as
having stated its own level when it has not, which is `PR6-LANEF-004`
answered by the wrong evidence.

So the walk is: from the first byte, over whitespace and **inner**
attributes only, stopping at the first token that is neither. That is
exactly the region an `#![…]` may govern the file module from, and it is the
same rule `super::is_module_level` applies to the inner half of its answer.

### Ordered, because rustc is ordered

`PR72-LEVELS-001`. This used to return at the **first** attribute naming
the lint, which is not what a prologue means. Lint levels at one scope
are applied in source order and the last one wins, so
`#![deny(L)] #![allow(L)]` is a file where `L` is **allowed** — and the
first-match reader called it a denial. That is the failure direction that
matters: a census asking "has this module closed the hole" was told yes
by a prologue whose second line reopens it, and the reopening line is
exactly what an author adding an exception writes.

`forbid` is not symmetrical with the rest and is not modelled as if it
were. Once a lint is forbidden at a scope, a later `allow`, `warn` or
`expect` of it is `E0453` — the crate does not compile — while a later
`deny` or `forbid` is accepted and leaves the forbid in force. Both halves
are **measured** rather than reasoned: every row of
`effects::tests::the_file_level_lint_reader_answers_what_rustc_does` is
compiled by `clippy-driver` and this reader's answer is checked against
the diagnostics that come back, so no sentence here is the authority for
what the compiler does.

### What it deliberately does not do

**Lint groups are not expanded.** `#![deny(clippy::all)]` denies this
lint to rustc and reads as `None` here. The direction is the safe one — a
census is told a module states nothing when it states something, which is
loud — and the tree is measured rather than trusted:
[`tests::the_three_blunt_governed_lints_are_used_by_nobody`](effects/tests.md) asserts that
`clippy::all`, `clippy::style` and `warnings` are used by no file at all.

Comments and string literals are blanked first, so a level quoted in a doc
comment or inside a `&str` is invisible — `PR4-CENSUS-COMMENT-ORACLE`, and
this crate's effect fixtures are written as exactly those two shapes.

`clippy::disallowed_methods` and `disallowed_methods` are the same lint;
`super::normalize_lint` is the bridge, as it is everywhere else here.

## `pub(crate) fn file_level_lint_resolution(source: &str, lint: &str) -> Resolution {` › `if bytes[at] != b'#' || bytes.get(at + 1) != Some(&b'!') {`

The prologue ends at the first token that is not an inner attribute.

## `pub(crate) fn file_level_lint_resolution(source: &str, lint: &str) -> Resolution {` › `let Some(list) = rest`

`allowance(…)` strips to `ance(…)`, which opens nothing: the
parenthesis is what makes the prefix an exact attribute name.

## `pub(crate) fn file_level_lint_resolution(source: &str, lint: &str) -> Resolution {` › `if resolution.level == Some("forbid") {`

Ordered, and `forbid` is sticky. A weaker level after a
`forbid` is `E0453`, which is the file not compiling rather
than a level; anything else replaces what came before it.

## `pub(crate) mod lint_levels` › `pub(crate) fn file_level_lint_state(source: &str, lint: &str) -> Option<&'static str> {`

The level in force for `lint` at `source`'s file-module scope, or none.

`file_level_lint_resolution` without the `E0453` bit, for the censuses
that ask only which level governs a module.

## `pub(crate) mod lint_levels` › `fn names_lint(entry: &str, lint: &str) -> bool {`

Whether an attribute entry names `lint`, qualified either way.

## `pub(crate) mod tests;`

`pub(crate)` so `cfg::WHOLE_FILE_TEST_MODULES` -- the crate's only statement
of the whole-file test-module population -- reaches the one census outside
this module that floors a count on it. Test-only either way: the module is
compiled only under `cfg(test)`.

## `if depth == 0`

A named function cannot be part of the preceding return type. If a
malformed test signature has no body, keep the following item visible
instead of taking its brace as the missing test body's boundary.

## `fn configured_function_return_start(bytes: &[u8], start: usize) -> Option<usize> {`

Recognize the return arrow of a named function item without type parameters.
A field's function-pointer type is not an item. Unknown prefixes, generic
parameter lists and incomplete signatures keep the conservative comma rule.
The input is already blanked, including any extern ABI string.

## `#[must_use]`

Every `allow`/`expect` of a governed lint in `source`, with where it sits.

Attributes are found in the blanked text and read out of the original, so a
fixture quoted in a doc comment is invisible and a real attribute is not.
