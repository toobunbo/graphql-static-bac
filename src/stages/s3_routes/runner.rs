use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use thiserror::Error;

use crate::artifact::{
    read_artifact, validate_envelope, write_json_atomic, ArtifactReadError, ArtifactWriteError,
};
use crate::contracts::{
    ArgumentsData, Envelope, Producer, RouteCoverage, RoutesData, SchemaIrData, SinksData, StageId,
    TypeKind,
};
use crate::graph::{build_type_graph, GraphBuildError};
use crate::route::{
    analyze_target, read_route_policy, LoadedRoutePolicy, RouteAnalysisError, RouteFacts,
    RouteFactsError, RoutePolicyError,
};

#[derive(Debug, Error)]
pub enum S3RouteError {
    #[error(transparent)]
    Read(#[from] ArtifactReadError),
    #[error(transparent)]
    Policy(#[from] RoutePolicyError),
    #[error(transparent)]
    Graph(#[from] GraphBuildError),
    #[error(transparent)]
    Facts(#[from] RouteFactsError),
    #[error(transparent)]
    Analysis(#[from] RouteAnalysisError),
    #[error("cross-stage contract violation: {0}")]
    Contract(String),
    #[error(transparent)]
    Write(#[from] ArtifactWriteError),
}

pub fn analyze_schema_target(
    schema: &Envelope<SchemaIrData>,
    arguments: &Envelope<ArgumentsData>,
    policy: &LoadedRoutePolicy,
    target: &str,
) -> Result<Envelope<RoutesData>, S3RouteError> {
    validate_pair(schema, arguments)?;
    let graph = build_type_graph(&schema.data)?;
    let facts = RouteFacts::build(&schema.data, &arguments.data, &policy.policy)?;
    let target_type_id = normalize_type_id(target);
    let target_routes = analyze_target(&graph, &target_type_id, &facts)?;
    let mut targets = BTreeMap::new();
    targets.insert(target_type_id, target_routes);
    Ok(route_envelope(schema, policy, targets))
}

pub fn analyze_selected_targets(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
    arguments: &Envelope<ArgumentsData>,
    policy: &LoadedRoutePolicy,
) -> Result<Envelope<RoutesData>, S3RouteError> {
    validate_inputs(schema, sinks, arguments)?;
    let graph = build_type_graph(&schema.data)?;
    let facts = RouteFacts::build(&schema.data, &arguments.data, &policy.policy)?;
    let mut targets = BTreeMap::new();
    let mut seen_targets = BTreeSet::new();

    for selected in &sinks.data.selected_types {
        validate_selected_target(schema, sinks, selected, &mut seen_targets)?;
        let mut target_routes = analyze_target(&graph, &selected.type_id, &facts)?;
        if target_routes.routes.is_empty() {
            return Err(S3RouteError::Contract(format!(
                "S1-selected target {} is query-unreachable",
                selected.type_id
            )));
        }
        let mut sink_ref_ids = selected.sink_ref_ids.clone();
        sink_ref_ids.sort();
        sink_ref_ids.dedup();
        target_routes.sink_ref_ids = sink_ref_ids;
        targets.insert(selected.type_id.clone(), target_routes);
    }

    Ok(route_envelope(schema, policy, targets))
}

pub fn read_and_run_routes(
    schema_path: &Path,
    sinks_path: &Path,
    arguments_path: &Path,
    policy_path: &Path,
) -> Result<Envelope<RoutesData>, S3RouteError> {
    let schema = read_artifact(schema_path, StageId::S0)?;
    let sinks = read_artifact(sinks_path, StageId::S1)?;
    let arguments = read_artifact(arguments_path, StageId::S2)?;
    let policy = read_route_policy(policy_path)?;
    analyze_selected_targets(&schema, &sinks, &arguments, &policy)
}

pub fn write_routes(artifact: &Envelope<RoutesData>, output: &Path) -> Result<(), S3RouteError> {
    write_json_atomic(artifact, output)?;
    Ok(())
}

fn route_envelope(
    schema: &Envelope<SchemaIrData>,
    policy: &LoadedRoutePolicy,
    targets: BTreeMap<String, crate::contracts::TargetRoutes>,
) -> Envelope<RoutesData> {
    Envelope::complete_with_version(
        "2.2",
        StageId::S3,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        RoutesData {
            analysis_model: "abstract-automaton-canonical-v1".to_string(),
            policy_fingerprint: policy.fingerprint.clone(),
            coverage: RouteCoverage::CanonicalPerProvenance,
            targets,
        },
    )
}

fn validate_pair(
    schema: &Envelope<SchemaIrData>,
    arguments: &Envelope<ArgumentsData>,
) -> Result<(), S3RouteError> {
    validate_envelope(schema, StageId::S0)?;
    validate_envelope(arguments, StageId::S2)?;
    if schema.schema_fingerprint != arguments.schema_fingerprint {
        return Err(S3RouteError::Contract(
            "S0 and S2 schema fingerprints differ".to_string(),
        ));
    }
    Ok(())
}

fn validate_inputs(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
    arguments: &Envelope<ArgumentsData>,
) -> Result<(), S3RouteError> {
    validate_pair(schema, arguments)?;
    validate_envelope(sinks, StageId::S1)?;
    if schema.schema_fingerprint != sinks.schema_fingerprint {
        return Err(S3RouteError::Contract(
            "S0 and S1 schema fingerprints differ".to_string(),
        ));
    }
    for (key, sink_ref) in &sinks.data.sink_refs {
        if key != &sink_ref.sink_ref_id {
            return Err(S3RouteError::Contract(format!(
                "sink ref map key differs from ID: {key}"
            )));
        }
    }
    Ok(())
}

fn validate_selected_target(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
    selected: &crate::contracts::SelectedType,
    seen_targets: &mut BTreeSet<String>,
) -> Result<(), S3RouteError> {
    if !seen_targets.insert(selected.type_id.clone()) {
        return Err(S3RouteError::Contract(format!(
            "duplicate selected target {}",
            selected.type_id
        )));
    }
    let type_name = selected
        .type_id
        .strip_prefix("type:")
        .unwrap_or(&selected.type_id);
    let definition = schema.data.types.get(type_name).ok_or_else(|| {
        S3RouteError::Contract(format!(
            "selected target {} is missing from S0",
            selected.type_id
        ))
    })?;
    if !matches!(
        definition.kind,
        TypeKind::Object | TypeKind::Interface | TypeKind::Union
    ) {
        return Err(S3RouteError::Contract(format!(
            "selected target {} is not a composite output type",
            selected.type_id
        )));
    }
    if definition.type_id != selected.type_id || selected.type_name != type_name {
        return Err(S3RouteError::Contract(format!(
            "selected target identity mismatch for {}",
            selected.type_id
        )));
    }

    let mut sink_ref_ids = selected.sink_ref_ids.clone();
    sink_ref_ids.sort();
    sink_ref_ids.dedup();
    if sink_ref_ids.len() != selected.sink_ref_ids.len() {
        return Err(S3RouteError::Contract(format!(
            "duplicate sink ref for {}",
            selected.type_id
        )));
    }
    for sink_ref_id in sink_ref_ids {
        let sink_ref = sinks.data.sink_refs.get(&sink_ref_id).ok_or_else(|| {
            S3RouteError::Contract(format!(
                "selected target {} references missing sink {}",
                selected.type_id, sink_ref_id
            ))
        })?;
        if sink_ref.sink_ref_id != sink_ref_id || sink_ref.type_id != selected.type_id {
            return Err(S3RouteError::Contract(format!(
                "sink {sink_ref_id} does not belong to {}",
                selected.type_id
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
