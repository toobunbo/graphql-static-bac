use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::contracts::{
    ArgumentClassification, BoundaryFamily, CycleTemplate, PathEdge, PathEdgeKind, Reachability,
    Route, RouteBoundary, RouteOrigin, RouteSelector, RouteSignature, RouteVerdict, RouteWitness,
    SelectorClass, SelectorContinuity, TargetRoutes,
};
use crate::graph::{GraphEdgeKind, TypeGraph};

use super::abstract_automaton::{
    AbstractAutomaton, AbstractState, CanonicalTrace, OriginMode, ProvenanceToken,
    RecordedTransition, TransitionEffect,
};
use super::signature::{path_family_id, route_id, witness_id};
use super::RouteFacts;

/// Expansion status returned alongside expanded routes.
#[derive(Debug, Clone)]
pub struct ExpansionStatus {
    /// True when every reachable path was emitted (budget not exhausted).
    pub complete: bool,
    /// Number of concrete routes emitted.
    pub routes_emitted: usize,
}

/// Lazily expand all concrete route families from the abstract automaton.
///
/// Unlike `analyze_target` (which emits one canonical witness per provenance
/// token), this function enumerates every distinct structural path from the
/// query root to the target type using an edge-simple DFS.  Selector branches
/// are forked at each `ActivateDefinite` transition.
///
/// `verdict_filter`: if `Some`, only routes whose verdict appears in the set
///   are collected.  Filtering happens before materialization so it is cheap.
/// `budget`: if `Some(n)`, stop after emitting `n` routes and set
///   `ExpansionStatus::complete = false`.
pub fn expand_routes(
    graph: &TypeGraph,
    target_type_id: &str,
    facts: &RouteFacts,
    verdict_filter: Option<&BTreeSet<RouteVerdict>>,
    budget: Option<usize>,
) -> Result<(TargetRoutes, ExpansionStatus), RouteAnalysisError> {
    let automaton = AbstractAutomaton::propagate(graph, target_type_id, facts)?;

    if !automaton.is_query_reachable() {
        return Ok((
            TargetRoutes {
                target_type_id: target_type_id.to_string(),
                sink_ref_ids: Vec::new(),
                reachability: Reachability::QueryUnreachable,
                best_verdict: None,
                routes: Vec::new(),
            },
            ExpansionStatus { complete: true, routes_emitted: 0 },
        ));
    }

    let root = automaton.root().expect("reachable automaton has root").clone();
    let mut routes_by_id: BTreeMap<String, Route> = BTreeMap::new();
    let mut budget_remaining = budget;
    let mut exhausted = false;

    // DFS stack entry: (state, transition_ids_in_path, used_transition_ids,
    //                   active_selector: Option<(activation_tid, selector_id)>)
    //
    // We push items in reverse order so that we process transitions in their
    // sorted order (the stack is LIFO).
    type ActiveSel = Option<(String, String)>;

    struct Frame {
        state: super::abstract_automaton::StateRef,
        path: Vec<String>,
        used: BTreeSet<String>,
        active: ActiveSel,
    }

    let mut stack: Vec<Frame> = vec![Frame {
        state: root,
        path: Vec::new(),
        used: BTreeSet::new(),
        active: None,
    }];

    while let Some(frame) = stack.pop() {
        if frame.state.type_id == target_type_id {
            // Terminal: materialize the route unless filtered.
            let v = verdict(&frame.state.state);
            if verdict_filter.is_some_and(|f| !f.contains(&v)) {
                continue;
            }
            if budget_remaining == Some(0) {
                exhausted = true;
                break;
            }

            let transitions: Vec<&RecordedTransition> = frame
                .path
                .iter()
                .map(|tid| {
                    automaton.transition(tid).ok_or_else(|| {
                        RouteAnalysisError::InvalidProvenance(format!(
                            "missing transition {tid}"
                        ))
                    })
                })
                .collect::<Result<_, _>>()?;

            if transitions.is_empty() {
                continue; // root IS the target — degenerate case
            }

            let selector_id = frame.active.as_ref().map(|(_, sid)| sid.clone());
            let selector = selector_id
                .as_deref()
                .map(|sid| materialize_selector(sid, facts))
                .transpose()?;

            let route = materialize_route(
                graph,
                target_type_id,
                &frame.state.state,
                &transitions,
                selector,
                facts,
            )?;

            routes_by_id
                .entry(route.route_id.clone())
                .or_insert(route);

            if let Some(b) = budget_remaining.as_mut() {
                *b = b.saturating_sub(1);
            }
            continue;
        }

        // Expand outgoing transitions.  Push in reverse sorted order so the
        // stack processes them front-to-back (deterministic DFS).
        let outgoing: Vec<String> =
            automaton.outgoing_transition_ids(&frame.state).to_vec();

        for tid in outgoing.iter().rev() {
            if frame.used.contains(tid) {
                continue; // edge-simple
            }
            let transition = automaton
                .transition(tid)
                .expect("transition ID in outgoing index");
            let mut new_used = frame.used.clone();
            new_used.insert(tid.clone());
            let mut new_path = frame.path.clone();
            new_path.push(tid.clone());
            let next = transition.target.clone();

            match transition.effect {
                TransitionEffect::ActivateDefinite => {
                    // One fork per selector generator (reversed for stack order).
                    for sel_id in transition.selector_generator_ids.iter().rev() {
                        stack.push(Frame {
                            state: next.clone(),
                            path: new_path.clone(),
                            used: new_used.clone(),
                            active: Some((tid.clone(), sel_id.clone())),
                        });
                    }
                }
                TransitionEffect::PassThrough | TransitionEffect::TypeCondition => {
                    stack.push(Frame {
                        state: next,
                        path: new_path,
                        used: new_used,
                        active: frame.active.clone(),
                    });
                }
            }
        }
    }

    let emitted = routes_by_id.len();
    let mut routes: Vec<Route> = routes_by_id.into_values().collect();
    routes.sort_by(compare_routes);
    let best_verdict = routes.first().map(|r| r.verdict);

    Ok((
        TargetRoutes {
            target_type_id: target_type_id.to_string(),
            sink_ref_ids: Vec::new(),
            reachability: Reachability::Reachable,
            best_verdict,
            routes,
        },
        ExpansionStatus {
            complete: !exhausted,
            routes_emitted: emitted,
        },
    ))
}

