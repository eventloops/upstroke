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
///
/// The Copilot half was checked against GitHub's supported-models reference on
/// 2026-08-09; plain `gpt-5` and `gemini-2.5-pro` had already left the roster
/// and were replaced (see the entries). Slugs churn faster than releases do, so
/// treat every name here as point-in-time: `tactus connect` cross-checks the
/// roster against what the installed CLI advertises wherever that CLI can
/// actually enumerate models, and says so when it cannot (today, neither can).
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
    // Slug pattern-derived, not verbatim: GitHub's reference lists the model as
    // "Gemini 3.1 Pro" and writes dots inside versions elsewhere
    // (`claude-sonnet-4.6`), but no example spells this one out. Lower
    // confidence than the entries around it — the `connect` cross-check is what
    // is meant to catch it once a CLI can enumerate models.
    entry("copilot", "gemini-3.1-pro", Tier::Mid, Family::Google),
    entry("copilot", "claude-sonnet-5", Tier::Mid, Family::Anthropic),
    // Verbatim from GitHub's CLI programmatic reference, where it appears as a
    // `--model` example — the highest exact-slug confidence available without
    // enumeration. It is also load-bearing: `different_family_at` picks it for
    // every cross-vendor second opinion at frontier, so a wrong name here fails
    // §11.3 reviews at runtime on exactly the paths blast radius protects.
    entry("copilot", "gpt-5.3-codex", Tier::Frontier, Family::OpenAI),
    entry(
        "copilot",
        "claude-opus-5",
        Tier::Frontier,
        Family::Anthropic,
    ),
    // Read from a live session's own rollout on 2026-08-11 rather than from a
    // docs page — the highest confidence any entry in this table has, since it
    // is the string the CLI recorded for a turn it actually ran. Frontier
    // because that is what it is and what it is for: the whole point of this
    // pool is implementing at the top rung without the reviewer's window
    // paying for it (§23.2, as scoped there).
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

/// Catalog entries for `agent` that the installed CLI does not advertise.
///
/// The guard the step-9 review asked for, and the reason `Discovery.models`
/// exists: this table is hand-maintained point-in-time data, and two of its
/// entries had already gone stale by Aug 2026 — including the one
/// [`different_family_at`] picks for every frontier second opinion, where a
/// wrong slug fails §11.3 review at runtime on exactly the blast-radius paths
/// the second opinion exists to protect.
///
/// **`advertised` is empty on both adapters today** (neither CLI enumerates
/// models non-interactively), and an empty list means *the CLI said nothing*,
/// not *the CLI has no models* — so it returns nothing rather than condemning
/// the whole roster. The check fires when a future CLI grows enumeration;
/// `Caps::model_list` is what gates asking at all.
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
    // Zero overlap is a format mismatch, not a stale catalog. GitHub's own
    // reference writes display names ("GPT-5.3-Codex", "Gemini 3.1 Pro") beside
    // the slugs `--model` takes, so a listing command that prints the former
    // would make every entry look missing — and a guard that names the entire
    // roster on its first real firing, advising an upgrade that cannot help, is
    // worse than no guard. One match is enough to trust the comparison.
    let overlap = CATALOG
        .iter()
        .filter(|entry| entry.agent == agent)
        .any(|entry| seen.contains(&normalize(entry.model)));
    if overlap { missing } else { Vec::new() }
}

/// Case and separators folded away, so `GPT-5.3-Codex`, `gpt-5.3-codex` and
/// `GPT 5.3 Codex` all compare equal. Slugs are the contract, but a listing
/// meant for humans is not obliged to use them.
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
        // Silence is not disagreement: a CLI that advertises nothing must not
        // read as one that has repudiated the whole roster.
        assert!(missing_from("copilot", &[]).is_empty());

        let advertised: Vec<String> = ["gpt-5-mini", "gpt-5.3-codex", "claude-sonnet-5"]
            .map(str::to_owned)
            .to_vec();
        let missing = missing_from("copilot", &advertised);
        assert_eq!(missing, ["gemini-3.1-pro", "claude-opus-5"], "{missing:?}");
        // Scoped to the agent asked about — Claude Code's roster is not
        // evidence about Copilot's.
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
        // The trap this exists to avoid: `copilot/claude-opus-5` is a different
        // AGENT at the same tier, so an agent-id comparison would happily pick
        // it — and the "different families share fewer blind spots" claim
        // (§11.3) would be silently untrue.
        let picked = different_family_at(Tier::Frontier, Family::Anthropic, everything)
            .expect("an openai frontier model exists");
        assert_eq!(picked.family, Family::OpenAI);
        assert_eq!((picked.agent, picked.model), ("copilot", "gpt-5.3-codex"));

        let picked = different_family_at(Tier::Mid, Family::Anthropic, everything)
            .expect("a non-anthropic mid model exists");
        assert_eq!((picked.agent, picked.model), ("copilot", "gemini-3.1-pro"));

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
