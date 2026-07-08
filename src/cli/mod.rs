mod commands;
pub(crate) mod formatter;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::artifact::ArtifactReadError;
use crate::pipeline::PolicyTypePipelineError;
use crate::route::RoutePolicyError;
use crate::schema::SchemaFormat;
use crate::stages::policy_oracle::PolicyOracleStageError;
use crate::stages::s0_ir::S0Error;
use crate::stages::s2_arguments::S2ArgumentError;
use crate::stages::s3_paths::S3Error;
use crate::stages::s3_routes::S3RouteError;
use crate::stages::s4_seed_plans::S4SeedPlanError;
use crate::stages::seed_runtime::SeedRuntimeStageError;

#[derive(Debug, Parser)]
#[command(name = "graphql-static-bac", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Analyze routes to a target type (schema → results in one step)")]
    Routes(RoutesArgs),
    #[command(about = "Run the legacy structural path enumerator for calibration")]
    Enumerate(EnumerateArgs),
    #[command(about = "Analyze semantic routes for one target type (requires pre-built S0/S2)")]
    Route(RouteArgs),
    Stage(StageCommand),
    Runtime(RuntimeCommand),
    Pipeline(PipelineCommand),
}

#[derive(Debug, Args)]
struct StageCommand {
    #[command(subcommand)]
    stage: Stage,
}

#[derive(Debug, Subcommand)]
enum Stage {
    S0(S0Args),
    S2(S2Args),
    S3(S3Args),
    S4(S4Args),
}

#[derive(Debug, Args)]
struct RuntimeCommand {
    #[command(subcommand)]
    command: RuntimeSubcommand,
}

#[derive(Debug, Subcommand)]
enum RuntimeSubcommand {
    #[command(about = "Execute S4 seed plans and validate selected S3 routes")]
    Seed(RuntimeSeedArgs),
    #[command(about = "Replay verified owner routes under an observer policy oracle")]
    PolicyOracle(PolicyOracleArgs),
}

#[derive(Debug, Args)]
struct PipelineCommand {
    #[command(subcommand)]
    command: PipelineSubcommand,
}

#[derive(Debug, Subcommand)]
enum PipelineSubcommand {
    #[command(about = "Run S3, S4, owner seed runtime, and the policy oracle for one type")]
    PolicyType(PolicyTypePipelineArgs),
}

