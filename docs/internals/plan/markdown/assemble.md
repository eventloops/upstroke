# `src/plan/markdown/assemble.rs`

Extended notes for [`src/plan/markdown/assemble.rs`](../../../../src/plan/markdown/assemble.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Assembly: ids, kinds, dependencies, artifacts.

The last step, and the only one that mints IR. Explicit `id=` values are
reserved before any slug is derived, so a derived id never collides with
one an author wrote; duplicate explicit ids are left intact for `validate`
to report. An absent `depends=` chains a task to its predecessor in
document order, and `depends=` with no value breaks that chain
deliberately. Kinds fall back to a keyword heuristic over the title.

The sink of the DAG: fed by [`super::drafts`] and [`super::hints`], read by
nothing but the adapter itself.

## `pub(super) fn assemble(drafts: Vec<Draft>) -> Vec<Task>` › `let annotated: Vec<_> = drafts`

Pair each draft with its annotation once. `Draft::annotation` returns an
owned copy, so reading it twice — once to reserve explicit ids, once to
build the task — copied every annotation twice per draft.

## `pub(super) fn assemble(drafts: Vec<Draft>) -> Vec<Task>` › `let mut taken: Vec<String> = annotated`

Reserve explicit ids first so derived slugs never collide with them.
Explicit duplicates are left intact for validation to report.

The reservation keeps its own copy of each explicit id, and that copy is
the point: `taken` has to stay valid after the loop below consumes the
pairing by value, and each annotation's own id moves from there into the
task it belongs to. The registry and the task are two owners of one
string, which is what the copy says; nothing here is a borrow the checker
refused.

## `pub(super) fn collect_artifacts(tasks: &mut [Task], warnings: &mut Vec<String>) -> Vec<Artifact> {`

Artifacts come from `out=` annotations; a bare plan with a Design task
defaults to a conventions brief produced by the first one (§9).
