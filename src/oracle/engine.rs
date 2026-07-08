use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use thiserror::Error;

use crate::contracts::{
    Envelope, PlanStatus, PolicyCandidateAuthContext, PolicyClass, PolicyHypothesesData,
    PolicyHypothesis, PolicyOracleData, PolicyOracleExecution, PolicyOracleExecutionStatus,
    PolicyOracleFailure, PolicyOracleFailureCode, PolicyOracleOutcome, PolicyOraclePhase,
    PolicyOracleResult, PolicyVictim, PolicyViolationCandidate, PolicyViolationCandidates,
    PolicyViolationVerdict, Producer, Route, RouteSeedPlan, RoutesData, SchemaIrData,
    SeedPlansData, SeedRuntimeData, StageId, TypeKind, TypeWrapper, VerifiedBindingSet,
};
use crate::runtime::{
    build_request, emit_projected_operation, GraphqlHttpResponse, GraphqlTransport,
    RuntimeExecutionMode, RuntimeRequestProfile, RuntimeSelection, TransportError,
};

use super::ids::{policy_oracle_execution_id, policy_oracle_run_id};

pub const POLICY_ORACLE_MODEL: &str = "policy-violation-oracle/v1";
const IDENTITY_ALIAS: &str = "_policyIdentity";
const POLICY_ALIAS: &str = "_policyValue";

#[derive(Debug, Error)]
pub enum PolicyOracleError {
    #[error("policy oracle contract violation: {0}")]
    Contract(String),
    #[error("could not emit policy oracle query: {0}")]
    Emission(#[from] crate::runtime::ValidationEmissionError),
    #[error("could not build policy oracle request: {0}")]
    Request(#[from] crate::runtime::RequestAdapterError),
}

struct OracleEngine<'a, T> {
    schema: &'a Envelope<SchemaIrData>,
    owner_profile: &'a RuntimeRequestProfile,
    observer_profile: &'a RuntimeRequestProfile,
    transport: &'a T,
    executions: BTreeMap<String, PolicyOracleExecution>,
    owner_requests: usize,
    observer_requests: usize,
    owner_route_requests: BTreeMap<String, usize>,
    observer_route_requests: BTreeMap<String, usize>,
}

struct PolicyField<'a> {
    name: &'a str,
    target_type_name: &'a str,
    accepted_typenames: BTreeSet<String>,
}

