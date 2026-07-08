use std::collections::{BTreeMap, BTreeSet};

use crate::contracts::{CycleTemplate, TypeKind};
use crate::graph::{reverse_reachable, TypeGraph};

use super::RouteAnalysisError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StructuralFamily {
    pub edge_indices: Vec<usize>,
    pub cycle_templates: Vec<CycleTemplate>,
}

#[derive(Debug)]
struct Frame {
    node_id: String,
    next_outgoing: usize,
    entered: bool,
    incoming_edge: Option<usize>,
}

pub(crate) fn enumerate_structural_families(
    graph: &TypeGraph,
    target_type_id: &str,
) -> Result<Vec<StructuralFamily>, RouteAnalysisError> {
    let target = graph
        .node(target_type_id)
        .ok_or_else(|| RouteAnalysisError::MissingTarget(target_type_id.to_string()))?;
    if !matches!(
        target.kind,
        TypeKind::Object | TypeKind::Interface | TypeKind::Union
    ) {
        return Err(RouteAnalysisError::InvalidTarget(
            target_type_id.to_string(),
        ));
    }

    let reachable = reverse_reachable(graph, target_type_id);
    if !reachable.contains(&graph.query_root_id) {
        return Ok(Vec::new());
    }

    let mut families: BTreeMap<Vec<String>, StructuralFamily> = BTreeMap::new();
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
                families
                    .entry(edge_ids(graph, &path))
                    .or_insert_with(|| StructuralFamily {
                        edge_indices: path.clone(),
                        cycle_templates: Vec::new(),
                    });
            }
        }

        let outgoing = graph.outgoing(&frame.node_id);
        if frame.next_outgoing >= outgoing.len() {
            let completed = stack.pop().expect("family traversal stack is non-empty");
            if let Some(edge_index) = completed.incoming_edge {
                used_edges.remove(&graph.edge(edge_index).edge_id);
                path.pop();
            }
            continue;
        }

        let edge_index = outgoing[frame.next_outgoing];
        frame.next_outgoing += 1;
        let edge = graph.edge(edge_index);
        if !reachable.contains(&edge.target_type_id) {
            continue;
        }
        if used_edges.contains(&edge.edge_id) {
            if frame.node_id == target_type_id && !path.is_empty() {
                attach_cycle_template(graph, &path, edge_index, &mut families);
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

    Ok(families.into_values().collect())
}

fn attach_cycle_template(
    graph: &TypeGraph,
    path: &[usize],
    repeated_edge_index: usize,
    families: &mut BTreeMap<Vec<String>, StructuralFamily>,
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
    if let Some(family) = families.get_mut(&edge_ids(graph, path)) {
        if !family.cycle_templates.contains(&template) {
            family.cycle_templates.push(template);
            family.cycle_templates.sort();
        }
    }
}

fn edge_ids(graph: &TypeGraph, path: &[usize]) -> Vec<String> {
    path.iter()
        .map(|index| graph.edge(*index).edge_id.clone())
        .collect()
}
