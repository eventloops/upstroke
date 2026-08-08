//! Markdown plan adapter (DESIGN.md §9).
//!
//! `##`/`###` sections become tasks (heading → title, prose → body). A plan
//! with no such sections falls back to top-level checklist items or numbered
//! plan-mode steps. The `<!-- tactus: ... -->` annotation grammar overrides
//! the heuristics;
//! annotations are read from pulldown-cmark HTML events, never regexed out of
//! raw text. Unknown annotation attributes warn and never error.

use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::{Parsed, PlanAdapter};
use crate::error::TactusError;
use crate::ir::{
    Artifact, ArtifactId, Plan, PlanSource, Task, TaskId, TaskKind, Tier, content_hash,
};

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

    fn parse_with_warnings(&self, raw: &str) -> Result<Parsed, TactusError> {
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
            return Err(TactusError::Parse {
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

/// `1. step` / `1) step` — Claude Code plan mode often writes numbered steps.
fn is_ordered_item(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let rest = &line[digits..];
    digits > 0 && (rest.starts_with(". ") || rest.starts_with(") "))
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

struct Section {
    title: String,
    /// Byte range of the section body in the original text: from the end of
    /// the heading block to the start of the next `##`/`###` heading.
    content: Range<usize>,
}

fn split_sections(raw: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    // (accumulated title text, heading block span)
    let mut in_heading: Option<(String, Range<usize>)> = None;
    for (event, range) in Parser::new_ext(raw, md_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2 | HeadingLevel::H3,
                ..
            }) => {
                in_heading = Some((String::new(), range));
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2 | HeadingLevel::H3)) => {
                if let Some((title, span)) = in_heading.take() {
                    if let Some(prev) = sections.last_mut() {
                        prev.content.end = span.start;
                    }
                    sections.push(Section {
                        title: title.trim().to_owned(),
                        content: span.end..raw.len(),
                    });
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((buf, _)) = in_heading.as_mut() {
                    buf.push_str(&t);
                }
            }
            _ => {}
        }
    }
    sections
}

// ---------------------------------------------------------------------------
// Drafts: one per task-to-be, before ids/deps are finalized
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Draft {
    title: String,
    body: String,
    acceptance: Vec<String>,
    hints: Vec<String>,
    ann: Annotation,
    ann_found: bool,
}

fn section_draft(raw: &str, section: &Section, warnings: &mut Vec<String>) -> Draft {
    let slice = &raw[section.content.clone()];
    let ctx = format!("section `{}`", section.title);
    let mut draft = Draft {
        title: section.title.clone(),
        ..Draft::default()
    };
    // Spans of tactus annotation comments (slice-relative), removed from body.
    let mut annotation_spans: Vec<Range<usize>> = Vec::new();

    let mut para_text = String::new();
    let mut in_para = false;
    // An `Acceptance:`-style paragraph arms the next top-level list.
    let mut armed = false;
    let mut acceptance_list_depth = 0usize;
    let mut item_text = String::new();
    let mut in_item = false;

    for (event, range) in Parser::new_ext(slice, md_options()).into_offset_iter() {
        match event {
            Event::Html(t) | Event::InlineHtml(t) => {
                for comment in comments_in(&t) {
                    let Some(body) = comment.inner.trim().strip_prefix("tactus:") else {
                        continue; // ordinary HTML comments are author content
                    };
                    if draft.ann_found {
                        warnings.push(format!(
                            "multiple tactus annotations in {ctx}; using the first"
                        ));
                    } else {
                        draft.ann = parse_annotation(body, &ctx, warnings);
                        draft.ann_found = true;
                    }
                    annotation_spans
                        .push(range.start + comment.span.start..range.start + comment.span.end);
                }
            }
            Event::Start(Tag::Paragraph) => {
                in_para = true;
                para_text.clear();
                armed = false;
            }
            Event::End(TagEnd::Paragraph) => {
                in_para = false;
                let text = para_text.trim().trim_end_matches(':').trim_end();
                if is_acceptance_header(text) {
                    armed = true;
                }
            }
            Event::Start(Tag::List(_)) => {
                if armed || acceptance_list_depth > 0 {
                    acceptance_list_depth += 1;
                }
                armed = false;
            }
            Event::End(TagEnd::List(_)) => {
                acceptance_list_depth = acceptance_list_depth.saturating_sub(1);
            }
            Event::Start(Tag::Item) => {
                if acceptance_list_depth > 0 {
                    in_item = true;
                    item_text.clear();
                }
            }
            Event::End(TagEnd::Item) => {
                if acceptance_list_depth > 0 && in_item {
                    let text = item_text.trim();
                    if !text.is_empty() {
                        draft.acceptance.push(text.to_owned());
                    }
                    in_item = false;
                }
            }
            Event::Start(_) => {
                armed = false;
            }
            Event::Text(t) => {
                if in_item {
                    item_text.push_str(&t);
                }
                if in_para {
                    para_text.push_str(&t);
                }
                collect_text_hints(&t, &mut draft.hints);
            }
            Event::Code(t) => {
                if in_item {
                    item_text.push_str(&t);
                }
                if in_para {
                    para_text.push_str(&t);
                }
                collect_code_hint(&t, &mut draft.hints);
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_item {
                    item_text.push(' ');
                }
                if in_para {
                    para_text.push(' ');
                }
            }
            _ => {}
        }
    }

    draft.body = strip_spans(slice, &annotation_spans).trim().to_owned();
    draft
}

fn is_acceptance_header(text: &str) -> bool {
    ["acceptance", "done when", "success criteria"]
        .iter()
        .any(|h| text.eq_ignore_ascii_case(h))
}

/// Fallback when a plan has no `##`/`###` sections: top-level checklist items
/// (`- [ ]` / `- [x]`) and ordered-list steps (`1.` — the common Claude Code
/// plan-mode shape) become tasks. Plain unordered bullets do not; prose lists
/// would false-positive. Nested content joins the body.
fn checklist_drafts(raw: &str, warnings: &mut Vec<String>) -> Vec<Draft> {
    let mut drafts = Vec::new();
    let mut list_depth = 0usize;
    let mut item_depth = 0usize;
    let mut current: Option<Draft> = None;
    let mut is_task_item = false;
    let mut top_list_ordered = false;

    for (event, _range) in Parser::new_ext(raw, md_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::List(start)) => {
                if list_depth == 0 {
                    top_list_ordered = start.is_some();
                }
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => {
                item_depth += 1;
                if list_depth == 1 && item_depth == 1 {
                    current = Some(Draft::default());
                    is_task_item = top_list_ordered;
                }
            }
            Event::End(TagEnd::Item) => {
                if list_depth == 1
                    && item_depth == 1
                    && let Some(mut draft) = current.take()
                {
                    draft.title = draft.title.trim().to_owned();
                    draft.body = draft.body.trim().to_owned();
                    if is_task_item && !draft.title.is_empty() {
                        drafts.push(draft);
                    }
                }
                item_depth = item_depth.saturating_sub(1);
            }
            Event::TaskListMarker(_) => {
                if list_depth == 1 && item_depth == 1 {
                    is_task_item = true;
                }
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                if let Some(draft) = current.as_mut() {
                    for comment in comments_in(&t) {
                        let Some(body) = comment.inner.trim().strip_prefix("tactus:") else {
                            continue;
                        };
                        if draft.ann_found {
                            warnings.push(
                                "multiple tactus annotations in checklist item; using the first"
                                    .to_owned(),
                            );
                        } else {
                            draft.ann = parse_annotation(body, "checklist item", warnings);
                            draft.ann_found = true;
                        }
                    }
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(draft) = current.as_mut() {
                    if list_depth == 1 && item_depth == 1 {
                        draft.title.push_str(&t);
                    } else {
                        draft.body.push_str(&t);
                        draft.body.push(' ');
                    }
                    collect_text_hints(&t, &mut draft.hints);
                }
            }
            _ => {}
        }
    }
    drafts
}

// ---------------------------------------------------------------------------
// Annotation grammar
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Annotation {
    id: Option<String>,
    kind: Option<TaskKind>,
    /// `Some(vec![])` means `depends=` — explicitly no dependencies, breaking
    /// the document-order default chain. `None` means the attribute is absent.
    depends: Option<Vec<String>>,
    tier: Option<Tier>,
    min: Option<Tier>,
    needs: Vec<String>,
    out: Vec<String>,
    paths: Vec<String>,
}

fn parse_annotation(body: &str, ctx: &str, warnings: &mut Vec<String>) -> Annotation {
    let mut ann = Annotation::default();
    for token in body.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            warnings.push(format!(
                "malformed annotation attribute `{token}` in {ctx} (expected key=value); ignored"
            ));
            continue;
        };
        match key {
            "id" => ann.id = non_empty(value),
            "kind" => match TaskKind::parse(value) {
                Some(kind) => ann.kind = Some(kind),
                None => warnings.push(format!(
                    "unknown kind `{value}` in {ctx}; falling back to heuristics"
                )),
            },
            "depends" => ann.depends = Some(csv(value)),
            "tier" => match Tier::parse(value) {
                Some(tier) => ann.tier = Some(tier),
                None => warnings.push(format!("unknown tier `{value}` in {ctx}; ignored")),
            },
            "min" => match Tier::parse(value) {
                Some(tier) => ann.min = Some(tier),
                None => warnings.push(format!("unknown min tier `{value}` in {ctx}; ignored")),
            },
            "needs" => ann.needs = csv(value),
            "out" => ann.out = csv(value),
            "paths" => ann.paths = csv(value),
            _ => warnings.push(format!(
                "unknown annotation attribute `{key}` in {ctx}; ignored"
            )),
        }
    }
    ann
}

