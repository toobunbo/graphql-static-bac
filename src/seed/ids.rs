use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contracts::{PathEdge, StaticBinding, TypeRef};

pub fn seed_requirement_id(
    consumer_arg_ref: &str,
    input_path: &[String],
    type_ref: &TypeRef,
) -> String {
    stable_id(
        "seed_req",
        &(
            consumer_arg_ref,
            input_path,
            &type_ref.display,
            &type_ref.named_type,
            type_ref.named_kind,
            &type_ref.wrappers,
        ),
    )
}

pub fn correlation_constraint_id(
    anchor_type_id: &str,
    members: &[String],
    dependent_field_id: &str,
) -> String {
    let mut members = members.to_vec();
    members.sort();
    members.dedup();
    stable_id(
        "seed_constraint",
        &(anchor_type_id, members, dependent_field_id),
    )
}

pub fn producer_job_id(
    strategy: &str,
    requirement_ids: &[String],
    producer_field_ids: &[String],
    edges: &[PathEdge],
    bindings: &[StaticBinding],
    unresolved_arg_refs: &[String],
) -> String {
    let mut requirement_ids = requirement_ids.to_vec();
    requirement_ids.sort();
    requirement_ids.dedup();
    let mut producer_field_ids = producer_field_ids.to_vec();
    producer_field_ids.sort();
    producer_field_ids.dedup();
    let mut bindings = bindings.to_vec();
    bindings.sort_by(|left, right| {
        (&left.arg_ref, &left.input_path, left.class, &left.value).cmp(&(
            &right.arg_ref,
            &right.input_path,
            right.class,
            &right.value,
        ))
    });
    let mut unresolved_arg_refs = unresolved_arg_refs.to_vec();
    unresolved_arg_refs.sort();
    unresolved_arg_refs.dedup();
    let edge_ids: Vec<_> = edges.iter().map(|edge| &edge.edge_id).collect();
    stable_id(
        "seed_job",
        &(
            strategy,
            requirement_ids,
            producer_field_ids,
            edge_ids,
            bindings,
            unresolved_arg_refs,
        ),
    )
}

pub fn dependency_id(
    from_job_id: &str,
    to_job_id: &str,
    output_field_id: &str,
    input_arg_ref: &str,
) -> String {
    stable_id(
        "seed_dependency",
        &(from_job_id, to_job_id, output_field_id, input_arg_ref),
    )
}

pub fn binding_set_plan_id(
    route_id: &str,
    selected_job_ids: &[String],
    discharged_constraint_ids: &[String],
) -> String {
    let mut selected_job_ids = selected_job_ids.to_vec();
    selected_job_ids.sort();
    selected_job_ids.dedup();
    let mut discharged_constraint_ids = discharged_constraint_ids.to_vec();
    discharged_constraint_ids.sort();
    discharged_constraint_ids.dedup();
    stable_id(
        "seed_binding_plan",
        &(route_id, selected_job_ids, discharged_constraint_ids),
    )
}

fn stable_id<T: Serialize>(namespace: &str, value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("stable ID input must serialize");
    let digest = Sha256::digest(bytes);
    format!("{namespace}:sha256:{digest:x}")
}
