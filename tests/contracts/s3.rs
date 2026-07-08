use graphql_static_bac::contracts::{
    CandidateAccessPath, CapOrigin, CycleTemplate, PathEdge, PathEdgeKind,
};

#[test]
fn s3_cap_serialization_contains_only_structural_fields() {
    let cap = CandidateAccessPath {
        cap_id: "cap:sha256:test".to_string(),
        origin: CapOrigin::Traversal,
        entry_field_id: "field:Query.user".to_string(),
        target_type_id: "type:User".to_string(),
        edges: vec![PathEdge {
            edge_id: "field:Query.user".to_string(),
            kind: PathEdgeKind::Field,
            source_type_id: "type:Query".to_string(),
            field_id: Some("field:Query.user".to_string()),
            target_type_id: "type:User".to_string(),
        }],
        cycle_templates: vec![CycleTemplate {
            repeated_edge_id: "field:User.manager".to_string(),
            cycle_start_index: 1,
            repeatable_edge_ids: vec!["field:User.manager".to_string()],
        }],
        display_projection: "Query.user -> User".to_string(),
    };

    let value = serde_json::to_value(cap).unwrap();
    for forbidden in [
        "selectors",
        "authz_modifiers",
        "sanitizer_boundaries",
        "flow",
        "ownership_continuity",
        "ranking_bucket",
        "score",
        "score_breakdown",
        "confidence",
    ] {
        assert!(value.get(forbidden).is_none(), "unexpected key {forbidden}");
    }
}
