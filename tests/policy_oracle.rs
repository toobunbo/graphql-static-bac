use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::Mutex;

use graphql_static_bac::contracts::{
    ArgumentClassification, BindingSetPlan, BindingValidation, BindingValidationStatus, Confidence,
    DependencyDag, Envelope, PathEdge, PathEdgeKind, PlanStatus, PolicyClass, PolicyHypothesesData,
    PolicyHypothesis, Producer, Reachability, RequirementSource, Route, RouteCoverage, RouteOrigin,
    RouteRuntimeResult, RouteRuntimeStatus, RouteSeedPlan, RouteSelector, RouteSignature,
    RouteVerdict, RouteWitness, RoutesData, RuntimeBinding, RuntimeCoverage, SeedPlansData,
    SeedRequirement, SeedRuntimeData, StageId, TargetRoutes, VerifiedBindingSet,
};
use graphql_static_bac::oracle::run_policy_oracle;
use graphql_static_bac::runtime::{
    emit_projected_operation, GraphqlHttpRequest, GraphqlHttpResponse, GraphqlTransport,
    RuntimeLimits, RuntimeRequestProfile, RuntimeRequestTemplate, RuntimeSelection, TransportError,
};
use graphql_static_bac::schema::parse_sdl;
use serde_json::{json, Value};

struct FakeTransport {
    observer_leaks: bool,
    owner_restricted: bool,
    observer_graphql_error: bool,
    requests: Mutex<Vec<(String, Value)>>,
}

impl GraphqlTransport for FakeTransport {
    fn execute(&self, request: &GraphqlHttpRequest) -> Result<GraphqlHttpResponse, TransportError> {
        let auth = request
            .headers
            .get("x-test-auth")
            .cloned()
            .unwrap_or_default();
        self.requests
            .lock()
            .unwrap()
            .push((auth.clone(), request.body.clone()));
        let is_owner = auth == "owner";
        let visible = is_owner || self.observer_leaks;
        let body = if !is_owner && self.observer_graphql_error {
            json!({
                "data": {"watchlist": null},
                "errors": [{"message": "forbidden"}]
            })
        } else if visible {
            json!({
                "data": {
                    "watchlist": {
                        "__typename": "Watchlist",
                        "_policyIdentity": request.body["variables"]["seed1"],
                        "_policyValue": !self.owner_restricted
                    }
                }
            })
        } else {
            json!({"data": {"watchlist": null}})
        };
        Ok(GraphqlHttpResponse {
            status_code: 200,
            body,
        })
    }
}

#[test]
fn emits_candidate_when_observer_replays_the_restricted_owner_object() {
    let fixture = fixture();
    let transport = FakeTransport {
        observer_leaks: true,
        owner_restricted: true,
        observer_graphql_error: false,
        requests: Mutex::new(Vec::new()),
    };
    let (runtime, candidates) = run_policy_oracle(
        &fixture.schema,
        &fixture.routes,
        &fixture.seed_plans,
        &fixture.owner_seeds,
        &fixture.hypotheses,
        &fixture.owner_profile,
        &fixture.observer_profile,
        &fixture.selection,
        &transport,
    )
    .unwrap();

    assert_eq!(candidates.candidates.len(), 1);
    let candidate = &candidates.candidates[0];
    assert_eq!(candidate.type_name, "Watchlist");
    assert_eq!(candidate.route_id, fixture.route_id);
    assert_eq!(candidate.auth_context.owner, "account_b");
    assert_eq!(candidate.auth_context.observer, "account_a");
    assert_eq!(
        candidate.response["data"]["watchlist"]["_policyIdentity"],
        "Watchlist:victim"
    );
    assert_eq!(
        runtime.data.results[0].outcome,
        graphql_static_bac::contracts::PolicyOracleOutcome::PolicyViolationCandidate
    );
    let golden: Value = serde_json::from_slice(
        &fs::read("tests/fixtures/policy_oracle/watchlist_candidates.json").unwrap(),
    )
    .unwrap();
    assert_eq!(serde_json::to_value(&candidates).unwrap(), golden);

    let requests = transport.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].1["variables"], requests[1].1["variables"]);
    assert_eq!(requests[0].1["variables"]["seed1"], "Watchlist:victim");
    let query = requests[0].1["query"].as_str().unwrap();
    assert!(query.contains("_policyIdentity: id"));
    assert!(query.contains("_policyValue: public"));
}

