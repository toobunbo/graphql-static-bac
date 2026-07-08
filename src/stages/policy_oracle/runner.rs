use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::artifact::{
    read_artifact, read_artifact_versions, write_json_atomic, ArtifactReadError,
    ArtifactWriteError,
};
use crate::contracts::{
    Envelope, PolicyHypothesesData, PolicyOracleData, PolicyViolationCandidates, RoutesData,
    SchemaIrData, SeedPlansData, SeedRuntimeData, StageId,
};
use crate::oracle::{run_policy_oracle, PolicyOracleError};
use crate::runtime::{CurlTransport, RuntimeRequestProfile, RuntimeSelection};

#[derive(Debug, Error)]
pub enum PolicyOracleStageError {
    #[error(transparent)]
    Read(#[from] ArtifactReadError),
    #[error("could not read runtime request profile {path}: {source}")]
    ProfileIo {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse runtime request profile {path}: {source}")]
    ProfileJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Oracle(#[from] PolicyOracleError),
    #[error(transparent)]
    Write(#[from] ArtifactWriteError),
}

#[allow(clippy::too_many_arguments)]
pub fn read_and_run_policy_oracle(
    schema_path: &Path,
    routes_path: &Path,
    seed_plans_path: &Path,
    owner_seeds_path: &Path,
    hypotheses_path: &Path,
    owner_profile_path: &Path,
    observer_profile_path: &Path,
    selection: &RuntimeSelection,
) -> Result<(Envelope<PolicyOracleData>, PolicyViolationCandidates), PolicyOracleStageError> {
    let schema = read_artifact::<SchemaIrData>(schema_path, StageId::S0)?;
    let routes =
        read_artifact_versions::<RoutesData>(routes_path, StageId::S3, &["2.1", "2.2"])?;
    let seed_plans = read_artifact::<SeedPlansData>(seed_plans_path, StageId::S4)?;
    let owner_seeds = read_artifact::<SeedRuntimeData>(owner_seeds_path, StageId::SeedRuntime)?;
    let hypotheses =
        read_artifact::<PolicyHypothesesData>(hypotheses_path, StageId::PolicyHypotheses)?;
    let owner_profile = read_profile(owner_profile_path)?;
    let observer_profile = read_profile(observer_profile_path)?;
    Ok(run_policy_oracle(
        &schema,
        &routes,
        &seed_plans,
        &owner_seeds,
        &hypotheses,
        &owner_profile,
        &observer_profile,
        selection,
        &CurlTransport,
    )?)
}

pub fn write_policy_oracle(
    artifact: &Envelope<PolicyOracleData>,
    candidates: &PolicyViolationCandidates,
    runtime_output: &Path,
    candidates_output: &Path,
) -> Result<(), PolicyOracleStageError> {
    write_json_atomic(artifact, runtime_output)?;
    write_json_atomic(candidates, candidates_output)?;
    Ok(())
}

fn read_profile(path: &Path) -> Result<RuntimeRequestProfile, PolicyOracleStageError> {
    let bytes = fs::read(path).map_err(|source| PolicyOracleStageError::ProfileIo {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| PolicyOracleStageError::ProfileJson {
        path: path.to_path_buf(),
        source,
    })
}
