//! Extended notes: `docs/internals/catalog.md`

use std::fmt;

use crate::ir::Tier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Anthropic,
    OpenAI,
    Google,
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Anthropic => "anthropic",
            Self::OpenAI => "openai",
            Self::Google => "google",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogEntry {
    pub agent: &'static str,
    pub model: &'static str,
    pub tier: Tier,
    pub family: Family,
}

const fn entry(
    agent: &'static str,
    model: &'static str,
    tier: Tier,
    family: Family,
) -> CatalogEntry {
    CatalogEntry {
        agent,
        model,
        tier,
        family,
    }
}

const EXAMPLE_SMALL: CatalogEntry = entry(
    "claude-code",
    "claude-haiku-4-5",
    Tier::Small,
    Family::Anthropic,
);
const EXAMPLE_MID: CatalogEntry = entry(
    "claude-code",
    "claude-sonnet-5",
    Tier::Mid,
    Family::Anthropic,
);
const EXAMPLE_FRONTIER: CatalogEntry = entry(
    "claude-code",
    "claude-opus-5",
    Tier::Frontier,
    Family::Anthropic,
);

pub const CATALOG: &[CatalogEntry] = &[
    EXAMPLE_SMALL,
    entry(
        "claude-code",
        "claude-sonnet-4-5",
        Tier::Mid,
        Family::Anthropic,
    ),
    EXAMPLE_MID,
    EXAMPLE_FRONTIER,
    entry(
        "claude-code",
        "claude-opus-4-8",
        Tier::Frontier,
        Family::Anthropic,
    ),
    entry("copilot", "gpt-5-mini", Tier::Small, Family::OpenAI),
    entry("copilot", "gemini-3.1-pro", Tier::Mid, Family::Google),
    entry("copilot", "claude-sonnet-5", Tier::Mid, Family::Anthropic),
    entry("copilot", "gpt-5.3-codex", Tier::Frontier, Family::OpenAI),
    entry(
        "copilot",
        "claude-opus-5",
        Tier::Frontier,
        Family::Anthropic,
    ),
    entry("codex", "gpt-5.6-sol", Tier::Frontier, Family::OpenAI),
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

pub fn example_binding(tier: Tier) -> CatalogEntry {
    match tier {
        Tier::Small => EXAMPLE_SMALL,
        Tier::Mid => EXAMPLE_MID,
        Tier::Frontier => EXAMPLE_FRONTIER,
    }
}

pub fn different_family_at(
    tier: Tier,
    not_family: Family,
    has_adapter: impl Fn(&str) -> bool,
) -> Option<CatalogEntry> {
    CATALOG
        .iter()
        .copied()
        .find(|e| e.tier == tier && e.family != not_family && has_adapter(e.agent))
}

pub fn missing_from(agent: &str, advertised: &[String]) -> Vec<&'static str> {
    if advertised.is_empty() {
        return Vec::new();
    }
    let seen: Vec<String> = advertised.iter().map(|model| normalize(model)).collect();
    let missing: Vec<&'static str> = CATALOG
        .iter()
        .filter(|entry| entry.agent == agent)
        .map(|entry| entry.model)
        .filter(|model| !seen.contains(&normalize(model)))
        .collect();
    let overlap = CATALOG
        .iter()
        .filter(|entry| entry.agent == agent)
        .any(|entry| seen.contains(&normalize(entry.model)));
    if overlap { missing } else { Vec::new() }
}

fn normalize(model: &str) -> String {
    model
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cli_that_enumerates_models_is_cross_checked_against_the_catalog() {
        assert!(missing_from("copilot", &[]).is_empty());

        let advertised: Vec<String> = ["gpt-5-mini", "gpt-5.3-codex", "claude-sonnet-5"]
            .map(str::to_owned)
            .to_vec();
        let missing = missing_from("copilot", &advertised);
        assert_eq!(missing, ["gemini-3.1-pro", "claude-opus-5"], "{missing:?}");
        assert!(!missing.contains(&"claude-haiku-4-5"));
    }

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
        assert!(models.contains(&"gpt-5.3-codex"));
        assert!(!models.contains(&"claude-opus-4-8"));
    }

    fn everything(_: &str) -> bool {
        true
    }

    #[test]
    fn a_second_opinion_crosses_families_not_merely_vendors() {
        let picked = different_family_at(Tier::Frontier, Family::Anthropic, everything)
            .expect("an openai frontier model exists");
        assert_eq!(picked.family, Family::OpenAI);
        assert_eq!((picked.agent, picked.model), ("copilot", "gpt-5.3-codex"));

        let picked = different_family_at(Tier::Mid, Family::Anthropic, everything)
            .expect("a non-anthropic mid model exists");
        assert_eq!((picked.agent, picked.model), ("copilot", "gemini-3.1-pro"));

        let picked = different_family_at(Tier::Frontier, Family::OpenAI, everything)
            .expect("an anthropic frontier model exists");
        assert_eq!(picked.family, Family::Anthropic);
        assert_eq!(
            (picked.agent, picked.model),
            ("claude-code", "claude-opus-5"),
            "the current Opus model must remain ahead of retained legacy slugs"
        );
    }

    #[test]
    fn a_tier_with_no_reachable_other_family_resolves_to_nothing() {
        let claude_only = |agent: &str| agent == "claude-code";
        for tier in [Tier::Small, Tier::Mid, Tier::Frontier] {
            assert!(
                different_family_at(tier, Family::Anthropic, claude_only).is_none(),
                "{tier} should have no cross-family option without copilot"
            );
        }
    }

    #[test]
    fn every_entry_declares_a_family_consistent_with_its_name() {
        for e in CATALOG {
            let expected = if e.model.starts_with("claude-") {
                Family::Anthropic
            } else if e.model.starts_with("gpt-") {
                Family::OpenAI
            } else if e.model.starts_with("gemini-") {
                Family::Google
            } else {
                continue;
            };
            assert_eq!(
                e.family, expected,
                "{}/{} is filed under the wrong family",
                e.agent, e.model
            );
        }
    }
}
