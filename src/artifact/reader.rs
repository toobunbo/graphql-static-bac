use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::contracts::{AnalysisScope, ArtifactStatus, Envelope, StageId};

#[derive(Debug, Error)]
pub enum ArtifactReadError {
    #[error("could not read artifact {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse artifact {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid artifact contract: {0}")]
    Contract(String),
}

pub fn read_artifact<T: DeserializeOwned>(
    path: &Path,
    expected_stage: StageId,
) -> Result<Envelope<T>, ArtifactReadError> {
    read_artifact_version(path, expected_stage, "1.0")
}

pub fn read_artifact_version<T: DeserializeOwned>(
    path: &Path,
    expected_stage: StageId,
    expected_version: &str,
) -> Result<Envelope<T>, ArtifactReadError> {
    let bytes = fs::read(path).map_err(|source| ArtifactReadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let header =
        serde_json::from_slice::<Envelope<serde_json::Value>>(&bytes).map_err(|source| {
            ArtifactReadError::Json {
                path: path.to_path_buf(),
                source,
            }
        })?;
    validate_envelope_version(&header, expected_stage, expected_version)?;
    let artifact = serde_json::from_slice::<Envelope<T>>(&bytes).map_err(|source| {
        ArtifactReadError::Json {
            path: path.to_path_buf(),
            source,
        }
    })?;
    validate_envelope_version(&artifact, expected_stage, expected_version)?;
    Ok(artifact)
}

pub fn read_artifact_versions<T: DeserializeOwned>(
    path: &Path,
    expected_stage: StageId,
    expected_versions: &[&str],
) -> Result<Envelope<T>, ArtifactReadError> {
    let bytes = fs::read(path).map_err(|source| ArtifactReadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let header =
        serde_json::from_slice::<Envelope<serde_json::Value>>(&bytes).map_err(|source| {
            ArtifactReadError::Json {
                path: path.to_path_buf(),
                source,
            }
        })?;
    validate_envelope_versions(&header, expected_stage, expected_versions)?;
    let artifact = serde_json::from_slice::<Envelope<T>>(&bytes).map_err(|source| {
        ArtifactReadError::Json {
            path: path.to_path_buf(),
            source,
        }
    })?;
    validate_envelope_versions(&artifact, expected_stage, expected_versions)?;
    Ok(artifact)
}

pub fn validate_envelope<T>(
    artifact: &Envelope<T>,
    expected_stage: StageId,
) -> Result<(), ArtifactReadError> {
    validate_envelope_version(artifact, expected_stage, "1.0")
}

pub fn validate_envelope_version<T>(
    artifact: &Envelope<T>,
    expected_stage: StageId,
    expected_version: &str,
) -> Result<(), ArtifactReadError> {
    if artifact.contract_version != expected_version {
        return Err(ArtifactReadError::Contract(format!(
            "expected contract_version {expected_version}, got {}",
            artifact.contract_version,
        )));
    }
    if artifact.stage != expected_stage {
        return Err(ArtifactReadError::Contract(format!(
            "expected stage {expected_stage}, got {}",
            artifact.stage
        )));
    }
    if artifact.scope != [AnalysisScope::Query] {
        return Err(ArtifactReadError::Contract(
            "scope must be exactly [\"query\"]".to_string(),
        ));
    }
    if artifact.status != ArtifactStatus::Complete {
        return Err(ArtifactReadError::Contract(format!(
            "input stage {expected_stage} is not complete"
        )));
    }
    Ok(())
}

pub fn validate_envelope_versions<T>(
    artifact: &Envelope<T>,
    expected_stage: StageId,
    expected_versions: &[&str],
) -> Result<(), ArtifactReadError> {
    if !expected_versions
        .iter()
        .any(|version| artifact.contract_version == *version)
    {
        return Err(ArtifactReadError::Contract(format!(
            "expected contract_version one of [{}], got {}",
            expected_versions.join(", "),
            artifact.contract_version,
        )));
    }
    if artifact.stage != expected_stage {
        return Err(ArtifactReadError::Contract(format!(
            "expected stage {expected_stage}, got {}",
            artifact.stage
        )));
    }
    if artifact.scope != [AnalysisScope::Query] {
        return Err(ArtifactReadError::Contract(
            "scope must be exactly [\"query\"]".to_string(),
        ));
    }
    if artifact.status != ArtifactStatus::Complete {
        return Err(ArtifactReadError::Contract(format!(
            "input stage {expected_stage} is not complete"
        )));
    }
    Ok(())
}
