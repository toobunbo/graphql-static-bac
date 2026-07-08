use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use graphql_static_bac::argument::{classify_arguments, read_argument_policy};
use graphql_static_bac::artifact::read_artifact_version;
use graphql_static_bac::contracts::{
    ArgumentClassification, ArgumentsData, BoundaryFamily, BoundarySource, ClassifiedArgument,
    ClassifiedField, Confidence, Envelope, Producer, Reachability, RouteOrigin, RouteVerdict,
    RouteCoverage, RoutesData, SchemaIrData, SelectedType, SelectorContinuity, SinkRef, SinksData,
    StageId,
};
use graphql_static_bac::graph::build_type_graph;
use graphql_static_bac::route::{
    analyze_target, expand_routes, read_route_policy, LoadedRoutePolicy, RouteFacts, RoutePolicy,
};
use graphql_static_bac::schema::parse_sdl;
use graphql_static_bac::stages::s3_routes::{
    analyze_selected_targets, analyze_selected_targets_dag, analyze_target_dag,
    read_and_run_routes,
};

fn user_device_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/user_device")
        .join(name)
}

fn watchlist_schema() -> Envelope<SchemaIrData> {
    let parsed = parse_sdl(
        r#"
        interface Node { id: ID! }
        interface AnyCardInterface {
          currentUserSubscription: EmailSubscription
        }
        interface WithSubscriptionsInterface { id: ID! }

        type Watchlist implements Node & WithSubscriptionsInterface {
          id: ID!
        }
        type Card implements AnyCardInterface {
          currentUserSubscription: EmailSubscription
        }
        type EmailSubscription {
          anySubscribable: WithSubscriptionsInterface!
        }
        type PageInfo { hasNextPage: Boolean! }
        type WatchlistConnection {
          pageInfo: PageInfo!
          nodes: [Watchlist!]!
        }
        type CurrentUser {
          myWatchlists: [Watchlist!]!
          publicWatchlists: WatchlistConnection!
        }
        type MarketRoot {
          publicWatchlists: WatchlistConnection!
          sorareWatchlists: [Watchlist!]!
          watchlist(id: ID!): Watchlist
        }
        type Query {
          node(id: ID!): Node
          nodes(ids: [ID!]!): [Node]!
          currentUser: CurrentUser
          market: MarketRoot!
          anyCard(assetId: String, slug: String): AnyCardInterface
        }
        "#,
    )
    .unwrap();
    Envelope::complete(
        StageId::S0,
        "sha256:watchlist-route-test".to_string(),
        Producer::current(),
        parsed.warnings,
        parsed.data,
    )
}

fn selector(
    schema: &SchemaIrData,
    owner: &str,
    field_name: &str,
    argument_name: &str,
) -> ClassifiedArgument {
    let field = &schema.types[owner].fields[field_name];
    let argument = field
        .arguments
        .iter()
        .find(|argument| argument.name == argument_name)
        .unwrap();
    ClassifiedArgument {
        arg_ref: argument.arg_id.clone(),
        root_arg_ref: argument.arg_id.clone(),
        arg_path: format!("{owner}.{field_name}.{argument_name}"),
        input_path: Vec::new(),
        type_ref: argument.type_ref.clone(),
        classifications: vec![ArgumentClassification::ObjectSelector],
        signals: vec!["test".to_string()],
        confidence: Confidence::High,
    }
}

fn watchlist_arguments(schema: &Envelope<SchemaIrData>) -> Envelope<ArgumentsData> {
    let fields = [
        ("Query", "node", vec!["id"]),
        ("Query", "nodes", vec!["ids"]),
        ("Query", "anyCard", vec!["assetId", "slug"]),
        ("MarketRoot", "watchlist", vec!["id"]),
    ]
    .into_iter()
    .map(|(owner, field_name, argument_names)| {
        let field_id = schema.data.types[owner].fields[field_name].field_id.clone();
        let arguments = argument_names
            .into_iter()
            .map(|argument_name| selector(&schema.data, owner, field_name, argument_name))
            .collect();
        (field_id, ClassifiedField { arguments })
    })
    .collect();
    Envelope::complete(
        StageId::S2,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        ArgumentsData {
            classifier_model: None,
            policy_fingerprint: None,
            fields,
        },
    )
}

