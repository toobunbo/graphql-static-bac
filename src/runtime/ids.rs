use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contracts::RuntimeBinding;

pub fn execution_id<T: Serialize>(
    kind: &str,
    operation_id: &str,
    endpoint: &str,
    schema_fingerprint: &str,
    auth_context_id: &str,
    input_bindings: &T,
) -> String {
    stable_id(
        "seed_exec",
        &(
            kind,
            operation_id,
            endpoint,
            schema_fingerprint,
            auth_context_id,
            input_bindings,
        ),
    )
}

pub fn binding_set_id(
    route_id: &str,
    bindings: &std::collections::BTreeMap<String, RuntimeBinding>,
    execution_ids: &[String],
) -> String {
    let mut execution_ids = execution_ids.to_vec();
    execution_ids.sort();
    execution_ids.dedup();
    stable_id("seed_binding", &(route_id, bindings, execution_ids))
}

fn stable_id<T: Serialize>(namespace: &str, value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("stable ID input must serialize");
    let digest = Sha256::digest(bytes);
    format!("{namespace}:sha256:{digest:x}")
}
