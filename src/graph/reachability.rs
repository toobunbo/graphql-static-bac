use std::collections::BTreeSet;

use super::TypeGraph;

pub fn reverse_reachable(graph: &TypeGraph, target_type_id: &str) -> BTreeSet<String> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![target_type_id.to_string()];
    while let Some(type_id) = pending.pop() {
        if !reachable.insert(type_id.clone()) {
            continue;
        }
        for edge_index in graph.incoming(&type_id) {
            pending.push(graph.edge(*edge_index).source_type_id.clone());
        }
    }
    reachable
}
