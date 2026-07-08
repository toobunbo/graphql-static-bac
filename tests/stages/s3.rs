use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use graphql_static_bac::artifact::read_artifact;
use graphql_static_bac::contracts::{
    CapOrigin, Envelope, PathsData, SchemaIrData, SinksData, StageId,
};
use graphql_static_bac::graph::build_type_graph;
use graphql_static_bac::schema::parse_sdl;
use graphql_static_bac::stages::s3_paths::{enumerate_selected_targets, enumerate_target};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/user_device")
        .join(name)
}

fn watchlist_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/s3/watchlist")
        .join(name)
}

fn edge_sequences(paths: &graphql_static_bac::contracts::TargetPaths) -> BTreeSet<Vec<String>> {
    paths
        .caps
        .iter()
        .map(|cap| cap.edges.iter().map(|edge| edge.edge_id.clone()).collect())
        .collect()
}

#[test]
fn retains_shared_suffixes_and_target_reentry_cycles() {
    let schema = parse_sdl(
        r#"
        type Query { root: Root }
        type Root { a: A b: B }
        type A { shared: Shared }
        type B { shared: Shared }
        type Shared { target: Target }
        type Target { next: Target }
        "#,
    )
    .unwrap()
    .data;
    let graph = build_type_graph(&schema).unwrap();
    let paths = enumerate_target(&graph, "type:Target").unwrap();
    let sequences = edge_sequences(&paths);

    assert!(sequences.contains(&vec![
        "field:Query.root".to_string(),
        "field:Root.a".to_string(),
        "field:A.shared".to_string(),
        "field:Shared.target".to_string(),
    ]));
    assert!(sequences.contains(&vec![
        "field:Query.root".to_string(),
        "field:Root.b".to_string(),
        "field:B.shared".to_string(),
        "field:Shared.target".to_string(),
    ]));
    assert_eq!(paths.caps.len(), 4);

    let recursive: Vec<_> = paths
        .caps
        .iter()
        .filter(|cap| cap.edges.last().unwrap().edge_id == "field:Target.next")
        .collect();
    assert_eq!(recursive.len(), 2);
    for cap in recursive {
        assert_eq!(cap.cycle_templates.len(), 1);
        assert_eq!(
            cap.cycle_templates[0].repeatable_edge_ids,
            ["field:Target.next"]
        );
    }
}

#[test]
fn traverses_a_non_node_interface_to_the_target() {
    let schema = parse_sdl(
        r#"
        interface AnyCardInterface {
          currentUserSubscription: EmailSubscription
        }
        type Card implements AnyCardInterface {
          currentUserSubscription: EmailSubscription
        }
        interface WithSubscriptionsInterface { id: ID! }
        type Watchlist implements WithSubscriptionsInterface { id: ID! }
        type EmailSubscription {
          anySubscribable: WithSubscriptionsInterface
        }
        type Query { anyCard: AnyCardInterface }
        "#,
    )
    .unwrap()
    .data;
    let graph = build_type_graph(&schema).unwrap();
    let paths = enumerate_target(&graph, "type:Watchlist").unwrap();

    assert!(edge_sequences(&paths).contains(&vec![
        "field:Query.anyCard".to_string(),
        "field:AnyCardInterface.currentUserSubscription".to_string(),
        "field:EmailSubscription.anySubscribable".to_string(),
        "type_condition:WithSubscriptionsInterface->Watchlist".to_string(),
    ]));
}

#[test]
fn emits_two_structurally_valid_global_id_caps_and_keeps_longer_traversal() {
    let schema = parse_sdl(
        r#"
        interface Node { id: ID! }
        type UserDevice implements Node { id: ID! }
        type Other implements Node { id: ID!, device: UserDevice }
        type Query {
          node(id: ID!): Node
          nodes(ids: [ID!]!): [Node]!
          invalidNode(id: String): Node
        }
        "#,
    )
    .unwrap()
    .data;
    let graph = build_type_graph(&schema).unwrap();
    let paths = enumerate_target(&graph, "type:UserDevice").unwrap();

    let global: Vec<_> = paths
        .caps
        .iter()
        .filter(|cap| cap.origin == CapOrigin::GlobalId)
        .collect();
    assert_eq!(global.len(), 2);
    assert_eq!(global[0].entry_field_id, "field:Query.node");
    assert_eq!(global[1].entry_field_id, "field:Query.nodes");
    assert!(paths.caps.iter().any(|cap| {
        cap.origin == CapOrigin::Traversal
            && cap.edges.iter().map(|edge| edge.edge_id.as_str()).eq([
                "field:Query.node",
                "type_condition:Node->Other",
                "field:Other.device",
            ])
    }));
}

