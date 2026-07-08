use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageId {
    #[serde(rename = "S0")]
    S0,
    #[serde(rename = "S1")]
    S1,
    #[serde(rename = "S2")]
    S2,
    #[serde(rename = "S3")]
    S3,
    #[serde(rename = "S4")]
    S4,
    #[serde(rename = "S5")]
    S5,
    #[serde(rename = "seed_runtime")]
    SeedRuntime,
    #[serde(rename = "policy_hypotheses")]
    PolicyHypotheses,
    #[serde(rename = "policy_oracle")]
    PolicyOracle,
}

impl std::fmt::Display for StageId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::S0 => "S0",
            Self::S1 => "S1",
            Self::S2 => "S2",
            Self::S3 => "S3",
            Self::S4 => "S4",
            Self::S5 => "S5",
            Self::SeedRuntime => "seed_runtime",
            Self::PolicyHypotheses => "policy_hypotheses",
            Self::PolicyOracle => "policy_oracle",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisScope {
    Query,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactStatus {
    Complete,
    Incomplete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    pub name: String,
    pub version: String,
}

impl Producer {
    pub fn current() -> Self {
        Self {
            name: "graphql-static-bac".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl Warning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub contract_version: String,
    pub stage: StageId,
    pub schema_fingerprint: String,
    pub scope: Vec<AnalysisScope>,
    pub producer: Producer,
    pub status: ArtifactStatus,
    pub warnings: Vec<Warning>,
    pub data: T,
}

impl<T> Envelope<T> {
    pub fn complete(
        stage: StageId,
        schema_fingerprint: String,
        producer: Producer,
        warnings: Vec<Warning>,
        data: T,
    ) -> Self {
        Self::complete_with_version("1.0", stage, schema_fingerprint, producer, warnings, data)
    }

    pub fn complete_with_version(
        contract_version: impl Into<String>,
        stage: StageId,
        schema_fingerprint: String,
        producer: Producer,
        warnings: Vec<Warning>,
        data: T,
    ) -> Self {
        Self {
            contract_version: contract_version.into(),
            stage,
            schema_fingerprint,
            scope: vec![AnalysisScope::Query],
            producer,
            status: ArtifactStatus::Complete,
            warnings,
            data,
        }
    }
}