#[test]
fn observer_null_is_not_written_to_the_candidate_artifact() {
    let fixture = fixture();
    let transport = FakeTransport {
        observer_leaks: false,
        owner_restricted: true,
        observer_graphql_error: false,
        requests: Mutex::new(Vec::new()),
    };
    let (runtime, candidates) = run_policy_oracle(
        &fixture.schema,
        &fixture.routes,
        &fixture.seed_plans,
        &fixture.owner_seeds,
        &fixture.hypotheses,
        &fixture.owner_profile,
        &fixture.observer_profile,
        &fixture.selection,
        &transport,
    )
    .unwrap();

    assert!(candidates.candidates.is_empty());
    assert_eq!(
        runtime.data.results[0].outcome,
        graphql_static_bac::contracts::PolicyOracleOutcome::NoViolationObserved
    );
}

#[test]
fn owner_non_restricted_object_is_inconclusive_without_observer_replay() {
    let fixture = fixture();
    let transport = FakeTransport {
        observer_leaks: true,
        owner_restricted: false,
        observer_graphql_error: false,
        requests: Mutex::new(Vec::new()),
    };
    let (runtime, candidates) = run_policy_oracle(
        &fixture.schema,
        &fixture.routes,
        &fixture.seed_plans,
        &fixture.owner_seeds,
        &fixture.hypotheses,
        &fixture.owner_profile,
        &fixture.observer_profile,
        &fixture.selection,
        &transport,
    )
    .unwrap();

    assert!(candidates.candidates.is_empty());
    assert_eq!(
        runtime.data.results[0].outcome,
        graphql_static_bac::contracts::PolicyOracleOutcome::Inconclusive
    );
    assert_eq!(transport.requests.lock().unwrap().len(), 1);
}

#[test]
fn observer_graphql_error_without_identity_is_inconclusive() {
    let fixture = fixture();
    let transport = FakeTransport {
        observer_leaks: false,
        owner_restricted: true,
        observer_graphql_error: true,
        requests: Mutex::new(Vec::new()),
    };
    let (runtime, candidates) = run_policy_oracle(
        &fixture.schema,
        &fixture.routes,
        &fixture.seed_plans,
        &fixture.owner_seeds,
        &fixture.hypotheses,
        &fixture.owner_profile,
        &fixture.observer_profile,
        &fixture.selection,
        &transport,
    )
    .unwrap();

    assert!(candidates.candidates.is_empty());
    assert_eq!(
        runtime.data.results[0].outcome,
        graphql_static_bac::contracts::PolicyOracleOutcome::Inconclusive
    );
}

#[test]
fn classifier_value_type_mismatch_is_inconclusive_without_network_calls() {
    let mut fixture = fixture();
    fixture.hypotheses.data.hypotheses[0].policy_class = PolicyClass::EnumVisibility;
    fixture.hypotheses.data.hypotheses[0].restricted_values = vec![json!("PRIVATE")];
    let transport = FakeTransport {
        observer_leaks: true,
        owner_restricted: true,
        observer_graphql_error: false,
        requests: Mutex::new(Vec::new()),
    };
    let (runtime, candidates) = run_policy_oracle(
        &fixture.schema,
        &fixture.routes,
        &fixture.seed_plans,
        &fixture.owner_seeds,
        &fixture.hypotheses,
        &fixture.owner_profile,
        &fixture.observer_profile,
        &fixture.selection,
        &transport,
    )
    .unwrap();

    assert!(candidates.candidates.is_empty());
    assert_eq!(
        runtime.data.results[0].failure.as_ref().unwrap().code,
        graphql_static_bac::contracts::PolicyOracleFailureCode::PolicyFieldTypeMismatch
    );
    assert!(transport.requests.lock().unwrap().is_empty());
}

