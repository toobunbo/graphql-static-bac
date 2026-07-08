use std::collections::BTreeSet;

use crate::contracts::{field_id, SchemaIrData, TypeKind};

#[derive(Debug, Clone, Default)]
pub struct PlumbingIndex {
    field_ids: BTreeSet<String>,
}

impl PlumbingIndex {
    pub fn build(schema: &SchemaIrData) -> Self {
        let mut field_ids = BTreeSet::new();
        let mut edge_types = BTreeSet::new();

        for (name, definition) in &schema.types {
            if definition.kind != TypeKind::Object {
                continue;
            }
            let has_page_info = definition.fields.contains_key("pageInfo");
            let has_nodes = definition.fields.contains_key("nodes");
            let has_edges = definition.fields.contains_key("edges");
            if has_page_info && (has_nodes || has_edges) {
                if has_nodes {
                    field_ids.insert(field_id(name, "nodes"));
                }
                if let Some(edges) = definition.fields.get("edges") {
                    field_ids.insert(edges.field_id.clone());
                    if edges.return_type.named_kind == TypeKind::Object {
                        edge_types.insert(edges.return_type.named_type.clone());
                    }
                }
            }
        }

        for edge_type in edge_types {
            let Some(definition) = schema.types.get(&edge_type) else {
                continue;
            };
            if definition.kind != TypeKind::Object {
                continue;
            }
            if let Some(node) = definition.fields.get("node") {
                if matches!(
                    node.return_type.named_kind,
                    TypeKind::Object | TypeKind::Interface | TypeKind::Union
                ) {
                    field_ids.insert(node.field_id.clone());
                }
            }
        }

        Self { field_ids }
    }

    pub fn is_plumbing(&self, field_id: &str) -> bool {
        self.field_ids.contains(field_id)
    }
}