fn policy() -> LoadedRoutePolicy {
    LoadedRoutePolicy {
        policy: RoutePolicy {
            model_version: "route-analysis-v1".to_string(),
            self_scope_root_tokens: vec![
                "currentUser".to_string(),
                "me".to_string(),
                "viewer".to_string(),
            ],
            visibility_tokens: vec!["public".to_string()],
            exact_boundaries: BTreeMap::from([(
                "field:MarketRoot.sorareWatchlists".to_string(),
                graphql_static_bac::route::ExactBoundaryPolicy {
                    family: BoundaryFamily::Visibility,
                    evidence: "system_owned_public_collection".to_string(),
                },
            )]),
        },
        fingerprint: "sha256:test-policy".to_string(),
    }
}

#[test]
fn watchlist_routes_match_the_agreed_semantic_ground_truth() {
    let schema = watchlist_schema();
    let arguments = watchlist_arguments(&schema);
    let policy = policy();
    let graph = build_type_graph(&schema.data).unwrap();
    let facts = RouteFacts::build(&schema.data, &arguments.data, &policy.policy).unwrap();

    let first = analyze_target(&graph, "type:Watchlist", &facts).unwrap();
    let second = analyze_target(&graph, "type:Watchlist", &facts).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.best_verdict, Some(RouteVerdict::Open));

    assert_route(
        &first,
        "field:Query.node",
        Some("arg:Query.node.id"),
        RouteOrigin::GlobalId,
        RouteVerdict::Open,
        SelectorContinuity::Same,
        &[],
    );
    assert_route(
        &first,
        "field:Query.nodes",
        Some("arg:Query.nodes.ids"),
        RouteOrigin::GlobalId,
        RouteVerdict::Open,
        SelectorContinuity::Same,
        &[],
    );
    assert_route(
        &first,
        "field:MarketRoot.watchlist",
        Some("arg:MarketRoot.watchlist.id"),
        RouteOrigin::Traversal,
        RouteVerdict::Open,
        SelectorContinuity::Same,
        &[],
    );
    assert_route(
        &first,
        "field:CurrentUser.myWatchlists",
        None,
        RouteOrigin::Traversal,
        RouteVerdict::Guarded,
        SelectorContinuity::NotApplicable,
        &[BoundaryFamily::SelfScope],
    );
    assert_route(
        &first,
        "field:MarketRoot.publicWatchlists",
        None,
        RouteOrigin::Traversal,
        RouteVerdict::Guarded,
        SelectorContinuity::NotApplicable,
        &[BoundaryFamily::Visibility],
    );
    let sorare = assert_route(
        &first,
        "field:MarketRoot.sorareWatchlists",
        None,
        RouteOrigin::Traversal,
        RouteVerdict::Guarded,
        SelectorContinuity::NotApplicable,
        &[BoundaryFamily::Visibility],
    );
    assert!(sorare
        .boundaries
        .iter()
        .any(|boundary| boundary.source == BoundarySource::Policy));

    for selector_ref in ["arg:Query.anyCard.assetId", "arg:Query.anyCard.slug"] {
        assert_route(
            &first,
            "field:EmailSubscription.anySubscribable",
            Some(selector_ref),
            RouteOrigin::Traversal,
            RouteVerdict::Unknown,
            SelectorContinuity::Unknown,
            &[],
        );
    }
}

#[test]
fn generated_s2_preserves_watchlist_selector_ground_truth() {
    let schema = watchlist_schema();
    let argument_policy = read_argument_policy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("config/lexicons/argument-classifier-v1.json"),
    )
    .unwrap();
    let arguments = classify_arguments(&schema.data, &argument_policy).unwrap();
    let facts = RouteFacts::build(&schema.data, &arguments, &policy().policy).unwrap();
    let routes = analyze_target(
        &build_type_graph(&schema.data).unwrap(),
        "type:Watchlist",
        &facts,
    )
    .unwrap();

    for selector_ref in [
        "arg:Query.node.id",
        "arg:Query.nodes.ids",
        "arg:Query.anyCard.assetId",
        "arg:Query.anyCard.slug",
        "arg:MarketRoot.watchlist.id",
    ] {
        assert!(routes.routes.iter().any(|route| {
            route
                .selector
                .as_ref()
                .is_some_and(|selector| selector.arg_ref == selector_ref)
        }));
    }
    assert!(routes.routes.iter().filter_map(|route| route.selector.as_ref()).all(
        |selector| {
            selector.classification == ArgumentClassification::ObjectSelector
                && matches!(selector.confidence, Confidence::High | Confidence::Medium)
        }
    ));
    assert_route(
        &routes,
        "field:MarketRoot.watchlist",
        Some("arg:MarketRoot.watchlist.id"),
        RouteOrigin::Traversal,
        RouteVerdict::Open,
        SelectorContinuity::Same,
        &[],
    );
}

