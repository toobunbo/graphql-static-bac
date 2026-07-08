use std::collections::BTreeSet;

use crate::cli::{RuntimeSeedArgs, RuntimeVerdictArg};
use crate::contracts::RouteVerdict;
use crate::runtime::RuntimeSelection;
use crate::stages::seed_runtime::{
    read_and_run_seed_runtime, write_seed_runtime, SeedRuntimeStageError,
};

pub(crate) fn execute(args: RuntimeSeedArgs) -> Result<(), SeedRuntimeStageError> {
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
        route_ids: args.route_ids.into_iter().collect::<BTreeSet<_>>(),
    };
    let artifact = read_and_run_seed_runtime(
        &args.schema_ir,
        &args.routes,
        &args.seed_plans,
        &args.request_template,
        &selection,
    )?;
    write_seed_runtime(&artifact, &args.output)?;
    println!("Wrote {}", args.output.display());
    Ok(())
}
