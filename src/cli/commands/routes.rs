use std::fs;

use thiserror::Error;

use crate::argument::{classify_arguments, default_argument_policy, read_argument_policy};
use crate::cli::formatter;
use crate::cli::{OutputFormatArg, RoutesArgs, VerdictFilterArg};
use crate::contracts::{Route, RouteVerdict, TargetRoutes};
use crate::graph::build_type_graph;
use crate::route::{analyze_target, default_route_policy, RouteFacts};
use crate::stages::s0_ir::{build_schema_ir, S0Options};

#[derive(Debug, Error)]
pub enum RoutesError {
    #[error("cannot read or parse schema: {0}")]
    Schema(String),
    #[error("argument classification failed: {0}")]
    Arguments(String),
    #[error("route analysis failed: {0}")]
    Routes(String),
    #[error("could not write output: {0}")]
    Output(#[from] std::io::Error),
}

pub(crate) fn execute(args: RoutesArgs) -> Result<(), RoutesError> {
    let use_color = !args.no_color && is_terminal();

    // ── Step 1: Parse schema (S0) ─────────────────────────────────────────────
    progress(&args, "Parsing schema...");
    let schema = build_schema_ir(&S0Options {
        input: args.schema.clone(),
        format: crate::schema::SchemaFormat::Auto,
        producer: crate::contracts::Producer::current(),
    })
    .map_err(|e| RoutesError::Schema(e.to_string()))?;
    let type_count = schema.data.types.len();
    let field_count: usize = schema.data.types.values().map(|t| t.fields.len()).sum();
    progress_done(&args, &format!("{type_count} types, {field_count} fields"));

    // ── Step 2: Classify arguments (S2) ──────────────────────────────────────
    progress(&args, "Classifying arguments...");
    let lex_policy = match &args.lexicon {
        Some(path) => read_argument_policy(path)
            .map_err(|e| RoutesError::Arguments(e.to_string()))?,
        None => default_argument_policy(),
    };
    let arguments = classify_arguments(&schema.data, &lex_policy)
        .map_err(|e| RoutesError::Arguments(e.to_string()))?;
    let total_args: usize = arguments.fields.values().map(|f| f.arguments.len()).sum();
    let selector_count = arguments
        .fields
        .values()
        .flat_map(|f| &f.arguments)
        .filter(|a| {
            a.classifications
                .contains(&crate::contracts::ArgumentClassification::ObjectSelector)
        })
        .count();
    progress_done(&args, &format!("{total_args} classified  ({selector_count} selectors)"));

    // ── Step 3: Route analysis (S3) ──────────────────────────────────────────
    progress(&args, "Analyzing routes...");
    let route_policy = default_route_policy();
    let facts = RouteFacts::build(&schema.data, &arguments, &route_policy.policy)
        .map_err(|e| RoutesError::Routes(e.to_string()))?;
    let graph =
        build_type_graph(&schema.data).map_err(|e| RoutesError::Routes(e.to_string()))?;

    let mut all_results: Vec<(String, TargetRoutes)> = Vec::new();
    for type_name in &args.types {
        let type_id = normalize_type_id(type_name);
        let target_routes =
            analyze_target(&graph, &type_id, &facts)
                .map_err(|e| RoutesError::Routes(e.to_string()))?;
        all_results.push((type_name.clone(), target_routes));
    }
    let total_routes: usize = all_results.iter().map(|(_, t)| t.routes.len()).sum();
    let total_open = all_results
        .iter()
        .flat_map(|(_, t)| &t.routes)
        .filter(|r| r.verdict == RouteVerdict::Open)
        .count();
    let total_unknown = all_results
        .iter()
        .flat_map(|(_, t)| &t.routes)
        .filter(|r| r.verdict == RouteVerdict::Unknown)
        .count();
    progress_done(
        &args,
        &format!("{total_routes} routes  ({total_open} open · {total_unknown} unknown)"),
    );
    if !args.quiet {
        eprintln!();
    }

    // ── Step 4: Filter by verdict ─────────────────────────────────────────────
    let verdict_filter = build_verdict_filter(&args.filter);
    let filtered: Vec<(String, Vec<&Route>)> = all_results
        .iter()
        .map(|(name, target)| {
            let routes: Vec<&Route> = target
                .routes
                .iter()
                .filter(|r| {
                    verdict_filter.is_empty() || verdict_filter.contains(&r.verdict)
                })
                .collect();
            (name.clone(), routes)
        })
        .collect();

    // ── Step 5: Format ────────────────────────────────────────────────────────
    let output = match args.format {
        OutputFormatArg::Table => formatter::table::render(&filtered, &all_results, use_color),
        OutputFormatArg::Tree => {
            let mut out = String::new();
            for (type_name, routes) in &filtered {
                out.push_str(&formatter::tree::render(
                    type_name, routes, use_color, args.depth,
                ));
            }
            out
        }
        OutputFormatArg::Json => formatter::json_fmt::render(&filtered),
        OutputFormatArg::Graphql => formatter::graphql_fmt::render(&filtered),
        OutputFormatArg::Md => formatter::md::render(&filtered, &all_results, &args.schema),
    };

    // ── Step 6: Output ────────────────────────────────────────────────────────
    match &args.out {
        Some(path) => {
            fs::write(path, &output)?;
            if !args.quiet {
                eprintln!("  Wrote {}", path.display());
            }
        }
        None => print!("{output}"),
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normalize_type_id(name: &str) -> String {
    if name.starts_with("type:") {
        name.to_string()
    } else {
        format!("type:{name}")
    }
}

fn build_verdict_filter(filter: &[VerdictFilterArg]) -> Vec<RouteVerdict> {
    if filter.iter().any(|f| matches!(f, VerdictFilterArg::All)) {
        return Vec::new(); // empty = no filter
    }
    filter
        .iter()
        .map(|f| match f {
            VerdictFilterArg::Open => RouteVerdict::Open,
            VerdictFilterArg::Unknown => RouteVerdict::Unknown,
            VerdictFilterArg::Guarded => RouteVerdict::Guarded,
            VerdictFilterArg::All => unreachable!(),
        })
        .collect()
}

fn is_terminal() -> bool {
    std::env::var("NO_COLOR").is_err()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(false)
}

fn progress(args: &RoutesArgs, msg: &str) {
    if !args.quiet {
        eprint!("  {msg}");
    }
}

fn progress_done(args: &RoutesArgs, detail: &str) {
    if !args.quiet {
        eprintln!("  {detail}");
    }
}
