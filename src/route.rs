//! Extended notes: `docs/internals/route.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods)]

use std::fmt;

use crate::catalog;
use crate::config::Config;
use crate::ir::{Task, Tier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainSource {
    Default,
    Annotation,
    Override,
}

impl fmt::Display for ChainSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Default => "default",
            Self::Annotation => "annotation",
            Self::Override => "override",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub agent: String,
    pub model: String,
    pub pinned: bool,
}

#[derive(Debug, Clone)]
pub struct Rung {
    pub tier: Tier,
    pub source: ChainSource,
    pub binding: Binding,
}

#[derive(Debug, Clone)]
pub struct ResolvedChain {
    pub rungs: Vec<Rung>,
    pub notes: Vec<String>,
    pub attempts_per: u32,
}

pub fn resolve(task: &Task, cfg: &Config) -> ResolvedChain {
    let kind_chain = cfg.chain_for(task.kind);
    let mut tiers: Vec<(Tier, ChainSource)> = kind_chain
        .chain
        .iter()
        .map(|t| (*t, ChainSource::Default))
        .collect();
    let mut notes = Vec::new();

    for ov in &cfg.overrides {
        if let Some(start_at) = ov.start_at {
            if task.path_hints.iter().any(|h| ov.globs.is_match(h))
                && raise_start(&mut tiers, start_at, ChainSource::Override)
            {
                notes.push(format!(
                    "override paths [{}] raised start to {start_at}",
                    ov.raw_paths.join(", "),
                ));
            }
        }
    }

    if let Some(tier) = task.suggested_tier {
        let raised = raise_start(&mut tiers, tier, ChainSource::Annotation);
        if !raised {
            if let Some(first) = tiers.first_mut() {
                if first.0 == tier && first.1 == ChainSource::Default {
                    first.1 = ChainSource::Annotation;
                }
            }
        }
    }

    if let Some(min) = task.min_tier {
        if raise_start(&mut tiers, min, ChainSource::Annotation) {
            notes.push(format!("min={min} clipped the chain start"));
        }
    }

    if !task.path_hints.is_empty() {
        notes.push(format!("paths: {}", task.path_hints.join(", ")));
    }

    let rungs = tiers
        .into_iter()
        .map(|(tier, source)| Rung {
            tier,
            source,
            binding: bind(tier, cfg),
        })
        .collect();
    ResolvedChain {
        rungs,
        notes,
        attempts_per: kind_chain.attempts_per,
    }
}

fn raise_start(tiers: &mut Vec<(Tier, ChainSource)>, floor: Tier, source: ChainSource) -> bool {
    let before = tiers.len();
    tiers.retain(|(t, _)| *t >= floor);
    if tiers.is_empty() {
        tiers.push((floor, source));
        return true;
    }
    let changed = tiers.len() != before;
    if changed {
        if let Some(first) = tiers.first_mut() {
            first.1 = source;
        }
    }
    changed
}

