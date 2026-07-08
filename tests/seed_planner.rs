use std::collections::BTreeMap;

use graphql_parser::parse_query;
use graphql_static_bac::artifact::read_artifact;
use graphql_static_bac::contracts::{
    ArgumentClassification, ArgumentsData, ClassifiedArgument, ClassifiedField, Confidence,
    PathEdge, PathEdgeKind, Reachability, Route, RouteCoverage, RouteOrigin, RouteSelector,
    RouteSignature, RouteVerdict, RouteWitness, RoutesData, SchemaIrData, SeedPlansData,
    SelectorContinuity, StageId, TargetRoutes,
};
use graphql_static_bac::schema::parse_sdl;
use graphql_static_bac::seed::plan_seed_routes;
use graphql_static_bac::stages::s3_routes::read_and_run_routes;
use graphql_static_bac::stages::s4_seed_plans::plan_seed_artifact;

fn fixture() -> (
    graphql_static_bac::contracts::SchemaIrData,
    ArgumentsData,
    RoutesData,
) {
    let schema = parse_sdl(
        r#"
        interface Node { id: ID! }
        type Deck { id: ID!, slug: String! }
        type DeckConnection { nodes: [Deck!]! }
        type Profile implements Node {
          id: ID!
          deck(slug: String): Deck
          decks(first: Int): DeckConnection!
        }
        type User { profile: Profile }
        type UserConnection { nodes: [User!]! }
        type Query {
          node(id: ID!): Node
          usersPaginated(first: Int): UserConnection!
        }
        "#,
    )
    .unwrap()
    .data;
    let node_id = schema.types["Query"].fields["node"].arguments[0].clone();
    let deck_slug = schema.types["Profile"].fields["deck"].arguments[0].clone();
    let arguments = ArgumentsData {
        classifier_model: None,
        policy_fingerprint: None,
        fields: BTreeMap::from([
            (
                "field:Query.node".to_string(),
                ClassifiedField {
                    arguments: vec![ClassifiedArgument {
                        arg_ref: node_id.arg_id.clone(),
                        root_arg_ref: node_id.arg_id.clone(),
                        arg_path: "Query.node.id".to_string(),
                        input_path: Vec::new(),
                        type_ref: node_id.type_ref.clone(),
                        classifications: vec![ArgumentClassification::ObjectSelector],
                        signals: vec!["fixture".to_string()],
                        confidence: Confidence::High,
                    }],
                },
            ),
            (
                "field:Profile.deck".to_string(),
                ClassifiedField {
                    arguments: vec![ClassifiedArgument {
                        arg_ref: deck_slug.arg_id.clone(),
                        root_arg_ref: deck_slug.arg_id.clone(),
                        arg_path: "Profile.deck.slug".to_string(),
                        input_path: Vec::new(),
                        type_ref: deck_slug.type_ref.clone(),
                        classifications: vec![ArgumentClassification::ObjectSelector],
                        signals: vec!["fixture".to_string()],
                        confidence: Confidence::High,
                    }],
                },
            ),
        ]),
    };
    let route_id = "route:sha256:profile-deck".to_string();
    let path_family_id = "family:sha256:profile-deck".to_string();
    let route = Route {
        route_id: route_id.clone(),
        path_family_id: path_family_id.clone(),
        target_type_id: "type:Deck".to_string(),
        origin: RouteOrigin::Traversal,
        verdict: RouteVerdict::Open,
        selector: Some(RouteSelector {
            selector_id: deck_slug.arg_id.clone(),
            arg_ref: deck_slug.arg_id.clone(),
            root_arg_ref: deck_slug.arg_id,
            arg_path: "Profile.deck.slug".to_string(),
            input_path: Vec::new(),
            type_ref: deck_slug.type_ref,
            classification: ArgumentClassification::ObjectSelector,
            confidence: Confidence::High,
            selected_type_id: "type:Deck".to_string(),
        }),
        selector_continuity: SelectorContinuity::Same,
        terminal_semantic_edge_id: "field:Profile.deck".to_string(),
        boundaries: Vec::new(),
        signature: RouteSignature {
            origin: RouteOrigin::Traversal,
            selector_id: Some("arg:Profile.deck.slug".to_string()),
            path_family_id,
            terminal_semantic_edge_id: "field:Profile.deck".to_string(),
            boundary_families: Vec::new(),
            selector_continuity: SelectorContinuity::Same,
            verdict: RouteVerdict::Open,
        },
        witness: RouteWitness {
            witness_id: "cap:sha256:profile-deck".to_string(),
            entry_field_id: "field:Query.node".to_string(),
            edges: vec![
                PathEdge {
                    edge_id: "field:Query.node".to_string(),
                    kind: PathEdgeKind::Field,
                    source_type_id: "type:Query".to_string(),
                    field_id: Some("field:Query.node".to_string()),
                    target_type_id: "type:Node".to_string(),
                },
                PathEdge {
                    edge_id: "type_condition:Node->Profile".to_string(),
                    kind: PathEdgeKind::TypeCondition,
                    source_type_id: "type:Node".to_string(),
                    field_id: None,
                    target_type_id: "type:Profile".to_string(),
                },
                PathEdge {
                    edge_id: "field:Profile.deck".to_string(),
                    kind: PathEdgeKind::Field,
                    source_type_id: "type:Profile".to_string(),
                    field_id: Some("field:Profile.deck".to_string()),
                    target_type_id: "type:Deck".to_string(),
                },
            ],
            cycle_templates: Vec::new(),
            field_hop_count: 2,
            display_projection: "Query.node -> ... on Profile -> Profile.deck -> Deck".to_string(),
        },
    };
    let routes = RoutesData {
        analysis_model: "fixture".to_string(),
        policy_fingerprint: "sha256:fixture".to_string(),
        coverage: RouteCoverage::CompleteFamilies,
        targets: BTreeMap::from([(
            "type:Deck".to_string(),
            TargetRoutes {
                target_type_id: "type:Deck".to_string(),
                sink_ref_ids: Vec::new(),
                reachability: Reachability::Reachable,
                best_verdict: Some(RouteVerdict::Open),
                routes: vec![route],
            },
        )]),
    };
    (schema, arguments, routes)
}

