use std::collections::BTreeSet;

use thiserror::Error;

use crate::contracts::{
    ArgumentClassification, ArgumentsData, ClassifiedArgument, SchemaIrData, TypeKind,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArgumentValidationError {
    #[error("{0}")]
    Invariant(String),
}

pub fn validate_arguments_data(
    schema: &SchemaIrData,
    arguments: &ArgumentsData,
) -> Result<(), ArgumentValidationError> {
    let expected_fields: BTreeSet<_> = schema
        .types
        .values()
        .flat_map(|definition| definition.fields.values())
        .map(|field| field.field_id.as_str())
        .collect();
    let actual_fields: BTreeSet<_> = arguments.fields.keys().map(String::as_str).collect();
    require(
        expected_fields == actual_fields,
        "S2 field coverage differs from S0".to_string(),
    )?;

    for (owner_name, definition) in &schema.types {
        for field in definition.fields.values() {
            let classified = &arguments.fields[&field.field_id];
            let mut occurrences = BTreeSet::new();
            for argument in &classified.arguments {
                require(
                    occurrences.insert((
                        argument.root_arg_ref.as_str(),
                        argument.input_path.as_slice(),
                    )),
                    format!(
                        "duplicate classified occurrence {} {:?}",
                        argument.root_arg_ref, argument.input_path
                    ),
                )?;
                validate_argument(schema, owner_name, &field.name, argument)?;
            }
            for root in &field.arguments {
                require(
                    classified
                        .arguments
                        .iter()
                        .any(|argument| argument.root_arg_ref == root.arg_id),
                    format!("root argument {} has no S2 record", root.arg_id),
                )?;
            }
        }
    }
    Ok(())
}

fn validate_argument(
    schema: &SchemaIrData,
    owner_name: &str,
    field_name: &str,
    argument: &ClassifiedArgument,
) -> Result<(), ArgumentValidationError> {
    require(
        !argument.classifications.is_empty(),
        format!("{} has no classifications", argument.arg_ref),
    )?;
    require(
        is_sorted_unique(&argument.classifications),
        format!(
            "{} classifications are not sorted and unique",
            argument.arg_ref
        ),
    )?;
    require(
        is_sorted_unique(&argument.signals),
        format!("{} signals are not sorted and unique", argument.arg_ref),
    )?;
    argument
        .type_ref
        .validate()
        .map_err(|error| ArgumentValidationError::Invariant(error.to_string()))?;
    require(
        argument.type_ref.display == argument.type_ref.rendered(),
        format!("{} has a non-canonical TypeRef", argument.arg_ref),
    )?;

    let field = schema.types[owner_name]
        .fields
        .get(field_name)
        .expect("field is sourced from schema");
    let root = field
        .arguments
        .iter()
        .find(|root| root.arg_id == argument.root_arg_ref)
        .ok_or_else(|| {
            ArgumentValidationError::Invariant(format!(
                "{} references missing root argument {}",
                argument.arg_ref, argument.root_arg_ref
            ))
        })?;

    let (expected_ref, expected_type) = if argument.input_path.is_empty() {
        (&root.arg_id, &root.type_ref)
    } else {
        let mut type_name = root.type_ref.named_type.as_str();
        let mut resolved = None;
        for segment in &argument.input_path {
            let definition = schema.types.get(type_name).ok_or_else(|| {
                ArgumentValidationError::Invariant(format!(
                    "{} traverses missing input object {type_name}",
                    argument.arg_ref
                ))
            })?;
            require(
                definition.kind == TypeKind::InputObject,
                format!("{type_name} is not INPUT_OBJECT"),
            )?;
            let input = definition.input_fields.get(segment).ok_or_else(|| {
                ArgumentValidationError::Invariant(format!(
                    "{} traverses missing input field {type_name}.{segment}",
                    argument.arg_ref
                ))
            })?;
            type_name = &input.type_ref.named_type;
            resolved = Some(input);
        }
        let input = resolved.expect("non-empty input path resolves a field");
        (&input.input_field_id, &input.type_ref)
    };
    require(
        argument.arg_ref == *expected_ref && argument.type_ref == *expected_type,
        format!("{} identity differs from S0", argument.arg_ref),
    )?;

    let expected_path = std::iter::once(owner_name)
        .chain(std::iter::once(field_name))
        .chain(std::iter::once(root.name.as_str()))
        .chain(argument.input_path.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(".");
    require(
        argument.arg_path == expected_path,
        format!(
            "{} has non-canonical arg_path; expected {expected_path}",
            argument.arg_ref
        ),
    )?;

    if argument.classifications == [ArgumentClassification::Noise] {
        require(
            argument
                .signals
                .iter()
                .any(|signal| signal.starts_with("name:definite_noise:")),
            format!(
                "{} is sole-noise without definite evidence",
                argument.arg_ref
            ),
        )?;
    }
    Ok(())
}

fn is_sorted_unique<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn require(condition: bool, message: String) -> Result<(), ArgumentValidationError> {
    if condition {
        Ok(())
    } else {
        Err(ArgumentValidationError::Invariant(message))
    }
}
