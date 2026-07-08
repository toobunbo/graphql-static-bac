use crate::cli::S3Args;
use crate::stages::s3_routes::{read_and_run_routes, write_routes, S3RouteError};

pub(crate) fn execute(args: S3Args) -> Result<(), S3RouteError> {
    let artifact =
        read_and_run_routes(&args.schema_ir, &args.sinks, &args.arguments, &args.policy)?;
    write_routes(&artifact, &args.output)?;
    println!("Wrote {}", args.output.display());
    Ok(())
}
