use crate::cli::{PolicyOracleArgs, RuntimeVerdictArg};
use crate::contracts::RouteVerdict;
use crate::runtime::RuntimeSelection;
use crate::stages::policy_oracle::{
    read_and_run_policy_oracle, write_policy_oracle, PolicyOracleStageError,
};

pub(crate) fn execute(args: PolicyOracleArgs) -> Result<(), PolicyOracleStageError> {
    let verdicts = args
        .verdicts
        .into_iter()
        .map(|verdict| match verdict {
            RuntimeVerdictArg::Open => RouteVerdict::Open,
            RuntimeVerdictArg::Unknown => RouteVerdict::Unknown,
            RuntimeVerdictArg::Guarded => RouteVerdict::Guarded,
        })
        .collect();
    let selection = RuntimeSelection {
        verdicts,
        route_ids: args.route_ids.into_iter().collect(),
    };
    let (artifact, candidates) = read_and_run_policy_oracle(
        &args.schema_ir,
        &args.routes,
        &args.seed_plans,
        &args.owner_seeds,
        &args.policy_hypotheses,
        &args.owner_request,
        &args.observer_request,
        &selection,
    )?;
    write_policy_oracle(
        &artifact,
        &candidates,
        &args.runtime_output,
        &args.candidates_output,
    )?;
    println!("Wrote {}", args.runtime_output.display());
    println!("Wrote {}", args.candidates_output.display());
    Ok(())
}