const SELF_SCOPE: u8 = 1;
const VISIBILITY: u8 = 2;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouteAnalysisError {
    #[error("target type does not exist in graph: {0}")]
    MissingTarget(String),
    #[error("target type is not a composite output type: {0}")]
    InvalidTarget(String),
    #[error("route state has selector occurrence {0} missing from semantic facts")]
    MissingSelector(String),
    #[error("reachable target route has no terminal semantic edge: {0}")]
    MissingTerminal(String),
    #[error("invalid abstract route provenance: {0}")]
    InvalidProvenance(String),
}

pub fn analyze_target(
    graph: &TypeGraph,
    target_type_id: &str,
    facts: &RouteFacts,
) -> Result<TargetRoutes, RouteAnalysisError> {
    let automaton = AbstractAutomaton::propagate(graph, target_type_id, facts)?;
    if !automaton.is_query_reachable() {
        return Ok(TargetRoutes {
            target_type_id: target_type_id.to_string(),
            sink_ref_ids: Vec::new(),
            reachability: Reachability::QueryUnreachable,
            best_verdict: None,
            routes: Vec::new(),
        });
    }

    let mut routes_by_id = BTreeMap::new();
    for trace in automaton.canonical_traces()? {
        let transitions: Vec<_> = trace
            .transition_ids
            .iter()
            .map(|transition_id| {
                automaton.transition(transition_id).ok_or_else(|| {
                    RouteAnalysisError::InvalidProvenance(format!(
                        "missing recorded transition {transition_id}"
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        validate_transition_chain(graph, target_type_id, &trace, &transitions)?;

        for selector_id in selector_ids_for_trace(&trace, &transitions)? {
            let selector = selector_id
                .as_deref()
                .map(|selector_id| materialize_selector(selector_id, facts))
                .transpose()?;
            let route = materialize_route(
                graph,
                target_type_id,
                &trace.terminal.state,
                &transitions,
                selector,
                facts,
            )?;
            match routes_by_id.entry(route.route_id.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(route);
                }
                std::collections::btree_map::Entry::Occupied(entry) => {
                    if entry.get() != &route {
                        return Err(RouteAnalysisError::InvalidProvenance(format!(
                            "route ID {} materialized with conflicting content",
                            route.route_id
                        )));
                    }
                }
            }
        }
    }

    let mut routes: Vec<_> = routes_by_id.into_values().collect();
    routes.sort_by(compare_routes);
    let best_verdict = routes.first().map(|route| route.verdict);
    Ok(TargetRoutes {
        target_type_id: target_type_id.to_string(),
        sink_ref_ids: Vec::new(),
        reachability: Reachability::Reachable,
        best_verdict,
        routes,
    })
}

fn selector_ids_for_trace(
    trace: &CanonicalTrace,
    transitions: &[&RecordedTransition],
) -> Result<Vec<Option<String>>, RouteAnalysisError> {
    let last_activation = transitions.iter().rev().find(|transition| {
        matches!(
            transition.effect,
            TransitionEffect::ActivateDefinite
        )
    });

    match &trace.token {
        ProvenanceToken::NoSelector => {
            if last_activation.is_some() {
                return Err(RouteAnalysisError::InvalidProvenance(
                    "no-selector terminal witness contains selector activation".to_string(),
                ));
            }
            Ok(vec![None])
        }
        ProvenanceToken::Selector {
            activation_transition_id: expected_id,
            selector_id,
        } => {
            let activation = last_activation.ok_or_else(|| {
                RouteAnalysisError::InvalidProvenance(format!(
                    "selector provenance {expected_id} is absent from its witness"
                ))
            })?;
            if activation.transition_id != *expected_id {
                return Err(RouteAnalysisError::InvalidProvenance(format!(
                    "selector provenance {expected_id} was overwritten by {}",
                    activation.transition_id
                )));
            }
            if activation.selector_generator_ids.is_empty() {
                return Err(RouteAnalysisError::InvalidProvenance(format!(
                    "selector activation {expected_id} has no generators"
                )));
            }
            if !activation.selector_generator_ids.contains(selector_id) {
                return Err(RouteAnalysisError::InvalidProvenance(format!(
                    "selector {selector_id} is not generated by activation {expected_id}"
                )));
            }
            Ok(vec![Some(selector_id.clone())])
        }
    }
}

fn validate_transition_chain(
    graph: &TypeGraph,
    target_type_id: &str,
    trace: &CanonicalTrace,
    transitions: &[&RecordedTransition],
) -> Result<(), RouteAnalysisError> {
    let Some(first) = transitions.first() else {
        return Err(RouteAnalysisError::MissingTerminal(
            target_type_id.to_string(),
        ));
    };
    if first.source.type_id != graph.query_root_id {
        return Err(RouteAnalysisError::InvalidProvenance(format!(
            "canonical witness starts at {}, expected {}",
            first.source.type_id, graph.query_root_id
        )));
    }

    for pair in transitions.windows(2) {
        if pair[0].target != pair[1].source {
            return Err(RouteAnalysisError::InvalidProvenance(format!(
                "disconnected transitions {} and {}",
                pair[0].transition_id, pair[1].transition_id
            )));
        }
    }

    let last = transitions.last().expect("non-empty checked above");
    if last.target != trace.terminal || last.target.type_id != target_type_id {
        return Err(RouteAnalysisError::InvalidProvenance(format!(
            "canonical witness ends at {}, expected {target_type_id}",
            last.target.type_id
        )));
    }
    if transitions
        .iter()
        .take(transitions.len().saturating_sub(1))
        .any(|transition| transition.target.type_id == target_type_id)
    {
        return Err(RouteAnalysisError::InvalidProvenance(
            "canonical witness expands after first target arrival".to_string(),
        ));
    }

    for transition in transitions {
        let edge = graph.edge(transition.edge_index);
        if edge.source_type_id != transition.source.type_id
            || edge.target_type_id != transition.target.type_id
        {
            return Err(RouteAnalysisError::InvalidProvenance(format!(
                "transition {} disagrees with graph edge {}",
                transition.transition_id, edge.edge_id
            )));
        }
    }
    Ok(())
}

fn materialize_route(
    graph: &TypeGraph,
    target_type_id: &str,
    state: &AbstractState,
    transitions: &[&RecordedTransition],
    selector: Option<RouteSelector>,
    facts: &RouteFacts,
) -> Result<Route, RouteAnalysisError> {
    let terminal_semantic_edge_id = state
        .terminal_semantic_edge_id
        .clone()
        .ok_or_else(|| RouteAnalysisError::MissingTerminal(target_type_id.to_string()))?;
    let edge_indices: Vec<_> = transitions
        .iter()
        .map(|transition| transition.edge_index)
        .collect();
    let edges: Vec<_> = edge_indices
        .iter()
        .map(|index| path_edge(graph, *index))
        .collect();
    let edge_ids: Vec<_> = edges.iter().map(|edge| edge.edge_id.clone()).collect();
    let cycle_templates = cycle_templates(&edge_ids);
    let actual_terminal = terminal_semantic_edge(graph, &edge_indices, facts);
    if actual_terminal.as_deref() != Some(&terminal_semantic_edge_id) {
        return Err(RouteAnalysisError::InvalidProvenance(format!(
            "terminal semantic edge mismatch: state={terminal_semantic_edge_id}, witness={actual_terminal:?}"
        )));
    }

    let actual_boundary_bits = witness_boundary_bits(graph, &edge_indices, facts);
    if actual_boundary_bits != state.boundary_bits {
        return Err(RouteAnalysisError::InvalidProvenance(format!(
            "boundary mismatch: state={}, witness={actual_boundary_bits}",
            state.boundary_bits
        )));
    }

    let origin = route_origin(graph, &edge_indices, facts);
    let state_origin = match state.origin_mode {
        OriginMode::GlobalIdPrefix => RouteOrigin::GlobalId,
        OriginMode::Traversal => RouteOrigin::Traversal,
    };
    if origin != state_origin {
        return Err(RouteAnalysisError::InvalidProvenance(format!(
            "origin mismatch: state={state_origin:?}, witness={origin:?}"
        )));
    }

    let family_id = path_family_id(target_type_id, &edge_ids, &cycle_templates);
    let verdict = verdict(state);
    let boundary_families = boundary_families(state.boundary_bits);
    let selector_id = selector
        .as_ref()
        .map(|selector| selector.selector_id.clone());
    let entry_field_id = edges
        .first()
        .and_then(|edge| edge.field_id.clone())
        .ok_or_else(|| {
            RouteAnalysisError::InvalidProvenance(
                "canonical Query route does not begin with a field".to_string(),
            )
        })?;
    let signature = RouteSignature {
        origin,
        selector_id: selector_id.clone(),
        path_family_id: family_id.clone(),
        terminal_semantic_edge_id: terminal_semantic_edge_id.clone(),
        boundary_families: boundary_families.clone(),
        selector_continuity: state.selector_continuity,
        verdict,
    };

    Ok(Route {
        route_id: route_id(
            target_type_id,
            origin,
            selector_id.as_deref(),
            &family_id,
            &terminal_semantic_edge_id,
            &boundary_families,
            state.selector_continuity,
            verdict,
        ),
        path_family_id: family_id,
        target_type_id: target_type_id.to_string(),
        origin,
        verdict,
        selector,
        selector_continuity: state.selector_continuity,
        terminal_semantic_edge_id,
        boundaries: materialize_boundaries(graph, &edge_indices, facts),
        signature,
        witness: RouteWitness {
            witness_id: witness_id(target_type_id, &edge_ids),
            entry_field_id,
            field_hop_count: field_hop_count_edges(&edges),
            display_projection: display_projection(&edges),
            edges,
            cycle_templates,
        },
    })
}

fn materialize_selector(
    selector_id: &str,
    facts: &RouteFacts,
) -> Result<RouteSelector, RouteAnalysisError> {
    let fact = facts
        .selectors_by_id
        .get(selector_id)
        .ok_or_else(|| RouteAnalysisError::MissingSelector(selector_id.to_string()))?;
    let classification = if fact
        .argument
        .classifications
        .contains(&ArgumentClassification::ObjectSelector)
    {
        ArgumentClassification::ObjectSelector
    } else {
        return Err(RouteAnalysisError::InvalidProvenance(format!(
            "selector {} is not backed by an object_selector argument",
            selector_id
        )));
    };
    Ok(RouteSelector {
        selector_id: fact.selector_id.clone(),
        arg_ref: fact.argument.arg_ref.clone(),
        root_arg_ref: fact.argument.root_arg_ref.clone(),
        arg_path: fact.argument.arg_path.clone(),
        input_path: fact.argument.input_path.clone(),
        type_ref: fact.argument.type_ref.clone(),
        classification,
        confidence: fact.argument.confidence,
        selected_type_id: fact.selected_type_id.clone(),
    })
}

fn materialize_boundaries(
    graph: &TypeGraph,
    edge_indices: &[usize],
    facts: &RouteFacts,
) -> Vec<RouteBoundary> {
    let mut boundaries: Vec<_> = edge_indices
        .iter()
        .filter_map(|index| graph.edge(*index).field.as_ref())
        .filter_map(|field| facts.boundaries_by_field.get(&field.field_id))
        .flatten()
        .cloned()
        .collect();
    boundaries.sort();
    boundaries.dedup();
    boundaries
}

fn witness_boundary_bits(graph: &TypeGraph, edge_indices: &[usize], facts: &RouteFacts) -> u8 {
    edge_indices
        .iter()
        .filter_map(|index| graph.edge(*index).field.as_ref())
        .fold(0, |bits, field| {
            bits | field_boundary_bits(facts, &field.field_id)
        })
}

fn field_boundary_bits(facts: &RouteFacts, field_id: &str) -> u8 {
    facts
        .boundaries_by_field
        .get(field_id)
        .into_iter()
        .flatten()
        .fold(0, |bits, boundary| {
            bits
                | match boundary.family {
                    BoundaryFamily::SelfScope => SELF_SCOPE,
                    BoundaryFamily::Visibility => VISIBILITY,
                }
        })
}

fn terminal_semantic_edge(
    graph: &TypeGraph,
    edge_indices: &[usize],
    facts: &RouteFacts,
) -> Option<String> {
    edge_indices.iter().rev().find_map(|index| {
        graph.edge(*index).field.as_ref().and_then(|field| {
            (!facts.plumbing.is_plumbing(&field.field_id)).then(|| field.field_id.clone())
        })
    })
}

fn route_origin(graph: &TypeGraph, edge_indices: &[usize], facts: &RouteFacts) -> RouteOrigin {
    let mut field_edges = edge_indices
        .iter()
        .filter(|index| graph.edge(**index).kind == GraphEdgeKind::Field);
    let first = field_edges.next();
    if first.is_some_and(|index| facts.global_id_fields.contains(&graph.edge(*index).edge_id))
        && field_edges.next().is_none()
    {
        RouteOrigin::GlobalId
    } else {
        RouteOrigin::Traversal
    }
}

fn verdict(state: &AbstractState) -> RouteVerdict {
    if state.boundary_bits != 0 {
        RouteVerdict::Guarded
    } else if state.selector_class == SelectorClass::Definite
        && state.selector_continuity == SelectorContinuity::Same
    {
        RouteVerdict::Open
    } else {
        RouteVerdict::Unknown
    }
}

fn boundary_families(bits: u8) -> Vec<BoundaryFamily> {
    let mut families = Vec::new();
    if bits & SELF_SCOPE != 0 {
        families.push(BoundaryFamily::SelfScope);
    }
    if bits & VISIBILITY != 0 {
        families.push(BoundaryFamily::Visibility);
    }
    families
}

fn path_edge(graph: &TypeGraph, edge_index: usize) -> PathEdge {
    let edge = graph.edge(edge_index);
    PathEdge {
        edge_id: edge.edge_id.clone(),
        kind: match edge.kind {
            GraphEdgeKind::Field => PathEdgeKind::Field,
            GraphEdgeKind::TypeCondition => PathEdgeKind::TypeCondition,
        },
        source_type_id: edge.source_type_id.clone(),
        field_id: edge.field.as_ref().map(|field| field.field_id.clone()),
        target_type_id: edge.target_type_id.clone(),
    }
}

fn cycle_templates(edge_ids: &[String]) -> Vec<CycleTemplate> {
    let mut first_seen = BTreeMap::new();
    let mut templates = BTreeSet::new();
    for (index, edge_id) in edge_ids.iter().enumerate() {
        if let Some(cycle_start_index) = first_seen.get(edge_id).copied() {
            templates.insert(CycleTemplate {
                repeated_edge_id: edge_id.clone(),
                cycle_start_index,
                repeatable_edge_ids: edge_ids[cycle_start_index..index].to_vec(),
            });
        } else {
            first_seen.insert(edge_id.clone(), index);
        }
    }
    templates.into_iter().collect()
}

fn field_hop_count_edges(edges: &[PathEdge]) -> usize {
    edges
        .iter()
        .filter(|edge| edge.kind == PathEdgeKind::Field)
        .count()
}

fn display_projection(edges: &[PathEdge]) -> String {
    let mut parts = Vec::with_capacity(edges.len() + 1);
    for edge in edges {
        match edge.kind {
            PathEdgeKind::Field => {
                let source = type_name(&edge.source_type_id);
                let field = edge
                    .field_id
                    .as_deref()
                    .and_then(|value| value.rsplit_once('.'))
                    .map_or("?", |(_, name)| name);
                parts.push(format!("{source}.{field}"));
            }
            PathEdgeKind::TypeCondition => {
                parts.push(format!("... on {}", type_name(&edge.target_type_id)));
            }
        }
    }
    if edges
        .last()
        .is_some_and(|edge| edge.kind == PathEdgeKind::Field)
    {
        parts.push(type_name(&edges.last().expect("checked above").target_type_id).to_string());
    }
    parts.join(" -> ")
}

fn type_name(type_id: &str) -> &str {
    type_id.strip_prefix("type:").unwrap_or(type_id)
}

fn compare_routes(left: &Route, right: &Route) -> Ordering {
    left.verdict
        .cmp(&right.verdict)
        .then_with(|| left.origin.cmp(&right.origin))
        .then_with(|| {
            left.terminal_semantic_edge_id
                .cmp(&right.terminal_semantic_edge_id)
        })
        .then_with(|| match (&left.selector, &right.selector) {
            (Some(left), Some(right)) => left.selector_id.cmp(&right.selector_id),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        })
        .then_with(|| left.path_family_id.cmp(&right.path_family_id))
        .then_with(|| left.route_id.cmp(&right.route_id))
}
