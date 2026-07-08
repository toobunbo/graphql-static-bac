use std::collections::BTreeMap;

use serde::Deserialize;

use crate::contracts::{
    arg_id, field_id, input_field_id, type_id, ArgumentDefinition, EnumValueDefinition,
    FieldDefinition, InputFieldDefinition, SchemaIrData, SchemaRoots, TypeDefinition, TypeKind,
    TypeRef, TypeWrapper, Warning,
};

use super::{ParsedSchema, SchemaParseError};

#[derive(Debug, Deserialize)]
struct Response {
    data: Option<ResponseData>,
    #[serde(rename = "__schema")]
    schema: Option<IntrospectionSchema>,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ResponseData {
    #[serde(rename = "__schema")]
    schema: Option<IntrospectionSchema>,
}

#[derive(Debug, Deserialize)]
struct IntrospectionSchema {
    #[serde(rename = "queryType")]
    query_type: Option<NamedRef>,
    #[serde(rename = "mutationType")]
    mutation_type: Option<NamedRef>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<NamedRef>,
    types: Vec<IntrospectionType>,
}

#[derive(Debug, Deserialize)]
struct NamedRef {
    name: String,
}

#[derive(Debug, Deserialize)]
struct IntrospectionType {
    kind: String,
    name: Option<String>,
    description: Option<String>,
    fields: Option<Vec<IntrospectionField>>,
    #[serde(rename = "inputFields")]
    input_fields: Option<Vec<IntrospectionInputValue>>,
    interfaces: Option<Vec<NamedRef>>,
    #[serde(rename = "possibleTypes")]
    possible_types: Option<Vec<NamedRef>>,
    #[serde(rename = "enumValues")]
    enum_values: Option<Vec<IntrospectionEnumValue>>,
}

#[derive(Debug, Deserialize)]
struct IntrospectionField {
    name: String,
    description: Option<String>,
    #[serde(default)]
    args: Vec<IntrospectionInputValue>,
    #[serde(rename = "type")]
    type_ref: IntrospectionTypeRef,
}

#[derive(Debug, Deserialize)]
struct IntrospectionInputValue {
    name: String,
    description: Option<String>,
    #[serde(rename = "defaultValue")]
    default_value: Option<String>,
    #[serde(rename = "type")]
    type_ref: IntrospectionTypeRef,
}

#[derive(Debug, Deserialize)]
struct IntrospectionEnumValue {
    name: String,
    description: Option<String>,
    #[serde(rename = "isDeprecated", default)]
    is_deprecated: bool,
    #[serde(rename = "deprecationReason")]
    deprecation_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IntrospectionTypeRef {
    kind: String,
    name: Option<String>,
    #[serde(rename = "ofType")]
    of_type: Option<Box<IntrospectionTypeRef>>,
}

pub fn parse_introspection(bytes: &[u8]) -> Result<ParsedSchema, SchemaParseError> {
    let response: Response = serde_json::from_slice(bytes)?;
    let has_errors = !response.errors.is_empty();
    let schema = response
        .data
        .and_then(|data| data.schema)
        .or(response.schema)
        .ok_or(SchemaParseError::MissingSchema)?;

    let kind_map = schema
        .types
        .iter()
        .filter_map(|item| {
            item.name
                .as_ref()
                .map(|name| parse_kind(&item.kind).map(|kind| (name.clone(), kind)))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let mut types = BTreeMap::new();
    for item in schema.types {
        let Some(name) = item.name else {
            continue;
        };
        let kind = parse_kind(&item.kind)?;
        let mut fields = BTreeMap::new();
        for field in item.fields.unwrap_or_default() {
            let definition = convert_field(&name, field, &kind_map)?;
            let field_name = definition.name.clone();
            if fields.insert(field_name.clone(), definition).is_some() {
                return Err(SchemaParseError::DuplicateDefinition(format!(
                    "{name}.{field_name}"
                )));
            }
        }
        let mut input_fields = BTreeMap::new();
        for field in item.input_fields.unwrap_or_default() {
            let definition = convert_input_field(&name, field, &kind_map)?;
            let field_name = definition.name.clone();
            if input_fields
                .insert(field_name.clone(), definition)
                .is_some()
            {
                return Err(SchemaParseError::DuplicateDefinition(format!(
                    "{name}.{field_name}"
                )));
            }
        }
        let mut interfaces = names(item.interfaces);
        let mut possible_types = names(item.possible_types);
        interfaces.sort();
        interfaces.dedup();
        possible_types.sort();
        possible_types.dedup();
        let mut enum_values = item
            .enum_values
            .unwrap_or_default()
            .into_iter()
            .map(|value| EnumValueDefinition {
                name: value.name,
                description: value.description,
                is_deprecated: value.is_deprecated,
                deprecation_reason: value.deprecation_reason,
            })
            .collect::<Vec<_>>();
        enum_values.sort_by(|left, right| left.name.cmp(&right.name));

        let definition = TypeDefinition {
            type_id: type_id(&name),
            kind,
            description: item.description,
            interfaces,
            possible_types,
            fields,
            input_fields,
            enum_values,
        };
        if types.insert(name.clone(), definition).is_some() {
            return Err(SchemaParseError::DuplicateDefinition(name));
        }
    }

    let warnings = if has_errors {
        vec![Warning::new(
            "introspection_response_errors",
            "The response contained GraphQL errors alongside a usable schema.",
        )]
    } else {
        Vec::new()
    };
    Ok(ParsedSchema {
        data: SchemaIrData {
            roots: SchemaRoots {
                query: schema.query_type.map(|value| value.name),
                mutation: schema.mutation_type.map(|value| value.name),
                subscription: schema.subscription_type.map(|value| value.name),
            },
            types,
        },
        warnings,
    })
}

fn convert_field(
    owner: &str,
    field: IntrospectionField,
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<FieldDefinition, SchemaParseError> {
    let mut arguments = field
        .args
        .into_iter()
        .map(|argument| {
            Ok(ArgumentDefinition {
                arg_id: arg_id(owner, &field.name, &argument.name),
                name: argument.name,
                description: argument.description,
                default_value: argument.default_value,
                type_ref: convert_type_ref(argument.type_ref, kinds)?,
            })
        })
        .collect::<Result<Vec<_>, SchemaParseError>>()?;
    arguments.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(FieldDefinition {
        field_id: field_id(owner, &field.name),
        name: field.name,
        description: field.description,
        arguments,
        return_type: convert_type_ref(field.type_ref, kinds)?,
    })
}

fn convert_input_field(
    owner: &str,
    field: IntrospectionInputValue,
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<InputFieldDefinition, SchemaParseError> {
    Ok(InputFieldDefinition {
        input_field_id: input_field_id(owner, &field.name),
        name: field.name,
        description: field.description,
        default_value: field.default_value,
        type_ref: convert_type_ref(field.type_ref, kinds)?,
    })
}

fn convert_type_ref(
    raw: IntrospectionTypeRef,
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<TypeRef, SchemaParseError> {
    let mut wrappers = Vec::new();
    let mut current = raw;
    loop {
        match current.kind.as_str() {
            "NON_NULL" => {
                wrappers.push(TypeWrapper::NonNull);
                current = *current.of_type.ok_or(SchemaParseError::TruncatedTypeRef)?;
            }
            "LIST" => {
                wrappers.push(TypeWrapper::List);
                current = *current.of_type.ok_or(SchemaParseError::TruncatedTypeRef)?;
            }
            _ => {
                let name = current.name.ok_or(SchemaParseError::TruncatedTypeRef)?;
                let parsed_kind = parse_kind(&current.kind)?;
                let named_kind = kinds.get(&name).copied().unwrap_or(parsed_kind);
                if named_kind != parsed_kind {
                    return Err(SchemaParseError::InvalidIntrospection(format!(
                        "type reference kind mismatch for {name}: source says {parsed_kind:?}, definition says {named_kind:?}"
                    )));
                }
                return TypeRef::new(name, named_kind, wrappers)
                    .map_err(|error| SchemaParseError::InvalidIntrospection(error.to_string()));
            }
        }
    }
}

fn parse_kind(value: &str) -> Result<TypeKind, SchemaParseError> {
    match value {
        "OBJECT" => Ok(TypeKind::Object),
        "INTERFACE" => Ok(TypeKind::Interface),
        "UNION" => Ok(TypeKind::Union),
        "INPUT_OBJECT" => Ok(TypeKind::InputObject),
        "ENUM" => Ok(TypeKind::Enum),
        "SCALAR" => Ok(TypeKind::Scalar),
        other => Err(SchemaParseError::UnknownKind(other.to_string())),
    }
}

fn names(values: Option<Vec<NamedRef>>) -> Vec<String> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.name)
        .collect()
}
