use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contracts::{BoundaryFamily, SelectorClass, SelectorContinuity, TypeKind};
use crate::graph::{reverse_reachable, GraphEdgeKind, TypeGraph};

use super::facts::RouteFacts;
use super::worklist::RouteAnalysisError;

const SELF_SCOPE: u8 = 1;
const VISIBILITY: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OriginMode {
    Traversal,
    GlobalIdPrefix,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct AbstractState {
    pub origin_mode: OriginMode,
    pub selector_class: SelectorClass,
    pub selector_continuity: SelectorContinuity,
    pub boundary_bits: u8,
    pub terminal_semantic_edge_id: Option<String>,
}

impl AbstractState {
    fn initial() -> Self {
        Self {
            origin_mode: OriginMode::Traversal,
            selector_class: SelectorClass::None,
            selector_continuity: SelectorContinuity::NotApplicable,
            boundary_bits: 0,
            terminal_semantic_edge_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct StateRef {
    pub type_id: String,
    pub state: AbstractState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransitionEffect {
    PassThrough,
    ActivateDefinite,
    TypeCondition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedTransition {
    pub transition_id: String,
    pub source: StateRef,
    pub target: StateRef,
    pub edge_index: usize,
    pub effect: TransitionEffect,
    pub selector_generator_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProvenanceToken {
    NoSelector,
    Selector {
        activation_transition_id: String,
        selector_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CanonicalTrace {
    pub terminal: StateRef,
    pub token: ProvenanceToken,
    pub transition_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct FieldAbstractFact {
    has_definite: bool,
    has_required: bool,
    definite_selector_ids: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct AbstractAutomaton {
    root: Option<StateRef>,
    states: BTreeSet<StateRef>,
    terminals: BTreeSet<StateRef>,
    transitions: BTreeMap<String, RecordedTransition>,
    transitions_by_source: BTreeMap<StateRef, Vec<String>>,
    phase_a_predecessors: BTreeMap<StateRef, String>,
}

impl AbstractAutomaton {
    pub(crate) fn propagate(
        graph: &TypeGraph,
        target_type_id: &str,
        facts: &RouteFacts,
    ) -> Result<Self, RouteAnalysisError> {
        validate_target(graph, target_type_id)?;
        let reachable = reverse_reachable(graph, target_type_id);
        if !reachable.contains(&graph.query_root_id) {
            return Ok(Self::empty());
        }

        let field_facts = build_field_facts(facts);
        let mut automaton = Self::empty();
        let phase_a_started = Instant::now();
        automaton.run_phase_a(graph, target_type_id, facts, &reachable, &field_facts);
        if std::env::var_os("GRAPHQL_STATIC_BAC_ROUTE_STATS").is_some() {
            eprintln!(
                "route_stats phase=a elapsed_ms={} states={} transitions={} terminals={}",
                phase_a_started.elapsed().as_millis(),
                automaton.states.len(),
                automaton.transitions.len(),
                automaton.terminals.len()
            );
        }
        Ok(automaton)
    }

    fn empty() -> Self {
        Self {
            root: None,
            states: BTreeSet::new(),
            terminals: BTreeSet::new(),
            transitions: BTreeMap::new(),
            transitions_by_source: BTreeMap::new(),
            phase_a_predecessors: BTreeMap::new(),
        }
    }

    fn run_phase_a(
        &mut self,
        graph: &TypeGraph,
        target_type_id: &str,
        facts: &RouteFacts,
        reachable: &BTreeSet<String>,
        field_facts: &BTreeMap<String, FieldAbstractFact>,
    ) {
        let root = StateRef {
            type_id: graph.query_root_id.clone(),
            state: AbstractState::initial(),
        };
        self.root = Some(root.clone());
        self.states.insert(root.clone());

        let mut pending = VecDeque::from([root]);
        while let Some(source) = pending.pop_front() {
            if source.type_id == target_type_id {
                self.terminals.insert(source);
                continue;
            }

            for edge_index in graph.outgoing(&source.type_id) {
                let edge = graph.edge(*edge_index);
                if !reachable.contains(&edge.target_type_id) {
                    continue;
                }

                for (next_state, effect, selector_generator_ids) in abstract_transitions(
                    graph,
                    &source,
                    *edge_index,
                    facts,
                    field_facts,
                ) {
                    let target = StateRef {
                        type_id: edge.target_type_id.clone(),
                        state: next_state,
                    };
                    let transition_id =
                        transition_id(&source, &target, &edge.edge_id, effect);
                    let transition = RecordedTransition {
                        transition_id: transition_id.clone(),
                        source: source.clone(),
                        target: target.clone(),
                        edge_index: *edge_index,
                        effect,
                        selector_generator_ids,
                    };
                    if self
                        .transitions
                        .insert(transition_id.clone(), transition)
                        .is_none()
                    {
                        self.transitions_by_source
                            .entry(source.clone())
                            .or_default()
                            .push(transition_id.clone());
                    }
                    if self.states.insert(target.clone()) {
                        self.phase_a_predecessors
                            .insert(target.clone(), transition_id);
                        if target.type_id == target_type_id {
                            self.terminals.insert(target);
                        } else {
                            pending.push_back(target);
                        }
                    }
                }
            }
        }

        for transition_ids in self.transitions_by_source.values_mut() {
            transition_ids.sort();
            transition_ids.dedup();
        }
    }

    pub(crate) fn is_query_reachable(&self) -> bool {
        self.root.is_some() && !self.terminals.is_empty()
    }

    pub(crate) fn canonical_traces(&self) -> Result<Vec<CanonicalTrace>, RouteAnalysisError> {
        let Some(root) = self.root.as_ref() else {
            return Ok(Vec::new());
        };
        let started = Instant::now();
        let states: Vec<_> = self.states.iter().cloned().collect();
        let state_ids: BTreeMap<_, _> = states
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, state)| (state, index))
            .collect();
        let transitions: Vec<_> = self.transitions.values().collect();
        let mut incoming = vec![Vec::new(); states.len()];
        for (index, transition) in transitions.iter().enumerate() {
            let target_id = state_ids[&transition.target];
            incoming[target_id].push(index);
        }
        let root_id = state_ids[root];
        let mut traces = Vec::new();

        for terminal in &self.terminals {
            let terminal_id = state_ids[terminal];
            let mut visited = vec![false; states.len()];
            let mut successor = vec![usize::MAX; states.len()];
            let mut selectors: BTreeMap<String, usize> = BTreeMap::new();
            let mut pending = VecDeque::from([terminal_id]);
            visited[terminal_id] = true;

            while let Some(target_id) = pending.pop_front() {
                for transition_index in &incoming[target_id] {
                    let transition = transitions[*transition_index];
                    match transition.effect {
                        TransitionEffect::PassThrough | TransitionEffect::TypeCondition => {
                            let source_id = state_ids[&transition.source];
                            if !visited[source_id] {
                                visited[source_id] = true;
                                successor[source_id] = *transition_index;
                                pending.push_back(source_id);
                            }
                        }
                        TransitionEffect::ActivateDefinite => {
                            for selector_id in &transition.selector_generator_ids {
                                selectors
                                    .entry(selector_id.clone())
                                    .or_insert(*transition_index);
                            }
                        }
                    }
                }
            }

            if visited[root_id] {
                traces.push(CanonicalTrace {
                    terminal: terminal.clone(),
                    token: ProvenanceToken::NoSelector,
                    transition_ids: reconstruct_numeric_suffix(
                        root_id,
                        terminal_id,
                        &successor,
                        &transitions,
                        &state_ids,
                    )?,
                });
            }

            for (selector_id, transition_index) in selectors {
                let activation = transitions[transition_index];
                let mut transition_ids = self.reconstruct_prefix(&activation.source)?;
                transition_ids.push(activation.transition_id.clone());
                transition_ids.extend(reconstruct_numeric_suffix(
                    state_ids[&activation.target],
                    terminal_id,
                    &successor,
                    &transitions,
                    &state_ids,
                )?);
                traces.push(CanonicalTrace {
                    terminal: terminal.clone(),
                    token: ProvenanceToken::Selector {
                        activation_transition_id: activation.transition_id.clone(),
                        selector_id,
                    },
                    transition_ids,
                });
            }
        }

        traces.sort();
        if std::env::var_os("GRAPHQL_STATIC_BAC_ROUTE_STATS").is_some() {
            eprintln!(
                "route_stats phase=provenance elapsed_ms={} canonical_traces={}",
                started.elapsed().as_millis(),
                traces.len()
            );
        }
        Ok(traces)
    }

    pub(crate) fn transition(&self, transition_id: &str) -> Option<&RecordedTransition> {
        self.transitions.get(transition_id)
    }

    fn reconstruct_prefix(&self, state: &StateRef) -> Result<Vec<String>, RouteAnalysisError> {
        let root = self
            .root
            .as_ref()
            .expect("reachable automaton has a root state");
        let mut current = state.clone();
        let mut transition_ids = Vec::new();
        let mut remaining = self.phase_a_predecessors.len() + 1;

        while &current != root {
            if remaining == 0 {
                return Err(RouteAnalysisError::InvalidProvenance(
                    "Phase A predecessor chain contains a cycle".to_string(),
                ));
            }
            remaining -= 1;
            let transition_id = self.phase_a_predecessors.get(&current).ok_or_else(|| {
                RouteAnalysisError::InvalidProvenance(format!(
                    "missing Phase A predecessor for {:?}",
                    current
                ))
            })?;
            let transition = self
                .transitions
                .get(transition_id)
                .expect("Phase A predecessor transition resolves");
            transition_ids.push(transition_id.clone());
            current = transition.source.clone();
        }
        transition_ids.reverse();
        Ok(transition_ids)
    }

    pub(crate) fn root(&self) -> Option<&StateRef> {
        self.root.as_ref()
    }

    pub(crate) fn all_states(&self) -> &BTreeSet<StateRef> {
        &self.states
    }

    pub(crate) fn terminal_states(&self) -> &BTreeSet<StateRef> {
        &self.terminals
    }

    pub(crate) fn all_transitions_map(&self) -> &BTreeMap<String, RecordedTransition> {
        &self.transitions
    }

    pub(crate) fn outgoing_transition_ids(&self, state: &StateRef) -> &[String] {
        self.transitions_by_source
            .get(state)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    #[cfg(test)]
    pub(crate) fn state_count(&self) -> usize {
        self.states.len()
    }

    #[cfg(test)]
    pub(crate) fn transition_count(&self) -> usize {
        self.transitions.len()
    }
}

fn reconstruct_numeric_suffix(
    start_id: usize,
    terminal_id: usize,
    successor: &[usize],
    transitions: &[&RecordedTransition],
    state_ids: &BTreeMap<StateRef, usize>,
) -> Result<Vec<String>, RouteAnalysisError> {
    let mut current_id = start_id;
    let mut transition_ids = Vec::new();
    let mut remaining = successor.len() + 1;

    while current_id != terminal_id {
        if remaining == 0 {
            return Err(RouteAnalysisError::InvalidProvenance(
                "passive suffix contains a cycle".to_string(),
            ));
        }
        remaining -= 1;
        let transition_index = successor[current_id];
        if transition_index == usize::MAX {
            return Err(RouteAnalysisError::InvalidProvenance(format!(
                "missing passive suffix from state {current_id} to {terminal_id}"
            )));
        }
        let transition = transitions[transition_index];
        transition_ids.push(transition.transition_id.clone());
        current_id = state_ids[&transition.target];
    }
    Ok(transition_ids)
}

fn validate_target(graph: &TypeGraph, target_type_id: &str) -> Result<(), RouteAnalysisError> {
    let target = graph
        .node(target_type_id)
        .ok_or_else(|| RouteAnalysisError::MissingTarget(target_type_id.to_string()))?;
    if !matches!(
        target.kind,
        TypeKind::Object | TypeKind::Interface | TypeKind::Union
    ) {
        return Err(RouteAnalysisError::InvalidTarget(
            target_type_id.to_string(),
        ));
    }
    Ok(())
}

fn build_field_facts(facts: &RouteFacts) -> BTreeMap<String, FieldAbstractFact> {
    let mut result = BTreeMap::new();
    for (field_id, selectors) in &facts.selectors_by_field {
        let mut fact = FieldAbstractFact::default();
        for selector in selectors {
            fact.has_required |= selector.required;
            match selector.class {
                SelectorClass::Definite => {
                    fact.has_definite = true;
                    fact.definite_selector_ids
                        .push(selector.selector_id.clone());
                }
                SelectorClass::Possible | SelectorClass::None => {}
            }
        }
        fact.definite_selector_ids.sort();
        fact.definite_selector_ids.dedup();
        result.insert(field_id.clone(), fact);
    }
    result
}

fn abstract_transitions(
    graph: &TypeGraph,
    source: &StateRef,
    edge_index: usize,
    facts: &RouteFacts,
    field_facts: &BTreeMap<String, FieldAbstractFact>,
) -> Vec<(AbstractState, TransitionEffect, Vec<String>)> {
    let edge = graph.edge(edge_index);
    if edge.kind == GraphEdgeKind::TypeCondition {
        return vec![(
            source.state.clone(),
            TransitionEffect::TypeCondition,
            Vec::new(),
        )];
    }

    let field = edge
        .field
        .as_ref()
        .expect("FIELD graph edge retains its definition");
    let field_fact = field_facts
        .get(&field.field_id)
        .cloned()
        .unwrap_or_default();
    let plumbing = facts.plumbing.is_plumbing(&field.field_id);
    let boundary_add = boundary_bits(facts, &field.field_id);
    let origin_mode = next_origin_mode(graph, source, &edge.edge_id, facts);
    let mut result = Vec::new();

    if !field_fact.has_required {
        let mut next = source.state.clone();
        next.origin_mode = origin_mode;
        next.boundary_bits |= boundary_add;
        if !plumbing {
            next.terminal_semantic_edge_id = Some(field.field_id.clone());
            next.selector_continuity = if next.selector_class == SelectorClass::None {
                SelectorContinuity::NotApplicable
            } else {
                SelectorContinuity::Unknown
            };
        }
        result.push((next, TransitionEffect::PassThrough, Vec::new()));
    }

    if field_fact.has_definite {
        let mut next = source.state.clone();
        next.origin_mode = origin_mode;
        next.selector_class = SelectorClass::Definite;
        next.selector_continuity = SelectorContinuity::Same;
        next.boundary_bits |= boundary_add;
        if !plumbing {
            next.terminal_semantic_edge_id = Some(field.field_id.clone());
        }
        result.push((
            next,
            TransitionEffect::ActivateDefinite,
            field_fact.definite_selector_ids,
        ));
    }

    result
}

fn next_origin_mode(
    graph: &TypeGraph,
    source: &StateRef,
    edge_id: &str,
    facts: &RouteFacts,
) -> OriginMode {
    if source.type_id == graph.query_root_id
        && source.state == AbstractState::initial()
        && facts.global_id_fields.contains(edge_id)
    {
        OriginMode::GlobalIdPrefix
    } else {
        OriginMode::Traversal
    }
}

fn boundary_bits(facts: &RouteFacts, field_id: &str) -> u8 {
    facts
        .boundaries_by_field
        .get(field_id)
        .into_iter()
        .flatten()
        .fold(0, |bits, boundary| {
            bits
                | match boundary.family {
                    BoundaryFamily::SelfScope => SELF_SCOPE,
                    BoundaryFamily::Visibility => VISIBILITY,
                }
        })
}

fn transition_id(
    source: &StateRef,
    target: &StateRef,
    edge_id: &str,
    effect: TransitionEffect,
) -> String {
    let canonical = serde_json::to_vec(&(source, target, edge_id, effect))
        .expect("abstract transition identity serializes");
    format!("transition:sha256:{:x}", Sha256::digest(canonical))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::contracts::{
        ArgumentClassification, ClassifiedArgument, Confidence, SelectorClass,
    };
    use crate::graph::{build_type_graph, PlumbingIndex};
    use crate::schema::parse_sdl;

    use super::*;
    use crate::route::facts::SelectorFact;

    #[test]
    fn exact_selector_count_does_not_change_abstract_graph_size() {
        let schema = parse_sdl(
            r#"
            type Query { item(id: ID!): Target }
            type Target { id: ID! }
            "#,
        )
        .unwrap()
        .data;
        let graph = build_type_graph(&schema).unwrap();

        let one = facts_with_selector_count(&schema, 1);
        let many = facts_with_selector_count(&schema, 1_000);
        let one_automaton =
            AbstractAutomaton::propagate(&graph, "type:Target", &one).unwrap();
        let many_automaton =
            AbstractAutomaton::propagate(&graph, "type:Target", &many).unwrap();

        assert_eq!(one_automaton.state_count(), many_automaton.state_count());
        assert_eq!(
            one_automaton.transition_count(),
            many_automaton.transition_count()
        );
        assert!(many_automaton
            .terminals
            .iter()
            .all(|terminal| !many_automaton.transitions_by_source.contains_key(terminal)));
    }

    fn facts_with_selector_count(
        schema: &crate::contracts::SchemaIrData,
        count: usize,
    ) -> RouteFacts {
        let field = &schema.types["Query"].fields["item"];
        let argument = &field.arguments[0];
        let classified = ClassifiedArgument {
            arg_ref: argument.arg_id.clone(),
            root_arg_ref: argument.arg_id.clone(),
            arg_path: "Query.item.id".to_string(),
            input_path: Vec::new(),
            type_ref: argument.type_ref.clone(),
            classifications: vec![ArgumentClassification::ObjectSelector],
            signals: vec!["test".to_string()],
            confidence: Confidence::High,
        };
        let selectors: Vec<_> = (0..count)
            .map(|index| SelectorFact {
                selector_id: format!("selector:test:{index}"),
                argument: classified.clone(),
                selected_type_id: "type:Target".to_string(),
                class: SelectorClass::Definite,
                required: true,
            })
            .collect();

        RouteFacts {
            selectors_by_field: BTreeMap::from([(
                "field:Query.item".to_string(),
                selectors.clone(),
            )]),
            selectors_by_ref: BTreeMap::new(),
            selectors_by_id: selectors
                .into_iter()
                .map(|selector| (selector.selector_id.clone(), selector))
                .collect(),
            boundaries_by_field: BTreeMap::new(),
            plumbing: PlumbingIndex::default(),
            global_id_fields: BTreeSet::new(),
        }
    }
}
