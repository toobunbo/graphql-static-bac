use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::identifier::canonical_identifier;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgumentClassifierPolicy {
    pub model_version: String,
    pub exact_selector_names: Vec<String>,
    pub selector_suffix_tokens: Vec<String>,
    pub authz_modifier_prefixes: Vec<String>,
    pub definite_noise_names: Vec<String>,
    pub identity_scalar_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LoadedArgumentPolicy {
    pub policy: ArgumentClassifierPolicy,
    pub fingerprint: String,
    exact_selectors: BTreeSet<String>,
    selector_suffixes: BTreeSet<String>,
    authz_prefixes: BTreeSet<String>,
    definite_noise: BTreeSet<String>,
    identity_scalars: BTreeSet<String>,
}

#[derive(Debug, Error)]
pub enum ArgumentPolicyError {
    #[error("could not read argument classifier policy {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse argument classifier policy {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid argument classifier policy: {0}")]
    Contract(String),
}

impl LoadedArgumentPolicy {
    pub fn from_policy(
        policy: ArgumentClassifierPolicy,
        fingerprint: impl Into<String>,
    ) -> Result<Self, ArgumentPolicyError> {
        if policy.model_version != "argument-classifier-v1" {
            return Err(ArgumentPolicyError::Contract(format!(
                "unsupported model_version {}",
                policy.model_version
            )));
        }
        validate_entries("exact_selector_names", &policy.exact_selector_names)?;
        validate_entries("selector_suffix_tokens", &policy.selector_suffix_tokens)?;
        validate_entries("authz_modifier_prefixes", &policy.authz_modifier_prefixes)?;
        validate_entries("definite_noise_names", &policy.definite_noise_names)?;
        validate_entries("identity_scalar_names", &policy.identity_scalar_names)?;

        Ok(Self {
            exact_selectors: normalized_identifiers(&policy.exact_selector_names),
            selector_suffixes: normalized_identifiers(&policy.selector_suffix_tokens),
            authz_prefixes: normalized_identifiers(&policy.authz_modifier_prefixes),
            definite_noise: normalized_identifiers(&policy.definite_noise_names),
            identity_scalars: policy
                .identity_scalar_names
                .iter()
                .map(|value| value.to_ascii_lowercase())
                .collect(),
            policy,
            fingerprint: fingerprint.into(),
        })
    }

    pub(crate) fn is_exact_selector(&self, name: &str) -> bool {
        self.exact_selectors.contains(&canonical_identifier(name))
    }

    pub(crate) fn is_selector_suffix(&self, token: &str) -> bool {
        self.selector_suffixes
            .contains(&canonical_identifier(token))
    }

    pub(crate) fn is_authz_modifier(&self, name: &str) -> bool {
        let canonical = canonical_identifier(name);
        self.authz_prefixes
            .iter()
            .any(|prefix| canonical.starts_with(prefix))
    }

    pub(crate) fn is_definite_noise(&self, name: &str) -> bool {
        self.definite_noise.contains(&canonical_identifier(name))
    }

    pub(crate) fn is_identity_scalar(&self, name: &str) -> bool {
        self.identity_scalars.contains(&name.to_ascii_lowercase())
    }
}

pub fn default_argument_policy() -> LoadedArgumentPolicy {
    let bytes = crate::embedded::DEFAULT_LEXICON_JSON.as_bytes();
    let policy: ArgumentClassifierPolicy =
        serde_json::from_slice(bytes).expect("bundled lexicon is valid JSON");
    let fingerprint = format!("sha256:{:x}", Sha256::digest(bytes));
    LoadedArgumentPolicy::from_policy(policy, fingerprint)
        .expect("bundled lexicon satisfies all constraints")
}

pub fn read_argument_policy(path: &Path) -> Result<LoadedArgumentPolicy, ArgumentPolicyError> {
    let bytes = fs::read(path).map_err(|source| ArgumentPolicyError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let policy = serde_json::from_slice(&bytes).map_err(|source| ArgumentPolicyError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    LoadedArgumentPolicy::from_policy(policy, format!("sha256:{:x}", Sha256::digest(&bytes)))
}

fn validate_entries(name: &str, entries: &[String]) -> Result<(), ArgumentPolicyError> {
    if entries.iter().any(|entry| entry.trim().is_empty()) {
        return Err(ArgumentPolicyError::Contract(format!(
            "{name} must not contain empty values"
        )));
    }
    let normalized: Vec<_> = entries
        .iter()
        .map(|entry| canonical_identifier(entry))
        .collect();
    let unique: BTreeSet<_> = normalized.iter().collect();
    if unique.len() != normalized.len() {
        return Err(ArgumentPolicyError::Contract(format!(
            "{name} contains duplicate values"
        )));
    }
    Ok(())
}

fn normalized_identifiers(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| canonical_identifier(value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ArgumentClassifierPolicy, ArgumentPolicyError, LoadedArgumentPolicy};

    fn policy() -> ArgumentClassifierPolicy {
        ArgumentClassifierPolicy {
            model_version: "argument-classifier-v1".to_string(),
            exact_selector_names: vec!["assetId".to_string()],
            selector_suffix_tokens: vec!["id".to_string()],
            authz_modifier_prefixes: vec!["viewAs".to_string()],
            definite_noise_names: vec!["first".to_string()],
            identity_scalar_names: vec!["UUID".to_string()],
        }
    }

    #[test]
    fn normalizes_identifier_rules() {
        let loaded = LoadedArgumentPolicy::from_policy(policy(), "sha256:test").unwrap();
        assert!(loaded.is_exact_selector("asset_id"));
        assert!(loaded.is_selector_suffix("ID"));
        assert!(loaded.is_authz_modifier("viewAsUserId"));
        assert!(loaded.is_definite_noise("FIRST"));
        assert!(loaded.is_identity_scalar("uuid"));
    }

    #[test]
    fn rejects_duplicate_normalized_entries() {
        let mut value = policy();
        value.exact_selector_names.push("asset_id".to_string());
        assert!(matches!(
            LoadedArgumentPolicy::from_policy(value, "sha256:test"),
            Err(ArgumentPolicyError::Contract(_))
        ));
    }
}
