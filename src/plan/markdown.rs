//! Extended notes: `docs/internals/plan/markdown.md`

mod annotation;
mod assemble;
mod drafts;
mod hints;
mod sections;

use pulldown_cmark::Options;

use super::{Parsed, PlanAdapter};
use crate::error::UpstrokeError;
use crate::ir::{Plan, PlanSource, content_hash};

use assemble::{assemble, collect_artifacts};
use drafts::{Draft, checklist_drafts, section_draft};
use sections::split_sections;

pub const ADAPTER_ID: &str = "markdown";

pub struct MarkdownPlanAdapter;

impl PlanAdapter for MarkdownPlanAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }

    fn sniff(&self, raw: &str) -> bool {
        raw.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with('#') || t.starts_with("- [") || is_ordered_item(t)
        })
    }

    fn parse_with_warnings(&self, raw: &str) -> Result<Parsed, UpstrokeError> {
        let mut warnings = Vec::new();
        let sections = split_sections(raw);
        let drafts: Vec<Draft> = if sections.is_empty() {
            checklist_drafts(raw, &mut warnings)
        } else {
            sections
                .iter()
                .map(|s| section_draft(raw, s, &mut warnings))
                .collect()
        };
        if drafts.is_empty() {
            return Err(UpstrokeError::Parse {
                message: "no tasks found: expected `##`/`###` sections, a top-level checklist, \
                          or numbered steps"
                    .to_owned(),
            });
        }
        let mut tasks = assemble(drafts);
        let artifacts = collect_artifacts(&mut tasks, &mut warnings);
        Ok(Parsed {
            plan: Plan {
                source: PlanSource {
                    adapter: ADAPTER_ID.to_owned(),
                    hash: content_hash(raw.as_bytes()),
                },
                tasks,
                artifacts,
            },
            warnings,
        })
    }
}

fn md_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TASKLISTS);
    options
}