#[derive(Debug, Args)]
pub(crate) struct S0Args {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long, value_enum, default_value_t = FormatArg::Auto)]
    pub(crate) format: FormatArg,
    #[arg(long)]
    pub(crate) output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct S2Args {
    #[arg(long)]
    pub(crate) schema_ir: PathBuf,
    #[arg(long)]
    pub(crate) policy: PathBuf,
    #[arg(long)]
    pub(crate) output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct EnumerateArgs {
    #[arg(long)]
    pub(crate) schema_ir: PathBuf,
    #[arg(long)]
    pub(crate) target: String,
    #[arg(long)]
    pub(crate) output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct RouteArgs {
    #[arg(long)]
    pub(crate) schema_ir: PathBuf,
    #[arg(long = "args")]
    pub(crate) arguments: PathBuf,
    #[arg(long)]
    pub(crate) policy: PathBuf,
    #[arg(long)]
    pub(crate) target: String,
    #[arg(long)]
    pub(crate) output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct S3Args {
    #[arg(long)]
    pub(crate) schema_ir: PathBuf,
    #[arg(long)]
    pub(crate) sinks: PathBuf,
    #[arg(long = "args")]
    pub(crate) arguments: PathBuf,
    #[arg(long)]
    pub(crate) policy: PathBuf,
    #[arg(long)]
    pub(crate) output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct S4Args {
    #[arg(long)]
    pub(crate) schema_ir: PathBuf,
    #[arg(long = "args")]
    pub(crate) arguments: PathBuf,
    #[arg(long)]
    pub(crate) routes: PathBuf,
    #[arg(long)]
    pub(crate) output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct RuntimeSeedArgs {
    #[arg(long)]
    pub(crate) schema_ir: PathBuf,
    #[arg(long)]
    pub(crate) routes: PathBuf,
    #[arg(long)]
    pub(crate) seed_plans: PathBuf,
    #[arg(long)]
    pub(crate) request_template: PathBuf,
    #[arg(
        long = "verdict",
        value_enum,
        default_values_t = [RuntimeVerdictArg::Open, RuntimeVerdictArg::Unknown]
    )]
    pub(crate) verdicts: Vec<RuntimeVerdictArg>,
    #[arg(long = "route-id")]
    pub(crate) route_ids: Vec<String>,
    #[arg(long)]
    pub(crate) output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct PolicyOracleArgs {
    #[arg(long)]
    pub(crate) schema_ir: PathBuf,
    #[arg(long)]
    pub(crate) routes: PathBuf,
    #[arg(long)]
    pub(crate) seed_plans: PathBuf,
    #[arg(long)]
    pub(crate) owner_seeds: PathBuf,
    #[arg(long)]
    pub(crate) policy_hypotheses: PathBuf,
    #[arg(long)]
    pub(crate) owner_request: PathBuf,
    #[arg(long)]
    pub(crate) observer_request: PathBuf,
    #[arg(
        long = "verdict",
        value_enum,
        default_values_t = [RuntimeVerdictArg::Open, RuntimeVerdictArg::Unknown]
    )]
    pub(crate) verdicts: Vec<RuntimeVerdictArg>,
    #[arg(long = "route-id")]
    pub(crate) route_ids: Vec<String>,
    #[arg(long)]
    pub(crate) runtime_output: PathBuf,
    #[arg(long)]
    pub(crate) candidates_output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct PolicyTypePipelineArgs {
    #[arg(long)]
    pub(crate) schema_ir: PathBuf,
    #[arg(long = "args")]
    pub(crate) arguments: PathBuf,
    #[arg(long)]
    pub(crate) policy: PathBuf,
    #[arg(long)]
    pub(crate) target: String,
    #[arg(long)]
    pub(crate) policy_hypotheses: PathBuf,
    #[arg(long)]
    pub(crate) owner_request: PathBuf,
    #[arg(long)]
    pub(crate) observer_request: PathBuf,
    #[arg(long)]
    pub(crate) output_dir: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct RoutesArgs {
    /// Schema file: SDL (.graphql) or introspection JSON (auto-detected)
    pub(crate) schema: PathBuf,

    /// Target type name — can be repeated: --type Event --type User
    #[arg(long = "type", value_name = "TYPE", required = true)]
    pub(crate) types: Vec<String>,

    /// Verdict filter, comma-separated [default: open,unknown]
    #[arg(long, value_delimiter = ',', default_value = "open,unknown")]
    pub(crate) filter: Vec<VerdictFilterArg>,

    /// Output format [default: table]
    #[arg(long, default_value = "table")]
    pub(crate) format: OutputFormatArg,

    /// Write output to file instead of stdout
    #[arg(long)]
    pub(crate) out: Option<PathBuf>,

    /// Custom lexicon file (default: bundled argument-classifier-v1)
    #[arg(long)]
    pub(crate) lexicon: Option<PathBuf>,

    /// Max tree depth for --format tree [default: 4, 0 = unlimited]
    #[arg(long, default_value = "4")]
    pub(crate) depth: usize,

    /// Disable color output
    #[arg(long)]
    pub(crate) no_color: bool,

    /// Suppress progress messages
    #[arg(long)]
    pub(crate) quiet: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum VerdictFilterArg {
    Open,
    Unknown,
    Guarded,
    All,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum OutputFormatArg {
    Table,
    Tree,
    Json,
    Graphql,
    Md,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum RuntimeVerdictArg {
    Open,
    Unknown,
    Guarded,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum FormatArg {
    Auto,
    Introspection,
    Sdl,
}

impl From<FormatArg> for SchemaFormat {
    fn from(value: FormatArg) -> Self {
        match value {
            FormatArg::Auto => Self::Auto,
            FormatArg::Introspection => Self::Introspection,
            FormatArg::Sdl => Self::Sdl,
        }
    }
}

pub fn run() -> i32 {
    run_from(std::env::args_os())
}

pub fn run_from<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let code = if error.use_stderr() { 2 } else { 0 };
            let _ = error.print();
            return code;
        }
    };
    match execute(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            error_exit_code(&error)
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{0}")]
    RoutesCmd(#[from] commands::routes::RoutesError),
    #[error("stage s0 failed: {0}")]
    S0(#[from] S0Error),
    #[error("stage s2 failed: {0}")]
    S2(#[from] S2ArgumentError),
    #[error("legacy path enumeration failed: {0}")]
    Paths(#[from] S3Error),
    #[error("route analysis failed: {0}")]
    Routes(#[from] S3RouteError),
    #[error("seed planning failed: {0}")]
    SeedPlans(#[from] S4SeedPlanError),
    #[error("seed runtime failed: {0}")]
    SeedRuntime(#[from] SeedRuntimeStageError),
    #[error("policy oracle failed: {0}")]
    PolicyOracle(#[from] PolicyOracleStageError),
    #[error("policy type pipeline failed: {0}")]
    PolicyTypePipeline(#[from] PolicyTypePipelineError),
}

fn execute(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Routes(args) => commands::routes::execute(args).map_err(CliError::from),
        Command::Enumerate(args) => commands::enumerate::execute(args).map_err(CliError::from),
        Command::Route(args) => commands::route::execute(args).map_err(CliError::from),
        Command::Stage(command) => match command.stage {
            Stage::S0(args) => commands::s0::execute(args).map_err(CliError::from),
            Stage::S2(args) => commands::s2::execute(args).map_err(CliError::from),
            Stage::S3(args) => commands::s3::execute(args).map_err(CliError::from),
            Stage::S4(args) => commands::s4::execute(args).map_err(CliError::from),
        },
        Command::Runtime(command) => match command.command {
            RuntimeSubcommand::Seed(args) => {
                commands::seed_runtime::execute(args).map_err(CliError::from)
            }
            RuntimeSubcommand::PolicyOracle(args) => {
                commands::policy_oracle::execute(args).map_err(CliError::from)
            }
        },
        Command::Pipeline(command) => match command.command {
            PipelineSubcommand::PolicyType(args) => {
                commands::policy_type_pipeline::execute(args).map_err(CliError::from)
            }
        },
    }
}

fn error_exit_code(error: &CliError) -> i32 {
    match error {
        CliError::RoutesCmd(e) => match e {
            commands::routes::RoutesError::Schema(_) => 4,
            commands::routes::RoutesError::Arguments(_) => 5,
            commands::routes::RoutesError::Routes(_) => 5,
            commands::routes::RoutesError::Output(_) => 6,
        },
        CliError::S0(error) => match error {
            S0Error::Source(_) => 3,
            S0Error::Parse(_) => 4,
            S0Error::Validation(_) => 5,
            S0Error::Write(_) => 6,
        },
        CliError::S2(error) => match error {
            S2ArgumentError::Read(ArtifactReadError::Io { .. })
            | S2ArgumentError::Policy(crate::argument::ArgumentPolicyError::Io { .. }) => 3,
            S2ArgumentError::Read(
                ArtifactReadError::Json { .. } | ArtifactReadError::Contract(_),
            )
            | S2ArgumentError::Policy(
                crate::argument::ArgumentPolicyError::Json { .. }
                | crate::argument::ArgumentPolicyError::Contract(_),
            ) => 4,
            S2ArgumentError::Schema(_) | S2ArgumentError::Classification(_) => 5,
            S2ArgumentError::Write(_) => 6,
        },
        CliError::Paths(error) => match error {
            S3Error::Read(ArtifactReadError::Io { .. }) => 3,
            S3Error::Read(ArtifactReadError::Json { .. } | ArtifactReadError::Contract(_)) => 4,
            S3Error::Graph(_) | S3Error::Enumeration(_) | S3Error::Contract(_) => 5,
            S3Error::Write(_) => 6,
        },
        CliError::Routes(error) => match error {
            S3RouteError::Read(ArtifactReadError::Io { .. })
            | S3RouteError::Policy(RoutePolicyError::Io { .. }) => 3,
            S3RouteError::Read(ArtifactReadError::Json { .. } | ArtifactReadError::Contract(_))
            | S3RouteError::Policy(RoutePolicyError::Json { .. } | RoutePolicyError::Contract(_)) => {
                4
            }
            S3RouteError::Graph(_)
            | S3RouteError::Facts(_)
            | S3RouteError::Analysis(_)
            | S3RouteError::Contract(_) => 5,
            S3RouteError::Write(_) => 6,
        },
        CliError::SeedPlans(error) => match error {
            S4SeedPlanError::Read(ArtifactReadError::Io { .. }) => 3,
            S4SeedPlanError::Read(
                ArtifactReadError::Json { .. } | ArtifactReadError::Contract(_),
            ) => 4,
            S4SeedPlanError::Planning(_) | S4SeedPlanError::Contract(_) => 5,
            S4SeedPlanError::Write(_) => 6,
        },
        CliError::SeedRuntime(error) => match error {
            SeedRuntimeStageError::Read(ArtifactReadError::Io { .. })
            | SeedRuntimeStageError::ProfileIo { .. } => 3,
            SeedRuntimeStageError::Read(
                ArtifactReadError::Json { .. } | ArtifactReadError::Contract(_),
            )
            | SeedRuntimeStageError::ProfileJson { .. } => 4,
            SeedRuntimeStageError::Runtime(_) => 5,
            SeedRuntimeStageError::Write(_) => 6,
        },
        CliError::PolicyOracle(error) => match error {
            PolicyOracleStageError::Read(ArtifactReadError::Io { .. })
            | PolicyOracleStageError::ProfileIo { .. } => 3,
            PolicyOracleStageError::Read(
                ArtifactReadError::Json { .. } | ArtifactReadError::Contract(_),
            )
            | PolicyOracleStageError::ProfileJson { .. } => 4,
            PolicyOracleStageError::Oracle(_) => 5,
            PolicyOracleStageError::Write(_) => 6,
        },
        CliError::PolicyTypePipeline(_) => 5,
    }
}