#[test]
fn planner_builds_joint_and_threaded_profile_deck_strategies() {
    let (schema, arguments, routes) = fixture();
    let plans = plan_seed_routes(&schema, &arguments, &routes).unwrap();
    let route = &plans.routes["route:sha256:profile-deck"];

    assert_eq!(route.requirements.len(), 2);
    assert_eq!(route.correlation_constraints.len(), 1);
    assert!(route.binding_set_plans[0]
        .unresolved_requirement_ids
        .is_empty());

    let joint_jobs: Vec<_> = route
        .producer_jobs
        .iter()
        .filter(|job| job.strategy == graphql_static_bac::contracts::ProducerStrategy::JointCoRead)
        .collect();
    assert!(!joint_jobs.is_empty());
    assert!(joint_jobs.iter().any(|job| {
        job.operation
            .as_deref()
            .is_some_and(|operation| operation.contains("decks(first: 20)"))
    }));
    assert!(joint_jobs.iter().all(|job| job.extraction.anchor.is_some()));

    assert!(route.producer_jobs.iter().any(|job| {
        job.strategy == graphql_static_bac::contracts::ProducerStrategy::ThreadedDependency
    }));
    assert!(!route.dependency_dag.edges.is_empty());

    for operation in route
        .producer_jobs
        .iter()
        .filter_map(|job| job.operation.as_deref())
    {
        parse_query::<String>(operation).unwrap();
    }
}

#[test]
fn planner_is_deterministic() {
    let (schema, arguments, routes) = fixture();
    let first = plan_seed_routes(&schema, &arguments, &routes).unwrap();
    let second = plan_seed_routes(&schema, &arguments, &routes).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}

