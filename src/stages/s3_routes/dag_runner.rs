use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use thiserror::Error;

use crate::artifact::{
    read_artifact, validate_envelope, write_json_atomic, ArtifactReadError, ArtifactWriteError,
};
use crate::contracts::{
    ArgumentClassification, ArgumentsData, CardinalityStatus, DagCanonicalWitness, DagCoverage,
    DagEdge, DagOriginMode, DagRoutesData, DagSelectorFact, DagState, DagTerminal,
    DagTransition, DagTransitionEffect, Envelope, FamilyCardinality, Producer, Reachability,
    RouteOrigin, SchemaIrData, SinksData, StageId, TargetDag, TypeKind,
    BoundaryFamily,
    DagComponent as ContractDagComponent,
    DagComponentEdge as ContractDagComponentEdge,
};
use crate::graph::{build_type_graph, GraphBuildError, GraphEdgeKind};
use crate::route::{
    analyze_target, read_route_policy, LoadedRoutePolicy, RouteAnalysisError, RouteFacts,
    RouteFactsError, RoutePolicyError, AbstractAutomaton, OriginMode, TransitionEffect,
};
use crate::route::dag::{state_id, selector_set_id, RouteFamilyDag};


const SELF_SCOPE: u8 = 1;
const VISIBILITY: u8 = 2;

