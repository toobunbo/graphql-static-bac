use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use super::abstract_automaton::{AbstractAutomaton, RecordedTransition, StateRef};

// ---------------------------------------------------------------------------
// Stable ID helpers
// ---------------------------------------------------------------------------

pub(crate) fn state_id(state_ref: &StateRef) -> String {
    let canonical = serde_json::to_vec(state_ref).expect("StateRef serializes");
    format!("state:sha256:{:x}", Sha256::digest(canonical))
}

pub(crate) fn selector_set_id(sorted_selector_ids: &[String]) -> String {
    let canonical =
        serde_json::to_vec(sorted_selector_ids).expect("selector set serializes");
    format!("selset:sha256:{:x}", Sha256::digest(canonical))
}

fn component_id_from_members(sorted_member_state_ids: &[String]) -> String {
    let canonical =
        serde_json::to_vec(sorted_member_state_ids).expect("component id serializes");
    format!("cmp:sha256:{:x}", Sha256::digest(canonical))
}

fn dag_id_hash(
    target_type_id: &str,
    selector_mode: &str,
    sorted_component_ids: &[String],
    sorted_component_edge_keys: &[String],
) -> String {
    let canonical = serde_json::to_vec(&(
        target_type_id,
        selector_mode,
        sorted_component_ids,
        sorted_component_edge_keys,
    ))
    .expect("dag id serializes");
    format!("dag:sha256:{:x}", Sha256::digest(canonical))
}

// ---------------------------------------------------------------------------
// DAG component types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct DagComponent {
    #[allow(dead_code)]
    pub component_id: String,
    /// Sorted stable state IDs in this SCC.
    pub member_state_ids: Vec<String>,
    /// Transition IDs of edges whose source and target are both in this component.
    pub internal_transition_ids: Vec<String>,
    /// True if component has multiple states or a self-loop transition.
    pub is_cycle_capable: bool,
    /// True if every member state is a target terminal.
    pub is_terminal: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DagComponentEdge {
    pub source_component_id: String,
    pub target_component_id: String,
    pub transition_id: String,
}

// ---------------------------------------------------------------------------
// RouteFamilyDag
// ---------------------------------------------------------------------------

/// Compact SCC-condensed DAG built from an `AbstractAutomaton`.
#[derive(Debug)]
pub(crate) struct RouteFamilyDag {
    pub dag_id: String,
    #[allow(dead_code)]
    pub target_type_id: String,
    /// Component ID of the root (entry) state.
    pub entry_component_id: String,
    /// All abstract states: state_id → StateRef.
    pub states: BTreeMap<String, StateRef>,
    /// All transitions: transition_id → RecordedTransition.
    pub transitions: BTreeMap<String, RecordedTransition>,
    /// Interned selector sets: selector_set_id → sorted selector_ids.
    pub selector_sets: BTreeMap<String, Vec<String>>,
    /// SCC components: component_id → DagComponent.
    pub components: BTreeMap<String, DagComponent>,
    /// Inter-component edges (sorted canonically).
    pub component_edges: Vec<DagComponentEdge>,
    /// state_id → component_id for every state.
    pub state_to_component: BTreeMap<String, String>,
    /// Component IDs that are target-terminal (all members are terminals).
    pub terminal_component_ids: BTreeSet<String>,
    /// Per-component sorted indices into `component_edges` for outgoing edges (used by expander).
    #[allow(dead_code)]
    pub component_outgoing: BTreeMap<String, Vec<usize>>,
}

