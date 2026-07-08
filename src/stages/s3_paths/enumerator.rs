use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::contracts::{
    CandidateAccessPath, CapOrigin, CycleTemplate, EnumerationStatus, PathEdge, PathEdgeKind,
    TargetPaths, TypeKind,
};
use crate::graph::{reverse_reachable, GraphEdgeKind, TypeGraph};

use super::formatter::display_projection;
use super::global_id::global_id_caps;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnumerationError {
    #[error("target type does not exist in graph: {0}")]
    MissingTarget(String),
    #[error("target type is not an OBJECT: {0}")]
    InvalidTarget(String),
}

#[derive(Debug)]
struct Frame {
    node_id: String,
    next_outgoing: usize,
    entered: bool,
    incoming_edge: Option<usize>,
}

pub fn enumerate_target(
    graph: &TypeGraph,
    target_type_id: &str,
) -> Result<TargetPaths, EnumerationError> {
    enumerate_target_internal(graph, target_type_id, true)
}

fn enumerate_target_internal(
    graph: &TypeGraph,
    target_type_id: &str,
    use_reachability: bool,
) -> Result<TargetPaths, EnumerationError> {
    let target = graph
        .node(target_type_id)
        .ok_or_else(|| EnumerationError::MissingTarget(target_type_id.to_string()))?;
    if target.kind != TypeKind::Object {
        return Err(EnumerationError::InvalidTarget(target_type_id.to_string()));
    }

    let reachable = reverse_reachable(graph, target_type_id);
    let mut caps: BTreeMap<Vec<String>, CandidateAccessPath> = BTreeMap::new();
    let mut path = Vec::new();
    let mut used_edges = BTreeSet::new();
    let mut stack = vec![Frame {
        node_id: graph.query_root_id.clone(),
        next_outgoing: 0,
        entered: false,
        incoming_edge: None,
    }];

    while let Some(frame) = stack.last_mut() {
        if !frame.entered {
            frame.entered = true;
            if frame.node_id == target_type_id && !path.is_empty() {
                let cap = cap_from_edge_indices(graph, target_type_id, &path, CapOrigin::Traversal);
                caps.entry(edge_ids(graph, &path)).or_insert(cap);
            }
        }

        let outgoing = graph.outgoing(&frame.node_id);
        if frame.next_outgoing >= outgoing.len() {
            let completed = stack.pop().expect("stack is not empty");
            if let Some(edge_index) = completed.incoming_edge {
                used_edges.remove(&graph.edge(edge_index).edge_id);
                path.pop();
            }
            continue;
        }

        let edge_index = outgoing[frame.next_outgoing];
        frame.next_outgoing += 1;
        let edge = graph.edge(edge_index);
        if use_reachability && !reachable.contains(&edge.target_type_id) {
            continue;
        }
        if used_edges.contains(&edge.edge_id) {
            if frame.node_id == target_type_id && !path.is_empty() {
                attach_cycle_template(graph, &path, edge_index, &mut caps);
            }
            continue;
        }

        used_edges.insert(edge.edge_id.clone());
        path.push(edge_index);
        stack.push(Frame {
            node_id: edge.target_type_id.clone(),
            next_outgoing: 0,
            entered: false,
            incoming_edge: Some(edge_index),
        });
    }

    for global_cap in global_id_caps(graph, target_type_id) {
        let key = global_cap
            .edges
            .iter()
            .map(|edge| edge.edge_id.clone())
            .collect();
        caps.insert(key, global_cap);
    }

    let mut caps: Vec<_> = caps.into_values().collect();
    caps.sort_by(|left, right| {
        left.origin
            .cmp(&right.origin)
            .then_with(|| cap_edge_ids(left).cmp(&cap_edge_ids(right)))
            .then_with(|| left.cap_id.cmp(&right.cap_id))
    });

    Ok(TargetPaths {
        target_type_id: target_type_id.to_string(),
        sink_ref_ids: Vec::new(),
        enumeration_status: EnumerationStatus::Complete,
        caps,
    })
}

pub(crate) fn cap_from_edge_indices(
    graph: &TypeGraph,
    target_type_id: &str,
    edge_indices: &[usize],
    origin: CapOrigin,
) -> CandidateAccessPath {
    let edges: Vec<_> = edge_indices
        .iter()
        .map(|index| path_edge(graph, *index))
        .collect();
    let edge_ids: Vec<_> = edges.iter().map(|edge| edge.edge_id.clone()).collect();
    let entry_field_id = edges
        .first()
        .and_then(|edge| edge.field_id.clone())
        .expect("all paths begin with a Query FIELD edge");
    CandidateAccessPath {
        cap_id: cap_id(target_type_id, &edge_ids),
        origin,
        entry_field_id,
        target_type_id: target_type_id.to_string(),
        display_projection: display_projection(&edges),
        edges,
        cycle_templates: Vec::new(),
    }
}

pub fn cap_id(target_type_id: &str, edge_ids: &[String]) -> String {
    let canonical =
        serde_json::to_vec(&(target_type_id, edge_ids)).expect("CAP identity tuple serializes");
    format!("cap:sha256:{:x}", Sha256::digest(canonical))
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

fn attach_cycle_template(
    graph: &TypeGraph,
    path: &[usize],
    repeated_edge_index: usize,
    caps: &mut BTreeMap<Vec<String>, CandidateAccessPath>,
) {
    let repeated_edge_id = &graph.edge(repeated_edge_index).edge_id;
    let Some(cycle_start_index) = path
        .iter()
        .position(|index| graph.edge(*index).edge_id == *repeated_edge_id)
    else {
        return;
    };
    let template = CycleTemplate {
        repeated_edge_id: repeated_edge_id.clone(),
        cycle_start_index,
        repeatable_edge_ids: path[cycle_start_index..]
            .iter()
            .map(|index| graph.edge(*index).edge_id.clone())
            .collect(),
    };
    if let Some(cap) = caps.get_mut(&edge_ids(graph, path)) {
        if !cap.cycle_templates.contains(&template) {
            cap.cycle_templates.push(template);
            cap.cycle_templates.sort();
        }
    }
}

fn edge_ids(graph: &TypeGraph, path: &[usize]) -> Vec<String> {
    path.iter()
        .map(|index| graph.edge(*index).edge_id.clone())
        .collect()
}

fn cap_edge_ids(cap: &CandidateAccessPath) -> Vec<&str> {
    cap.edges.iter().map(|edge| edge.edge_id.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use crate::graph::build_type_graph;
    use crate::schema::parse_sdl;

    use super::enumerate_target_internal;

    #[test]
    fn reverse_reachability_is_lossless() {
        let schema = parse_sdl(
            r#"
            type Query { useful: A dead: Dead }
            type A { target: Target branch: B }
            type B { target: Target }
            type Target { loop: Target }
            type Dead { next: Dead }
            "#,
        )
        .unwrap()
        .data;
        let graph = build_type_graph(&schema).unwrap();
        let pruned = enumerate_target_internal(&graph, "type:Target", true).unwrap();
        let unpruned = enumerate_target_internal(&graph, "type:Target", false).unwrap();
        assert_eq!(pruned, unpruned);
    }
}
