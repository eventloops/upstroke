# `src/effects/tests/artifacts.rs`

Extended notes for [`src/effects/tests/artifacts.rs`](../../../../src/effects/tests/artifacts.rs).

The code is the authority for what it does. These notes started as the module's source prose.
Each code fragment in a heading is an exact source substring. When a heading names an enclosing
item before `›`, find that item first, then the following fragment within it.

## Module

`outputs`, the **declarations**: what the generated artifacts contain and
what the frozen site inventory says about this tree.

Six items. Four are what an artifact or a table *states* -- the line
discipline every artifact comparison is made under, the module a group's
funnel bodies are actually in, the sites the inventory declares that no
funnel names, and the sampling N the residue record freezes. The other two
render two of those artifacts from the enums. None of them reads a
checked-in file, writes one, or starts a process.

**No name here is a test name.** The section's six `#[test]` wrappers stay
in `super`, under the names the contract and CI know, so `--list` over the
test binary is unchanged and nothing nests under
`effects::tests::artifacts`. Five of the six read a declaration from here;
the sixth, `no_production_api_exports_a_writable_process_command`, reads
none and is a structural control over production signatures rather than a
claim about an artifact's content, which is why it did not follow.

**The three regeneration writes deliberately did not come with them, and
that is what fixes this boundary.** `fs::write` is the first entry of
`clippy.toml`'s `disallowed-methods`, the denials below are restored rather
than inherited, and that file records where an allowance may live: "only as
a module-level attribute in a file listed in `effects/allowlist.toml`". So a
child that regenerated an artifact would have to be added to that file.

Measured, not argued: with the regenerating arm of
`the_checked_in_effect_sites_json_is_what_the_enums_generate` moved here as
a `pub(super)` helper, `cargo clippy --all-targets --all-features -- -D
warnings` fails with `error: use of a disallowed method `std::fs::write``
against this file, and the second span it prints is the `#![deny(...)]`
below -- so the refusal is this module's own restored denial and not an
ambient rule. Allowing it is a change to the allowlist, which is a
governance claim about where an effect may live and not a mechanical
consequence of moving a declaration. The cut that needs no such claim is the
one that leaves the three writes with the harness, which is where
`policy.rs` left the effectful build helpers and for the same reason.

What that leaves is a file reached by a plain `mod` declaration, which is
inside every whole-tree census's domain: `whole_file_test_modules` derives
its skip set from the crate's own test-module declarations alone and this
file is not one of them. That costs nothing here, because the declarations
carry no needle any of those censuses looks for -- no container-runtime
name, no container intent type, no process-builder construction, no
durability barrier, no governed allow. Their needles are deliberately not
written out here even in prose, and neither is the declaration form that
would move the whole-file census, for the reason `policy.rs` gives: one
written inside a comment is the exact shape that once derived a phantom skip
and removed a real file from every census below it, and the blanking that
now defeats it is not a reason to write another. Measured instead of
asserted -- `runner::tests::every_production_process_start_is_classified`,
`every_production_command_spec_payload_is_classified` and
`runner::container::resolve::tests::no_module_outside_the_container_runner_writes_a_container_intent`
are green with this file in their domain, and
`the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names`
still resolves the `cfg::WHOLE_FILE_TEST_MODULES` population, this file not
among them.

The three effect denials are **restored** here rather than inherited.
`super` allows them because it drives a compiler over fixtures it creates
and regenerates the artifacts these declarations describe; nothing in this
file does either, so the allowance has no business reaching it. That is also
what keeps this module out of `effects/allowlist.toml`: an allowance is what
that file records, and this module takes none.

## `pub(super) fn artifact_content(text: &str) -> String {`

---------------------------------------------------------------------------
The line discipline every artifact comparison is made under
---------------------------------------------------------------------------

## `pub(super) fn artifact_content(text: &str) -> String {`

A generated artifact's content, with the line discipline the checkout gave it
taken out of the comparison.

**Measured, not anticipated.** The first three Windows guest runs failed both
artifact tests and nothing else: the guest's `core.autocrlf` checks these
files out with `\r\n`, and `serde_json::to_string_pretty` emits `\n`, so the
byte comparison was asserting the checkout's line endings rather than the
document's content. `test (windows-latest)` in CI would have failed the same
way. The claim these tests make is that the *inventory* is what the enums
generate; the separator between its lines is the filesystem's business.

## `pub(super) fn funnel_module_record() -> String {`

---------------------------------------------------------------------------
`effects/funnel-modules.json`: the inventory's claim against the tree's answer
---------------------------------------------------------------------------

## `pub(super) fn funnel_module_record() -> String {`

The companion record, from the enums and from `funnel_module`.

## `pub(super) fn funnel_module(site: EffectSiteId) -> &'static str {`

The module a group's funnel bodies are actually in.

`FunnelGroup::module()` is PR3's answer and is frozen. For one group it is
not where PR5 put the code: `mechanism` (2) places "the answer funnels in
src/interaction.rs", and lane B put the bodies in `src/rundir.rs`, leaving
`interaction::{write_question, write_answer, read_answer}` as thin
delegations. Both files are in the allowlist's funnel section and the
disagreement is section J of `reconciliation-D.md`; it is recorded here
rather than resolved by silence, because silently searching the right file
would make the inventory's `module` column read as correct.

## `pub(super) const SITES_WITHOUT_A_FUNNEL: &[&str] = &[`

The sites the frozen inventory declares that no funnel in this tree names.

Every one is a row in `reconciliation-D.md`'s site inventory with the packet
key that defers it. They are written out rather than counted because *which*
site is missing is the finding: a count would survive a swap.

## `pub(super) const SITES_WITHOUT_A_FUNNEL` › `"Report.Write"`

The **Container group is no longer here.** PR5 recorded all eight as
unimplemented because `FunnelGroup::Container.module()` names
`src/runner/container.rs` and that file was not in the tree; PR6 adds it,
and every one of the eight is taken by value by an API in it. The group
leaving this list is the finding that PR6 landed, and a variant coming
back would mean a funnel stopped naming its site.

`ReportSite::Write` maps to `src/util.rs`, and the report write this slice
ships is `RunDir.WriteReport` in `src/rundir.rs` (`rundir::write_report`,
which calls `util::write_json`). `PR3-REPORT-DOUBLE-NAME` in
`reviews/FINDINGS.md` is the standing entry for the two names on one file
and is the owner's, not this slice's.

## `pub(super) const SAMPLING_N: u32 = 8;`

---------------------------------------------------------------------------
`effects/residue-classes.json`: the declarations half
---------------------------------------------------------------------------

## `pub(super) const SAMPLING_N: u32 = 8;`

The sampling N `effects/residue-classes.json` freezes.
