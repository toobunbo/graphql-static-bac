use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::artifact::{read_artifact, ArtifactReadError};
use crate::contracts::{ArgumentsData, RouteVerdict, SchemaIrData, StageId};
use crate::route::{read_route_policy, RoutePolicyError};
use crate::runtime::RuntimeSelection;
use crate::stages::policy_oracle::{
    read_and_run_policy_oracle, write_policy_oracle, PolicyOracleStageError,
};
use crate::stages::s3_routes::{analyze_schema_target, write_routes, S3RouteError};
use crate::stages::s4_seed_plans::{read_and_run_seed_plans, write_seed_plans, S4SeedPlanError};
use crate::stages::seed_runtime::{
    read_and_run_seed_runtime, write_seed_runtime, SeedRuntimeStageError,
};

#[derive(Debug, Clone)]
pub struct PolicyTypePipelineOptions {
    pub schema_ir: PathBuf,
    pub arguments: PathBuf,
    pub route_policy: PathBuf,
    pub target: String,
    pub policy_hypotheses: PathBuf,
    pub owner_request: PathBuf,
    pub observer_request: PathBuf,
    pub output_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum PolicyTypePipelineError {
    #[error(transparent)]
    Read(#[from] ArtifactReadError),
    #[error(transparent)]
    Policy(#[from] RoutePolicyError),
    #[error(transparent)]
    Routes(#[from] S3RouteError),
    #[error(transparent)]
    SeedPlans(#[from] S4SeedPlanError),
    #[error(transparent)]
    SeedRuntime(#[from] SeedRuntimeStageError),
    #[error(transparent)]
    PolicyOracle(#[from] PolicyOracleStageError),
}

pub fn run_policy_type_pipeline(
    options: &PolicyTypePipelineOptions,
) -> Result<(), PolicyTypePipelineError> {
    let stem = safe_stem(&options.target);
    let routes_path = output(&options.output_dir, &format!("routes.{stem}.json"));
    let seed_plans_path = output(&options.output_dir, &format!("seed-plans.{stem}.json"));
    let owner_seeds_path = output(
        &options.output_dir,
        &format!("seed-runtime.owner.{stem}.json"),
    );
    let oracle_runtime_path = output(
        &options.output_dir,
        &format!("policy-oracle-runtime.{stem}.json"),
    );
    let candidates_path = output(
        &options.output_dir,
        &format!("policy_violation_candidates.{stem}.json"),
    );

    let schema = read_artifact::<SchemaIrData>(&options.schema_ir, StageId::S0)?;
    let arguments = read_artifact::<ArgumentsData>(&options.arguments, StageId::S2)?;
    let policy = read_route_policy(&options.route_policy)?;
    let routes = analyze_schema_target(&schema, &arguments, &policy, &options.target)?;
    write_routes(&routes, &routes_path)?;

    let seed_plans = read_and_run_seed_plans(&options.schema_ir, &options.arguments, &routes_path)?;
    write_seed_plans(&seed_plans, &seed_plans_path)?;

    let selection = RuntimeSelection {
        verdicts: BTreeSet::from([RouteVerdict::Open, RouteVerdict::Unknown]),
        route_ids: BTreeSet::new(),
    };
    let owner_seeds = read_and_run_seed_runtime(
        &options.schema_ir,
        &routes_path,
        &seed_plans_path,
        &options.owner_request,
        &selection,
    )?;
    write_seed_runtime(&owner_seeds, &owner_seeds_path)?;

    let (oracle_runtime, candidates) = read_and_run_policy_oracle(
        &options.schema_ir,
        &routes_path,
        &seed_plans_path,
        &owner_seeds_path,
        &options.policy_hypotheses,
        &options.owner_request,
        &options.observer_request,
        &selection,
    )?;
    write_policy_oracle(
        &oracle_runtime,
        &candidates,
        &oracle_runtime_path,
        &candidates_path,
    )?;
    Ok(())
}

fn safe_stem(target: &str) -> String {
    let value: String = target
        .strip_prefix("type:")
        .unwrap_or(target)
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if value.is_empty() {
        "target".to_string()
    } else {
        value
    }
}

fn output(directory: &Path, file_name: &str) -> PathBuf {
    directory.join(file_name)
}
