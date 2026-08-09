//! Static capability catalog (DESIGN.md §13): model → tier classification.
//!
//! This is point-in-time data shipped with the binary — the no-HTTP invariant
//! holds, so it can only be updated by releasing. Unknown models are never
//! auto-selected; a pin naming one is a hard error.

use std::fmt;

use crate::ir::Tier;

/// Which lab trained the model, independent of which CLI serves it.
///
/// §11.3's cross-vendor second opinion turns on this and **not** on the agent
/// id: Copilot serves Anthropic models too, so `claude-code/claude-opus-5` and
/// `copilot/claude-opus-5` are a harness switch that shares every blind spot.
/// "Different families share fewer blind spots" is the whole claim, so family
/// is what the binder compares.
///
/// Assigned explicitly per entry rather than inferred from the model name. This
/// is hand-maintained data, and a prefix heuristic that silently misclassifies
/// one rename would quietly pair a reviewer with its own family — a verification
/// property failing without a symptom.
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

/// Preview default per tier when no pin applies (also listed in [`CATALOG`]).
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

/// Snapshot of Claude Code and Copilot CLI rosters as of Aug 2026.
///
/// **Table order is preference order.** [`different_family_at`] returns the
/// first match, so moving an entry up promotes it for cross-family review.
pub const CATALOG: &[CatalogEntry] = &[
    EXAMPLE_SMALL,
    entry(
        "claude-code",
        "claude-sonnet-4-5",
        Tier::Mid,
        Family::Anthropic,
    ),
    EXAMPLE_MID,
    entry(
        "claude-code",
        "claude-opus-4-8",
        Tier::Frontier,
        Family::Anthropic,
    ),
    EXAMPLE_FRONTIER,
    entry("copilot", "gpt-5-mini", Tier::Small, Family::OpenAI),
    entry("copilot", "gemini-2.5-pro", Tier::Mid, Family::Google),
    entry("copilot", "claude-sonnet-5", Tier::Mid, Family::Anthropic),
    entry("copilot", "gpt-5", Tier::Frontier, Family::OpenAI),
    entry(
        "copilot",
        "claude-opus-5",
        Tier::Frontier,
        Family::Anthropic,
    ),
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

/// The model §11.3's cross-vendor second opinion binds to: same tier, a
/// different training family, and an agent this build can actually drive.
///
/// First match in [`CATALOG`] order wins, so the choice is deterministic and
/// the table is where the preference is expressed. `has_adapter` is injected
/// rather than read from the registry so the engine can ask about the adapters
/// its own harness holds — which, under test, is not the built-in set.
///
/// `None` means this build cannot second-opinion that tier at all. Callers
/// decide what that costs: a configured `second_opinion` refuses the run, while
/// the implicit anti-self-review rebind settles for a warning.
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

    fn everything(_: &str) -> bool {
        true
    }

    #[test]
    fn a_second_opinion_crosses_families_not_merely_vendors() {
        // The trap this exists to avoid: `copilot/claude-opus-5` is a different
        // AGENT at the same tier, so an agent-id comparison would happily pick
        // it — and the "different families share fewer blind spots" claim
        // (§11.3) would be silently untrue.
        let picked = different_family_at(Tier::Frontier, Family::Anthropic, everything)
            .expect("an openai frontier model exists");
        assert_eq!(picked.family, Family::OpenAI);
        assert_eq!((picked.agent, picked.model), ("copilot", "gpt-5"));

        let picked = different_family_at(Tier::Mid, Family::Anthropic, everything)
            .expect("a non-anthropic mid model exists");
        assert_eq!((picked.agent, picked.model), ("copilot", "gemini-2.5-pro"));

        // Asked from the other side, it comes back to Anthropic.
        let picked = different_family_at(Tier::Frontier, Family::OpenAI, everything)
            .expect("an anthropic frontier model exists");
        assert_eq!(picked.family, Family::Anthropic);
    }

    #[test]
    fn a_tier_with_no_reachable_other_family_resolves_to_nothing() {
        // The single-vendor install: only claude-code has an adapter, and every
        // claude-code entry is Anthropic. Callers must handle this rather than
        // quietly pairing a model with itself.
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
        // Families are assigned by hand, so this is the guard against a typo in
        // the table rather than a naming rule the code relies on anywhere.
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
