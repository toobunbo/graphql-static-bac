# Plan: CLI Simplification

## Mục tiêu

Thay thế pipeline nhiều bước thủ công bằng **một lệnh duy nhất** cho user:

```bash
# Hiện tại (không ai dùng được)
graphql-static-bac stage s0 --input schema.graphql --output /tmp/s0.json
graphql-static-bac stage s2 --schema-ir /tmp/s0.json --policy lexicon.json --output /tmp/s2.json
graphql-static-bac route --schema-ir /tmp/s0.json --args /tmp/s2.json --policy route-policy.json --target Event --output /tmp/routes.json

# Mục tiêu
tophql routes schema.graphql --type Event
```

---

## Command interface cuối cùng

```
tophql routes <schema> --type <TypeName> [options]

Arguments:
  <schema>          Schema file: .graphql (SDL) hoặc introspection JSON
                    Detect format tự động theo content

Options:
  --type <name>     Target type cần phân tích (có thể dùng nhiều lần)
                    Ví dụ: --type Event --type User

  --filter <list>   Verdict filter, comma-separated [default: open,unknown]
                    Choices: open, unknown, guarded, all

  --format <fmt>    Output format [default: table]
                    Choices: table, json, graphql, md

  --out <path>      Ghi kết quả ra file thay vì stdout
                    Extension .md → tự chọn format md
                    Extension .graphql → tự chọn format graphql
                    Extension .json → tự chọn format json

  --lexicon <path>  Custom lexicon file [default: bundled default]

  --no-color        Tắt màu trong terminal output

  --quiet           Chỉ in kết quả, không có progress/summary

  --verbose         In thêm debug info (state count, transition count...)
```

### Ví dụ lệnh thực tế

```bash
# Cơ bản nhất
tophql routes schema.graphql --type Event

# Chỉ open routes
tophql routes schema.graphql --type Event --filter open

# Export GraphQL query templates
tophql routes schema.graphql --type Event --filter open --format graphql

# Export JSON machine-readable
tophql routes schema.graphql --type Event --out routes-event.json

# Multiple targets
tophql routes schema.graphql --type Event --type User --type Community

# Custom lexicon cho target có naming convention riêng
tophql routes schema.graphql --type Event --lexicon classifier-custom.json

# Introspection JSON input
tophql routes introspection.json --type Event
```

---

## Kiến trúc code thay đổi

### 1. Tên binary

Đổi `graphql-static-bac` → `tophql` trong `Cargo.toml`:

```toml
[[bin]]
name = "tophql"
path = "src/main.rs"
```

Giữ nguyên internal library name `graphql_static_bac`.

### 2. Bundle default configs

Embed vào binary bằng `include_str!`, không đọc file runtime:

```rust
// src/embedded.rs
pub(crate) const DEFAULT_LEXICON: &str =
    include_str!("../config/lexicons/argument-classifier-v1.json");

pub(crate) const DEFAULT_ROUTE_POLICY: &str =
    include_str!("../config/profiles/route-analysis-v1.json");
```

### 3. New `routes` command

Thêm vào `src/cli/mod.rs`:

```rust
#[derive(Debug, Args)]
pub(crate) struct RoutesArgs {
    /// Schema file: SDL (.graphql) or introspection JSON
    pub(crate) schema: PathBuf,

    /// Target type name(s) to analyze
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) types: Vec<String>,

    /// Verdict filter
    #[arg(long, value_delimiter = ',', default_value = "open,unknown")]
    pub(crate) filter: Vec<VerdictFilter>,

    /// Output format
    #[arg(long, default_value = "table")]
    pub(crate) format: OutputFormat,

    /// Output file (optional, default: stdout)
    #[arg(long)]
    pub(crate) out: Option<PathBuf>,

    /// Custom lexicon file
    #[arg(long)]
    pub(crate) lexicon: Option<PathBuf>,

    #[arg(long)]
    pub(crate) no_color: bool,

    #[arg(long)]
    pub(crate) quiet: bool,
}
```

### 4. New command handler: `src/cli/commands/routes.rs`

```rust
pub(crate) fn execute(args: RoutesArgs) -> Result<(), RoutesError> {
    // Step 1: Parse schema (S0)
    if !args.quiet { eprint!("Parsing schema..."); }
    let schema = parse_schema_auto(&args.schema)?;

    // Step 2: Classify arguments (S2) — dùng bundled lexicon nếu không có --lexicon
    if !args.quiet { eprint!(" Classifying arguments..."); }
    let policy = match &args.lexicon {
        Some(path) => read_argument_policy(path)?,
        None       => default_argument_policy(),
    };
    let arguments = classify_arguments(&schema.data, &policy)?;

    // Step 3: Route analysis (S3) — dùng bundled route policy
    if !args.quiet { eprint!(" Analyzing routes...\n"); }
    let route_policy = default_route_policy();
    let facts = RouteFacts::build(&schema.data, &arguments, &route_policy.policy)?;
    let graph = build_type_graph(&schema.data)?;

    let mut all_target_routes = Vec::new();
    for type_name in &args.types {
        let type_id = format!("type:{type_name}");
        let target_routes = analyze_target(&graph, &type_id, &facts)?;
        all_target_routes.push((type_name.clone(), target_routes));
    }

    // Step 4: Filter + format + output
    let filtered = filter_routes(&all_target_routes, &args.filter);
    let output = format_output(&filtered, &args.format, !args.no_color);

    match &args.out {
        Some(path) => fs::write(path, output)?,
        None       => println!("{output}"),
    }
    Ok(())
}
```

