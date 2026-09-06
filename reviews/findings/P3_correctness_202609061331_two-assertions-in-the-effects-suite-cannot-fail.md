---
id: SWEEP-VOCAB-002
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/tests.rs:726
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects/tests.rs`, queue row 27 of `standards/SWEEP.md`
---

## Failure sequence

Two assertions in `src/topology/effects/tests.rs` are true for every
implementation of the thing they name, so no mutation of that thing can make
them fail. Found while sweeping `src/topology/effects/vocab.rs` (queue row 26),
whose accessors they were the apparent coverage of.

**`the_inventory_is_the_eleven_groups_and_every_one_of_them_has_sites`, line
726:**

    assert!(site.name().starts_with(&format!("{}.", site.group().name())))

`EffectSiteId::name` is `format!("{}.{}", self.group().name(), self.variant())`
(`src/topology/effects.rs:347`). Both sides are the same call, so the assertion
holds whatever `FunnelGroup::name` returns: spell `Worktree` as `"worktree"`,
or as `""`, and it still passes. Its comment — "the site's group prefix is its
group's own name, not a second copy" — describes a property that is true by
construction and cannot be broken here.

**`there_is_no_host_on_which_a_containment_point_is_unrequired`, line 6136:**

    assert_eq!(Host::current().other(), Host::current().other());

Reflexive. It passes for `Host::other` returning its own argument, for the
constant `Host::Unix`, and for anything else deterministic. The line below it,
`assert_ne!(Host::current().other(), Host::current())`, is the live one.

Neither is a wrong claim about the system; both are evidence that is not
evidence, in a file whose other assertions carry real weight, and a reader
counting coverage of `FunnelGroup::name` would count line 726 and find nothing
else. Outside the topology suite the spelling is held only by
`effects/residue-classes.json`'s byte comparison.

Covered forward, not backward: `FunnelGroup::name` and `Host::other` are now
pinned per variant by
`topology::effects::vocab::tests::every_funnel_group_spells_the_name_a_dotted_site_name_is_built_from`
and
`topology::effects::vocab::tests::each_host_names_itself_requires_its_own_platform_and_is_the_others_other`,
each witnessed against a mutation. So the gap in coverage is closed; what is
left is the two dead assertions themselves, in a file this sweep's own-file
bound excludes.

## What the change that takes this up should do

Delete line 6136 outright — the `assert_ne!` beneath it is the assertion that
was meant. Replace line 726 with a claim about a value rather than about an
expression: assert the dotted name of a named site against a literal
(`EffectSiteId::Worktree(WorktreeSite::Add).name() == "Worktree.Add"`), which
dies when either half of the format changes, or delete it and let row 26's
per-variant spelling table carry it.
