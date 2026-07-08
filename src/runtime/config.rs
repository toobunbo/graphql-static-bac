use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeRequestProfile {
    pub request_profile_id: String,
    pub auth_context_id: String,
    pub request: RuntimeRequestTemplate,
    #[serde(default)]
    pub injection: RequestInjection,
    #[serde(default)]
    pub limits: RuntimeLimits,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeRequestTemplate {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestInjection {
    #[serde(default = "default_operation_pointer")]
    pub operation_name_pointer: String,
    #[serde(default = "default_query_pointer")]
    pub query_pointer: String,
    #[serde(default = "default_variables_pointer")]
    pub variables_pointer: String,
}

impl Default for RequestInjection {
    fn default() -> Self {
        Self {
            operation_name_pointer: default_operation_pointer(),
            query_pointer: default_query_pointer(),
            variables_pointer: default_variables_pointer(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLimits {
    #[serde(default)]
    pub execution_mode: RuntimeExecutionMode,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_max_requests")]
    pub max_requests: usize,
    #[serde(default = "default_max_requests_per_route")]
    pub max_requests_per_route: usize,
    #[serde(default = "default_max_producers_per_route")]
    pub max_producers_per_route: usize,
    #[serde(default = "default_max_values_per_producer")]
    pub max_values_per_producer: usize,
    #[serde(default = "default_max_adapter_attempts")]
    pub max_adapter_attempts_per_binding_set: usize,
    #[serde(default = "default_max_values")]
    pub max_values_per_requirement: usize,
    #[serde(default = "default_max_bindings")]
    pub max_verified_bindings_per_route: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            execution_mode: RuntimeExecutionMode::default(),
            timeout_ms: default_timeout(),
            max_requests: default_max_requests(),
            max_requests_per_route: default_max_requests_per_route(),
            max_producers_per_route: default_max_producers_per_route(),
            max_values_per_producer: default_max_values_per_producer(),
            max_adapter_attempts_per_binding_set: default_max_adapter_attempts(),
            max_values_per_requirement: default_max_values(),
            max_verified_bindings_per_route: default_max_bindings(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionMode {
    #[default]
    FirstVerified,
    Exhaustive,
}

fn default_method() -> String {
    "POST".to_string()
}
fn default_operation_pointer() -> String {
    "/operationName".to_string()
}
fn default_query_pointer() -> String {
    "/query".to_string()
}
fn default_variables_pointer() -> String {
    "/variables".to_string()
}
fn default_timeout() -> u64 {
    15_000
}
fn default_max_requests() -> usize {
    500
}
fn default_max_requests_per_route() -> usize {
    3
}
fn default_max_producers_per_route() -> usize {
    3
}
fn default_max_values_per_producer() -> usize {
    3
}
fn default_max_adapter_attempts() -> usize {
    2
}
fn default_max_values() -> usize {
    100
}
fn default_max_bindings() -> usize {
    20
}
