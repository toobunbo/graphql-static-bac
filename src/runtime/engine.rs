use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use thiserror::Error;

use crate::contracts::{
    AdapterAttempt, BindingSetPlan, BindingValidation, BindingValidationStatus, Envelope,
    ExecutionKind, ExecutionRecord, FailureCode, PlanStatus, Producer, ProducerJob,
    ProducerStrategy, Route, RouteRuntimeResult, RouteRuntimeStatus, RouteSeedPlan, RouteVerdict,
    RoutesData, RuntimeBinding, RuntimeCoverage, RuntimeExecutionStatus, RuntimeFact,
    RuntimeFailure, SchemaIrData, SeedPlansData, SeedRequirement, SeedRuntimeData, StageId,
    StaticBindingClass, TypeKind, TypeWrapper, VerifiedBindingSet,
};

use super::{
    adapter_candidates, binding_set_id, build_request, emit_validation_operation, execution_id,
    extract_joint, response_reaches_target, GraphqlTransport, RuntimeExecutionMode,
    RuntimeRequestProfile, TransportError,
};

pub const SEED_RUNTIME_MODEL: &str = "seed-runtime/v1";
const MAX_ADAPTER_COMBINATIONS: usize = 32;

#[derive(Debug, Clone, Default)]
pub struct RuntimeSelection {
    pub verdicts: BTreeSet<RouteVerdict>,
    pub route_ids: BTreeSet<String>,
}

#[derive(Debug, Error)]
pub enum SeedRuntimeError {
    #[error("runtime contract violation: {0}")]
    Contract(String),
    #[error("could not build request: {0}")]
    Request(#[from] super::RequestAdapterError),
    #[error("could not emit route validation query: {0}")]
    Validation(#[from] super::ValidationEmissionError),
}

#[derive(Debug, Clone)]
struct BindingRow {
    bindings: BTreeMap<String, RuntimeBinding>,
    producer_execution_ids: Vec<String>,
}

struct PlanExecutionContext<'a> {
    route: &'a Route,
    seed_plan: &'a RouteSeedPlan,
    plan: &'a BindingSetPlan,
    requirements: &'a BTreeMap<&'a str, &'a SeedRequirement>,
    request_limit: usize,
}

struct RouteExecutionBudget {
    request_limit: usize,
    producer_job_ids: BTreeSet<String>,
    producer_limit_reached: bool,
    values_truncated: bool,
    adapters_truncated: bool,
}

impl RouteExecutionBudget {
    fn claim_producer(&mut self, job_id: &str, limit: usize) -> bool {
        if self.producer_job_ids.contains(job_id) {
            return true;
        }
        if self.producer_job_ids.len() >= limit {
            self.producer_limit_reached = true;
            return false;
        }
        self.producer_job_ids.insert(job_id.to_string());
        true
    }

    fn bounded(&self) -> bool {
        self.producer_limit_reached || self.values_truncated || self.adapters_truncated
    }

    fn hard_exhausted(&self) -> bool {
        self.producer_limit_reached
    }
}

struct RuntimeEngine<'a, T> {
    schema: &'a Envelope<SchemaIrData>,
    profile: &'a RuntimeRequestProfile,
    transport: &'a T,
    executions: BTreeMap<String, ExecutionRecord>,
    requests_used: usize,
    runtime_facts: Vec<RuntimeFact>,
}

