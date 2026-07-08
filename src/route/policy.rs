use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::contracts::BoundaryFamily;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePolicy {
    pub model_version: String,
    pub self_scope_root_tokens: Vec<String>,
    pub visibility_tokens: Vec<String>,
    pub exact_boundaries: BTreeMap<String, ExactBoundaryPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactBoundaryPolicy {
    pub family: BoundaryFamily,
    pub evidence: String,
}

#[derive(Debug, Clone)]
pub struct LoadedRoutePolicy {
    pub policy: RoutePolicy,
    pub fingerprint: String,
}

#[derive(Debug, Error)]
pub enum RoutePolicyError {
    #[error("could not read route policy {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse route policy {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid route policy: {0}")]
    Contract(String),
}

pub fn default_route_policy() -> LoadedRoutePolicy {
    let bytes = crate::embedded::DEFAULT_ROUTE_POLICY_JSON.as_bytes();
    let mut policy: RoutePolicy =
        serde_json::from_slice(bytes).expect("bundled route policy is valid JSON");
    policy.self_scope_root_tokens.sort();
    policy.self_scope_root_tokens.dedup();
    policy.visibility_tokens.sort();
    policy.visibility_tokens.dedup();
    LoadedRoutePolicy {
        policy,
        fingerprint: format!("sha256:{:x}", sha2::Sha256::digest(bytes)),
    }
}

pub fn read_route_policy(path: &Path) -> Result<LoadedRoutePolicy, RoutePolicyError> {
    let bytes = fs::read(path).map_err(|source| RoutePolicyError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut policy: RoutePolicy =
        serde_json::from_slice(&bytes).map_err(|source| RoutePolicyError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    if policy.model_version != "route-analysis-v1" {
        return Err(RoutePolicyError::Contract(format!(
            "unsupported model_version {}",
            policy.model_version
        )));
    }
    if policy
        .self_scope_root_tokens
        .iter()
        .chain(&policy.visibility_tokens)
        .any(|token| token.trim().is_empty())
    {
        return Err(RoutePolicyError::Contract(
            "boundary tokens must not be empty".to_string(),
        ));
    }
    if policy.exact_boundaries.iter().any(|(field_id, boundary)| {
        !field_id.starts_with("field:") || boundary.evidence.trim().is_empty()
    }) {
        return Err(RoutePolicyError::Contract(
            "exact boundaries require a field ID and non-empty evidence".to_string(),
        ));
    }
    policy.self_scope_root_tokens.sort();
    policy.self_scope_root_tokens.dedup();
    policy.visibility_tokens.sort();
    policy.visibility_tokens.dedup();
    Ok(LoadedRoutePolicy {
        policy,
        fingerprint: format!("sha256:{:x}", Sha256::digest(bytes)),
    })
}
