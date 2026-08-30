//! The plan's dependency graph, as `analyze` checks it: duplicate ids, unknown
//! `depends` targets, cycles, and the artifact wiring that only warns.
//!
//! Split out of `super` unchanged — every function here is the one that stood
//! beside `analyze_captured`, moved rather than rewritten, and `check_graph` is
//! still the single entry point it calls.
//!
//! **The denial is restored rather than inherited.** `super` carries
//! `#![allow(clippy::disallowed_methods)]` because `write_normalized_json`
//! writes the normalized plan with `fs::write`, and a module-level allow reaches
//! every child of the module it sits in. Nothing here writes anything — these
//! are pure functions over a parsed [`Plan`] — so that allowance has no business
//! extending here, and this line is what stops it. That is also what keeps this
//! file out of `effects/allowlist.toml`: an allowance is what that file records,
//! and this module takes none.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use crate::error::{UpstrokeError, ValidationErrors};
use crate::ir::{Plan, Task, TaskId};

/// Duplicate ids, unknown `depends` targets, then cycles — all collected so a
/// broken plan reports everything in one run. On a clean graph, artifact
/// wiring that contradicts the dependency order is surfaced as warnings.
pub(super) fn check_graph(plan: &Plan, warnings: &mut Vec<String>) -> Result<(), UpstrokeError> {
    let mut problems = Vec::new();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for task in &plan.tasks {
        *seen.entry(task.id.as_str()).or_insert(0) += 1;
    }
    for (id, count) in &seen {
        if *count > 1 {
            problems.push(format!("duplicate task id `{id}` ({count} tasks share it)"));
        }
    }
    for task in &plan.tasks {
        for dep in &task.depends_on {
            if !seen.contains_key(dep.as_str()) {
                problems.push(format!("task `{}` depends on unknown id `{dep}`", task.id));
            }
        }
    }
    // Cycle detection only makes sense on a graph whose edges all resolve.
    if problems.is_empty() {
        if let Some(cycle) = find_cycle(plan) {
            problems.push(format!("dependency cycle: {}", cycle.join(" -> ")));
        }
    }
    if !problems.is_empty() {
        return Err(UpstrokeError::Validation(ValidationErrors(problems)));
    }
    check_artifact_wiring(plan, warnings);
    Ok(())
}

/// A task that `needs` an artifact should depend — directly or transitively —
/// on its producer, or execution order cannot guarantee the artifact exists.
/// The plan is frozen (§5), so this warns rather than inventing edges.
fn check_artifact_wiring(plan: &Plan, warnings: &mut Vec<String>) {
    let index = index_by_id(plan);
    for task in &plan.tasks {
        for needed in &task.artifacts_in {
            let producer = plan
                .artifacts
                .iter()
                .find(|a| a.id == *needed)
                .and_then(|a| a.produced_by.as_ref());
            // Unknown producers already warned during parsing.
            let Some(producer) = producer else { continue };
            if *producer != task.id && !depends_transitively(&index, &task.id, producer) {
                warnings.push(format!(
                    "task `{}` needs artifact `{needed}` produced by `{producer}` but does not \
                     depend on it (directly or transitively)",
                    task.id
                ));
            }
        }
    }
}

/// Id → task, built once per pass and shared by the graph checks.
fn index_by_id(plan: &Plan) -> BTreeMap<&str, &Task> {
    plan.tasks.iter().map(|t| (t.id.as_str(), t)).collect()
}

fn depends_transitively(index: &BTreeMap<&str, &Task>, from: &TaskId, target: &TaskId) -> bool {
    let mut queue: Vec<&TaskId> = index
        .get(from.as_str())
        .map(|t| t.depends_on.iter().collect())
        .unwrap_or_default();
    let mut visited: Vec<&str> = Vec::new();
    while let Some(dep) = queue.pop() {
        if dep == target {
            return true;
        }
        if visited.contains(&dep.as_str()) {
            continue;
        }
        visited.push(dep.as_str());
        if let Some(task) = index.get(dep.as_str()) {
            queue.extend(task.depends_on.iter());
        }
    }
    false
}

fn find_cycle(plan: &Plan) -> Option<Vec<String>> {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let index: BTreeMap<&str, usize> = plan
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();

    fn dfs(
        current: usize,
        plan: &Plan,
        index: &BTreeMap<&str, usize>,
        color: &mut [u8],
        stack: &mut Vec<usize>,
    ) -> Option<Vec<String>> {
        color[current] = GRAY;
        stack.push(current);
        for dep in &plan.tasks[current].depends_on {
            let Some(&next) = index.get(dep.as_str()) else {
                continue;
            };
            if color[next] == GRAY {
                let from = stack.iter().position(|&i| i == next).unwrap_or(0);
                let mut cycle: Vec<String> = stack[from..]
                    .iter()
                    .map(|&i| plan.tasks[i].id.to_string())
                    .collect();
                cycle.push(plan.tasks[next].id.to_string());
                return Some(cycle);
            }
            if color[next] == WHITE {
                if let Some(cycle) = dfs(next, plan, index, color, stack) {
                    return Some(cycle);
                }
            }
        }
        stack.pop();
        color[current] = BLACK;
        None
    }

    let mut color = vec![WHITE; plan.tasks.len()];
    let mut stack = Vec::new();
    for start in 0..plan.tasks.len() {
        if color[start] == WHITE {
            if let Some(cycle) = dfs(start, plan, &index, &mut color, &mut stack) {
                return Some(cycle);
            }
        }
    }
    None
}
