use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn policy_hypothesis_id<T: Serialize>(value: &T) -> String {
    stable_id("policy_hypothesis", value)
}

pub fn policy_oracle_run_id<T: Serialize>(value: &T) -> String {
    stable_id("policy_oracle_run", value)
}

pub fn policy_oracle_execution_id<T: Serialize>(value: &T) -> String {
    stable_id("policy_oracle_execution", value)
}

fn stable_id<T: Serialize>(namespace: &str, value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("stable ID input must serialize");
    let digest = Sha256::digest(bytes);
    format!("{namespace}:sha256:{digest:x}")
}
