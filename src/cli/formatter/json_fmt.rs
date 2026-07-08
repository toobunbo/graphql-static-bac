use crate::contracts::Route;

pub(crate) fn render(filtered: &[(String, Vec<&Route>)]) -> String {
    let targets: Vec<serde_json::Value> = filtered
        .iter()
        .map(|(type_name, routes)| {
            let route_values: Vec<serde_json::Value> = routes
                .iter()
                .map(|r| {
                    let sel = r.selector.as_ref().map(|s| {
                        serde_json::json!({
                            "arg_path": s.arg_path,
                            "type": s.type_ref.display,
                            "nested": !s.input_path.is_empty(),
                            "input_path": s.input_path,
                        })
                    });
                    serde_json::json!({
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
                })
                .collect();
            serde_json::json!({
                "type": type_name,
                "total": routes.len(),
                "routes": route_values,
            })
        })
        .collect();

    let output = serde_json::json!({ "targets": targets });
    serde_json::to_string_pretty(&output).unwrap_or_default()
}
