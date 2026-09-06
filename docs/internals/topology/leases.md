# `src/topology/leases.rs`

Extended notes for [`src/topology/leases.rs`](../../../src/topology/leases.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Who holds which region of the repository, and what a settlement may do to
that holding.

Two tasks may run in parallel exactly when the regions they touch do not
overlap, so a lease is the run's admission currency. There are three owners
and they are not interchangeable:

* **Generation** — the *predicted* region an ordinary dispatch took from the
  plan's path hints. It is a guess, and it is replaced rather than confirmed.
* **Candidate** — the *actual* region the diff touched, taken when the
  candidate is prepared. This is what the merge queue is entitled to trust.
* **Lineage** — the region a rejected candidate and every repair descended
  from it hold together, widened by each rejection's conflict paths.

The comparison itself is [`PathPolicy`]'s: component-wise
equal/ancestor/descendant, case-folded when the run resolved a case-folding
filesystem, and [`PathSet::RepoWide`] overlapping everything. Component-wise
is the whole of the subtlety — `src/foo` and `src/foobar` are different
regions, and a byte-prefix comparison would serialize them against each
other forever.

## `pub enum LeaseOwner {`

Whose holding a region is.

## `pub enum LeaseOwner` › `Generation {`

The predicted region of one generation.

## `pub enum LeaseOwner` › `Candidate {`

The actual region of one prepared candidate.

## `pub enum LeaseOwner` › `Lineage { root: TaskKey },`

The region a repair lineage holds, named by the original it descends
from.

## `impl LeaseOwner` › `pub fn key(self) -> TaskKey {`

The task this holding is attributed to.

## `impl LeaseOwner` › `pub fn is_lineage(self) -> bool {`

Whether this is the lineage holding, which no settlement ever changes.

## `pub fn regions_overlap(left: &PathSet, right: &PathSet, policy: &PathPolicy) -> bool {`

Whether two regions have any path in common.

[`PathSet::RepoWide`] overlaps everything, including another `RepoWide` and
including the empty region: it is the answer for a region nobody could read,
and the safe reading of an unread region is that it might be anywhere.

## `pub fn paths_overlap(left: &GitPath, right: &GitPath, policy: &PathPolicy) -> bool {`

Whether two paths name regions that contain one another.

Equal, ancestor, or descendant — decided component by component, so
`src/foo` neither contains nor is contained by `src/foobar` even though one
is a byte prefix of the other.

## `pub fn paths_overlap(left: &GitPath, right: &GitPath, polic…` › `(None, _) | (_, None) => return true,`

One list ran out while every component so far matched: the
shorter path is an ancestor of the longer one, or they are equal.

## `fn components(path: &GitPath) -> impl Iterator<Item = &str> {`

A Git path's components, ignoring empty ones so that a trailing or doubled
separator cannot make two names of one directory look like two directories.

## `fn components_equal(left: &str, right: &str, case_fold: bool) -> bool {`

Whether two path components name the same component.

Case-folded by Unicode simple lowercase rather than by ASCII alone: a
case-folding filesystem folds `Ü` the same way it folds `U`, and a
comparison that only folded ASCII would admit two tasks in parallel over one
file whose name is not written in it. Compared lazily so nothing allocates.

## `pub struct LineageLease {`

One lineage's holding, with the order it was created in.

The order is load-bearing: a lineage member's candidate is ineligible while
it overlaps an *older* lineage's lease, so that two lineages contending for
one region resolve in a fixed direction instead of taking turns blocking
each other.

## `pub struct LineageLease` › `pub age: u32,`

Run-local creation ordinal, dense from 0.

## `pub struct LeaseTable {`

Every region this run currently holds.

## `impl LeaseTable` › `pub fn grant(&mut self, owner: LeaseOwner, paths: PathSet) {`

Take a holding for `owner`, replacing any it already had.

## `impl LeaseTable` › `pub fn widen_lineage(&mut self, root: TaskKey, paths: &PathSet) {`

Add `paths` to a lineage's holding. A lineage only ever grows.

## `impl LeaseTable` › `pub fn release(&mut self, owner: LeaseOwner) {`

Give up a holding. Releasing one nobody holds is not an error: the
caller is stating an outcome, not performing a bookkeeping operation.

## `impl LeaseTable` › `pub fn any_candidate_or_lineage(&self) -> bool {`

Whether any candidate or lineage holding is active — the two `Complete`
refuses to leave behind.

## `impl LeaseTable` › `pub fn overlaps_another(`

Whether `paths` collide with a holding belonging to anyone but `owner`.

The dispatch check: an ordinary dispatch is blocked by any overlapping
active lease of another owner, and a repair dispatch is never
lease-blocked, which is the caller's distinction rather than this one's.

## `impl LeaseTable` › `pub fn overlapping_lineages<'a>(`

Every lineage holding that collides with `paths`, oldest first.

## `fn union(left: &PathSet, right: &PathSet) -> PathSet {`

The region covering both, with `RepoWide` absorbing everything.

## `pub enum GenerationLease {`

What kind of holding a generation has, which is what decides the
dispositions its settlements may record.

## `pub enum GenerationLease` › `Own,`

The generation's own predicted region, later replaced by its
candidate's actual one.

## `pub enum GenerationLease` › `InheritedLineage { root: TaskKey },`

A repair executes inside the lineage lease its root already holds and
takes nothing of its own.

## `impl GenerationLease` › `pub fn expected(self, survives: bool) -> LeaseDisposition {`

The disposition an event must record, given whether the generation
survives it.

Total, and the whole of the rule. A repair never changes a lineage
lease, so every one of its settlements records
[`LeaseDisposition::LineageHeld`]. An ordinary generation holds a
region of its own, so the disposition is exactly whether it still holds
it: a settlement that closes the generation releases it, and one that
leaves the generation open — an interruption, or the success that hands
the region to the candidate — keeps it.
