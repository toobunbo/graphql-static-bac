use std::collections::BTreeMap;

use crate::contracts::{
    ArgumentsData, ClassifiedArgument, FieldDefinition, SchemaIrData, TypeDefinition,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct IndexedField<'a> {
    pub field: &'a FieldDefinition,
}

pub(crate) struct SeedIndex<'a> {
    pub schema: &'a SchemaIrData,
    fields: BTreeMap<String, IndexedField<'a>>,
    arguments: BTreeMap<(String, Vec<String>), &'a ClassifiedArgument>,
}

impl<'a> SeedIndex<'a> {
    pub fn build(schema: &'a SchemaIrData, arguments: &'a ArgumentsData) -> Self {
        let mut fields = BTreeMap::new();
        for definition in schema.types.values() {
            for field in definition.fields.values() {
                fields.insert(field.field_id.clone(), IndexedField { field });
            }
        }

        let mut classified = BTreeMap::new();
        for field in arguments.fields.values() {
            for argument in &field.arguments {
                classified.insert(
                    (argument.root_arg_ref.clone(), argument.input_path.clone()),
                    argument,
                );
            }
        }
        Self {
            schema,
            fields,
            arguments: classified,
        }
    }

    pub fn field(&self, field_id: &str) -> Option<IndexedField<'a>> {
        self.fields.get(field_id).copied()
    }

    pub fn type_by_id(&self, type_id: &str) -> Option<&'a TypeDefinition> {
        self.schema
            .types
            .values()
            .find(|definition| definition.type_id == type_id)
    }

    pub fn type_by_name(&self, type_name: &str) -> Option<&'a TypeDefinition> {
        self.schema.types.get(type_name)
    }

    pub fn classified_argument(
        &self,
        root_arg_ref: &str,
        input_path: &[String],
    ) -> Option<&'a ClassifiedArgument> {
        self.arguments
            .get(&(root_arg_ref.to_string(), input_path.to_vec()))
            .copied()
    }
}