pub fn run_seed_runtime<T: GraphqlTransport>(
    schema: &Envelope<SchemaIrData>,
    routes: &Envelope<RoutesData>,
    seed_plans: &Envelope<SeedPlansData>,
    profile: &RuntimeRequestProfile,
    selection: &RuntimeSelection,
    transport: &T,
) -> Result<Envelope<SeedRuntimeData>, SeedRuntimeError> {
    validate_inputs(schema, routes, seed_plans, profile)?;
    let mut engine = RuntimeEngine {
        schema,
        profile,
        transport,
        executions: BTreeMap::new(),
        requests_used: 0,
        runtime_facts: Vec::new(),
    };
    let mut route_bindings = BTreeMap::new();
    for target in routes.data.targets.values() {
        for route in &target.routes {
            if !selected(route, selection) {
                continue;
            }
            let seed_plan = seed_plans.data.routes.get(&route.route_id).ok_or_else(|| {
                SeedRuntimeError::Contract(format!("route {} is missing from S4", route.route_id))
            })?;
            let result = engine.run_route(route, seed_plan)?;
            route_bindings.insert(route.route_id.clone(), result);
        }
    }
    Ok(Envelope::complete(
        StageId::SeedRuntime,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        SeedRuntimeData {
            runtime_model: SEED_RUNTIME_MODEL.to_string(),
            request_profile_id: profile.request_profile_id.clone(),
            auth_context_id: profile.auth_context_id.clone(),
            route_bindings,
            executions: engine.executions,
            runtime_facts: engine.runtime_facts,
        },
    ))
}

