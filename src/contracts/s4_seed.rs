use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Confidence, PathEdge, TypeRef};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedPlansData {
    pub planning_model: String,
    pub routes: BTreeMap<String, RouteSeedPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteSeedPlan {
    pub route_id: String,
    pub target_type_id: String,
    #[serde(default)]
    pub portfolio_truncated: bool,
    pub requirements: Vec<SeedRequirement>,
    pub correlation_constraints: Vec<CorrelationConstraint>,
    pub producer_jobs: Vec<ProducerJob>,
    pub dependency_dag: DependencyDag,
    pub binding_set_plans: Vec<BindingSetPlan>,
    pub unresolved_requirements: Vec<UnresolvedRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedRequirement {
    pub requirement_id: String,
    pub consumer_arg_ref: String,
    pub root_arg_ref: String,
    pub consumer_field_id: String,
    pub input_path: Vec<String>,
    pub leaf_name: String,
    pub type_ref: TypeRef,
    pub source: RequirementSource,
    pub selected_type_id: String,
    pub static_bindings: Vec<StaticBinding>,
    pub producer_candidates: Vec<ProducerCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StaticBinding {
    pub arg_ref: String,
    pub input_path: Vec<String>,
    pub class: StaticBindingClass,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerCandidate {
    pub producer_field_id: String,
    pub producer_parent_type_id: String,
    pub derivation: ProducerDerivation,
    pub field_locus: ProducerLocus,
    pub type_compatibility: TypeCompatibility,
    pub confidence: Confidence,
    pub automatic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrelationConstraint {
    pub constraint_id: String,
    pub members: Vec<String>,
    pub anchor_type_id: String,
    pub basis: CorrelationBasis,
    pub discharged_by_job_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrelationBasis {
    pub kind: CorrelationBasisKind,
    pub selector_requirement_id: String,
    pub dependent_requirement_id: String,
    pub dependent_field_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerJob {
    pub job_id: String,
    pub strategy: ProducerStrategy,
    #[serde(default)]
    pub producer_priority: u8,
    pub covers_requirements: Vec<String>,
    pub producer_field_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_field_id: Option<String>,
    pub witness: ProducerWitness,
    pub static_bindings: Vec<StaticBinding>,
    pub unresolved_arg_refs: Vec<String>,
    pub extraction: ExtractionPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub executable: bool,
    pub rejection_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerWitness {
    pub edges: Vec<PathEdge>,
    pub terminal_field_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<ExtractionAnchor>,
    pub members: BTreeMap<String, ExtractionMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionAnchor {
    pub type_id: String,
    pub response_path: String,
    pub instance_rule: AnchorInstanceRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionMember {
    pub response_path: String,
    pub relative_path: String,
    pub cardinality: Cardinality,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyDag {
    pub nodes: Vec<String>,
    pub edges: Vec<DependencyEdge>,
    pub acyclic: bool,
    pub execution_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub dependency_id: String,
    pub from_job_id: String,
    pub to_job_id: String,
    pub output_field_id: String,
    pub input_arg_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingSetPlan {
    pub binding_set_plan_id: String,
    pub selected_job_ids: Vec<String>,
    pub discharged_constraint_ids: Vec<String>,
    pub execution_order: Vec<String>,
    pub status: PlanStatus,
    pub unresolved_requirement_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedRequirement {
    pub requirement_id: String,
    pub reason: UnresolvedReason,
    pub details: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementSource {
    SchemaRequired,
    RouteSelector,
    RequiredInputField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticBindingClass {
    SchemaDefault,
    BoundedPagination,
    SchemaEnumValue,
    GeneratedBoolean,
    UnresolvedLiteral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerDerivation {
    ExactLeafMatch,
    RelatedInterfaceField,
    RelatedConcreteField,
    IdentityCompatible,
    TypeOnlySuggestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerLocus {
    Object,
    Interface,
    Concrete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeCompatibility {
    Exact,
    IdString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerStrategy {
    Standalone,
    JointCoRead,
    ThreadedDependency,
    StaticBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationBasisKind {
    RouteLineage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorInstanceRule {
    NearestSharedInstance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    One,
    Optional,
    Many,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Executable,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedReason {
    NoProducerField,
    NoProducerPath,
    UnsupportedLiteral,
    RecursiveDependency,
    DependencyCycle,
    CorrelationUnsatisfied,
}