#[test]
fn planner_prioritizes_authenticated_inventory_producers() {
    let schema = parse_sdl(
        r#"
        type Card { assetId: String! }
        type CardConnection { nodes: [Card!]! }
        type CurrentUser { cards(first: Int): CardConnection! }
        type Query {
          anyCard(assetId: String): Card
          currentUser: CurrentUser
        }
        "#,
    )
    .unwrap()
    .data;
    let selector = schema.types["Query"].fields["anyCard"].arguments[0].clone();
    let arguments = ArgumentsData {
        classifier_model: None,
        policy_fingerprint: None,
        fields: BTreeMap::from([(
            "field:Query.anyCard".to_string(),
            ClassifiedField {
                arguments: vec![ClassifiedArgument {
                    arg_ref: selector.arg_id.clone(),
                    root_arg_ref: selector.arg_id.clone(),
                    arg_path: "Query.anyCard.assetId".to_string(),
                    input_path: Vec::new(),
                    type_ref: selector.type_ref.clone(),
                    classifications: vec![ArgumentClassification::ObjectSelector],
                    signals: vec!["fixture".to_string()],
                    confidence: Confidence::High,
                }],
            },
        )]),
    };
    let route_id = "route:sha256:card-inventory".to_string();
    let path_family_id = "family:sha256:card-inventory".to_string();
    let route = Route {
        route_id: route_id.clone(),
        path_family_id: path_family_id.clone(),
        target_type_id: "type:Card".to_string(),
        origin: RouteOrigin::Traversal,
        verdict: RouteVerdict::Open,
        selector: Some(RouteSelector {
            selector_id: selector.arg_id.clone(),
            arg_ref: selector.arg_id.clone(),
            root_arg_ref: selector.arg_id,
            arg_path: "Query.anyCard.assetId".to_string(),
            input_path: Vec::new(),
            type_ref: selector.type_ref,
            classification: ArgumentClassification::ObjectSelector,
            confidence: Confidence::High,
            selected_type_id: "type:Card".to_string(),
        }),
        selector_continuity: SelectorContinuity::Same,
        terminal_semantic_edge_id: "field:Query.anyCard".to_string(),
        boundaries: Vec::new(),
        signature: RouteSignature {
            origin: RouteOrigin::Traversal,
            selector_id: Some("arg:Query.anyCard.assetId".to_string()),
            path_family_id,
            terminal_semantic_edge_id: "field:Query.anyCard".to_string(),
            boundary_families: Vec::new(),
            selector_continuity: SelectorContinuity::Same,
            verdict: RouteVerdict::Open,
        },
        witness: RouteWitness {
            witness_id: "witness:card-inventory".to_string(),
            entry_field_id: "field:Query.anyCard".to_string(),
            edges: vec![PathEdge {
                edge_id: "field:Query.anyCard".to_string(),
                kind: PathEdgeKind::Field,
                source_type_id: "type:Query".to_string(),
                field_id: Some("field:Query.anyCard".to_string()),
                target_type_id: "type:Card".to_string(),
            }],
            cycle_templates: Vec::new(),
            field_hop_count: 1,
            display_projection: "Query.anyCard -> Card".to_string(),
        },
    };
    let routes = RoutesData {
        analysis_model: "fixture".to_string(),
        policy_fingerprint: "sha256:fixture".to_string(),
        coverage: RouteCoverage::CompleteFamilies,
        targets: BTreeMap::from([(
            "type:Card".to_string(),
            TargetRoutes {
                target_type_id: "type:Card".to_string(),
                sink_ref_ids: Vec::new(),
                reachability: Reachability::Reachable,
                best_verdict: Some(RouteVerdict::Open),
                routes: vec![route],
            },
        )]),
    };

    let plans = plan_seed_routes(&schema, &arguments, &routes).unwrap();
    let plan = &plans.routes[&route_id];
    let first = &plan.binding_set_plans[0];
    assert!(first.selected_job_ids.iter().any(|job_id| {
        plan.producer_jobs.iter().any(|job| {
            job.job_id == *job_id
                && job
                    .witness
                    .edges
                    .first()
                    .and_then(|edge| edge.field_id.as_deref())
                    == Some("field:Query.currentUser")
        })
    }));
}