impl<T: GraphqlTransport> RuntimeEngine<'_, T> {
    fn run_route(
        &mut self,
        route: &Route,
        seed_plan: &RouteSeedPlan,
    ) -> Result<RouteRuntimeResult, SeedRuntimeError> {
        let mut result = RouteRuntimeResult {
            route_id: route.route_id.clone(),
            target_type_id: route.target_type_id.clone(),
            status: RouteRuntimeStatus::Unresolved,
            verified_binding_sets: Vec::new(),
            attempted_plan_ids: Vec::new(),
            failures: Vec::new(),
            coverage: if seed_plan.portfolio_truncated {
                RuntimeCoverage::Bounded
            } else {
                RuntimeCoverage::Complete
            },
        };
        let request_limit = self
            .requests_used
            .saturating_add(self.profile.limits.max_requests_per_route)
            .min(self.profile.limits.max_requests);
        let mut budget = RouteExecutionBudget {
            request_limit,
            producer_job_ids: BTreeSet::new(),
            producer_limit_reached: false,
            values_truncated: false,
            adapters_truncated: false,
        };

        if seed_plan.requirements.is_empty() {
            let row = BindingRow {
                bindings: BTreeMap::new(),
                producer_execution_ids: Vec::new(),
            };
            if let Some(verified) = self.validate_row(
                route,
                seed_plan,
                "",
                &row,
                &mut budget,
                &mut result.failures,
            )? {
                result.verified_binding_sets.push(verified);
                result.status = RouteRuntimeStatus::NoSeedRequired;
            } else if self.requests_used >= request_limit {
                result.status = RouteRuntimeStatus::BudgetExhausted;
                result.coverage = RuntimeCoverage::Bounded;
            }
            return Ok(result);
        }

        'plans: for plan in &seed_plan.binding_set_plans {
            if result.verified_binding_sets.len()
                >= self.profile.limits.max_verified_bindings_per_route
            {
                result.coverage = RuntimeCoverage::Bounded;
                break;
            }
            if self.requests_used >= request_limit {
                result.status = RouteRuntimeStatus::BudgetExhausted;
                result.failures.push(failure(
                    FailureCode::RequestBudgetExhausted,
                    "runtime request budget exhausted",
                    Some(&plan.binding_set_plan_id),
                    None,
                    None,
                ));
                result.coverage = RuntimeCoverage::Bounded;
                break;
            }
            if plan.status != PlanStatus::Executable {
                continue;
            }
            result
                .attempted_plan_ids
                .push(plan.binding_set_plan_id.clone());
            let rows =
                self.execute_plan(route, seed_plan, plan, &mut budget, &mut result.failures)?;
            for row in rows {
                if result.verified_binding_sets.len()
                    >= self.profile.limits.max_verified_bindings_per_route
                {
                    result.coverage = RuntimeCoverage::Bounded;
                    break;
                }
                if self.requests_used >= request_limit {
                    result.coverage = RuntimeCoverage::Bounded;
                    break;
                }
                if let Some(verified) = self.validate_row(
                    route,
                    seed_plan,
                    &plan.binding_set_plan_id,
                    &row,
                    &mut budget,
                    &mut result.failures,
                )? {
                    result.verified_binding_sets.push(verified);
                    if self.profile.limits.execution_mode == RuntimeExecutionMode::FirstVerified {
                        result.coverage = RuntimeCoverage::Bounded;
                        break 'plans;
                    }
                }
            }
            if self.requests_used >= request_limit {
                result.coverage = RuntimeCoverage::Bounded;
                if result.verified_binding_sets.is_empty() {
                    result.status = RouteRuntimeStatus::BudgetExhausted;
                }
                break;
            }
            if budget.hard_exhausted() {
                result.coverage = RuntimeCoverage::Bounded;
                if result.verified_binding_sets.is_empty() {
                    result.status = RouteRuntimeStatus::BudgetExhausted;
                }
                break;
            }
        }
        if budget.bounded() {
            result.coverage = RuntimeCoverage::Bounded;
        }
        if !result.verified_binding_sets.is_empty() {
            result.status = RouteRuntimeStatus::Verified;
        } else if result.status != RouteRuntimeStatus::BudgetExhausted {
            result.status = RouteRuntimeStatus::Unresolved;
        }
        Ok(result)
    }

    fn execute_plan(
        &mut self,
        route: &Route,
        seed_plan: &RouteSeedPlan,
        plan: &BindingSetPlan,
        budget: &mut RouteExecutionBudget,
        failures: &mut Vec<RuntimeFailure>,
    ) -> Result<Vec<BindingRow>, SeedRuntimeError> {
        let jobs: BTreeMap<_, _> = seed_plan
            .producer_jobs
            .iter()
            .map(|job| (job.job_id.as_str(), job))
            .collect();
        let requirements: BTreeMap<_, _> = seed_plan
            .requirements
            .iter()
            .map(|requirement| (requirement.requirement_id.as_str(), requirement))
            .collect();
        let mut rows = vec![BindingRow {
            bindings: BTreeMap::new(),
            producer_execution_ids: Vec::new(),
        }];
        for job_id in &plan.execution_order {
            let Some(job) = jobs.get(job_id.as_str()).copied() else {
                return Err(SeedRuntimeError::Contract(format!(
                    "plan {} references missing job {job_id}",
                    plan.binding_set_plan_id
                )));
            };
            if !plan.selected_job_ids.contains(job_id) {
                continue;
            }
            rows = if job.strategy == ProducerStrategy::StaticBinding {
                apply_static_job(job, &requirements, rows)?
            } else {
                if !budget.claim_producer(&job.job_id, self.profile.limits.max_producers_per_route)
                {
                    failures.push(failure(
                        FailureCode::CoverageIncomplete,
                        "producer path budget exhausted",
                        Some(&plan.binding_set_plan_id),
                        Some(&job.job_id),
                        None,
                    ));
                    break;
                }
                let context = PlanExecutionContext {
                    route,
                    seed_plan,
                    plan,
                    requirements: &requirements,
                    request_limit: budget.request_limit,
                };
                self.execute_producer_job(&context, job, rows, budget, failures)?
            };
            if rows.is_empty() {
                break;
            }
        }
        rows.truncate(self.profile.limits.max_values_per_requirement);
        Ok(rows)
    }

    fn execute_producer_job(
        &mut self,
        context: &PlanExecutionContext<'_>,
        job: &ProducerJob,
        rows: Vec<BindingRow>,
        budget: &mut RouteExecutionBudget,
        failures: &mut Vec<RuntimeFailure>,
    ) -> Result<Vec<BindingRow>, SeedRuntimeError> {
        let operation = job.operation.as_ref().ok_or_else(|| {
            SeedRuntimeError::Contract(format!("job {} has no operation", job.job_id))
        })?;
        let operation_name = job.operation_name.as_ref().ok_or_else(|| {
            SeedRuntimeError::Contract(format!("job {} has no operation name", job.job_id))
        })?;
        let mut output = Vec::new();
        let value_limit = self
            .profile
            .limits
            .max_values_per_producer
            .min(self.profile.limits.max_values_per_requirement);
        for row in rows {
            if output.len() >= value_limit {
                budget.values_truncated = true;
                break;
            }
            let Some(variables) = producer_variables(self.schema, context.seed_plan, job, &row)
            else {
                failures.push(failure(
                    FailureCode::DependencyUnresolved,
                    "producer dependency value is unavailable",
                    Some(&context.plan.binding_set_plan_id),
                    Some(&job.job_id),
                    None,
                ));
                continue;
            };
            let execution_id = execution_id(
                "producer",
                &job.job_id,
                &self.profile.request.url,
                &self.schema.schema_fingerprint,
                &self.profile.auth_context_id,
                &variables,
            );
            let record = if let Some(record) = self.executions.get(&execution_id) {
                record.clone()
            } else {
                if self.requests_used >= context.request_limit {
                    failures.push(failure(
                        FailureCode::RequestBudgetExhausted,
                        "runtime request budget exhausted",
                        Some(&context.plan.binding_set_plan_id),
                        Some(&job.job_id),
                        Some(&execution_id),
                    ));
                    break;
                }
                let request =
                    build_request(self.profile, operation_name, operation, variables.clone())?;
                self.requests_used += 1;
                let record = producer_record(
                    self.transport.execute(&request),
                    &execution_id,
                    context.route,
                    job,
                    request.body,
                );
                self.executions.insert(execution_id.clone(), record.clone());
                record
            };
            if record.status != RuntimeExecutionStatus::Extracted {
                failures.push(record.failure.clone().unwrap_or_else(|| {
                    failure(
                        FailureCode::ProducerExhausted,
                        "producer did not yield extractable data",
                        Some(&context.plan.binding_set_plan_id),
                        Some(&job.job_id),
                        Some(&execution_id),
                    )
                }));
                continue;
            }
            let response = record.response.as_ref().expect("extracted response exists");
            let data = response.get("data").unwrap_or(response);
            let extracted = match extract_joint(data, &job.extraction) {
                Ok(extracted) => extracted,
                Err(error) => {
                    let extraction_failure = failure(
                        FailureCode::ExtractionPathMissing,
                        error.to_string(),
                        Some(&context.plan.binding_set_plan_id),
                        Some(&job.job_id),
                        Some(&execution_id),
                    );
                    if let Some(stored) = self.executions.get_mut(&execution_id) {
                        stored.status = RuntimeExecutionStatus::ExtractionFailed;
                        stored.failure = Some(extraction_failure.clone());
                    }
                    failures.push(extraction_failure);
                    continue;
                }
            };
            let mut extracted_values = BTreeMap::<String, Vec<Value>>::new();
            for extracted_row in &extracted {
                for (requirement_id, extracted) in extracted_row {
                    extracted_values
                        .entry(requirement_id.clone())
                        .or_default()
                        .push(extracted.value.clone());
                }
            }
            if let Some(stored) = self.executions.get_mut(&execution_id) {
                stored.extracted_values = extracted_values;
            }
            let remaining = value_limit.saturating_sub(output.len());
            if extracted.len() > remaining {
                budget.values_truncated = true;
            }
            for extracted_row in extracted.into_iter().take(remaining) {
                let mut next = row.clone();
                next.producer_execution_ids.push(execution_id.clone());
                next.producer_execution_ids.sort();
                next.producer_execution_ids.dedup();
                for (requirement_id, extracted) in extracted_row {
                    let Some(requirement) = context.requirements.get(requirement_id.as_str())
                    else {
                        return Err(SeedRuntimeError::Contract(format!(
                            "job {} extracts unknown requirement {requirement_id}",
                            job.job_id
                        )));
                    };
                    next.bindings.insert(
                        requirement_id.clone(),
                        RuntimeBinding {
                            requirement_id,
                            input_path: requirement.input_path.clone(),
                            source_value: extracted.value.clone(),
                            consumer_value: extracted.value,
                            adapter_chain: vec!["identity".to_string()],
                            producer_job_id: job.job_id.clone(),
                            producer_execution_id: Some(execution_id.clone()),
                            extraction_provenance: vec![extracted.provenance],
                        },
                    );
                }
                output.push(next);
            }
        }
        Ok(dedup_rows(output))
    }

    fn validate_row(
        &mut self,
        route: &Route,
        seed_plan: &RouteSeedPlan,
        source_plan_id: &str,
        row: &BindingRow,
        budget: &mut RouteExecutionBudget,
        failures: &mut Vec<RuntimeFailure>,
    ) -> Result<Option<VerifiedBindingSet>, SeedRuntimeError> {
        let (combinations, truncated) = adapted_binding_rows(
            &seed_plan.requirements,
            &row.bindings,
            MAX_ADAPTER_COMBINATIONS.min(self.profile.limits.max_adapter_attempts_per_binding_set),
        );
        budget.adapters_truncated |= truncated;
        let mut attempts = Vec::new();
        let mut last_failure = None;
        for bindings in combinations {
            if self.requests_used >= budget.request_limit {
                return Ok(None);
            }
            let operation = emit_validation_operation(
                &self.schema.data,
                route,
                &seed_plan.requirements,
                &bindings,
            )?;
            let execution_id = execution_id(
                "validation",
                &route.route_id,
                &self.profile.request.url,
                &self.schema.schema_fingerprint,
                &self.profile.auth_context_id,
                &operation.variables,
            );
            let record = if let Some(record) = self.executions.get(&execution_id) {
                record.clone()
            } else {
                let request = build_request(
                    self.profile,
                    &operation.operation_name,
                    &operation.operation,
                    operation.variables.clone(),
                )?;
                self.requests_used += 1;
                let record = validation_record(
                    self.transport.execute(&request),
                    &execution_id,
                    route,
                    request.body,
                );
                self.executions.insert(execution_id.clone(), record.clone());
                record
            };
            let validation_status = match record.status {
                RuntimeExecutionStatus::Verified => BindingValidationStatus::Verified,
                RuntimeExecutionStatus::ConsumerRejected | RuntimeExecutionStatus::GraphqlError => {
                    BindingValidationStatus::ConsumerRejected
                }
                RuntimeExecutionStatus::TargetNotReached => {
                    BindingValidationStatus::TargetNotReached
                }
                _ => BindingValidationStatus::RuntimeError,
            };
            let chain = bindings
                .values()
                .flat_map(|binding| {
                    binding
                        .adapter_chain
                        .iter()
                        .map(move |adapter| format!("{}:{adapter}", binding.requirement_id))
                })
                .collect();
            attempts.push(AdapterAttempt {
                adapter_chain: chain,
                status: validation_status,
            });
            if validation_status != BindingValidationStatus::Verified {
                last_failure = record.failure.clone();
                continue;
            }
            let binding_set_id =
                binding_set_id(&route.route_id, &bindings, &row.producer_execution_ids);
            self.runtime_facts.push(RuntimeFact {
                kind: "route_target_reached".to_string(),
                subject: route.route_id.clone(),
                value: binding_set_id.clone(),
            });
            return Ok(Some(VerifiedBindingSet {
                binding_set_id,
                source_binding_set_plan_id: source_plan_id.to_string(),
                bindings,
                producer_execution_ids: row.producer_execution_ids.clone(),
                validation_execution_id: execution_id,
                validation: BindingValidation {
                    status: BindingValidationStatus::Verified,
                    target_type_id: route.target_type_id.clone(),
                    resolved_typename: Some(
                        route
                            .target_type_id
                            .strip_prefix("type:")
                            .unwrap_or(&route.target_type_id)
                            .to_string(),
                    ),
                    adapter_attempts: attempts,
                },
            }));
        }
        if let Some(failure) = last_failure {
            failures.push(failure);
        }
        Ok(None)
    }
}

