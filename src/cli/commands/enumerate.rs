use crate::artifact::read_artifact;
use crate::cli::EnumerateArgs;
use crate::contracts::{SchemaIrData, StageId};
use crate::stages::s3_paths::{enumerate_schema_target, write_paths, S3Error};

pub(crate) fn execute(args: EnumerateArgs) -> Result<(), S3Error> {
    let schema = read_artifact::<SchemaIrData>(&args.schema_ir, StageId::S0)?;
    let artifact = enumerate_schema_target(&schema, &args.target)?;
    write_paths(&artifact, &args.output)?;
    println!("Wrote {}", args.output.display());
    Ok(())
}
