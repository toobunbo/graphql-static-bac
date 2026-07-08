use crate::contracts::{PathEdge, PathEdgeKind};

pub fn display_projection(edges: &[PathEdge]) -> String {
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