fn selected(route: &Route, selection: &RuntimeSelection) -> bool {
    (selection.verdicts.is_empty() || selection.verdicts.contains(&route.verdict))
        && (selection.route_ids.is_empty() || selection.route_ids.contains(&route.route_id))
}

fn validate_inputs(
    schema: &Envelope<SchemaIrData>,
    routes: &Envelope<RoutesData>,
    seed_plans: &Envelope<SeedPlansData>,
    profile: &RuntimeRequestProfile,
) -> Result<(), SeedRuntimeError> {
    if schema.stage != StageId::S0 || routes.stage != StageId::S3 || seed_plans.stage != StageId::S4
    {
        return Err(SeedRuntimeError::Contract(
            "runtime requires S0, S3/v2, and S4 artifacts".to_string(),
        ));
    }
    if schema.schema_fingerprint != routes.schema_fingerprint
        || schema.schema_fingerprint != seed_plans.schema_fingerprint
    {
        return Err(SeedRuntimeError::Contract(
            "runtime input schema fingerprints differ".to_string(),
        ));
    }
    if profile.request_profile_id.is_empty() || profile.auth_context_id.is_empty() {
        return Err(SeedRuntimeError::Contract(
            "request_profile_id and auth_context_id must be non-empty".to_string(),
        ));
    }
    if profile.limits.max_requests == 0
        || profile.limits.max_producers_per_route == 0
        || profile.limits.max_values_per_producer == 0
        || profile.limits.max_adapter_attempts_per_binding_set == 0
        || profile.limits.max_values_per_requirement == 0
        || profile.limits.max_requests_per_route == 0
        || profile.limits.max_verified_bindings_per_route == 0
    {
        return Err(SeedRuntimeError::Contract(
            "runtime limits must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

fn apply_static_job(
    job: &ProducerJob,
    requirements: &BTreeMap<&str, &SeedRequirement>,
    rows: Vec<BindingRow>,
) -> Result<Vec<BindingRow>, SeedRuntimeError> {
    let mut output = rows;
    for requirement_id in &job.covers_requirements {
        let requirement = requirements.get(requirement_id.as_str()).ok_or_else(|| {
            SeedRuntimeError::Contract(format!(
                "static job {} covers unknown requirement {requirement_id}",
                job.job_id
            ))
        })?;
        let binding = job
            .static_bindings
            .iter()
            .find(|binding| {
                binding.arg_ref == requirement.root_arg_ref
                    && binding.input_path == requirement.input_path
                    && binding.class != StaticBindingClass::UnresolvedLiteral
            })
            .and_then(|binding| binding.value.as_ref())
            .ok_or_else(|| {
                SeedRuntimeError::Contract(format!(
                    "static job {} has no value for {requirement_id}",
                    job.job_id
                ))
            })?;
        let value = parse_static_value(binding, requirement);
        for row in &mut output {
            row.bindings.insert(
                requirement_id.clone(),
                RuntimeBinding {
                    requirement_id: requirement_id.clone(),
                    input_path: requirement.input_path.clone(),
                    source_value: value.clone(),
                    consumer_value: value.clone(),
                    adapter_chain: vec!["static_binding".to_string()],
                    producer_job_id: job.job_id.clone(),
                    producer_execution_id: None,
                    extraction_provenance: Vec::new(),
                },
            );
        }
    }
    Ok(output)
}

fn parse_static_value(value: &str, requirement: &SeedRequirement) -> Value {
    match requirement.type_ref.named_kind {
        TypeKind::Enum | TypeKind::Scalar
            if matches!(requirement.type_ref.named_type.as_str(), "ID" | "String") =>
        {
            Value::String(value.trim_matches('"').to_string())
        }
        TypeKind::Enum => Value::String(value.to_string()),
        TypeKind::Scalar if requirement.type_ref.named_type == "Boolean" => {
            Value::Bool(value == "true")
        }
        TypeKind::Scalar if requirement.type_ref.named_type == "Int" => value
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(value.to_string())),
        TypeKind::Scalar if requirement.type_ref.named_type == "Float" => value
            .parse::<f64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(value.to_string())),
        _ => serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string())),
    }
}

