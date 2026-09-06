//! Extended notes: `docs/internals/effects/tests/artifacts.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use crate::topology::effects::EffectSiteId;

pub(super) fn artifact_content(text: &str) -> String {
    text.replace("\r\n", "\n")
}

pub(super) fn funnel_module_record() -> String {
    let mut disagreements = Vec::new();
    for site in EffectSiteId::all() {
        let inventory = site.module();
        let actual = funnel_module(site);
        if inventory != actual {
            disagreements.push(serde_json::json!({
                "site": site.name(),
                "group": site.group().name(),
                "inventory_module": inventory,
                "funnel_module": actual,
            }));
        }
    }
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "note": "PR5-CONF-018. effect_sites.json's `module` column is \
                     EffectSiteId::module(), which is PR3's frozen answer and the \
                     packet's -- mechanism (2) places the answer funnels in \
                     src/interaction.rs. PR5 lane B put those three funnel BODIES in \
                     src/rundir.rs and left interaction::{write_question, write_answer, \
                     read_answer} as delegations. Both files are allowlisted funnel \
                     modules and enforcement is unchanged either way, so Fable ruled \
                     this a preference and Sol a low defect; what is not a matter of \
                     taste is that a gate-attached artifact stated something untrue of \
                     this tree with nothing checked in saying otherwise. The generator \
                     is src/topology/effects.rs, frozen, so the column is corrected \
                     here rather than in place, and the funnel bodies are NOT moved.",
            "sites_checked": EffectSiteId::all().len(),
            "disagreements": disagreements,
        }))
        .expect("the funnel-module record serializes")
    )
}

pub(super) fn funnel_module(site: EffectSiteId) -> &'static str {
    match site.group().name() {
        "Answer" => "src/rundir.rs",
        _ => site.module(),
    }
}

pub(super) const SITES_WITHOUT_A_FUNNEL: &[&str] = &["Report.Write"];

pub(super) const SAMPLING_N: u32 = 8;

pub(super) fn residue_record() -> String {
    let mut sites = Vec::new();
    for site in EffectSiteId::all() {
        if site.residue_classes().is_empty() {
            continue;
        }
        sites.push(serde_json::json!({
            "site": site.name(),
            "group": site.group().name(),
            "row": site.row(),
            "module": site.module(),
            "sampling_n": SAMPLING_N,
            "classes": site
                .residue_classes()
                .iter()
                .map(|class| serde_json::json!({
                    "class": class,
                    "label": class.label(),
                    "classified_as": class.classified_as(),
                }))
                .collect::<Vec<_>>(),
            "elements": site.residue_elements(),
        }));
    }
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "note": "decisions.effect_site_inventory.outputs: the residue-class \
                     evidence record, DECLARATIONS half. Per element: constructed, \
                     classified, recovered -- proven by workspace_manager::tests. Per \
                     site: the frozen sampling N. The observed-class histogram is \
                     machine-varying and cannot be pinned in a file compared \
                     byte-for-byte, so it is emitted to effects/residue-histogram.json \
                     on every run by \
                     sampled_git_child_kills_every_residue_classified_and_recovered, \
                     which reads it back and checks it accounts for every sample \
                     (PR5-CONF-004).",
            "sites": sites,
        }))
        .expect("the residue record serializes")
    )
}
