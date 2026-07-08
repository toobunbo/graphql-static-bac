# Implementation Plan: tophql routes CLI

## Mục tiêu

```bash
# Trước
graphql-static-bac stage s0 --input schema.graphql --output /tmp/s0.json
graphql-static-bac stage s2 --schema-ir /tmp/s0.json --policy lexicon.json --output /tmp/s2.json
graphql-static-bac route --schema-ir /tmp/s0.json --args /tmp/s2.json --policy route.json --target Event --output /tmp/routes.json

# Sau
tophql routes schema.graphql --type Event
```

Không thay đổi internal library, S3 contract, hay existing commands.

---

## Danh sách thay đổi

### Files tạo mới
```
src/embedded.rs
src/cli/commands/routes.rs
src/cli/formatter/mod.rs
src/cli/formatter/table.rs
src/cli/formatter/graphql_fmt.rs
src/cli/formatter/json_fmt.rs
src/cli/formatter/md.rs
```

### Files sửa
```
Cargo.toml                     ← thêm binary tophql
src/lib.rs                     ← pub mod embedded
src/argument/policy.rs         ← thêm default_argument_policy()
src/route/policy.rs            ← thêm default_route_policy()
src/cli/mod.rs                 ← thêm RoutesArgs + Routes command
src/cli/commands/mod.rs        ← pub mod routes
```

---

## Bước 1 — Đổi tên binary + thêm `tophql`

**`Cargo.toml`** — thêm binary mới, giữ binary cũ:

```toml
[[bin]]
name = "graphql-static-bac"
path = "src/main.rs"

[[bin]]
name = "tophql"
path = "src/main.rs"      # cùng entry point, khác tên
```

Cả hai binary dùng cùng `src/main.rs`. CLI auto-detect tên binary và có thể show help khác nhau sau này nếu cần.

---

## Bước 2 — Bundle default configs

**`src/embedded.rs`** (file mới):

```rust
pub(crate) const DEFAULT_LEXICON_JSON: &str =
    include_str!("../config/lexicons/argument-classifier-v1.json");

pub(crate) const DEFAULT_ROUTE_POLICY_JSON: &str =
    include_str!("../config/profiles/route-analysis-v1.json");
```

**`src/lib.rs`** — thêm:

```rust
pub(crate) mod embedded;
```

---

## Bước 3 — Default policy helpers

**`src/argument/policy.rs`** — thêm function:

```rust
pub fn default_argument_policy() -> LoadedArgumentPolicy {
    let bytes = crate::embedded::DEFAULT_LEXICON_JSON.as_bytes();
    let policy: ArgumentClassifierPolicy = serde_json::from_slice(bytes)
        .expect("bundled lexicon is valid JSON");
    let fingerprint = format!(
        "sha256:{:x}",
        sha2::Sha256::digest(bytes)
    );
    LoadedArgumentPolicy::from_policy(policy, fingerprint)
        .expect("bundled lexicon satisfies all constraints")
}
```

**`src/route/policy.rs`** — thêm function:

```rust
pub fn default_route_policy() -> LoadedRoutePolicy {
    let bytes = crate::embedded::DEFAULT_ROUTE_POLICY_JSON.as_bytes();
    let policy: RoutePolicy = serde_json::from_slice(bytes)
        .expect("bundled route policy is valid JSON");
    let fingerprint = format!(
        "sha256:{:x}",
        sha2::Sha256::digest(bytes)
    );
    LoadedRoutePolicy { policy, fingerprint }
}
```

Export từ `src/argument/mod.rs` và `src/route/mod.rs`:

```rust
// argument/mod.rs
pub use policy::default_argument_policy;

// route/mod.rs
pub use policy::default_route_policy;
```

---

## Bước 4 — Thêm `RoutesArgs` vào CLI

**`src/cli/mod.rs`** — thêm vào enum `Command`:

