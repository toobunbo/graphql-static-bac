use std::collections::BTreeMap;

use crate::contracts::{FieldDefinition, TypeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphEdgeKind {
    Field,
    TypeCondition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    pub edge_id: String,
    pub kind: GraphEdgeKind,
    pub source_type_id: String,
    pub target_type_id: String,
    pub field: Option<FieldDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    pub type_id: String,
    pub type_name: String,
    pub kind: TypeKind,
    pub outgoing: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeGraph {
    pub query_root_id: String,
    pub nodes: BTreeMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub incoming: BTreeMap<String, Vec<usize>>,
}

impl TypeGraph {
    pub fn node(&self, type_id: &str) -> Option<&GraphNode> {
        self.nodes.get(type_id)
    }

    pub fn edge(&self, index: usize) -> &GraphEdge {
        &self.edges[index]
    }

    pub fn outgoing(&self, type_id: &str) -> &[usize] {
        self.nodes
            .get(type_id)
            .map_or(&[], |node| node.outgoing.as_slice())
    }

    pub fn incoming(&self, type_id: &str) -> &[usize] {
        self.incoming.get(type_id).map_or(&[], Vec::as_slice)
    }
}
