use std::collections::BTreeMap;

use crate::contracts::{
    PathEdgeKind, RequirementSource, Route, SeedRequirement, StaticBinding, StaticBindingClass,
    TypeKind, TypeRef, TypeWrapper,
};

use super::{ids::seed_requirement_id, index::SeedIndex};

#[derive(Debug, Clone)]
pub(crate) struct CollectedRequirement {
    pub requirement: SeedRequirement,
    pub witness_edge_index: usize,
}

pub(crate) fn collect_requirements(
    route: &Route,
    index: &SeedIndex<'_>,
) -> Vec<CollectedRequirement> {
    let mut requirements = BTreeMap::<String, CollectedRequirement>::new();

    for (edge_index, edge) in route.witness.edges.iter().enumerate() {
        if edge.kind != PathEdgeKind::Field {
            continue;
        }
        let Some(field_id) = edge.field_id.as_deref() else {
            continue;
        };
        let Some(indexed) = index.field(field_id) else {
            continue;
        };
        let selected_type_id = selected_type_after(route, edge_index);

        for argument in &indexed.field.arguments {
            if argument.default_value.is_some() || !is_required(&argument.type_ref) {
                continue;
            }
            collect_argument(
                &mut requirements,
                index,
                field_id,
                &argument.arg_id,
                &argument.arg_id,
                &argument.name,
                &argument.type_ref,
                Vec::new(),
                RequirementSource::SchemaRequired,
                &selected_type_id,
                edge_index,
            );
        }
    }

    if let Some(selector) = &route.selector {
        let consumer_field_id = selector
            .root_arg_ref
            .strip_prefix("arg:")
            .and_then(|value| {
                value
                    .rsplit_once('.')
                    .map(|(field, _)| format!("field:{field}"))
            })
            .unwrap_or_default();
        let leaf_name = selector
            .arg_path
            .rsplit('.')
            .next()
            .unwrap_or(&selector.arg_path);
        let requirement_id = seed_requirement_id(
            &selector.root_arg_ref,
            &selector.input_path,
            &selector.type_ref,
        );
        let edge_index = route
            .witness
            .edges
            .iter()
            .position(|edge| edge.field_id.as_deref() == Some(&consumer_field_id))
            .unwrap_or(0);
        let contextual_selected_type_id = selected_type_after(route, edge_index);
        let replacement = CollectedRequirement {
            requirement: SeedRequirement {
                requirement_id: requirement_id.clone(),
                consumer_arg_ref: selector.arg_ref.clone(),
                root_arg_ref: selector.root_arg_ref.clone(),
                consumer_field_id,
                input_path: selector.input_path.clone(),
                leaf_name: leaf_name.to_string(),
                type_ref: selector.type_ref.clone(),
                source: RequirementSource::RouteSelector,
                selected_type_id: contextual_selected_type_id,
                static_bindings: static_bindings(
                    index,
                    &selector.root_arg_ref,
                    &selector.input_path,
                    leaf_name,
                    &selector.type_ref,
                ),
                producer_candidates: Vec::new(),
            },
            witness_edge_index: edge_index,
        };
        requirements
            .entry(requirement_id)
            .and_modify(|existing| {
                existing.requirement.source = RequirementSource::RouteSelector;
                existing.requirement.consumer_arg_ref = selector.arg_ref.clone();
            })
            .or_insert(replacement);
    }

    requirements.into_values().collect()
}

#[allow(clippy::too_many_arguments)]
fn collect_argument(
    requirements: &mut BTreeMap<String, CollectedRequirement>,
    index: &SeedIndex<'_>,
    consumer_field_id: &str,
    consumer_arg_ref: &str,
    root_arg_ref: &str,
    leaf_name: &str,
    type_ref: &TypeRef,
    input_path: Vec<String>,
    source: RequirementSource,
    selected_type_id: &str,
    witness_edge_index: usize,
) {
    if type_ref.named_kind == TypeKind::InputObject {
        let Some(input_object) = index.type_by_name(&type_ref.named_type) else {
            insert_requirement(
                requirements,
                index,
                consumer_field_id,
                consumer_arg_ref,
                root_arg_ref,
                leaf_name,
                type_ref,
                input_path,
                source,
                selected_type_id,
                witness_edge_index,
            );
            return;
        };
        let required_fields: Vec<_> = input_object
            .input_fields
            .values()
            .filter(|field| field.default_value.is_none() && is_required(&field.type_ref))
            .collect();
        if required_fields.is_empty() {
            insert_requirement(
                requirements,
                index,
                consumer_field_id,
                consumer_arg_ref,
                root_arg_ref,
                leaf_name,
                type_ref,
                input_path,
                source,
                selected_type_id,
                witness_edge_index,
            );
            return;
        }
        for field in required_fields {
            let mut nested_input_path = input_path.clone();
            nested_input_path.push(field.name.clone());
            collect_argument(
                requirements,
                index,
                consumer_field_id,
                &field.input_field_id,
                root_arg_ref,
                &field.name,
                &field.type_ref,
                nested_input_path,
                RequirementSource::RequiredInputField,
                selected_type_id,
                witness_edge_index,
            );
        }
        return;
    }

    insert_requirement(
        requirements,
        index,
        consumer_field_id,
        consumer_arg_ref,
        root_arg_ref,
        leaf_name,
        type_ref,
        input_path,
        source,
        selected_type_id,
        witness_edge_index,
    );
}