fn producer_variables(
    schema: &Envelope<SchemaIrData>,
    seed_plan: &RouteSeedPlan,
    job: &ProducerJob,
    row: &BindingRow,
) -> Option<Value> {
    let mut values = Map::new();
    let mut refs = job.unresolved_arg_refs.clone();
    refs.sort();
    for (position, arg_ref) in refs.iter().enumerate() {
        let edge = seed_plan
            .dependency_dag
            .edges
            .iter()
            .find(|edge| edge.to_job_id == job.job_id && edge.input_arg_ref == *arg_ref)?;
        let source_job = seed_plan
            .producer_jobs
            .iter()
            .find(|source| source.job_id == edge.from_job_id)?;
        let binding = source_job
            .covers_requirements
            .iter()
            .find_map(|requirement_id| row.bindings.get(requirement_id))?;
        let argument = schema
            .data
            .types
            .values()
            .flat_map(|definition| definition.fields.values())
            .flat_map(|field| field.arguments.iter())
            .find(|argument| argument.arg_id == *arg_ref)?;
        let consumer_is_list = argument.type_ref.wrappers.contains(&TypeWrapper::List);
        let candidate = adapter_candidates(&binding.consumer_value, consumer_is_list)
            .into_iter()
            .find(|candidate| candidate.value.is_array() == consumer_is_list)
            .unwrap_or_else(|| super::AdapterCandidate {
                value: binding.consumer_value.clone(),
                chain: vec!["identity".to_string()],
            });
        values.insert(format!("seed{}", position + 1), candidate.value);
    }
    Some(Value::Object(values))
}

