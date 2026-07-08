use sha2::{Digest, Sha256};

use crate::contracts::{BoundaryFamily, RouteOrigin, RouteVerdict, SelectorContinuity};

pub fn route_id(
    target_type_id: &str,
    origin: RouteOrigin,
    selector_id: Option<&str>,
    path_family_id: &str,
    terminal_semantic_edge_id: &str,
    boundary_families: &[BoundaryFamily],
    selector_continuity: SelectorContinuity,
    verdict: RouteVerdict,
) -> String {
    let canonical = serde_json::to_vec(&(
        target_type_id,
        origin,
        selector_id,
        path_family_id,
        terminal_semantic_edge_id,
        boundary_families,
        selector_continuity,
        verdict,
    ))
    .expect("route identity tuple serializes");
    format!("route:sha256:{:x}", Sha256::digest(canonical))
}

pub fn witness_id(target_type_id: &str, edge_ids: &[String]) -> String {
    let canonical =
        serde_json::to_vec(&(target_type_id, edge_ids)).expect("witness identity tuple serializes");
    format!("cap:sha256:{:x}", Sha256::digest(canonical))
}

pub fn path_family_id<T: serde::Serialize>(
    target_type_id: &str,
    edge_ids: &[String],
    cycle_templates: &[T],
) -> String {
    let canonical = serde_json::to_vec(&(target_type_id, edge_ids, cycle_templates))
        .expect("path family identity tuple serializes");
    format!("family:sha256:{:x}", Sha256::digest(canonical))
}
