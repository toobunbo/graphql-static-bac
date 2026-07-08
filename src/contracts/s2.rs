use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::TypeRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgumentsData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_fingerprint: Option<String>,
    pub fields: BTreeMap<String, ClassifiedField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedField {
    pub arguments: Vec<ClassifiedArgument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedArgument {
    pub arg_ref: String,
    pub root_arg_ref: String,
    pub arg_path: String,
    pub input_path: Vec<String>,
    pub type_ref: TypeRef,
    pub classifications: Vec<ArgumentClassification>,
    pub signals: Vec<String>,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentClassification {
    ObjectSelector,
    AuthzModifier,
    Noise,
    PossibleSelector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}
