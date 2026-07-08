use crate::cli::S0Args;
use crate::contracts::Producer;
use crate::stages::s0_ir::{build_schema_ir, write_schema_ir, S0Error, S0Options};

pub(crate) fn execute(args: S0Args) -> Result<(), S0Error> {
    let options = S0Options {
        input: args.input,
        format: args.format.into(),
        producer: Producer::current(),
    };
    let artifact = build_schema_ir(&options)?;
    write_schema_ir(&artifact, &args.output)?;
    println!("Wrote {}", args.output.display());
    Ok(())
}
