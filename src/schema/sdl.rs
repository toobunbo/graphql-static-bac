use std::collections::{BTreeMap, BTreeSet};

use graphql_parser::schema::{
    parse_schema, Definition, Document, EnumValue, Field, InputValue, Type,
    TypeDefinition as AstTypeDefinition, TypeExtension, Value,
};

use crate::contracts::{
    arg_id, field_id, input_field_id, type_id, ArgumentDefinition, EnumValueDefinition,
    FieldDefinition, InputFieldDefinition, SchemaIrData, SchemaRoots, TypeDefinition, TypeKind,
    TypeRef, TypeWrapper, Warning,
};

use super::{ParsedSchema, SchemaParseError};

pub fn parse_sdl(text: &str) -> Result<ParsedSchema, SchemaParseError> {
    let (rewritten, schema_extension_count) = rewrite_schema_extensions(text)?;
    let document = parse_schema::<String>(&rewritten)
        .map_err(|error| SchemaParseError::InvalidSdl(error.to_string()))?;
    let schema_definition_count = document
        .definitions
        .iter()
        .filter(|definition| matches!(definition, Definition::SchemaDefinition(_)))
        .count();
    let base_schema_definition_count =
        schema_definition_count.saturating_sub(schema_extension_count);
    if base_schema_definition_count > 1 {
        return Err(SchemaParseError::DuplicateDefinition(
            "schema definition".to_string(),
        ));
    }
    let kind_map = collect_type_kinds(&document)?;
    let mut roots = None;
    let mut types = BTreeMap::new();
    let mut has_applied_directives = false;

    for definition in &document.definitions {
        match definition {
            Definition::SchemaDefinition(schema) => {
                has_applied_directives |= !schema.directives.is_empty();
                merge_roots(
                    &mut roots,
                    SchemaRoots {
                        query: schema.query.clone(),
                        mutation: schema.mutation.clone(),
                        subscription: schema.subscription.clone(),
                    },
                )?;
            }
            Definition::TypeDefinition(definition) => {
                let converted = convert_definition(definition, &kind_map)?;
                let name = type_name(definition).to_string();
                has_applied_directives |= definition_has_directives(definition);
                if types.insert(name.clone(), converted).is_some() {
                    return Err(SchemaParseError::DuplicateDefinition(name));
                }
            }
            Definition::TypeExtension(extension) => {
                has_applied_directives |= extension_has_directives(extension);
            }
            Definition::DirectiveDefinition(_) => {}
        }
    }

    for definition in &document.definitions {
        if let Definition::TypeExtension(extension) = definition {
            apply_extension(&mut types, extension, &kind_map)?;
        }
    }

    populate_interface_possible_types(&mut types);
    let roots = match roots {
        Some(mut roots) if base_schema_definition_count == 0 => {
            roots.query = roots
                .query
                .or_else(|| types.contains_key("Query").then(|| "Query".to_string()));
            roots.mutation = roots.mutation.or_else(|| {
                types
                    .contains_key("Mutation")
                    .then(|| "Mutation".to_string())
            });
            roots.subscription = roots.subscription.or_else(|| {
                types
                    .contains_key("Subscription")
                    .then(|| "Subscription".to_string())
            });
            roots
        }
        Some(roots) => roots,
        None => SchemaRoots {
            query: types.contains_key("Query").then(|| "Query".to_string()),
            mutation: types
                .contains_key("Mutation")
                .then(|| "Mutation".to_string()),
            subscription: types
                .contains_key("Subscription")
                .then(|| "Subscription".to_string()),
        },
    };
    let warnings = if has_applied_directives {
        vec![Warning::new(
            "applied_directives_omitted",
            "SDL applied directives were parsed but are not representable in contract version 1.0.",
        )]
    } else {
        Vec::new()
    };
    Ok(ParsedSchema {
        data: SchemaIrData { roots, types },
        warnings,
    })
}

