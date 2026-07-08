use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::contracts::{type_id, SchemaIrData, TypeKind};

use super::{GraphEdge, GraphEdgeKind, GraphNode, TypeGraph};

#[derive(Debug, Error)]
pub enum GraphBuildError {
    #[error("schema has no Query root")]
    MissingQueryRoot,
    #[error("duplicate graph edge ID: {0}")]
    DuplicateEdge(String),
    #[error("graph edge {edge_id} references missing composite type {type_id}")]
    MissingCompositeType { edge_id: String, type_id: String },
}

pub fn build_type_graph(schema: &SchemaIrData) -> Result<TypeGraph, GraphBuildError> {
    let query_name = schema
        .roots
        .query
        .as_deref()
        .ok_or(GraphBuildError::MissingQueryRoot)?;
    let query_root_id = type_id(query_name);

    let mut nodes = BTreeMap::new();
    for (name, definition) in &schema.types {
        if is_composite(definition.kind) {
            nodes.insert(
                definition.type_id.clone(),
                GraphNode {
                    type_id: definition.type_id.clone(),
                    type_name: name.clone(),
                    kind: definition.kind,
                    outgoing: Vec::new(),
                },
            );
        }
    }
    if !nodes.contains_key(&query_root_id) {
        return Err(GraphBuildError::MissingCompositeType {
            edge_id: "schema.root.query".to_string(),
            type_id: query_root_id,
        });
    }

    let mut pending = Vec::new();
    for definition in schema.types.values() {
        if matches!(definition.kind, TypeKind::Object | TypeKind::Interface) {
            for field in definition.fields.values() {
                if is_composite(field.return_type.named_kind) {
                    pending.push(GraphEdge {
                        edge_id: field.field_id.clone(),
                        kind: GraphEdgeKind::Field,
                        source_type_id: definition.type_id.clone(),
                        target_type_id: type_id(&field.return_type.named_type),
                        field: Some(field.clone()),
                    });
                }
            }
        }
        if matches!(definition.kind, TypeKind::Interface | TypeKind::Union) {
            for possible_type in &definition.possible_types {
                pending.push(GraphEdge {
                    edge_id: format!(
                        "type_condition:{}->{possible_type}",
                        definition
                            .type_id
                            .strip_prefix("type:")
                            .unwrap_or(&definition.type_id)
                    ),
                    kind: GraphEdgeKind::TypeCondition,
                    source_type_id: definition.type_id.clone(),
                    target_type_id: type_id(possible_type),
                    field: None,
                });
            }
        }
    }
    pending.sort_by(|left, right| left.edge_id.cmp(&right.edge_id));

    let mut seen = BTreeSet::new();
    let mut edges = Vec::with_capacity(pending.len());
    let mut incoming: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for edge in pending {
        if !seen.insert(edge.edge_id.clone()) {
            return Err(GraphBuildError::DuplicateEdge(edge.edge_id));
        }
        for type_id in [&edge.source_type_id, &edge.target_type_id] {
            if !nodes.contains_key(type_id) {
                return Err(GraphBuildError::MissingCompositeType {
                    edge_id: edge.edge_id.clone(),
                    type_id: type_id.clone(),
                });
            }
        }
        let index = edges.len();
        nodes
            .get_mut(&edge.source_type_id)
            .expect("source node checked above")
            .outgoing
            .push(index);
        incoming
            .entry(edge.target_type_id.clone())
            .or_default()
            .push(index);
        edges.push(edge);
    }

    for node in nodes.values_mut() {
        node.outgoing
            .sort_by(|left, right| edges[*left].edge_id.cmp(&edges[*right].edge_id));
    }
    for edge_indices in incoming.values_mut() {
        edge_indices.sort_by(|left, right| edges[*left].edge_id.cmp(&edges[*right].edge_id));
    }

    Ok(TypeGraph {
        query_root_id,
        nodes,
        edges,
        incoming,
    })
}

fn is_composite(kind: TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Object | TypeKind::Interface | TypeKind::Union
    )
}