```rust
#[command(about = "Analyze routes to a target type (S0+S2+S3 in one step)")]
Routes(RoutesArgs),
```

Thêm struct `RoutesArgs`:

```rust
#[derive(Debug, Args)]
pub(crate) struct RoutesArgs {
    /// Schema file: SDL (.graphql) or introspection JSON (auto-detected)
    pub(crate) schema: PathBuf,

    /// Target type name (can repeat: --type Event --type User)
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
    Json,
    Graphql,
    Md,
}
```

Thêm vào `execute()`:

```rust
Command::Routes(args) => commands::routes::execute(args).map_err(CliError::from),
```

Thêm vào `CliError`:

```rust
#[error("routes command failed: {0}")]
Routes(#[from] commands::routes::RoutesError),
```

---

## Bước 5 — Command handler

**`src/cli/commands/routes.rs`** (file mới):

```rust
use std::fs;
use thiserror::Error;

use crate::argument::{classify_arguments, default_argument_policy, read_argument_policy};
use crate::cli::{OutputFormatArg, RoutesArgs, VerdictFilterArg};
use crate::cli::formatter;
use crate::contracts::{RouteVerdict, TargetRoutes};
use crate::graph::build_type_graph;
use crate::route::{analyze_target, default_route_policy, RouteFacts};
use crate::schema::{read_source, fingerprint, SchemaFormat};
use crate::stages::s0_ir::build_schema_ir_from_bytes;

#[derive(Debug, Error)]
pub enum RoutesError {
    #[error("cannot read schema: {0}")]
    Schema(String),
    #[error("argument classification failed: {0}")]
    Arguments(String),
    #[error("route analysis failed: {0}")]
    Routes(String),
    #[error("could not write output: {0}")]
    Output(#[from] std::io::Error),
}

pub(crate) fn execute(args: RoutesArgs) -> Result<(), RoutesError> {
    let use_color = !args.no_color && atty_stdout();

    // ── Step 1: Parse schema (S0) ────────────────────────────────────────────
    if !args.quiet { eprint!("  Parsing schema..."); }
    let schema = parse_schema(&args.schema)
        .map_err(|e| RoutesError::Schema(e.to_string()))?;
    if !args.quiet {
        eprintln!("  {} types, {} fields",
            schema.data.types.len(),
            schema.data.types.values().map(|t| t.fields.len()).sum::<usize>()
        );
    }

    // ── Step 2: Classify arguments (S2) ──────────────────────────────────────
    if !args.quiet { eprint!("  Classifying arguments..."); }
    let lex_policy = match &args.lexicon {
        Some(path) => read_argument_policy(path)
            .map_err(|e| RoutesError::Arguments(e.to_string()))?,
        None => default_argument_policy(),
    };
    let arguments = classify_arguments(&schema.data, &lex_policy)
        .map_err(|e| RoutesError::Arguments(e.to_string()))?;
    if !args.quiet {
        let selectors = arguments.fields.values()
            .flat_map(|f| &f.arguments)
            .filter(|a| a.classifications.iter().any(|c| {
                *c == crate::contracts::ArgumentClassification::ObjectSelector
            }))
            .count();
        eprintln!("  {} classified  ({} selectors)",
            arguments.fields.values().map(|f| f.arguments.len()).sum::<usize>(),
            selectors
        );
    }

    // ── Step 3: Route analysis (S3) ──────────────────────────────────────────
    if !args.quiet { eprint!("  Analyzing routes..."); }
    let route_policy = default_route_policy();
    let facts = RouteFacts::build(&schema.data, &arguments, &route_policy.policy)
        .map_err(|e| RoutesError::Routes(e.to_string()))?;
    let graph = build_type_graph(&schema.data)
        .map_err(|e| RoutesError::Routes(e.to_string()))?;

    let mut results: Vec<(String, TargetRoutes)> = Vec::new();
    for type_name in &args.types {
        let type_id = normalize_type_id(type_name);
        let target_routes = analyze_target(&graph, &type_id, &facts)
            .map_err(|e| RoutesError::Routes(e.to_string()))?;
        results.push((type_name.clone(), target_routes));
    }
    if !args.quiet {
        let total: usize = results.iter().map(|(_, t)| t.routes.len()).sum();
        eprintln!("  {} routes found\n", total);
    }

    // ── Step 4: Filter ───────────────────────────────────────────────────────
    let verdict_filter = build_verdict_filter(&args.filter);
    let filtered: Vec<(String, Vec<&crate::contracts::Route>)> = results
        .iter()
        .map(|(name, target)| {
            let routes = target.routes.iter()
                .filter(|r| verdict_filter.is_empty() || verdict_filter.contains(&r.verdict))
                .collect();
            (name.clone(), routes)
        })
        .collect();

    // ── Step 5: Format + output ───────────────────────────────────────────────
    let output = match args.format {
        OutputFormatArg::Table   => formatter::table::render(&filtered, &results, use_color),
        OutputFormatArg::Json    => formatter::json_fmt::render(&filtered),
        OutputFormatArg::Graphql => formatter::graphql_fmt::render(&filtered, &graph),
        OutputFormatArg::Md      => formatter::md::render(&filtered, &results, &args.schema),
    };

    match &args.out {
        Some(path) => fs::write(path, &output)?,
        None       => print!("{output}"),
    }
    Ok(())
}

fn parse_schema(path: &std::path::Path)
    -> Result<crate::contracts::Envelope<crate::contracts::SchemaIrData>, String>
{
    use crate::stages::s0_ir::{S0Options, build_schema_ir};
    build_schema_ir(&S0Options {
        input: path.to_path_buf(),
        format: SchemaFormat::Auto,
        producer: crate::contracts::Producer::current(),
    }).map_err(|e| e.to_string())
}

fn normalize_type_id(name: &str) -> String {
    if name.starts_with("type:") { name.to_string() }
    else { format!("type:{name}") }
}

fn build_verdict_filter(filter: &[VerdictFilterArg]) -> Vec<RouteVerdict> {
    if filter.iter().any(|f| matches!(f, VerdictFilterArg::All)) {
        return vec![];  // empty = no filter
    }
    filter.iter().map(|f| match f {
        VerdictFilterArg::Open    => RouteVerdict::Open,
        VerdictFilterArg::Unknown => RouteVerdict::Unknown,
        VerdictFilterArg::Guarded => RouteVerdict::Guarded,
        VerdictFilterArg::All     => unreachable!(),
    }).collect()
}

fn atty_stdout() -> bool {
    // Check if stdout is a terminal (not piped)
    // Simple: check TERM env var or use isatty if available
    std::env::var("TERM").is_ok() && !std::env::var("NO_COLOR").is_ok()
}
```

