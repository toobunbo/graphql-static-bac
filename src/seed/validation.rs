use std::collections::BTreeSet;

use thiserror::Error;

use crate::contracts::{PlanStatus, SeedPlansData};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SeedPlanValidationError {
    #[error("route map key differs from route ID: {0}")]
    RouteKeyMismatch(String),
    #[error("duplicate {kind} ID in route {route_id}: {id}")]
    DuplicateId {
        route_id: String,
        kind: &'static str,
        id: String,
    },
    #[error("route {route_id} references missing {kind} ID: {id}")]
    MissingReference {
        route_id: String,
        kind: &'static str,
        id: String,
    },
    #[error("route {0} dependency DAG node set differs from producer jobs")]
    DagNodeMismatch(String),
    #[error("route {0} executable binding plan uses a cyclic DAG")]
    ExecutableCyclicDag(String),
    #[error("route {route_id} executable binding plan has unresolved requirements: {plan_id}")]
    ExecutableUnresolved { route_id: String, plan_id: String },
    #[error("route {route_id} executable binding plan does not cover requirement {requirement_id}: {plan_id}")]
    ExecutableMissingCoverage {
        route_id: String,
        plan_id: String,
        requirement_id: String,
    },
    #[error("route {route_id} executable binding plan does not discharge constraint {constraint_id}: {plan_id}")]
    ExecutableMissingConstraint {
        route_id: String,
        plan_id: String,
        constraint_id: String,
    },
    #[error(
        "route {route_id} binding plan execution order omits selected job {job_id}: {plan_id}"
    )]
    MissingExecutionJob {
        route_id: String,
        plan_id: String,
        job_id: String,
    },
}

