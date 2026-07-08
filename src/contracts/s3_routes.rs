use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ArgumentClassification, Confidence, CycleTemplate, PathEdge, TypeRef};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutesData {
    pub analysis_model: String,
    pub policy_fingerprint: String,
    #[serde(default)]
    pub coverage: RouteCoverage,
    pub targets: BTreeMap<String, TargetRoutes>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteCoverage {
    CompleteFamilies,
    CanonicalPerProvenance,
}

impl Default for RouteCoverage {
    fn default() -> Self {
        Self::CompleteFamilies
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRoutes {
    pub target_type_id: String,
    pub sink_ref_ids: Vec<String>,
    pub reachability: Reachability,
    pub best_verdict: Option<RouteVerdict>,
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub route_id: String,
    pub path_family_id: String,
    pub target_type_id: String,
    pub origin: RouteOrigin,
    pub verdict: RouteVerdict,
    pub selector: Option<RouteSelector>,
    pub selector_continuity: SelectorContinuity,
    pub terminal_semantic_edge_id: String,
    pub boundaries: Vec<RouteBoundary>,
    pub signature: RouteSignature,
    pub witness: RouteWitness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteSelector {
    pub selector_id: String,
    pub arg_ref: String,
    pub root_arg_ref: String,
    pub arg_path: String,
    pub input_path: Vec<String>,
    pub type_ref: TypeRef,
    pub classification: ArgumentClassification,
    pub confidence: Confidence,
    pub selected_type_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RouteBoundary {
    pub family: BoundaryFamily,
    pub source: BoundarySource,
    pub root_edge_id: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RouteSignature {
    pub origin: RouteOrigin,
    pub selector_id: Option<String>,
    pub path_family_id: String,
    pub terminal_semantic_edge_id: String,
    pub boundary_families: Vec<BoundaryFamily>,
    pub selector_continuity: SelectorContinuity,
    pub verdict: RouteVerdict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteWitness {
    pub witness_id: String,
    pub entry_field_id: String,
    pub edges: Vec<PathEdge>,
    pub cycle_templates: Vec<CycleTemplate>,
    pub field_hop_count: usize,
    pub display_projection: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteOrigin {
    GlobalId,
    Traversal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteVerdict {
    Open,
    Unknown,
    Guarded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorClass {
    None,
    Possible,
    Definite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorContinuity {
    NotApplicable,
    Same,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryFamily {
    SelfScope,
    Visibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundarySource {
    Heuristic,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reachability {
    Reachable,
    QueryUnreachable,
}