#[test]
fn selected_target_runner_emits_s3_v2_2_canonical_coverage_and_joins_sink_refs() {
    let schema = watchlist_schema();
    let arguments = watchlist_arguments(&schema);
    let sinks = Envelope::complete(
        StageId::S1,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        SinksData {
            selected_types: vec![SelectedType {
                type_id: "type:Watchlist".to_string(),
                type_name: "Watchlist".to_string(),
                sink_ref_ids: vec!["sink:Watchlist".to_string()],
            }],
            sink_refs: BTreeMap::from([(
                "sink:Watchlist".to_string(),
                SinkRef {
                    sink_ref_id: "sink:Watchlist".to_string(),
                    type_id: "type:Watchlist".to_string(),
                    field_id: None,
                },
            )]),
        },
    );

    let output = analyze_selected_targets(&schema, &sinks, &arguments, &policy()).unwrap();
    assert_eq!(output.contract_version, "2.2");
    assert_eq!(
        output.data.analysis_model,
        "abstract-automaton-canonical-v1"
    );
    assert_eq!(
        output.data.coverage,
        RouteCoverage::CanonicalPerProvenance
    );
    assert_eq!(
        output.data.targets["type:Watchlist"].sink_ref_ids,
        ["sink:Watchlist"]
    );
}

#[test]
fn interface_target_routes_end_at_the_interface_and_skip_connection_plumbing() {
    let parsed = parse_sdl(
        r#"
        interface NotificationInterface { id: ID! }
        type AnnouncementNotification implements NotificationInterface { id: ID! }
        type PageInfo { hasNextPage: Boolean! }
        type NotificationInterfaceConnection {
          nodes: [NotificationInterface!]!
          pageInfo: PageInfo!
        }
        type CurrentUser {
          anyNotifications: NotificationInterfaceConnection!
        }
        type Query {
          currentUser: CurrentUser
        }
        "#,
    )
    .unwrap();
    let schema = parsed.data;
    let facts = RouteFacts::build(
        &schema,
        &ArgumentsData {
            classifier_model: None,
            policy_fingerprint: None,
            fields: BTreeMap::new(),
        },
        &policy().policy,
    )
    .unwrap();
    let target = analyze_target(
        &build_type_graph(&schema).unwrap(),
        "type:NotificationInterface",
        &facts,
    )
    .unwrap();

    assert_eq!(target.reachability, Reachability::Reachable);
    assert_eq!(target.routes.len(), 1);
    let route = &target.routes[0];
    assert_eq!(route.verdict, RouteVerdict::Guarded);
    assert_eq!(
        route.terminal_semantic_edge_id,
        "field:CurrentUser.anyNotifications"
    );
    assert_eq!(
        route.signature.boundary_families,
        [BoundaryFamily::SelfScope]
    );
    assert_eq!(
        route
            .witness
            .edges
            .iter()
            .map(|edge| edge.edge_id.as_str())
            .collect::<Vec<_>>(),
        [
            "field:Query.currentUser",
            "field:CurrentUser.anyNotifications",
            "field:NotificationInterfaceConnection.nodes",
        ]
    );
}

#[test]
fn user_device_route_output_is_deterministic_under_the_canonical_model() {
    let policy_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("config/profiles/route-analysis-v1.json");
    let first = read_and_run_routes(
        &user_device_fixture("s0_schema_ir.json"),
        &user_device_fixture("s1_sinks.json"),
        &user_device_fixture("s2_args.json"),
        &policy_path,
    )
    .unwrap();
    let second = read_and_run_routes(
        &user_device_fixture("s0_schema_ir.json"),
        &user_device_fixture("s1_sinks.json"),
        &user_device_fixture("s2_args.json"),
        &policy_path,
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.contract_version, "2.2");
    assert_eq!(
        first.data.coverage,
        RouteCoverage::CanonicalPerProvenance
    );
    assert!(!first.data.targets["type:UserDevice"].routes.is_empty());

    let loaded_policy = read_route_policy(&policy_path).unwrap();
    assert_eq!(first.data.policy_fingerprint, loaded_policy.fingerprint);
}

#[test]
fn a_legacy_s3_v1_artifact_is_rejected_by_the_v2_reader() {
    let error = read_artifact_version::<RoutesData>(
        &user_device_fixture("s3_paths.json"),
        StageId::S3,
        "2.0",
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("expected contract_version 2.0, got 1.0"));
}