struct ExtractedVictims {
    victims: Vec<PolicyVictim>,
    restricted_without_identity: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn run_policy_oracle<T: GraphqlTransport>(
    schema: &Envelope<SchemaIrData>,
    routes: &Envelope<RoutesData>,
    seed_plans: &Envelope<SeedPlansData>,
    owner_seeds: &Envelope<SeedRuntimeData>,
    hypotheses: &Envelope<PolicyHypothesesData>,
    owner_profile: &RuntimeRequestProfile,
    observer_profile: &RuntimeRequestProfile,
    selection: &RuntimeSelection,
    transport: &T,
) -> Result<(Envelope<PolicyOracleData>, PolicyViolationCandidates), PolicyOracleError> {
    validate_inputs(
        schema,
        routes,
        seed_plans,
        owner_seeds,
        hypotheses,
        owner_profile,
        observer_profile,
    )?;
    let mut engine = OracleEngine {
        schema,
        owner_profile,
        observer_profile,
        transport,
        executions: BTreeMap::new(),
        owner_requests: 0,
        observer_requests: 0,
        owner_route_requests: BTreeMap::new(),
        observer_route_requests: BTreeMap::new(),
    };
    let mut results = Vec::new();
    let mut candidates = Vec::new();
    let mut candidate_keys = BTreeSet::new();

    for hypothesis in &hypotheses.data.hypotheses {
        let policy_field = match resolve_policy_field(&schema.data, hypothesis) {
            Ok(field) => field,
            Err(failure) => {
                emit_hypothesis_failures(routes, selection, hypothesis, failure, &mut results);
                continue;
            }
        };
        let Some(target) = routes.data.targets.get(&hypothesis.type_id) else {
            continue;
        };
        for route in &target.routes {
            if !selected(route, selection) {
                continue;
            }
            let Some(owner_route) = owner_seeds.data.route_bindings.get(&route.route_id) else {
                results.push(inconclusive_result(
                    hypothesis,
                    route,
                    "",
                    failure(
                        PolicyOracleFailureCode::OwnerSeedUnavailable,
                        "owner seed runtime has no result for route",
                    ),
                ));
                continue;
            };
            let Some(seed_plan) = seed_plans.data.routes.get(&route.route_id) else {
                return Err(PolicyOracleError::Contract(format!(
                    "route {} is missing from S4",
                    route.route_id
                )));
            };
            if owner_route.verified_binding_sets.is_empty() {
                results.push(inconclusive_result(
                    hypothesis,
                    route,
                    "",
                    failure(
                        PolicyOracleFailureCode::OwnerSeedUnavailable,
                        "owner route has no verified binding set",
                    ),
                ));
                continue;
            }
            for binding_set in &owner_route.verified_binding_sets {
                let operation = match emit_policy_operation(
                    &schema.data,
                    route,
                    seed_plan,
                    binding_set,
                    &policy_field,
                ) {
                    Ok(operation) => operation,
                    Err(error) => {
                        results.push(inconclusive_result(
                            hypothesis,
                            route,
                            &binding_set.binding_set_id,
                            failure(
                                PolicyOracleFailureCode::RouteRebindingFailed,
                                error.to_string(),
                            ),
                        ));
                        continue;
                    }
                };
                let owner_execution = match engine.execute(
                    PolicyOraclePhase::OwnerCalibration,
                    hypothesis,
                    route,
                    binding_set,
                    &operation.operation_name,
                    &operation.operation,
                    &operation.variables,
                )? {
                    Some(execution) => execution,
                    None => {
                        results.push(inconclusive_result(
                            hypothesis,
                            route,
                            &binding_set.binding_set_id,
                            failure(
                                PolicyOracleFailureCode::RequestBudgetExhausted,
                                "owner calibration request budget exhausted",
                            ),
                        ));
                        continue;
                    }
                };
                let extracted = owner_execution
                    .response
                    .as_ref()
                    .map(|response| {
                        extract_restricted_victims(
                            response,
                            &policy_field.accepted_typenames,
                            &hypothesis.restricted_values,
                        )
                    })
                    .unwrap_or_else(|| ExtractedVictims {
                        victims: Vec::new(),
                        restricted_without_identity: false,
                    });
                if extracted.victims.is_empty() {
                    let (code, message) = if extracted.restricted_without_identity {
                        (
                            PolicyOracleFailureCode::TargetIdentityUnavailable,
                            "owner resolved a restricted target without a stable identity",
                        )
                    } else if owner_execution.status != PolicyOracleExecutionStatus::Succeeded {
                        (
                            PolicyOracleFailureCode::OwnerRouteFailed,
                            "owner calibration did not return usable target data",
                        )
                    } else {
                        (
                            PolicyOracleFailureCode::RestrictedObjectNotFound,
                            "owner response contained no target with a restricted policy value",
                        )
                    };
                    let mut result = inconclusive_result(
                        hypothesis,
                        route,
                        &binding_set.binding_set_id,
                        failure(code, message),
                    );
                    result.owner_execution_id = Some(owner_execution.execution_id.clone());
                    results.push(result);
                    continue;
                }

                let observer_execution = match engine.execute(
                    PolicyOraclePhase::ObserverReplay,
                    hypothesis,
                    route,
                    binding_set,
                    &operation.operation_name,
                    &operation.operation,
                    &operation.variables,
                )? {
                    Some(execution) => execution,
                    None => {
                        for victim in extracted.victims {
                            let mut result = inconclusive_result(
                                hypothesis,
                                route,
                                &binding_set.binding_set_id,
                                failure(
                                    PolicyOracleFailureCode::RequestBudgetExhausted,
                                    "observer replay request budget exhausted",
                                ),
                            );
                            result.victim = Some(victim);
                            result.owner_execution_id = Some(owner_execution.execution_id.clone());
                            results.push(result);
                        }
                        continue;
                    }
                };
                let observer_identities = observer_execution
                    .response
                    .as_ref()
                    .map(|response| {
                        extract_target_identities(response, &policy_field.accepted_typenames)
                    })
                    .unwrap_or_default();

                let mut candidate_observed = false;
                for victim in extracted.victims {
                    let observed = observer_identities
                        .iter()
                        .any(|identity| identity == &victim.target_identity);
                    let outcome = if observed {
                        PolicyOracleOutcome::PolicyViolationCandidate
                    } else if observer_execution.status == PolicyOracleExecutionStatus::Succeeded {
                        PolicyOracleOutcome::NoViolationObserved
                    } else {
                        PolicyOracleOutcome::Inconclusive
                    };
                    let result_failure = match outcome {
                        PolicyOracleOutcome::PolicyViolationCandidate => None,
                        PolicyOracleOutcome::NoViolationObserved => Some(failure(
                            PolicyOracleFailureCode::VictimIdentityNotObserved,
                            "observer response did not contain the calibrated victim identity",
                        )),
                        PolicyOracleOutcome::Inconclusive => {
                            observer_execution.failure.clone().or_else(|| {
                                Some(failure(
                                    PolicyOracleFailureCode::ObserverHttpError,
                                    "observer replay failed",
                                ))
                            })
                        }
                    };
                    results.push(PolicyOracleResult {
                        run_id: run_id(
                            hypothesis,
                            route,
                            &binding_set.binding_set_id,
                            Some(&victim.target_identity),
                            owner_profile,
                            observer_profile,
                        ),
                        hypothesis_id: hypothesis.hypothesis_id.clone(),
                        route_id: route.route_id.clone(),
                        binding_set_id: binding_set.binding_set_id.clone(),
                        outcome,
                        victim: Some(victim),
                        owner_execution_id: Some(owner_execution.execution_id.clone()),
                        observer_execution_id: Some(observer_execution.execution_id.clone()),
                        failure: result_failure,
                    });
                    if observed {
                        candidate_observed = true;
                        let response = observer_execution.response.clone().unwrap_or(Value::Null);
                        let candidate = PolicyViolationCandidate {
                            verdict: PolicyViolationVerdict::PolicyViolationCandidate,
                            type_name: policy_field.target_type_name.to_string(),
                            route_id: route.route_id.clone(),
                            auth_context: PolicyCandidateAuthContext {
                                owner: owner_profile.auth_context_id.clone(),
                                observer: observer_profile.auth_context_id.clone(),
                            },
                            response,
                        };
                        let key = serde_json::to_string(&candidate)
                            .expect("policy candidate must serialize");
                        if candidate_keys.insert(key) {
                            candidates.push(candidate);
                        }
                    }
                }
                if observer_profile.limits.execution_mode == RuntimeExecutionMode::FirstVerified
                    && candidate_observed
                {
                    break;
                }
            }
        }
    }
    results.sort_by(|left, right| {
        (
            &left.hypothesis_id,
            &left.route_id,
            &left.binding_set_id,
            &left.run_id,
        )
            .cmp(&(
                &right.hypothesis_id,
                &right.route_id,
                &right.binding_set_id,
                &right.run_id,
            ))
    });
    candidates.sort_by(|left, right| {
        (
            &left.type_name,
            &left.route_id,
            &left.auth_context.owner,
            &left.auth_context.observer,
        )
            .cmp(&(
                &right.type_name,
                &right.route_id,
                &right.auth_context.owner,
                &right.auth_context.observer,
            ))
    });
    let artifact = Envelope::complete(
        StageId::PolicyOracle,
        schema.schema_fingerprint.clone(),
        Producer::current(),
        Vec::new(),
        PolicyOracleData {
            oracle_model: POLICY_ORACLE_MODEL.to_string(),
            owner_request_profile_id: owner_profile.request_profile_id.clone(),
            owner_auth_context_id: owner_profile.auth_context_id.clone(),
            observer_request_profile_id: observer_profile.request_profile_id.clone(),
            observer_auth_context_id: observer_profile.auth_context_id.clone(),
            results,
            executions: engine.executions,
        },
    );
    Ok((artifact, PolicyViolationCandidates { candidates }))
}

impl<T: GraphqlTransport> OracleEngine<'_, T> {
    #[allow(clippy::too_many_arguments)]
    fn execute(
        &mut self,
        phase: PolicyOraclePhase,
        hypothesis: &PolicyHypothesis,
        route: &Route,
        binding_set: &VerifiedBindingSet,
        operation_name: &str,
        operation: &str,
        variables: &Value,
    ) -> Result<Option<PolicyOracleExecution>, PolicyOracleError> {
        let (profile, requests_used, route_requests) = match phase {
            PolicyOraclePhase::OwnerCalibration => (
                self.owner_profile,
                &mut self.owner_requests,
                &mut self.owner_route_requests,
            ),
            PolicyOraclePhase::ObserverReplay => (
                self.observer_profile,
                &mut self.observer_requests,
                &mut self.observer_route_requests,
            ),
        };
        let route_used = route_requests.entry(route.route_id.clone()).or_default();
        if *requests_used >= profile.limits.max_requests
            || *route_used >= profile.limits.max_requests_per_route
        {
            return Ok(None);
        }
        let execution_id = policy_oracle_execution_id(&(
            phase,
            &hypothesis.hypothesis_id,
            &route.route_id,
            &binding_set.binding_set_id,
            &profile.request.url,
            &self.schema.schema_fingerprint,
            &profile.auth_context_id,
            variables,
        ));
        if let Some(execution) = self.executions.get(&execution_id) {
            return Ok(Some(execution.clone()));
        }
        let request = build_request(profile, operation_name, operation, variables.clone())?;
        *requests_used += 1;
        *route_used += 1;
        let execution = execution_record(
            self.transport.execute(&request),
            execution_id,
            phase,
            hypothesis,
            route,
            binding_set,
            profile,
            request.body,
        );
        self.executions
            .insert(execution.execution_id.clone(), execution.clone());
        Ok(Some(execution))
    }
}

