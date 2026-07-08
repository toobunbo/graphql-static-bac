use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::contracts::{
    ArgumentClassification, ArgumentDefinition, ArgumentsData, ClassifiedArgument, ClassifiedField,
    Confidence, InputFieldDefinition, SchemaIrData, TypeKind, TypeRef,
};
use crate::identifier::identifier_tokens;

use super::policy::LoadedArgumentPolicy;
use super::validation::{validate_arguments_data, ArgumentValidationError};

#[derive(Debug, Error)]
pub enum ArgumentClassifierError {
    #[error("invalid S0 argument graph: {0}")]
    Contract(String),
    #[error(transparent)]
    Validation(#[from] ArgumentValidationError),
}

pub fn classify_arguments(
    schema: &SchemaIrData,
    policy: &LoadedArgumentPolicy,
) -> Result<ArgumentsData, ArgumentClassifierError> {
    let mut fields = BTreeMap::new();
    for (owner_name, definition) in &schema.types {
        for field in definition.fields.values() {
            let mut arguments = Vec::new();
            for root in &field.arguments {
                match root.type_ref.named_kind {
                    TypeKind::Scalar | TypeKind::Enum => {
                        arguments.push(classify_direct(owner_name, &field.name, root, policy));
                    }
                    TypeKind::InputObject => {
                        let mut active_types = BTreeSet::new();
                        active_types.insert(root.type_ref.named_type.clone());
                        expand_input_object(
                            schema,
                            policy,
                            owner_name,
                            &field.name,
                            root,
                            &root.type_ref.named_type,
                            &mut Vec::new(),
                            &mut active_types,
                            &mut arguments,
                        )?;
                        if arguments
                            .iter()
                            .all(|argument| argument.root_arg_ref != root.arg_id)
                        {
                            arguments.push(classify_empty_input(
                                owner_name,
                                &field.name,
                                root,
                                policy,
                            ));
                        }
                    }
                    kind => {
                        return Err(ArgumentClassifierError::Contract(format!(
                            "argument {} has non-input kind {kind:?}",
                            root.arg_id
                        )));
                    }
                }
            }
            arguments.sort_by(|left, right| {
                left.root_arg_ref
                    .cmp(&right.root_arg_ref)
                    .then_with(|| left.input_path.cmp(&right.input_path))
                    .then_with(|| left.arg_ref.cmp(&right.arg_ref))
            });
            fields.insert(field.field_id.clone(), ClassifiedField { arguments });
        }
    }

    let data = ArgumentsData {
        classifier_model: Some(policy.policy.model_version.clone()),
        policy_fingerprint: Some(policy.fingerprint.clone()),
        fields,
    };
    validate_arguments_data(schema, &data)?;
    Ok(data)
}

#[allow(clippy::too_many_arguments)]
fn expand_input_object(
    schema: &SchemaIrData,
    policy: &LoadedArgumentPolicy,
    owner_name: &str,
    field_name: &str,
    root: &ArgumentDefinition,
    input_type_name: &str,
    input_path: &mut Vec<String>,
    active_types: &mut BTreeSet<String>,
    output: &mut Vec<ClassifiedArgument>,
) -> Result<(), ArgumentClassifierError> {
    let definition = schema.types.get(input_type_name).ok_or_else(|| {
        ArgumentClassifierError::Contract(format!(
            "{} references missing input object {input_type_name}",
            root.arg_id
        ))
    })?;
    if definition.kind != TypeKind::InputObject {
        return Err(ArgumentClassifierError::Contract(format!(
            "{} references {input_type_name}, which is not INPUT_OBJECT",
            root.arg_id
        )));
    }

    for input in definition.input_fields.values() {
        input_path.push(input.name.clone());
        match input.type_ref.named_kind {
            TypeKind::Scalar | TypeKind::Enum => output.push(classify_input_leaf(
                owner_name, field_name, root, input, input_path, policy,
            )),
            TypeKind::InputObject => {
                if active_types.contains(&input.type_ref.named_type) {
                    output.push(classify_cycle(
                        owner_name, field_name, root, input, input_path,
                    ));
                } else {
                    active_types.insert(input.type_ref.named_type.clone());
                    expand_input_object(
                        schema,
                        policy,
                        owner_name,
                        field_name,
                        root,
                        &input.type_ref.named_type,
                        input_path,
                        active_types,
                        output,
                    )?;
                    active_types.remove(&input.type_ref.named_type);
                }
            }
            kind => {
                return Err(ArgumentClassifierError::Contract(format!(
                    "{} has non-input kind {kind:?}",
                    input.input_field_id
                )));
            }
        }
        input_path.pop();
    }
    Ok(())
}

fn classify_direct(
    owner_name: &str,
    field_name: &str,
    root: &ArgumentDefinition,
    policy: &LoadedArgumentPolicy,
) -> ClassifiedArgument {
    classified(
        root.arg_id.clone(),
        root.arg_id.clone(),
        format!("{owner_name}.{field_name}.{}", root.name),
        Vec::new(),
        root.type_ref.clone(),
        &root.name,
        policy,
    )
}

fn classify_input_leaf(
    owner_name: &str,
    field_name: &str,
    root: &ArgumentDefinition,
    input: &InputFieldDefinition,
    input_path: &[String],
    policy: &LoadedArgumentPolicy,
) -> ClassifiedArgument {
    classified(
        input.input_field_id.clone(),
        root.arg_id.clone(),
        canonical_arg_path(owner_name, field_name, &root.name, input_path),
        input_path.to_vec(),
        input.type_ref.clone(),
        &input.name,
        policy,
    )
}

fn classify_empty_input(
    owner_name: &str,
    field_name: &str,
    root: &ArgumentDefinition,
    _policy: &LoadedArgumentPolicy,
) -> ClassifiedArgument {
    ClassifiedArgument {
        arg_ref: root.arg_id.clone(),
        root_arg_ref: root.arg_id.clone(),
        arg_path: format!("{owner_name}.{field_name}.{}", root.name),
        input_path: Vec::new(),
        type_ref: root.type_ref.clone(),
        classifications: vec![ArgumentClassification::PossibleSelector],
        signals: vec!["input:empty_object".to_string()],
        confidence: Confidence::Low,
    }
}

fn classify_cycle(
    owner_name: &str,
    field_name: &str,
    root: &ArgumentDefinition,
    input: &InputFieldDefinition,
    input_path: &[String],
) -> ClassifiedArgument {
    ClassifiedArgument {
        arg_ref: input.input_field_id.clone(),
        root_arg_ref: root.arg_id.clone(),
        arg_path: canonical_arg_path(owner_name, field_name, &root.name, input_path),
        input_path: input_path.to_vec(),
        type_ref: input.type_ref.clone(),
        classifications: vec![ArgumentClassification::PossibleSelector],
        signals: vec!["input:cycle_truncated".to_string()],
        confidence: Confidence::Low,
    }
}

#[allow(clippy::too_many_arguments)]
fn classified(
    arg_ref: String,
    root_arg_ref: String,
    arg_path: String,
    input_path: Vec<String>,
    type_ref: TypeRef,
    name: &str,
    policy: &LoadedArgumentPolicy,
) -> ClassifiedArgument {
    let mut classifications = BTreeSet::new();
    let mut signals = BTreeSet::new();
    let noise = policy.is_definite_noise(name);
    let authz = policy.is_authz_modifier(name);
    let graphql_id = type_ref.named_kind == TypeKind::Scalar && type_ref.named_type == "ID";
    let identity_scalar = type_ref.named_kind == TypeKind::Scalar
        && type_ref.named_type != "ID"
        && policy.is_identity_scalar(&type_ref.named_type);
    let exact_selector = !noise && policy.is_exact_selector(name);
    let suffix_selector = !noise
        && identifier_tokens(name)
            .last()
            .is_some_and(|token| policy.is_selector_suffix(token));
    let selector = graphql_id || identity_scalar || exact_selector || suffix_selector || authz;

    if graphql_id {
        signals.insert("type:graphql_id".to_string());
    }
    if identity_scalar {
        signals.insert(format!(
            "type:identity_scalar:{}",
            type_ref.named_type.to_ascii_lowercase()
        ));
    }
    if exact_selector {
        signals.insert(format!("name:exact_selector:{}", name.to_ascii_lowercase()));
    }
    if suffix_selector {
        let token = identifier_tokens(name)
            .last()
            .expect("suffix selector has a final token")
            .clone();
        signals.insert(format!("name:selector_suffix:{token}"));
    }
    if authz {
        classifications.insert(ArgumentClassification::AuthzModifier);
        signals.insert(format!("name:authz_modifier:{}", name.to_ascii_lowercase()));
    }
    if selector {
        classifications.insert(ArgumentClassification::ObjectSelector);
    }
    if noise {
        classifications.insert(ArgumentClassification::Noise);
        signals.insert(format!("name:definite_noise:{}", name.to_ascii_lowercase()));
    }
    if classifications.is_empty() {
        classifications.insert(ArgumentClassification::PossibleSelector);
        signals.insert("fallback:possible_selector".to_string());
    }

    let conflict = selector && noise;
    let confidence = if conflict {
        Confidence::Low
    } else if graphql_id || exact_selector || suffix_selector || authz {
        Confidence::High
    } else if identity_scalar {
        Confidence::Medium
    } else if noise {
        Confidence::High
    } else {
        Confidence::Low
    };

    ClassifiedArgument {
        arg_ref,
        root_arg_ref,
        arg_path,
        input_path,
        type_ref,
        classifications: classifications.into_iter().collect(),
        signals: signals.into_iter().collect(),
        confidence,
    }
}

fn canonical_arg_path(
    owner_name: &str,
    field_name: &str,
    root_name: &str,
    input_path: &[String],
) -> String {
    std::iter::once(owner_name)
        .chain(std::iter::once(field_name))
        .chain(std::iter::once(root_name))
        .chain(input_path.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(".")
}