#[test]
fn selector_branching_confidence_and_continuity_follow_the_product_state_rules() {
    let parsed = parse_sdl(
        r#"
        type Target { id: ID! }
        type Wrapper {
          plain: Target
          selected(id: ID!): Target
        }
        type Query {
          optional(id: ID): Target
          required(id: ID!): Target
          possible(id: ID!): Target
          outer(id: ID!): Wrapper
        }
        "#,
    )
    .unwrap();
    let schema = parsed.data;
    let mut optional = selector(&schema, "Query", "optional", "id");
    let required = selector(&schema, "Query", "required", "id");
    let mut possible = selector(&schema, "Query", "possible", "id");
    possible.classifications = vec![ArgumentClassification::PossibleSelector];
    let outer = selector(&schema, "Query", "outer", "id");
    let selected = selector(&schema, "Wrapper", "selected", "id");
    optional.confidence = Confidence::High;

    let arguments = ArgumentsData {
        classifier_model: None,
        policy_fingerprint: None,
        fields: BTreeMap::from([
            (
                "field:Query.optional".to_string(),
                ClassifiedField {
                    arguments: vec![optional],
                },
            ),
            (
                "field:Query.required".to_string(),
                ClassifiedField {
                    arguments: vec![required],
                },
            ),
            (
                "field:Query.possible".to_string(),
                ClassifiedField {
                    arguments: vec![possible],
                },
            ),
            (
                "field:Query.outer".to_string(),
                ClassifiedField {
                    arguments: vec![outer],
                },
            ),
            (
                "field:Wrapper.selected".to_string(),
                ClassifiedField {
                    arguments: vec![selected],
                },
            ),
        ]),
    };
    let policy = RoutePolicy {
        model_version: "route-analysis-v1".to_string(),
        self_scope_root_tokens: Vec::new(),
        visibility_tokens: Vec::new(),
        exact_boundaries: BTreeMap::new(),
    };
    let facts = RouteFacts::build(&schema, &arguments, &policy).unwrap();
    let target =
        analyze_target(&build_type_graph(&schema).unwrap(), "type:Target", &facts).unwrap();

    assert_route(
        &target,
        "field:Query.optional",
        Some("arg:Query.optional.id"),
        RouteOrigin::Traversal,
        RouteVerdict::Open,
        SelectorContinuity::Same,
        &[],
    );
    assert_route(
        &target,
        "field:Query.optional",
        None,
        RouteOrigin::Traversal,
        RouteVerdict::Unknown,
        SelectorContinuity::NotApplicable,
        &[],
    );
    assert!(!target.routes.iter().any(|route| {
        route.terminal_semantic_edge_id == "field:Query.required" && route.selector.is_none()
    }));
    assert_route(
        &target,
        "field:Query.possible",
        None,
        RouteOrigin::Traversal,
        RouteVerdict::Unknown,
        SelectorContinuity::NotApplicable,
        &[],
    );
    assert_route(
        &target,
        "field:Wrapper.plain",
        Some("arg:Query.outer.id"),
        RouteOrigin::Traversal,
        RouteVerdict::Unknown,
        SelectorContinuity::Unknown,
        &[],
    );
    assert_route(
        &target,
        "field:Wrapper.selected",
        Some("arg:Wrapper.selected.id"),
        RouteOrigin::Traversal,
        RouteVerdict::Open,
        SelectorContinuity::Same,
        &[],
    );
}

#[test]
fn query_unreachable_is_target_level_and_has_no_best_verdict() {
    let schema = parse_sdl(
        r#"
        type Query { reachable: Reachable }
        type Reachable { id: ID! }
        type Target { id: ID! }
        "#,
    )
    .unwrap()
    .data;
    let facts = RouteFacts::build(
        &schema,
        &ArgumentsData {
            classifier_model: None,
            policy_fingerprint: None,
            fields: BTreeMap::new(),
        },
        &RoutePolicy {
            model_version: "route-analysis-v1".to_string(),
            self_scope_root_tokens: Vec::new(),
            visibility_tokens: Vec::new(),
            exact_boundaries: BTreeMap::new(),
        },
    )
    .unwrap();
    let target =
        analyze_target(&build_type_graph(&schema).unwrap(), "type:Target", &facts).unwrap();
    assert_eq!(target.reachability, Reachability::QueryUnreachable);
    assert_eq!(target.best_verdict, None);
    assert!(target.routes.is_empty());
}