fn emit_policy_operation(
    schema: &SchemaIrData,
    route: &Route,
    seed_plan: &RouteSeedPlan,
    binding_set: &VerifiedBindingSet,
    policy_field: &PolicyField<'_>,
) -> Result<crate::runtime::ValidationOperation, crate::runtime::ValidationEmissionError> {
    if !seed_plan.binding_set_plans.iter().any(|plan| {
        plan.binding_set_plan_id == binding_set.source_binding_set_plan_id
            && plan.status == PlanStatus::Executable
    }) && !binding_set.source_binding_set_plan_id.is_empty()
    {
        return Err(crate::runtime::ValidationEmissionError::MissingBinding(
            binding_set.source_binding_set_plan_id.clone(),
        ));
    }
    emit_projected_operation(
        schema,
        route,
        &seed_plan.requirements,
        &binding_set.bindings,
        "PolicyOracle",
        &[
            "__typename".to_string(),
            format!("{IDENTITY_ALIAS}: id"),
            format!("{POLICY_ALIAS}: {}", policy_field.name),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn execution_record(
    response: Result<GraphqlHttpResponse, TransportError>,
    execution_id: String,
    phase: PolicyOraclePhase,
    hypothesis: &PolicyHypothesis,
    route: &Route,
    binding_set: &VerifiedBindingSet,
    profile: &RuntimeRequestProfile,
    request: Value,
) -> PolicyOracleExecution {
    match response {
        Ok(response) => {
            let has_errors = response
                .body
                .get("errors")
                .and_then(Value::as_array)
                .is_some_and(|errors| !errors.is_empty());
            let (status, failure) = if matches!(response.status_code, 401 | 403) {
                (
                    PolicyOracleExecutionStatus::HttpError,
                    Some(failure(
                        if phase == PolicyOraclePhase::ObserverReplay {
                            PolicyOracleFailureCode::ObserverAuthFailed
                        } else {
                            PolicyOracleFailureCode::OwnerRouteFailed
                        },
                        format!("HTTP {}", response.status_code),
                    )),
                )
            } else if !(200..300).contains(&response.status_code) {
                (
                    PolicyOracleExecutionStatus::HttpError,
                    Some(failure(
                        if phase == PolicyOraclePhase::ObserverReplay {
                            PolicyOracleFailureCode::ObserverHttpError
                        } else {
                            PolicyOracleFailureCode::OwnerRouteFailed
                        },
                        format!("HTTP {}", response.status_code),
                    )),
                )
            } else if has_errors {
                (
                    PolicyOracleExecutionStatus::GraphqlError,
                    Some(failure(
                        if phase == PolicyOraclePhase::ObserverReplay {
                            PolicyOracleFailureCode::ObserverGraphqlError
                        } else {
                            PolicyOracleFailureCode::OwnerRouteFailed
                        },
                        "GraphQL response contains errors",
                    )),
                )
            } else {
                (PolicyOracleExecutionStatus::Succeeded, None)
            };
            PolicyOracleExecution {
                execution_id,
                phase,
                hypothesis_id: hypothesis.hypothesis_id.clone(),
                route_id: route.route_id.clone(),
                binding_set_id: binding_set.binding_set_id.clone(),
                auth_context_id: profile.auth_context_id.clone(),
                request,
                http_status: Some(response.status_code),
                response: Some(response.body),
                status,
                failure,
            }
        }
        Err(error) => PolicyOracleExecution {
            execution_id,
            phase,
            hypothesis_id: hypothesis.hypothesis_id.clone(),
            route_id: route.route_id.clone(),
            binding_set_id: binding_set.binding_set_id.clone(),
            auth_context_id: profile.auth_context_id.clone(),
            request,
            http_status: None,
            response: None,
            status: PolicyOracleExecutionStatus::TransportError,
            failure: Some(failure(
                if phase == PolicyOraclePhase::ObserverReplay {
                    PolicyOracleFailureCode::ObserverHttpError
                } else {
                    PolicyOracleFailureCode::OwnerRouteFailed
                },
                error.to_string(),
            )),
        },
    }
}

fn resolve_policy_field<'a>(
    schema: &'a SchemaIrData,
    hypothesis: &'a PolicyHypothesis,
) -> Result<PolicyField<'a>, PolicyOracleFailure> {
    let type_name = hypothesis
        .type_id
        .strip_prefix("type:")
        .unwrap_or(&hypothesis.type_id);
    let definition = schema.types.get(type_name).ok_or_else(|| {
        failure(
            PolicyOracleFailureCode::PolicyFieldMissing,
            "policy target type is missing from S0",
        )
    })?;
    if !definition.fields.contains_key("id") {
        return Err(failure(
            PolicyOracleFailureCode::TargetIdentityUnavailable,
            "policy target type has no id field",
        ));
    }
    let field = definition
        .fields
        .values()
        .find(|field| field.field_id == hypothesis.field_id)
        .ok_or_else(|| {
            failure(
                PolicyOracleFailureCode::PolicyFieldMissing,
                "policy field is missing from the target type",
            )
        })?;
    if !matches!(
        field.return_type.named_kind,
        TypeKind::Scalar | TypeKind::Enum
    ) || field.return_type.wrappers.contains(&TypeWrapper::List)
        || field.arguments.iter().any(|argument| {
            argument.type_ref.wrappers.first() == Some(&TypeWrapper::NonNull)
                && argument.default_value.is_none()
        })
    {
        return Err(failure(
            PolicyOracleFailureCode::PolicyFieldTypeMismatch,
            "policy field must be a scalar or enum projection without required arguments",
        ));
    }
    let typed_values_match = match hypothesis.policy_class {
        PolicyClass::BooleanVisibility | PolicyClass::BooleanPrivacy => {
            field.return_type.named_type == "Boolean"
                && hypothesis.restricted_values.iter().all(Value::is_boolean)
        }
        PolicyClass::EnumVisibility | PolicyClass::EnumPrivacy | PolicyClass::EnumState => {
            field.return_type.named_kind == TypeKind::Enum
                && hypothesis.restricted_values.iter().all(Value::is_string)
        }
    };
    if !typed_values_match {
        return Err(failure(
            PolicyOracleFailureCode::PolicyFieldTypeMismatch,
            "restricted values do not match the classifier policy class and field type",
        ));
    }
    let mut accepted_typenames = BTreeSet::new();
    accepted_typenames.insert(type_name.to_string());
    accepted_typenames.extend(
        definition
            .possible_types
            .iter()
            .map(|type_id| type_id.strip_prefix("type:").unwrap_or(type_id).to_string()),
    );
    Ok(PolicyField {
        name: &field.name,
        target_type_name: type_name,
        accepted_typenames,
    })
}

fn extract_restricted_victims(
    response: &Value,
    accepted_typenames: &BTreeSet<String>,
    restricted_values: &[Value],
) -> ExtractedVictims {
    let mut victims = Vec::new();
    let mut seen = BTreeSet::new();
    let mut restricted_without_identity = false;
    visit_objects(response.get("data").unwrap_or(response), &mut |object| {
        let accepted = object
            .get("__typename")
            .and_then(Value::as_str)
            .is_some_and(|typename| accepted_typenames.contains(typename));
        let Some(policy_value) = object.get(POLICY_ALIAS) else {
            return;
        };
        if !accepted || !restricted_values.iter().any(|value| value == policy_value) {
            return;
        }
        let Some(identity) = object.get(IDENTITY_ALIAS).filter(|value| !value.is_null()) else {
            restricted_without_identity = true;
            return;
        };
        let key = serde_json::to_string(&(identity, policy_value))
            .expect("victim identity must serialize");
        if seen.insert(key) {
            victims.push(PolicyVictim {
                target_identity: identity.clone(),
                policy_value: policy_value.clone(),
            });
        }
    });
    ExtractedVictims {
        victims,
        restricted_without_identity,
    }
}

fn extract_target_identities(
    response: &Value,
    accepted_typenames: &BTreeSet<String>,
) -> Vec<Value> {
    let mut identities = Vec::new();
    let mut seen = BTreeSet::new();
    visit_objects(response.get("data").unwrap_or(response), &mut |object| {
        let accepted = object
            .get("__typename")
            .and_then(Value::as_str)
            .is_some_and(|typename| accepted_typenames.contains(typename));
        let Some(identity) = object.get(IDENTITY_ALIAS).filter(|value| !value.is_null()) else {
            return;
        };
        if accepted {
            let key = serde_json::to_string(identity).expect("identity must serialize");
            if seen.insert(key) {
                identities.push(identity.clone());
            }
        }
    });
    identities
}

fn visit_objects(value: &Value, visitor: &mut impl FnMut(&serde_json::Map<String, Value>)) {
    match value {
        Value::Object(object) => {
            visitor(object);
            for child in object.values() {
                visit_objects(child, visitor);
            }
        }
        Value::Array(values) => {
            for child in values {
                visit_objects(child, visitor);
            }
        }
        _ => {}
    }
}

fn validate_inputs(
    schema: &Envelope<SchemaIrData>,
    routes: &Envelope<RoutesData>,
    seed_plans: &Envelope<SeedPlansData>,
    owner_seeds: &Envelope<SeedRuntimeData>,
    hypotheses: &Envelope<PolicyHypothesesData>,
    owner_profile: &RuntimeRequestProfile,
    observer_profile: &RuntimeRequestProfile,
) -> Result<(), PolicyOracleError> {
    if schema.stage != StageId::S0
        || routes.stage != StageId::S3
        || seed_plans.stage != StageId::S4
        || owner_seeds.stage != StageId::SeedRuntime
        || hypotheses.stage != StageId::PolicyHypotheses
    {
        return Err(PolicyOracleError::Contract(
            "oracle requires S0, S3/v2, S4, seed_runtime, and policy_hypotheses artifacts"
                .to_string(),
        ));
    }
    if [
        &routes.schema_fingerprint,
        &seed_plans.schema_fingerprint,
        &owner_seeds.schema_fingerprint,
        &hypotheses.schema_fingerprint,
    ]
    .iter()
    .any(|fingerprint| *fingerprint != &schema.schema_fingerprint)
    {
        return Err(PolicyOracleError::Contract(
            "oracle input schema fingerprints differ".to_string(),
        ));
    }
    if owner_profile.auth_context_id.is_empty()
        || observer_profile.auth_context_id.is_empty()
        || owner_profile.auth_context_id == observer_profile.auth_context_id
    {
        return Err(PolicyOracleError::Contract(
            "owner and observer auth_context_id values must be non-empty and distinct".to_string(),
        ));
    }
    if owner_seeds.data.auth_context_id != owner_profile.auth_context_id {
        return Err(PolicyOracleError::Contract(
            "owner seed runtime auth context differs from owner request profile".to_string(),
        ));
    }
    if owner_seeds.data.request_profile_id != owner_profile.request_profile_id {
        return Err(PolicyOracleError::Contract(
            "owner seed runtime request profile differs from owner request profile".to_string(),
        ));
    }
    if hypotheses.data.hypotheses.is_empty() {
        return Err(PolicyOracleError::Contract(
            "policy hypotheses artifact is empty".to_string(),
        ));
    }
    let mut ids = BTreeSet::new();
    for hypothesis in &hypotheses.data.hypotheses {
        if hypothesis.hypothesis_id.is_empty()
            || hypothesis.restricted_values.is_empty()
            || !ids.insert(&hypothesis.hypothesis_id)
        {
            return Err(PolicyOracleError::Contract(
                "policy hypotheses require unique non-empty IDs and restricted values".to_string(),
            ));
        }
    }
    Ok(())
}

fn selected(route: &Route, selection: &RuntimeSelection) -> bool {
    (selection.verdicts.is_empty() || selection.verdicts.contains(&route.verdict))
        && (selection.route_ids.is_empty() || selection.route_ids.contains(&route.route_id))
}

fn emit_hypothesis_failures(
    routes: &Envelope<RoutesData>,
    selection: &RuntimeSelection,
    hypothesis: &PolicyHypothesis,
    failure: PolicyOracleFailure,
    results: &mut Vec<PolicyOracleResult>,
) {
    if let Some(target) = routes.data.targets.get(&hypothesis.type_id) {
        for route in &target.routes {
            if selected(route, selection) {
                results.push(inconclusive_result(hypothesis, route, "", failure.clone()));
            }
        }
    }
}

fn inconclusive_result(
    hypothesis: &PolicyHypothesis,
    route: &Route,
    binding_set_id: &str,
    failure: PolicyOracleFailure,
) -> PolicyOracleResult {
    PolicyOracleResult {
        run_id: policy_oracle_run_id(&(
            &hypothesis.hypothesis_id,
            &route.route_id,
            binding_set_id,
            &failure.code,
        )),
        hypothesis_id: hypothesis.hypothesis_id.clone(),
        route_id: route.route_id.clone(),
        binding_set_id: binding_set_id.to_string(),
        outcome: PolicyOracleOutcome::Inconclusive,
        victim: None,
        owner_execution_id: None,
        observer_execution_id: None,
        failure: Some(failure),
    }
}

fn run_id(
    hypothesis: &PolicyHypothesis,
    route: &Route,
    binding_set_id: &str,
    victim_identity: Option<&Value>,
    owner_profile: &RuntimeRequestProfile,
    observer_profile: &RuntimeRequestProfile,
) -> String {
    policy_oracle_run_id(&(
        &hypothesis.hypothesis_id,
        &route.route_id,
        binding_set_id,
        victim_identity,
        &owner_profile.auth_context_id,
        &observer_profile.auth_context_id,
    ))
}

fn failure(code: PolicyOracleFailureCode, message: impl Into<String>) -> PolicyOracleFailure {
    PolicyOracleFailure {
        code,
        message: message.into(),
    }
}
