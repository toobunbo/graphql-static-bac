use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use graphql_static_bac::contracts::{
    ArgumentClassification, ArgumentsData, ClassifiedArgument, ClassifiedField, Confidence,
    Envelope, ExtractionAnchor, ExtractionMember, ExtractionPlan, PathEdge, PathEdgeKind, Producer,
    Reachability, Route, RouteCoverage, RouteOrigin, RouteSelector, RouteSignature, RouteVerdict,
    RouteWitness, RoutesData, SelectorContinuity, StageId, TargetRoutes,
};
use graphql_static_bac::runtime::{
    extract_joint, run_seed_runtime, GraphqlHttpRequest, GraphqlHttpResponse, GraphqlTransport,
    RuntimeExecutionMode, RuntimeLimits, RuntimeRequestProfile, RuntimeRequestTemplate,
    RuntimeSelection, TransportError,
};
use graphql_static_bac::schema::parse_sdl;
use graphql_static_bac::seed::plan_seed_routes;
use serde_json::json;

#[derive(Default)]
struct FakeTransport {
    requests: Mutex<Vec<serde_json::Value>>,
}

impl GraphqlTransport for FakeTransport {
    fn execute(&self, request: &GraphqlHttpRequest) -> Result<GraphqlHttpResponse, TransportError> {
        self.requests.lock().unwrap().push(request.body.clone());
        let query = request.body["query"].as_str().unwrap();
        let body = if query.contains("currentUser") {
            json!({
                "data": {
                    "currentUser": {
                        "myWatchlists": [
                            {"id": "Watchlist:seed-1"},
                            {"id": "Watchlist:seed-2"}
                        ]
                    }
                }
            })
        } else {
            let id = request.body["variables"]["seed1"].as_str();
            if matches!(id, Some("seed-1" | "seed-2")) {
                json!({
                    "data": {
                        "watchlist": {
                            "__typename": "Watchlist",
                            "id": format!("Watchlist:{}", id.unwrap())
                        }
                    }
                })
            } else {
                json!({"data": {"watchlist": null}})
            }
        };
        Ok(GraphqlHttpResponse {
            status_code: 200,
            body,
        })
    }
}