fn producer_record(
    response: Result<super::GraphqlHttpResponse, TransportError>,
    execution_id: &str,
    route: &Route,
    job: &ProducerJob,
    request: Value,
) -> ExecutionRecord {
    match response {
        Err(error) => ExecutionRecord {
            execution_id: execution_id.to_string(),
            kind: ExecutionKind::Producer,
            job_id: Some(job.job_id.clone()),
            route_id: route.route_id.clone(),
            request,
            http_status: None,
            response: None,
            status: RuntimeExecutionStatus::HttpError,
            extracted_values: BTreeMap::new(),
            failure: Some(failure(
                FailureCode::ProducerHttpError,
                error.to_string(),
                None,
                Some(&job.job_id),
                Some(execution_id),
            )),
        },
        Ok(response) if !(200..300).contains(&response.status_code) => ExecutionRecord {
            execution_id: execution_id.to_string(),
            kind: ExecutionKind::Producer,
            job_id: Some(job.job_id.clone()),
            route_id: route.route_id.clone(),
            request,
            http_status: Some(response.status_code),
            response: Some(response.body),
            status: RuntimeExecutionStatus::HttpError,
            extracted_values: BTreeMap::new(),
            failure: Some(failure(
                FailureCode::ProducerHttpError,
                "producer returned a non-success HTTP status",
                None,
                Some(&job.job_id),
                Some(execution_id),
            )),
        },
        Ok(response) if has_graphql_errors(&response.body) => ExecutionRecord {
            execution_id: execution_id.to_string(),
            kind: ExecutionKind::Producer,
            job_id: Some(job.job_id.clone()),
            route_id: route.route_id.clone(),
            request,
            http_status: Some(response.status_code),
            response: Some(response.body),
            status: RuntimeExecutionStatus::GraphqlError,
            extracted_values: BTreeMap::new(),
            failure: Some(failure(
                FailureCode::ProducerGraphqlError,
                "producer returned GraphQL errors",
                None,
                Some(&job.job_id),
                Some(execution_id),
            )),
        },
        Ok(response) => ExecutionRecord {
            execution_id: execution_id.to_string(),
            kind: ExecutionKind::Producer,
            job_id: Some(job.job_id.clone()),
            route_id: route.route_id.clone(),
            request,
            http_status: Some(response.status_code),
            response: Some(response.body),
            status: RuntimeExecutionStatus::Extracted,
            extracted_values: BTreeMap::new(),
            failure: None,
        },
    }
}

