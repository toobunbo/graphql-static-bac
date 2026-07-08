use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeedRuntimeData {
    pub runtime_model: String,
    pub request_profile_id: String,
    pub auth_context_id: String,
    pub route_bindings: BTreeMap<String, RouteRuntimeResult>,
    pub executions: BTreeMap<String, ExecutionRecord>,
    pub runtime_facts: Vec<RuntimeFact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRuntimeResult {
    pub route_id: String,
    pub target_type_id: String,
    pub status: RouteRuntimeStatus,
    pub verified_binding_sets: Vec<VerifiedBindingSet>,
    pub attempted_plan_ids: Vec<String>,
    pub failures: Vec<RuntimeFailure>,
    pub coverage: RuntimeCoverage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifiedBindingSet {
    pub binding_set_id: String,
    pub source_binding_set_plan_id: String,
    pub bindings: BTreeMap<String, RuntimeBinding>,
    pub producer_execution_ids: Vec<String>,
    pub validation_execution_id: String,
    pub validation: BindingValidation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeBinding {
    pub requirement_id: String,
    pub input_path: Vec<String>,
    pub source_value: Value,
    pub consumer_value: Value,
    pub adapter_chain: Vec<String>,
    pub producer_job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer_execution_id: Option<String>,
    pub extraction_provenance: Vec<ExtractionProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionProvenance {
    pub response_path: String,
    pub branch_path: String,
    pub list_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub kind: ExecutionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    pub route_id: String,
    pub request: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    pub status: RuntimeExecutionStatus,
    pub extracted_values: BTreeMap<String, Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<RuntimeFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindingValidation {
    pub status: BindingValidationStatus,
    pub target_type_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_typename: Option<String>,
    pub adapter_attempts: Vec<AdapterAttempt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterAttempt {
    pub adapter_chain: Vec<String>,
    pub status: BindingValidationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFailure {
    pub code: FailureCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFact {
    pub kind: String,
    pub subject: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteRuntimeStatus {
    Verified,
    Unresolved,
    NoSeedRequired,
    BudgetExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCoverage {
    Complete,
    Bounded,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionKind {
    Producer,
    Validation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionStatus {
    NotRun,
    Empty,
    HttpError,
    GraphqlError,
    ExtractionFailed,
    Extracted,
    Verified,
    ConsumerRejected,
    TargetNotReached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingValidationStatus {
    NotRun,
    ConsumerRejected,
    TargetNotReached,
    RuntimeError,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    ProducerEmpty,
    ProducerHttpError,
    ProducerGraphqlError,
    ExtractionPathMissing,
    ExtractionShapeMismatch,
    RepresentationMismatch,
    ConsumerRejected,
    TargetNotReached,
    DependencyUnresolved,
    ProducerExhausted,
    CoverageIncomplete,
    RequestBudgetExhausted,
}
