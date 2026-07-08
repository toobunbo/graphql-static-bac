use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use thiserror::Error;

use crate::artifact::{
    read_artifact, read_artifact_versions, read_artifact_version, validate_envelope,
    validate_envelope_versions, validate_envelope_version, write_json_atomic, ArtifactReadError,
    ArtifactWriteError,
};
use crate::contracts::{
    ArgumentsData, DagRoutesData, Envelope, Producer, RouteVerdict, RoutesData, SchemaIrData,
    SeedPlansData, StageId,
};
use crate::graph::build_type_graph;
use crate::route::{expand_routes, RouteFacts, RoutePolicy};
use crate::seed::{plan_seed_routes, SeedPlanningError};

#[derive(Debug, Error)]
pub enum S4SeedPlanError {
    #[error(transparent)]
    Read(#[from] ArtifactReadError),
    #[error(transparent)]
    Planning(#[from] SeedPlanningError),
    #[error("cross-stage contract violation: {0}")]
    Contract(String),
    #[error(transparent)]
    Write(#[from] ArtifactWriteError),
}

pub fn plan_seed_artifact(
    schema: &Envelope<SchemaIrData>,
    arguments: &Envelope<ArgumentsData>,
    routes: &Envelope<RoutesData>,
) -> Result<Envelope<SeedPlansData>, S4SeedPlanError> {
    validate_inputs(schema, arguments, routes)?;
    let data = plan_seed_routes(&schema.data, &arguments.data, &routes.data)?;
    Ok(Envelope::complete(
        StageId::S4,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        data,
    ))
}

pub fn read_and_run_seed_plans(
    schema_path: &Path,
    arguments_path: &Path,
    routes_path: &Path,
) -> Result<Envelope<SeedPlansData>, S4SeedPlanError> {
    let schema = read_artifact(schema_path, StageId::S0)?;
    let arguments = read_artifact(arguments_path, StageId::S2)?;
    let routes = read_artifact_versions(routes_path, StageId::S3, &["2.1", "2.2"])?;
    plan_seed_artifact(&schema, &arguments, &routes)
}

pub fn write_seed_plans(
    artifact: &Envelope<SeedPlansData>,
    output: &Path,
) -> Result<(), S4SeedPlanError> {
    write_json_atomic(artifact, output)?;
    Ok(())
}

/// Adapter: plan seed artifact from a v3 DAG artifact.
///
/// The DAG is used only for its target list and policy fingerprint; routes are
/// expanded lazily from the automaton rebuilt from `schema`, `arguments`, and
/// `policy`.  This is the "adapter layer" approach—S4 planning logic is
/// unchanged; only the route source changes.
pub fn plan_seed_artifact_from_dag(
    schema: &Envelope<SchemaIrData>,
    arguments: &Envelope<ArgumentsData>,
    dag_routes: &Envelope<DagRoutesData>,
    policy: &RoutePolicy,
) -> Result<Envelope<SeedPlansData>, S4SeedPlanError> {
    validate_dag_inputs(schema, arguments, dag_routes)?;

    let graph = build_type_graph(&schema.data)
        .map_err(|e| S4SeedPlanError::Contract(e.to_string()))?;
    let facts = RouteFacts::build(&schema.data, &arguments.data, policy)
        .map_err(|e| S4SeedPlanError::Contract(e.to_string()))?;

    // Only expand open + unknown routes (actionable).
    let actionable_verdicts: std::collections::BTreeSet<RouteVerdict> =
        [RouteVerdict::Open, RouteVerdict::Unknown].into_iter().collect();

    let mut routes_data_targets = BTreeMap::new();
    for (target_id, target_dag) in &dag_routes.data.targets {
        let (target_routes, _status) = expand_routes(
            &graph,
            target_id,
            &facts,
            Some(&actionable_verdicts),
            None,
        )
        .map_err(|e| S4SeedPlanError::Contract(e.to_string()))?;
        let mut tr = target_routes;
        tr.sink_ref_ids = target_dag.sink_ref_ids.clone();
        routes_data_targets.insert(target_id.clone(), tr);
    }

    let routes_data = RoutesData {
        analysis_model: "route-family-dag-v1-expanded".to_string(),
        policy_fingerprint: dag_routes.data.policy_fingerprint.clone(),
        coverage: crate::contracts::RouteCoverage::CompleteFamilies,
        targets: routes_data_targets,
    };
    let routes_envelope = Envelope::complete_with_version(
        "2.2",
        StageId::S3,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        routes_data,
    );
    let data = plan_seed_routes(&schema.data, &arguments.data, &routes_envelope.data)?;
    Ok(Envelope::complete(
        StageId::S4,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        data,
    ))
}

/// Read schema, arguments, policy, and v3 DAG artifact; expand routes; plan seeds.
pub fn read_and_run_seed_plans_dag(
    schema_path: &Path,
    arguments_path: &Path,
    dag_routes_path: &Path,
    policy_path: &Path,
) -> Result<Envelope<SeedPlansData>, S4SeedPlanError> {
    use crate::route::read_route_policy;
    use crate::route::RoutePolicyError;

    let schema = read_artifact(schema_path, StageId::S0)?;
    let arguments = read_artifact(arguments_path, StageId::S2)?;
    let dag_routes = read_artifact_version(dag_routes_path, StageId::S3, "3.0")?;
    let loaded_policy = read_route_policy(policy_path)
        .map_err(|e: RoutePolicyError| S4SeedPlanError::Contract(e.to_string()))?;
    plan_seed_artifact_from_dag(&schema, &arguments, &dag_routes, &loaded_policy.policy)
}

fn validate_dag_inputs(
    schema: &Envelope<SchemaIrData>,
    arguments: &Envelope<ArgumentsData>,
    dag_routes: &Envelope<DagRoutesData>,
) -> Result<(), S4SeedPlanError> {
    validate_envelope(schema, StageId::S0)?;
    validate_envelope(arguments, StageId::S2)?;
    validate_envelope_version(dag_routes, StageId::S3, "3.0")?;
    if schema.schema_fingerprint != arguments.schema_fingerprint
        || schema.schema_fingerprint != dag_routes.schema_fingerprint
    {
        return Err(S4SeedPlanError::Contract(
            "S0, S2, and S3 schema fingerprints differ".to_string(),
        ));
    }
    Ok(())
}

fn validate_inputs(
    schema: &Envelope<SchemaIrData>,
    arguments: &Envelope<ArgumentsData>,
    routes: &Envelope<RoutesData>,
) -> Result<(), S4SeedPlanError> {
    validate_envelope(schema, StageId::S0)?;
    validate_envelope(arguments, StageId::S2)?;
    validate_envelope_versions(routes, StageId::S3, &["2.1", "2.2"])?;
    if schema.schema_fingerprint != arguments.schema_fingerprint
        || schema.schema_fingerprint != routes.schema_fingerprint
    {
        return Err(S4SeedPlanError::Contract(
            "S0, S2, and S3 schema fingerprints differ".to_string(),
        ));
    }

    let fields: BTreeMap<_, _> = schema
        .data
        .types
        .values()
        .flat_map(|definition| {
            definition
                .fields
                .values()
                .map(|field| (field.field_id.as_str(), field))
        })
        .collect();
    let mut route_ids = BTreeSet::new();
    for (target_key, target) in &routes.data.targets {
        if target_key != &target.target_type_id {
            return Err(S4SeedPlanError::Contract(format!(
                "S3 target map key differs from target ID: {target_key}"
            )));
        }
        if schema
            .data
            .types
            .values()
            .all(|definition| definition.type_id != target.target_type_id)
        {
            return Err(S4SeedPlanError::Contract(format!(
                "S3 target {} is missing from S0",
                target.target_type_id
            )));
        }
        for route in &target.routes {
            if !route_ids.insert(route.route_id.clone()) {
                return Err(S4SeedPlanError::Contract(format!(
                    "duplicate S3 route ID {}",
                    route.route_id
                )));
            }
            if route.target_type_id != target.target_type_id {
                return Err(S4SeedPlanError::Contract(format!(
                    "route {} target differs from its target group",
                    route.route_id
                )));
            }
            for edge in &route.witness.edges {
                if let Some(field_id) = &edge.field_id {
                    let field = fields.get(field_id.as_str()).ok_or_else(|| {
                        S4SeedPlanError::Contract(format!(
                            "route {} references missing S0 field {field_id}",
                            route.route_id
                        ))
                    })?;
                    if field.field_id != *field_id {
                        return Err(S4SeedPlanError::Contract(format!(
                            "route {} field identity mismatch for {field_id}",
                            route.route_id
                        )));
                    }
                }
            }
            if let Some(selector) = &route.selector {
                let classified = arguments
                    .data
                    .fields
                    .values()
                    .flat_map(|field| field.arguments.iter())
                    .any(|argument| {
                        argument.arg_ref == selector.arg_ref
                            && argument.root_arg_ref == selector.root_arg_ref
                            && argument.input_path == selector.input_path
                    });
                if !classified {
                    return Err(S4SeedPlanError::Contract(format!(
                        "route {} selector {} is missing from S2",
                        route.route_id, selector.arg_ref
                    )));
                }
            }
        }
    }
    Ok(())
}
