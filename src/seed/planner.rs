use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::contracts::{
    ArgumentsData, BindingSetPlan, Cardinality, DependencyDag, ExtractionMember, ExtractionPlan,
    PlanStatus, ProducerJob, ProducerStrategy, ProducerWitness, Route, RouteSeedPlan, RoutesData,
    SchemaIrData, SeedPlansData, StaticBindingClass, TypeWrapper, UnresolvedReason,
    UnresolvedRequirement,
};
use crate::graph::{build_type_graph, GraphBuildError, TypeGraph};

use super::{
    binding_set_plan_id,
    correlation::{build_joint_jobs, derive_constraints},
    dependency::{build_dependencies, dependency_closure},
    emitter::{emit_operation, ProjectionBranch, QueryEmissionError},
    ids::producer_job_id,
    index::SeedIndex,
    producers::derive_producers,
    requirements::{collect_requirements, CollectedRequirement},
    search::{path_response_segments, search_producer_paths, ProducerPath},
    validate_seed_plans, SeedPlanValidationError,
};

pub const SEED_PLANNING_MODEL: &str = "seed-planning-v1";

#[derive(Debug, Error)]
pub enum SeedPlanningError {
    #[error(transparent)]
    Graph(#[from] GraphBuildError),
    #[error("could not emit seed query: {0}")]
    Query(String),
    #[error(transparent)]
    Validation(#[from] SeedPlanValidationError),
    #[error("seed planning contract violation: {0}")]
    Contract(String),
}

impl From<QueryEmissionError> for SeedPlanningError {
    fn from(value: QueryEmissionError) -> Self {
        Self::Query(value.to_string())
    }
}

pub fn plan_seed_routes(
    schema: &SchemaIrData,
    arguments: &ArgumentsData,
    routes: &RoutesData,
) -> Result<SeedPlansData, SeedPlanningError> {
    let graph = build_type_graph(schema)?;
    let index = SeedIndex::build(schema, arguments);
    let mut path_cache = BTreeMap::<(String, String), Vec<ProducerPath>>::new();
    let mut planned_routes = BTreeMap::new();

    for target in routes.targets.values() {
        for route in &target.routes {
            if planned_routes.contains_key(&route.route_id) {
                return Err(SeedPlanningError::Contract(format!(
                    "duplicate S3 route ID {}",
                    route.route_id
                )));
            }
            let route_plan = plan_route(route, &graph, &index, &mut path_cache)?;
            planned_routes.insert(route.route_id.clone(), route_plan);
        }
    }

    let data = SeedPlansData {
        planning_model: SEED_PLANNING_MODEL.to_string(),
        routes: planned_routes,
    };
    validate_seed_plans(&data)?;
    Ok(data)
}

fn plan_route(
    route: &Route,
    graph: &TypeGraph,
    index: &SeedIndex<'_>,
    path_cache: &mut BTreeMap<(String, String), Vec<ProducerPath>>,
) -> Result<RouteSeedPlan, SeedPlanningError> {
    let mut collected = collect_requirements(route, index);
    for value in &mut collected {
        value.requirement.producer_candidates = derive_producers(&value.requirement, index);
    }
    let mut jobs = Vec::new();
    for value in &collected {
        jobs.extend(static_jobs(value));
        for candidate in producer_candidates_for_jobs(&value.requirement) {
            let paths = path_cache
                .entry((
                    graph.query_root_id.clone(),
                    candidate.producer_parent_type_id.clone(),
                ))
                .or_insert_with(|| {
                    search_producer_paths(
                        graph,
                        index,
                        &graph.query_root_id,
                        &candidate.producer_parent_type_id,
                    )
                });
            for path in paths {
                jobs.push(producer_job(index, value, candidate, path)?);
            }
        }
    }
    let mut constraints = derive_constraints(route, &collected, index);
    jobs.extend(build_joint_jobs(
        graph,
        index,
        &collected,
        &mut constraints,
        path_cache,
    )?);
    jobs.sort_by(compare_jobs);
    jobs.dedup_by(|left, right| left.job_id == right.job_id);

    let requirements: Vec<_> = collected
        .iter()
        .map(|value| value.requirement.clone())
        .collect();
    let dependency_dag = build_dependencies(&requirements, &mut constraints, &mut jobs);
    let (binding_set_plans, unresolved_requirements, portfolio_truncated) = select_plans(
        &route.route_id,
        &requirements,
        &constraints,
        &jobs,
        &dependency_dag,
    );

    Ok(RouteSeedPlan {
        route_id: route.route_id.clone(),
        target_type_id: route.target_type_id.clone(),
        portfolio_truncated,
        requirements,
        correlation_constraints: constraints,
        producer_jobs: jobs,
        dependency_dag,
        binding_set_plans,
        unresolved_requirements,
    })
}

pub(crate) fn producer_candidates_for_jobs(
    requirement: &crate::contracts::SeedRequirement,
) -> Vec<&crate::contracts::ProducerCandidate> {
    let mut candidates: Vec<_> = requirement
        .producer_candidates
        .iter()
        .filter(|candidate| candidate.automatic)
        .collect();
    candidates.sort_by(|left, right| {
        producer_candidate_rank(left)
            .cmp(&producer_candidate_rank(right))
            .then_with(|| left.producer_field_id.cmp(&right.producer_field_id))
    });
    candidates.truncate(4);
    candidates
}

pub(crate) fn producer_candidate_rank(candidate: &crate::contracts::ProducerCandidate) -> u8 {
    use crate::contracts::{ProducerDerivation, ProducerLocus};

    match (candidate.derivation, candidate.field_locus) {
        (ProducerDerivation::ExactLeafMatch, ProducerLocus::Interface) => 0,
        (ProducerDerivation::ExactLeafMatch, ProducerLocus::Object) => 1,
        (ProducerDerivation::IdentityCompatible, ProducerLocus::Interface) => 2,
        (ProducerDerivation::IdentityCompatible, ProducerLocus::Object) => 3,
        (ProducerDerivation::RelatedInterfaceField, _) => 4,
        (ProducerDerivation::RelatedConcreteField, _) => 5,
        (ProducerDerivation::IdentityCompatible, ProducerLocus::Concrete) => 6,
        (ProducerDerivation::TypeOnlySuggestion, _) => 7,
        (ProducerDerivation::ExactLeafMatch, ProducerLocus::Concrete) => 5,
    }
}

fn static_jobs(value: &CollectedRequirement) -> Vec<ProducerJob> {
    value
        .requirement
        .static_bindings
        .iter()
        .filter(|binding| {
            binding.value.is_some() && binding.class != StaticBindingClass::UnresolvedLiteral
        })
        .map(|binding| {
            let bindings = vec![binding.clone()];
            let job_id = producer_job_id(
                "static_binding",
                std::slice::from_ref(&value.requirement.requirement_id),
                &[],
                &[],
                &bindings,
                &[],
            );
            ProducerJob {
                job_id,
                strategy: ProducerStrategy::StaticBinding,
                producer_priority: 0,
                covers_requirements: vec![value.requirement.requirement_id.clone()],
                producer_field_ids: Vec::new(),
                entry_field_id: None,
                witness: ProducerWitness {
                    edges: Vec::new(),
                    terminal_field_ids: Vec::new(),
                },
                static_bindings: bindings,
                unresolved_arg_refs: Vec::new(),
                extraction: ExtractionPlan {
                    anchor: None,
                    members: BTreeMap::new(),
                },
                operation_name: None,
                operation: None,
                executable: true,
                rejection_reasons: Vec::new(),
            }
        })
        .collect()
}

fn producer_job(
    index: &SeedIndex<'_>,
    value: &CollectedRequirement,
    candidate: &crate::contracts::ProducerCandidate,
    path: &ProducerPath,
) -> Result<ProducerJob, SeedPlanningError> {
    let strategy_name = "standalone";
    let job_id = producer_job_id(
        strategy_name,
        std::slice::from_ref(&value.requirement.requirement_id),
        std::slice::from_ref(&candidate.producer_field_id),
        &path.edges,
        &path.static_bindings,
        &path.unresolved_arg_refs,
    );
    let operation_name = operation_name(&job_id);
    let operation = emit_operation(
        index,
        &operation_name,
        &[ProjectionBranch {
            edges: path.edges.clone(),
            terminal_field_id: candidate.producer_field_id.clone(),
        }],
        &path.static_bindings,
        &path.unresolved_arg_refs,
    )?;
    let member = extraction_member(index, path, &candidate.producer_field_id)?;
    let executable = path.unresolved_arg_refs.is_empty();
    Ok(ProducerJob {
        job_id,
        strategy: ProducerStrategy::Standalone,
        producer_priority: producer_candidate_rank(candidate),
        covers_requirements: vec![value.requirement.requirement_id.clone()],
        producer_field_ids: vec![candidate.producer_field_id.clone()],
        entry_field_id: path.entry_field_id.clone(),
        witness: ProducerWitness {
            edges: path.edges.clone(),
            terminal_field_ids: vec![candidate.producer_field_id.clone()],
        },
        static_bindings: path.static_bindings.clone(),
        unresolved_arg_refs: path.unresolved_arg_refs.clone(),
        extraction: ExtractionPlan {
            anchor: None,
            members: BTreeMap::from([(value.requirement.requirement_id.clone(), member)]),
        },
        operation_name: Some(operation_name),
        operation: Some(operation),
        executable,
        rejection_reasons: if executable {
            Vec::new()
        } else {
            vec!["producer_requires_unresolved_arguments".to_string()]
        },
    })
}

fn extraction_member(
    index: &SeedIndex<'_>,
    path: &ProducerPath,
    producer_field_id: &str,
) -> Result<ExtractionMember, SeedPlanningError> {
    let mut segments = path_response_segments(index, &path.edges);
    let producer = index.field(producer_field_id).ok_or_else(|| {
        SeedPlanningError::Contract(format!("missing producer field {producer_field_id}"))
    })?;
    let producer_is_list = producer
        .field
        .return_type
        .wrappers
        .contains(&TypeWrapper::List);
    segments.push((producer.field.name.clone(), producer_is_list));
    let response_path = render_response_path(&segments);
    let cardinality = if segments.iter().any(|(_, is_list)| *is_list) {
        Cardinality::Many
    } else if producer.field.return_type.wrappers.first() == Some(&TypeWrapper::NonNull) {
        Cardinality::One
    } else {
        Cardinality::Optional
    };
    Ok(ExtractionMember {
        response_path: response_path.clone(),
        relative_path: response_path,
        cardinality,
    })
}

fn render_response_path(segments: &[(String, bool)]) -> String {
    segments
        .iter()
        .map(|(name, is_list)| {
            if *is_list {
                format!("{name}[]")
            } else {
                name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

pub(crate) fn operation_name(job_id: &str) -> String {
    let suffix = job_id
        .strip_prefix("seed_job:sha256:")
        .unwrap_or(job_id)
        .chars()
        .take(16)
        .collect::<String>();
    format!("Harvest_{suffix}")
}

const MAX_BINDING_SET_PLANS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PlanSelection {
    selected_job_ids: BTreeSet<String>,
    covered_requirement_ids: BTreeSet<String>,
    discharged_constraint_ids: BTreeSet<String>,
}

fn select_plans(
    route_id: &str,
    requirements: &[crate::contracts::SeedRequirement],
    constraints: &[crate::contracts::CorrelationConstraint],
    jobs: &[ProducerJob],
    dependency_dag: &DependencyDag,
) -> (Vec<BindingSetPlan>, Vec<UnresolvedRequirement>, bool) {
    let jobs_by_id: BTreeMap<_, _> = jobs.iter().map(|job| (job.job_id.as_str(), job)).collect();
    let mut portfolio_truncated = requirements.iter().any(|requirement| {
        jobs.iter()
            .filter(|job| {
                job.executable
                    && job
                        .covers_requirements
                        .contains(&requirement.requirement_id)
            })
            .count()
            > MAX_BINDING_SET_PLANS
    });
    let mut selections = vec![PlanSelection {
        selected_job_ids: BTreeSet::new(),
        covered_requirement_ids: BTreeSet::new(),
        discharged_constraint_ids: BTreeSet::new(),
    }];

    for constraint in constraints {
        let mut options: Vec<_> = constraint
            .discharged_by_job_ids
            .iter()
            .filter_map(|job_id| jobs.iter().find(|job| &job.job_id == job_id))
            .filter(|job| job.executable)
            .collect();
        options.sort_by(|left, right| compare_jobs(left, right));
        if options.is_empty() {
            continue;
        }
        selections = expand_selections(
            selections,
            &options,
            Some(&constraint.constraint_id),
            &jobs_by_id,
        );
    }

    for requirement in requirements {
        let mut options: Vec<_> = jobs
            .iter()
            .filter(|job| {
                job.executable
                    && job
                        .covers_requirements
                        .contains(&requirement.requirement_id)
            })
            .collect();
        options.sort_by(|left, right| compare_jobs(left, right));
        if options.is_empty()
            || selections.iter().all(|selection| {
                selection
                    .covered_requirement_ids
                    .contains(&requirement.requirement_id)
            })
        {
            continue;
        }
        selections = expand_requirement_selections(
            selections,
            &requirement.requirement_id,
            &options,
            &jobs_by_id,
        );
    }

    let all_requirements: BTreeSet<_> = requirements
        .iter()
        .map(|requirement| requirement.requirement_id.clone())
        .collect();
    let all_constraints: BTreeSet<_> = constraints
        .iter()
        .map(|constraint| constraint.constraint_id.clone())
        .collect();
    for selection in &mut selections {
        dependency_closure(&mut selection.selected_job_ids, dependency_dag);
        selection.covered_requirement_ids = selection
            .selected_job_ids
            .iter()
            .filter_map(|job_id| jobs_by_id.get(job_id.as_str()))
            .flat_map(|job| job.covers_requirements.iter().cloned())
            .collect();
    }
    selections.sort_by(|left, right| compare_selections(left, right, &jobs_by_id));
    selections.dedup_by(|left, right| left.selected_job_ids == right.selected_job_ids);
    portfolio_truncated |= selections.len() > MAX_BINDING_SET_PLANS;
    selections.truncate(MAX_BINDING_SET_PLANS);

    let mut plans = Vec::new();
    for selection in &selections {
        let unresolved_requirement_ids: Vec<_> = all_requirements
            .difference(&selection.covered_requirement_ids)
            .cloned()
            .collect();
        let missing_constraints: Vec<_> = all_constraints
            .difference(&selection.discharged_constraint_ids)
            .cloned()
            .collect();
        let status = if unresolved_requirement_ids.is_empty()
            && missing_constraints.is_empty()
            && dependency_dag.acyclic
        {
            PlanStatus::Executable
        } else {
            PlanStatus::Unresolved
        };
        let selected_job_ids: Vec<_> = selection.selected_job_ids.iter().cloned().collect();
        let discharged_constraint_ids: Vec<_> = selection
            .discharged_constraint_ids
            .iter()
            .cloned()
            .collect();
        let execution_order: Vec<_> = dependency_dag
            .execution_order
            .iter()
            .filter(|job_id| selection.selected_job_ids.contains(*job_id))
            .cloned()
            .collect();
        plans.push(BindingSetPlan {
            binding_set_plan_id: binding_set_plan_id(
                route_id,
                &selected_job_ids,
                &discharged_constraint_ids,
            ),
            selected_job_ids,
            discharged_constraint_ids,
            execution_order,
            status,
            unresolved_requirement_ids,
        });
    }

    if plans.is_empty() {
        plans.push(BindingSetPlan {
            binding_set_plan_id: binding_set_plan_id(route_id, &[], &[]),
            selected_job_ids: Vec::new(),
            discharged_constraint_ids: Vec::new(),
            execution_order: Vec::new(),
            status: if requirements.is_empty() && constraints.is_empty() {
                PlanStatus::Executable
            } else {
                PlanStatus::Unresolved
            },
            unresolved_requirement_ids: all_requirements.iter().cloned().collect(),
        });
    }

    let any_executable = plans
        .iter()
        .any(|plan| plan.status == PlanStatus::Executable);
    let mut unresolved = Vec::new();
    if !any_executable {
        for requirement in requirements {
            unresolved.push(UnresolvedRequirement {
                requirement_id: requirement.requirement_id.clone(),
                reason: unresolved_reason(requirement, jobs),
                details: unresolved_details(requirement, jobs),
            });
        }
    }
    for constraint in constraints {
        if constraint.discharged_by_job_ids.is_empty() && !any_executable {
            for requirement_id in &constraint.members {
                if unresolved
                    .iter()
                    .all(|value| &value.requirement_id != requirement_id)
                {
                    unresolved.push(UnresolvedRequirement {
                        requirement_id: requirement_id.clone(),
                        reason: UnresolvedReason::CorrelationUnsatisfied,
                        details: format!(
                            "correlation constraint {} has no executable joint or threaded strategy",
                            constraint.constraint_id
                        ),
                    });
                }
            }
        }
    }
    unresolved.sort_by(|left, right| left.requirement_id.cmp(&right.requirement_id));
    (plans, unresolved, portfolio_truncated)
}

fn expand_selections(
    selections: Vec<PlanSelection>,
    options: &[&ProducerJob],
    constraint_id: Option<&str>,
    jobs_by_id: &BTreeMap<&str, &ProducerJob>,
) -> Vec<PlanSelection> {
    let mut expanded = Vec::new();
    for selection in selections {
        for job in options {
            let mut next = selection.clone();
            next.selected_job_ids.insert(job.job_id.clone());
            next.covered_requirement_ids
                .extend(job.covers_requirements.iter().cloned());
            if let Some(constraint_id) = constraint_id {
                next.discharged_constraint_ids
                    .insert(constraint_id.to_string());
            }
            expanded.push(next);
        }
    }
    rank_and_truncate(expanded, jobs_by_id)
}

fn expand_requirement_selections(
    selections: Vec<PlanSelection>,
    requirement_id: &str,
    options: &[&ProducerJob],
    jobs_by_id: &BTreeMap<&str, &ProducerJob>,
) -> Vec<PlanSelection> {
    let mut expanded = Vec::new();
    for selection in selections {
        if selection.covered_requirement_ids.contains(requirement_id) {
            expanded.push(selection);
            continue;
        }
        for job in options {
            let mut next = selection.clone();
            next.selected_job_ids.insert(job.job_id.clone());
            next.covered_requirement_ids
                .extend(job.covers_requirements.iter().cloned());
            expanded.push(next);
        }
    }
    rank_and_truncate(expanded, jobs_by_id)
}

fn rank_and_truncate(
    mut selections: Vec<PlanSelection>,
    jobs_by_id: &BTreeMap<&str, &ProducerJob>,
) -> Vec<PlanSelection> {
    selections.sort_by(|left, right| compare_selections(left, right, jobs_by_id));
    selections.dedup_by(|left, right| left.selected_job_ids == right.selected_job_ids);
    select_diverse_selections(selections, jobs_by_id, MAX_BINDING_SET_PLANS)
}

fn select_diverse_selections(
    selections: Vec<PlanSelection>,
    jobs_by_id: &BTreeMap<&str, &ProducerJob>,
    limit: usize,
) -> Vec<PlanSelection> {
    if selections.len() <= limit {
        return selections;
    }
    let mut selected = Vec::new();
    let mut selected_job_sets = BTreeSet::new();
    let mut seen_static_dimensions = BTreeSet::new();
    for selection in &selections {
        let static_signature: Vec<_> = selection
            .selected_job_ids
            .iter()
            .filter_map(|job_id| jobs_by_id.get(job_id.as_str()))
            .filter(|job| job.strategy == ProducerStrategy::StaticBinding)
            .flat_map(|job| job.static_bindings.iter().cloned())
            .collect();
        if !static_signature.is_empty()
            && seen_static_dimensions.insert(static_signature)
            && selected_job_sets.insert(selection.selected_job_ids.clone())
        {
            selected.push(selection.clone());
            if selected.len() == limit {
                return selected;
            }
        }
    }
    let mut seen_prefixes = BTreeSet::new();
    for selection in &selections {
        let entry_signature: Vec<_> = selection
            .selected_job_ids
            .iter()
            .filter_map(|job_id| jobs_by_id.get(job_id.as_str()))
            .map(|job| {
                job.witness
                    .edges
                    .iter()
                    .filter_map(|edge| edge.field_id.clone())
                    .take(2)
                    .collect::<Vec<_>>()
            })
            .collect();
        if seen_prefixes.insert(entry_signature)
            && selected_job_sets.insert(selection.selected_job_ids.clone())
        {
            selected.push(selection.clone());
            if selected.len() == limit {
                return selected;
            }
        }
    }
    for selection in selections {
        if selected_job_sets.insert(selection.selected_job_ids.clone()) {
            selected.push(selection);
            if selected.len() == limit {
                break;
            }
        }
    }
    selected.sort_by(|left, right| compare_selections(left, right, jobs_by_id));
    selected
}

fn compare_selections(
    left: &PlanSelection,
    right: &PlanSelection,
    jobs_by_id: &BTreeMap<&str, &ProducerJob>,
) -> Ordering {
    let left_rank: usize = left
        .selected_job_ids
        .iter()
        .filter_map(|job_id| jobs_by_id.get(job_id.as_str()))
        .map(|job| {
            job_rank(job) as usize * 1_000
                + job.producer_priority as usize * 100
                + job_access_rank(job) as usize * 10
                + job_binding_rank(job) as usize
                + job.witness.edges.len()
        })
        .sum();
    let right_rank: usize = right
        .selected_job_ids
        .iter()
        .filter_map(|job_id| jobs_by_id.get(job_id.as_str()))
        .map(|job| {
            job_rank(job) as usize * 1_000
                + job.producer_priority as usize * 100
                + job_access_rank(job) as usize * 10
                + job_binding_rank(job) as usize
                + job.witness.edges.len()
        })
        .sum();
    left_rank
        .cmp(&right_rank)
        .then_with(|| {
            left.selected_job_ids
                .len()
                .cmp(&right.selected_job_ids.len())
        })
        .then_with(|| left.selected_job_ids.cmp(&right.selected_job_ids))
}

fn unresolved_reason(
    requirement: &crate::contracts::SeedRequirement,
    jobs: &[ProducerJob],
) -> UnresolvedReason {
    if requirement
        .static_bindings
        .iter()
        .any(|binding| binding.class == StaticBindingClass::UnresolvedLiteral)
    {
        return UnresolvedReason::UnsupportedLiteral;
    }
    if requirement.producer_candidates.is_empty() {
        return UnresolvedReason::NoProducerField;
    }
    if jobs.iter().any(|job| {
        job.covers_requirements
            .contains(&requirement.requirement_id)
    }) {
        UnresolvedReason::RecursiveDependency
    } else {
        UnresolvedReason::NoProducerPath
    }
}

fn unresolved_details(
    requirement: &crate::contracts::SeedRequirement,
    jobs: &[ProducerJob],
) -> String {
    let unresolved: BTreeSet<_> = jobs
        .iter()
        .filter(|job| {
            job.covers_requirements
                .contains(&requirement.requirement_id)
        })
        .flat_map(|job| job.unresolved_arg_refs.iter().cloned())
        .collect();
    if unresolved.is_empty() {
        "no executable static binding or producer path".to_string()
    } else {
        format!(
            "producer paths require unresolved arguments: {}",
            unresolved.into_iter().collect::<Vec<_>>().join(", ")
        )
    }
}

fn compare_jobs(left: &ProducerJob, right: &ProducerJob) -> Ordering {
    job_rank(left)
        .cmp(&job_rank(right))
        .then_with(|| left.producer_priority.cmp(&right.producer_priority))
        .then_with(|| job_access_rank(left).cmp(&job_access_rank(right)))
        .then_with(|| job_binding_rank(left).cmp(&job_binding_rank(right)))
        .then_with(|| left.witness.edges.len().cmp(&right.witness.edges.len()))
        .then_with(|| left.job_id.cmp(&right.job_id))
}

fn job_binding_rank(job: &ProducerJob) -> u8 {
    job.static_bindings
        .iter()
        .map(|binding| match binding.class {
            StaticBindingClass::BoundedPagination | StaticBindingClass::SchemaDefault => 0,
            StaticBindingClass::SchemaEnumValue | StaticBindingClass::GeneratedBoolean => 1,
            StaticBindingClass::UnresolvedLiteral => 2,
        })
        .sum()
}

fn job_access_rank(job: &ProducerJob) -> u8 {
    match job.entry_field_id.as_deref() {
        Some("field:Query.currentUser") => 0,
        Some("field:Query.market") => 1,
        Some(_) => 2,
        None => 0,
    }
}

fn job_rank(job: &ProducerJob) -> u8 {
    match job.strategy {
        ProducerStrategy::JointCoRead => 0,
        ProducerStrategy::StaticBinding => 1,
        ProducerStrategy::Standalone if job.executable => 2,
        ProducerStrategy::ThreadedDependency => 3,
        ProducerStrategy::Standalone => 4,
    }
}
