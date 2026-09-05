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

## `pub(super) fn assemble(drafts: Vec<Draft>) -> Vec<Task>` › `let mut taken: Vec<String> = drafts`

Reserve explicit ids first so derived slugs never collide with them.
Explicit duplicates are left intact for validation to report.

## `pub(super) fn collect_artifacts(tasks: &mut [Task], warnings: &mut Vec<String>) -> Vec<Artifact> {`

Artifacts come from `out=` annotations; a bare plan with a Design task
defaults to a conventions brief produced by the first one (§9).