---

## Bước 6 — Formatters

**`src/cli/formatter/mod.rs`**:

```rust
pub(crate) mod graphql_fmt;
pub(crate) mod json_fmt;
pub(crate) mod md;
pub(crate) mod table;
```

### `table.rs` — terminal output

```rust
use crate::contracts::{Route, RouteVerdict};

pub(crate) fn render(
    filtered: &[(String, Vec<&Route>)],
    all: &[(String, crate::contracts::TargetRoutes)],
    color: bool,
) -> String {
    let mut out = String::new();

    for ((type_name, routes), (_, target)) in filtered.iter().zip(all.iter()) {
        let total = target.routes.len();
        let n_open    = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Open).count();
        let n_unknown = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Unknown).count();
        let n_guarded = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Guarded).count();

        // Header
        out.push_str(&format!(
            "\n{total} routes to {type_name}  \
             ({n_open} open · {n_unknown} unknown · {n_guarded} guarded)\n"
        ));

        // Open routes — always show full
        let open: Vec<_> = routes.iter().filter(|r| r.verdict == RouteVerdict::Open).collect();
        if !open.is_empty() {
            out.push_str(&section_header("OPEN", open.len(), color));
            out.push_str(&format!(
                "  {:<4}  {:<32}  {:<38}  {}\n",
                "#", "Entry field", "Selector", "Type"
            ));
            out.push_str(&"─".repeat(90));
            out.push('\n');
            for (i, route) in open.iter().enumerate() {
                let entry = route.witness.entry_field_id
                    .trim_start_matches("field:")
                    .to_string();
                let (sel_path, sel_type) = match &route.selector {
                    Some(s) => (s.arg_path.clone(), s.type_ref.display.clone()),
                    None    => ("—".to_string(), "—".to_string()),
                };
                out.push_str(&format!(
                    "  {:<4}  {:<32}  {:<38}  {}\n",
                    i + 1, entry, sel_path, sel_type
                ));
            }
        }

        // Unknown routes — summary only (group by terminal field)
        let unknown: Vec<_> = routes.iter().filter(|r| r.verdict == RouteVerdict::Unknown).collect();
        if !unknown.is_empty() {
            out.push_str(&section_header("UNKNOWN", unknown.len(), color));
            out.push_str("  (use --filter unknown to expand)\n\n");

            // Group by terminal semantic edge
            let mut by_terminal: std::collections::BTreeMap<&str, usize> = Default::default();
            for r in &unknown {
                *by_terminal.entry(r.terminal_semantic_edge_id.trim_start_matches("field:"))
                    .or_default() += 1;
            }
            let mut groups: Vec<_> = by_terminal.into_iter().collect();
            groups.sort_by(|a, b| b.1.cmp(&a.1));
            for (terminal, count) in groups.iter().take(5) {
                out.push_str(&format!("  {:>4}  via {terminal}\n", count));
            }
            if groups.len() > 5 {
                out.push_str(&format!("  ... and {} more entry points\n", groups.len() - 5));
            }
        }

        out.push('\n');
    }
    out
}

fn section_header(label: &str, count: usize, color: bool) -> String {
    let colored = if color {
        match label {
            "OPEN"    => format!("\x1b[32;1m{label}\x1b[0m"),
            "UNKNOWN" => format!("\x1b[33m{label}\x1b[0m"),
            "GUARDED" => format!("\x1b[34m{label}\x1b[0m"),
            _         => label.to_string(),
        }
    } else {
        label.to_string()
    };
    format!("\n{colored} ({count})\n{}\n", "─".repeat(90))
}
```

