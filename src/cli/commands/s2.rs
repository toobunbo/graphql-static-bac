use crate::cli::S2Args;
use crate::stages::s2_arguments::{read_and_run_arguments, write_arguments, S2ArgumentError};

pub(crate) fn execute(args: S2Args) -> Result<(), S2ArgumentError> {
    let artifact = read_and_run_arguments(&args.schema_ir, &args.policy)?;
    write_arguments(&artifact, &args.output)?;
    println!("Wrote {}", args.output.display());
    Ok(())
}
