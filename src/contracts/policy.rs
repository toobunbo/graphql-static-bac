use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyHypothesesData {
    pub classifier_model: String,
    pub hypotheses: Vec<PolicyHypothesis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyHypothesis {
    pub hypothesis_id: String,
    pub type_id: String,
    pub field_id: String,
    pub policy_class: PolicyClass,
    pub restricted_values: Vec<Value>,
    pub rule: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyClass {
    BooleanVisibility,
    BooleanPrivacy,
    EnumVisibility,
    EnumPrivacy,
    EnumState,
}
