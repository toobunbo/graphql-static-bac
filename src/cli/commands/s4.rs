use crate::cli::S4Args;
use crate::stages::s4_seed_plans::{read_and_run_seed_plans, write_seed_plans, S4SeedPlanError};

pub(crate) fn execute(args: S4Args) -> Result<(), S4SeedPlanError> {
    let artifact = read_and_run_seed_plans(&args.schema_ir, &args.arguments, &args.routes)?;
    write_seed_plans(&artifact, &args.output)?;
    println!("Wrote {}", args.output.display());
    Ok(())
}
