use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::artifact::{
    read_artifact, read_artifact_versions, write_json_atomic, ArtifactReadError,
    ArtifactWriteError,
};
use crate::contracts::{
    Envelope, RoutesData, SchemaIrData, SeedPlansData, SeedRuntimeData, StageId,
};
use crate::runtime::{
    run_seed_runtime, CurlTransport, RuntimeRequestProfile, RuntimeSelection, SeedRuntimeError,
};

#[derive(Debug, Error)]
pub enum SeedRuntimeStageError {
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
    Runtime(#[from] SeedRuntimeError),
    #[error(transparent)]
    Write(#[from] ArtifactWriteError),
}

pub fn read_and_run_seed_runtime(
    schema_path: &Path,
    routes_path: &Path,
    seed_plans_path: &Path,
    request_profile_path: &Path,
    selection: &RuntimeSelection,
) -> Result<Envelope<SeedRuntimeData>, SeedRuntimeStageError> {
    let schema = read_artifact::<SchemaIrData>(schema_path, StageId::S0)?;
    let routes =
        read_artifact_versions::<RoutesData>(routes_path, StageId::S3, &["2.1", "2.2"])?;
    let seed_plans = read_artifact::<SeedPlansData>(seed_plans_path, StageId::S4)?;
    let profile = read_profile(request_profile_path)?;
    Ok(run_seed_runtime(
        &schema,
        &routes,
        &seed_plans,
        &profile,
        selection,
        &CurlTransport,
    )?)
}

pub fn write_seed_runtime(
    artifact: &Envelope<SeedRuntimeData>,
    output: &Path,
) -> Result<(), SeedRuntimeStageError> {
    write_json_atomic(artifact, output)?;
    Ok(())
}

fn read_profile(path: &Path) -> Result<RuntimeRequestProfile, SeedRuntimeStageError> {
    let bytes = fs::read(path).map_err(|source| SeedRuntimeStageError::ProfileIo {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| SeedRuntimeStageError::ProfileJson {
        path: path.to_path_buf(),
        source,
    })
}
