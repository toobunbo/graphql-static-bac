use std::collections::{BTreeMap, BTreeSet};

use graphql_parser::parse_query;
use thiserror::Error;

use crate::contracts::{PathEdge, PathEdgeKind, StaticBinding};

use super::index::SeedIndex;

#[derive(Debug, Clone)]
pub(crate) struct ProjectionBranch {
    pub edges: Vec<PathEdge>,
    pub terminal_field_id: String,
}

#[derive(Debug, Error)]
pub(crate) enum QueryEmissionError {
    #[error("query witness references unknown field {0}")]
    MissingField(String),
    #[error("query witness contains a terminal projection before a composite selection")]
    InvalidProjection,
    #[error("unresolved argument {0} is not present in the emitted witness")]
    MissingArgument(String),
    #[error("emitted GraphQL operation is invalid: {0}")]
    InvalidGraphql(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SelectionKey {
    Field(String),
    TypeCondition(String),
}

#[derive(Debug, Clone, Default)]
struct SelectionNode {
    children: BTreeMap<SelectionKey, SelectionNode>,
}

pub(crate) fn emit_operation(
    index: &SeedIndex<'_>,
    operation_name: &str,
    branches: &[ProjectionBranch],
    static_bindings: &[StaticBinding],
    unresolved_arg_refs: &[String],
) -> Result<String, QueryEmissionError> {
    let mut root = SelectionNode::default();
    for branch in branches {
        insert_branch(&mut root, branch)?;
    }

    let bindings: BTreeMap<_, _> = static_bindings
        .iter()
        .filter_map(|binding| {
            if binding.input_path.is_empty() {
                binding
                    .value
                    .as_ref()
                    .map(|value| (binding.arg_ref.as_str(), value.as_str()))
            } else {
                None
            }
        })
        .collect();
    let unresolved: BTreeSet<_> = unresolved_arg_refs.iter().map(String::as_str).collect();
    let variables = build_variables(index, &root, &unresolved)?;
    let variable_definitions = variables
        .iter()
        .map(|(arg_ref, (name, type_ref))| {
            let _ = arg_ref;
            format!("${name}: {type_ref}")
        })
        .collect::<Vec<_>>();
    let header = if variable_definitions.is_empty() {
        format!("query {operation_name}")
    } else {
        format!(
            "query {operation_name}({})",
            variable_definitions.join(", ")
        )
    };
    let body = render_children(index, &root, &bindings, &variables, 1)?;
    let operation = format!("{header} {{\n{body}}}\n");
    parse_query::<String>(&operation)
        .map_err(|error| QueryEmissionError::InvalidGraphql(error.to_string()))?;
    Ok(operation)
}

fn insert_branch(
    root: &mut SelectionNode,
    branch: &ProjectionBranch,
) -> Result<(), QueryEmissionError> {
    let mut current = root;
    for edge in &branch.edges {
        let key = match edge.kind {
            PathEdgeKind::Field => SelectionKey::Field(
                edge.field_id
                    .clone()
                    .ok_or(QueryEmissionError::InvalidProjection)?,
            ),
            PathEdgeKind::TypeCondition => SelectionKey::TypeCondition(edge.target_type_id.clone()),
        };
        current = current.children.entry(key).or_default();
    }
    current
        .children
        .entry(SelectionKey::Field(branch.terminal_field_id.clone()))
        .or_default();
    Ok(())
}

type Variables<'a> = BTreeMap<&'a str, (String, String)>;

fn build_variables<'a>(
    index: &'a SeedIndex<'_>,
    root: &SelectionNode,
    unresolved: &BTreeSet<&'a str>,
) -> Result<Variables<'a>, QueryEmissionError> {
    let mut present = BTreeMap::<&str, String>::new();
    collect_argument_types(index, root, &mut present)?;
    let mut variables = BTreeMap::new();
    for (position, arg_ref) in unresolved.iter().enumerate() {
        let type_ref = present
            .get(arg_ref)
            .ok_or_else(|| QueryEmissionError::MissingArgument((*arg_ref).to_string()))?;
        variables.insert(
            *arg_ref,
            (format!("seed{}", position + 1), type_ref.clone()),
        );
    }
    Ok(variables)
}

fn collect_argument_types<'a>(
    index: &'a SeedIndex<'_>,
    node: &SelectionNode,
    output: &mut BTreeMap<&'a str, String>,
) -> Result<(), QueryEmissionError> {
    for (key, child) in &node.children {
        if let SelectionKey::Field(field_id) = key {
            let indexed = index
                .field(field_id)
                .ok_or_else(|| QueryEmissionError::MissingField(field_id.clone()))?;
            for argument in &indexed.field.arguments {
                output.insert(&argument.arg_id, argument.type_ref.display.clone());
            }
        }
        collect_argument_types(index, child, output)?;
    }
    Ok(())
}

fn render_children(
    index: &SeedIndex<'_>,
    node: &SelectionNode,
    bindings: &BTreeMap<&str, &str>,
    variables: &Variables<'_>,
    depth: usize,
) -> Result<String, QueryEmissionError> {
    let mut output = String::new();
    let indent = "  ".repeat(depth);
    for (key, child) in &node.children {
        match key {
            SelectionKey::Field(field_id) => {
                let indexed = index
                    .field(field_id)
                    .ok_or_else(|| QueryEmissionError::MissingField(field_id.clone()))?;
                output.push_str(&indent);
                output.push_str(&indexed.field.name);
                let args = indexed
                    .field
                    .arguments
                    .iter()
                    .filter_map(|argument| {
                        bindings
                            .get(argument.arg_id.as_str())
                            .map(|value| format!("{}: {value}", argument.name))
                            .or_else(|| {
                                variables
                                    .get(argument.arg_id.as_str())
                                    .map(|(variable, _)| format!("{}: ${variable}", argument.name))
                            })
                    })
                    .collect::<Vec<_>>();
                if !args.is_empty() {
                    output.push('(');
                    output.push_str(&args.join(", "));
                    output.push(')');
                }
                if child.children.is_empty() {
                    output.push('\n');
                } else {
                    output.push_str(" {\n");
                    output.push_str(&render_children(
                        index,
                        child,
                        bindings,
                        variables,
                        depth + 1,
                    )?);
                    output.push_str(&indent);
                    output.push_str("}\n");
                }
            }
            SelectionKey::TypeCondition(target_type_id) => {
                let type_name = target_type_id
                    .strip_prefix("type:")
                    .unwrap_or(target_type_id);
                output.push_str(&format!("{indent}... on {type_name} {{\n"));
                output.push_str(&render_children(
                    index,
                    child,
                    bindings,
                    variables,
                    depth + 1,
                )?);
                output.push_str(&indent);
                output.push_str("}\n");
            }
        }
    }
    Ok(output)
}