### 5. Helper functions cần viết

| Function | Vị trí | Việc làm |
|---|---|---|
| `parse_schema_auto(path)` | `src/schema/mod.rs` | Detect SDL vs JSON, parse, wrap Envelope |
| `default_argument_policy()` | `src/argument/policy.rs` | Parse từ `DEFAULT_LEXICON` const |
| `default_route_policy()` | `src/route/policy.rs` | Parse từ `DEFAULT_ROUTE_POLICY` const |
| `filter_routes(...)` | `src/cli/commands/routes.rs` | Filter theo verdict |
| `format_output(...)` | `src/cli/formatter/` | Dispatch đến table/json/graphql/md formatter |

---

## Output formatters cần viết

### `src/cli/formatter/table.rs` — Terminal table

Chỉ dùng standard chars, không cần crate màu nếu không muốn thêm dependency:

```
Analyzing Event in schema.graphql...
752 types  ·  2253 arguments classified  ·  313 routes found

OPEN (6)
───────────────────────────────────────────────────────────────────────
  #   Entry field        Selector                       Selector type
  1   Query.event        Query.event.id                 ID!
  2   Query.events       Query.events.ids               [String!]
  3   Query.events       Query.events.slugs             [String!]
  4   Query.events       Query.events.filters.ids       [ID!]
  5   Query.events       Query.events.filters.communityIds  [ID!]
  6   Query.events       Query.events.filters.slugs     [String!]

UNKNOWN (307)  — use --filter unknown to expand
  Top 5 by terminal field:
  · field:Document.event (47 routes)
  · field:EventsConnection.nodes (38 routes)
  · field:EventPerson.events (21 routes)
  · ...

──────────────────────────────────────────────────
  Total: 6 open  ·  307 unknown  ·  0 guarded
```

### `src/cli/formatter/graphql.rs` — Query templates

```graphql
# [OPEN] Route 1 — Query.event(id: ID!)
# Selector: id directly identifies Event
query TestEvent_1($id: ID!) {
  event(id: $id) {
    __typename
    id
  }
}

# [OPEN] Route 4 — Query.events(filters: { ids: [ID!] })
# Selector: filters.ids (nested input)
query TestEvent_4($ids: [ID!]) {
  events(filters: { ids: $ids }) {
    nodes {
      __typename
      id
    }
  }
}
```

### `src/cli/formatter/json.rs` — Simplified JSON

Không phải full S3 contract, chỉ những field cần cho người đọc:

```json
{
  "schema": "schema.graphql",
  "target": "Event",
  "total": 313,
  "summary": { "open": 6, "unknown": 307, "guarded": 0 },
  "routes": [
    {
      "verdict": "open",
      "origin": "traversal",
      "entry_field": "Query.event",
      "path": "Query.event -> Event",
      "selector": {
        "arg_path": "Query.event.id",
        "type": "ID!",
        "nested": false
      }
    }
  ]
}
```

### `src/cli/formatter/md.rs` — Markdown report

```markdown
# Route Analysis: Event

**Schema:** schema.graphql  
**Date:** 2026-06-16

## Summary

| Verdict | Count |
|---------|------:|
| open    | 6     |
| unknown | 307   |
| guarded | 0     |

## Open Routes

These routes provide **direct object access** via a selector argument.

### 1. `Query.event(id: ID!)`

- **Path:** `Query.event → Event`
- **Selector:** `id` (type: `ID!`)
- **Test query:**
  ```graphql
  query { event(id: "<EVENT_ID>") { __typename id } }
  ```
...
```

---

## Giữ nguyên gì

- Tất cả internal stages (S0, S2, S3) không thay đổi
- `stage s0/s2/s3/s4` commands giữ nguyên cho power users
- `route` command hiện tại giữ nguyên (backward compat)
- Artifact format JSON (S3 v2.2) không thay đổi

---

## Thứ tự implement

1. `src/embedded.rs` — bundle DEFAULT_LEXICON, DEFAULT_ROUTE_POLICY
2. `default_argument_policy()` / `default_route_policy()` helpers
3. `parse_schema_auto()` — detect format, return schema envelope
4. `RoutesArgs` struct trong CLI
5. `execute()` handler — wire S0+S2+S3
6. `formatter/table.rs` — terminal output (priority: người dùng thấy ngay)
7. `formatter/graphql.rs` — query templates
8. `formatter/json.rs` — simplified JSON
9. `formatter/md.rs` — markdown report
10. Rename binary → `tophql`
11. Update tests
