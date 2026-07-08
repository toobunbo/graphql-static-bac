use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::contracts::{
    ArgumentClassification, ArgumentsData, BoundaryFamily, BoundarySource, ClassifiedArgument,
    Confidence, RouteBoundary, SchemaIrData, SelectorClass, TypeDefinition, TypeKind, TypeWrapper,
};
use crate::graph::PlumbingIndex;
use crate::identifier::identifier_tokens;

use super::RoutePolicy;

#[derive(Debug, Clone)]
pub struct SelectorFact {
    pub selector_id: String,
    pub argument: ClassifiedArgument,
    pub selected_type_id: String,
    pub class: SelectorClass,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct RouteFacts {
    pub selectors_by_field: BTreeMap<String, Vec<SelectorFact>>,
    pub selectors_by_ref: BTreeMap<(String, Vec<String>), SelectorFact>,
    pub selectors_by_id: BTreeMap<String, SelectorFact>,
    pub boundaries_by_field: BTreeMap<String, Vec<RouteBoundary>>,
    pub plumbing: PlumbingIndex,
    pub global_id_fields: BTreeSet<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouteFactsError {
    #[error("{0}")]
    Contract(String),
}

impl RouteFacts {
    pub fn build(
        schema: &SchemaIrData,
        arguments: &ArgumentsData,
        policy: &RoutePolicy,
    ) -> Result<Self, RouteFactsError> {
        let mut selectors_by_field: BTreeMap<String, Vec<SelectorFact>> = BTreeMap::new();
        let mut selectors_by_ref = BTreeMap::new();
        let mut selectors_by_id = BTreeMap::new();

        for (field_id, classified_field) in &arguments.fields {
            let (owner, field_name) = split_field_id(field_id)?;
            let definition = schema.types.get(owner).ok_or_else(|| {
                RouteFactsError::Contract(format!("S2 references missing type {owner}"))
            })?;
            let field = definition.fields.get(field_name).ok_or_else(|| {
                RouteFactsError::Contract(format!("S2 references missing field {field_id}"))
            })?;
            if field.field_id != *field_id {
                return Err(RouteFactsError::Contract(format!(
                    "S2 field map key differs from field ID: {field_id}"
                )));
            }

            for argument in &classified_field.arguments {
                validate_argument(schema, definition, field_name, argument)?;
                let class = executable_selector_class(argument);
                if class == SelectorClass::None {
                    continue;
                }
                let fact = SelectorFact {
                    selector_id: selector_id(field_id, argument),
                    argument: argument.clone(),
                    selected_type_id: format!("type:{}", field.return_type.named_type),
                    class,
                    required: selector_required(schema, definition, field_name, argument)?,
                };
                let selector_key = (argument.root_arg_ref.clone(), argument.input_path.clone());
                if selectors_by_ref
                    .insert(selector_key, fact.clone())
                    .is_some()
                {
                    return Err(RouteFactsError::Contract(format!(
                        "duplicate selector occurrence {} {:?}",
                        argument.root_arg_ref, argument.input_path
                    )));
                }
                if selectors_by_id
                    .insert(fact.selector_id.clone(), fact.clone())
                    .is_some()
                {
                    return Err(RouteFactsError::Contract(format!(
                        "duplicate selector ID {}",
                        fact.selector_id
                    )));
                }
                selectors_by_field
                    .entry(field_id.clone())
                    .or_default()
                    .push(fact);
            }
        }
        for selectors in selectors_by_field.values_mut() {
            selectors.sort_by(|left, right| {
                left.argument
                    .arg_ref
                    .cmp(&right.argument.arg_ref)
                    .then_with(|| left.class.cmp(&right.class))
            });
        }

        let boundaries_by_field = build_boundaries(schema, policy, &selectors_by_field)?;
        let global_id_fields = find_global_id_fields(schema);

        Ok(Self {
            selectors_by_field,
            selectors_by_ref,
            selectors_by_id,
            boundaries_by_field,
            plumbing: PlumbingIndex::build(schema),
            global_id_fields,
        })
    }
}

fn selector_id(field_id: &str, argument: &ClassifiedArgument) -> String {
    if argument.input_path.is_empty() {
        return argument.arg_ref.clone();
    }
    let canonical = serde_json::to_vec(&(
        field_id,
        &argument.arg_ref,
        &argument.root_arg_ref,
        &argument.input_path,
    ))
    .expect("selector occurrence identity serializes");
    format!("selector:sha256:{:x}", Sha256::digest(canonical))
}

fn executable_selector_class(argument: &ClassifiedArgument) -> SelectorClass {
    if argument
        .classifications
        .contains(&ArgumentClassification::ObjectSelector)
    {
        if argument.confidence != Confidence::Low {
            SelectorClass::Definite
        } else {
            SelectorClass::None
        }
    } else {
        SelectorClass::None
    }
}

fn build_boundaries(
    schema: &SchemaIrData,
    policy: &RoutePolicy,
    selectors: &BTreeMap<String, Vec<SelectorFact>>,
) -> Result<BTreeMap<String, Vec<RouteBoundary>>, RouteFactsError> {
    let mut result: BTreeMap<String, Vec<RouteBoundary>> = BTreeMap::new();
    let query_name = schema
        .roots
        .query
        .as_deref()
        .ok_or_else(|| RouteFactsError::Contract("schema has no Query root".to_string()))?;

    for (owner, definition) in &schema.types {
        for field in definition.fields.values() {
            if owner == query_name
                && selectors.get(&field.field_id).is_none_or(Vec::is_empty)
                && policy
                    .self_scope_root_tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case(&field.name))
            {
                result
                    .entry(field.field_id.clone())
                    .or_default()
                    .push(RouteBoundary {
                        family: BoundaryFamily::SelfScope,
                        source: BoundarySource::Heuristic,
                        root_edge_id: field.field_id.clone(),
                        evidence: format!("root_field:{}", field.name),
                    });
            }
            let tokens = identifier_tokens(&field.name);
            if policy
                .visibility_tokens
                .iter()
                .any(|token| tokens.iter().any(|item| item == &token.to_lowercase()))
            {
                result
                    .entry(field.field_id.clone())
                    .or_default()
                    .push(RouteBoundary {
                        family: BoundaryFamily::Visibility,
                        source: BoundarySource::Heuristic,
                        root_edge_id: field.field_id.clone(),
                        evidence: format!("field_token:{}", field.name),
                    });
            }
        }
    }