fn is_ordered_item(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let rest = &line[digits..];
    digits > 0 && (rest.starts_with(". ") || rest.starts_with(") "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ArtifactId, TaskId, TaskKind, Tier};
    use crate::plan::corpus;

    fn parse(raw: &str) -> Parsed {
        MarkdownPlanAdapter
            .parse_with_warnings(raw)
            .expect("plan should parse")
    }

    #[test]
    fn annotated_sample_fixture_parses_fully() {
        let parsed = parse(corpus::SAMPLE_PLAN);
        let tasks = &parsed.plan.tasks;
        assert_eq!(tasks.len(), 4);

        let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["api-design", "cursors", "fix-obo", "docs"]);

        let api = &tasks[0];
        assert_eq!(api.kind, TaskKind::Design);
        assert!(
            api.depends_on.is_empty(),
            "depends= breaks the default chain"
        );
        assert_eq!(api.suggested_tier, Some(Tier::Frontier));
        assert_eq!(api.artifacts_out, [ArtifactId::from("api-contract")]);
        assert_eq!(api.acceptance.len(), 2);
        assert!(api.acceptance[0].contains("Cursor format"));
        assert!(api.body.contains("Define cursor format"));
        assert!(
            !api.body.contains("upstroke:"),
            "annotation stripped from body"
        );

        let cursors = &tasks[1];
        assert_eq!(cursors.depends_on, [TaskId::from("api-design")]);
        assert_eq!(cursors.artifacts_in, [ArtifactId::from("api-contract")]);
        assert!(cursors.path_hints.iter().any(|p| p == "src/api/**"));

        let fix = &tasks[2];
        assert_eq!(fix.kind, TaskKind::Fix);
        assert_eq!(fix.min_tier, Some(Tier::Mid));

        assert_eq!(parsed.plan.artifacts.len(), 1);
        assert_eq!(
            parsed.plan.artifacts[0].produced_by,
            Some(TaskId::from("api-design"))
        );
        assert!(
            parsed.warnings.is_empty(),
            "no warnings expected: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn bare_fixture_uses_heuristics() {
        let parsed = parse(corpus::BARE_PLAN);
        let tasks = &parsed.plan.tasks;
        assert_eq!(tasks.len(), 5);

        let kinds: Vec<TaskKind> = tasks.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [
                TaskKind::Design,
                TaskKind::Implement,
                TaskKind::Fix,
                TaskKind::Test,
                TaskKind::Docs,
            ]
        );

        assert!(tasks[0].depends_on.is_empty());
        for pair in tasks.windows(2) {
            assert_eq!(pair[1].depends_on, [pair[0].id.clone()]);
        }

        assert_eq!(parsed.plan.artifacts.len(), 1);
        assert_eq!(
            parsed.plan.artifacts[0].id,
            ArtifactId::from("conventions-brief")
        );
    }

    #[test]
    fn unknown_attribute_warns_but_still_parses() {
        let parsed = parse("## Fix the thing\n<!-- upstroke: id=fix-1 wibble=frob -->\n");
        assert_eq!(parsed.plan.tasks[0].id, TaskId::from("fix-1"));
        assert!(
            parsed
                .warnings
                .iter()
                .any(|w| w.contains("unknown annotation attribute `wibble`")),
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn malformed_attribute_and_bad_values_warn() {
        let parsed = parse("## Task one\n<!-- upstroke: standalone tier=galactic kind=wat -->\n");
        let joined = parsed.warnings.join("\n");
        assert!(joined.contains("malformed annotation attribute `standalone`"));
        assert!(joined.contains("unknown tier `galactic`"));
        assert!(joined.contains("unknown kind `wat`"));
        assert_eq!(parsed.plan.tasks[0].suggested_tier, None);
    }

    #[test]
    fn empty_depends_is_explicit_none_and_absent_is_document_order() {
        let raw = "## First\n\n## Second\n<!-- upstroke: depends= -->\n\n## Third\n";
        let tasks = parse(raw).plan.tasks;
        assert!(tasks[0].depends_on.is_empty());
        assert!(
            tasks[1].depends_on.is_empty(),
            "depends= must break the chain"
        );
        assert_eq!(tasks[2].depends_on, [tasks[1].id.clone()]);
    }

    #[test]
    fn derived_ids_are_slugged_and_uniquified() {
        let raw = "## Fix the parser\n\n## Fix the parser\n\n## Fix the parser!\n";
        let tasks = parse(raw).plan.tasks;
        let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            ["fix-the-parser", "fix-the-parser-2", "fix-the-parser-3"]
        );
    }

    #[test]
    fn checklist_plans_become_tasks() {
        let raw = "\
# Checklist plan

- [ ] Design the widget API <!-- upstroke: id=widget-api tier=frontier -->
- [x] Implement the widget store
- [ ] Test widget rendering
";
        let parsed = parse(raw);
        let tasks = &parsed.plan.tasks;
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].id, TaskId::from("widget-api"));
        assert_eq!(tasks[0].suggested_tier, Some(Tier::Frontier));
        assert_eq!(tasks[1].kind, TaskKind::Implement);
        assert_eq!(tasks[2].kind, TaskKind::Test);
        assert_eq!(tasks[2].depends_on, [tasks[1].id.clone()]);
        assert!(
            !tasks[0].title.contains("upstroke"),
            "annotation not in title"
        );
    }

    #[test]
    fn ordered_step_plans_become_tasks() {
        let parsed = parse(corpus::STEPS_PLAN);
        let tasks = &parsed.plan.tasks;
        assert_eq!(tasks.len(), 4);
        let kinds: Vec<TaskKind> = tasks.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [
                TaskKind::Design,
                TaskKind::Implement,
                TaskKind::Fix,
                TaskKind::Docs,
            ]
        );
        assert_eq!(tasks[1].depends_on, [tasks[0].id.clone()]);
        assert!(
            tasks[1]
                .path_hints
                .iter()
                .any(|h| h == "src/limit/bucket.rs"),
            "nested bullet feeds hints: {:?}",
            tasks[1].path_hints
        );
    }

    #[test]
    fn plain_unordered_bullets_are_not_tasks() {
        let raw = "# Notes\n\n- just a thought\n- another thought\n";
        let err = MarkdownPlanAdapter
            .parse_with_warnings(raw)
            .expect_err("prose bullets must not become tasks");
        assert!(err.to_string().contains("no tasks found"));
    }

    #[test]
    fn plan_without_tasks_errors() {
        let err = MarkdownPlanAdapter
            .parse_with_warnings("just prose, no structure\n")
            .expect_err("should fail");
        assert!(err.to_string().contains("no tasks found"));
    }

    #[test]
    fn needs_without_producer_warns() {
        let parsed = parse("## Build it\n<!-- upstroke: needs=ghost-contract -->\n");
        assert!(parsed.warnings.iter().any(|w| w.contains("ghost-contract")));
    }

    #[test]
    fn path_hints_collected_from_body_and_annotation() {
        let parsed = parse(
            "## Implement cursor codec\n<!-- upstroke: paths=src/api/** -->\nTouch `src/api/cursor.rs` and src/api/mod.rs while at it.\n",
        );
        let hints = &parsed.plan.tasks[0].path_hints;
        assert_eq!(hints[0], "src/api/**", "annotation paths come first");
        assert!(hints.iter().any(|h| h == "src/api/cursor.rs"));
        assert!(hints.iter().any(|h| h == "src/api/mod.rs"));
    }

    #[test]
    fn nested_acceptance_items_keep_the_parent_criterion() {
        let parsed = parse(
            "## Task\n\nAcceptance:\n- Cursor format documented\n  - covers the base64 form\n- Errors covered\n",
        );
        assert_eq!(
            parsed.plan.tasks[0].acceptance,
            [
                "Cursor format documented",
                "covers the base64 form",
                "Errors covered"
            ],
            "parent and child criteria both survive, in document order"
        );
    }

    #[test]
    fn acceptance_heading_forms_arm_without_becoming_tasks() {
        let parsed = parse(
            "## Implement search\n\nBuild it.\n\n### Acceptance\n- Field list agreed\n\n## Next task\n",
        );
        let tasks = &parsed.plan.tasks;
        assert_eq!(tasks.len(), 2, "`### Acceptance` is not a task: {tasks:?}");
        assert_eq!(tasks[0].acceptance, ["Field list agreed"]);
        assert_eq!(tasks[1].title, "Next task");

        let parsed = parse("## Task\n\n#### Done when\n- it works\n");
        assert_eq!(parsed.plan.tasks[0].acceptance, ["it works"]);
    }

    #[test]
    fn an_invisible_comment_does_not_disarm_acceptance() {
        let parsed = parse(
            "## Task\n<!-- upstroke: id=t1 -->\n\nAcceptance:\n\n<!-- note -->\n\n- still collected\n",
        );
        assert_eq!(parsed.plan.tasks[0].acceptance, ["still collected"]);
    }

    #[test]
    fn headings_inside_containers_are_not_tasks() {
        let raw = "## Review notes\n\n> ## Original proposal\n> keep cursors opaque\n\nOur take.\n";
        let parsed = parse(raw);
        let tasks = &parsed.plan.tasks;
        assert_eq!(
            tasks.len(),
            1,
            "quoted heading must not spawn a task: {tasks:?}"
        );
        assert_eq!(tasks[0].title, "Review notes");
        assert!(
            tasks[0].body.contains("Our take"),
            "body survives past the quote"
        );
    }

    #[test]
    fn multi_line_annotations_are_parsed() {
        let parsed = parse(
            "## Design the API\n<!-- upstroke: id=api-design kind=design\n     depends= tier=frontier -->\nBody text.\n",
        );
        let task = &parsed.plan.tasks[0];
        assert_eq!(task.id, TaskId::from("api-design"));
        assert_eq!(task.kind, TaskKind::Design);
        assert_eq!(task.suggested_tier, Some(Tier::Frontier));
        assert!(task.depends_on.is_empty(), "depends= honored across lines");
        assert!(
            !task.body.contains("upstroke:"),
            "annotation stripped: {}",
            task.body
        );
        assert!(
            parsed.warnings.is_empty(),
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn inline_heading_annotations_are_parsed() {
        let parsed = parse("## Fix the parser <!-- upstroke: id=fix-1 depends= -->\nBody.\n");
        let task = &parsed.plan.tasks[0];
        assert_eq!(task.id, TaskId::from("fix-1"));
        assert!(task.depends_on.is_empty());
        assert!(!task.title.contains("upstroke"), "title: {}", task.title);
    }

    #[test]
    fn wrapped_checklist_titles_keep_word_spacing() {
        let raw =
            "# Plan\n\n1. Implement the token-bucket\n   middleware for the API\n2. Ship it\n";
        let parsed = parse(raw);
        assert_eq!(
            parsed.plan.tasks[0].title,
            "Implement the token-bucket middleware for the API"
        );
        assert_eq!(
            parsed.plan.tasks[0].id,
            TaskId::from("implement-the-token-bucket-middleware-for-the-api")
        );
    }

    #[test]
    fn crlf_plans_parse_identically() {
        let lf = corpus::SAMPLE_PLAN.replace("\r\n", "\n");
        let crlf = lf.replace('\n', "\r\n");
        let from_lf = MarkdownPlanAdapter
            .parse_with_warnings(&lf)
            .expect("lf parses");
        let from_crlf = MarkdownPlanAdapter
            .parse_with_warnings(&crlf)
            .expect("crlf parses");
        assert_eq!(from_lf.plan.source.hash, from_crlf.plan.source.hash);
        let ids =
            |p: &Parsed| -> Vec<String> { p.plan.tasks.iter().map(|t| t.id.to_string()).collect() };
        assert_eq!(ids(&from_lf), ids(&from_crlf));
        assert_eq!(
            from_lf.plan.tasks[0].acceptance,
            from_crlf.plan.tasks[0].acceptance
        );
    }

    #[test]
    fn sniff_accepts_markdown_shapes() {
        assert!(MarkdownPlanAdapter.sniff("# Title\n"));
        assert!(MarkdownPlanAdapter.sniff("- [ ] item\n"));
        assert!(!MarkdownPlanAdapter.sniff("{\"tasks\": []}"));
    }
}