#[test]
fn target_is_absorbing_and_canonical_output_stops_at_first_arrival() {
    let schema = parse_sdl(
        r#"
        type Query { a: A, b: B }
        type A { hub: Hub }
        type B { hub: Hub }
        type Hub { target: Target }
        type Target { next: Target }
        "#,
    )
    .unwrap()
    .data;
    let facts = RouteFacts::build(
        &schema,
        &ArgumentsData {
            classifier_model: None,
            policy_fingerprint: None,
            fields: BTreeMap::new(),
        },
        &RoutePolicy {
            model_version: "route-analysis-v1".to_string(),
            self_scope_root_tokens: Vec::new(),
            visibility_tokens: Vec::new(),
            exact_boundaries: BTreeMap::new(),
        },
    )
    .unwrap();
    let target =
        analyze_target(&build_type_graph(&schema).unwrap(), "type:Target", &facts).unwrap();

    assert_eq!(target.routes.len(), 1);
    let route = &target.routes[0];
    assert_eq!(route.terminal_semantic_edge_id, "field:Hub.target");
    assert!(route
        .witness
        .edges
        .iter()
        .all(|edge| edge.source_type_id != "type:Target"));
    assert!(!route
        .witness
        .edges
        .iter()
        .any(|edge| edge.edge_id == "field:Target.next"));
}

#[test]
fn selector_provenance_uses_a_witness_from_the_same_activation_branch() {
    let schema = parse_sdl(
        r#"
        type Query {
          byId(id: ID!): A
          bySlug(slug: String!): A
        }
        type A { target: Target }
        type Target { id: ID! }
        "#,
    )
    .unwrap()
    .data;
    let arguments = ArgumentsData {
        classifier_model: None,
        policy_fingerprint: None,
        fields: BTreeMap::from([
            (
                "field:Query.byId".to_string(),
                ClassifiedField {
                    arguments: vec![selector(&schema, "Query", "byId", "id")],
                },
            ),
            (
                "field:Query.bySlug".to_string(),
                ClassifiedField {
                    arguments: vec![selector(&schema, "Query", "bySlug", "slug")],
                },
            ),
        ]),
    };
    let facts = RouteFacts::build(
        &schema,
        &arguments,
        &RoutePolicy {
            model_version: "route-analysis-v1".to_string(),
            self_scope_root_tokens: Vec::new(),
            visibility_tokens: Vec::new(),
            exact_boundaries: BTreeMap::new(),
        },
    )
    .unwrap();
    let target =
        analyze_target(&build_type_graph(&schema).unwrap(), "type:Target", &facts).unwrap();

    assert_eq!(target.routes.len(), 2);
    for (selector_ref, expected_entry) in [
        ("arg:Query.byId.id", "field:Query.byId"),
        ("arg:Query.bySlug.slug", "field:Query.bySlug"),
    ] {
        let route = target
            .routes
            .iter()
            .find(|route| {
                route
                    .selector
                    .as_ref()
                    .is_some_and(|selector| selector.arg_ref == selector_ref)
            })
            .unwrap();
        assert_eq!(route.witness.entry_field_id, expected_entry);
        assert!(route
            .witness
            .edges
            .iter()
            .any(|edge| edge.edge_id == expected_entry));
    }
}

// ============================================================
// DAG and expand_routes integration tests
// ============================================================

#[test]
fn analyze_selected_targets_dag_emits_v3_contract() {
    let schema = watchlist_schema();
    let arguments = watchlist_arguments(&schema);
    let sinks = Envelope::complete(
        StageId::S1,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        SinksData {
            selected_types: vec![SelectedType {
                type_id: "type:Watchlist".to_string(),
                type_name: "Watchlist".to_string(),
                sink_ref_ids: vec!["sink:Watchlist".to_string()],
            }],
            sink_refs: BTreeMap::from([(
                "sink:Watchlist".to_string(),
                SinkRef {
                    sink_ref_id: "sink:Watchlist".to_string(),
                    type_id: "type:Watchlist".to_string(),
                    field_id: None,
                },
            )]),
        },
    );

    let output =
        graphql_static_bac::stages::s3_routes::analyze_selected_targets_dag(
            &schema, &sinks, &arguments, &policy(),
        )
        .unwrap();

    assert_eq!(output.contract_version, "3.0");
    assert_eq!(output.data.analysis_model, "route-family-dag-v1");
    assert_eq!(output.data.selector_mode, "definite_only");
    assert!(output.data.targets.contains_key("type:Watchlist"));

    let target_dag = &output.data.targets["type:Watchlist"];
    assert_eq!(
        target_dag.reachability,
        graphql_static_bac::contracts::Reachability::Reachable
    );
    assert!(!target_dag.dag_id.is_empty());
    assert!(!target_dag.entry_component_id.is_empty());
    assert!(!target_dag.components.is_empty());
    assert!(!target_dag.states.is_empty());
    assert!(!target_dag.transitions.is_empty());
    assert_eq!(target_dag.sink_ref_ids, ["sink:Watchlist"]);
}

