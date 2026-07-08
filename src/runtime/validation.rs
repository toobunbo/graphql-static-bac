use std::collections::BTreeMap;

use graphql_parser::parse_query;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::contracts::{
    PathEdge, PathEdgeKind, Route, RuntimeBinding, SchemaIrData, SeedRequirement,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationOperation {
    pub operation_name: String,
    pub operation: String,
    pub variables: Value,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationEmissionError {
    #[error("route references unknown field {0}")]
    MissingField(String),
    #[error("route requirement references unknown argument {0}")]
    MissingArgument(String),
    #[error("route requirement {0} has no runtime binding")]
    MissingBinding(String),
    #[error("multiple direct bindings disagree for argument {0}")]
    ConflictingBinding(String),
    #[error("cannot insert nested input value for argument {0}")]
    InvalidInputPath(String),
    #[error("route operation must project at least one target field")]
    EmptyProjection,
    #[error("emitted validation operation is invalid: {0}")]
    InvalidGraphql(String),
}

pub fn emit_validation_operation(
    schema: &SchemaIrData,
    route: &Route,
    requirements: &[SeedRequirement],
    bindings: &BTreeMap<String, RuntimeBinding>,
) -> Result<ValidationOperation, ValidationEmissionError> {
    let target_type_name = route
        .target_type_id
        .strip_prefix("type:")
        .unwrap_or(&route.target_type_id);
    let target_has_id = schema
        .types
        .get(target_type_name)
        .is_some_and(|definition| definition.fields.contains_key("id"));
    let mut projections = vec!["__typename".to_string()];
    if target_has_id {
        projections.push("id".to_string());
    }
    emit_projected_operation(
        schema,
        route,
        requirements,
        bindings,
        "Validate",
        &projections,
    )
}

pub fn emit_projected_operation(
    schema: &SchemaIrData,
    route: &Route,
    requirements: &[SeedRequirement],
    bindings: &BTreeMap<String, RuntimeBinding>,
    operation_prefix: &str,
    projections: &[String],
) -> Result<ValidationOperation, ValidationEmissionError> {
    if projections.is_empty() {
        return Err(ValidationEmissionError::EmptyProjection);
    }
    let fields = field_index(schema);
    let mut grouped = BTreeMap::<String, Vec<&SeedRequirement>>::new();
    for requirement in requirements {
        if !bindings.contains_key(&requirement.requirement_id) {
            return Err(ValidationEmissionError::MissingBinding(
                requirement.requirement_id.clone(),
            ));
        }
        grouped
            .entry(requirement.root_arg_ref.clone())
            .or_default()
            .push(requirement);
    }

    let mut variables = Map::new();
    let mut variable_by_arg = BTreeMap::<String, String>::new();
    let mut definitions = Vec::new();
    for (position, (root_arg_ref, members)) in grouped.iter().enumerate() {
        let argument = fields
            .values()
            .flat_map(|field| field.arguments.iter())
            .find(|argument| argument.arg_id == *root_arg_ref)
            .ok_or_else(|| ValidationEmissionError::MissingArgument(root_arg_ref.clone()))?;
        let variable_name = format!("seed{}", position + 1);
        let value = assemble_root_value(root_arg_ref, members, bindings)?;
        definitions.push(format!("${variable_name}: {}", argument.type_ref.display));
        variables.insert(variable_name.clone(), value);
        variable_by_arg.insert(root_arg_ref.clone(), variable_name);
    }

    let operation_suffix: String = route
        .route_id
        .strip_prefix("route:sha256:")
        .unwrap_or(&route.route_id)
        .chars()
        .take(16)
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    let prefix: String = operation_prefix
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    let operation_name = format!("{prefix}_{operation_suffix}");
    let header = if definitions.is_empty() {
        format!("query {operation_name}")
    } else {
        format!("query {operation_name}({})", definitions.join(", "))
    };
    let body = render_path(
        &route.witness.edges,
        0,
        &fields,
        &variable_by_arg,
        projections,
        1,
    )?;
    let operation = format!("{header} {{\n{body}}}\n");
    parse_query::<String>(&operation)
        .map_err(|error| ValidationEmissionError::InvalidGraphql(error.to_string()))?;
    Ok(ValidationOperation {
        operation_name,
        operation,
        variables: Value::Object(variables),
    })
}

pub fn response_reaches_target(response: &Value, target_type_id: &str) -> bool {
    let target = target_type_id
        .strip_prefix("type:")
        .unwrap_or(target_type_id);
    contains_typename(response.get("data").unwrap_or(response), target)
}

fn contains_typename(value: &Value, target: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.get("__typename").and_then(Value::as_str) == Some(target)
                || object
                    .values()
                    .any(|value| contains_typename(value, target))
        }
        Value::Array(values) => values.iter().any(|value| contains_typename(value, target)),
        _ => false,
    }
}