fn merge_roots(
    target: &mut Option<SchemaRoots>,
    incoming: SchemaRoots,
) -> Result<(), SchemaParseError> {
    let target = target.get_or_insert(SchemaRoots {
        query: None,
        mutation: None,
        subscription: None,
    });
    merge_root_value("query", &mut target.query, incoming.query)?;
    merge_root_value("mutation", &mut target.mutation, incoming.mutation)?;
    merge_root_value(
        "subscription",
        &mut target.subscription,
        incoming.subscription,
    )
}

fn merge_root_value(
    operation: &str,
    target: &mut Option<String>,
    incoming: Option<String>,
) -> Result<(), SchemaParseError> {
    let Some(incoming) = incoming else {
        return Ok(());
    };
    match target {
        Some(existing) if existing != &incoming => Err(SchemaParseError::DuplicateDefinition(
            format!("conflicting {operation} roots: {existing} and {incoming}"),
        )),
        Some(_) => Err(SchemaParseError::DuplicateDefinition(format!(
            "duplicate {operation} root"
        ))),
        None => {
            *target = Some(incoming);
            Ok(())
        }
    }
}

fn rewrite_schema_extensions(text: &str) -> Result<(String, usize), SchemaParseError> {
    let bytes = text.as_bytes();
    let mut rewritten = bytes.to_vec();
    let mut index = 0;
    let mut brace_depth = 0usize;
    let mut schema_extension_count = 0usize;
    let mut insertions = Vec::new();
    while index < bytes.len() {
        match bytes[index] {
            b'#' => {
                index = skip_comment(bytes, index);
            }
            b'"' if bytes.get(index..index + 3) == Some(b"\"\"\"") => {
                index = skip_block_string(bytes, index)?;
            }
            b'"' => {
                index = skip_string(bytes, index)?;
            }
            byte if is_name_start(byte) => {
                let end = skip_name(bytes, index);
                if brace_depth == 0 && &bytes[index..end] == b"extend" {
                    let next = skip_ignored(bytes, end);
                    let next_end = skip_name(bytes, next);
                    if next < bytes.len() && &bytes[next..next_end] == b"schema" {
                        rewritten[index..end].fill(b' ');
                        schema_extension_count += 1;
                        if let Some(insertion) = directive_only_schema_extension(bytes, next_end)? {
                            insertions.push(insertion);
                        }
                        index = next_end;
                        continue;
                    }
                }
                index = end;
            }
            b'{' => {
                brace_depth += 1;
                index += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                index += 1;
            }
            _ => index += 1,
        }
    }
    let mut rewritten = String::from_utf8(rewritten)
        .map_err(|error| SchemaParseError::InvalidSdl(error.to_string()))?;
    for insertion in insertions.into_iter().rev() {
        rewritten.insert_str(insertion, " {}");
    }
    Ok((rewritten, schema_extension_count))
}

fn directive_only_schema_extension(
    bytes: &[u8],
    mut index: usize,
) -> Result<Option<usize>, SchemaParseError> {
    let mut has_directive = false;
    loop {
        index = skip_ignored(bytes, index);
        if bytes.get(index) != Some(&b'@') {
            break;
        }
        has_directive = true;
        index = skip_ignored(bytes, index + 1);
        if !bytes.get(index).is_some_and(|byte| is_name_start(*byte)) {
            return Err(SchemaParseError::InvalidSdl(
                "schema extension directive is missing a name".to_string(),
            ));
        }
        index = skip_name(bytes, index);
        index = skip_ignored(bytes, index);
        if bytes.get(index) == Some(&b'(') {
            index = skip_balanced(bytes, index, b'(', b')')?;
        }
    }
    index = skip_ignored(bytes, index);
    if bytes.get(index) == Some(&b'{') || !has_directive {
        Ok(None)
    } else {
        Ok(Some(index))
    }
}