fn csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

struct HtmlComment<'a> {
    /// Span within the HTML event text, including the delimiters.
    span: Range<usize>,
    inner: &'a str,
}

fn comments_in(html: &str) -> Vec<HtmlComment<'_>> {
    let mut comments = Vec::new();
    let mut pos = 0;
    while let Some(open_rel) = html[pos..].find("<!--") {
        let open = pos + open_rel;
        let inner_start = open + 4;
        let Some(close_rel) = html[inner_start..].find("-->") else {
            break;
        };
        let close = inner_start + close_rel;
        comments.push(HtmlComment {
            span: open..close + 3,
            inner: &html[inner_start..close],
        });
        pos = close + 3;
    }
    comments
}

fn strip_spans(slice: &str, spans: &[Range<usize>]) -> String {
    let mut out = String::with_capacity(slice.len());
    let mut pos = 0;
    for span in spans {
        if span.start > pos {
            out.push_str(&slice[pos..span.start]);
        }
        pos = pos.max(span.end);
    }
    out.push_str(&slice[pos..]);
    out
}

// ---------------------------------------------------------------------------
// Path-hint heuristics
// ---------------------------------------------------------------------------

const HINT_EXTENSIONS: &[&str] = &[
    ".rs", ".md", ".toml", ".json", ".yml", ".yaml", ".txt", ".py", ".ts", ".js", ".lock",
];

