use std::collections::BTreeSet;

use thiserror::Error;

use crate::contracts::{
    arg_id, field_id, input_field_id, type_id, SchemaIrData, TypeKind, TypeRef,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("{0}")]
    Invariant(String),
}

pub fn validate_schema_ir(schema: &SchemaIrData) -> Result<(), ValidationError> {
    for (name, definition) in &schema.types {
        require(
            name == definition_name(definition),
            format!("type map key mismatch: {name}"),
        )?;
        require(
            definition.type_id == type_id(name),
            format!("invalid type_id for {name}"),
        )?;
        match definition.kind {
            TypeKind::Object | TypeKind::Interface => {}
            _ => require(
                definition.fields.is_empty(),
                format!("non-output composite type {name} contains fields"),
            )?,
        }
        if definition.kind != TypeKind::InputObject {
            require(
                definition.input_fields.is_empty(),
                format!("non-input-object type {name} contains input_fields"),
            )?;
        }
        if definition.kind != TypeKind::Enum {
            require(
                definition.enum_values.is_empty(),
                format!("non-enum type {name} contains enum_values"),
            )?;
        }

        for interface in &definition.interfaces {
            let target = schema.types.get(interface).ok_or_else(|| {
                ValidationError::Invariant(format!(
                    "{name} references missing interface {interface}"
                ))
            })?;
            require(
                target.kind == TypeKind::Interface,
                format!("{name} references non-interface {interface}"),
            )?;
        }
        for possible in &definition.possible_types {
            let target = schema.types.get(possible).ok_or_else(|| {
                ValidationError::Invariant(format!(
                    "{name} references missing possible type {possible}"
                ))
            })?;
            require(
                target.kind == TypeKind::Object,
                format!("{name} possible type {possible} is not OBJECT"),
            )?;
        }

        for (field_name, field) in &definition.fields {
            require(
                field_name == &field.name,
                format!("field map key mismatch: {name}.{field_name}"),
            )?;
            require(
                field.field_id == field_id(name, field_name),
                format!("invalid field_id for {name}.{field_name}"),
            )?;
            validate_type_ref(schema, &field.return_type, false)?;
            let mut argument_names = BTreeSet::new();
            for argument in &field.arguments {
                require(
                    argument_names.insert(&argument.name),
                    format!("duplicate argument {name}.{field_name}.{}", argument.name),
                )?;
                require(
                    argument.arg_id == arg_id(name, field_name, &argument.name),
                    format!("invalid arg_id for {name}.{field_name}.{}", argument.name),
                )?;
                validate_type_ref(schema, &argument.type_ref, true)?;
            }
        }

        let mut enum_names = BTreeSet::new();
        for value in &definition.enum_values {
            require(
                enum_names.insert(&value.name),
                format!("duplicate enum value {name}.{}", value.name),
            )?;
        }

        for (field_name, field) in &definition.input_fields {
            require(
                field_name == &field.name,
                format!("input field map key mismatch: {name}.{field_name}"),
            )?;
            require(
                field.input_field_id == input_field_id(name, field_name),
                format!("invalid input_field_id for {name}.{field_name}"),
            )?;
            validate_type_ref(schema, &field.type_ref, true)?;
        }
    }

    validate_root(schema, "query", schema.roots.query.as_deref())?;
    validate_root(schema, "mutation", schema.roots.mutation.as_deref())?;
    validate_root(schema, "subscription", schema.roots.subscription.as_deref())?;
    Ok(())
}

fn validate_type_ref(
    schema: &SchemaIrData,
    type_ref: &TypeRef,
    input_position: bool,
) -> Result<(), ValidationError> {
    type_ref
        .validate()
        .map_err(|error| ValidationError::Invariant(error.to_string()))?;
    require(
        type_ref.display == type_ref.rendered(),
        format!("non-canonical TypeRef display: {}", type_ref.display),
    )?;
    let actual_kind = schema
        .types
        .get(&type_ref.named_type)
        .map(|definition| definition.kind)
        .or_else(|| builtin_scalar(&type_ref.named_type).then_some(TypeKind::Scalar))
        .ok_or_else(|| {
            ValidationError::Invariant(format!(
                "TypeRef references missing type {}",
                type_ref.named_type
            ))
        })?;
    require(
        actual_kind == type_ref.named_kind,
        format!("TypeRef kind mismatch for {}", type_ref.named_type),
    )?;
    let valid_position = if input_position {
        actual_kind.is_input_type()
    } else {
        actual_kind.is_output_type()
    };
    require(
        valid_position,
        format!(
            "type {} is invalid in {} position",
            type_ref.named_type,
            if input_position { "input" } else { "output" }
        ),
    )
}

fn validate_root(
    schema: &SchemaIrData,
    operation: &str,
    root: Option<&str>,
) -> Result<(), ValidationError> {
    let Some(root) = root else {
        return Ok(());
    };
    let definition = schema.types.get(root).ok_or_else(|| {
        ValidationError::Invariant(format!("{operation} root references missing type {root}"))
    })?;
    require(
        definition.kind == TypeKind::Object,
        format!("{operation} root {root} is not OBJECT"),
    )
}

fn definition_name(definition: &crate::contracts::TypeDefinition) -> &str {
    definition
        .type_id
        .strip_prefix("type:")
        .unwrap_or(&definition.type_id)
}

fn require(condition: bool, message: String) -> Result<(), ValidationError> {
    if condition {
        Ok(())
    } else {
        Err(ValidationError::Invariant(message))
    }
}

fn builtin_scalar(name: &str) -> bool {
    matches!(name, "String" | "Int" | "Float" | "Boolean" | "ID")
}