impl RouteFamilyDag {
    pub(crate) fn build(automaton: &AbstractAutomaton, target_type_id: &str) -> Self {
        if !automaton.is_query_reachable() {
            return Self::empty(target_type_id);
        }

        // Index all states
        let states_vec: Vec<StateRef> = automaton.all_states().iter().cloned().collect();
        let n = states_vec.len();
        let state_to_idx: BTreeMap<&StateRef, usize> =
            states_vec.iter().enumerate().map(|(i, s)| (s, i)).collect();

        // Build forward and reverse adjacency (transition-level)
        // adj[i] = sorted list of reachable state indices
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut radj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for t in automaton.all_transitions_map().values() {
            let src = state_to_idx[&t.source];
            let tgt = state_to_idx[&t.target];
            adj[src].push(tgt);
            radj[tgt].push(src);
        }
        for v in adj.iter_mut().chain(radj.iter_mut()) {
            v.sort_unstable();
            v.dedup();
        }

        // Kosaraju step 1: forward DFS in sorted state order, record finish order.
        let sorted_start: Vec<usize> = {
            let mut indexed: Vec<(StateRef, usize)> =
                states_vec.iter().cloned().zip(0..).collect();
            indexed.sort_by(|(a, _), (b, _)| a.cmp(b));
            indexed.into_iter().map(|(_, i)| i).collect()
        };
        let mut visited = vec![false; n];
        let mut finish = Vec::with_capacity(n);
        for start in sorted_start {
            if !visited[start] {
                dfs_forward(start, &adj, &mut visited, &mut finish);
            }
        }

        // Kosaraju step 2: reverse DFS in reverse finish order.
        let mut component_of: Vec<usize> = vec![usize::MAX; n];
        let mut scc_members: Vec<Vec<usize>> = Vec::new();
        let mut rev_visited = vec![false; n];
        for &start in finish.iter().rev() {
            if !rev_visited[start] {
                let mut members = Vec::new();
                dfs_reverse(start, &radj, &mut rev_visited, &mut members);
                members.sort_unstable();
                let cmp_idx = scc_members.len();
                for &m in &members {
                    component_of[m] = cmp_idx;
                }
                scc_members.push(members);
            }
        }

        // Build state_id lookup
        let mut state_id_of: Vec<String> = vec![String::new(); n];
        let mut states: BTreeMap<String, StateRef> = BTreeMap::new();
        for (idx, sr) in states_vec.iter().enumerate() {
            let sid = state_id(sr);
            state_id_of[idx] = sid.clone();
            states.insert(sid, sr.clone());
        }

        // Identify terminal states
        let terminal_state_ids: BTreeSet<String> = automaton
            .terminal_states()
            .iter()
            .map(state_id)
            .collect();

        // Build components
        let mut components: BTreeMap<String, DagComponent> = BTreeMap::new();
        let mut state_to_component: BTreeMap<String, String> = BTreeMap::new();
        let mut terminal_component_ids: BTreeSet<String> = BTreeSet::new();
        let mut component_id_of_scc: Vec<String> = Vec::with_capacity(scc_members.len());

        for members in &scc_members {
            let mut sorted_sids: Vec<String> =
                members.iter().map(|&i| state_id_of[i].clone()).collect();
            sorted_sids.sort();
            let cid = component_id_from_members(&sorted_sids);
            component_id_of_scc.push(cid.clone());

            for sid in &sorted_sids {
                state_to_component.insert(sid.clone(), cid.clone());
            }

            let is_terminal = sorted_sids
                .iter()
                .all(|sid| terminal_state_ids.contains(sid));
            if is_terminal {
                terminal_component_ids.insert(cid.clone());
            }

            // Cycle-capable: multiple members or a self-loop edge
            let is_cycle_capable = members.len() > 1
                || members.iter().any(|&m| adj[m].contains(&m));

            components.insert(
                cid.clone(),
                DagComponent {
                    component_id: cid,
                    member_state_ids: sorted_sids,
                    internal_transition_ids: Vec::new(),
                    is_cycle_capable,
                    is_terminal,
                },
            );
        }

        // Classify transitions and intern selector sets
        let mut transitions: BTreeMap<String, RecordedTransition> = BTreeMap::new();
        let mut selector_sets: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut component_edges: Vec<DagComponentEdge> = Vec::new();
        let mut internal_by_component: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for t in automaton.all_transitions_map().values() {
            let src_idx = state_to_idx[&t.source];
            let tgt_idx = state_to_idx[&t.target];
            let src_cid = &component_id_of_scc[component_of[src_idx]];
            let tgt_cid = &component_id_of_scc[component_of[tgt_idx]];

            if !t.selector_generator_ids.is_empty() {
                let mut sorted_ids = t.selector_generator_ids.clone();
                sorted_ids.sort();
                let ssid = selector_set_id(&sorted_ids);
                selector_sets.entry(ssid).or_insert(sorted_ids);
            }

            transitions.insert(t.transition_id.clone(), t.clone());

            if src_cid == tgt_cid {
                internal_by_component
                    .entry(src_cid.clone())
                    .or_default()
                    .push(t.transition_id.clone());
            } else {
                component_edges.push(DagComponentEdge {
                    source_component_id: src_cid.clone(),
                    target_component_id: tgt_cid.clone(),
                    transition_id: t.transition_id.clone(),
                });
            }
        }

        // Fill sorted internal transitions into components
        for (cid, mut tids) in internal_by_component {
            tids.sort();
            if let Some(c) = components.get_mut(&cid) {
                c.internal_transition_ids = tids;
            }
        }

        // Sort component edges canonically
        component_edges.sort_by(|a, b| {
            a.source_component_id
                .cmp(&b.source_component_id)
                .then_with(|| a.target_component_id.cmp(&b.target_component_id))
                .then_with(|| a.transition_id.cmp(&b.transition_id))
        });

        // Build per-component outgoing edge indices
        let mut component_outgoing: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, edge) in component_edges.iter().enumerate() {
            component_outgoing
                .entry(edge.source_component_id.clone())
                .or_default()
                .push(i);
        }

