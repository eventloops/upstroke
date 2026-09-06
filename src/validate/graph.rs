//! Extended notes: `docs/internals/validate/graph.md`

#![deny(clippy::disallowed_methods)]
#![deny(clippy::indexing_slicing, clippy::unreachable)]

use std::collections::{BTreeMap, BTreeSet};
use std::slice;

use crate::error::{UpstrokeError, ValidationErrors};
use crate::ir::{Plan, Task, TaskId};

pub(super) fn check_graph(plan: &Plan, warnings: &mut Vec<String>) -> Result<(), UpstrokeError> {
    let mut problems = Vec::new();
    let mut occurrences: BTreeMap<&str, usize> = BTreeMap::new();
    for task in &plan.tasks {
        *occurrences.entry(task.id.as_str()).or_insert(0) += 1;
    }
    for (id, count) in &occurrences {
        if *count > 1 {
            problems.push(format!("duplicate task id `{id}` ({count} tasks share it)"));
        }
    }
    for task in &plan.tasks {
        for dep in &task.depends_on {
            if !occurrences.contains_key(dep.as_str()) {
                problems.push(format!("task `{}` depends on unknown id `{dep}`", task.id));
            }
        }
    }
    if !problems.is_empty() {
        return Err(UpstrokeError::Validation(ValidationErrors(problems)));
    }
    let index = index_by_id(plan);
    if let Some(cycle) = find_cycle(plan, &index) {
        let problem = format!("dependency cycle: {}", cycle.join(" -> "));
        return Err(UpstrokeError::Validation(ValidationErrors(vec![problem])));
    }
    check_artifact_wiring(plan, &index, warnings);
    Ok(())
}

fn index_by_id(plan: &Plan) -> BTreeMap<&str, &Task> {
    plan.tasks.iter().map(|t| (t.id.as_str(), t)).collect()
}

