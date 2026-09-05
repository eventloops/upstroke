//! Extended notes: `docs/internals/validate.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods)]

mod graph;
mod render;

use std::fs;
use std::path::{Path, PathBuf};

use crate::agent;
use crate::capacity;
use crate::config::{self, Config};
use crate::error::UpstrokeError;
use crate::gates::{self, ShellGate};
use crate::ir::Plan;
use crate::plan::{self, Parsed};
use crate::review;
use crate::route::{self, ResolvedChain};

#[derive(Debug, Clone)]
pub struct ValidateOptions {
    pub plan_path: PathBuf,
    pub config_path: Option<PathBuf>,
    pub config_root: PathBuf,
    pub pools_path: Option<PathBuf>,
    pub engine_limits: config::EngineLimits,
}

#[derive(Debug)]
pub struct Row {
    pub id: String,
    pub kind: String,
    pub deps: String,
    pub chain: String,
}

#[derive(Debug)]
pub struct Report {
    pub plan: Plan,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
    pub strategy: String,
    pub capacity: String,
    pub review: String,
    pub effort: String,
    pub gates: Vec<String>,
    pub gates_from_config: bool,
}

#[derive(Debug)]
pub struct Analysis {
    pub plan: Plan,
    pub config: Config,
    pub chains: Vec<ResolvedChain>,
    pub gates: Vec<ShellGate>,
    pub gates_from_config: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedInputs {
    plan: config::FileSnapshot,
    config: config::CapturedConfig,
    gate_inputs: Vec<config::FileSnapshot>,
}

const GATE_DERIVATION_INPUTS: &[&str] = &["Cargo.toml", "go.mod", "package.json"];

impl CapturedInputs {
    #[must_use]
    pub fn capture(opts: &ValidateOptions) -> Self {
        Self {
            plan: config::snapshot_file(&opts.plan_path, true),
            config: config::CapturedConfig::capture(
                opts.config_path.as_deref(),
                &opts.config_root,
                opts.pools_path.as_deref(),
            ),
            gate_inputs: GATE_DERIVATION_INPUTS
                .iter()
                .map(|name| config::snapshot_file(&opts.config_root.join(name), false))
                .collect(),
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        std::iter::once(&self.plan)
            .chain(self.config.files())
            .chain(&self.gate_inputs)
            .map(|file| file.path().to_path_buf())
            .collect()
    }
}

pub fn analyze(opts: &ValidateOptions) -> Result<Analysis, UpstrokeError> {
    analyze_captured(&CapturedInputs::capture(opts), opts)
}

pub fn analyze_captured(
    captured: &CapturedInputs,
    opts: &ValidateOptions,
) -> Result<Analysis, UpstrokeError> {
    let raw = captured.plan.text()?.ok_or_else(|| UpstrokeError::Io {
        path: captured.plan.path().to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "plan not found"),
    })?;
    let Parsed {
        plan,
        warnings: mut all_warnings,
    } = plan::detect(&raw)?.parse_with_warnings(&raw)?;
    let config =
        match config::load_captured(&captured.config, opts.engine_limits, &mut all_warnings) {
            Ok(config) => config,
            Err(error) => return Err(error.with_warnings(all_warnings)),
        };
    if let Err(error) = graph::check_graph(&plan, &mut all_warnings) {
        return Err(error.with_warnings(all_warnings));
    }
    let default_config_path = opts.config_root.join("upstroke.toml");
    let config_path = opts.config_path.as_deref().unwrap_or(&default_config_path);
    if let Err(error) = check_pin_adapters(&config.pins, builtin_adapter, config_path) {
        return Err(error.with_warnings(all_warnings));
    }
    let chains: Vec<ResolvedChain> = plan
        .tasks
        .iter()
        .map(|t| route::resolve(t, &config))
        .collect();
    let gates_from_config = config.gates.is_some();
    let gates = match &config.gates {
        Some(configured) => configured
            .iter()
            .map(|g| ShellGate {
                name: g.name.clone(),
                cmd: g.cmd.clone(),
                timeout: g.timeout,
                shell: config.shell,
            })
            .collect(),
        None => gates::derive(&opts.config_root, config.shell),
    };
    Ok(Analysis {
        plan,
        config,
        chains,
        gates,
        gates_from_config,
        warnings: all_warnings,
    })
}

pub fn builtin_adapter(agent: &str) -> bool {
    agent::by_id(agent).is_some()
}

fn adapter_list() -> String {
    agent::ADAPTERS
        .iter()
        .map(|a| a.id())
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_pin_adapters(
    pins: &[config::Pin],
    has_adapter: impl Fn(&str) -> bool,
    config_path: &Path,
) -> Result<(), UpstrokeError> {
    for pin in pins {
        if !has_adapter(&pin.agent) {
            return Err(UpstrokeError::Config {
                path: config_path.to_path_buf(),
                message: format!(
                    "pin for tier `{}` names agent `{}`, which has no adapter in this build \
                     (available: {})",
                    pin.tier,
                    pin.agent,
                    adapter_list()
                ),
            });
        }
    }
    Ok(())
}

pub fn run(opts: &ValidateOptions) -> Result<Report, UpstrokeError> {
    let analysis = analyze(opts)?;
    let mut warnings = analysis.warnings;
    gates::preview_resolution(&analysis.gates, &opts.config_root, &mut warnings);
    let reviews = match review::plan_for(
        &analysis.plan,
        &analysis.chains,
        &analysis.config,
        builtin_adapter,
        &mut warnings,
    ) {
        Ok(reviews) => reviews,
        Err(error) => return Err(error.with_warnings(warnings)),
    };
    let rows = analysis
        .plan
        .tasks
        .iter()
        .zip(analysis.chains)
        .enumerate()
        .map(|(index, (task, chain))| {
            let second = reviews.second_opinion.get(index).and_then(Option::as_ref);
            render::to_row(task, chain, second)
        })
        .collect();
    let (observations, run_id) = latest_run_observations(
        &opts.config_root,
        !analysis.config.pools.is_empty(),
        &mut warnings,
    );
    Ok(Report {
        rows,
        warnings,
        strategy: render::strategy_echo(&analysis.config),
        capacity: render::capacity_echo(&analysis.config, &observations, run_id.as_deref()),
        review: render::review_echo(&reviews),
        effort: render::effort_echo(&analysis.config),
        gates: analysis.gates.into_iter().map(|gate| gate.name).collect(),
        gates_from_config: analysis.gates_from_config,
        plan: analysis.plan,
    })
}

fn latest_run_observations(
    repo_root: &Path,
    has_pools: bool,
    warnings: &mut Vec<String>,
) -> (capacity::Observations, Option<String>) {
    let none = || (capacity::Observations::default(), None);
    if !has_pools {
        return none();
    }
    let Some(run_id) = crate::rundir::latest_run(repo_root) else {
        return none();
    };
    let events_path = crate::rundir::public_dir(repo_root, &run_id).join("events.jsonl");
    let mut ignored = Vec::new();
    match crate::events::read_all(&events_path, &mut ignored) {
        Ok(events) => (capacity::observe(&events), Some(run_id)),
        Err(error) => {
            warnings.push(format!(
                "run {run_id} exists but its event log could not be folded for self-metered \
                 spend ({error}); the capacity block below rests on rate-limit signals alone"
            ));
            none()
        }
    }
}

impl Report {
    pub fn render(&self) -> String {
        render::report(self)
    }

    pub fn write_normalized_json(&self, path: &Path) -> Result<(), UpstrokeError> {
        let json = serde_json::to_string_pretty(&self.plan).map_err(|e| UpstrokeError::Parse {
            message: format!("serializing normalized plan: {e}"),
        })?;
        fs::write(path, json + "\n").map_err(|source| UpstrokeError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::OnceLock;

    fn opts(plan: &str) -> ValidateOptions {
        let hermetic_root =
            env::temp_dir().join(format!("upstroke-validate-hermetic-{}", std::process::id()));
        fs::create_dir_all(&hermetic_root).expect("hermetic root");
        ValidateOptions {
            plan_path: PathBuf::from(plan),
            config_path: None,
            config_root: hermetic_root,
            engine_limits: config::EngineLimits::Fresh,
            pools_path: Some({
                static PATH: OnceLock<PathBuf> = OnceLock::new();
                PATH.get_or_init(|| {
                    let dir = env::temp_dir()
                        .join(format!("upstroke-validate-nopools-{}", std::process::id()));
                    fs::create_dir_all(&dir).expect("scratch dir");
                    let path = dir.join("pools.toml");
                    fs::write(
                        &path,
                        "# no pools
",
                    )
                    .expect("empty pools file");
                    path
                })
                .clone()
            }),
        }
    }

    fn scratch_root(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("upstroke-validate-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch root");
        dir
    }

    fn opts_in(root: &Path, plan: &str) -> ValidateOptions {
        let mut opts = opts(plan);
        opts.config_root = root.to_path_buf();
        opts
    }

    #[test]
    fn annotation_pass2_literal_quote_marker_does_not_create_a_duplicate_id() {
        use crate::plan::PlanAdapter;

        let parsed = plan::markdown::MarkdownPlanAdapter
            .parse_with_warnings(
                "## First\n<!-- upstroke: id=a -->\n\n## Second\n<!-- upstroke: >id=a -->\n",
            )
            .expect("an unknown annotation key must still parse");
        let mut warnings = parsed.warnings;
        let result = graph::check_graph(&parsed.plan, &mut warnings);
        assert!(result.is_ok(), "graph: {result:?}; warnings: {warnings:?}");
        assert_eq!(parsed.plan.tasks[1].id.as_str(), "second");
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("unknown annotation attribute `>id`")),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn annotation_pass2_lone_cr_quote_keeps_the_frontier_routing_floor() {
        use crate::plan::PlanAdapter;

        let parsed = plan::markdown::MarkdownPlanAdapter
            .parse_with_warnings(
                "## Fix bug\r> Context <!-- upstroke: id=a\r>min=frontier --> more.\r",
            )
            .expect("a plan with lone CR line endings must parse");
        let options = opts("fixtures/bare-plan.md");
        let config = config::load(
            None,
            &options.config_root,
            options.pools_path.as_deref(),
            &mut Vec::new(),
        )
        .expect("the isolated default config must load");
        let chain = route::resolve(&parsed.plan.tasks[0], &config);
        assert_eq!(
            chain.rungs.first().map(|rung| rung.tier),
            Some(crate::ir::Tier::Frontier),
            "warnings: {:?}",
            parsed.warnings
        );
        assert_eq!(
            parsed.plan.tasks[0].min_tier,
            Some(crate::ir::Tier::Frontier)
        );
        assert!(
            parsed.warnings.is_empty(),
            "warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn annotation_pass2_graph_refusal_preserves_the_unknown_attribute_warning() {
        let error = analyze(&opts("fixtures/annotation-invalid-plan.md"))
            .expect_err("the duplicate task IDs must refuse validation");
        let rendered = error.to_string();
        assert!(rendered.contains("duplicate task id `a`"), "{rendered}");
        assert!(
            rendered.contains("unknown annotation attribute `wibble`"),
            "the refusal must retain the gathered warning: {rendered}"
        );
    }

    #[test]
    fn annotation_warnings_survive_config_refusal_without_changing_a_clean_error_variant() {
        for (plan, warned) in [
            ("fixtures/annotation-invalid-plan.md", true),
            ("fixtures/bare-plan.md", false),
        ] {
            let mut options = opts(plan);
            options.config_path = Some(PathBuf::from("fixtures/annotation-invalid-plan.md"));
            let error = analyze(&options).expect_err("Markdown is not a TOML config");
            if warned {
                let UpstrokeError::WithWarnings(bundle) = &error else {
                    panic!("the parser warning must accompany the config refusal");
                };
                assert!(matches!(
                    bundle.error.as_ref(),
                    UpstrokeError::Config { .. }
                ));
                assert_eq!(
                    error
                        .to_string()
                        .matches("unknown annotation attribute `wibble`")
                        .count(),
                    1
                );
            } else {
                assert!(matches!(error, UpstrokeError::Config { .. }));
            }
        }
    }

    #[test]
    fn annotation_warnings_reach_successful_previews_and_review_planning_refusals_once() {
        let mut options = opts("fixtures/annotation-warning-plan.md");
        let rendered = run(&options)
            .expect("an unknown attribute does not refuse a valid plan")
            .render();
        assert_eq!(
            rendered
                .matches("unknown annotation attribute `wibble`")
                .count(),
            1
        );
        options.config_path = Some(PathBuf::from("fixtures/annotation-review-conflict.toml"));
        let error =
            run(&options).expect_err("disabled review conflicts with a demanded second opinion");
        let UpstrokeError::WithWarnings(bundle) = &error else {
            panic!("the annotation warning must accompany the review-planning refusal");
        };
        assert!(matches!(
            bundle.error.as_ref(),
            UpstrokeError::Refused { .. }
        ));
        let rendered = error.to_string();
        assert!(rendered.contains("turns review off"), "{rendered}");
        assert_eq!(
            rendered
                .matches("unknown annotation attribute `wibble`")
                .count(),
            1
        );
    }

    #[test]
    fn the_captured_set_names_every_file_an_analysis_reads() {
        let root = scratch_root("capturedset");
        let plan = root.join("plan.md");
        fs::write(&plan, "## One\n<!-- upstroke: id=t1 depends= -->\n").expect("plan");
        let mut options = opts_in(&root, plan.to_str().expect("utf-8 path"));
        options.config_path = Some(root.join("upstroke.toml"));

        let captured = CapturedInputs::capture(&options);
        let mut expected = vec![plan, root.join("upstroke.toml")];
        expected.push(options.pools_path.clone().expect("the fixture pools file"));
        expected.extend(GATE_DERIVATION_INPUTS.iter().map(|name| root.join(name)));
        assert_eq!(captured.paths(), expected);
    }

    #[test]
    fn an_analysis_is_parsed_out_of_the_captured_plan_not_a_second_read_of_it() {
        let root = scratch_root("capturedplan");
        let plan = root.join("plan.md");
        fs::write(&plan, "## One\n<!-- upstroke: id=t1 depends= -->\n").expect("captured plan");
        let options = opts_in(&root, plan.to_str().expect("utf-8 path"));
        let captured = CapturedInputs::capture(&options);

        fs::write(
            &plan,
            "## One\n<!-- upstroke: id=t1 depends= -->\n\
             ## Two\n<!-- upstroke: id=t2 depends=t1 -->\n",
        )
        .expect("the transient plan");
        let analysis = analyze_captured(&captured, &options).expect("the captured plan analyses");
        assert_eq!(
            analysis
                .plan
                .tasks
                .iter()
                .map(|t| t.id.as_str().to_owned())
                .collect::<Vec<_>>(),
            vec!["t1"],
            "the transient plan was parsed in place of the captured one"
        );

        fs::write(&plan, "## One\n<!-- upstroke: id=t1 depends= -->\n").expect("restored");
        assert_eq!(
            CapturedInputs::capture(&options),
            captured,
            "and the excursion leaves no trace for a confirmation to find"
        );
    }

    #[test]
    fn a_gate_derivation_input_is_part_of_the_captured_set() {
        let root = scratch_root("capturedgates");
        let plan = root.join("plan.md");
        fs::write(&plan, "## One\n<!-- upstroke: id=t1 depends= -->\n").expect("plan");
        let options = opts_in(&root, plan.to_str().expect("utf-8 path"));

        let bare = CapturedInputs::capture(&options);
        let analysis = analyze_captured(&bare, &options).expect("analysis");
        assert!(
            analysis.gates.is_empty(),
            "a repo of no recognised shape derives no gates: {:?}",
            analysis.gates
        );

        fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("a rust repo now");
        let shaped = CapturedInputs::capture(&options);
        assert_ne!(shaped, bare, "the capture must see the worktree change");
        let analysis = analyze_captured(&shaped, &options).expect("analysis");
        assert_eq!(
            analysis
                .gates
                .iter()
                .map(|g| g.name.clone())
                .collect::<Vec<_>>(),
            vec!["check".to_owned(), "test".to_owned()],
            "and the change is one the derivation acts on"
        );
    }

    #[test]
    fn a_pin_without_an_adapter_fails_validate_not_just_run() {
        let pins = vec![config::Pin {
            tier: crate::ir::Tier::Frontier,
            agent: "aider".to_owned(),
            model: "qwen-3-coder".to_owned(),
            effort: None,
        }];
        let err = check_pin_adapters(&pins, builtin_adapter, Path::new("upstroke.toml"))
            .expect_err("preview must not promise a binding run would refuse");
        let message = err.to_string();
        assert!(message.contains("no adapter"), "got: {message}");
        assert!(
            message.contains("claude-code") && message.contains("copilot"),
            "lists what is available: {message}"
        );

        let pins = vec![config::Pin {
            tier: crate::ir::Tier::Frontier,
            agent: "copilot".to_owned(),
            model: "gpt-5.3-codex".to_owned(),
            effort: None,
        }];
        assert!(
            check_pin_adapters(&pins, builtin_adapter, Path::new("upstroke.toml")).is_ok(),
            "copilot gained an adapter in step 9"
        );
    }

    #[test]
    fn the_preview_shows_who_reviews_without_promising_a_binary_it_cannot_probe() {
        let root = env::temp_dir().join(format!("upstroke-validate-review-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        let plan = root.join("plan.md");
        fs::write(
            &plan,
            "## Rotate the signing key\n\
             <!-- upstroke: id=rotate kind=implement depends= paths=src/auth/** -->\n\n\
             ## Note it down\n<!-- upstroke: id=note kind=docs depends=rotate -->\n",
        )
        .expect("plan");
        let cfg = root.join("upstroke.toml");
        fs::write(
            &cfg,
            "[[routing.overrides]]\npaths = [\"src/auth/**\"]\nsecond_opinion = \
             \"different-vendor\"\n",
        )
        .expect("config");
        let mut o = opts("unused");
        o.plan_path = plan;
        o.config_path = Some(cfg);
        let rendered = run(&o).expect("validate").render();

        assert!(
            rendered.contains("review: claude-code/claude-opus-5"),
            "{rendered}"
        );
        assert!(rendered.contains("if installed"), "{rendered}");
        assert!(
            rendered.contains("1 task(s) also require a second opinion"),
            "{rendered}"
        );
        let rotate = rendered
            .lines()
            .find(|l| l.starts_with("rotate"))
            .expect("row");
        assert!(
            rotate.contains("[second opinion: copilot/gpt-5.3-codex]"),
            "{rotate}"
        );
        let note = rendered
            .lines()
            .find(|l| l.starts_with("note"))
            .expect("row");
        assert!(!note.contains("second opinion"), "{note}");
    }

    #[test]
    fn the_preview_echoes_resolved_role_tier_pin_and_disabled_review_effort() {
        let root = env::temp_dir().join(format!("upstroke-validate-effort-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        let cases = [
            (
                "defaults",
                "",
                "effort: implementation=by tier (small=low, mid=medium, frontier=high), review=high",
            ),
            (
                "pin-fallback",
                "[routing]\nreview = { tier = \"small\" }\n\n\
                 [[pins]]\ntier = \"small\"\nagent = \"claude-code\"\n\
                 model = \"claude-haiku-4-5\"\neffort = \"max\"\n",
                "effort: implementation=by tier (small=max, mid=medium, frontier=high), review=max",
            ),
            (
                "other-role-values",
                "[routing.effort]\nimplementation = \"low\"\nreview = \"xhigh\"\n",
                "effort: implementation=low, review=xhigh",
            ),
            (
                "configured-role-values",
                "[routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n",
                "effort: implementation=xhigh, review=max",
            ),
            (
                "review-disabled",
                "[routing]\nreview = { enabled = false }\n",
                "effort: implementation=by tier (small=low, mid=medium, frontier=high), review=disabled",
            ),
        ];

        for (name, config, expected) in cases {
            let cfg = root.join(format!("{name}.toml"));
            fs::write(&cfg, config).expect("config");
            let mut o = opts("fixtures/sample-plan.md");
            o.config_path = Some(cfg);
            let rendered = run(&o).expect("validate").render();
            let actual = rendered
                .lines()
                .find(|line| line.starts_with("effort:"))
                .expect("effort line");
            assert_eq!(actual, expected, "case {name}:\n{rendered}");
        }
    }

    #[test]
    fn the_capacity_block_estimates_without_probing_and_never_reads_unknown_as_full() {
        let dir = env::temp_dir().join(format!("upstroke-validate-pools-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let pools = dir.join("pools.toml");
        fs::write(
            &pools,
            "[pools.claude-max]\nkind = \"subscription-window\"\nagent = \
             \"claude-code\"\nwindow = \"5h\"\nweekly = true\nsources = [\"signals\", \"self\", \
             \"local-logs\"]\nprofile = \"personal\"\n",
        )
        .expect("pools");
        let mut o = opts("fixtures/sample-plan.md");
        o.pools_path = Some(pools);
        let rendered = run(&o).expect("validates").render();

        assert!(rendered.contains("claude-max"), "rendered:\n{rendered}");
        assert!(!rendered.contains("capacity: not connected"));
        assert!(rendered.contains("window=5h"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("profile=personal"),
            "rendered:\n{rendered}"
        );
        assert!(
            rendered.contains("claude-max: unknown [unknown]"),
            "rendered:\n{rendered}"
        );
        assert!(rendered.contains("not full"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("local-logs") && rendered.contains("not read in v0.1"),
            "rendered:\n{rendered}"
        );
        assert!(rendered.contains("never probes"), "rendered:\n{rendered}");
        assert!(rendered.contains("read-only"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("no run in this repository yet"),
            "rendered:\n{rendered}"
        );
    }

    #[test]
    fn derived_gates_appear_in_the_preview() {
        let root = env::temp_dir().join(format!("upstroke-validate-gates-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("Cargo.toml"), "[package]\nname='x'\n").expect("marker");
        let mut o = opts("fixtures/sample-plan.md");
        o.config_root = root;
        let report = run(&o).expect("validates");
        let rendered = report.render();
        assert!(
            rendered.contains("gates: check, test [derived]"),
            "rendered:\n{rendered}"
        );

        let report = run(&opts("fixtures/sample-plan.md")).expect("validates");
        assert!(report.render().contains("gates: none"));
    }

    #[test]
    fn sample_plan_renders_expected_table() {
        let report = run(&opts("fixtures/sample-plan.md")).expect("sample plan validates");
        let rendered = report.render();

        assert!(rendered.contains("api-design"));
        assert!(
            rendered.contains("frontier(annotation)"),
            "rendered:\n{rendered}"
        );
        assert!(
            rendered.contains("mid(annotation)"),
            "min clip shows as annotation source"
        );
        assert!(rendered.contains("min=mid clipped the chain start"));
        assert!(rendered.contains("paths: src/api/**"));
        assert!(rendered.contains("small(default)=claude-code/claude-haiku-4-5(preview)"));
        assert!(rendered.contains("capacity: not connected"));
        assert!(rendered.contains("ok: 4 tasks, no cycles"));
    }

    #[test]
    fn bare_plan_validates_via_heuristics() {
        let report = run(&opts("fixtures/bare-plan.md")).expect("bare plan validates");
        let rendered = report.render();
        assert!(rendered.contains("ok: 5 tasks, no cycles"));
        assert!(rendered.contains("design-the-search-index-schema"));
    }

    #[test]
    fn cyclic_plan_fails_naming_the_cycle() {
        let err = run(&opts("fixtures/cyclic-plan.md")).expect_err("cycle must fail");
        let message = err.to_string();
        assert!(message.contains("dependency cycle"), "got: {message}");
        assert!(message.contains("a -> c -> b -> a"), "got: {message}");
    }

    #[test]
    fn unknown_depends_fails_clearly() {
        let dir = env::temp_dir().join(format!("upstroke-validate-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("unknown-dep.md");
        fs::write(&plan, "## One\n<!-- upstroke: id=one depends=ghost -->\n").expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("unknown dep must fail");
        let message = err.to_string();
        assert!(message.contains("unknown id `ghost`"), "got: {message}");
    }

    #[test]
    fn duplicate_ids_fail() {
        let dir = env::temp_dir().join(format!("upstroke-validate-dup-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("dup.md");
        fs::write(
            &plan,
            "## One\n<!-- upstroke: id=same -->\n\n## Two\n<!-- upstroke: id=same depends= -->\n",
        )
        .expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("duplicate ids must fail");
        assert!(err.to_string().contains("duplicate task id `same`"));
    }

    #[test]
    fn steps_plan_validates_via_ordered_list_fallback() {
        let report = run(&opts("fixtures/steps-plan.md")).expect("steps plan validates");
        let rendered = report.render();
        assert!(rendered.contains("ok: 4 tasks, no cycles"));
        assert!(rendered.contains("design-the-limiter-interface-and-storage-schema"));
    }

    #[test]
    fn artifact_needed_from_a_non_dependency_warns() {
        let dir = env::temp_dir().join(format!("upstroke-wiring-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("wiring.md");
        fs::write(
            &plan,
            "## Design\n<!-- upstroke: id=d out=contract depends= -->\n\n\
             ## Build\n<!-- upstroke: id=b needs=contract depends= -->\n",
        )
        .expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let report = run(&o).expect("wiring problems warn, not fail");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("`b` needs artifact `contract` produced by `d`")),
            "warnings: {:?}",
            report.warnings
        );

        let clean = run(&opts("fixtures/sample-plan.md")).expect("sample validates");
        assert!(clean.warnings.is_empty(), "warnings: {:?}", clean.warnings);
    }

    #[test]
    fn unrecognized_plan_format_names_available_adapters() {
        let dir = env::temp_dir().join(format!("upstroke-sniff-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("plan.json");
        fs::write(&plan, "{\"tasks\": []}\n").expect("write file");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("json must not sniff as markdown");
        assert!(err.to_string().contains("no plan adapter recognizes"));
    }

    #[test]
    fn emit_json_round_trips_through_the_ir() {
        let report = run(&opts("fixtures/sample-plan.md")).expect("sample plan validates");
        let dir = env::temp_dir().join(format!("upstroke-emit-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let json_path = dir.join("plan.normalized.json");
        report
            .write_normalized_json(&json_path)
            .expect("write json");

        let text = fs::read_to_string(&json_path).expect("read back");
        let plan: Plan = serde_json::from_str(&text).expect("json matches the IR");
        assert_eq!(plan.tasks.len(), 4);
        assert_eq!(plan.source.adapter, "markdown");
        assert_eq!(plan.tasks[2].min_tier, Some(crate::ir::Tier::Mid));
    }
}