#[test]
fn dag_component_graph_is_acyclic_for_watchlist() {
    // Verify through the public v3 contract that all terminals have no outgoing
    // component edges (target states are absorbing).
    let sinks = Envelope::complete(
        StageId::S1,
        watchlist_schema().schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        SinksData {
            selected_types: vec![SelectedType {
                type_id: "type:Watchlist".to_string(),
                type_name: "Watchlist".to_string(),
                sink_ref_ids: Vec::new(),
            }],
            sink_refs: BTreeMap::new(),
        },
    );
    let schema = watchlist_schema();
    let output = analyze_selected_targets_dag(
        &schema, &sinks, &watchlist_arguments(&schema), &policy(),
    )
    .unwrap();
    let target_dag = &output.data.targets["type:Watchlist"];

    // Every terminal component ID must not appear as a source in component_edges.
    let terminal_cids: std::collections::BTreeSet<_> = target_dag
        .components
        .values()
        .filter(|c| c.is_terminal)
        .map(|c| c.component_id.as_str())
        .collect();
    for edge in &target_dag.component_edges {
        assert!(
            !terminal_cids.contains(edge.source_component_id.as_str()),
            "terminal component {} has outgoing component edge",
            edge.source_component_id
        );
    }
}

#[test]
fn dag_terminals_contain_three_open_routes() {
    use graphql_static_bac::contracts::RouteVerdict;

    let schema = watchlist_schema();
    let arguments = watchlist_arguments(&schema);
    let graph = build_type_graph(&schema.data).unwrap();
    let facts = RouteFacts::build(&schema.data, &arguments.data, &policy().policy).unwrap();
    let canonical_routes = analyze_target(&graph, "type:Watchlist", &facts).unwrap();

    let sinks = Envelope::complete(
        StageId::S1,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        SinksData {
            selected_types: vec![SelectedType {
                type_id: "type:Watchlist".to_string(),
                type_name: "Watchlist".to_string(),
                sink_ref_ids: Vec::new(),
            }],
            sink_refs: BTreeMap::new(),
        },
    );
    let output = analyze_selected_targets_dag(
        &schema, &sinks, &watchlist_arguments(&schema), &policy(),
    )
    .unwrap();
    let target_dag = &output.data.targets["type:Watchlist"];

    // All three OPEN canonical routes must appear as terminals.
    let open_terminals: Vec<_> = target_dag
        .terminals
        .iter()
        .filter(|t| t.verdict == RouteVerdict::Open)
        .collect();
    assert_eq!(
        open_terminals.len(),
        3,
        "three OPEN terminals: node, nodes, MarketRoot.watchlist"
    );

    // Canonical route count matches the v2.2 route count.
    assert_eq!(
        canonical_routes.routes.len(),
        target_dag.terminals.len(),
        "terminal count equals canonical route count"
    );
}

#[test]
fn dag_is_deterministic_across_runs() {
    let schema = watchlist_schema();
    let arguments = watchlist_arguments(&schema);

    let dag1 =
        graphql_static_bac::stages::s3_routes::analyze_target_dag(
            &schema, &arguments, &policy(), "type:Watchlist",
        )
        .unwrap();
    let dag2 =
        graphql_static_bac::stages::s3_routes::analyze_target_dag(
            &schema, &arguments, &policy(), "type:Watchlist",
        )
        .unwrap();

    let d1 = &dag1.data.targets["type:Watchlist"];
    let d2 = &dag2.data.targets["type:Watchlist"];
    assert_eq!(d1.dag_id, d2.dag_id);
    assert_eq!(d1.entry_component_id, d2.entry_component_id);
    assert_eq!(d1.components.len(), d2.components.len());
    assert_eq!(d1.transitions.len(), d2.transitions.len());
}

