use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{TypeKind, TypeRef};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaRoots {
    pub query: Option<String>,
    pub mutation: Option<String>,
    pub subscription: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaIrData {
    pub roots: SchemaRoots,
    pub types: BTreeMap<String, TypeDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeDefinition {
    pub type_id: String,
    pub kind: TypeKind,
    pub description: Option<String>,
    pub interfaces: Vec<String>,
    pub possible_types: Vec<String>,
    pub fields: BTreeMap<String, FieldDefinition>,
    pub input_fields: BTreeMap<String, InputFieldDefinition>,
    pub enum_values: Vec<EnumValueDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub field_id: String,
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<ArgumentDefinition>,
    pub return_type: TypeRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgumentDefinition {
    pub arg_id: String,
    pub name: String,
    pub description: Option<String>,
    pub default_value: Option<String>,
    pub type_ref: TypeRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputFieldDefinition {
    pub input_field_id: String,
    pub name: String,
    pub description: Option<String>,
    pub default_value: Option<String>,
    pub type_ref: TypeRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumValueDefinition {
    pub name: String,
    pub description: Option<String>,
    pub is_deprecated: bool,
    pub deprecation_reason: Option<String>,
}
