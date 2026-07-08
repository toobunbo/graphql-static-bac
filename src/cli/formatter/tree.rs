use std::collections::BTreeMap;

use crate::contracts::{Route, RouteVerdict};

/// Render routes as a reversed DAG tree, grouping shared path suffixes.
///
/// The target type is the root. Each level shows which field leads into
/// the next level, collapsing routes that share a common tail.
/// `max_depth`: max levels to expand (0 = unlimited).
pub(crate) fn render(type_name: &str, routes: &[&Route], color: bool, max_depth: usize) -> String {
    let mut out = String::new();

    let open_count = routes.iter().filter(|r| r.verdict == RouteVerdict::Open).count();
    let unknown_count = routes.iter().filter(|r| r.verdict == RouteVerdict::Unknown).count();
    let guarded_count = routes.iter().filter(|r| r.verdict == RouteVerdict::Guarded).count();

    out.push_str(&format!(
        "\n{type_name}  ({open_count} open · {unknown_count} unknown · {guarded_count} guarded)\n"
    ));

    // Build reversed path segments for each route.
    // Each route becomes a Vec<String> from terminal → root (reversed).
    let trees: Vec<ReversedPath> = routes
        .iter()
        .map(|r| build_reversed_path(r))
        .collect();

    // Group into a tree by common reversed-path prefixes.
    let root = build_tree(&trees);

    // Render the tree
    let limit = if max_depth == 0 { usize::MAX } else { max_depth };
    render_node(&root, &mut out, "", true, color, 0, limit);

    out
}

// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ReversedPath {
    /// Field hops from terminal → root (reversed witness, field edges only)
    hops: Vec<String>,
    verdict: RouteVerdict,
    /// Selectors relevant to this route, for annotation at the leaf
    selector_labels: Vec<String>,
}

fn build_reversed_path(route: &Route) -> ReversedPath {
    // Extract field names from witness edges (skip type conditions)
    let field_hops: Vec<String> = route
        .witness
        .edges
        .iter()
        .filter(|e| e.kind == crate::contracts::PathEdgeKind::Field)
        .filter_map(|e| {
            e.field_id
                .as_deref()
                .and_then(|fid| fid.rsplit('.').next())
                .map(|name| {
                    let owner = e.source_type_id.trim_start_matches("type:");
                    format!("{owner}.{name}")
                })
        })
        .collect();

    // Reversed: terminal field is first
    let mut hops = field_hops;
    hops.reverse();

    // Selector label for leaf annotation
    let selector_labels = route
        .selector
        .as_ref()
        .map(|s| {
            let leaf = s.arg_path.rsplit('.').next().unwrap_or(&s.arg_path);
            vec![format!("{} ({})", leaf, s.type_ref.display)]
        })
        .unwrap_or_default();

    ReversedPath { hops, verdict: route.verdict, selector_labels }
}

// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct TreeNode {
    /// label for this node (field hop name, e.g. "Document.event")
    label: String,
    verdict: Option<RouteVerdict>,
    /// selector annotations collected at leaf nodes
    selectors: Vec<String>,
    /// child nodes keyed by their label
    children: BTreeMap<String, TreeNode>,
}

fn build_tree(paths: &[ReversedPath]) -> TreeNode {
    let mut root = TreeNode::default();
    for path in paths {
        insert_path(&mut root, path, 0);
    }
    root
}

fn insert_path(node: &mut TreeNode, path: &ReversedPath, depth: usize) {
    if depth >= path.hops.len() {
        // Leaf: record verdict + selectors
        if node.verdict.is_none()
            || path.verdict < node.verdict.unwrap_or(RouteVerdict::Guarded)
        {
            node.verdict = Some(path.verdict);
        }
        for sel in &path.selector_labels {
            if !node.selectors.contains(sel) {
                node.selectors.push(sel.clone());
            }
        }
        return;
    }
    let hop = &path.hops[depth];
    let child = node.children.entry(hop.clone()).or_insert_with(|| TreeNode {
        label: hop.clone(),
        ..Default::default()
    });
    insert_path(child, path, depth + 1);
}

// ---------------------------------------------------------------------------

fn render_node(
    node: &TreeNode,
    out: &mut String,
    prefix: &str,
    is_last: bool,
    color: bool,
    depth: usize,
    max_depth: usize,
) {
    if node.label.is_empty() {
        // Root node — render children directly
        let children: Vec<_> = node.children.values().collect();
        for (i, child) in children.iter().enumerate() {
            render_node(child, out, "", i == children.len() - 1, color, 0, max_depth);
        }
        return;
    }

    let connector = if is_last { "└── " } else { "├── " };
    let child_prefix = if is_last { "    " } else { "│   " };

    // Verdict badge
    let badge = match node.verdict {
        Some(RouteVerdict::Open) => {
            if color { "\x1b[32;1m[OPEN]\x1b[0m " } else { "[OPEN] " }
        }
        Some(RouteVerdict::Unknown) => {
            if color { "\x1b[33m[UNKNOWN]\x1b[0m " } else { "" }
        }
        Some(RouteVerdict::Guarded) => {
            if color { "\x1b[34m[GUARDED]\x1b[0m " } else { "[GUARDED] " }
        }
        None => "",
    };

    // Selector annotation
    let sel_note = if !node.selectors.is_empty() {
        let joined = if node.selectors.len() <= 3 {
            node.selectors.join(" / ")
        } else {
            format!(
                "{} / {} / ... (+{})",
                node.selectors[0],
                node.selectors[1],
                node.selectors.len() - 2
            )
        };
        format!("  ← {joined}")
    } else {
        String::new()
    };

    out.push_str(&format!(
        "{prefix}{connector}{badge}{}{sel_note}\n",
        node.label
    ));

    let children: Vec<_> = node.children.values().collect();

    // Depth limit: show count of hidden subtree instead of expanding
    if depth + 1 >= max_depth && !children.is_empty() {
        let total = count_leaves(node);
        let new_prefix = format!("{prefix}{child_prefix}");
        out.push_str(&format!(
            "{new_prefix}└── ({total} more paths — use --depth {} to expand)\n",
            max_depth + 2
        ));
        return;
    }

    for (i, child) in children.iter().enumerate() {
        let new_prefix = format!("{prefix}{child_prefix}");
        render_node(child, out, &new_prefix, i == children.len() - 1, color, depth + 1, max_depth);
    }
}

fn count_leaves(node: &TreeNode) -> usize {
    if node.children.is_empty() {
        1
    } else {
        node.children.values().map(count_leaves).sum()
    }
}
