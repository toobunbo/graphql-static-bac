use std::path::Path;

use crate::contracts::{Route, RouteVerdict, TargetRoutes};

pub(crate) fn render(
    filtered: &[(String, Vec<&Route>)],
    all: &[(String, TargetRoutes)],
    schema_path: &Path,
) -> String {
    let schema_name = schema_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("schema");

    let mut out = format!("# Route Analysis\n\n");
    out.push_str(&format!("**Schema:** `{schema_name}`\n\n"));

    for ((type_name, routes), (_, target)) in filtered.iter().zip(all.iter()) {
        let n_open = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Open).count();
        let n_unknown = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Unknown).count();
        let n_guarded = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Guarded).count();

        out.push_str(&format!("## {type_name}\n\n"));
        out.push_str("| Verdict | Count |\n|---------|------:|\n");
        out.push_str(&format!("| open | {n_open} |\n"));
        out.push_str(&format!("| unknown | {n_unknown} |\n"));
        out.push_str(&format!("| guarded | {n_guarded} |\n\n"));

        // Open routes
        let open: Vec<_> = routes.iter().filter(|r| r.verdict == RouteVerdict::Open).collect();
        if !open.is_empty() {
            out.push_str("### Open Routes\n\n");
            out.push_str("These routes provide **direct object access** via a selector argument.\n\n");
            out.push_str("| # | Path | Selector | Type |\n");
            out.push_str("|---|------|----------|------|\n");
            for (i, r) in open.iter().enumerate() {
                let path = &r.witness.display_projection;
                let (sel, tref) = match &r.selector {
                    Some(s) => (s.arg_path.as_str(), s.type_ref.display.as_str()),
                    None => ("—", "—"),
                };
                out.push_str(&format!("| {} | `{path}` | `{sel}` | `{tref}` |\n", i + 1));
            }
            out.push('\n');

            // Test queries for open routes
            out.push_str("#### Test Queries\n\n");
            for (i, r) in open.iter().enumerate() {
                let entry = r.witness.entry_field_id.trim_start_matches("field:");
                out.push_str(&format!("**Route {}** — `{entry}`\n\n", i + 1));
                let query = super::graphql_fmt::render(
                    &[(type_name.clone(), vec![r])],
                );
                // Strip the header comments, just the query
                let query_only: String = query
                    .lines()
                    .filter(|l| !l.starts_with('#'))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
                out.push_str("```graphql\n");
                out.push_str(&query_only);
                out.push_str("\n```\n\n");
            }
        }

        // Unknown routes — summary
        let unknown: Vec<_> =
            routes.iter().filter(|r| r.verdict == RouteVerdict::Unknown).collect();
        if !unknown.is_empty() {
            out.push_str("### Unknown Routes\n\n");
            out.push_str(&format!(
                "**{} routes** reach `{type_name}` through traversal. \
                Access control depends on intermediate field permissions.\n\n",
                unknown.len()
            ));

            // Group by terminal
            let mut by_terminal: std::collections::BTreeMap<&str, usize> =
                std::collections::BTreeMap::new();
            for r in &unknown {
                *by_terminal
                    .entry(r.terminal_semantic_edge_id.trim_start_matches("field:"))
                    .or_default() += 1;
            }
            let mut groups: Vec<_> = by_terminal.into_iter().collect();
            groups.sort_by(|a, b| b.1.cmp(&a.1));

            out.push_str("| Count | Entry point |\n|------:|-------------|\n");
            for (terminal, count) in groups.iter().take(10) {
                out.push_str(&format!("| {count} | `{terminal}` |\n"));
            }
            if groups.len() > 10 {
                out.push_str(&format!(
                    "\n*... and {} more entry points*\n",
                    groups.len() - 10
                ));
            }
            out.push('\n');
        }

        // Guarded routes
        let guarded: Vec<_> =
            routes.iter().filter(|r| r.verdict == RouteVerdict::Guarded).collect();
        if !guarded.is_empty() {
            out.push_str("### Guarded Routes\n\n");
            out.push_str("These routes cross an auth boundary (self-scope or visibility).\n\n");
            out.push_str("| # | Path | Boundary |\n|---|------|----------|\n");
            for (i, r) in guarded.iter().enumerate() {
                let path = &r.witness.display_projection;
                let bounds: Vec<_> = r.signature.boundary_families.iter()
                    .map(|b| format!("`{}`", format!("{b:?}").to_lowercase()))
                    .collect();
                out.push_str(&format!("| {} | `{path}` | {} |\n", i + 1, bounds.join(", ")));
            }
            out.push('\n');
        }
    }
    out
}