        // Find entry component from root state
        let entry_component_id = automaton
            .root()
            .map(|root| {
                let idx = state_to_idx[root];
                component_id_of_scc[component_of[idx]].clone()
            })
            .unwrap_or_default();

        // Compute dag_id
        let mut sorted_cids: Vec<String> = components.keys().cloned().collect();
        sorted_cids.sort();
        let edge_keys: Vec<String> = component_edges
            .iter()
            .map(|e| {
                format!(
                    "{}->{}:{}",
                    e.source_component_id, e.target_component_id, e.transition_id
                )
            })
            .collect();
        let did = dag_id_hash(target_type_id, "definite_only", &sorted_cids, &edge_keys);

        Self {
            dag_id: did,
            target_type_id: target_type_id.to_string(),
            entry_component_id,
            states,
            transitions,
            selector_sets,
            components,
            component_edges,
            state_to_component,
            terminal_component_ids,
            component_outgoing,
        }
    }

    pub(crate) fn is_reachable(&self) -> bool {
        !self.terminal_component_ids.is_empty()
    }

    fn empty(target_type_id: &str) -> Self {
        let did = dag_id_hash(target_type_id, "definite_only", &[], &[]);
        Self {
            dag_id: did,
            target_type_id: target_type_id.to_string(),
            entry_component_id: String::new(),
            states: BTreeMap::new(),
            transitions: BTreeMap::new(),
            selector_sets: BTreeMap::new(),
            components: BTreeMap::new(),
            component_edges: Vec::new(),
            state_to_component: BTreeMap::new(),
            terminal_component_ids: BTreeSet::new(),
            component_outgoing: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Kosaraju helpers (iterative, no recursion limit concerns)
// ---------------------------------------------------------------------------

fn dfs_forward(
    start: usize,
    adj: &[Vec<usize>],
    visited: &mut Vec<bool>,
    finish: &mut Vec<usize>,
) {
    visited[start] = true;
    let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
    while let Some((node, edge_idx)) = stack.last_mut() {
        let node = *node;
        let neighbors = &adj[node];
        if *edge_idx < neighbors.len() {
            let next = neighbors[*edge_idx];
            *edge_idx += 1;
            if !visited[next] {
                visited[next] = true;
                stack.push((next, 0));
            }
        } else {
            stack.pop();
            finish.push(node);
        }
    }
}

fn dfs_reverse(
    start: usize,
    radj: &[Vec<usize>],
    visited: &mut Vec<bool>,
    members: &mut Vec<usize>,
) {
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        if visited[node] {
            continue;
        }
        visited[node] = true;
        members.push(node);
        for &next in &radj[node] {
            if !visited[next] {
                stack.push(next);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::contracts::{ArgumentClassification, ClassifiedArgument, Confidence, SelectorClass};
    use crate::graph::{build_type_graph, PlumbingIndex};
    use crate::route::facts::{RouteFacts, SelectorFact};
    use crate::schema::parse_sdl;

    fn empty_facts(schema: &crate::contracts::SchemaIrData) -> RouteFacts {
        use std::collections::{BTreeMap, BTreeSet};
        RouteFacts {
            selectors_by_field: BTreeMap::new(),
            selectors_by_ref: BTreeMap::new(),
            selectors_by_id: BTreeMap::new(),
            boundaries_by_field: BTreeMap::new(),
            plumbing: PlumbingIndex::build(schema),
            global_id_fields: BTreeSet::new(),
        }
    }

    fn selector_fact(
        schema: &crate::contracts::SchemaIrData,
        owner: &str,
        field_name: &str,
        arg_name: &str,
    ) -> SelectorFact {
        let field = &schema.types[owner].fields[field_name];
        let arg = field
            .arguments
            .iter()
            .find(|a| a.name == arg_name)
            .unwrap();
        let classified = ClassifiedArgument {
            arg_ref: arg.arg_id.clone(),
            root_arg_ref: arg.arg_id.clone(),
            arg_path: format!("{owner}.{field_name}.{arg_name}"),
            input_path: Vec::new(),
            type_ref: arg.type_ref.clone(),
            classifications: vec![ArgumentClassification::ObjectSelector],
            signals: vec!["test".to_string()],
            confidence: Confidence::High,
        };
        SelectorFact {
            selector_id: arg.arg_id.clone(),
            argument: classified,
            selected_type_id: format!("type:{}", field.return_type.named_type),
            class: SelectorClass::Definite,
            required: true,
        }
    }

    #[test]
    fn component_graph_is_acyclic_for_linear_schema() {
        let schema = parse_sdl(
            r#"
            type Query { a: A }
            type A { b: B }
            type B { target: Target }
            type Target { id: ID! }
            "#,
        )
        .unwrap()
        .data;
        let graph = build_type_graph(&schema).unwrap();
        let facts = empty_facts(&schema);
        let automaton =
            crate::route::abstract_automaton::AbstractAutomaton::propagate(
                &graph, "type:Target", &facts,
            )
            .unwrap();
        let dag = RouteFamilyDag::build(&automaton, "type:Target");

        assert!(dag.is_reachable());
        // Verify component graph is acyclic: no component appears in its own
        // transitive reach via component_edges.
        assert_no_component_cycle(&dag);
    }

    #[test]
    fn shared_suffix_stored_once_three_families_expand() {
        // B->Hub->Target, C->Hub->Target, D->Hub->Target
        // Each B/C/D introduces a distinct terminal_semantic_edge_id for the Hub
        // abstract state, so Hub has 3 variants.  However, all three Hub paths
        // converge to the SAME Target abstract state (terminal_semantic_edge_id
        // = "field:Hub.target" for all).  That shared terminal is stored once.
        let schema = parse_sdl(
            r#"
            type Query { b: B, c: C, d: D }
            type B { hub: Hub }
            type C { hub: Hub }
            type D { hub: Hub }
            type Hub { target: Target }
            type Target { id: ID! }
            "#,
        )
        .unwrap()
        .data;
        let graph = build_type_graph(&schema).unwrap();
        let facts = empty_facts(&schema);
        let automaton =
            crate::route::abstract_automaton::AbstractAutomaton::propagate(
                &graph, "type:Target", &facts,
            )
            .unwrap();
        let dag = RouteFamilyDag::build(&automaton, "type:Target");

        assert!(dag.is_reachable());

        // The Target terminal state is shared: all paths end at the same abstract
        // state (origin Traversal, terminal_semantic_edge Hub.target).
        let target_states: Vec<_> = dag
            .states
            .values()
            .filter(|s| s.type_id == "type:Target")
            .collect();
        assert_eq!(
            target_states.len(),
            1,
            "shared Target terminal stored exactly once"
        );

        // Hub has 3 distinct abstract states (one per entry field: B.hub, C.hub, D.hub)
        // because terminal_semantic_edge_id differs per entry.
        let hub_states: Vec<_> = dag
            .states
            .values()
            .filter(|s| s.type_id == "type:Hub")
            .collect();
        assert_eq!(hub_states.len(), 3, "each entry creates a distinct Hub state");

        // The terminal component is referenced by 3 different component edges.
        let terminal_cid = dag
            .state_to_component
            .get(&crate::route::dag::state_id(target_states[0]))
            .unwrap();
        let incoming_to_terminal: Vec<_> = dag
            .component_edges
            .iter()
            .filter(|e| &e.target_component_id == terminal_cid)
            .collect();
        assert_eq!(
            incoming_to_terminal.len(),
            3,
            "three component edges converge to the shared Target component"
        );

        assert_no_component_cycle(&dag);
    }

    #[test]
    fn terminal_components_have_no_outgoing_component_edges() {
        let schema = parse_sdl(
            r#"
            type Query { a: A }
            type A { target: Target }
            type Target { next: Target }
            "#,
        )
        .unwrap()
        .data;
        let graph = build_type_graph(&schema).unwrap();
        let facts = empty_facts(&schema);
        let automaton =
            crate::route::abstract_automaton::AbstractAutomaton::propagate(
                &graph, "type:Target", &facts,
            )
            .unwrap();
        let dag = RouteFamilyDag::build(&automaton, "type:Target");

        for terminal_cid in &dag.terminal_component_ids {
            assert!(
                dag.component_outgoing
                    .get(terminal_cid)
                    .is_none_or(Vec::is_empty),
                "terminal component {terminal_cid} has outgoing component edges"
            );
        }
    }

    #[test]
    fn dag_id_is_deterministic() {
        let schema = parse_sdl(
            r#"
            type Query { a: A, b: B }
            type A { target: Target }
            type B { target: Target }
            type Target { id: ID! }
            "#,
        )
        .unwrap()
        .data;
        let graph = build_type_graph(&schema).unwrap();
        let facts = empty_facts(&schema);
        let a1 = crate::route::abstract_automaton::AbstractAutomaton::propagate(
            &graph, "type:Target", &facts,
        )
        .unwrap();
        let a2 = crate::route::abstract_automaton::AbstractAutomaton::propagate(
            &graph, "type:Target", &facts,
        )
        .unwrap();
        let d1 = RouteFamilyDag::build(&a1, "type:Target");
        let d2 = RouteFamilyDag::build(&a2, "type:Target");
        assert_eq!(d1.dag_id, d2.dag_id);
        assert_eq!(d1.entry_component_id, d2.entry_component_id);
        assert_eq!(d1.components.len(), d2.components.len());
    }

    #[test]
    fn selector_sets_are_interned() {
        let schema = parse_sdl(
            r#"
            type Query { item(id: ID!): Target }
            type Target { id: ID! }
            "#,
        )
        .unwrap()
        .data;
        let graph = build_type_graph(&schema).unwrap();
        let sf = selector_fact(&schema, "Query", "item", "id");
        let mut facts = empty_facts(&schema);
        facts.selectors_by_field.insert(
            "field:Query.item".to_string(),
            vec![sf.clone()],
        );
        facts.selectors_by_id.insert(sf.selector_id.clone(), sf);
        let automaton =
            crate::route::abstract_automaton::AbstractAutomaton::propagate(
                &graph, "type:Target", &facts,
            )
            .unwrap();
        let dag = RouteFamilyDag::build(&automaton, "type:Target");

        // There should be exactly one selector set (the one for `id`)
        assert_eq!(dag.selector_sets.len(), 1);
    }

    // Utility: assert the component graph has no cycles via DFS.
    pub(crate) fn assert_no_component_cycle(dag: &RouteFamilyDag) {
        // State: 0=unvisited, 1=in_stack, 2=done
        let cids: Vec<&String> = dag.components.keys().collect();
        let cid_to_idx: BTreeMap<&String, usize> =
            cids.iter().enumerate().map(|(i, c)| (*c, i)).collect();
        let mut color = vec![0u8; cids.len()];

        fn visit(
            cid_idx: usize,
            cids: &[&String],
            outgoing: &BTreeMap<String, Vec<usize>>,
            edges: &[DagComponentEdge],
            cid_to_idx: &BTreeMap<&String, usize>,
            color: &mut Vec<u8>,
        ) {
            if color[cid_idx] == 2 {
                return;
            }
            assert_ne!(color[cid_idx], 1, "cycle detected in component graph");
            color[cid_idx] = 1;
            if let Some(edge_indices) = outgoing.get(cids[cid_idx]) {
                for &ei in edge_indices {
                    let tgt = cid_to_idx[&edges[ei].target_component_id];
                    visit(tgt, cids, outgoing, edges, cid_to_idx, color);
                }
            }
            color[cid_idx] = 2;
        }

        for i in 0..cids.len() {
            visit(
                i,
                &cids,
                &dag.component_outgoing,
                &dag.component_edges,
                &cid_to_idx,
                &mut color,
            );
        }
    }
}
