use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathsData {
    pub targets: BTreeMap<String, TargetPaths>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetPaths {
    pub target_type_id: String,
    pub sink_ref_ids: Vec<String>,
    pub enumeration_status: EnumerationStatus,
    pub caps: Vec<CandidateAccessPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateAccessPath {
    pub cap_id: String,
    pub origin: CapOrigin,
    pub entry_field_id: String,
    pub target_type_id: String,
    pub edges: Vec<PathEdge>,
    pub cycle_templates: Vec<CycleTemplate>,
    pub display_projection: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapOrigin {
    GlobalId,
    Traversal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PathEdgeKind {
    Field,
    TypeCondition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathEdge {
    pub edge_id: String,
    pub kind: PathEdgeKind,
    pub source_type_id: String,
    pub field_id: Option<String>,
    pub target_type_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CycleTemplate {
    pub repeated_edge_id: String,
    pub cycle_start_index: usize,
    pub repeatable_edge_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnumerationStatus {
    Complete,
    Incomplete,
    Failed,
}
