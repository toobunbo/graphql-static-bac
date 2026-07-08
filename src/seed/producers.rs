use std::collections::BTreeMap;

use crate::contracts::{
    Confidence, ProducerCandidate, ProducerDerivation, ProducerLocus, SeedRequirement,
    TypeCompatibility, TypeKind,
};

use super::index::SeedIndex;

pub(crate) fn derive_producers(
    requirement: &SeedRequirement,
    index: &SeedIndex<'_>,
) -> Vec<ProducerCandidate> {
    if requirement
        .static_bindings
        .iter()
        .any(|binding| binding.value.is_some())
    {
        return Vec::new();
    }

    let Some(selected) = index.type_by_id(&requirement.selected_type_id) else {
        return Vec::new();
    };
    let mut candidates = BTreeMap::<String, ProducerCandidate>::new();

    add_exact_fields(
        &mut candidates,
        selected,
        requirement,
        locus_for_selected(selected.kind),
        ProducerDerivation::ExactLeafMatch,
        index,
    );

    for interface_name in &selected.interfaces {
        if let Some(interface) = index.type_by_name(interface_name) {
            add_exact_fields(
                &mut candidates,
                interface,
                requirement,
                ProducerLocus::Interface,
                ProducerDerivation::RelatedInterfaceField,
                index,
            );
        }
    }

    if matches!(selected.kind, TypeKind::Interface | TypeKind::Union) {
        for possible_type in &selected.possible_types {
            if let Some(concrete) = index.type_by_name(possible_type) {
                add_exact_fields(
                    &mut candidates,
                    concrete,
                    requirement,
                    ProducerLocus::Concrete,
                    ProducerDerivation::RelatedConcreteField,
                    index,
                );
            }
        }
    }

    candidates.into_values().collect()
}

fn add_exact_fields(
    candidates: &mut BTreeMap<String, ProducerCandidate>,
    definition: &crate::contracts::TypeDefinition,
    requirement: &SeedRequirement,
    field_locus: ProducerLocus,
    derivation: ProducerDerivation,
    _index: &SeedIndex<'_>,
) {
    let field_names = producer_field_names(&requirement.leaf_name);
    let Some(field) = field_names
        .iter()
        .find_map(|name| definition.fields.get(name))
    else {
        return;
    };
    if !matches!(
        field.return_type.named_kind,
        TypeKind::Scalar | TypeKind::Enum
    ) {
        return;
    }
    let Some(type_compatibility) = compatible_type(
        &requirement.leaf_name,
        &requirement.type_ref.named_type,
        &field.return_type.named_type,
    ) else {
        return;
    };
    let derivation = if type_compatibility == TypeCompatibility::IdString
        || field.name != requirement.leaf_name
    {
        ProducerDerivation::IdentityCompatible
    } else {
        derivation
    };
    candidates.insert(
        field.field_id.clone(),
        ProducerCandidate {
            producer_field_id: field.field_id.clone(),
            producer_parent_type_id: definition.type_id.clone(),
            derivation,
            field_locus,
            type_compatibility,
            confidence: Confidence::High,
            automatic: true,
        },
    );
}

fn producer_field_names(consumer_leaf: &str) -> Vec<String> {
    let mut names = vec![consumer_leaf.to_string()];
    if consumer_leaf == "ids" {
        names.push("id".to_string());
    } else if let Some(singular) = consumer_leaf.strip_suffix('s') {
        if matches!(singular, "slug" | "assetId" | "uuid") {
            names.push(singular.to_string());
        }
    }
    names
}

fn compatible_type(
    leaf_name: &str,
    consumer_type: &str,
    producer_type: &str,
) -> Option<TypeCompatibility> {
    if consumer_type == producer_type {
        return Some(TypeCompatibility::Exact);
    }
    if is_identity_name(leaf_name)
        && matches!(
            (consumer_type, producer_type),
            ("ID", "String") | ("String", "ID")
        )
    {
        return Some(TypeCompatibility::IdString);
    }
    None
}

fn is_identity_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "id" || lower.ends_with("id") || lower.ends_with("_id")
}

fn locus_for_selected(kind: TypeKind) -> ProducerLocus {
    match kind {
        TypeKind::Interface => ProducerLocus::Interface,
        _ => ProducerLocus::Object,
    }
}
