use std::path::{Path, PathBuf};

use graphql_static_bac::argument::{classify_arguments, read_argument_policy};
use graphql_static_bac::contracts::{
    ArgumentClassification, Confidence, Envelope, Producer, StageId,
};
use graphql_static_bac::schema::parse_sdl;
use graphql_static_bac::stages::s2_arguments::classify_schema_arguments;

fn policy_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("config/lexicons/argument-classifier-v1.json")
}

fn fixture_schema() -> Envelope<graphql_static_bac::contracts::SchemaIrData> {
    let parsed = parse_sdl(
        r#"
        scalar UUID
        enum Sport { FOOTBALL BASEBALL }

        input NestedFilter {
          ownerId: ID!
          sport: Sport
          first: Int
          next: NestedFilter
        }

        input SearchInput {
          nested: NestedFilter!
          clientMutationId: String
        }

        type Item { id: ID! }

        type Query {
          lookup(
            id: ID!
            cursor: ID
            sport: Sport
            assetId: String
            uuid: UUID
            input: SearchInput
          ): Item
        }

        type Mutation {
          update(viewAsUserId: ID!, input: SearchInput): Item
        }

        type Subscription {
          changed(after: String): Item
        }
        "#,
    )
    .unwrap();
    Envelope::complete(
        StageId::S0,
        "sha256:s2-test".to_string(),
        Producer::current(),
        parsed.warnings,
        parsed.data,
    )
}

#[test]
fn classifies_selector_noise_conflict_and_unknown_evidence() {
    let schema = fixture_schema();
    let policy = read_argument_policy(&policy_path()).unwrap();
    let arguments = classify_arguments(&schema.data, &policy).unwrap();
    let lookup = &arguments.fields["field:Query.lookup"].arguments;

    let id = direct(lookup, "arg:Query.lookup.id");
    assert_eq!(id.classifications, [ArgumentClassification::ObjectSelector]);
    assert_eq!(id.confidence, Confidence::High);

    let cursor = direct(lookup, "arg:Query.lookup.cursor");
    assert_eq!(
        cursor.classifications,
        [
            ArgumentClassification::ObjectSelector,
            ArgumentClassification::Noise
        ]
    );
    assert_eq!(cursor.confidence, Confidence::Low);

    let sport = direct(lookup, "arg:Query.lookup.sport");
    assert_eq!(
        sport.classifications,
        [ArgumentClassification::PossibleSelector]
    );
    assert_eq!(sport.confidence, Confidence::Low);

    let asset_id = direct(lookup, "arg:Query.lookup.assetId");
    assert_eq!(
        asset_id.classifications,
        [ArgumentClassification::ObjectSelector]
    );
    assert_eq!(asset_id.confidence, Confidence::High);

    let uuid = direct(lookup, "arg:Query.lookup.uuid");
    assert_eq!(
        uuid.classifications,
        [ArgumentClassification::ObjectSelector]
    );
    assert_eq!(uuid.confidence, Confidence::High);
    assert!(uuid
        .signals
        .contains(&"type:identity_scalar:uuid".to_string()));
}

#[test]
fn recursively_classifies_input_leaves_and_records_cycles() {
    let schema = fixture_schema();
    let policy = read_argument_policy(&policy_path()).unwrap();
    let arguments = classify_arguments(&schema.data, &policy).unwrap();
    let lookup = &arguments.fields["field:Query.lookup"].arguments;

    let owner = nested(lookup, "arg:Query.lookup.input", &["nested", "ownerId"]);
    assert_eq!(owner.arg_ref, "input_field:NestedFilter.ownerId");
    assert_eq!(owner.arg_path, "Query.lookup.input.nested.ownerId");
    assert_eq!(
        owner.classifications,
        [ArgumentClassification::ObjectSelector]
    );

    let first = nested(lookup, "arg:Query.lookup.input", &["nested", "first"]);
    assert_eq!(first.classifications, [ArgumentClassification::Noise]);

    let client_mutation_id = nested(lookup, "arg:Query.lookup.input", &["clientMutationId"]);
    assert_eq!(
        client_mutation_id.classifications,
        [ArgumentClassification::Noise]
    );

    let cycle = nested(lookup, "arg:Query.lookup.input", &["nested", "next"]);
    assert_eq!(
        cycle.classifications,
        [ArgumentClassification::PossibleSelector]
    );
    assert_eq!(cycle.signals, ["input:cycle_truncated"]);
}

#[test]
fn covers_all_operation_roots_and_is_deterministic() {
    let schema = fixture_schema();
    let policy = read_argument_policy(&policy_path()).unwrap();
    let first = classify_schema_arguments(&schema, &policy).unwrap();
    let second = classify_schema_arguments(&schema, &policy).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.data.classifier_model.as_deref(),
        Some("argument-classifier-v1")
    );
    assert_eq!(
        first.data.policy_fingerprint.as_deref(),
        Some(policy.fingerprint.as_str())
    );
    assert!(first.data.fields.contains_key("field:Query.lookup"));
    assert!(first.data.fields.contains_key("field:Mutation.update"));
    assert!(first.data.fields.contains_key("field:Subscription.changed"));

    let update = &first.data.fields["field:Mutation.update"].arguments;
    let view_as = direct(update, "arg:Mutation.update.viewAsUserId");
    assert_eq!(
        view_as.classifications,
        [
            ArgumentClassification::ObjectSelector,
            ArgumentClassification::AuthzModifier
        ]
    );

    let changed = &first.data.fields["field:Subscription.changed"].arguments;
    assert_eq!(
        direct(changed, "arg:Subscription.changed.after").classifications,
        [ArgumentClassification::Noise]
    );
}

fn direct<'a>(
    arguments: &'a [graphql_static_bac::contracts::ClassifiedArgument],
    arg_ref: &str,
) -> &'a graphql_static_bac::contracts::ClassifiedArgument {
    arguments
        .iter()
        .find(|argument| argument.arg_ref == arg_ref && argument.input_path.is_empty())
        .unwrap()
}

fn nested<'a>(
    arguments: &'a [graphql_static_bac::contracts::ClassifiedArgument],
    root_arg_ref: &str,
    input_path: &[&str],
) -> &'a graphql_static_bac::contracts::ClassifiedArgument {
    arguments
        .iter()
        .find(|argument| {
            argument.root_arg_ref == root_arg_ref
                && argument.input_path
                    == input_path
                        .iter()
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>()
        })
        .unwrap()
}