fn validation_record(
    response: Result<super::GraphqlHttpResponse, TransportError>,
    execution_id: &str,
    route: &Route,
    request: Value,
) -> ExecutionRecord {
    match response {
        Err(error) => ExecutionRecord {
            execution_id: execution_id.to_string(),
            kind: ExecutionKind::Validation,
            job_id: None,
            route_id: route.route_id.clone(),
            request,
            http_status: None,
            response: None,
            status: RuntimeExecutionStatus::HttpError,
            extracted_values: BTreeMap::new(),
            failure: Some(failure(
                FailureCode::ProducerHttpError,
                error.to_string(),
                None,
                None,
                Some(execution_id),
            )),
        },
        Ok(response) if !(200..300).contains(&response.status_code) => ExecutionRecord {
            execution_id: execution_id.to_string(),
            kind: ExecutionKind::Validation,
            job_id: None,
            route_id: route.route_id.clone(),
            request,
            http_status: Some(response.status_code),
            response: Some(response.body),
            status: RuntimeExecutionStatus::HttpError,
            extracted_values: BTreeMap::new(),
            failure: Some(failure(
                FailureCode::ConsumerRejected,
                "validation returned a non-success HTTP status",
                None,
                None,
                Some(execution_id),
            )),
        },
        Ok(response) if has_graphql_errors(&response.body) => ExecutionRecord {
            execution_id: execution_id.to_string(),
            kind: ExecutionKind::Validation,
            job_id: None,
            route_id: route.route_id.clone(),
            request,
            http_status: Some(response.status_code),
            response: Some(response.body),
            status: RuntimeExecutionStatus::ConsumerRejected,
            extracted_values: BTreeMap::new(),
            failure: Some(failure(
                FailureCode::ConsumerRejected,
                "validation returned GraphQL errors",
                None,
                None,
                Some(execution_id),
            )),
        },
        Ok(response) => {
            let reached = response_reaches_target(&response.body, &route.target_type_id);
            ExecutionRecord {
                execution_id: execution_id.to_string(),
                kind: ExecutionKind::Validation,
                job_id: None,
                route_id: route.route_id.clone(),
                request,
                http_status: Some(response.status_code),
                response: Some(response.body),
                status: if reached {
                    RuntimeExecutionStatus::Verified
                } else {
                    RuntimeExecutionStatus::TargetNotReached
                },
                extracted_values: BTreeMap::new(),
                failure: if reached {
                    None
                } else {
                    Some(failure(
                        FailureCode::TargetNotReached,
                        "validation completed but did not resolve the target type",
                        None,
                        None,
                        Some(execution_id),
                    ))
                },
            }
        }
    }
}

