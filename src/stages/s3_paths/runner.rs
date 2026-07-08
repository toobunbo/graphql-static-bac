use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use thiserror::Error;

use crate::artifact::{
    read_artifact, validate_envelope, write_json_atomic, ArtifactReadError, ArtifactWriteError,
};
use crate::contracts::{
    ArtifactStatus, Envelope, PathsData, Producer, SchemaIrData, SinksData, StageId, TypeKind,
};
use crate::graph::{build_type_graph, GraphBuildError};

use super::{enumerate_target, EnumerationError};

#[derive(Debug, Error)]
pub enum S3Error {
    #[error(transparent)]
    Read(#[from] ArtifactReadError),
    #[error(transparent)]
    Graph(#[from] GraphBuildError),
    #[error(transparent)]
    Enumeration(#[from] EnumerationError),
    #[error("cross-stage contract violation: {0}")]
    Contract(String),
    #[error(transparent)]
    Write(#[from] ArtifactWriteError),
}

pub fn enumerate_schema_target(
    schema: &Envelope<SchemaIrData>,
    target: &str,
) -> Result<Envelope<PathsData>, S3Error> {
    validate_envelope(schema, StageId::S0)?;
    let graph = build_type_graph(&schema.data)?;
    let target_type_id = normalize_type_id(target);
    let target_paths = enumerate_target(&graph, &target_type_id)?;
    let mut targets = BTreeMap::new();
    targets.insert(target_type_id, target_paths);
    Ok(Envelope::complete(
        StageId::S3,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        PathsData { targets },
    ))
}

pub fn enumerate_selected_targets(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
) -> Result<Envelope<PathsData>, S3Error> {
    validate_envelope(schema, StageId::S0)?;
    validate_envelope(sinks, StageId::S1)?;
    validate_inputs(schema, sinks)?;
    let graph = build_type_graph(&schema.data)?;
    let mut targets = BTreeMap::new();
    let mut seen_targets = BTreeSet::new();

    for selected in &sinks.data.selected_types {
        if !seen_targets.insert(selected.type_id.clone()) {
            return Err(S3Error::Contract(format!(
                "duplicate selected target {}",
                selected.type_id
            )));
        }
        let type_name = selected
            .type_id
            .strip_prefix("type:")
            .unwrap_or(&selected.type_id);
        let Some(definition) = schema.data.types.get(type_name) else {
            return Err(S3Error::Contract(format!(
                "selected target {} is missing from S0",
                selected.type_id
            )));
        };
        if definition.kind != TypeKind::Object {
            return Err(S3Error::Contract(format!(
                "selected target {} is not OBJECT",
                selected.type_id
            )));
        }
        if definition.type_id != selected.type_id || selected.type_name != type_name {
            return Err(S3Error::Contract(format!(
                "selected target identity mismatch for {}",
                selected.type_id
            )));
        }

        let mut sink_ref_ids = selected.sink_ref_ids.clone();
        sink_ref_ids.sort();
        sink_ref_ids.dedup();
        if sink_ref_ids.len() != selected.sink_ref_ids.len() {
            return Err(S3Error::Contract(format!(
                "duplicate sink ref for {}",
                selected.type_id
            )));
        }
        for sink_ref_id in &sink_ref_ids {
            let Some(sink_ref) = sinks.data.sink_refs.get(sink_ref_id) else {
                return Err(S3Error::Contract(format!(
                    "selected target {} references missing sink {}",
                    selected.type_id, sink_ref_id
                )));
            };
            if sink_ref.sink_ref_id != *sink_ref_id || sink_ref.type_id != selected.type_id {
                return Err(S3Error::Contract(format!(
                    "sink {sink_ref_id} does not belong to {}",
                    selected.type_id
                )));
            }
        }

        let mut target_paths = enumerate_target(&graph, &selected.type_id)?;
        if target_paths.caps.is_empty() {
            return Err(S3Error::Contract(format!(
                "selected Query-reachable target {} has no CAP",
                selected.type_id
            )));
        }
        target_paths.sink_ref_ids = sink_ref_ids;
        targets.insert(selected.type_id.clone(), target_paths);
    }

    Ok(Envelope::complete(
        StageId::S3,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        PathsData { targets },
    ))
}

pub fn read_and_run_s3(
    schema_path: &Path,
    sinks_path: &Path,
) -> Result<Envelope<PathsData>, S3Error> {
    let schema = read_artifact(schema_path, StageId::S0)?;
    let sinks = read_artifact(sinks_path, StageId::S1)?;
    enumerate_selected_targets(&schema, &sinks)
}

pub fn write_paths(artifact: &Envelope<PathsData>, output: &Path) -> Result<(), S3Error> {
    write_json_atomic(artifact, output)?;
    Ok(())
}

fn validate_inputs(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
) -> Result<(), S3Error> {
    if schema.stage != StageId::S0
        || sinks.stage != StageId::S1
        || schema.status != ArtifactStatus::Complete
        || sinks.status != ArtifactStatus::Complete
    {
        return Err(S3Error::Contract(
            "S3 requires complete S0 and S1 artifacts".to_string(),
        ));
    }
    if schema.schema_fingerprint != sinks.schema_fingerprint {
        return Err(S3Error::Contract(
            "S0 and S1 schema fingerprints differ".to_string(),
        ));
    }
    for (key, sink_ref) in &sinks.data.sink_refs {
        if key != &sink_ref.sink_ref_id {
            return Err(S3Error::Contract(format!(
                "sink ref map key differs from ID: {key}"
            )));
        }
    }
    Ok(())
}

fn normalize_type_id(target: &str) -> String {
    if target.starts_with("type:") {
        target.to_string()
    } else {
        format!("type:{target}")
    }
}