    for (field_id, exact) in &policy.exact_boundaries {
        let (owner, field_name) = split_field_id(field_id)?;
        let exists = schema
            .types
            .get(owner)
            .and_then(|definition| definition.fields.get(field_name))
            .is_some_and(|field| field.field_id == *field_id);
        if !exists {
            continue;
        }
        result
            .entry(field_id.clone())
            .or_default()
            .push(RouteBoundary {
                family: exact.family,
                source: BoundarySource::Policy,
                root_edge_id: field_id.clone(),
                evidence: exact.evidence.clone(),
            });
    }
    for boundaries in result.values_mut() {
        boundaries.sort();
        boundaries.dedup();
    }
    Ok(result)
}

fn validate_argument(
    schema: &SchemaIrData,
    owner: &TypeDefinition,
    field_name: &str,
    argument: &ClassifiedArgument,
) -> Result<(), RouteFactsError> {
    if argument.classifications.is_empty() {
        return Err(RouteFactsError::Contract(format!(
            "{} has no classifications",
            argument.arg_ref
        )));
    }
    let mut unique = BTreeSet::new();
    if argument
        .classifications
        .iter()
        .any(|value| !unique.insert(*value))
    {
        return Err(RouteFactsError::Contract(format!(
            "{} has duplicate classifications",
            argument.arg_ref
        )));
    }
    argument.type_ref.validate().map_err(|error| {
        RouteFactsError::Contract(format!("invalid TypeRef for {}: {error}", argument.arg_ref))
    })?;
    if argument.type_ref.display != argument.type_ref.rendered() {
        return Err(RouteFactsError::Contract(format!(
            "non-canonical TypeRef for {}",
            argument.arg_ref
        )));
    }
    let field = owner
        .fields
        .get(field_name)
        .expect("field checked by caller");
    let root = field
        .arguments
        .iter()
        .find(|item| item.arg_id == argument.root_arg_ref)
        .ok_or_else(|| {
            RouteFactsError::Contract(format!(
                "{} references missing root argument {}",
                argument.arg_ref, argument.root_arg_ref
            ))
        })?;
    if argument.input_path.is_empty() {
        if argument.arg_ref != root.arg_id || argument.type_ref != root.type_ref {
            return Err(RouteFactsError::Contract(format!(
                "direct argument mismatch for {}",
                argument.arg_ref
            )));
        }
    } else {
        let final_field =
            resolve_input_path(schema, &root.type_ref.named_type, &argument.input_path)?;
        if argument.arg_ref != final_field.input_field_id
            || argument.type_ref != final_field.type_ref
        {
            return Err(RouteFactsError::Contract(format!(
                "nested argument mismatch for {}",
                argument.arg_ref
            )));
        }
    }
    let expected_arg_path = std::iter::once(owner.type_id.trim_start_matches("type:"))
        .chain(std::iter::once(field_name))
        .chain(std::iter::once(root.name.as_str()))
        .chain(argument.input_path.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(".");
    if argument.arg_path != expected_arg_path {
        return Err(RouteFactsError::Contract(format!(
            "non-canonical arg_path for {}: expected {expected_arg_path}",
            argument.arg_ref
        )));
    }
    Ok(())
}

fn selector_required(
    schema: &SchemaIrData,
    owner: &TypeDefinition,
    field_name: &str,
    argument: &ClassifiedArgument,
) -> Result<bool, RouteFactsError> {
    let field = owner
        .fields
        .get(field_name)
        .expect("field checked by caller");
    let root = field
        .arguments
        .iter()
        .find(|item| item.arg_id == argument.root_arg_ref)
        .expect("root argument validated");
    let mut required = is_required(&root.type_ref.wrappers) && root.default_value.is_none();
    if argument.input_path.is_empty() {
        return Ok(required);
    }
    let mut type_name = root.type_ref.named_type.clone();
    for segment in &argument.input_path {
        let definition = schema.types.get(&type_name).ok_or_else(|| {
            RouteFactsError::Contract(format!("missing input object {type_name}"))
        })?;
        let field = definition.input_fields.get(segment).ok_or_else(|| {
            RouteFactsError::Contract(format!("missing input field {type_name}.{segment}"))
        })?;
        required &= is_required(&field.type_ref.wrappers) && field.default_value.is_none();
        type_name = field.type_ref.named_type.clone();
    }
    Ok(required)
}

fn resolve_input_path<'a>(
    schema: &'a SchemaIrData,
    root_type: &str,
    path: &[String],
) -> Result<&'a crate::contracts::InputFieldDefinition, RouteFactsError> {
    let mut type_name = root_type;
    let mut resolved = None;
    for segment in path {
        let definition = schema.types.get(type_name).ok_or_else(|| {
            RouteFactsError::Contract(format!("missing input object {type_name}"))
        })?;
        let field = definition.input_fields.get(segment).ok_or_else(|| {
            RouteFactsError::Contract(format!("missing input field {type_name}.{segment}"))
        })?;
        type_name = &field.type_ref.named_type;
        resolved = Some(field);
    }
    resolved.ok_or_else(|| RouteFactsError::Contract("empty input path".to_string()))
}

