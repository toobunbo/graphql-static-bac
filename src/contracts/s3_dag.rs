use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    ArgumentClassification, BoundaryFamily, Confidence, PathEdgeKind, Reachability,
    RouteOrigin, RouteVerdict, SelectorClass, SelectorContinuity, TypeRef,
};

// ---------------------------------------------------------------------------
// Outer data envelope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagRoutesData {
    pub analysis_model: String,
    pub selector_mode: String,
    pub coverage: DagCoverage,
    pub policy_fingerprint: String,
    pub targets: BTreeMap<String, TargetDag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DagCoverage {
    CompleteGraph,
}

// ---------------------------------------------------------------------------
// Per-target DAG
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetDag {
    pub target_type_id: String,
    pub dag_id: String,
    pub entry_component_id: String,
    pub reachability: Reachability,
    #[serde(default)]
    pub sink_ref_ids: Vec<String>,

    /// Interned abstract states: state_id → DagState
    pub states: BTreeMap<String, DagState>,
    /// Interned schema edges referenced by transitions.
    pub edge_table: Vec<DagEdge>,
    /// Interned selector sets: selector_set_id → sorted selector_ids
    pub selector_sets: BTreeMap<String, Vec<String>>,
    /// Selector metadata keyed by selector_id.
    pub selector_facts: BTreeMap<String, DagSelectorFact>,

    /// Transitions: transition_id → DagTransition
    pub transitions: BTreeMap<String, DagTransition>,
    /// SCC components: component_id → DagComponent
    pub components: BTreeMap<String, DagComponent>,
    /// Inter-component edges (sorted canonically).
    pub component_edges: Vec<DagComponentEdge>,
    /// Terminal records.
    pub terminals: Vec<DagTerminal>,

    pub family_cardinality: FamilyCardinality,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagState {
    pub state_id: String,
    pub type_id: String,
    pub origin_mode: DagOriginMode,
    pub selector_class: SelectorClass,
    pub selector_continuity: SelectorContinuity,
    pub boundary_bits: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_semantic_edge_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DagOriginMode {
    Traversal,
    GlobalIdPrefix,
}

// ---------------------------------------------------------------------------
// Edge table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagEdge {
    pub edge_id: String,
    pub kind: PathEdgeKind,
    pub source_type_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_id: Option<String>,
    pub target_type_id: String,
}

// ---------------------------------------------------------------------------
// Transition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagTransition {
    pub transition_id: String,
    pub source_state_id: String,
    pub target_state_id: String,
    /// Index into `edge_table`.
    pub edge_index: usize,
    pub effect: DagTransitionEffect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector_set_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DagTransitionEffect {
    PassThrough,
    ActivateDefinite,
    TypeCondition,
}

// ---------------------------------------------------------------------------
// Component (SCC)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagComponent {
    pub component_id: String,
    pub member_state_ids: Vec<String>,
    pub internal_transition_ids: Vec<String>,
    pub is_cycle_capable: bool,
    pub is_terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagComponentEdge {
    pub source_component_id: String,
    pub target_component_id: String,
    pub transition_id: String,
}

// ---------------------------------------------------------------------------
// Terminal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagTerminal {
    pub terminal_id: String,
    pub state_id: String,
    pub component_id: String,
    pub verdict: RouteVerdict,
    pub origin: RouteOrigin,
    pub boundary_families: Vec<BoundaryFamily>,
    pub selector_continuity: SelectorContinuity,
    pub terminal_semantic_edge_id: String,
    /// One canonical witness per terminal (diagnostic only).
    pub canonical_witness: DagCanonicalWitness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagCanonicalWitness {
    pub witness_id: String,
    /// Ordered transition IDs from root to terminal.
    pub transition_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Selector fact (stored in DAG for self-contained expansion)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagSelectorFact {
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

// ---------------------------------------------------------------------------
// Cardinality metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilyCardinality {
    pub status: CardinalityStatus,
    pub lower_bound: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardinalityStatus {
    NotMaterialized,
    Exact,
}
