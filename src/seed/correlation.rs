use std::collections::BTreeMap;

use crate::contracts::{
    AnchorInstanceRule, Cardinality, CorrelationBasis, CorrelationBasisKind, CorrelationConstraint,
    ExtractionAnchor, ExtractionMember, ExtractionPlan, ProducerJob, ProducerStrategy,
    ProducerWitness, Route, TypeWrapper,
};
use crate::graph::TypeGraph;

use super::{
    correlation_constraint_id,
    emitter::{emit_operation, ProjectionBranch, QueryEmissionError},
    ids::producer_job_id,
    index::SeedIndex,
    planner::{operation_name, producer_candidates_for_jobs},
    requirements::CollectedRequirement,
    search::{is_selector_arg, path_response_segments, search_producer_paths, ProducerPath},
};

pub(crate) fn derive_constraints(
    route: &Route,
    requirements: &[CollectedRequirement],
    index: &SeedIndex<'_>,
) -> Vec<CorrelationConstraint> {
    let mut constraints = BTreeMap::new();
    for selector in requirements {
        if has_executable_static_binding(selector)
            || selector.requirement.producer_candidates.is_empty()
        {
            continue;
        }
        if !is_selector_arg(
            index,
            &selector.requirement.root_arg_ref,
            &selector.requirement.input_path,
        ) {
            continue;
        }
        for dependent in requirements {
            if selector.requirement.requirement_id == dependent.requirement.requirement_id
                || selector.witness_edge_index >= dependent.witness_edge_index
                || has_executable_static_binding(dependent)
                || dependent.requirement.producer_candidates.is_empty()
            {
                continue;
            }
            if !lineage_connects(
                route,
                selector.witness_edge_index,
                dependent.witness_edge_index,
                &selector.requirement.selected_type_id,
            ) {
                continue;
            }
            let members = vec![
                selector.requirement.requirement_id.clone(),
                dependent.requirement.requirement_id.clone(),
            ];
            let constraint_id = correlation_constraint_id(
                &selector.requirement.selected_type_id,
                &members,
                &dependent.requirement.consumer_field_id,
            );
            constraints.insert(
                constraint_id.clone(),
                CorrelationConstraint {
                    constraint_id,
                    members,
                    anchor_type_id: selector.requirement.selected_type_id.clone(),
                    basis: CorrelationBasis {
                        kind: CorrelationBasisKind::RouteLineage,
                        selector_requirement_id: selector.requirement.requirement_id.clone(),
                        dependent_requirement_id: dependent.requirement.requirement_id.clone(),
                        dependent_field_id: dependent.requirement.consumer_field_id.clone(),
                    },
                    discharged_by_job_ids: Vec::new(),
                },
            );
        }
    }
    constraints.into_values().collect()
}

fn has_executable_static_binding(requirement: &CollectedRequirement) -> bool {
    requirement
        .requirement
        .static_bindings
        .iter()
        .any(|binding| binding.value.is_some())
}