fn skip_balanced(
    bytes: &[u8],
    mut index: usize,
    open: u8,
    close: u8,
) -> Result<usize, SchemaParseError> {
    let mut stack = vec![close];
    index += 1;
    while index < bytes.len() {
        match bytes[index] {
            b'#' => index = skip_comment(bytes, index),
            b'"' if bytes.get(index..index + 3) == Some(b"\"\"\"") => {
                index = skip_block_string(bytes, index)?;
            }
            b'"' => index = skip_string(bytes, index)?,
            b'(' => {
                stack.push(b')');
                index += 1;
            }
            b'[' => {
                stack.push(b']');
                index += 1;
            }
            b'{' => {
                stack.push(b'}');
                index += 1;
            }
            byte if Some(&byte) == stack.last() => {
                stack.pop();
                index += 1;
                if stack.is_empty() {
                    return Ok(index);
                }
            }
            _ => index += 1,
        }
    }
    Err(SchemaParseError::InvalidSdl(format!(
        "unterminated delimiter {}...{}",
        open as char, close as char
    )))
}

fn skip_ignored(bytes: &[u8], mut index: usize) -> usize {
    loop {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) == Some(&b'#') {
            index = skip_comment(bytes, index);
            continue;
        }
        return index;
    }
}

fn skip_comment(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

fn skip_string(bytes: &[u8], mut index: usize) -> Result<usize, SchemaParseError> {
    index += 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Ok(index + 1),
            _ => index += 1,
        }
    }
    Err(SchemaParseError::InvalidSdl(
        "unterminated string literal".to_string(),
    ))
}

fn skip_block_string(bytes: &[u8], mut index: usize) -> Result<usize, SchemaParseError> {
    index += 3;
    while index + 2 < bytes.len() {
        if &bytes[index..index + 3] == b"\"\"\"" {
            return Ok(index + 3);
        }
        index += 1;
    }
    Err(SchemaParseError::InvalidSdl(
        "unterminated block string literal".to_string(),
    ))
}

fn skip_name(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(|byte| is_name_continue(*byte)) {
        index += 1;
    }
    index
}

fn is_name_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_name_continue(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit()
}

fn collect_type_kinds(
    document: &Document<'_, String>,
) -> Result<BTreeMap<String, TypeKind>, SchemaParseError> {
    let mut kinds = BTreeMap::new();
    for definition in &document.definitions {
        if let Definition::TypeDefinition(definition) = definition {
            let name = type_name(definition).to_string();
            let kind = definition_kind(definition);
            if kinds.insert(name.clone(), kind).is_some() {
                return Err(SchemaParseError::DuplicateDefinition(name));
            }
        }
    }
    Ok(kinds)
}

