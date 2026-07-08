use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArtifactWriteError {
    #[error("could not serialize artifact: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("could not write artifact {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

pub fn write_json_atomic<T: Serialize>(value: &T, output: &Path) -> Result<(), ArtifactWriteError> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| ArtifactWriteError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    let temp_path = temporary_path(output);
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|source| ArtifactWriteError::Io {
                path: temp_path.clone(),
                source,
            })?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|source| ArtifactWriteError::Io {
                path: temp_path.clone(),
                source,
            })?;
        fs::rename(&temp_path, output).map_err(|source| ArtifactWriteError::Io {
            path: output.to_path_buf(),
            source,
        })?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn temporary_path(output: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact.json");
    output.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce))
}
