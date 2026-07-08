use crate::contracts::{CandidateAccessPath, CapOrigin, TypeKind, TypeWrapper};
use crate::graph::{GraphEdgeKind, TypeGraph};

use super::enumerator::cap_from_edge_indices;

pub fn global_id_caps(graph: &TypeGraph, target_type_id: &str) -> Vec<CandidateAccessPath> {
    let Some(target) = graph.node(target_type_id) else {
        return Vec::new();
    };
    if target.kind != TypeKind::Object {
        return Vec::new();
    }

    let Some((node_id, condition_index)) = find_node_condition(graph, target_type_id) else {
        return Vec::new();
    };
    let Some(node) = graph.node(&node_id) else {
        return Vec::new();
    };
    if !matches!(node.kind, TypeKind::Interface | TypeKind::Union) {
        return Vec::new();
    }

    let mut caps = Vec::new();
    for edge_index in graph.outgoing(&graph.query_root_id) {
        let edge = graph.edge(*edge_index);
        let Some(field) = edge.field.as_ref() else {
            continue;
        };
        if edge.kind != GraphEdgeKind::Field
            || edge.target_type_id != node_id
            || field.return_type.named_type != "Node"
        {
            continue;
        }
        let valid = match field.name.as_str() {
            "node" => {
                list_count(&field.return_type.wrappers) == 0 && has_id_argument(field, "id", 0)
            }
            "nodes" => {
                list_count(&field.return_type.wrappers) == 1 && has_id_argument(field, "ids", 1)
            }
            _ => false,
        };
        if valid {
            caps.push(cap_from_edge_indices(
                graph,
                target_type_id,
                &[*edge_index, condition_index],
                CapOrigin::GlobalId,
            ));
        }
    }
    caps
}

fn find_node_condition(graph: &TypeGraph, target_type_id: &str) -> Option<(String, usize)> {
    for node in graph.nodes.values() {
        if node.type_name != "Node" {
            continue;
        }
        for edge_index in &node.outgoing {
            let edge = graph.edge(*edge_index);
            if edge.kind == GraphEdgeKind::TypeCondition && edge.target_type_id == target_type_id {
                return Some((node.type_id.clone(), *edge_index));
            }
        }
    }
    None
}

fn has_id_argument(
    field: &crate::contracts::FieldDefinition,
    argument_name: &str,
    expected_lists: usize,
) -> bool {
    field.arguments.iter().any(|argument| {
        argument.name == argument_name
            && argument.type_ref.named_type == "ID"
            && argument.type_ref.named_kind == TypeKind::Scalar
            && list_count(&argument.type_ref.wrappers) == expected_lists
    })
}

fn list_count(wrappers: &[TypeWrapper]) -> usize {
    wrappers
        .iter()
        .filter(|wrapper| **wrapper == TypeWrapper::List)
        .count()
}