#[test]
fn projected_operation_replays_an_indirect_route_selector_not_the_target_id() {
    let schema = parse_sdl(
        r#"
        type Deck { id: ID!, public: Boolean! }
        type Card { decks: [Deck!]! }
        type Query { anyCard(assetId: String!): Card }
        "#,
    )
    .unwrap()
    .data;
    let argument = schema.types["Query"].fields["anyCard"].arguments[0].clone();
    let path_family_id = "family:sha256:indirect-deck".to_string();
    let route = Route {
        route_id: "route:sha256:indirect-deck".to_string(),
        path_family_id: path_family_id.clone(),
        target_type_id: "type:Deck".to_string(),
        origin: RouteOrigin::Traversal,
        verdict: RouteVerdict::Unknown,
        selector: None,
        selector_continuity: graphql_static_bac::contracts::SelectorContinuity::Unknown,
        terminal_semantic_edge_id: "field:Card.decks".to_string(),
        boundaries: Vec::new(),
        signature: RouteSignature {
            origin: RouteOrigin::Traversal,
            selector_id: Some(argument.arg_id.clone()),
            path_family_id,
            terminal_semantic_edge_id: "field:Card.decks".to_string(),
            boundary_families: Vec::new(),
            selector_continuity: graphql_static_bac::contracts::SelectorContinuity::Unknown,
            verdict: RouteVerdict::Unknown,
        },
        witness: RouteWitness {
            witness_id: "cap:sha256:indirect-deck".to_string(),
            entry_field_id: "field:Query.anyCard".to_string(),
            edges: vec![
                PathEdge {
                    edge_id: "field:Query.anyCard".to_string(),
                    kind: PathEdgeKind::Field,
                    source_type_id: "type:Query".to_string(),
                    field_id: Some("field:Query.anyCard".to_string()),
                    target_type_id: "type:Card".to_string(),
                },
                PathEdge {
                    edge_id: "field:Card.decks".to_string(),
                    kind: PathEdgeKind::Field,
                    source_type_id: "type:Card".to_string(),
                    field_id: Some("field:Card.decks".to_string()),
                    target_type_id: "type:Deck".to_string(),
                },
            ],
            cycle_templates: Vec::new(),
            field_hop_count: 2,
            display_projection: "Query.anyCard -> Card.decks -> Deck".to_string(),
        },
    };
    let requirement = SeedRequirement {
        requirement_id: "seed_req:asset".to_string(),
        consumer_arg_ref: argument.arg_id.clone(),
        root_arg_ref: argument.arg_id,
        consumer_field_id: "field:Query.anyCard".to_string(),
        input_path: Vec::new(),
        leaf_name: "assetId".to_string(),
        type_ref: argument.type_ref,
        source: RequirementSource::RouteSelector,
        selected_type_id: "type:Card".to_string(),
        static_bindings: Vec::new(),
        producer_candidates: Vec::new(),
    };
    let bindings = BTreeMap::from([(
        requirement.requirement_id.clone(),
        RuntimeBinding {
            requirement_id: requirement.requirement_id.clone(),
            input_path: Vec::new(),
            source_value: json!("card-asset-17"),
            consumer_value: json!("card-asset-17"),
            adapter_chain: vec!["identity".to_string()],
            producer_job_id: "seed_job:fixture".to_string(),
            producer_execution_id: None,
            extraction_provenance: Vec::new(),
        },
    )]);
    let operation = emit_projected_operation(
        &schema,
        &route,
        &[requirement],
        &bindings,
        "PolicyOracle",
        &[
            "__typename".to_string(),
            "_policyIdentity: id".to_string(),
            "_policyValue: public".to_string(),
        ],
    )
    .unwrap();

    assert!(operation.operation.contains("anyCard(assetId: $seed1)"));
    assert!(operation.operation.contains("decks {"));
    assert_eq!(operation.variables["seed1"], "card-asset-17");
    assert_ne!(operation.variables["seed1"], "Deck:victim");
}

