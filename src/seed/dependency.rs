use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::contracts::{
    CorrelationConstraint, DependencyDag, DependencyEdge, ProducerJob, ProducerStrategy,
    SeedRequirement,
};

use super::{dependency_id, producer_job_id};

pub(crate) fn build_dependencies(
    requirements: &[SeedRequirement],
    constraints: &mut [CorrelationConstraint],
    jobs: &mut Vec<ProducerJob>,
) -> DependencyDag {
    let requirements_by_arg: BTreeMap<_, _> = requirements
        .iter()
        .flat_map(|requirement| {
            [
                (
                    requirement.consumer_arg_ref.clone(),
                    requirement.requirement_id.clone(),
                ),
                (
                    requirement.root_arg_ref.clone(),
                    requirement.requirement_id.clone(),
                ),
            ]
        })
        .collect();
    let source_jobs = jobs.clone();
    let unresolved_jobs: Vec<_> = jobs
        .iter()
        .filter(|job| !job.unresolved_arg_refs.is_empty())
        .cloned()
        .collect();
    let mut new_jobs = Vec::new();
    let mut edges = Vec::new();

    for target in unresolved_jobs {
        let mut sources = Vec::new();
        let mut feasible = true;
        for arg_ref in &target.unresolved_arg_refs {
            let Some(requirement_id) = requirements_by_arg.get(arg_ref) else {
                feasible = false;
                break;
            };
            let source = source_jobs
                .iter()
                .filter(|job| {
                    job.executable
                        && job.unresolved_arg_refs.is_empty()
                        && job.covers_requirements.contains(requirement_id)
                        && !job.producer_field_ids.is_empty()
                })
                .min_by(|left, right| {
                    left.witness
                        .edges
                        .len()
                        .cmp(&right.witness.edges.len())
                        .then_with(|| left.job_id.cmp(&right.job_id))
                });
            let Some(source) = source else {
                feasible = false;
                break;
            };
            sources.push((source, arg_ref));
        }
        if !feasible || sources.is_empty() {
            continue;
        }

        let job_id = producer_job_id(
            "threaded_dependency",
            &target.covers_requirements,
            &target.producer_field_ids,
            &target.witness.edges,
            &target.static_bindings,
            &target.unresolved_arg_refs,
        );
        let mut threaded = target.clone();
        threaded.job_id = job_id.clone();
        threaded.strategy = ProducerStrategy::ThreadedDependency;
        threaded.executable = true;
        threaded.rejection_reasons.clear();

        for (source, arg_ref) in sources {
            if source.job_id == threaded.job_id {
                feasible = false;
                break;
            }
            let output_field_id = source.producer_field_ids[0].clone();
            edges.push(DependencyEdge {
                dependency_id: dependency_id(
                    &source.job_id,
                    &threaded.job_id,
                    &output_field_id,
                    arg_ref,
                ),
                from_job_id: source.job_id.clone(),
                to_job_id: threaded.job_id.clone(),
                output_field_id,
                input_arg_ref: arg_ref.clone(),
            });
        }
        if feasible {
            for constraint in constraints.iter_mut() {
                if threaded
                    .covers_requirements
                    .contains(&constraint.basis.dependent_requirement_id)
                    && edges.iter().any(|edge| {
                        edge.to_job_id == threaded.job_id
                            && source_jobs.iter().any(|source| {
                                source.job_id == edge.from_job_id
                                    && source
                                        .covers_requirements
                                        .contains(&constraint.basis.selector_requirement_id)
                            })
                    })
                {
                    constraint
                        .discharged_by_job_ids
                        .push(threaded.job_id.clone());
                }
            }
            new_jobs.push(threaded);
        }
    }

    jobs.extend(new_jobs);
    jobs.sort_by(|left, right| left.job_id.cmp(&right.job_id));
    jobs.dedup_by(|left, right| left.job_id == right.job_id);
    let job_ids: BTreeSet<_> = jobs.iter().map(|job| job.job_id.clone()).collect();
    edges.retain(|edge| job_ids.contains(&edge.from_job_id) && job_ids.contains(&edge.to_job_id));
    edges.sort_by(|left, right| left.dependency_id.cmp(&right.dependency_id));
    edges.dedup_by(|left, right| left.dependency_id == right.dependency_id);
    let (acyclic, execution_order) = topological_order(&job_ids, &edges);
    DependencyDag {
        nodes: job_ids.into_iter().collect(),
        edges,
        acyclic,
        execution_order,
    }
}

pub(crate) fn dependency_closure(selected_job_ids: &mut BTreeSet<String>, dag: &DependencyDag) {
    loop {
        let mut changed = false;
        for edge in &dag.edges {
            if selected_job_ids.contains(&edge.to_job_id)
                && selected_job_ids.insert(edge.from_job_id.clone())
            {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn topological_order(nodes: &BTreeSet<String>, edges: &[DependencyEdge]) -> (bool, Vec<String>) {
    let mut indegree: BTreeMap<_, usize> = nodes.iter().map(|node| (node.clone(), 0)).collect();
    let mut outgoing = BTreeMap::<String, Vec<String>>::new();
    for edge in edges {
        *indegree.entry(edge.to_job_id.clone()).or_default() += 1;
        outgoing
            .entry(edge.from_job_id.clone())
            .or_default()
            .push(edge.to_job_id.clone());
    }
    for values in outgoing.values_mut() {
        values.sort();
        values.dedup();
    }
    let mut queue: VecDeque<_> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(node, _)| node.clone())
        .collect();
    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.clone());
        if let Some(targets) = outgoing.get(&node) {
            for target in targets {
                let degree = indegree
                    .get_mut(target)
                    .expect("dependency target belongs to node set");
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(target.clone());
                }
            }
        }
    }
    (order.len() == nodes.len(), order)
}
