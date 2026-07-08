use std::collections::BTreeMap;

use crate::contracts::{Route, RouteVerdict, TargetRoutes};

const GREEN: &str = "\x1b[32;1m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const RESET: &str = "\x1b[0m";

pub(crate) fn render(
    filtered: &[(String, Vec<&Route>)],
    all: &[(String, TargetRoutes)],
    color: bool,
) -> String {
    let mut out = String::new();

    for ((type_name, routes), (_, target)) in filtered.iter().zip(all.iter()) {
        let total = target.routes.len();
        let n_open = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Open).count();
        let n_unknown = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Unknown).count();
        let n_guarded = target.routes.iter().filter(|r| r.verdict == RouteVerdict::Guarded).count();

        out.push_str(&format!(
            "\n{total} routes to {type_name}  \
             ({n_open} open · {n_unknown} unknown · {n_guarded} guarded)\n"
        ));

        // OPEN — always show all
        let open: Vec<_> = routes.iter().filter(|r| r.verdict == RouteVerdict::Open).collect();
        if !open.is_empty() {
            out.push_str(&section_header("OPEN", open.len(), color));
            out.push_str(&format!(
                "  {:<4}  {:<35}  {:<42}  {}\n",
                "#", "Entry field", "Selector", "Type"
            ));
            out.push_str(&format!("  {}\n", "─".repeat(100)));
            for (i, route) in open.iter().enumerate() {
                let entry = route.witness.entry_field_id
                    .trim_start_matches("field:")
                    .to_string();
                let (sel_path, sel_type) = match &route.selector {
                    Some(s) => (s.arg_path.clone(), s.type_ref.display.clone()),
                    None => ("—".to_string(), "—".to_string()),
                };
                out.push_str(&format!(
                    "  {:<4}  {:<35}  {:<42}  {}\n",
                    i + 1,
                    entry,
                    sel_path,
                    sel_type,
                ));
            }
        }

        // UNKNOWN — summary by terminal, full list if explicitly requested
        let unknown: Vec<_> =
            routes.iter().filter(|r| r.verdict == RouteVerdict::Unknown).collect();
        if !unknown.is_empty() {
            if routes.iter().any(|r| r.verdict == RouteVerdict::Open) {
                // Mixed: only show summary for unknown
                out.push_str(&section_header("UNKNOWN", unknown.len(), color));
                out.push_str("  (use --filter unknown to expand)\n\n");
                render_unknown_summary(&unknown, &mut out);
            } else {
                // Unknown-only filter: show full list, two-line layout
                out.push_str(&section_header("UNKNOWN", unknown.len(), color));
                for (i, route) in unknown.iter().enumerate().take(50) {
                    let path = &route.witness.display_projection;
                    let sel = route.selector.as_ref()
                        .map(|s| s.arg_path.as_str())
                        .unwrap_or("—");
                    out.push_str(&format!("  [{:>3}]  {}\n", i + 1, path));
                    out.push_str(&format!("         selector: {}\n\n", sel));
                }
                if unknown.len() > 50 {
                    out.push_str(&format!("  ... and {} more routes\n", unknown.len() - 50));
                }
            }
        }

        // GUARDED — always show if present
        let guarded: Vec<_> =
            routes.iter().filter(|r| r.verdict == RouteVerdict::Guarded).collect();
        if !guarded.is_empty() {
            out.push_str(&section_header("GUARDED", guarded.len(), color));
            out.push_str(&format!("  {:<4}  {:<40}  {}\n", "#", "Path", "Boundary"));
            out.push_str(&format!("  {}\n", "─".repeat(80)));
            for (i, route) in guarded.iter().enumerate() {
                let path = route.witness.display_projection
                    .chars().take(40).collect::<String>();
                let bounds: Vec<_> = route.signature.boundary_families.iter()
                    .map(|b| format!("{b:?}").to_lowercase())
                    .collect();
                out.push_str(&format!(
                    "  {:<4}  {:<40}  {}\n",
                    i + 1, path, bounds.join(", ")
                ));
            }
        }

        if !open.is_empty() || !unknown.is_empty() || !guarded.is_empty() {
            out.push_str(&format!("\n  Total: {n_open} open · {n_unknown} unknown · {n_guarded} guarded\n"));
        }
    }

    out
}

fn render_unknown_summary(unknown: &[&&Route], out: &mut String) {
    let mut by_terminal: BTreeMap<&str, usize> = BTreeMap::new();
    for r in unknown {
        *by_terminal
            .entry(r.terminal_semantic_edge_id.trim_start_matches("field:"))
            .or_default() += 1;
    }
    let mut groups: Vec<_> = by_terminal.into_iter().collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1));
    out.push_str("  Top entry points:\n");
    for (terminal, count) in groups.iter().take(5) {
        out.push_str(&format!("  {:>5}  via {terminal}\n", count));
    }
    if groups.len() > 5 {
        out.push_str(&format!("         ... and {} more entry points\n", groups.len() - 5));
    }
    out.push('\n');
}

fn section_header(label: &str, count: usize, color: bool) -> String {
    let colored = if color {
        match label {
            "OPEN" => format!("{GREEN}{label}{RESET}"),
            "UNKNOWN" => format!("{YELLOW}{label}{RESET}"),
            "GUARDED" => format!("{BLUE}{label}{RESET}"),
            _ => label.to_string(),
        }
    } else {
        label.to_string()
    };
    format!("\n{colored} ({count})\n  {}\n", "─".repeat(100))
}