fn bind(tier: Tier, cfg: &Config) -> Binding {
    if let Some(pin) = cfg.pins.iter().find(|p| p.tier == tier) {
        return Binding {
            agent: pin.agent.clone(),
            model: pin.model.clone(),
            pinned: true,
        };
    }
    let example = catalog::example_binding(tier);
    Binding {
        agent: example.agent.to_owned(),
        model: example.model.to_owned(),
        pinned: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::ir::{TaskId, TaskKind};
    use std::path::PathBuf;
    use std::sync::OnceLock;

    fn hermetic() -> (PathBuf, PathBuf) {
        static DIRS: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
        DIRS.get_or_init(|| {
            let dir = std::env::temp_dir()
                .join(format!("upstroke-route-hermetic-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("scratch dir");
            let empty = dir.join("no-pools.toml");
            std::fs::write(&empty, "# no pools\n").expect("empty pools file");
            (dir, empty)
        })
        .clone()
    }

    fn default_config() -> Config {
        let mut warnings = Vec::new();
        let (dir, empty) = hermetic();
        config::load(None, &dir, Some(&empty), &mut warnings).expect("default config")
    }

    fn task(kind: TaskKind) -> Task {
        Task {
            id: TaskId::from("t"),
            kind,
            title: String::new(),
            body: String::new(),
            depends_on: Vec::new(),
            acceptance: Vec::new(),
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::new(),
        }
    }

    fn tiers(rc: &ResolvedChain) -> Vec<Tier> {
        rc.rungs.iter().map(|r| r.tier).collect()
    }

    #[test]
    fn default_chains_have_default_source_and_preview_bindings() {
        let cfg = default_config();
        let rc = resolve(&task(TaskKind::Fix), &cfg);
        assert_eq!(tiers(&rc), [Tier::Small, Tier::Mid, Tier::Frontier]);
        assert!(rc.rungs.iter().all(|r| r.source == ChainSource::Default));
        assert!(rc.rungs.iter().all(|r| !r.binding.pinned));
        assert_eq!(rc.attempts_per, 2);
    }

    #[test]
    fn min_clips_and_notes() {
        let cfg = default_config();
        let mut t = task(TaskKind::Fix);
        t.min_tier = Some(Tier::Mid);
        let rc = resolve(&t, &cfg);
        assert_eq!(tiers(&rc), [Tier::Mid, Tier::Frontier]);
        assert_eq!(rc.rungs[0].source, ChainSource::Annotation);
        assert_eq!(rc.rungs[1].source, ChainSource::Default);
        assert!(rc.notes.iter().any(|n| n.contains("min=mid")));
    }

    #[test]
    fn advisory_tier_raises_or_relabels_but_never_lowers() {
        let cfg = default_config();

        let mut raiser = task(TaskKind::Fix);
        raiser.suggested_tier = Some(Tier::Frontier);
        let rc = resolve(&raiser, &cfg);
        assert_eq!(tiers(&rc), [Tier::Frontier]);
        assert_eq!(rc.rungs[0].source, ChainSource::Annotation);

        let mut equal = task(TaskKind::Design);
        equal.suggested_tier = Some(Tier::Frontier);
        let rc = resolve(&equal, &cfg);
        assert_eq!(tiers(&rc), [Tier::Frontier]);
        assert_eq!(rc.rungs[0].source, ChainSource::Annotation);

        let mut lower = task(TaskKind::Design);
        lower.suggested_tier = Some(Tier::Small);
        let rc = resolve(&lower, &cfg);
        assert_eq!(tiers(&rc), [Tier::Frontier]);
        assert_eq!(rc.rungs[0].source, ChainSource::Default);
    }

    #[test]
    fn path_floor_raises_start_with_override_source() {
        let dir = std::env::temp_dir().join(format!("upstroke-route-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let cfg_path: PathBuf = dir.join("floor.toml");
        std::fs::write(
            &cfg_path,
            "[[routing.overrides]]\npaths = [\"src/auth/**\"]\nstart_at = \"frontier\"\n",
        )
        .expect("write config");
        let missing = dir.join("missing-pools.toml");
        std::fs::write(
            &missing,
            "# no pools
",
        )
        .expect("empty pools file");
        let mut warnings = Vec::new();
        let cfg = config::load(Some(&cfg_path), &dir, Some(&missing), &mut warnings).expect("load");

        let mut t = task(TaskKind::Fix);
        t.path_hints.push("src/auth/login.rs".to_owned());
        let rc = resolve(&t, &cfg);
        assert_eq!(tiers(&rc), [Tier::Frontier]);
        assert_eq!(rc.rungs[0].source, ChainSource::Override);
        assert!(rc.notes.iter().any(|n| n.contains("src/auth/**")));

        let mut unmatched = task(TaskKind::Fix);
        unmatched.path_hints.push("src/api/list.rs".to_owned());
        let rc = resolve(&unmatched, &cfg);
        assert_eq!(tiers(&rc), [Tier::Small, Tier::Mid, Tier::Frontier]);

        let mut agreeing = task(TaskKind::Fix);
        agreeing.path_hints.push("src/auth/login.rs".to_owned());
        agreeing.suggested_tier = Some(Tier::Frontier);
        let rc = resolve(&agreeing, &cfg);
        assert_eq!(
            rc.rungs[0].source,
            ChainSource::Override,
            "blast radius is what binds"
        );
    }

    #[test]
    fn pins_bind_their_tier() {
        let dir = std::env::temp_dir().join(format!("upstroke-route-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let cfg_path = dir.join("pin.toml");
        std::fs::write(
            &cfg_path,
            "[[pins]]\ntier = \"frontier\"\nagent = \"claude-code\"\nmodel = \"claude-opus-4-8\"\n",
        )
        .expect("write config");
        let missing = dir.join("missing-pools.toml");
        std::fs::write(
            &missing,
            "# no pools
",
        )
        .expect("empty pools file");
        let mut warnings = Vec::new();
        let cfg = config::load(Some(&cfg_path), &dir, Some(&missing), &mut warnings).expect("load");

        let rc = resolve(&task(TaskKind::Design), &cfg);
        assert_eq!(rc.rungs[0].binding.model, "claude-opus-4-8");
        assert!(rc.rungs[0].binding.pinned);

        let rc = resolve(&task(TaskKind::Docs), &cfg);
        assert!(
            rc.rungs.iter().all(|r| !r.binding.pinned),
            "pin scoped to its tier"
        );
    }
}