struct Fixture {
    route_id: String,
    schema: Envelope<graphql_static_bac::contracts::SchemaIrData>,
    routes: Envelope<RoutesData>,
    seed_plans: Envelope<SeedPlansData>,
    owner_seeds: Envelope<SeedRuntimeData>,
    hypotheses: Envelope<PolicyHypothesesData>,
    owner_profile: RuntimeRequestProfile,
    observer_profile: RuntimeRequestProfile,
    selection: RuntimeSelection,
}

fn fixture() -> Fixture {
    let parsed = parse_sdl(
        r#"
        type Watchlist {
          id: ID!
          public: Boolean!
        }
        type Query {
          watchlist(id: ID!): Watchlist
        }
        "#,
    )
    .unwrap();
    let schema_data = parsed.data;
    let argument = schema_data.types["Query"].fields["watchlist"].arguments[0].clone();
    let route_id = "route:sha256:policy-watchlist".to_string();
    let path_family_id = "family:sha256:policy-watchlist".to_string();
    let route = Route {
        route_id: route_id.clone(),
        path_family_id: path_family_id.clone(),
        target_type_id: "type:Watchlist".to_string(),
        origin: RouteOrigin::Traversal,
        verdict: RouteVerdict::Open,
        selector: Some(RouteSelector {
            selector_id: argument.arg_id.clone(),
            arg_ref: argument.arg_id.clone(),
            root_arg_ref: argument.arg_id.clone(),
            arg_path: "Query.watchlist.id".to_string(),
            input_path: Vec::new(),
            type_ref: argument.type_ref.clone(),
            classification: ArgumentClassification::ObjectSelector,
            confidence: Confidence::High,
            selected_type_id: "type:Watchlist".to_string(),
        }),
        selector_continuity: graphql_static_bac::contracts::SelectorContinuity::Same,
        terminal_semantic_edge_id: "field:Query.watchlist".to_string(),
        boundaries: Vec::new(),
        signature: RouteSignature {
            origin: RouteOrigin::Traversal,
            selector_id: Some(argument.arg_id.clone()),
            path_family_id,
            terminal_semantic_edge_id: "field:Query.watchlist".to_string(),
            boundary_families: Vec::new(),
            selector_continuity: graphql_static_bac::contracts::SelectorContinuity::Same,
            verdict: RouteVerdict::Open,
        },
        witness: RouteWitness {
            witness_id: "cap:sha256:policy-watchlist".to_string(),
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
    let requirement_id = "seed_req:sha256:policy-watchlist".to_string();
    let requirement = SeedRequirement {
        requirement_id: requirement_id.clone(),
        consumer_arg_ref: argument.arg_id.clone(),
        root_arg_ref: argument.arg_id.clone(),
        consumer_field_id: "field:Query.watchlist".to_string(),
        input_path: Vec::new(),
        leaf_name: "id".to_string(),
        type_ref: argument.type_ref,
        source: RequirementSource::RouteSelector,
        selected_type_id: "type:Watchlist".to_string(),
        static_bindings: Vec::new(),
        producer_candidates: Vec::new(),
    };
    let plan_id = "seed_binding_plan:sha256:policy-watchlist".to_string();
    let seed_plan = RouteSeedPlan {
        route_id: route_id.clone(),
        target_type_id: "type:Watchlist".to_string(),
        portfolio_truncated: false,
        requirements: vec![requirement],
        correlation_constraints: Vec::new(),
        producer_jobs: Vec::new(),
        dependency_dag: DependencyDag {
            nodes: Vec::new(),
            edges: Vec::new(),
            acyclic: true,
            execution_order: Vec::new(),
        },
        binding_set_plans: vec![BindingSetPlan {
            binding_set_plan_id: plan_id.clone(),
            selected_job_ids: Vec::new(),
            discharged_constraint_ids: Vec::new(),
            execution_order: Vec::new(),
            status: PlanStatus::Executable,
            unresolved_requirement_ids: Vec::new(),
        }],
        unresolved_requirements: Vec::new(),
    };
    let binding_set = VerifiedBindingSet {
        binding_set_id: "seed_binding:sha256:policy-watchlist".to_string(),
        source_binding_set_plan_id: plan_id,
        bindings: BTreeMap::from([(
            requirement_id.clone(),
            RuntimeBinding {
                requirement_id,
                input_path: Vec::new(),
                source_value: json!("Watchlist:victim"),
                consumer_value: json!("Watchlist:victim"),
                adapter_chain: vec!["identity".to_string()],
                producer_job_id: "seed_job:fixture".to_string(),
                producer_execution_id: None,
                extraction_provenance: Vec::new(),
            },
        )]),
        producer_execution_ids: Vec::new(),
        validation_execution_id: "seed_exec:fixture".to_string(),
        validation: BindingValidation {
            status: BindingValidationStatus::Verified,
            target_type_id: "type:Watchlist".to_string(),
            resolved_typename: Some("Watchlist".to_string()),
            adapter_attempts: Vec::new(),
        },
    };
    let fingerprint = "sha256:policy-fixture".to_string();
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
        RoutesData {
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
        },
    );
    let seed_plans = Envelope::complete(
        StageId::S4,
        fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        SeedPlansData {
            planning_model: "fixture".to_string(),
            routes: BTreeMap::from([(route_id.clone(), seed_plan)]),
        },
    );
    let owner_seeds = Envelope::complete(
        StageId::SeedRuntime,
        fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        SeedRuntimeData {
            runtime_model: "fixture".to_string(),
            request_profile_id: "owner-profile".to_string(),
            auth_context_id: "account_b".to_string(),
            route_bindings: BTreeMap::from([(
                route_id.clone(),
                RouteRuntimeResult {
                    route_id: route_id.clone(),
                    target_type_id: "type:Watchlist".to_string(),
                    status: RouteRuntimeStatus::Verified,
                    verified_binding_sets: vec![binding_set],
                    attempted_plan_ids: Vec::new(),
                    failures: Vec::new(),
                    coverage: RuntimeCoverage::Complete,
                },
            )]),
            executions: BTreeMap::new(),
            runtime_facts: Vec::new(),
        },
    );
    let hypotheses = Envelope::complete(
        StageId::PolicyHypotheses,
        fingerprint,
        Producer::current(),
        Vec::new(),
        PolicyHypothesesData {
            classifier_model: "fixture".to_string(),
            hypotheses: vec![PolicyHypothesis {
                hypothesis_id: "policy_hypothesis:sha256:watchlist-public".to_string(),
                type_id: "type:Watchlist".to_string(),
                field_id: "field:Watchlist.public".to_string(),
                policy_class: PolicyClass::BooleanVisibility,
                restricted_values: vec![json!(false)],
                rule: "public_false".to_string(),
            }],
        },
    );
    let limits = RuntimeLimits {
        max_requests: 10,
        max_requests_per_route: 3,
        ..Default::default()
    };
    let profile =
        |request_profile_id: &str, auth_context_id: &str, auth: &str| RuntimeRequestProfile {
            request_profile_id: request_profile_id.to_string(),
            auth_context_id: auth_context_id.to_string(),
            request: RuntimeRequestTemplate {
                url: "https://example.test/graphql".to_string(),
                method: "POST".to_string(),
                headers: BTreeMap::from([("x-test-auth".to_string(), auth.to_string())]),
                body: json!({"operationName": "", "query": "", "variables": {}}),
            },
            injection: Default::default(),
            limits: limits.clone(),
        };
    Fixture {
        route_id,
        schema,
        routes,
        seed_plans,
        owner_seeds,
        hypotheses,
        owner_profile: profile("owner-profile", "account_b", "owner"),
        observer_profile: profile("observer-profile", "account_a", "observer"),
        selection: RuntimeSelection {
            verdicts: BTreeSet::from([RouteVerdict::Open, RouteVerdict::Unknown]),
            route_ids: BTreeSet::new(),
        },
    }
}