#[allow(clippy::too_many_arguments)]
fn insert_requirement(
    requirements: &mut BTreeMap<String, CollectedRequirement>,
    index: &SeedIndex<'_>,
    consumer_field_id: &str,
    consumer_arg_ref: &str,
    root_arg_ref: &str,
    leaf_name: &str,
    type_ref: &TypeRef,
    input_path: Vec<String>,
    source: RequirementSource,
    selected_type_id: &str,
    witness_edge_index: usize,
) {
    let requirement_id = seed_requirement_id(root_arg_ref, &input_path, type_ref);
    let bindings = static_bindings(index, root_arg_ref, &input_path, leaf_name, type_ref);
    requirements
        .entry(requirement_id.clone())
        .or_insert_with(|| CollectedRequirement {
            requirement: SeedRequirement {
                requirement_id,
                consumer_arg_ref: consumer_arg_ref.to_string(),
                root_arg_ref: root_arg_ref.to_string(),
                consumer_field_id: consumer_field_id.to_string(),
                input_path,
                leaf_name: leaf_name.to_string(),
                type_ref: type_ref.clone(),
                source,
                selected_type_id: selected_type_id.to_string(),
                static_bindings: bindings,
                producer_candidates: Vec::new(),
            },
            witness_edge_index,
        });
}

fn static_bindings(
    index: &SeedIndex<'_>,
    arg_ref: &str,
    input_path: &[String],
    leaf_name: &str,
    type_ref: &TypeRef,
) -> Vec<StaticBinding> {
    if is_pagination(leaf_name) && matches!(type_ref.named_type.as_str(), "Int" | "Float") {
        return vec![StaticBinding {
            arg_ref: arg_ref.to_string(),
            input_path: input_path.to_vec(),
            class: StaticBindingClass::BoundedPagination,
            value: Some("20".to_string()),
        }];
    }
    match type_ref.named_kind {
        TypeKind::Enum => index
            .type_by_name(&type_ref.named_type)
            .into_iter()
            .flat_map(|definition| definition.enum_values.iter())
            .map(|value| StaticBinding {
                arg_ref: arg_ref.to_string(),
                input_path: input_path.to_vec(),
                class: StaticBindingClass::SchemaEnumValue,
                value: Some(value.name.clone()),
            })
            .collect(),
        TypeKind::Scalar if type_ref.named_type == "Boolean" => ["true", "false"]
            .into_iter()
            .map(|value| StaticBinding {
                arg_ref: arg_ref.to_string(),
                input_path: input_path.to_vec(),
                class: StaticBindingClass::GeneratedBoolean,
                value: Some(value.to_string()),
            })
            .collect(),
        TypeKind::InputObject => vec![StaticBinding {
            arg_ref: arg_ref.to_string(),
            input_path: input_path.to_vec(),
            class: StaticBindingClass::UnresolvedLiteral,
            value: None,
        }],
        _ => Vec::new(),
    }
}

fn selected_type_after(route: &Route, field_edge_index: usize) -> String {
    let field_edge = &route.witness.edges[field_edge_index];
    route
        .witness
        .edges
        .iter()
        .skip(field_edge_index + 1)
        .take_while(|edge| edge.kind == PathEdgeKind::TypeCondition)
        .last()
        .map_or_else(
            || field_edge.target_type_id.clone(),
            |edge| edge.target_type_id.clone(),
        )
}

pub(crate) fn is_required(type_ref: &TypeRef) -> bool {
    type_ref.wrappers.first() == Some(&TypeWrapper::NonNull)
}

pub(crate) fn is_pagination(name: &str) -> bool {
    matches!(name, "first" | "last" | "limit" | "pageSize")
}