pub fn validate_seed_plans(data: &SeedPlansData) -> Result<(), SeedPlanValidationError> {
    for (route_key, route) in &data.routes {
        if route_key != &route.route_id {
            return Err(SeedPlanValidationError::RouteKeyMismatch(route_key.clone()));
        }

        let requirements = unique_ids(
            &route.route_id,
            "requirement",
            route
                .requirements
                .iter()
                .map(|value| value.requirement_id.as_str()),
        )?;
        let constraints = unique_ids(
            &route.route_id,
            "constraint",
            route
                .correlation_constraints
                .iter()
                .map(|value| value.constraint_id.as_str()),
        )?;
        let jobs = unique_ids(
            &route.route_id,
            "producer job",
            route
                .producer_jobs
                .iter()
                .map(|value| value.job_id.as_str()),
        )?;
        unique_ids(
            &route.route_id,
            "dependency",
            route
                .dependency_dag
                .edges
                .iter()
                .map(|value| value.dependency_id.as_str()),
        )?;
        unique_ids(
            &route.route_id,
            "binding-set plan",
            route
                .binding_set_plans
                .iter()
                .map(|value| value.binding_set_plan_id.as_str()),
        )?;

        for constraint in &route.correlation_constraints {
            require_all(
                &route.route_id,
                "requirement",
                &requirements,
                &constraint.members,
            )?;
            require(
                &route.route_id,
                "requirement",
                &requirements,
                &constraint.basis.selector_requirement_id,
            )?;
            require(
                &route.route_id,
                "requirement",
                &requirements,
                &constraint.basis.dependent_requirement_id,
            )?;
            require_all(
                &route.route_id,
                "producer job",
                &jobs,
                &constraint.discharged_by_job_ids,
            )?;
        }

        for job in &route.producer_jobs {
            require_all(
                &route.route_id,
                "requirement",
                &requirements,
                &job.covers_requirements,
            )?;
            for member in job.extraction.members.keys() {
                require(&route.route_id, "requirement", &requirements, member)?;
            }
        }

        let dag_nodes: BTreeSet<_> = route.dependency_dag.nodes.iter().cloned().collect();
        if dag_nodes != jobs {
            return Err(SeedPlanValidationError::DagNodeMismatch(
                route.route_id.clone(),
            ));
        }
        for edge in &route.dependency_dag.edges {
            require(&route.route_id, "producer job", &jobs, &edge.from_job_id)?;
            require(&route.route_id, "producer job", &jobs, &edge.to_job_id)?;
        }
        require_all(
            &route.route_id,
            "producer job",
            &jobs,
            &route.dependency_dag.execution_order,
        )?;

        for plan in &route.binding_set_plans {
            require_all(
                &route.route_id,
                "producer job",
                &jobs,
                &plan.selected_job_ids,
            )?;
            require_all(
                &route.route_id,
                "constraint",
                &constraints,
                &plan.discharged_constraint_ids,
            )?;
            require_all(
                &route.route_id,
                "requirement",
                &requirements,
                &plan.unresolved_requirement_ids,
            )?;
            if plan.status == PlanStatus::Executable {
                if !route.dependency_dag.acyclic {
                    return Err(SeedPlanValidationError::ExecutableCyclicDag(
                        route.route_id.clone(),
                    ));
                }
                if !plan.unresolved_requirement_ids.is_empty() {
                    return Err(SeedPlanValidationError::ExecutableUnresolved {
                        route_id: route.route_id.clone(),
                        plan_id: plan.binding_set_plan_id.clone(),
                    });
                }
                let covered: BTreeSet<_> = route
                    .producer_jobs
                    .iter()
                    .filter(|job| plan.selected_job_ids.contains(&job.job_id))
                    .flat_map(|job| job.covers_requirements.iter().cloned())
                    .collect();
                for requirement_id in &requirements {
                    if !covered.contains(requirement_id) {
                        return Err(SeedPlanValidationError::ExecutableMissingCoverage {
                            route_id: route.route_id.clone(),
                            plan_id: plan.binding_set_plan_id.clone(),
                            requirement_id: requirement_id.clone(),
                        });
                    }
                }
                for constraint_id in &constraints {
                    if !plan.discharged_constraint_ids.contains(constraint_id) {
                        return Err(SeedPlanValidationError::ExecutableMissingConstraint {
                            route_id: route.route_id.clone(),
                            plan_id: plan.binding_set_plan_id.clone(),
                            constraint_id: constraint_id.clone(),
                        });
                    }
                }
            }
            for job_id in &plan.selected_job_ids {
                if !plan.execution_order.contains(job_id) {
                    return Err(SeedPlanValidationError::MissingExecutionJob {
                        route_id: route.route_id.clone(),
                        plan_id: plan.binding_set_plan_id.clone(),
                        job_id: job_id.clone(),
                    });
                }
            }
        }
        for unresolved in &route.unresolved_requirements {
            require(
                &route.route_id,
                "requirement",
                &requirements,
                &unresolved.requirement_id,
            )?;
        }
    }
    Ok(())
}

fn unique_ids<'a>(
    route_id: &str,
    kind: &'static str,
    values: impl Iterator<Item = &'a str>,
) -> Result<BTreeSet<String>, SeedPlanValidationError> {
    let mut ids = BTreeSet::new();
    for value in values {
        if !ids.insert(value.to_string()) {
            return Err(SeedPlanValidationError::DuplicateId {
                route_id: route_id.to_string(),
                kind,
                id: value.to_string(),
            });
        }
    }
    Ok(ids)
}

fn require(
    route_id: &str,
    kind: &'static str,
    ids: &BTreeSet<String>,
    id: &str,
) -> Result<(), SeedPlanValidationError> {
    if !ids.contains(id) {
        return Err(SeedPlanValidationError::MissingReference {
            route_id: route_id.to_string(),
            kind,
            id: id.to_string(),
        });
    }
    Ok(())
}

fn require_all(
    route_id: &str,
    kind: &'static str,
    ids: &BTreeSet<String>,
    values: &[String],
) -> Result<(), SeedPlanValidationError> {
    for value in values {
        require(route_id, kind, ids, value)?;
    }
    Ok(())
}
