//! Static capability catalog (DESIGN.md §13): model → tier classification.
//!
//! This is point-in-time data shipped with the binary — the no-HTTP invariant
//! holds, so it can only be updated by releasing. Unknown models are never
//! auto-selected; a pin naming one is a hard error.

use crate::ir::Tier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogEntry {
    pub agent: &'static str,
    pub model: &'static str,
    pub tier: Tier,
}

const fn entry(agent: &'static str, model: &'static str, tier: Tier) -> CatalogEntry {
    CatalogEntry { agent, model, tier }
}

/// Preview default per tier when no pin applies (also listed in [`CATALOG`]).
const EXAMPLE_SMALL: CatalogEntry = entry("claude-code", "claude-haiku-4-5", Tier::Small);
const EXAMPLE_MID: CatalogEntry = entry("claude-code", "claude-sonnet-5", Tier::Mid);
const EXAMPLE_FRONTIER: CatalogEntry = entry("claude-code", "claude-opus-5", Tier::Frontier);

/// Snapshot of Claude Code and Copilot CLI rosters as of Aug 2026.
pub const CATALOG: &[CatalogEntry] = &[
    EXAMPLE_SMALL,
    entry("claude-code", "claude-sonnet-4-5", Tier::Mid),
    EXAMPLE_MID,
    entry("claude-code", "claude-opus-4-8", Tier::Frontier),
    EXAMPLE_FRONTIER,
    entry("copilot", "gpt-5-mini", Tier::Small),
    entry("copilot", "gemini-2.5-pro", Tier::Mid),
    entry("copilot", "claude-sonnet-5", Tier::Mid),
    entry("copilot", "gpt-5", Tier::Frontier),
    entry("copilot", "claude-opus-5", Tier::Frontier),
];

pub fn lookup(agent: &str, model: &str) -> Option<CatalogEntry> {
    CATALOG
        .iter()
        .copied()
        .find(|e| e.agent == agent && e.model == model)
}

pub fn known_models(agent: &str) -> Vec<&'static str> {
    CATALOG
        .iter()
        .filter(|e| e.agent == agent)
        .map(|e| e.model)
        .collect()
}

/// Catalog-derived example binding shown by the binder preview.
pub fn example_binding(tier: Tier) -> CatalogEntry {
    match tier {
        Tier::Small => EXAMPLE_SMALL,
        Tier::Mid => EXAMPLE_MID,
        Tier::Frontier => EXAMPLE_FRONTIER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_bindings_cover_every_tier_and_exist_in_catalog() {
        for tier in [Tier::Small, Tier::Mid, Tier::Frontier] {
            let e = example_binding(tier);
            assert_eq!(e.tier, tier);
            assert_eq!(lookup(e.agent, e.model), Some(e));
        }
    }

    #[test]
    fn lookup_misses_unknown_models() {
        assert!(lookup("claude-code", "claude-opus-4-8").is_some());
        assert!(lookup("claude-code", "gpt-5").is_none());
        assert!(lookup("aider", "anything").is_none());
    }

    #[test]
    fn known_models_scoped_to_agent() {
        let models = known_models("copilot");
        assert!(models.contains(&"gpt-5"));
        assert!(!models.contains(&"claude-opus-4-8"));
    }
}
