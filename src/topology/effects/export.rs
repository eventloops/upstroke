//! The generated `effect_sites.json` inventory: one record per site, per point
//! and per residue class.
//!
//! Split out of `topology::effects`; the parent re-exports every item here, so
//! `crate::topology::effects::effect_sites` and its siblings are unchanged
//! paths.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::EffectSiteId;
use super::residue_authority::{
    EvidenceLabel, ObjectResidue, ObservableOrder, ResidueClass, ResidueElement,
};
use super::vocab::{
    Adjacent, EnforcementDomain, FaultRow, FunnelGroup, InjectionMode, Platform, ResourceRow,
    SiteScope, SubEffectPoint,
};

// ---------------------------------------------------------------------------
// effect_sites.json
// ---------------------------------------------------------------------------

/// One point of a site, as the generated inventory records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointExport {
    /// Which point.
    pub point: SubEffectPoint,
    /// The host it exists on.
    pub platform: Platform,
    /// Every mode it supports.
    pub modes: Vec<InjectionMode>,
}

/// One residue class of a site, as the generated inventory records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidueClassExport {
    /// Which class.
    pub class: ResidueClass,
    /// The label it must carry. Always recovery-proven.
    pub label: EvidenceLabel,
    /// The classifier outcome it is the class of.
    pub classified_as: ObjectResidue,
    /// Every element its synthetic construction must build.
    pub elements: Vec<ResidueElement>,
}

/// One site of `effect_sites.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSiteExport {
    /// The dotted name.
    pub site: EffectSiteId,
    /// Its group.
    pub group: FunnelGroup,
    /// Its row.
    pub row: ResourceRow,
    /// The row's enforcement domain.
    pub domain: EnforcementDomain,
    /// Its adjacency.
    pub adjacent: Adjacent,
    /// The orders a fault here can leave observable.
    pub observable_orders: Vec<ObservableOrder>,
    /// Its fault-matrix row.
    pub fault_row: FaultRow,
    /// Its scope.
    pub scope: SiteScope,
    /// The module its funnel lives in.
    pub module: String,
    /// Whether it performs no effect.
    pub read_only: bool,
    /// Its parent-side sub-effect points.
    pub sub_effect_points: Vec<PointExport>,
    /// Its residue classes.
    pub residue_classes: Vec<ResidueClassExport>,
}

/// The generated inventory, in group and declaration order.
///
/// Generated *from* the enums, so it cannot describe a site that does not
/// exist and cannot omit one that does.
pub fn effect_sites() -> Vec<EffectSiteExport> {
    EffectSiteId::all()
        .into_iter()
        .map(|site| EffectSiteExport {
            site,
            group: site.group(),
            row: site.row(),
            domain: site.row().domain(),
            adjacent: site.adjacent(),
            observable_orders: site.observable_orders().to_vec(),
            fault_row: site.fault_row(),
            scope: site.scope(),
            module: site.module().to_owned(),
            read_only: site.is_read_only(),
            sub_effect_points: site
                .sub_effects()
                .iter()
                .map(|point| PointExport {
                    point: *point,
                    platform: point.platform(),
                    modes: point.modes().to_vec(),
                })
                .collect(),
            residue_classes: site
                .residue_classes()
                .iter()
                .map(|class| ResidueClassExport {
                    class: *class,
                    label: class.label(),
                    classified_as: class.classified_as(),
                    elements: site.residue_elements().to_vec(),
                })
                .collect(),
        })
        .collect()
}

/// Why serializing the generated inventory failed.
#[derive(Debug, Error)]
#[error("failed to serialize the effect site inventory: {0}")]
pub struct ExportError(#[from] serde_json::Error);

/// `effect_sites.json`, pretty-printed for a gate report to attach.
///
/// # Errors
///
/// Returns [`ExportError`] if the generated inventory cannot be serialized to JSON.
pub fn effect_sites_json() -> Result<String, ExportError> {
    Ok(serde_json::to_string_pretty(&effect_sites())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_error_names_the_operation_and_wraps_the_source() {
        let source = serde_json::from_str::<serde_json::Value>("not json")
            .expect_err("malformed input is not valid JSON");
        let error = ExportError::from(source);
        let message = error.to_string();
        assert!(
            message.starts_with("failed to serialize the effect site inventory: "),
            "{message}"
        );
        assert!(!message.ends_with('.'), "{message}");
    }
}