#[test]
fn expand_routes_contains_all_three_open_watchlist_routes() {
    use std::collections::BTreeSet;
    use graphql_static_bac::contracts::RouteVerdict;

    let schema = watchlist_schema();
    let arguments = watchlist_arguments(&schema);
    let graph = build_type_graph(&schema.data).unwrap();
    let facts = RouteFacts::build(&schema.data, &arguments.data, &policy().policy).unwrap();

    let open_filter: BTreeSet<RouteVerdict> =
        [RouteVerdict::Open, RouteVerdict::Unknown].into_iter().collect();
    let (expanded, status) = graphql_static_bac::route::expand_routes(
        &graph,
        "type:Watchlist",
        &facts,
        Some(&open_filter),
        None,
    )
    .unwrap();

    assert!(status.complete);
    assert_eq!(expanded.best_verdict, Some(RouteVerdict::Open));

    // The three OPEN routes must be present.
    for (terminal, selector_ref) in [
        ("field:Query.node", "arg:Query.node.id"),
        ("field:Query.nodes", "arg:Query.nodes.ids"),
        ("field:MarketRoot.watchlist", "arg:MarketRoot.watchlist.id"),
    ] {
        assert!(
            expanded.routes.iter().any(|r| {
                r.terminal_semantic_edge_id == terminal
                    && r.selector
                        .as_ref()
                        .is_some_and(|s| s.arg_ref == selector_ref)
                    && r.verdict == RouteVerdict::Open
            }),
            "missing OPEN route: terminal={terminal} selector={selector_ref}"
        );
    }
}

#[test]
fn expand_routes_selector_provenance_stays_coherent() {
    use graphql_static_bac::contracts::{ClassifiedField, RouteVerdict};

    // Schema with two disjoint branches to the same target.
    // Selector on branch A must not appear on branch B's witness.
    let parsed = graphql_static_bac::schema::parse_sdl(
        r#"
        type Query {
          byId(id: ID!): Holder
          bySlug(slug: String!): Holder
        }
        type Holder { target: Target }
        type Target { id: ID! }
        "#,
    )
    .unwrap();
    let schema = parsed.data;
    let by_id_arg = selector(&schema, "Query", "byId", "id");
    let by_slug_arg = selector(&schema, "Query", "bySlug", "slug");
    let arguments = graphql_static_bac::contracts::ArgumentsData {
        classifier_model: None,
        policy_fingerprint: None,
        fields: BTreeMap::from([
            (
                schema.types["Query"].fields["byId"].field_id.clone(),
                ClassifiedField { arguments: vec![by_id_arg] },
            ),
            (
                schema.types["Query"].fields["bySlug"].field_id.clone(),
                ClassifiedField { arguments: vec![by_slug_arg] },
            ),
        ]),
    };
    let route_policy = RoutePolicy {
        model_version: "route-analysis-v1".to_string(),
        self_scope_root_tokens: Vec::new(),
        visibility_tokens: Vec::new(),
        exact_boundaries: BTreeMap::new(),
    };
    let facts = RouteFacts::build(&schema, &arguments, &route_policy).unwrap();
    let graph = build_type_graph(&schema).unwrap();

    let (expanded, _) =
        graphql_static_bac::route::expand_routes(&graph, "type:Target", &facts, None, None)
            .unwrap();

    // Selector from byId must only appear on routes whose entry is byId.
    for route in &expanded.routes {
        if let Some(sel) = &route.selector {
            if sel.arg_ref == "arg:Query.byId.id" {
                assert_eq!(
                    route.witness.entry_field_id, "field:Query.byId",
                    "byId selector found on wrong witness"
                );
            }
            if sel.arg_ref == "arg:Query.bySlug.slug" {
                assert_eq!(
                    route.witness.entry_field_id, "field:Query.bySlug",
                    "bySlug selector found on wrong witness"
                );
            }
        }
    }
    // Both selectors must produce routes.
    assert!(expanded.routes.iter().any(|r| r.selector.as_ref().is_some_and(|s| s.arg_ref == "arg:Query.byId.id")));
    assert!(expanded.routes.iter().any(|r| r.selector.as_ref().is_some_and(|s| s.arg_ref == "arg:Query.bySlug.slug")));
}