fn has_hint_extension(token: &str) -> bool {
    HINT_EXTENSIONS.iter().any(|ext| token.ends_with(ext))
}

fn collect_text_hints(text: &str, hints: &mut Vec<String>) {
    for word in text.split_whitespace() {
        let token = word.trim_matches(|c: char| {
            matches!(
                c,
                ',' | '.' | ';' | ':' | '(' | ')' | '`' | '"' | '\'' | '!' | '?'
            )
        });
        if token.contains('/')
            && !token.contains("://")
            && (has_hint_extension(token) || token.contains('*') || token.matches('/').count() >= 2)
        {
            push_unique(hints, token);
        }
    }
}

fn collect_code_hint(code: &str, hints: &mut Vec<String>) {
    let token = code.trim();
    if token.contains(' ') || token.contains("://") {
        return;
    }
    if token.contains('/') || has_hint_extension(token) {
        push_unique(hints, token);
    }
}

fn push_unique(items: &mut Vec<String>, candidate: &str) {
    if !candidate.is_empty() && !items.iter().any(|i| i == candidate) {
        items.push(candidate.to_owned());
    }
}

// ---------------------------------------------------------------------------
// Assembly: ids, kinds, dependencies, artifacts
// ---------------------------------------------------------------------------

fn assemble(drafts: Vec<Draft>) -> Vec<Task> {
    // Reserve explicit ids first so derived slugs never collide with them.
    // Explicit duplicates are left intact for validation to report.
    let mut taken: Vec<String> = drafts.iter().filter_map(|d| d.ann.id.clone()).collect();
    let mut previous_id: Option<TaskId> = None;
    let mut tasks = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let id = match draft.ann.id.clone() {
            Some(explicit) => explicit,
            None => unique_slug(&draft.title, &mut taken),
        };
        let kind = draft
            .ann
            .kind
            .unwrap_or_else(|| heuristic_kind(&draft.title));
        let depends_on: Vec<TaskId> = match &draft.ann.depends {
            Some(ids) => ids.iter().map(|s| TaskId::from(s.as_str())).collect(),
            None => previous_id.clone().into_iter().collect(),
        };
        let mut path_hints = draft.ann.paths.clone();
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
            suggested_tier: draft.ann.tier,
            min_tier: draft.ann.min,
            artifacts_in: draft
                .ann
                .needs
                .iter()
                .map(|s| ArtifactId::from(s.as_str()))
                .collect(),
            artifacts_out: draft
                .ann
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
fn collect_artifacts(tasks: &mut [Task], warnings: &mut Vec<String>) -> Vec<Artifact> {
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
    if artifacts.is_empty()
        && let Some(design) = tasks.iter_mut().find(|t| t.kind == TaskKind::Design)
    {
        let id = ArtifactId::from("conventions-brief");
        design.artifacts_out.push(id.clone());
        artifacts.push(Artifact {
            id,
            produced_by: Some(design.id.clone()),
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Parsed {
        MarkdownPlanAdapter
            .parse_with_warnings(raw)
            .expect("plan should parse")
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(std::path::Path::new("fixtures").join(name))
            .expect("fixture should exist")
    }

    #[test]
    fn annotated_sample_fixture_parses_fully() {
        let parsed = parse(&fixture("sample-plan.md"));
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
            !api.body.contains("tactus:"),
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
        let parsed = parse(&fixture("bare-plan.md"));
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

        // Document-order dependencies: task N depends on task N-1.
        assert!(tasks[0].depends_on.is_empty());
        for pair in tasks.windows(2) {
            assert_eq!(pair[1].depends_on, [pair[0].id.clone()]);
        }

        // Bare plan with a Design task gets the default conventions brief.
        assert_eq!(parsed.plan.artifacts.len(), 1);
        assert_eq!(
            parsed.plan.artifacts[0].id,
            ArtifactId::from("conventions-brief")
        );
    }

    #[test]
    fn unknown_attribute_warns_but_still_parses() {
        let parsed = parse("## Fix the thing\n<!-- tactus: id=fix-1 wibble=frob -->\n");
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
        let parsed = parse("## Task one\n<!-- tactus: standalone tier=galactic kind=wat -->\n");
        let joined = parsed.warnings.join("\n");
        assert!(joined.contains("malformed annotation attribute `standalone`"));
        assert!(joined.contains("unknown tier `galactic`"));
        assert!(joined.contains("unknown kind `wat`"));
        // Bad values fall back rather than erroring.
        assert_eq!(parsed.plan.tasks[0].suggested_tier, None);
    }

    #[test]
    fn empty_depends_is_explicit_none_and_absent_is_document_order() {
        let raw = "## First\n\n## Second\n<!-- tactus: depends= -->\n\n## Third\n";
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

- [ ] Design the widget API <!-- tactus: id=widget-api tier=frontier -->
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
            !tasks[0].title.contains("tactus"),
            "annotation not in title"
        );
    }

    #[test]
    fn ordered_step_plans_become_tasks() {
        let parsed = parse(&fixture("steps-plan.md"));
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
        let parsed = parse("## Build it\n<!-- tactus: needs=ghost-contract -->\n");
        assert!(parsed.warnings.iter().any(|w| w.contains("ghost-contract")));
    }

    #[test]
    fn path_hints_collected_from_body_and_annotation() {
        let parsed = parse(
            "## Implement cursor codec\n<!-- tactus: paths=src/api/** -->\nTouch `src/api/cursor.rs` and src/api/mod.rs while at it.\n",
        );
        let hints = &parsed.plan.tasks[0].path_hints;
        assert_eq!(hints[0], "src/api/**", "annotation paths come first");
        assert!(hints.iter().any(|h| h == "src/api/cursor.rs"));
        assert!(hints.iter().any(|h| h == "src/api/mod.rs"));
    }

    #[test]
    fn sniff_accepts_markdown_shapes() {
        assert!(MarkdownPlanAdapter.sniff("# Title\n"));
        assert!(MarkdownPlanAdapter.sniff("- [ ] item\n"));
        assert!(!MarkdownPlanAdapter.sniff("{\"tasks\": []}"));
    }
}