pub(crate) fn build_joint_jobs(
    graph: &TypeGraph,
    index: &SeedIndex<'_>,
    requirements: &[CollectedRequirement],
    constraints: &mut [CorrelationConstraint],
    path_cache: &mut BTreeMap<(String, String), Vec<ProducerPath>>,
) -> Result<Vec<ProducerJob>, QueryEmissionError> {
    let by_id: BTreeMap<_, _> = requirements
        .iter()
        .map(|value| (value.requirement.requirement_id.as_str(), value))
        .collect();
    let mut jobs = Vec::new();

    for constraint in constraints {
        let Some(selector) = by_id.get(constraint.basis.selector_requirement_id.as_str()) else {
            continue;
        };
        let Some(dependent) = by_id.get(constraint.basis.dependent_requirement_id.as_str()) else {
            continue;
        };
        let Some(selector_candidate) = producer_candidates_for_jobs(&selector.requirement)
            .into_iter()
            .find(|candidate| {
                candidate.automatic
                    && candidate.producer_parent_type_id == constraint.anchor_type_id
            })
        else {
            continue;
        };
        let root_paths = path_cache
            .entry((
                graph.query_root_id.clone(),
                constraint.anchor_type_id.clone(),
            ))
            .or_insert_with(|| {
                search_producer_paths(
                    graph,
                    index,
                    &graph.query_root_id,
                    &constraint.anchor_type_id,
                )
            });
        let root_paths = executable_paths(root_paths, 8);
        if root_paths.is_empty() {
            continue;
        }

        let mut alternatives = Vec::new();
        for dependent_candidate in producer_candidates_for_jobs(&dependent.requirement) {
            let relative_paths = path_cache
                .entry((
                    constraint.anchor_type_id.clone(),
                    dependent_candidate.producer_parent_type_id.clone(),
                ))
                .or_insert_with(|| {
                    search_producer_paths(
                        graph,
                        index,
                        &constraint.anchor_type_id,
                        &dependent_candidate.producer_parent_type_id,
                    )
                });
            let relative_paths = executable_paths(relative_paths, 8);
            if relative_paths.is_empty() {
                continue;
            }
            for root_path in &root_paths {
                for relative_path in &relative_paths {
                    alternatives.push(joint_job(
                        index,
                        selector,
                        dependent,
                        selector_candidate,
                        dependent_candidate,
                        root_path,
                        relative_path,
                    )?);
                }
            }
        }
        alternatives.sort_by(|left, right| {
            left.witness
                .edges
                .len()
                .cmp(&right.witness.edges.len())
                .then_with(|| left.job_id.cmp(&right.job_id))
        });
        alternatives.dedup_by(|left, right| left.job_id == right.job_id);
        alternatives.truncate(8);
        for job in alternatives {
            constraint.discharged_by_job_ids.push(job.job_id.clone());
            jobs.push(job);
        }
    }
    Ok(jobs)
}

fn joint_job(
    index: &SeedIndex<'_>,
    selector: &CollectedRequirement,
    dependent: &CollectedRequirement,
    selector_candidate: &crate::contracts::ProducerCandidate,
    dependent_candidate: &crate::contracts::ProducerCandidate,
    root_path: &ProducerPath,
    relative_path: &ProducerPath,
) -> Result<ProducerJob, QueryEmissionError> {
    let requirement_ids = vec![
        selector.requirement.requirement_id.clone(),
        dependent.requirement.requirement_id.clone(),
    ];
    let producer_field_ids = vec![
        selector_candidate.producer_field_id.clone(),
        dependent_candidate.producer_field_id.clone(),
    ];
    let mut combined_edges = root_path.edges.clone();
    combined_edges.extend(relative_path.edges.clone());
    let mut bindings = root_path.static_bindings.clone();
    bindings.extend(relative_path.static_bindings.clone());
    bindings.sort();
    bindings.dedup();
    let job_id = producer_job_id(
        "joint_co_read",
        &requirement_ids,
        &producer_field_ids,
        &combined_edges,
        &bindings,
        &[],
    );
    let operation_name = operation_name(&job_id);
    let mut dependent_edges = root_path.edges.clone();
    dependent_edges.extend(relative_path.edges.clone());
    let operation = emit_operation(
        index,
        &operation_name,
        &[
            ProjectionBranch {
                edges: root_path.edges.clone(),
                terminal_field_id: selector_candidate.producer_field_id.clone(),
            },
            ProjectionBranch {
                edges: dependent_edges,
                terminal_field_id: dependent_candidate.producer_field_id.clone(),
            },
        ],
        &bindings,
        &[],
    )?;

    let anchor_segments = path_response_segments(index, &root_path.edges);
    let anchor_path = render_path(&anchor_segments);
    let selector_field = index
        .field(&selector_candidate.producer_field_id)
        .expect("producer candidate resolves against S0");
    let relative_segments = path_response_segments(index, &relative_path.edges);
    let dependent_field = index
        .field(&dependent_candidate.producer_field_id)
        .expect("producer candidate resolves against S0");
    let selector_relative = selector_field.field.name.clone();
    let mut dependent_relative_segments = relative_segments.clone();
    dependent_relative_segments.push((
        dependent_field.field.name.clone(),
        has_list(&dependent_field.field.return_type),
    ));
    let dependent_relative = render_path(&dependent_relative_segments);
    let selector_response = join_path(&anchor_path, &selector_relative);
    let dependent_response = join_path(&anchor_path, &dependent_relative);

    Ok(ProducerJob {
        job_id,
        strategy: ProducerStrategy::JointCoRead,
        producer_priority: 0,
        covers_requirements: requirement_ids,
        producer_field_ids: producer_field_ids.clone(),
        entry_field_id: root_path.entry_field_id.clone(),
        witness: ProducerWitness {
            edges: combined_edges,
            terminal_field_ids: producer_field_ids,
        },
        static_bindings: bindings,
        unresolved_arg_refs: Vec::new(),
        extraction: ExtractionPlan {
            anchor: Some(ExtractionAnchor {
                type_id: selector.requirement.selected_type_id.clone(),
                response_path: anchor_path,
                instance_rule: AnchorInstanceRule::NearestSharedInstance,
            }),
            members: BTreeMap::from([
                (
                    selector.requirement.requirement_id.clone(),
                    ExtractionMember {
                        response_path: selector_response,
                        relative_path: selector_relative,
                        cardinality: field_cardinality(&selector_field.field.return_type, false),
                    },
                ),
                (
                    dependent.requirement.requirement_id.clone(),
                    ExtractionMember {
                        response_path: dependent_response,
                        relative_path: dependent_relative,
                        cardinality: field_cardinality(
                            &dependent_field.field.return_type,
                            relative_segments.iter().any(|(_, list)| *list),
                        ),
                    },
                ),
            ]),
        },
        operation_name: Some(operation_name),
        operation: Some(operation),
        executable: true,
        rejection_reasons: Vec::new(),
    })
}