#[test]
fn rejects_invalid_global_id_signatures() {
    let schema = parse_sdl(
        r#"
        interface Node { id: ID! }
        type UserDevice implements Node { id: ID! }
        type Query {
          node(id: String): Node
          nodes(ids: ID): [Node]
        }
        "#,
    )
    .unwrap()
    .data;
    let graph = build_type_graph(&schema).unwrap();
    let paths = enumerate_target(&graph, "type:UserDevice").unwrap();
    assert!(paths
        .caps
        .iter()
        .all(|cap| cap.origin == CapOrigin::Traversal));
}

#[test]
fn user_device_runner_matches_golden_cap_content() {
    let schema = read_artifact::<SchemaIrData>(&fixture("s0_schema_ir.json"), StageId::S0).unwrap();
    let sinks = read_artifact::<SinksData>(&fixture("s1_sinks.json"), StageId::S1).unwrap();
    let actual = enumerate_selected_targets(&schema, &sinks).unwrap();
    let expected: Envelope<PathsData> =
        serde_json::from_slice(&fs::read(fixture("s3_paths.json")).unwrap()).unwrap();

    let actual_target = &actual.data.targets["type:UserDevice"];
    let expected_target = &expected.data.targets["type:UserDevice"];
    assert_eq!(actual_target.sink_ref_ids, expected_target.sink_ref_ids);
    assert_eq!(actual_target.caps.len(), 4);

    let actual_by_id: BTreeMap<_, _> = actual_target
        .caps
        .iter()
        .map(|cap| (&cap.cap_id, cap))
        .collect();
    let expected_by_id: BTreeMap<_, _> = expected_target
        .caps
        .iter()
        .map(|cap| (&cap.cap_id, cap))
        .collect();
    assert_eq!(actual_by_id, expected_by_id);

    let ordering: Vec<_> = actual_target
        .caps
        .iter()
        .map(|cap| (cap.origin, cap.entry_field_id.as_str(), cap.cap_id.as_str()))
        .collect();
    assert_eq!(ordering[0].0, CapOrigin::GlobalId);
    assert_eq!(ordering[1].0, CapOrigin::GlobalId);
    assert_eq!(ordering[2].0, CapOrigin::Traversal);
    assert_eq!(ordering[3].0, CapOrigin::Traversal);
}

#[test]
fn rejects_mismatched_stage_fingerprints() {
    let schema = read_artifact::<SchemaIrData>(&fixture("s0_schema_ir.json"), StageId::S0).unwrap();
    let mut sinks = read_artifact::<SinksData>(&fixture("s1_sinks.json"), StageId::S1).unwrap();
    sinks.schema_fingerprint = "sha256:different".to_string();
    let error = enumerate_selected_targets(&schema, &sinks).unwrap_err();
    assert!(error.to_string().contains("fingerprints differ"));
}

#[test]
fn watchlist_is_a_superset_of_all_68_legacy_paths() {
    let schema =
        read_artifact::<SchemaIrData>(&watchlist_fixture("schema_ir.json"), StageId::S0).unwrap();
    let graph = build_type_graph(&schema.data).unwrap();
    let first = enumerate_target(&graph, "type:Watchlist").unwrap();
    let second = enumerate_target(&graph, "type:Watchlist").unwrap();
    assert_eq!(first, second);

    let legacy: Vec<Vec<String>> = serde_json::from_slice(
        &fs::read(watchlist_fixture("legacy_paths.canonical.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(legacy.len(), 68);
    assert_eq!(
        legacy.iter().cloned().collect::<BTreeSet<_>>().len(),
        68,
        "legacy canonical paths must be unique"
    );
    let traversal: BTreeSet<Vec<String>> = first
        .caps
        .iter()
        .filter(|cap| cap.origin == CapOrigin::Traversal)
        .map(|cap| cap.edges.iter().map(|edge| edge.edge_id.clone()).collect())
        .collect();
    assert!(traversal.len() >= 68);
    for path in legacy {
        assert!(traversal.contains(&path), "missing legacy path: {path:?}");
    }

    let global: Vec<_> = first
        .caps
        .iter()
        .filter(|cap| cap.origin == CapOrigin::GlobalId)
        .collect();
    assert_eq!(global.len(), 2);
    assert_eq!(
        global[0]
            .edges
            .iter()
            .map(|edge| edge.edge_id.as_str())
            .collect::<Vec<_>>(),
        ["field:Query.node", "type_condition:Node->Watchlist"]
    );
    assert_eq!(
        global[0].cap_id,
        "cap:sha256:9facf438ee0e4a90c1df7a9e702009d432eeeb922d2d262faa38effe89e35920"
    );
    assert_eq!(
        global[1]
            .edges
            .iter()
            .map(|edge| edge.edge_id.as_str())
            .collect::<Vec<_>>(),
        ["field:Query.nodes", "type_condition:Node->Watchlist"]
    );
    assert_eq!(
        global[1].cap_id,
        "cap:sha256:afef116c94b63051d73fe858c861484ca8ece1cffe59012b0d6192a3f922b3cc"
    );
}