#[test]
fn runtime_harvests_adapts_and_validates_a_route() {
    let parsed = parse_sdl(
        r#"
        type Watchlist { id: ID! }
        type CurrentUser { myWatchlists: [Watchlist!]! }
        type Query {
          currentUser: CurrentUser
          watchlist(id: ID!): Watchlist
        }
        "#,
    )
    .unwrap();
    let schema_data = parsed.data;
    let selector = schema_data.types["Query"].fields["watchlist"].arguments[0].clone();
    let arguments_data = ArgumentsData {
        classifier_model: None,
        policy_fingerprint: None,
        fields: BTreeMap::from([(
            "field:Query.watchlist".to_string(),
            ClassifiedField {
                arguments: vec![ClassifiedArgument {
                    arg_ref: selector.arg_id.clone(),
                    root_arg_ref: selector.arg_id.clone(),
                    arg_path: "Query.watchlist.id".to_string(),
                    input_path: Vec::new(),
                    type_ref: selector.type_ref.clone(),
                    classifications: vec![ArgumentClassification::ObjectSelector],
                    signals: vec!["fixture".to_string()],
                    confidence: Confidence::High,
                }],
            },
        )]),
    };
    let route_id = "route:sha256:watchlist-runtime".to_string();
    let path_family_id = "family:sha256:watchlist-runtime".to_string();
    let route = Route {
        route_id: route_id.clone(),
        path_family_id: path_family_id.clone(),
        target_type_id: "type:Watchlist".to_string(),
        origin: RouteOrigin::Traversal,
        verdict: RouteVerdict::Open,
        selector: Some(RouteSelector {
            selector_id: selector.arg_id.clone(),
            arg_ref: selector.arg_id.clone(),
            root_arg_ref: selector.arg_id,
            arg_path: "Query.watchlist.id".to_string(),
            input_path: Vec::new(),
            type_ref: selector.type_ref,
            classification: ArgumentClassification::ObjectSelector,
            confidence: Confidence::High,
            selected_type_id: "type:Watchlist".to_string(),
        }),
        selector_continuity: SelectorContinuity::Same,
        terminal_semantic_edge_id: "field:Query.watchlist".to_string(),
        boundaries: Vec::new(),
        signature: RouteSignature {
            origin: RouteOrigin::Traversal,
            selector_id: Some("arg:Query.watchlist.id".to_string()),
            path_family_id,
            terminal_semantic_edge_id: "field:Query.watchlist".to_string(),
            boundary_families: Vec::new(),
            selector_continuity: SelectorContinuity::Same,
            verdict: RouteVerdict::Open,
        },
        witness: RouteWitness {
            witness_id: "witness:watchlist-runtime".to_string(),
            entry_field_id: "field:Query.watchlist".to_string(),
            edges: vec![PathEdge {
                edge_id: "field:Query.watchlist".to_string(),
                kind: PathEdgeKind::Field,
                source_type_id: "type:Query".to_string(),
                field_id: Some("field:Query.watchlist".to_string()),
                target_type_id: "type:Watchlist".to_string(),
            }],
            cycle_templates: Vec::new(),
            field_hop_count: 1,
            display_projection: "Query.watchlist -> Watchlist".to_string(),
        },
    };
    let routes_data = RoutesData {
        analysis_model: "fixture".to_string(),
        policy_fingerprint: "sha256:fixture".to_string(),
        coverage: RouteCoverage::CompleteFamilies,
        targets: BTreeMap::from([(
            "type:Watchlist".to_string(),
            TargetRoutes {
                target_type_id: "type:Watchlist".to_string(),
                sink_ref_ids: Vec::new(),
                reachability: Reachability::Reachable,
                best_verdict: Some(RouteVerdict::Open),
                routes: vec![route],
            },
        )]),
    };
    let seed_data = plan_seed_routes(&schema_data, &arguments_data, &routes_data).unwrap();
    assert!(seed_data.routes[&route_id]
        .binding_set_plans
        .iter()
        .any(|plan| {
            plan.selected_job_ids.iter().any(|job_id| {
                seed_data.routes[&route_id].producer_jobs.iter().any(|job| {
                    job.job_id == *job_id
                        && job
                            .operation
                            .as_deref()
                            .is_some_and(|operation| operation.contains("myWatchlists"))
                })
            })
        }));

    let fingerprint = "sha256:fixture".to_string();
    let schema = Envelope::complete(
        StageId::S0,
        fingerprint.clone(),
        Producer::current(),
        parsed.warnings,
        schema_data,
    );
    let routes = Envelope::complete_with_version(
        "2.1",
        StageId::S3,
        fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        routes_data,
    );
    let seed_plans = Envelope::complete(
        StageId::S4,
        fingerprint,
        Producer::current(),
        Vec::new(),
        seed_data,
    );
    let limits = RuntimeLimits {
        max_requests_per_route: 10,
        max_verified_bindings_per_route: 20,
        ..Default::default()
    };
    assert_eq!(limits.execution_mode, RuntimeExecutionMode::FirstVerified);
    assert_eq!(limits.max_producers_per_route, 3);
    assert_eq!(limits.max_values_per_producer, 3);
    assert_eq!(limits.max_adapter_attempts_per_binding_set, 2);
    let profile = RuntimeRequestProfile {
        request_profile_id: "fixture-profile".to_string(),
        auth_context_id: "fixture-auth".to_string(),
        request: RuntimeRequestTemplate {
            url: "https://example.test/graphql".to_string(),
            method: "POST".to_string(),
            headers: BTreeMap::new(),
            body: json!({"operationName": "", "query": "", "variables": {}}),
        },
        injection: Default::default(),
        limits,
    };
    let transport = FakeTransport::default();
    let artifact = run_seed_runtime(
        &schema,
        &routes,
        &seed_plans,
        &profile,
        &RuntimeSelection {
            verdicts: BTreeSet::from([RouteVerdict::Open]),
            route_ids: BTreeSet::new(),
        },
        &transport,
    )
    .unwrap();

    let result = &artifact.data.route_bindings[&route_id];
    assert_eq!(result.verified_binding_sets.len(), 1);
    let binding = result.verified_binding_sets[0]
        .bindings
        .values()
        .next()
        .unwrap();
    assert_eq!(binding.source_value, json!("Watchlist:seed-1"));
    assert_eq!(binding.consumer_value, json!("seed-1"));
    assert!(binding
        .adapter_chain
        .contains(&"global_id_to_payload".to_string()));
    assert!(artifact
        .data
        .executions
        .values()
        .any(|execution| execution.request["query"]
            .as_str()
            .is_some_and(|query| query.contains("watchlist(id: $seed1)"))));
    assert_eq!(transport.requests.lock().unwrap().len(), 3);

    let mut exhaustive_profile = profile.clone();
    exhaustive_profile.limits.execution_mode = RuntimeExecutionMode::Exhaustive;
    exhaustive_profile.limits.max_verified_bindings_per_route = 2;
    let exhaustive_transport = FakeTransport::default();
    let exhaustive = run_seed_runtime(
        &schema,
        &routes,
        &seed_plans,
        &exhaustive_profile,
        &RuntimeSelection {
            verdicts: BTreeSet::from([RouteVerdict::Open]),
            route_ids: BTreeSet::new(),
        },
        &exhaustive_transport,
    )
    .unwrap();
    assert_eq!(
        exhaustive.data.route_bindings[&route_id]
            .verified_binding_sets
            .len(),
        2
    );
    assert_eq!(exhaustive_transport.requests.lock().unwrap().len(), 5);
}

#[test]
fn joint_extraction_preserves_anchor_lineage() {
    let plan = ExtractionPlan {
        anchor: Some(ExtractionAnchor {
            type_id: "type:Profile".to_string(),
            response_path: "profiles[]".to_string(),
            instance_rule: graphql_static_bac::contracts::AnchorInstanceRule::NearestSharedInstance,
        }),
        members: BTreeMap::from([
            (
                "profile".to_string(),
                ExtractionMember {
                    response_path: "profiles[].id".to_string(),
                    relative_path: "id".to_string(),
                    cardinality: graphql_static_bac::contracts::Cardinality::One,
                },
            ),
            (
                "slug".to_string(),
                ExtractionMember {
                    response_path: "profiles[].decks[].slug".to_string(),
                    relative_path: "decks[].slug".to_string(),
                    cardinality: graphql_static_bac::contracts::Cardinality::Many,
                },
            ),
        ]),
    };
    let rows = extract_joint(
        &json!({
            "profiles": [
                {"id": "p1", "decks": [{"slug": "a"}, {"slug": "b"}]},
                {"id": "p2", "decks": [{"slug": "c"}]}
            ]
        }),
        &plan,
    )
    .unwrap();
    let tuples: Vec<_> = rows
        .iter()
        .map(|row| (row["profile"].value.clone(), row["slug"].value.clone()))
        .collect();
    assert_eq!(
        tuples,
        vec![
            (json!("p1"), json!("a")),
            (json!("p1"), json!("b")),
            (json!("p2"), json!("c"))
        ]
    );
}
