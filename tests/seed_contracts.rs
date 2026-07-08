use std::collections::BTreeMap;

use graphql_static_bac::contracts::{
    AnchorInstanceRule, BindingSetPlan, Cardinality, CorrelationBasis, CorrelationBasisKind,
    CorrelationConstraint, DependencyDag, ExtractionAnchor, ExtractionMember, ExtractionPlan,
    PathEdge, PathEdgeKind, PlanStatus, ProducerJob, ProducerStrategy, ProducerWitness,
    RequirementSource, RouteSeedPlan, SeedPlansData, SeedRequirement, TypeKind, TypeRef,
};
use graphql_static_bac::seed::{
    binding_set_plan_id, correlation_constraint_id, producer_job_id, seed_requirement_id,
    validate_seed_plans, SeedPlanValidationError,
};

fn type_ref(name: &str, kind: TypeKind) -> TypeRef {
    TypeRef::new(name, kind, Vec::new()).unwrap()
}

#[test]
fn seed_ids_are_deterministic_and_normalize_set_members() {
    let type_ref = type_ref("ID", TypeKind::Scalar);
    let requirement = seed_requirement_id("arg:Query.node.id", &[], &type_ref);
    assert_eq!(
        requirement,
        seed_requirement_id("arg:Query.node.id", &[], &type_ref)
    );

    let left = correlation_constraint_id(
        "type:Profile",
        &["req:b".to_string(), "req:a".to_string()],
        "field:Profile.deck",
    );
    let right = correlation_constraint_id(
        "type:Profile",
        &["req:a".to_string(), "req:b".to_string()],
        "field:Profile.deck",
    );
    assert_eq!(left, right);

    let edge_a = PathEdge {
        edge_id: "field:Query.profile".to_string(),
        kind: PathEdgeKind::Field,
        source_type_id: "type:Query".to_string(),
        field_id: Some("field:Query.profile".to_string()),
        target_type_id: "type:Profile".to_string(),
    };
    let edge_b = PathEdge {
        edge_id: "field:Query.viewer".to_string(),
        kind: PathEdgeKind::Field,
        source_type_id: "type:Query".to_string(),
        field_id: Some("field:Query.viewer".to_string()),
        target_type_id: "type:Profile".to_string(),
    };
    let first = producer_job_id(
        "standalone",
        std::slice::from_ref(&requirement),
        &["field:Profile.id".to_string()],
        std::slice::from_ref(&edge_a),
        &[],
        &[],
    );
    let second = producer_job_id(
        "standalone",
        std::slice::from_ref(&requirement),
        &["field:Profile.id".to_string()],
        std::slice::from_ref(&edge_b),
        &[],
        &[],
    );
    assert_ne!(first, second);
}

#[test]
fn seed_contract_validation_checks_reference_integrity() {
    let requirement_id = "seed_req:sha256:req".to_string();
    let constraint_id = "seed_constraint:sha256:constraint".to_string();
    let job_id = "seed_job:sha256:job".to_string();
    let plan_id = binding_set_plan_id("route:sha256:route", std::slice::from_ref(&job_id), &[]);
    let requirement = SeedRequirement {
        requirement_id: requirement_id.clone(),
        consumer_arg_ref: "arg:Query.node.id".to_string(),
        root_arg_ref: "arg:Query.node.id".to_string(),
        consumer_field_id: "field:Query.node".to_string(),
        input_path: Vec::new(),
        leaf_name: "id".to_string(),
        type_ref: type_ref("ID", TypeKind::Scalar),
        source: RequirementSource::SchemaRequired,
        selected_type_id: "type:Profile".to_string(),
        static_bindings: Vec::new(),
        producer_candidates: Vec::new(),
    };
    let constraint = CorrelationConstraint {
        constraint_id: constraint_id.clone(),
        members: vec![requirement_id.clone()],
        anchor_type_id: "type:Profile".to_string(),
        basis: CorrelationBasis {
            kind: CorrelationBasisKind::RouteLineage,
            selector_requirement_id: requirement_id.clone(),
            dependent_requirement_id: requirement_id.clone(),
            dependent_field_id: "field:Query.node".to_string(),
        },
        discharged_by_job_ids: vec![job_id.clone()],
    };
    let job = ProducerJob {
        job_id: job_id.clone(),
        strategy: ProducerStrategy::Standalone,
        producer_priority: 0,
        covers_requirements: vec![requirement_id.clone()],
        producer_field_ids: vec!["field:Profile.id".to_string()],
        entry_field_id: Some("field:Query.profile".to_string()),
        witness: ProducerWitness {
            edges: Vec::new(),
            terminal_field_ids: vec!["field:Profile.id".to_string()],
        },
        static_bindings: Vec::new(),
        unresolved_arg_refs: Vec::new(),
        extraction: ExtractionPlan {
            anchor: Some(ExtractionAnchor {
                type_id: "type:Profile".to_string(),
                response_path: "profile".to_string(),
                instance_rule: AnchorInstanceRule::NearestSharedInstance,
            }),
            members: BTreeMap::from([(
                requirement_id.clone(),
                ExtractionMember {
                    response_path: "profile.id".to_string(),
                    relative_path: "id".to_string(),
                    cardinality: Cardinality::One,
                },
            )]),
        },
        operation_name: None,
        operation: None,
        executable: true,
        rejection_reasons: Vec::new(),
    };
    let route = RouteSeedPlan {
        route_id: "route:sha256:route".to_string(),
        target_type_id: "type:Profile".to_string(),
        portfolio_truncated: false,
        requirements: vec![requirement],
        correlation_constraints: vec![constraint],
        producer_jobs: vec![job],
        dependency_dag: DependencyDag {
            nodes: vec![job_id.clone()],
            edges: Vec::new(),
            acyclic: true,
            execution_order: vec![job_id.clone()],
        },
        binding_set_plans: vec![BindingSetPlan {
            binding_set_plan_id: plan_id,
            selected_job_ids: vec![job_id],
            discharged_constraint_ids: vec![constraint_id],
            execution_order: vec!["seed_job:sha256:job".to_string()],
            status: PlanStatus::Executable,
            unresolved_requirement_ids: Vec::new(),
        }],
        unresolved_requirements: Vec::new(),
    };
    let mut data = SeedPlansData {
        planning_model: "seed-planning-v1".to_string(),
        routes: BTreeMap::from([(route.route_id.clone(), route)]),
    };
    validate_seed_plans(&data).unwrap();

    data.routes
        .get_mut("route:sha256:route")
        .unwrap()
        .binding_set_plans[0]
        .selected_job_ids = vec!["seed_job:sha256:missing".to_string()];
    assert!(matches!(
        validate_seed_plans(&data),
        Err(SeedPlanValidationError::MissingReference {
            kind: "producer job",
            ..
        })
    ));
}
