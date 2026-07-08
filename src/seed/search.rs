use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::contracts::{
    ArgumentClassification, PathEdge, PathEdgeKind, StaticBinding, StaticBindingClass, TypeKind,
};
use crate::graph::{reverse_reachable, GraphEdgeKind, TypeGraph};

use super::{
    index::SeedIndex,
    requirements::{is_pagination, is_required},
};

const MAX_SEARCH_STATES: usize = 5_000;
const MAX_BINDING_VARIANTS_PER_EDGE: usize = 16;
const MAX_STATES_PER_TYPE_ENTRY: usize = 16;
const MAX_PRODUCER_PATHS: usize = 48;

type StateGroupKey = (String, Option<String>, Vec<String>);
type StateGroups = BTreeMap<StateGroupKey, BTreeSet<SearchState>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProducerPath {
    pub entry_field_id: Option<String>,
    pub edges: Vec<PathEdge>,
    pub static_bindings: Vec<StaticBinding>,
    pub unresolved_arg_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SearchState {
    type_id: String,
    entry_field_id: Option<String>,
    prefix_field_ids: Vec<String>,
    static_bindings: Vec<StaticBinding>,
    unresolved_arg_refs: Vec<String>,
}

pub(crate) fn search_producer_paths(
    graph: &TypeGraph,
    index: &SeedIndex<'_>,
    source_type_id: &str,
    target_type_id: &str,
) -> Vec<ProducerPath> {
    if graph.node(source_type_id).is_none() || graph.node(target_type_id).is_none() {
        return Vec::new();
    }
    if source_type_id == target_type_id {
        return vec![ProducerPath {
            entry_field_id: None,
            edges: Vec::new(),
            static_bindings: Vec::new(),
            unresolved_arg_refs: Vec::new(),
        }];
    }

    let reverse = reverse_reachable(graph, target_type_id);
    if !reverse.contains(source_type_id) {
        return Vec::new();
    }
    let initial = SearchState {
        type_id: source_type_id.to_string(),
        entry_field_id: None,
        prefix_field_ids: Vec::new(),
        static_bindings: Vec::new(),
        unresolved_arg_refs: Vec::new(),
    };
    let mut reached = BTreeMap::from([(initial.clone(), Vec::<usize>::new())]);
    let mut groups = BTreeMap::from([(
        (
            initial.type_id.clone(),
            initial.entry_field_id.clone(),
            initial.prefix_field_ids.clone(),
        ),
        BTreeSet::from([initial.clone()]),
    )]);
    let mut queue = VecDeque::from([initial]);

    while let Some(state) = queue.pop_front() {
        if reached.len() >= MAX_SEARCH_STATES {
            break;
        }
        let Some(path) = reached.get(&state).cloned() else {
            continue;
        };
        for edge_index in graph.outgoing(&state.type_id) {
            let edge = graph.edge(*edge_index);
            if !reverse.contains(&edge.target_type_id) {
                continue;
            }
            let mut next_path = path.clone();
            next_path.push(*edge_index);
            match edge.kind {
                GraphEdgeKind::TypeCondition => {
                    let mut next = state.clone();
                    next.type_id = edge.target_type_id.clone();
                    enqueue(
                        graph,
                        &mut reached,
                        &mut groups,
                        &mut queue,
                        next,
                        next_path,
                    );
                }
                GraphEdgeKind::Field => {
                    let field = edge
                        .field
                        .as_ref()
                        .expect("FIELD graph edge retains definition");
                    for variant in field_binding_variants(index, field) {
                        let mut next = state.clone();
                        next.type_id = edge.target_type_id.clone();
                        next.entry_field_id
                            .get_or_insert_with(|| field.field_id.clone());
                        if next.prefix_field_ids.len() < 2 {
                            next.prefix_field_ids.push(field.field_id.clone());
                        }
                        merge_bindings(&mut next.static_bindings, variant.static_bindings);
                        merge_strings(&mut next.unresolved_arg_refs, variant.unresolved_arg_refs);
                        enqueue(
                            graph,
                            &mut reached,
                            &mut groups,
                            &mut queue,
                            next,
                            next_path.clone(),
                        );
                    }
                }
            }
        }
    }

    let mut results =
        BTreeMap::<(Option<String>, Vec<StaticBinding>, Vec<String>), ProducerPath>::new();
    for (state, path) in reached
        .into_iter()
        .filter(|(state, path)| state.type_id == target_type_id && !path.is_empty())
    {
        let result = ProducerPath {
            entry_field_id: state.entry_field_id.clone(),
            edges: path
                .iter()
                .map(|edge_index| materialize_edge(graph, *edge_index))
                .collect(),
            static_bindings: state.static_bindings.clone(),
            unresolved_arg_refs: state.unresolved_arg_refs.clone(),
        };
        let key = (
            state.entry_field_id,
            state.static_bindings,
            state.unresolved_arg_refs,
        );
        match results.get(&key) {
            Some(existing) if compare_path(existing, &result) != Ordering::Greater => {}
            _ => {
                results.insert(key, result);
            }
        }
    }
    let mut results: Vec<_> = results.into_values().collect();
    results.sort_by(compare_result_quality);
    select_diverse_paths(results, MAX_PRODUCER_PATHS)
}

fn select_diverse_paths(paths: Vec<ProducerPath>, limit: usize) -> Vec<ProducerPath> {
    if paths.len() <= limit {
        return paths;
    }

    let mut selected = Vec::new();
    let mut selected_ids = BTreeSet::new();
    let mut seen_prefixes = BTreeSet::new();
    for path in &paths {
        if seen_prefixes.insert(path_prefix_signature(path)) {
            selected_ids.insert(path_identity(path));
            selected.push(path.clone());
            if selected.len() == limit {
                return selected;
            }
        }
    }
    for path in paths {
        if selected_ids.insert(path_identity(&path)) {
            selected.push(path);
            if selected.len() == limit {
                break;
            }
        }
    }
    selected.sort_by(compare_result_quality);
    selected
}

fn path_prefix_signature(path: &ProducerPath) -> Vec<String> {
    path.edges
        .iter()
        .filter_map(|edge| edge.field_id.clone())
        .take(2)
        .collect()
}

fn path_identity(
    path: &ProducerPath,
) -> (Option<String>, Vec<String>, Vec<StaticBinding>, Vec<String>) {
    (
        path.entry_field_id.clone(),
        path.edges.iter().map(|edge| edge.edge_id.clone()).collect(),
        path.static_bindings.clone(),
        path.unresolved_arg_refs.clone(),
    )
}

#[derive(Debug, Clone)]
struct BindingVariant {
    static_bindings: Vec<StaticBinding>,
    unresolved_arg_refs: Vec<String>,
}

fn field_binding_variants(
    index: &SeedIndex<'_>,
    field: &crate::contracts::FieldDefinition,
) -> Vec<BindingVariant> {
    let mut variants = vec![BindingVariant {
        static_bindings: Vec::new(),
        unresolved_arg_refs: Vec::new(),
    }];
    for argument in &field.arguments {
        if argument.default_value.is_some() {
            continue;
        }
        let choices = if is_required(&argument.type_ref) {
            required_argument_choices(index, argument)
        } else if argument.name == "first"
            && matches!(argument.type_ref.named_type.as_str(), "Int" | "Float")
        {
            vec![BindingVariant {
                static_bindings: vec![StaticBinding {
                    arg_ref: argument.arg_id.clone(),
                    input_path: Vec::new(),
                    class: StaticBindingClass::BoundedPagination,
                    value: Some("20".to_string()),
                }],
                unresolved_arg_refs: Vec::new(),
            }]
        } else {
            continue;
        };
        variants = cross_variants(variants, choices);
        if variants.len() > MAX_BINDING_VARIANTS_PER_EDGE {
            variants.truncate(MAX_BINDING_VARIANTS_PER_EDGE);
        }
    }
    variants
}

fn required_argument_choices(
    index: &SeedIndex<'_>,
    argument: &crate::contracts::ArgumentDefinition,
) -> Vec<BindingVariant> {
    if is_pagination(&argument.name)
        && matches!(argument.type_ref.named_type.as_str(), "Int" | "Float")
    {
        return single_binding(
            &argument.arg_id,
            StaticBindingClass::BoundedPagination,
            "20",
        );
    }
    match argument.type_ref.named_kind {
        TypeKind::Enum => {
            let choices: Vec<_> = index
                .type_by_name(&argument.type_ref.named_type)
                .into_iter()
                .flat_map(|definition| definition.enum_values.iter())
                .flat_map(|value| {
                    single_binding(
                        &argument.arg_id,
                        StaticBindingClass::SchemaEnumValue,
                        &value.name,
                    )
                })
                .collect();
            if choices.is_empty() {
                unresolved(&argument.arg_id)
            } else {
                choices
            }
        }
        TypeKind::Scalar if argument.type_ref.named_type == "Boolean" => ["true", "false"]
            .into_iter()
            .flat_map(|value| {
                single_binding(
                    &argument.arg_id,
                    StaticBindingClass::GeneratedBoolean,
                    value,
                )
            })
            .collect(),
        TypeKind::Scalar | TypeKind::InputObject => unresolved(&argument.arg_id),
        _ => unresolved(&argument.arg_id),
    }
}

fn single_binding(arg_ref: &str, class: StaticBindingClass, value: &str) -> Vec<BindingVariant> {
    vec![BindingVariant {
        static_bindings: vec![StaticBinding {
            arg_ref: arg_ref.to_string(),
            input_path: Vec::new(),
            class,
            value: Some(value.to_string()),
        }],
        unresolved_arg_refs: Vec::new(),
    }]
}

fn unresolved(arg_ref: &str) -> Vec<BindingVariant> {
    vec![BindingVariant {
        static_bindings: Vec::new(),
        unresolved_arg_refs: vec![arg_ref.to_string()],
    }]
}

fn cross_variants(left: Vec<BindingVariant>, right: Vec<BindingVariant>) -> Vec<BindingVariant> {
    let mut output = Vec::new();
    for left in left {
        for right in &right {
            let mut combined = left.clone();
            merge_bindings(&mut combined.static_bindings, right.static_bindings.clone());
            merge_strings(
                &mut combined.unresolved_arg_refs,
                right.unresolved_arg_refs.clone(),
            );
            output.push(combined);
            if output.len() >= MAX_BINDING_VARIANTS_PER_EDGE {
                return output;
            }
        }
    }
    output
}

fn merge_bindings(target: &mut Vec<StaticBinding>, values: Vec<StaticBinding>) {
    target.extend(values);
    target.sort();
    target.dedup();
}

fn merge_strings(target: &mut Vec<String>, values: Vec<String>) {
    target.extend(values);
    target.sort();
    target.dedup();
}

fn enqueue(
    graph: &TypeGraph,
    reached: &mut BTreeMap<SearchState, Vec<usize>>,
    groups: &mut StateGroups,
    queue: &mut VecDeque<SearchState>,
    state: SearchState,
    path: Vec<usize>,
) {
    let group_key = (
        state.type_id.clone(),
        state.entry_field_id.clone(),
        state.prefix_field_ids.clone(),
    );
    let group = groups.entry(group_key.clone()).or_default();
    let dominated = group.iter().any(|existing| {
        let existing_path = reached
            .get(existing)
            .expect("group state retains a reached witness");
        if existing == &state {
            return compare_edge_paths(graph, existing_path, &path) != Ordering::Greater;
        }
        is_subset(&existing.static_bindings, &state.static_bindings)
            && is_subset(&existing.unresolved_arg_refs, &state.unresolved_arg_refs)
    });
    if dominated {
        return;
    }
    let dominated_states: Vec<_> = group
        .iter()
        .filter(|existing| {
            *existing != &state
                && is_subset(&state.static_bindings, &existing.static_bindings)
                && is_subset(&state.unresolved_arg_refs, &existing.unresolved_arg_refs)
        })
        .cloned()
        .collect();
    for dominated in dominated_states {
        reached.remove(&dominated);
        group.remove(&dominated);
    }
    if group.len() >= MAX_STATES_PER_TYPE_ENTRY {
        let worst = group
            .iter()
            .max_by(|left, right| compare_state_quality(left, right))
            .cloned()
            .expect("bounded group is non-empty");
        if compare_state_quality(&state, &worst) != Ordering::Less {
            return;
        }
        reached.remove(&worst);
        group.remove(&worst);
    }
    let should_update = reached
        .get(&state)
        .is_none_or(|current| compare_edge_paths(graph, current, &path) == Ordering::Greater);
    if should_update {
        reached.insert(state.clone(), path);
        group.insert(state.clone());
        queue.push_back(state);
    }
}

fn is_subset<T: Ord>(left: &[T], right: &[T]) -> bool {
    left.iter().all(|value| right.binary_search(value).is_ok())
}

fn compare_state_quality(left: &SearchState, right: &SearchState) -> Ordering {
    left.unresolved_arg_refs
        .len()
        .cmp(&right.unresolved_arg_refs.len())
        .then_with(|| left.static_bindings.len().cmp(&right.static_bindings.len()))
        .then_with(|| left.unresolved_arg_refs.cmp(&right.unresolved_arg_refs))
        .then_with(|| left.static_bindings.cmp(&right.static_bindings))
}

fn compare_edge_paths(graph: &TypeGraph, left: &[usize], right: &[usize]) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| {
        left.iter()
            .map(|edge| graph.edge(*edge).edge_id.as_str())
            .cmp(right.iter().map(|edge| graph.edge(*edge).edge_id.as_str()))
    })
}