fn find_cycle(plan: &Plan, index: &BTreeMap<&str, &Task>) -> Option<Vec<String>> {
    let mut finished: BTreeSet<&str> = BTreeSet::new();
    let mut path: Vec<(&str, slice::Iter<'_, TaskId>)> = Vec::new();
    let mut on_path: BTreeSet<&str> = BTreeSet::new();
    for root in &plan.tasks {
        if finished.contains(root.id.as_str()) {
            continue;
        }
        path.push((root.id.as_str(), root.depends_on.iter()));
        on_path.insert(root.id.as_str());
        while let Some((current, pending)) = path.last_mut() {
            let Some(dep) = pending.next() else {
                let current = *current;
                finished.insert(current);
                on_path.remove(current);
                path.pop();
                continue;
            };
            let dep = dep.as_str();
            if finished.contains(dep) {
                continue;
            }
            if on_path.contains(dep) {
                let mut cycle: Vec<String> = path
                    .iter()
                    .skip_while(|(id, _)| *id != dep)
                    .map(|(id, _)| (*id).to_owned())
                    .collect();
                cycle.push(dep.to_owned());
                return Some(cycle);
            }
            let Some(task) = index.get(dep) else {
                continue;
            };
            path.push((dep, task.depends_on.iter()));
            on_path.insert(dep);
        }
    }
    None
}

fn check_artifact_wiring(plan: &Plan, index: &BTreeMap<&str, &Task>, warnings: &mut Vec<String>) {
    for task in &plan.tasks {
        for needed in &task.artifacts_in {
            let producer = plan
                .artifacts
                .iter()
                .find(|a| a.id == *needed)
                .and_then(|a| a.produced_by.as_ref());
            let Some(producer) = producer else { continue };
            if *producer != task.id && !depends_transitively(index, task, producer) {
                warnings.push(format!(
                    "task `{}` needs artifact `{needed}` produced by `{producer}` but does not \
                     depend on it (directly or transitively)",
                    task.id
                ));
            }
        }
    }
}

fn depends_transitively(index: &BTreeMap<&str, &Task>, task: &Task, target: &TaskId) -> bool {
    let mut pending: Vec<&TaskId> = task.depends_on.iter().collect();
    let mut expanded: BTreeSet<&str> = BTreeSet::new();
    while let Some(dep) = pending.pop() {
        if dep == target {
            return true;
        }
        if !expanded.insert(dep.as_str()) {
            continue;
        }
        if let Some(next) = index.get(dep.as_str()) {
            pending.extend(next.depends_on.iter());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::check_graph;
    use crate::ir::{Artifact, ArtifactId, Plan, PlanSource, Task, TaskId, TaskKind};

    fn task(id: &str, depends_on: &[&str]) -> Task {
        Task {
            id: TaskId::from(id),
            kind: TaskKind::Implement,
            title: id.to_owned(),
            body: String::new(),
            depends_on: depends_on.iter().copied().map(TaskId::from).collect(),
            acceptance: Vec::new(),
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::new(),
        }
    }

    fn needing(id: &str, depends_on: &[&str], needs: &str) -> Task {
        let mut task = task(id, depends_on);
        task.artifacts_in.push(ArtifactId::from(needs));
        task
    }

    fn producing(id: &str, depends_on: &[&str], out: &str) -> Task {
        let mut task = task(id, depends_on);
        task.artifacts_out.push(ArtifactId::from(out));
        task
    }

    fn plan(tasks: Vec<Task>, artifacts: Vec<Artifact>) -> Plan {
        Plan {
            source: PlanSource {
                adapter: "test".to_owned(),
                hash: String::new(),
            },
            tasks,
            artifacts,
        }
    }

    fn produced(artifact: &str, by: &str) -> Artifact {
        Artifact {
            id: ArtifactId::from(artifact),
            produced_by: Some(TaskId::from(by)),
        }
    }

    fn check(plan: &Plan) -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();
        check_graph(plan, &mut warnings)
            .map(|()| warnings)
            .map_err(|e| e.to_string())
    }

    fn refusal(plan: &Plan) -> String {
        check(plan).expect_err("the plan must be refused")
    }

    #[test]
    fn a_task_that_depends_on_itself_is_a_cycle_of_one() {
        let plan = plan(vec![task("a", &["a"])], Vec::new());
        assert_eq!(
            refusal(&plan),
            "plan validation failed:\n  - dependency cycle: a -> a"
        );
    }

    #[test]
    fn a_cycle_entered_from_a_tail_is_reported_from_its_first_repeated_task() {
        let plan = plan(
            vec![task("x", &["c"]), task("c", &["b"]), task("b", &["c"])],
            Vec::new(),
        );
        assert_eq!(
            refusal(&plan),
            "plan validation failed:\n  - dependency cycle: c -> b -> c"
        );
    }

    #[test]
    fn a_cycle_the_first_task_does_not_reach_is_still_found_from_the_first_task_on_it() {
        let plan = plan(
            vec![task("z", &[]), task("c", &["b"]), task("b", &["c"])],
            Vec::new(),
        );
        assert_eq!(
            refusal(&plan),
            "plan validation failed:\n  - dependency cycle: c -> b -> c"
        );
    }

    #[test]
    fn a_diamond_is_not_a_cycle_and_reaches_the_shared_task_twice() {
        let plan = plan(
            vec![
                task("a", &["b", "c"]),
                task("b", &["d"]),
                task("c", &["d"]),
                task("d", &[]),
            ],
            Vec::new(),
        );
        assert_eq!(check(&plan), Ok(Vec::new()));
    }

    #[test]
    fn duplicates_and_unknown_targets_are_reported_together_and_before_any_cycle() {
        let plan = plan(
            vec![task("a", &["ghost"]), task("b", &[]), task("b", &["b"])],
            Vec::new(),
        );
        assert_eq!(
            refusal(&plan),
            "plan validation failed:\n  - duplicate task id `b` (2 tasks share it)\n  - task `a` \
             depends on unknown id `ghost`"
        );
    }

    #[test]
    fn a_transitive_dependency_on_the_producer_satisfies_the_wiring() {
        let plan = plan(
            vec![
                producing("d", &[], "contract"),
                task("c", &["d"]),
                needing("b", &["c"], "contract"),
            ],
            vec![produced("contract", "d")],
        );
        assert_eq!(check(&plan), Ok(Vec::new()));
    }

    #[test]
    fn a_needed_artifact_from_a_task_not_depended_on_is_warned_about() {
        let plan = plan(
            vec![
                producing("d", &[], "contract"),
                needing("b", &[], "contract"),
            ],
            vec![produced("contract", "d")],
        );
        assert_eq!(
            check(&plan),
            Ok(vec![
                "task `b` needs artifact `contract` produced by `d` but does not depend on it \
                 (directly or transitively)"
                    .to_owned()
            ])
        );
    }

    #[test]
    fn a_task_that_needs_what_it_is_recorded_as_producing_is_not_warned_about() {
        let mut d = producing("d", &[], "contract");
        d.artifacts_in.push(ArtifactId::from("contract"));
        let plan = plan(vec![d], vec![produced("contract", "d")]);
        assert_eq!(check(&plan), Ok(Vec::new()));
    }

    #[test]
    fn a_task_needing_and_declaring_what_an_earlier_task_also_declares_is_not_warned_about_through_the_adapter()
     {
        let raw = "## D1\n<!-- upstroke: id=d1 depends=d2 needs=contract out=contract -->\n\n\
                   ## D2\n<!-- upstroke: id=d2 depends= out=contract -->\n";
        let parsed = crate::plan::detect(raw)
            .expect("markdown is recognised")
            .parse_with_warnings(raw)
            .expect("the plan parses");
        let producers: Vec<String> = parsed
            .plan
            .artifacts
            .iter()
            .map(|a| {
                format!(
                    "{} by {:?}",
                    a.id,
                    a.produced_by.as_ref().map(TaskId::as_str)
                )
            })
            .collect();
        assert_eq!(producers, vec!["contract by Some(\"d1\")".to_owned()]);
        assert_eq!(parsed.warnings, Vec::<String>::new());
        assert_eq!(check(&parsed.plan), Ok(Vec::new()));
    }

    #[test]
    fn an_artifact_with_no_producer_wires_nothing() {
        let unproduced = Artifact {
            id: ArtifactId::from("contract"),
            produced_by: None,
        };
        let plan = plan(vec![needing("b", &[], "contract")], vec![unproduced]);
        assert_eq!(check(&plan), Ok(Vec::new()));
    }

    #[test]
    fn the_cycle_search_needs_no_call_stack_for_a_long_chain() {
        const LENGTH: usize = 50_000;
        let tasks: Vec<Task> = (0..LENGTH)
            .map(|i| {
                let next = (i + 1) % LENGTH;
                task(&format!("t{i}"), &[&format!("t{next}")])
            })
            .collect();
        let plan = plan(tasks, Vec::new());
        let refusal = thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || refusal(&plan))
            .expect("the checking thread spawns")
            .join()
            .expect("the checking thread completes");
        assert!(
            refusal.starts_with("plan validation failed:\n  - dependency cycle: t0 -> t1 -> "),
            "got: {}",
            refusal.get(..80).unwrap_or(&refusal)
        );
        assert!(refusal.ends_with(" -> t0"), "the report closes the cycle");
    }
}
