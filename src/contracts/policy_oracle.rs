use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyOracleData {
    pub oracle_model: String,
    pub owner_request_profile_id: String,
    pub owner_auth_context_id: String,
    pub observer_request_profile_id: String,
    pub observer_auth_context_id: String,
    pub results: Vec<PolicyOracleResult>,
    pub executions: BTreeMap<String, PolicyOracleExecution>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyOracleResult {
    pub run_id: String,
    pub hypothesis_id: String,
    pub route_id: String,
    pub binding_set_id: String,
    pub outcome: PolicyOracleOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub victim: Option<PolicyVictim>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<PolicyOracleFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyVictim {
    pub target_identity: Value,
    pub policy_value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyOracleExecution {
    pub execution_id: String,
    pub phase: PolicyOraclePhase,
    pub hypothesis_id: String,
    pub route_id: String,
    pub binding_set_id: String,
    pub auth_context_id: String,
    pub request: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    pub status: PolicyOracleExecutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<PolicyOracleFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyOracleFailure {
    pub code: PolicyOracleFailureCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyViolationCandidates {
    pub candidates: Vec<PolicyViolationCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyViolationCandidate {
    pub verdict: PolicyViolationVerdict,
    #[serde(rename = "type")]
    pub type_name: String,
    pub route_id: String,
    pub auth_context: PolicyCandidateAuthContext,
    pub response: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PolicyCandidateAuthContext {
    pub owner: String,
    pub observer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOracleOutcome {
    PolicyViolationCandidate,
    NoViolationObserved,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyViolationVerdict {
    PolicyViolationCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOraclePhase {
    OwnerCalibration,
    ObserverReplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOracleExecutionStatus {
    Succeeded,
    HttpError,
    GraphqlError,
    TransportError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOracleFailureCode {
    PolicyFieldMissing,
    PolicyFieldTypeMismatch,
    OwnerSeedUnavailable,
    OwnerRouteFailed,
    RestrictedObjectNotFound,
    TargetIdentityUnavailable,
    RouteRebindingFailed,
    ObserverAuthFailed,
    ObserverHttpError,
    ObserverGraphqlError,
    ResponseParseFailed,
    RequestBudgetExhausted,
    VictimIdentityNotObserved,
}
