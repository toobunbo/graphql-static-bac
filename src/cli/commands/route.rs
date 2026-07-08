use crate::artifact::read_artifact;
use crate::cli::RouteArgs;
use crate::contracts::{ArgumentsData, SchemaIrData, StageId};
use crate::route::read_route_policy;
use crate::stages::s3_routes::{analyze_schema_target, write_routes, S3RouteError};

pub(crate) fn execute(args: RouteArgs) -> Result<(), S3RouteError> {
    let schema = read_artifact::<SchemaIrData>(&args.schema_ir, StageId::S0)?;
    let arguments = read_artifact::<ArgumentsData>(&args.arguments, StageId::S2)?;
    let policy = read_route_policy(&args.policy)?;
    let artifact = analyze_schema_target(&schema, &arguments, &policy, &args.target)?;
    write_routes(&artifact, &args.output)?;
    println!("Wrote {}", args.output.display());
    Ok(())
}