fn convert_definition(
    definition: &AstTypeDefinition<'_, String>,
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<TypeDefinition, SchemaParseError> {
    let name = type_name(definition);
    let mut converted = empty_type(
        name,
        definition_kind(definition),
        definition_description(definition),
    );
    match definition {
        AstTypeDefinition::Scalar(_) => {}
        AstTypeDefinition::Object(object) => {
            converted.interfaces = sorted_names(&object.implements_interfaces);
            insert_fields(&mut converted.fields, name, &object.fields, kinds)?;
        }
        AstTypeDefinition::Interface(interface) => {
            converted.interfaces = sorted_names(&interface.implements_interfaces);
            insert_fields(&mut converted.fields, name, &interface.fields, kinds)?;
        }
        AstTypeDefinition::Union(union) => {
            converted.possible_types = sorted_names(&union.types);
        }
        AstTypeDefinition::Enum(enumeration) => {
            converted.enum_values = convert_enum_values(&enumeration.values);
        }
        AstTypeDefinition::InputObject(input) => {
            insert_input_fields(&mut converted.input_fields, name, &input.fields, kinds)?;
        }
    }
    Ok(converted)
}

fn apply_extension(
    types: &mut BTreeMap<String, TypeDefinition>,
    extension: &TypeExtension<'_, String>,
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<(), SchemaParseError> {
    let (name, expected_kind) = extension_name_kind(extension);
    let target = types.get_mut(name).ok_or_else(|| {
        SchemaParseError::DuplicateDefinition(format!("extension for unknown type {name}"))
    })?;
    if target.kind != expected_kind {
        return Err(SchemaParseError::DuplicateDefinition(format!(
            "extension kind mismatch for {name}"
        )));
    }
    match extension {
        TypeExtension::Scalar(_) => {}
        TypeExtension::Object(object) => {
            merge_names(&mut target.interfaces, &object.implements_interfaces);
            insert_fields(&mut target.fields, name, &object.fields, kinds)?;
        }
        TypeExtension::Interface(interface) => {
            merge_names(&mut target.interfaces, &interface.implements_interfaces);
            insert_fields(&mut target.fields, name, &interface.fields, kinds)?;
        }
        TypeExtension::Union(union) => merge_names(&mut target.possible_types, &union.types),
        TypeExtension::Enum(enumeration) => {
            for value in convert_enum_values(&enumeration.values) {
                if target
                    .enum_values
                    .iter()
                    .any(|current| current.name == value.name)
                {
                    return Err(SchemaParseError::DuplicateDefinition(format!(
                        "{name}.{}",
                        value.name
                    )));
                }
                target.enum_values.push(value);
            }
            target
                .enum_values
                .sort_by(|left, right| left.name.cmp(&right.name));
        }
        TypeExtension::InputObject(input) => {
            insert_input_fields(&mut target.input_fields, name, &input.fields, kinds)?;
        }
    }
    Ok(())
}

fn insert_fields(
    output: &mut BTreeMap<String, FieldDefinition>,
    owner: &str,
    fields: &[Field<'_, String>],
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<(), SchemaParseError> {
    for field in fields {
        let mut arguments = field
            .arguments
            .iter()
            .map(|argument| convert_argument(owner, &field.name, argument, kinds))
            .collect::<Result<Vec<_>, _>>()?;
        arguments.sort_by(|left, right| left.name.cmp(&right.name));
        let converted = FieldDefinition {
            field_id: field_id(owner, &field.name),
            name: field.name.clone(),
            description: field.description.clone(),
            arguments,
            return_type: convert_ast_type(&field.field_type, kinds)?,
        };
        if output.insert(field.name.clone(), converted).is_some() {
            return Err(SchemaParseError::DuplicateDefinition(format!(
                "{owner}.{}",
                field.name
            )));
        }
    }
    Ok(())
}

fn insert_input_fields(
    output: &mut BTreeMap<String, InputFieldDefinition>,
    owner: &str,
    fields: &[InputValue<'_, String>],
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<(), SchemaParseError> {
    for field in fields {
        let converted = InputFieldDefinition {
            input_field_id: input_field_id(owner, &field.name),
            name: field.name.clone(),
            description: field.description.clone(),
            default_value: field.default_value.as_ref().map(render_value),
            type_ref: convert_ast_type(&field.value_type, kinds)?,
        };
        if output.insert(field.name.clone(), converted).is_some() {
            return Err(SchemaParseError::DuplicateDefinition(format!(
                "{owner}.{}",
                field.name
            )));
        }
    }
    Ok(())
}

fn convert_argument(
    owner: &str,
    field: &str,
    argument: &InputValue<'_, String>,
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<ArgumentDefinition, SchemaParseError> {
    Ok(ArgumentDefinition {
        arg_id: arg_id(owner, field, &argument.name),
        name: argument.name.clone(),
        description: argument.description.clone(),
        default_value: argument.default_value.as_ref().map(render_value),
        type_ref: convert_ast_type(&argument.value_type, kinds)?,
    })
}

fn convert_ast_type(
    type_ref: &Type<'_, String>,
    kinds: &BTreeMap<String, TypeKind>,
) -> Result<TypeRef, SchemaParseError> {
    let mut wrappers = Vec::new();
    let mut current = type_ref;
    loop {
        match current {
            Type::NamedType(name) => {
                let kind = kinds
                    .get(name)
                    .copied()
                    .or_else(|| builtin_scalar(name).then_some(TypeKind::Scalar))
                    .ok_or_else(|| {
                        SchemaParseError::InvalidSdl(format!("unknown named type {name}"))
                    })?;
                return TypeRef::new(name.clone(), kind, wrappers)
                    .map_err(|error| SchemaParseError::InvalidSdl(error.to_string()));
            }
            Type::ListType(inner) => {
                wrappers.push(TypeWrapper::List);
                current = inner;
            }
            Type::NonNullType(inner) => {
                wrappers.push(TypeWrapper::NonNull);
                current = inner;
            }
        }
    }
}

fn convert_enum_values(values: &[EnumValue<'_, String>]) -> Vec<EnumValueDefinition> {
    let mut converted = values
        .iter()
        .map(|value| {
            let deprecated = value
                .directives
                .iter()
                .find(|directive| directive.name == "deprecated");
            let reason = deprecated.and_then(|directive| {
                directive
                    .arguments
                    .iter()
                    .find(|(name, _)| name == "reason")
                    .map(|(_, value)| match value {
                        Value::String(value) => value.clone(),
                        other => render_value(other),
                    })
            });
            EnumValueDefinition {
                name: value.name.clone(),
                description: value.description.clone(),
                is_deprecated: deprecated.is_some(),
                deprecation_reason: reason,
            }
        })
        .collect::<Vec<_>>();
    converted.sort_by(|left, right| left.name.cmp(&right.name));
    converted
}

fn populate_interface_possible_types(types: &mut BTreeMap<String, TypeDefinition>) {
    let relationships = types
        .iter()
        .filter(|(_, definition)| definition.kind == TypeKind::Object)
        .flat_map(|(name, definition)| {
            definition
                .interfaces
                .iter()
                .map(move |interface| (interface.clone(), name.clone()))
        })
        .collect::<Vec<_>>();
    for (interface, object) in relationships {
        if let Some(definition) = types.get_mut(&interface) {
            definition.possible_types.push(object);
            definition.possible_types.sort();
            definition.possible_types.dedup();
        }
    }
}

fn empty_type(name: &str, kind: TypeKind, description: Option<String>) -> TypeDefinition {
    TypeDefinition {
        type_id: type_id(name),
        kind,
        description,
        interfaces: Vec::new(),
        possible_types: Vec::new(),
        fields: BTreeMap::new(),
        input_fields: BTreeMap::new(),
        enum_values: Vec::new(),
    }
}

fn definition_kind(definition: &AstTypeDefinition<'_, String>) -> TypeKind {
    match definition {
        AstTypeDefinition::Scalar(_) => TypeKind::Scalar,
        AstTypeDefinition::Object(_) => TypeKind::Object,
        AstTypeDefinition::Interface(_) => TypeKind::Interface,
        AstTypeDefinition::Union(_) => TypeKind::Union,
        AstTypeDefinition::Enum(_) => TypeKind::Enum,
        AstTypeDefinition::InputObject(_) => TypeKind::InputObject,
    }
}

fn type_name<'a>(definition: &'a AstTypeDefinition<'_, String>) -> &'a str {
    match definition {
        AstTypeDefinition::Scalar(value) => &value.name,
        AstTypeDefinition::Object(value) => &value.name,
        AstTypeDefinition::Interface(value) => &value.name,
        AstTypeDefinition::Union(value) => &value.name,
        AstTypeDefinition::Enum(value) => &value.name,
        AstTypeDefinition::InputObject(value) => &value.name,
    }
}

fn definition_description(definition: &AstTypeDefinition<'_, String>) -> Option<String> {
    match definition {
        AstTypeDefinition::Scalar(value) => value.description.clone(),
        AstTypeDefinition::Object(value) => value.description.clone(),
        AstTypeDefinition::Interface(value) => value.description.clone(),
        AstTypeDefinition::Union(value) => value.description.clone(),
        AstTypeDefinition::Enum(value) => value.description.clone(),
        AstTypeDefinition::InputObject(value) => value.description.clone(),
    }
}

fn extension_name_kind<'a>(extension: &'a TypeExtension<'_, String>) -> (&'a str, TypeKind) {
    match extension {
        TypeExtension::Scalar(value) => (&value.name, TypeKind::Scalar),
        TypeExtension::Object(value) => (&value.name, TypeKind::Object),
        TypeExtension::Interface(value) => (&value.name, TypeKind::Interface),
        TypeExtension::Union(value) => (&value.name, TypeKind::Union),
        TypeExtension::Enum(value) => (&value.name, TypeKind::Enum),
        TypeExtension::InputObject(value) => (&value.name, TypeKind::InputObject),
    }
}

fn definition_has_directives(definition: &AstTypeDefinition<'_, String>) -> bool {
    match definition {
        AstTypeDefinition::Scalar(value) => !value.directives.is_empty(),
        AstTypeDefinition::Object(value) => {
            !value.directives.is_empty()
                || value.fields.iter().any(|field| {
                    !field.directives.is_empty()
                        || field.arguments.iter().any(|arg| !arg.directives.is_empty())
                })
        }
        AstTypeDefinition::Interface(value) => {
            !value.directives.is_empty()
                || value.fields.iter().any(|field| {
                    !field.directives.is_empty()
                        || field.arguments.iter().any(|arg| !arg.directives.is_empty())
                })
        }
        AstTypeDefinition::Union(value) => !value.directives.is_empty(),
        AstTypeDefinition::Enum(value) => {
            !value.directives.is_empty()
                || value.values.iter().any(|item| {
                    item.directives
                        .iter()
                        .any(|directive| directive.name != "deprecated")
                })
        }
        AstTypeDefinition::InputObject(value) => {
            !value.directives.is_empty()
                || value
                    .fields
                    .iter()
                    .any(|field| !field.directives.is_empty())
        }
    }
}

fn extension_has_directives(extension: &TypeExtension<'_, String>) -> bool {
    match extension {
        TypeExtension::Scalar(value) => !value.directives.is_empty(),
        TypeExtension::Object(value) => {
            !value.directives.is_empty()
                || value
                    .fields
                    .iter()
                    .any(|field| !field.directives.is_empty())
        }
        TypeExtension::Interface(value) => {
            !value.directives.is_empty()
                || value
                    .fields
                    .iter()
                    .any(|field| !field.directives.is_empty())
        }
        TypeExtension::Union(value) => !value.directives.is_empty(),
        TypeExtension::Enum(value) => {
            !value.directives.is_empty()
                || value.values.iter().any(|item| !item.directives.is_empty())
        }
        TypeExtension::InputObject(value) => {
            !value.directives.is_empty()
                || value
                    .fields
                    .iter()
                    .any(|field| !field.directives.is_empty())
        }
    }
}

fn render_value(value: &Value<'_, String>) -> String {
    match value {
        Value::Variable(name) => format!("${name}"),
        Value::Int(number) => number.as_i64().unwrap_or_default().to_string(),
        Value::Float(number) => number.to_string(),
        Value::String(value) => {
            serde_json::to_string(value).expect("string serialization cannot fail")
        }
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Enum(value) => value.clone(),
        Value::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(name, value)| format!("{name}: {}", render_value(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn sorted_names(values: &[String]) -> Vec<String> {
    let mut names = values.to_vec();
    names.sort();
    names.dedup();
    names
}

fn merge_names(target: &mut Vec<String>, values: &[String]) {
    let mut merged = target.iter().cloned().collect::<BTreeSet<_>>();
    merged.extend(values.iter().cloned());
    *target = merged.into_iter().collect();
}

fn builtin_scalar(name: &str) -> bool {
    matches!(name, "String" | "Int" | "Float" | "Boolean" | "ID")
}