fn lineage_connects(
    route: &Route,
    selector_edge_index: usize,
    dependent_edge_index: usize,
    anchor_type_id: &str,
) -> bool {
    let edges = &route.witness.edges;
    if selector_edge_index >= edges.len() || dependent_edge_index >= edges.len() {
        return false;
    }
    let selector_edge = &edges[selector_edge_index];
    if selector_edge.target_type_id == anchor_type_id {
        return true;
    }
    edges
        .iter()
        .skip(selector_edge_index + 1)
        .take(dependent_edge_index.saturating_sub(selector_edge_index))
        .any(|edge| edge.target_type_id == anchor_type_id)
}

fn executable_paths(paths: &[ProducerPath], limit: usize) -> Vec<ProducerPath> {
    let mut paths: Vec<_> = paths
        .iter()
        .filter(|path| path.unresolved_arg_refs.is_empty())
        .cloned()
        .collect();
    paths.sort_by(|left, right| {
        left.edges.len().cmp(&right.edges.len()).then_with(|| {
            left.edges
                .iter()
                .map(|edge| edge.edge_id.as_str())
                .cmp(right.edges.iter().map(|edge| edge.edge_id.as_str()))
        })
    });
    paths.truncate(limit);
    paths
}

fn has_list(type_ref: &crate::contracts::TypeRef) -> bool {
    type_ref.wrappers.contains(&TypeWrapper::List)
}

fn field_cardinality(type_ref: &crate::contracts::TypeRef, parent_many: bool) -> Cardinality {
    if parent_many || has_list(type_ref) {
        Cardinality::Many
    } else if type_ref.wrappers.first() == Some(&TypeWrapper::NonNull) {
        Cardinality::One
    } else {
        Cardinality::Optional
    }
}

fn render_path(segments: &[(String, bool)]) -> String {
    segments
        .iter()
        .map(|(name, list)| {
            if *list {
                format!("{name}[]")
            } else {
                name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else if child.is_empty() {
        parent.to_string()
    } else {
        format!("{parent}.{child}")
    }
}
