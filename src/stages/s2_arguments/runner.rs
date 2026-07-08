use std::path::Path;

use thiserror::Error;

use crate::argument::{
    classify_arguments, read_argument_policy, ArgumentClassifierError, ArgumentPolicyError,
    LoadedArgumentPolicy,
};
use crate::artifact::{
    read_artifact, validate_envelope, write_json_atomic, ArtifactReadError, ArtifactWriteError,
};
use crate::contracts::{ArgumentsData, Envelope, Producer, SchemaIrData, StageId};
use crate::schema::{validate_schema_ir, ValidationError};

#[derive(Debug, Error)]
pub enum S2ArgumentError {
    #[error(transparent)]
    Read(#[from] ArtifactReadError),
    #[error(transparent)]
    Policy(#[from] ArgumentPolicyError),
    #[error(transparent)]
    Schema(#[from] ValidationError),
    #[error(transparent)]
    Classification(#[from] ArgumentClassifierError),
    #[error(transparent)]
    Write(#[from] ArtifactWriteError),
}

pub fn classify_schema_arguments(
    schema: &Envelope<SchemaIrData>,
    policy: &LoadedArgumentPolicy,
) -> Result<Envelope<ArgumentsData>, S2ArgumentError> {
    validate_envelope(schema, StageId::S0)?;
    validate_schema_ir(&schema.data)?;
    let data = classify_arguments(&schema.data, policy)?;
    Ok(Envelope::complete(
        StageId::S2,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        data,
    ))
}

pub fn read_and_run_arguments(
    schema_path: &Path,
    policy_path: &Path,
) -> Result<Envelope<ArgumentsData>, S2ArgumentError> {
    let schema = read_artifact(schema_path, StageId::S0)?;
    let policy = read_argument_policy(policy_path)?;
    classify_schema_arguments(&schema, &policy)
}

pub fn write_arguments(
    artifact: &Envelope<ArgumentsData>,
    output: &Path,
) -> Result<(), S2ArgumentError> {
    write_json_atomic(artifact, output)?;
    Ok(())
}