#[test]
fn planner_preserves_nested_input_paths_for_static_bindings() {
    let schema = parse_sdl(
        r#"
        enum ItemKind { PRIMARY SECONDARY }
        input ItemLookup { id: ID!, kind: ItemKind! }
        type Item { id: ID! }
        type Query {
          item(filter: ItemLookup!): Item
          items: [Item!]!
        }
        "#,
    )
    .unwrap()
    .data;
    let filter = schema.types["Query"].fields["item"].arguments[0].clone();
    let id_field = schema.types["ItemLookup"].input_fields["id"].clone();
    let arguments = ArgumentsData {
        classifier_model: None,
        policy_fingerprint: None,
        fields: BTreeMap::from([(
            "field:Query.item".to_string(),
            ClassifiedField {
                arguments: vec![ClassifiedArgument {
                    arg_ref: id_field.input_field_id.clone(),
                    root_arg_ref: filter.arg_id.clone(),
                    arg_path: "Query.item.filter.id".to_string(),
                    input_path: vec!["id".to_string()],
                    type_ref: id_field.type_ref.clone(),
                    classifications: vec![ArgumentClassification::ObjectSelector],
                    signals: vec!["fixture".to_string()],
                    confidence: Confidence::High,
                }],
            },
        )]),
    };
    let route_id = "route:sha256:nested-input".to_string();
    let path_family_id = "family:sha256:nested-input".to_string();
    let routes = RoutesData {
        analysis_model: "fixture".to_string(),
        policy_fingerprint: "sha256:fixture".to_string(),
        coverage: RouteCoverage::CompleteFamilies,
        targets: BTreeMap::from([(
            "type:Item".to_string(),
            TargetRoutes {
                target_type_id: "type:Item".to_string(),
                sink_ref_ids: Vec::new(),
                reachability: Reachability::Reachable,
                best_verdict: Some(RouteVerdict::Open),
                routes: vec![Route {
                    route_id: route_id.clone(),
                    path_family_id: path_family_id.clone(),
                    target_type_id: "type:Item".to_string(),
                    origin: RouteOrigin::Traversal,
                    verdict: RouteVerdict::Open,
                    selector: Some(RouteSelector {
                        selector_id: "selector:sha256:nested-input".to_string(),
                        arg_ref: id_field.input_field_id,
                        root_arg_ref: filter.arg_id,
                        arg_path: "Query.item.filter.id".to_string(),
                        input_path: vec!["id".to_string()],
                        type_ref: id_field.type_ref,
                        classification: ArgumentClassification::ObjectSelector,
                        confidence: Confidence::High,
                        selected_type_id: "type:Item".to_string(),
                    }),
                    selector_continuity: SelectorContinuity::Same,
                    terminal_semantic_edge_id: "field:Query.item".to_string(),
                    boundaries: Vec::new(),
                    signature: RouteSignature {
                        origin: RouteOrigin::Traversal,
                        selector_id: Some("selector:sha256:nested-input".to_string()),
                        path_family_id,
                        terminal_semantic_edge_id: "field:Query.item".to_string(),
                        boundary_families: Vec::new(),
                        selector_continuity: SelectorContinuity::Same,
                        verdict: RouteVerdict::Open,
                    },
                    witness: RouteWitness {
                        witness_id: "cap:sha256:nested-input".to_string(),
                        entry_field_id: "field:Query.item".to_string(),
                        edges: vec![PathEdge {
                            edge_id: "field:Query.item".to_string(),
                            kind: PathEdgeKind::Field,
                            source_type_id: "type:Query".to_string(),
                            field_id: Some("field:Query.item".to_string()),
                            target_type_id: "type:Item".to_string(),
                        }],
                        cycle_templates: Vec::new(),
                        field_hop_count: 1,
                        display_projection: "Query.item -> Item".to_string(),
                    },
                }],
            },
        )]),
    };

    let plans = plan_seed_routes(&schema, &arguments, &routes).unwrap();
    let route = &plans.routes[&route_id];
    assert_eq!(route.requirements.len(), 2);
    let kind = route
        .requirements
        .iter()
        .find(|requirement| requirement.leaf_name == "kind")
        .unwrap();
    assert_eq!(kind.input_path, ["kind"]);
    assert_eq!(kind.static_bindings.len(), 2);
    assert!(kind
        .static_bindings
        .iter()
        .all(|binding| binding.input_path == ["kind"]));
    assert_eq!(
        route.binding_set_plans[0].status,
        graphql_static_bac::contracts::PlanStatus::Executable
    );
    assert!(route.binding_set_plans.len() >= 2);
    let selected_enum_values: std::collections::BTreeSet<_> = route
        .binding_set_plans
        .iter()
        .flat_map(|plan| plan.selected_job_ids.iter())
        .filter_map(|job_id| route.producer_jobs.iter().find(|job| &job.job_id == job_id))
        .flat_map(|job| job.static_bindings.iter())
        .filter(|binding| binding.input_path == ["kind"])
        .filter_map(|binding| binding.value.as_deref())
        .collect();
    assert_eq!(
        selected_enum_values,
        ["PRIMARY", "SECONDARY"].into_iter().collect()
    );
}

#[test]
fn global_id_requirement_uses_the_concrete_type_condition_target() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/user_device");
    let schema =
        read_artifact::<SchemaIrData>(&root.join("s0_schema_ir.json"), StageId::S0).unwrap();
    let arguments =
        read_artifact::<ArgumentsData>(&root.join("s2_args.json"), StageId::S2).unwrap();
    let routes = read_and_run_routes(
        &root.join("s0_schema_ir.json"),
        &root.join("s1_sinks.json"),
        &root.join("s2_args.json"),
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("config/profiles/route-analysis-v1.json"),
    )
    .unwrap();
    let plans = plan_seed_artifact(&schema, &arguments, &routes).unwrap();
    assert_global_id_producers_are_concrete(&plans.data);
}

fn assert_global_id_producers_are_concrete(plans: &SeedPlansData) {
    let global_routes: Vec<_> = plans
        .routes
        .values()
        .filter(|route| {
            route
                .requirements
                .iter()
                .any(|requirement| requirement.root_arg_ref.starts_with("arg:Query.node"))
        })
        .collect();
    assert_eq!(global_routes.len(), 2);
    for route in global_routes {
        assert!(route
            .requirements
            .iter()
            .all(|requirement| requirement.selected_type_id == "type:UserDevice"));
        let primary = &route.binding_set_plans[0];
        assert!(route.producer_jobs.iter().any(|job| {
            primary.selected_job_ids.contains(&job.job_id)
                && job
                    .producer_field_ids
                    .contains(&"field:UserDevice.id".to_string())
        }));
    }
}