fn assemble_root_value(
    root_arg_ref: &str,
    requirements: &[&SeedRequirement],
    bindings: &BTreeMap<String, RuntimeBinding>,
) -> Result<Value, ValidationEmissionError> {
    let mut root = Value::Object(Map::new());
    let mut direct = None;
    for requirement in requirements {
        let binding = bindings.get(&requirement.requirement_id).ok_or_else(|| {
            ValidationEmissionError::MissingBinding(requirement.requirement_id.clone())
        })?;
        if requirement.input_path.is_empty() {
            if direct
                .as_ref()
                .is_some_and(|existing| existing != &binding.consumer_value)
            {
                return Err(ValidationEmissionError::ConflictingBinding(
                    root_arg_ref.to_string(),
                ));
            }
            direct = Some(binding.consumer_value.clone());
        } else {
            insert_input_path(
                &mut root,
                &requirement.input_path,
                binding.consumer_value.clone(),
            )
            .map_err(|_| ValidationEmissionError::InvalidInputPath(root_arg_ref.to_string()))?;
        }
    }
    Ok(direct.unwrap_or(root))
}

fn insert_input_path(root: &mut Value, path: &[String], value: Value) -> Result<(), ()> {
    let mut current = root;
    for (position, segment) in path.iter().enumerate() {
        let object = current.as_object_mut().ok_or(())?;
        if position + 1 == path.len() {
            object.insert(segment.clone(), value);
            return Ok(());
        }
        current = object
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    Err(())
}

fn render_path(
    edges: &[PathEdge],
    position: usize,
    fields: &BTreeMap<String, &crate::contracts::FieldDefinition>,
    variable_by_arg: &BTreeMap<String, String>,
    projections: &[String],
    depth: usize,
) -> Result<String, ValidationEmissionError> {
    if position == edges.len() {
        let indent = "  ".repeat(depth);
        let mut output = String::new();
        for projection in projections {
            output.push_str(&indent);
            output.push_str(projection);
            output.push('\n');
        }
        return Ok(output);
    }

    let edge = &edges[position];
    let indent = "  ".repeat(depth);
    let mut output = String::new();
    match edge.kind {
        PathEdgeKind::Field => {
            let field_id = edge
                .field_id
                .as_ref()
                .ok_or_else(|| ValidationEmissionError::MissingField(edge.edge_id.clone()))?;
            let field = fields
                .get(field_id)
                .ok_or_else(|| ValidationEmissionError::MissingField(field_id.clone()))?;
            output.push_str(&indent);
            output.push_str(&field.name);
            let args = field
                .arguments
                .iter()
                .filter_map(|argument| {
                    variable_by_arg
                        .get(&argument.arg_id)
                        .map(|variable| format!("{}: ${variable}", argument.name))
                })
                .collect::<Vec<_>>();
            if !args.is_empty() {
                output.push('(');
                output.push_str(&args.join(", "));
                output.push(')');
            }
            output.push_str(" {\n");
            output.push_str(&render_path(
                edges,
                position + 1,
                fields,
                variable_by_arg,
                projections,
                depth + 1,
            )?);
            output.push_str(&indent);
            output.push_str("}\n");
        }
        PathEdgeKind::TypeCondition => {
            let type_name = edge
                .target_type_id
                .strip_prefix("type:")
                .unwrap_or(&edge.target_type_id);
            output.push_str(&format!("{indent}... on {type_name} {{\n"));
            output.push_str(&render_path(
                edges,
                position + 1,
                fields,
                variable_by_arg,
                projections,
                depth + 1,
            )?);
            output.push_str(&indent);
            output.push_str("}\n");
        }
    }
    Ok(output)
}

fn field_index(schema: &SchemaIrData) -> BTreeMap<String, &crate::contracts::FieldDefinition> {
    schema
        .types
        .values()
        .flat_map(|definition| definition.fields.values())
        .map(|field| (field.field_id.clone(), field))
        .collect()
}