fn adapted_binding_rows(
    requirements: &[SeedRequirement],
    bindings: &BTreeMap<String, RuntimeBinding>,
    limit: usize,
) -> (Vec<BTreeMap<String, RuntimeBinding>>, bool) {
    let mut rows = vec![BTreeMap::new()];
    let mut truncated = false;
    for requirement in requirements {
        let Some(binding) = bindings.get(&requirement.requirement_id) else {
            return (Vec::new(), false);
        };
        let is_list = requirement.type_ref.wrappers.contains(&TypeWrapper::List);
        let candidates = adapter_candidates(&binding.source_value, is_list);
        let mut expanded = Vec::new();
        'rows: for row in rows {
            for candidate in &candidates {
                if expanded.len() >= limit {
                    truncated = true;
                    break 'rows;
                }
                let mut next = row.clone();
                let mut adapted = binding.clone();
                adapted.consumer_value = candidate.value.clone();
                adapted.adapter_chain = candidate.chain.clone();
                next.insert(requirement.requirement_id.clone(), adapted);
                expanded.push(next);
            }
        }
        rows = expanded;
    }
    (rows, truncated)
}

fn dedup_rows(rows: Vec<BindingRow>) -> Vec<BindingRow> {
    let mut seen = BTreeSet::new();
    rows.into_iter()
        .filter(|row| {
            let values: BTreeMap<_, _> = row
                .bindings
                .iter()
                .map(|(id, binding)| (id, &binding.source_value))
                .collect();
            seen.insert(serde_json::to_string(&values).expect("binding values serialize"))
        })
        .collect()
}

fn has_graphql_errors(body: &Value) -> bool {
    body.get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
}

fn failure(
    code: FailureCode,
    message: impl Into<String>,
    plan_id: Option<&str>,
    job_id: Option<&str>,
    execution_id: Option<&str>,
) -> RuntimeFailure {
    RuntimeFailure {
        code,
        message: message.into(),
        execution_id: execution_id.map(str::to_string),
        plan_id: plan_id.map(str::to_string),
        job_id: job_id.map(str::to_string),
    }
}