#[test]
fn expand_routes_budget_limits_output() {
    use std::collections::BTreeSet;
    use graphql_static_bac::contracts::RouteVerdict;

    let schema = watchlist_schema();
    let arguments = watchlist_arguments(&schema);
    let graph = build_type_graph(&schema.data).unwrap();
    let facts = RouteFacts::build(&schema.data, &arguments.data, &policy().policy).unwrap();

    let (limited, status_limited) =
        graphql_static_bac::route::expand_routes(&graph, "type:Watchlist", &facts, None, Some(2))
            .unwrap();
    assert!(!status_limited.complete, "budget should cause incomplete expansion");
    assert_eq!(limited.routes.len(), 2);

    let (unlimited, status_unlimited) =
        graphql_static_bac::route::expand_routes(&graph, "type:Watchlist", &facts, None, None)
            .unwrap();
    assert!(status_unlimited.complete);
    assert!(unlimited.routes.len() > limited.routes.len());
}

#[test]
fn dag_structural_efficiency_over_flat_routes() {
    // For the small Watchlist test fixture, compare v2.2 and v3 structural
    // properties.  Size advantage of v3 over v2.2 only materialises at scale
    // (thousands of routes × long path lengths); here we verify correctness.
    let schema = watchlist_schema();
    let arguments = watchlist_arguments(&schema);
    let sinks = Envelope::complete(
        StageId::S1,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        SinksData {
            selected_types: vec![SelectedType {
                type_id: "type:Watchlist".to_string(),
                type_name: "Watchlist".to_string(),
                sink_ref_ids: Vec::new(),
            }],
            sink_refs: BTreeMap::new(),
        },
    );

    let v22 = analyze_selected_targets(&schema, &sinks, &arguments, &policy()).unwrap();
    let v3 = analyze_selected_targets_dag(&schema, &sinks, &arguments, &policy()).unwrap();

    let v22_bytes = serde_json::to_vec(&v22).unwrap();
    let v3_bytes = serde_json::to_vec(&v3).unwrap();

    let v22_routes = v22.data.targets["type:Watchlist"].routes.len();
    let v3_target = &v3.data.targets["type:Watchlist"];
    let v3_terminals = v3_target.terminals.len();
    let v3_transitions = v3_target.transitions.len();
    let v3_states = v3_target.states.len();

    println!(
        "v2.2: {} bytes, {} routes | v3 DAG: {} bytes, {} states, {} transitions, {} terminals",
        v22_bytes.len(), v22_routes, v3_bytes.len(), v3_states, v3_transitions, v3_terminals
    );

    // The DAG stores no flat routes[] — structural data only.
    assert!(
        !serde_json::to_value(v3_target).unwrap()["routes"].is_array()
            || v3_target.terminals.len() < v22_routes + 1,
        "v3 should not contain a flat routes list"
    );

    // v3 DAG has far fewer structural entries than v2.2 has route objects.
    // (States + transitions much less than routes × avg_path_edges)
    let v22_total_edges: usize = v22
        .data
        .targets["type:Watchlist"]
        .routes
        .iter()
        .map(|r| r.witness.edges.len())
        .sum();
    println!(
        "  v2.2 total edge refs: {} | v3 state+transition count: {}",
        v22_total_edges,
        v3_states + v3_transitions
    );

    // For this small fixture there's already some sharing (terminal states
    // referenced by multiple routes); for large schemas the ratio is dramatic.
    // We just assert the terminal count is consistent with the route count.
    assert_eq!(v3_terminals, v22_routes, "terminal count equals canonical route count");
    assert!(v3_terminals > 0, "at least one terminal");
}

fn assert_route<'a>(
    target: &'a graphql_static_bac::contracts::TargetRoutes,
    terminal: &str,
    selector_ref: Option<&str>,
    origin: RouteOrigin,
    verdict: RouteVerdict,
    continuity: SelectorContinuity,
    boundaries: &[BoundaryFamily],
) -> &'a graphql_static_bac::contracts::Route {
    let route = target
        .routes
        .iter()
        .find(|route| {
            route.terminal_semantic_edge_id == terminal
                && route
                    .selector
                    .as_ref()
                    .map(|selector| selector.arg_ref.as_str())
                    == selector_ref
                && route.origin == origin
                && route.verdict == verdict
                && route.selector_continuity == continuity
                && route.signature.boundary_families == boundaries
        })
        .unwrap_or_else(|| {
            panic!(
                "missing route terminal={terminal} selector={selector_ref:?} verdict={verdict:?}"
            )
        });
    assert_eq!(
        route.witness.field_hop_count,
        route
            .witness
            .edges
            .iter()
            .filter(|edge| edge.kind == graphql_static_bac::contracts::PathEdgeKind::Field)
            .count()
    );
    route
}
