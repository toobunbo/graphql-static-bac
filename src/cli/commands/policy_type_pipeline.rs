use crate::cli::PolicyTypePipelineArgs;
use crate::pipeline::{
    run_policy_type_pipeline, PolicyTypePipelineError, PolicyTypePipelineOptions,
};

pub(crate) fn execute(args: PolicyTypePipelineArgs) -> Result<(), PolicyTypePipelineError> {
    let options = PolicyTypePipelineOptions {
        schema_ir: args.schema_ir,
        arguments: args.arguments,
        route_policy: args.policy,
        target: args.target,
        policy_hypotheses: args.policy_hypotheses,
        owner_request: args.owner_request,
        observer_request: args.observer_request,
        output_dir: args.output_dir,
    };
    run_policy_type_pipeline(&options)?;
    println!(
        "Wrote policy type pipeline outputs to {}",
        options.output_dir.display()
    );
    Ok(())
}
