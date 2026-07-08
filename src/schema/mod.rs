mod introspection;
mod sdl;
mod source;
mod validate;

pub use introspection::parse_introspection;
pub use sdl::parse_sdl;
pub use source::{fingerprint, read_source, SchemaFormat, SourceError};
pub use validate::{validate_schema_ir, ValidationError};

use thiserror::Error;

use crate::contracts::{SchemaIrData, Warning};

#[derive(Debug, Error)]
pub enum SchemaParseError {
    #[error("invalid introspection JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("introspection response does not contain __schema")]
    MissingSchema,
    #[error("invalid introspection schema: {0}")]
    InvalidIntrospection(String),
    #[error("invalid SDL: {0}")]
    InvalidSdl(String),
    #[error("duplicate or incompatible schema definition: {0}")]
    DuplicateDefinition(String),
    #[error("unknown GraphQL type kind: {0}")]
    UnknownKind(String),
    #[error("type reference does not terminate in a named type")]
    TruncatedTypeRef,
}

pub struct ParsedSchema {
    pub data: SchemaIrData,
    pub warnings: Vec<Warning>,
}