#[derive(Debug, Error)]
pub enum S3DagError {
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

/// Build a v3 DAG artifact for a single named target type.
pub fn analyze_target_dag(
    schema: &Envelope<SchemaIrData>,
    arguments: &Envelope<ArgumentsData>,
    policy: &LoadedRoutePolicy,
    target: &str,
) -> Result<Envelope<DagRoutesData>, S3DagError> {
    validate_pair_dag(schema, arguments)?;
    let graph = build_type_graph(&schema.data)?;
    let facts = RouteFacts::build(&schema.data, &arguments.data, &policy.policy)?;
    let target_type_id = normalize_type_id(target);

    let automaton =
        AbstractAutomaton::propagate(&graph, &target_type_id, &facts)?;
    let dag = RouteFamilyDag::build(&automaton, &target_type_id);

    let canonical_routes = analyze_target(&graph, &target_type_id, &facts)?;
    let target_dag = build_target_dag(
        &dag,
        &automaton,
        &graph,
        &facts,
        &target_type_id,
        &canonical_routes.routes,
        Vec::new(),
    );

    let mut targets = BTreeMap::new();
    targets.insert(target_type_id, target_dag);
    Ok(dag_envelope(schema, policy, targets))
}

/// Build a v3 DAG artifact for all selected targets from the S1 sinks list.
pub fn analyze_selected_targets_dag(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
    arguments: &Envelope<ArgumentsData>,
    policy: &LoadedRoutePolicy,
) -> Result<Envelope<DagRoutesData>, S3DagError> {
    validate_inputs_dag(schema, sinks, arguments)?;
    let graph = build_type_graph(&schema.data)?;
    let facts = RouteFacts::build(&schema.data, &arguments.data, &policy.policy)?;
    let mut targets = BTreeMap::new();
    let mut seen_targets = BTreeSet::new();

    for selected in &sinks.data.selected_types {
        validate_selected_target_dag(schema, sinks, selected, &mut seen_targets)?;
        let automaton =
            AbstractAutomaton::propagate(&graph, &selected.type_id, &facts)?;

        if automaton.terminal_states().is_empty() {
            return Err(S3DagError::Contract(format!(
                "S1-selected target {} is query-unreachable",
                selected.type_id
            )));
        }

        let dag = RouteFamilyDag::build(&automaton, &selected.type_id);
        let canonical_routes = analyze_target(&graph, &selected.type_id, &facts)?;

        let mut sink_ref_ids = selected.sink_ref_ids.clone();
        sink_ref_ids.sort();
        sink_ref_ids.dedup();

        let target_dag = build_target_dag(
            &dag,
            &automaton,
            &graph,
            &facts,
            &selected.type_id,
            &canonical_routes.routes,
            sink_ref_ids,
        );
        targets.insert(selected.type_id.clone(), target_dag);
    }
    Ok(dag_envelope(schema, policy, targets))
}

pub fn read_and_run_dag(
    schema_path: &Path,
    sinks_path: &Path,
    arguments_path: &Path,
    policy_path: &Path,
) -> Result<Envelope<DagRoutesData>, S3DagError> {
    let schema = read_artifact(schema_path, StageId::S0)?;
    let sinks = read_artifact(sinks_path, StageId::S1)?;
    let arguments = read_artifact(arguments_path, StageId::S2)?;
    let policy = read_route_policy(policy_path)?;
    analyze_selected_targets_dag(&schema, &sinks, &arguments, &policy)
}

pub fn write_dag_routes(
    artifact: &Envelope<DagRoutesData>,
    output: &Path,
) -> Result<(), S3DagError> {
    write_json_atomic(artifact, output)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal: build TargetDag from RouteFamilyDag + canonical routes
// ---------------------------------------------------------------------------

fn build_target_dag(
    dag: &RouteFamilyDag,
    _automaton: &AbstractAutomaton,
    graph: &crate::graph::TypeGraph,
    facts: &RouteFacts,
    target_type_id: &str,
    canonical_routes: &[crate::contracts::Route],
    sink_ref_ids: Vec<String>,
) -> TargetDag {
    if !dag.is_reachable() {
        return TargetDag {
            target_type_id: target_type_id.to_string(),
            dag_id: dag.dag_id.clone(),
            entry_component_id: String::new(),
            reachability: Reachability::QueryUnreachable,
            sink_ref_ids,
            states: BTreeMap::new(),
            edge_table: Vec::new(),
            selector_sets: dag.selector_sets.clone(),
            selector_facts: BTreeMap::new(),
            transitions: BTreeMap::new(),
            components: BTreeMap::new(),
            component_edges: Vec::new(),
            terminals: Vec::new(),
            family_cardinality: FamilyCardinality {
                status: CardinalityStatus::NotMaterialized,
                lower_bound: 0,
            },
        };
    }

    // Build edge table (schema edges referenced by any transition)
    let mut edge_table_map: BTreeMap<String, usize> = BTreeMap::new(); // edge_id → index
    let mut edge_table: Vec<DagEdge> = Vec::new();
    for t in dag.transitions.values() {
        let edge = graph.edge(t.edge_index);
        if !edge_table_map.contains_key(&edge.edge_id) {
            let idx = edge_table.len();
            edge_table_map.insert(edge.edge_id.clone(), idx);
            edge_table.push(DagEdge {
                edge_id: edge.edge_id.clone(),
                kind: match edge.kind {
                    GraphEdgeKind::Field => crate::contracts::PathEdgeKind::Field,
                    GraphEdgeKind::TypeCondition => crate::contracts::PathEdgeKind::TypeCondition,
                },
                source_type_id: edge.source_type_id.clone(),
                field_id: edge.field.as_ref().map(|f| f.field_id.clone()),
                target_type_id: edge.target_type_id.clone(),
            });
        }
    }

    // Build serializable states
    let states: BTreeMap<String, DagState> = dag
        .states
        .iter()
        .map(|(sid, sr)| {
            let ds = DagState {
                state_id: sid.clone(),
                type_id: sr.type_id.clone(),
                origin_mode: match sr.state.origin_mode {
                    OriginMode::Traversal => DagOriginMode::Traversal,
                    OriginMode::GlobalIdPrefix => DagOriginMode::GlobalIdPrefix,
                },
                selector_class: sr.state.selector_class,
                selector_continuity: sr.state.selector_continuity,
                boundary_bits: sr.state.boundary_bits,
                terminal_semantic_edge_id: sr.state.terminal_semantic_edge_id.clone(),
            };
            (sid.clone(), ds)
        })
        .collect();

    // Build serializable transitions
    let transitions: BTreeMap<String, DagTransition> = dag
        .transitions
        .iter()
        .map(|(tid, t)| {
            let edge = graph.edge(t.edge_index);
            let edge_idx = edge_table_map[&edge.edge_id];
            let sel_set_id = if t.selector_generator_ids.is_empty() {
                None
            } else {
                let mut sorted = t.selector_generator_ids.clone();
                sorted.sort();
                Some(selector_set_id(&sorted))
            };
            let dt = DagTransition {
                transition_id: tid.clone(),
                source_state_id: state_id(&t.source),
                target_state_id: state_id(&t.target),
                edge_index: edge_idx,
                effect: match t.effect {
                    TransitionEffect::PassThrough => DagTransitionEffect::PassThrough,
                    TransitionEffect::ActivateDefinite => DagTransitionEffect::ActivateDefinite,
                    TransitionEffect::TypeCondition => DagTransitionEffect::TypeCondition,
                },
                selector_set_id: sel_set_id,
            };
            (tid.clone(), dt)
        })
        .collect();

    // Build serializable components
    let components: BTreeMap<String, ContractDagComponent> = dag
        .components
        .iter()
        .map(|(cid, c)| {
            (
                cid.clone(),
                ContractDagComponent {
                    component_id: cid.clone(),
                    member_state_ids: c.member_state_ids.clone(),
                    internal_transition_ids: c.internal_transition_ids.clone(),
                    is_cycle_capable: c.is_cycle_capable,
                    is_terminal: c.is_terminal,
                },
            )
        })
        .collect();

    // Build serializable component edges
    let component_edges: Vec<ContractDagComponentEdge> = dag
        .component_edges
        .iter()
        .map(|e| ContractDagComponentEdge {
            source_component_id: e.source_component_id.clone(),
            target_component_id: e.target_component_id.clone(),
            transition_id: e.transition_id.clone(),
        })
        .collect();

    // Build terminals from canonical routes (one per provenance token/terminal)
    let mut terminals: Vec<DagTerminal> = Vec::new();
    let mut seen_terminal_ids: BTreeSet<String> = BTreeSet::new();

    for route in canonical_routes {
        let terminal_state_id = dag
            .states
            .iter()
            .find(|(_, sr)| {
                sr.type_id == target_type_id
                    && sr.state.origin_mode
                        == match route.origin {
                            RouteOrigin::GlobalId => OriginMode::GlobalIdPrefix,
                            RouteOrigin::Traversal => OriginMode::Traversal,
                        }
                    && sr.state.selector_continuity == route.selector_continuity
                    && sr.state.boundary_bits == boundary_bits_for_families(&route.signature.boundary_families)
                    && sr.state.terminal_semantic_edge_id.as_deref()
                        == Some(&route.terminal_semantic_edge_id)
            })
            .map(|(sid, _)| sid.clone());

        let Some(tsid) = terminal_state_id else {
            continue;
        };
        let Some(component_id) = dag.state_to_component.get(&tsid).cloned() else {
            continue;
        };

        let selector_id = route.selector.as_ref().map(|s| s.selector_id.clone());
        let terminal_id = {
            let canonical = serde_json::to_vec(&(target_type_id, &tsid, &selector_id))
                .expect("terminal id serializes");
            use sha2::{Digest, Sha256};
            format!("term:sha256:{:x}", Sha256::digest(canonical))
        };

        if !seen_terminal_ids.insert(terminal_id.clone()) {
            continue; // deduplicate
        }

        let witness_transition_ids: Vec<String> = route
            .witness
            .edges
            .iter()
            .filter_map(|edge| {
                // Find transition matching this edge in the canonical path
                dag.transitions
                    .values()
                    .find(|t| {
                        let ge = graph.edge(t.edge_index);
                        ge.edge_id == edge.edge_id
                    })
                    .map(|t| t.transition_id.clone())
            })
            .collect();

        let witness_id = {
            let canonical = serde_json::to_vec(&(target_type_id, &witness_transition_ids))
                .expect("witness id serializes");
            use sha2::{Digest, Sha256};
            format!("wit:sha256:{:x}", Sha256::digest(canonical))
        };

        terminals.push(DagTerminal {
            terminal_id,
            state_id: tsid,
            component_id,
            verdict: route.verdict,
            origin: route.origin,
            boundary_families: route.signature.boundary_families.clone(),
            selector_continuity: route.selector_continuity,
            terminal_semantic_edge_id: route.terminal_semantic_edge_id.clone(),
            canonical_witness: DagCanonicalWitness {
                witness_id,
                transition_ids: witness_transition_ids,
                selector_id: selector_id.clone(),
            },
        });
    }
    terminals.sort_by(|a, b| a.terminal_id.cmp(&b.terminal_id));

    // Collect selector facts referenced anywhere in this target's transitions
    let mut selector_facts: BTreeMap<String, DagSelectorFact> = BTreeMap::new();
    for t in dag.transitions.values() {
        for sel_id in &t.selector_generator_ids {
            if selector_facts.contains_key(sel_id) {
                continue;
            }
            if let Some(sf) = facts.selectors_by_id.get(sel_id) {
                let classification = if sf
                    .argument
                    .classifications
                    .contains(&ArgumentClassification::ObjectSelector)
                {
                    ArgumentClassification::ObjectSelector
                } else {
                    continue;
                };
                selector_facts.insert(
                    sel_id.clone(),
                    DagSelectorFact {
                        selector_id: sel_id.clone(),
                        arg_ref: sf.argument.arg_ref.clone(),
                        root_arg_ref: sf.argument.root_arg_ref.clone(),
                        arg_path: sf.argument.arg_path.clone(),
                        input_path: sf.argument.input_path.clone(),
                        type_ref: sf.argument.type_ref.clone(),
                        classification,
                        confidence: sf.argument.confidence,
                        selected_type_id: sf.selected_type_id.clone(),
                    },
                );
            }
        }
    }

    TargetDag {
        target_type_id: target_type_id.to_string(),
        dag_id: dag.dag_id.clone(),
        entry_component_id: dag.entry_component_id.clone(),
        reachability: Reachability::Reachable,
        sink_ref_ids,
        states,
        edge_table,
        selector_sets: dag.selector_sets.clone(),
        selector_facts,
        transitions,
        components,
        component_edges,
        terminals,
        family_cardinality: FamilyCardinality {
            status: CardinalityStatus::NotMaterialized,
            lower_bound: canonical_routes.len(),
        },
    }
}

fn boundary_bits_for_families(families: &[BoundaryFamily]) -> u8 {
    let mut bits = 0u8;
    for f in families {
        bits |= match f {
            BoundaryFamily::SelfScope => SELF_SCOPE,
            BoundaryFamily::Visibility => VISIBILITY,
        };
    }
    bits
}

// ---------------------------------------------------------------------------
// Envelope builder
// ---------------------------------------------------------------------------

fn dag_envelope(
    schema: &Envelope<SchemaIrData>,
    policy: &LoadedRoutePolicy,
    targets: BTreeMap<String, TargetDag>,
) -> Envelope<DagRoutesData> {
    Envelope::complete_with_version(
        "3.0",
        StageId::S3,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        DagRoutesData {
            analysis_model: "route-family-dag-v1".to_string(),
            selector_mode: "definite_only".to_string(),
            coverage: DagCoverage::CompleteGraph,
            policy_fingerprint: policy.fingerprint.clone(),
            targets,
        },
    )
}

// ---------------------------------------------------------------------------
// Validation helpers (mirror of runner.rs, scoped to S3DagError)
// ---------------------------------------------------------------------------

fn validate_pair_dag(
    schema: &Envelope<SchemaIrData>,
    arguments: &Envelope<ArgumentsData>,
) -> Result<(), S3DagError> {
    validate_envelope(schema, StageId::S0)?;
    validate_envelope(arguments, StageId::S2)?;
    if schema.schema_fingerprint != arguments.schema_fingerprint {
        return Err(S3DagError::Contract(
            "S0 and S2 schema fingerprints differ".to_string(),
        ));
    }
    Ok(())
}

fn validate_inputs_dag(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
    arguments: &Envelope<ArgumentsData>,
) -> Result<(), S3DagError> {
    validate_pair_dag(schema, arguments)?;
    validate_envelope(sinks, StageId::S1)?;
    if schema.schema_fingerprint != sinks.schema_fingerprint {
        return Err(S3DagError::Contract(
            "S0 and S1 schema fingerprints differ".to_string(),
        ));
    }
    for (key, sink_ref) in &sinks.data.sink_refs {
        if key != &sink_ref.sink_ref_id {
            return Err(S3DagError::Contract(format!(
                "sink ref map key differs from ID: {key}"
            )));
        }
    }
    Ok(())
}

fn validate_selected_target_dag(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
    selected: &crate::contracts::SelectedType,
    seen_targets: &mut BTreeSet<String>,
) -> Result<(), S3DagError> {
    if !seen_targets.insert(selected.type_id.clone()) {
        return Err(S3DagError::Contract(format!(
            "duplicate selected target {}",
            selected.type_id
        )));
    }
    let type_name = selected
        .type_id
        .strip_prefix("type:")
        .unwrap_or(&selected.type_id);
    let definition = schema.data.types.get(type_name).ok_or_else(|| {
        S3DagError::Contract(format!(
            "selected target {} is missing from S0",
            selected.type_id
        ))
    })?;
    if !matches!(
        definition.kind,
        TypeKind::Object | TypeKind::Interface | TypeKind::Union
    ) {
        return Err(S3DagError::Contract(format!(
            "selected target {} is not a composite output type",
            selected.type_id
        )));
    }
    if definition.type_id != selected.type_id || selected.type_name != type_name {
        return Err(S3DagError::Contract(format!(
            "selected target identity mismatch for {}",
            selected.type_id
        )));
    }
    let mut sink_ref_ids = selected.sink_ref_ids.clone();
    sink_ref_ids.sort();
    sink_ref_ids.dedup();
    if sink_ref_ids.len() != selected.sink_ref_ids.len() {
        return Err(S3DagError::Contract(format!(
            "duplicate sink ref for {}",
            selected.type_id
        )));
    }
    for sink_ref_id in sink_ref_ids {
        let sink_ref = sinks.data.sink_refs.get(&sink_ref_id).ok_or_else(|| {
            S3DagError::Contract(format!(
                "selected target {} references missing sink {}",
                selected.type_id, sink_ref_id
            ))
        })?;
        if sink_ref.sink_ref_id != sink_ref_id || sink_ref.type_id != selected.type_id {
            return Err(S3DagError::Contract(format!(
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