fn compare_path(left: &ProducerPath, right: &ProducerPath) -> Ordering {
    left.edges.len().cmp(&right.edges.len()).then_with(|| {
        left.edges
            .iter()
            .map(|edge| edge.edge_id.as_str())
            .cmp(right.edges.iter().map(|edge| edge.edge_id.as_str()))
    })
}

fn compare_result_quality(left: &ProducerPath, right: &ProducerPath) -> Ordering {
    left.unresolved_arg_refs
        .len()
        .cmp(&right.unresolved_arg_refs.len())
        .then_with(|| left.static_bindings.len().cmp(&right.static_bindings.len()))
        .then_with(|| compare_path(left, right))
        .then_with(|| left.entry_field_id.cmp(&right.entry_field_id))
}

fn materialize_edge(graph: &TypeGraph, edge_index: usize) -> PathEdge {
    let edge = graph.edge(edge_index);
    PathEdge {
        edge_id: edge.edge_id.clone(),
        kind: match edge.kind {
            GraphEdgeKind::Field => PathEdgeKind::Field,
            GraphEdgeKind::TypeCondition => PathEdgeKind::TypeCondition,
        },
        source_type_id: edge.source_type_id.clone(),
        field_id: edge.field.as_ref().map(|field| field.field_id.clone()),
        target_type_id: edge.target_type_id.clone(),
    }
}

pub(crate) fn path_response_segments(
    index: &SeedIndex<'_>,
    edges: &[PathEdge],
) -> Vec<(String, bool)> {
    edges
        .iter()
        .filter_map(|edge| {
            let field_id = edge.field_id.as_deref()?;
            let field = index.field(field_id)?.field;
            let is_list = field
                .return_type
                .wrappers
                .iter()
                .any(|wrapper| matches!(wrapper, crate::contracts::TypeWrapper::List));
            Some((field.name.clone(), is_list))
        })
        .collect()
}

pub(crate) fn is_selector_arg(index: &SeedIndex<'_>, arg_ref: &str, input_path: &[String]) -> bool {
    index
        .classified_argument(arg_ref, input_path)
        .is_some_and(|argument| {
            argument
                .classifications
                .contains(&ArgumentClassification::ObjectSelector)
                && argument.confidence != crate::contracts::Confidence::Low
        })
}