fn is_required(wrappers: &[TypeWrapper]) -> bool {
    wrappers.first() == Some(&TypeWrapper::NonNull)
}

fn find_global_id_fields(schema: &SchemaIrData) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let Some(query_name) = schema.roots.query.as_deref() else {
        return result;
    };
    let Some(query) = schema.types.get(query_name) else {
        return result;
    };
    for field in query.fields.values() {
        let valid = match field.name.as_str() {
            "node" => {
                list_count(&field.return_type.wrappers) == 0
                    && field.return_type.named_type == "Node"
                    && has_id_argument(field, "id", 0)
            }
            "nodes" => {
                list_count(&field.return_type.wrappers) == 1
                    && field.return_type.named_type == "Node"
                    && has_id_argument(field, "ids", 1)
            }
            _ => false,
        };
        if valid {
            result.insert(field.field_id.clone());
        }
    }
    result
}

fn has_id_argument(field: &crate::contracts::FieldDefinition, name: &str, lists: usize) -> bool {
    field.arguments.iter().any(|argument| {
        argument.name == name
            && argument.type_ref.named_type == "ID"
            && argument.type_ref.named_kind == TypeKind::Scalar
            && list_count(&argument.type_ref.wrappers) == lists
    })
}

fn list_count(wrappers: &[TypeWrapper]) -> usize {
    wrappers
        .iter()
        .filter(|wrapper| **wrapper == TypeWrapper::List)
        .count()
}

fn split_field_id(field_id: &str) -> Result<(&str, &str), RouteFactsError> {
    field_id
        .strip_prefix("field:")
        .and_then(|value| value.split_once('.'))
        .ok_or_else(|| RouteFactsError::Contract(format!("invalid field ID {field_id}")))
}