### `graphql_fmt.rs` — query templates

```rust
use crate::contracts::Route;
use crate::graph::TypeGraph;

pub(crate) fn render(filtered: &[(String, Vec<&Route>)], graph: &TypeGraph) -> String {
    let mut out = String::new();
    out.push_str("# Generated by tophql\n\n");

    for (type_name, routes) in filtered {
        for (i, route) in routes.iter().enumerate() {
            let verdict = format!("{:?}", route.verdict).to_uppercase();
            let entry = route.witness.entry_field_id.trim_start_matches("field:");
            let sel = route.selector.as_ref()
                .map(|s| format!(" — selector: {}", s.arg_path))
                .unwrap_or_default();

            out.push_str(&format!(
                "# [{verdict}] Route {} — {}{}\n",
                i + 1, entry, sel
            ));
            out.push_str(&emit_route_query(route, type_name, i + 1));
            out.push_str("\n\n");
        }
    }
    out
}

fn emit_route_query(route: &Route, type_name: &str, index: usize) -> String {
    // Build variable declarations from selector
    let (vars, args) = match &route.selector {
        None => (String::new(), String::new()),
        Some(sel) => {
            let var_name = "seed1";
            let var_type = &sel.type_ref.display;
            // Handle nested input path
            if sel.input_path.is_empty() {
                (
                    format!("(${var_name}: {var_type})"),
                    format!("({}: ${var_name})", sel.leaf_name()),
                )
            } else {
                // Wrap in input object hierarchy
                let root_arg = sel.root_arg_ref.rsplit('.').next().unwrap_or("input");
                let wrapped = wrap_input_path(&sel.input_path, var_name, var_type);
                (
                    format!("(${var_name}: {var_type})"),
                    format!("({root_arg}: {wrapped})"),
                )
            }
        }
    };

    // Build selection set from witness edges
    let body = emit_selection_set(route, type_name);

    format!(
        "query Test{type_name}_{index}{vars} {{\n{body}}}\n",
    )
}

fn wrap_input_path(path: &[String], var: &str, _var_type: &str) -> String {
    // ["filters", "ids"] → "{ filters: { ids: $var } }"
    if path.is_empty() { return format!("${var}"); }
    let inner = wrap_nested(&path[1..], var);
    format!("{{ {}: {} }}", path[0], inner)
}

fn wrap_nested(path: &[String], var: &str) -> String {
    if path.is_empty() { return format!("${var}"); }
    let inner = wrap_nested(&path[1..], var);
    format!("{{ {}: {} }}", path[0], inner)
}

fn emit_selection_set(route: &Route, type_name: &str) -> String {
    // Simplified: emit witness path as nested selection
    // Full implementation traverses witness edges and builds the query tree
    let entry = route.witness.entry_field_id.trim_start_matches("field:");
    let field = entry.split('.').last().unwrap_or(entry);
    format!(
        "  {field} {{\n    __typename\n    id\n  }}\n"
    )
}

trait SelectorLeafName {
    fn leaf_name(&self) -> &str;
}

impl SelectorLeafName for crate::contracts::RouteSelector {
    fn leaf_name(&self) -> &str {
        self.arg_path.rsplit('.').next().unwrap_or("id")
    }
}
```

