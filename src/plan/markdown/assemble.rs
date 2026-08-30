//! Assembly: ids, kinds, dependencies, artifacts.
//!
//! The last step, and the only one that mints IR. Explicit `id=` values are
//! reserved before any slug is derived, so a derived id never collides with
//! one an author wrote; duplicate explicit ids are left intact for `validate`
//! to report. An absent `depends=` chains a task to its predecessor in
//! document order, and `depends=` with no value breaks that chain
//! deliberately. Kinds fall back to a keyword heuristic over the title.
//!
//! The sink of the DAG: fed by [`super::drafts`] and [`super::hints`], read by
//! nothing but the adapter itself.

use super::drafts::Draft;
use super::hints::push_unique;
use crate::ir::{Artifact, ArtifactId, Task, TaskId, TaskKind};

pub(super) fn assemble(drafts: Vec<Draft>) -> Vec<Task> {
    // Reserve explicit ids first so derived slugs never collide with them.
    // Explicit duplicates are left intact for validation to report.
    let mut taken: Vec<String> = drafts
        .iter()
        .filter_map(|d| d.annotation().id.clone())
        .collect();
    let mut previous_id: Option<TaskId> = None;
    let mut tasks = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let ann = draft.annotation();
        let id = match ann.id.clone() {
            Some(explicit) => explicit,
            None => unique_slug(&draft.title, &mut taken),
        };
        let kind = ann.kind.unwrap_or_else(|| heuristic_kind(&draft.title));
        let depends_on: Vec<TaskId> = match &ann.depends {
            Some(ids) => ids.iter().map(|s| TaskId::from(s.as_str())).collect(),
            None => previous_id.clone().into_iter().collect(),
        };
        let mut path_hints = ann.paths.clone();
        for hint in &draft.hints {
            push_unique(&mut path_hints, hint);
        }
        let task = Task {
            id: TaskId(id),
            kind,
            title: draft.title,
            body: draft.body,
            depends_on,
            acceptance: draft.acceptance,
            path_hints,
            suggested_tier: ann.tier,
            min_tier: ann.min,
            artifacts_in: ann
                .needs
                .iter()
                .map(|s| ArtifactId::from(s.as_str()))
                .collect(),
            artifacts_out: ann
                .out
                .iter()
                .map(|s| ArtifactId::from(s.as_str()))
                .collect(),
        };
        previous_id = Some(task.id.clone());
        tasks.push(task);
    }
    tasks
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() {
        "task".to_owned()
    } else {
        slug
    }
}

fn unique_slug(title: &str, taken: &mut Vec<String>) -> String {
    let base = slugify(title);
    let mut candidate = base.clone();
    let mut n = 1;
    while taken.iter().any(|t| t == &candidate) {
        n += 1;
        candidate = format!("{base}-{n}");
    }
    taken.push(candidate.clone());
    candidate
}

fn heuristic_kind(title: &str) -> TaskKind {
    let words: Vec<String> = title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let has = |needles: &[&str]| words.iter().any(|w| needles.contains(&w.as_str()));
    if has(&["fix", "bug", "bugfix", "hotfix", "repair"]) {
        TaskKind::Fix
    } else if has(&["test", "tests", "testing", "coverage"]) {
        TaskKind::Test
    } else if has(&[
        "doc",
        "docs",
        "document",
        "documentation",
        "readme",
        "changelog",
    ]) {
        TaskKind::Docs
    } else if has(&["refactor", "refactoring", "restructure"]) {
        TaskKind::Refactor
    } else if has(&["design", "spec", "architecture"]) {
        TaskKind::Design
    } else if has(&["chore", "cleanup", "bump", "rename", "upgrade"]) {
        TaskKind::Chore
    } else {
        TaskKind::Implement
    }
}

/// Artifacts come from `out=` annotations; a bare plan with a Design task
/// defaults to a conventions brief produced by the first one (§9).
pub(super) fn collect_artifacts(tasks: &mut [Task], warnings: &mut Vec<String>) -> Vec<Artifact> {
    let mut artifacts: Vec<Artifact> = Vec::new();
    for task in tasks.iter() {
        for out in &task.artifacts_out {
            if !artifacts.iter().any(|a| a.id == *out) {
                artifacts.push(Artifact {
                    id: out.clone(),
                    produced_by: Some(task.id.clone()),
                });
            }
        }
    }
    if artifacts.is_empty() {
        if let Some(design) = tasks.iter_mut().find(|t| t.kind == TaskKind::Design) {
            let id = ArtifactId::from("conventions-brief");
            design.artifacts_out.push(id.clone());
            artifacts.push(Artifact {
                id,
                produced_by: Some(design.id.clone()),
            });
        }
    }
    for task in tasks.iter() {
        for needed in &task.artifacts_in {
            if !artifacts.iter().any(|a| a.id == *needed) {
                warnings.push(format!(
                    "task `{}` needs artifact `{needed}` that no task produces",
                    task.id
                ));
            }
        }
    }
    artifacts
}
