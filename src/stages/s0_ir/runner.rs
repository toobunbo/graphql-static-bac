use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::artifact::{write_json_atomic, ArtifactWriteError};
use crate::contracts::{Envelope, Producer, SchemaIrData, StageId};
use crate::schema::{
    fingerprint, parse_introspection, parse_sdl, read_source, validate_schema_ir, SchemaFormat,
    SchemaParseError, SourceError, ValidationError,
};

#[derive(Debug, Clone)]
pub struct S0Options {
    pub input: PathBuf,
    pub format: SchemaFormat,
    pub producer: Producer,
}

#[derive(Debug, Error)]
pub enum S0Error {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Parse(#[from] SchemaParseError),
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error(transparent)]
    Write(#[from] ArtifactWriteError),
}

pub fn build_schema_ir(options: &S0Options) -> Result<Envelope<SchemaIrData>, S0Error> {
    let bytes = read_source(&options.input)?;
    let schema_fingerprint = fingerprint(&bytes);
    let format = options.format.resolve(&options.input, &bytes)?;
    let parsed = match format {
        SchemaFormat::Introspection => parse_introspection(&bytes)?,
        SchemaFormat::Sdl => {
            let text = std::str::from_utf8(&bytes).map_err(SourceError::from)?;
            parse_sdl(text)?
        }
        SchemaFormat::Auto => unreachable!("auto format is resolved before parsing"),
    };
    validate_schema_ir(&parsed.data)?;
    Ok(Envelope::complete(
        StageId::S0,
        schema_fingerprint,
        options.producer.clone(),
        parsed.warnings,
        parsed.data,
    ))
}

pub fn write_schema_ir(artifact: &Envelope<SchemaIrData>, output: &Path) -> Result<(), S0Error> {
    write_json_atomic(artifact, output)?;
    Ok(())
}