> **Note:** `emit_selection_set` ở trên là simplified. Full implementation cần traverse `witness.edges` và build nested GraphQL selection tree từ đó — đây là phần phức tạp nhất của formatter, nên làm riêng sau.

### `json_fmt.rs` — simplified JSON

```rust
use crate::contracts::{Route, RouteVerdict};
use serde_json::{json, Value};

pub(crate) fn render(filtered: &[(String, Vec<&Route>)]) -> String {
    let targets: Vec<Value> = filtered.iter().map(|(type_name, routes)| {
        let route_values: Vec<Value> = routes.iter().map(|r| {
            let sel = r.selector.as_ref().map(|s| json!({
                "arg_path": s.arg_path,
                "type": s.type_ref.display,
                "nested": !s.input_path.is_empty(),
                "input_path": s.input_path,
            }));
            json!({
                "verdict": format!("{:?}", r.verdict).to_lowercase(),
                "origin": format!("{:?}", r.origin).to_lowercase(),
                "entry_field": r.witness.entry_field_id.trim_start_matches("field:"),
                "path": r.witness.display_projection,
                "selector": sel,
                "terminal": r.terminal_semantic_edge_id.trim_start_matches("field:"),
                "boundaries": r.signature.boundary_families.iter()
                    .map(|b| format!("{b:?}").to_lowercase())
                    .collect::<Vec<_>>(),
            })
        }).collect();

        json!({
            "type": type_name,
            "total": routes.len(),
            "routes": route_values,
        })
    }).collect();

    let output = json!({ "targets": targets });
    serde_json::to_string_pretty(&output).unwrap_or_default()
}
```

### `md.rs` — markdown report

