use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaFormat {
    Auto,
    Introspection,
    Sdl,
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("could not read schema source {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("schema source is not valid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("could not detect schema format; pass --format explicitly")]
    UnknownFormat,
}

pub fn read_source(path: &Path) -> Result<Vec<u8>, SourceError> {
    fs::read(path).map_err(|source| SourceError::Read {
        path: path.to_path_buf(),
        source,
    })
}

pub fn fingerprint(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

impl SchemaFormat {
    pub fn resolve(self, path: &Path, bytes: &[u8]) -> Result<Self, SourceError> {
        if self != Self::Auto {
            return Ok(self);
        }
        match path.extension().and_then(|value| value.to_str()) {
            Some("json") => return Ok(Self::Introspection),
            Some("graphql" | "graphqls" | "gql") => return Ok(Self::Sdl),
            _ => {}
        }
        let text = std::str::from_utf8(bytes)?;
        let first = text.trim_start().chars().next();
        match first {
            Some('{') => Ok(Self::Introspection),
            Some(_) => Ok(Self::Sdl),
            None => Err(SourceError::UnknownFormat),
        }
    }
}
