//! The plan's dependency graph, as `analyze` checks it: duplicate ids, unknown
//! `depends` targets, cycles, and the artifact wiring that only warns.
//!
//! Split out of `super` on 2026-09-03 (`1292653`) and swept on 2026-09-05.
//! `check_graph` is still the single entry point `analyze_captured` calls and
//! every message it produced before the sweep is produced unchanged; what
//! changed is inside. The cycle search is keyed by id rather than by position
//! in `plan.tasks`, and walks an explicit path instead of recursing, so
//! nothing here indexes a collection and nothing grows the call stack with the
//! plan — a chain as long as the plan is a loop, not a recursion, whatever the
//! thread's stack (see `the_cycle_search_needs_no_call_stack_for_a_long_chain`
//! below). Every function is total over any [`Plan`] a caller can build: an
//! unknown id, a `produced_by` of `None` or a duplicate reaches a `continue`
//! or a refusal, never an index, an `unwrap_or` or an `unreachable!`, and the
//! second attribute below is what makes that a build error rather than a
//! reading.
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
// §7's panic surface, mechanised for this file ahead of the crate-wide
// `[lints]` entry `standards/SWEEP.md` says is owed: the sweep left no index,
// slice or `unreachable!` here, and this keeps it so. The tests below are under
// it too, since `clippy.toml` takes no test allowance for either lint yet.
#![deny(clippy::indexing_slicing, clippy::unreachable)]

use std::collections::{BTreeMap, BTreeSet};
use std::slice;

use crate::error::{UpstrokeError, ValidationErrors};
use crate::ir::{Plan, Task, TaskId};

/// Duplicate ids, unknown `depends` targets, then cycles — all collected so a
/// broken plan reports everything in one run. On a clean graph, artifact
/// wiring that contradicts the dependency order is surfaced as warnings.
///
/// Duplicates are listed in id order and unknown targets in document order.
/// The cycle search runs only when both lists are empty: an edge that resolves
/// to no task, or to two, has no dependency order to check, and reporting a
/// cycle through it would name a graph the author never wrote.
///
/// # Errors
///
/// [`UpstrokeError::Validation`] carrying every duplicate id and every unknown
/// dependency, or — on a graph where every id names one task and every edge
/// resolves — one dependency cycle, written as the ids along it with the first
/// repeated at the end (`a -> a` for a task that depends on itself).
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
    // From here every id names exactly one task, which is what makes an
    // id-keyed index a faithful picture of the plan.
    let index = index_by_id(plan);
    if let Some(cycle) = find_cycle(plan, &index) {
        let problem = format!("dependency cycle: {}", cycle.join(" -> "));
        return Err(UpstrokeError::Validation(ValidationErrors(vec![problem])));
    }
    check_artifact_wiring(plan, &index, warnings);
    Ok(())
}

/// Id → task. Faithful only once every id names one task: on a plan with a
/// duplicate the map would keep one of the two tasks and drop the other, which
/// is why `check_graph` refuses duplicates before building it.
fn index_by_id(plan: &Plan) -> BTreeMap<&str, &Task> {
    plan.tasks.iter().map(|t| (t.id.as_str(), t)).collect()
}

/// One dependency cycle as the ids along it, the first repeated at the end, or
/// `None` when the graph has none.
///
/// A depth-first search from each task in document order, following each
/// task's `depends_on` in its own order, so the cycle a plan reports is the
/// same on every run and every platform, and the same one the recursive search
/// this replaced reported. `path` is the chain of tasks the search is inside,
/// outermost first, each with the dependencies it has yet to follow; `on_path`
/// is that chain as a set, and the two change together at the one push and the
/// one pop below, so membership is a lookup rather than a scan. A dependency
/// already on the path closes a cycle, which is the path from that task down
/// plus the edge back to it. A task whose dependencies have all been followed
/// is `finished`, and an edge into a finished task leads into a subgraph
/// already known to be acyclic, so it is not followed again: a diamond is not
/// a cycle, and no task is expanded twice.
///
/// An edge to an id the index lacks is skipped: it belongs to no cycle, and
/// `check_graph` has refused the plan before this runs.
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

/// A task that `needs` an artifact should depend — directly or transitively —
/// on its producer, or execution order cannot guarantee the artifact exists.
/// The plan is frozen (§5), so this warns rather than inventing edges.
///
/// A task that needs an artifact whose recorded producer is the task itself
/// is not warned about. `plan.artifacts` records one producer per artifact,
/// and the markdown adapter records the first task that declares `out=`; a
/// second declaration survives only in that task's `artifacts_out`. So a plan
/// in which an earlier task the needing task depends on also declares the
/// artifact reaches this check with the same recorded producer as a plan in
/// which nothing else declares it, and what that second declaration means —
/// an update, a conflict, an error — `design/09` does not say. The check
/// cannot tell the two apart from the record and stays silent on both rather
/// than guess (`SWEEP-GRAPH-004`, behind the adapter's open question
/// `SWEEP-GRAPH-009`), which is what the base did.
///
/// An artifact no task produces is not in `plan.artifacts` at all — the
/// markdown adapter warns about it while assembling the plan — and one whose
/// `produced_by` is `None` is treated the same way: there is nothing to wire.
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

/// Whether `target` is reachable from `task` along `depends_on` edges. A
/// depth-first walk seeded from the task's own dependencies, each id expanded
/// at most once, so it terminates on any graph — the cycle check has run by
/// the time this is called, but nothing here relies on that. An edge is a
/// match when its id is `target`, whether or not the index has it; an edge to
/// any other id the index lacks is a dead end. `check_graph` refuses a plan
/// with an unknown id before this runs, so both halves of that sentence
/// describe an index that holds every id in play.
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

    /// A task with an id and its dependencies, every other field at rest. A
    /// struct literal rather than a builder so a field added to `Task` fails
    /// here (§12: fixtures derive their field lists from the production type).
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

    /// `task`, and it needs the artifact named.
    fn needing(id: &str, depends_on: &[&str], needs: &str) -> Task {
        let mut task = task(id, depends_on);
        task.artifacts_in.push(ArtifactId::from(needs));
        task
    }

    /// `task`, and it produces the artifact named.
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

    /// The warnings of a plan the check accepts, or the rendered refusal.
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
        // `x` is not on the cycle; the report starts where the path re-enters.
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
        // Roots are taken in document order, not id order: `z` first, then
        // `c`, so the cycle is reported from `c` rather than from `b`.
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
        // The second `b` depends on itself; with `b` duplicated the cycle
        // search does not run, so the refusal lists the two structural
        // problems and nothing else.
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
        // The markdown adapter records `d1` as the one producer of `contract`
        // and keeps `d2`'s claim only in its `artifacts_out`. `d1` depends on
        // `d2` and needs what both declare: accepted input whose meaning the
        // design leaves undefined (`SWEEP-GRAPH-009`), not a proven update. The
        // base is silent on it, and a warning that `d1` needs what it produces
        // itself was the one a pass on `2bbf35b` showed this input producing.
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
        // Fifty thousand tasks, each depending on the next, checked on a
        // thread with a 256 KiB stack: a search that recursed once per task
        // would overflow it long before the end of the chain, and the last
        // task closes the chain into a cycle so the whole of it is walked.
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