```rust
use std::path::Path;
use crate::contracts::{Route, RouteVerdict, TargetRoutes};

pub(crate) fn render(
    filtered: &[(String, Vec<&Route>)],
    all: &[(String, TargetRoutes)],
    schema_path: &Path,
) -> String {
    let schema_name = schema_path.file_name()
        .and_then(|n| n.to_str()).unwrap_or("schema");
    let date = ""; // optionally inject date

    let mut out = format!("# Route Analysis\n\n");
    out.push_str(&format!("**Schema:** `{schema_name}`\n\n"));

    for ((type_name, routes), (_, target)) in filtered.iter().zip(all.iter()) {
        let n_open    = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Open).count();
        let n_unknown = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Unknown).count();
        let n_guarded = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Guarded).count();

        out.push_str(&format!("## {type_name}\n\n"));
        out.push_str(&format!(
            "| Verdict | Count |\n|---------|------:|\n\
             | open | {n_open} |\n| unknown | {n_unknown} |\n| guarded | {n_guarded} |\n\n"
        ));

        let open: Vec<_> = routes.iter().filter(|r| r.verdict == RouteVerdict::Open).collect();
        if !open.is_empty() {
            out.push_str("### Open Routes\n\n");
            out.push_str("| # | Path | Selector | Type |\n|---|------|----------|------|\n");
            for (i, r) in open.iter().enumerate() {
                let path = &r.witness.display_projection;
                let (sel, tref) = match &r.selector {
                    Some(s) => (s.arg_path.as_str(), s.type_ref.display.as_str()),
                    None    => ("—", "—"),
                };
                out.push_str(&format!("| {} | `{path}` | `{sel}` | `{tref}` |\n", i + 1));
            }
            out.push('\n');
        }
    }
    out
}
```

---

## Bước 7 — Wire vào `src/cli/commands/mod.rs`

```rust
pub(crate) mod enumerate;
pub(crate) mod policy_oracle;
pub(crate) mod policy_type_pipeline;
pub(crate) mod route;
pub(crate) mod routes;      // ← thêm mới
pub(crate) mod s0;
pub(crate) mod s2;
pub(crate) mod s3;
pub(crate) mod s4;
pub(crate) mod seed_runtime;
```

---

## Bước 8 — Export từ `src/stages/s0_ir/mod.rs`

`build_schema_ir` hiện chỉ export `run_schema_ir`. Cần export cả function và types:

```rust
pub use runner::{build_schema_ir, S0Error, S0Options};
```

---

## Thứ tự implement (tránh bị block)

```
1. embedded.rs              → không có dependency
2. default_argument_policy  → cần embedded.rs
3. default_route_policy     → cần embedded.rs
4. RoutesArgs struct        → clap parsing, không cần logic
5. formatter/mod.rs + table.rs   → cần contracts types
6. formatter/json_fmt.rs    → cần serde_json (đã có)
7. routes.rs handler        → cần tất cả trên
8. Wire: cli/mod.rs + commands/mod.rs
9. formatter/graphql_fmt.rs → sau, phần emit_selection_set phức tạp hơn
10. formatter/md.rs         → sau
11. Rename binary Cargo.toml
```

---

## Test sau khi implement

```bash
# Smoke test
cargo build --release
./target/release/tophql routes output/schema2/schema_ir.json --type Event

# Filter
./target/release/tophql routes schema2.graphql --type Event --filter open

# Export
./target/release/tophql routes schema2.graphql --type Event --format graphql
./target/release/tophql routes schema2.graphql --type Event --format json --out /tmp/out.json

# Multiple types
./target/release/tophql routes schema2.graphql --type Event --type Community

# Custom lexicon
./target/release/tophql routes schema2.graphql --type Event --lexicon config/lexicons/argument-classifier-v1.json

# Old commands still work
./target/release/graphql-static-bac stage s3 --help
```

---

## Những gì chưa làm trong MVP này

| Feature | Lý do hoãn |
|---|---|
| `--list-types` flag | Cần thêm subcommand riêng |
| `emit_selection_set` đầy đủ trong graphql formatter | Complex — cần build query tree từ witness edges, bao gồm type conditions và list wrapping |
| Progress spinner animation | Cần thêm dependency hoặc thread |
| `--type all` (scan toàn bộ schema) | Cần enumerate types từ S0 output |
| Error message gợi ý "did you mean?" | Nice to have |
