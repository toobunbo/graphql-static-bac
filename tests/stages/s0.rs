use std::fs;
use std::path::{Path, PathBuf};

use graphql_static_bac::contracts::{ArtifactStatus, TypeKind, TypeWrapper};
use graphql_static_bac::schema::{fingerprint, SchemaFormat};
use graphql_static_bac::stages::s0_ir::{build_schema_ir, write_schema_ir, S0Error, S0Options};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/s0")
        .join(path)
}

fn options(path: &str, format: SchemaFormat) -> S0Options {
    S0Options {
        input: fixture(path),
        format,
        producer: graphql_static_bac::contracts::Producer::current(),
    }
}

#[test]
fn parses_standard_introspection_with_complete_metadata() {
    let artifact =
        build_schema_ir(&options("introspection/minimal.json", SchemaFormat::Auto)).unwrap();
    assert_eq!(artifact.status, ArtifactStatus::Complete);
    assert_eq!(artifact.data.roots.query.as_deref(), Some("Query"));
    assert_eq!(artifact.data.roots.mutation, None);

    let search = &artifact.data.types["Query"].fields["search"];
    assert_eq!(search.return_type.display, "[SearchResult!]!");
    assert_eq!(
        search.return_type.wrappers,
        vec![
            TypeWrapper::NonNull,
            TypeWrapper::List,
            TypeWrapper::NonNull
        ]
    );
    let filter = &artifact.data.types["SearchFilter"];
    assert_eq!(filter.kind, TypeKind::InputObject);
    assert_eq!(filter.input_fields["ownerIds"].type_ref.display, "[ID!]!");
    assert_eq!(
        filter.input_fields["limit"].default_value.as_deref(),
        Some("20")
    );
    assert_eq!(artifact.data.types["Node"].possible_types, ["Team", "User"]);
    assert!(artifact.data.types["Role"].enum_values[1].is_deprecated);
}

#[test]
fn accepts_direct_schema_compatibility_shape() {
    let artifact = build_schema_ir(&options(
        "introspection/direct-schema.json",
        SchemaFormat::Introspection,
    ))
    .unwrap();
    assert_eq!(
        artifact.data.types["Query"].fields["ok"]
            .return_type
            .display,
        "Boolean"
    );
}

#[test]
fn keeps_usable_schema_when_response_contains_errors() {
    let artifact = build_schema_ir(&options(
        "introspection/partial-with-errors.json",
        SchemaFormat::Introspection,
    ))
    .unwrap();
    assert_eq!(artifact.warnings[0].code, "introspection_response_errors");
}

#[test]
fn rejects_truncated_introspection_type_ref() {
    let error = build_schema_ir(&options(
        "introspection/malformed-typeref.json",
        SchemaFormat::Introspection,
    ))
    .unwrap_err();
    assert!(matches!(error, S0Error::Parse(_)));
}

#[test]
fn parses_sdl_extensions_interfaces_unions_and_defaults() {
    let artifact = build_schema_ir(&options("sdl/minimal.graphql", SchemaFormat::Auto)).unwrap();
    assert_eq!(artifact.data.roots.query.as_deref(), Some("Query"));
    assert!(artifact.data.types["User"].fields.contains_key("email"));
    assert_eq!(artifact.data.types["DateTime"].kind, TypeKind::Scalar);
    assert_eq!(
        artifact.data.types["User"].fields["createdAt"]
            .return_type
            .display,
        "DateTime!"
    );
    assert_eq!(artifact.data.types["Node"].possible_types, ["Team", "User"]);
    assert_eq!(
        artifact.data.types["SearchResult"].possible_types,
        ["Team", "User"]
    );
    assert_eq!(
        artifact.data.types["SearchFilter"].input_fields["role"]
            .default_value
            .as_deref(),
        Some("USER")
    );
    let deprecated = artifact.data.types["Role"]
        .enum_values
        .iter()
        .find(|value| value.name == "OLD")
        .unwrap();
    assert!(deprecated.is_deprecated);
}

#[test]
fn respects_explicit_root_names() {
    let artifact =
        build_schema_ir(&options("sdl/explicit-roots.graphql", SchemaFormat::Sdl)).unwrap();
    assert_eq!(artifact.data.roots.query.as_deref(), Some("RootQuery"));
    assert_eq!(
        artifact.data.roots.mutation.as_deref(),
        Some("RootMutation")
    );
    assert_eq!(artifact.data.roots.subscription, None);
}

#[test]
fn warns_when_applied_directives_are_omitted() {
    let artifact = build_schema_ir(&options("sdl/directives.graphql", SchemaFormat::Sdl)).unwrap();
    assert_eq!(artifact.warnings[0].code, "applied_directives_omitted");
}

#[test]
fn parses_directive_only_schema_extension() {
    let artifact = build_schema_ir(&options("sdl/federation.graphql", SchemaFormat::Sdl)).unwrap();
    assert_eq!(artifact.data.roots.query.as_deref(), Some("Query"));
    assert_eq!(artifact.warnings[0].code, "applied_directives_omitted");
}

#[test]
fn invalid_sdl_does_not_write_an_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("schema_ir.json");
    let result = build_schema_ir(&options("sdl/invalid.graphql", SchemaFormat::Sdl));
    assert!(matches!(result, Err(S0Error::Parse(_))));
    assert!(!output.exists());
}

#[test]
fn fingerprint_uses_exact_input_bytes() {
    assert_ne!(
        fingerprint(b"type Query { ok: Boolean }"),
        fingerprint(b"type Query { ok: Boolean }\n")
    );
}

#[test]
fn identical_input_produces_identical_output_bytes() {
    let artifact = build_schema_ir(&options("sdl/minimal.graphql", SchemaFormat::Sdl)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.json");
    let second = directory.path().join("second.json");
    write_schema_ir(&artifact, &first).unwrap();
    write_schema_ir(&artifact, &second).unwrap();
    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}

#[test]
fn artifact_fingerprint_matches_source_bytes() {
    let source = fixture("introspection/minimal.json");
    let artifact =
        build_schema_ir(&options("introspection/minimal.json", SchemaFormat::Auto)).unwrap();
    assert_eq!(
        artifact.schema_fingerprint,
        fingerprint(&fs::read(source).unwrap())
    );
}

#[test]
fn introspection_output_matches_golden_artifact() {
    assert_matches_golden(
        "introspection/minimal.json",
        SchemaFormat::Introspection,
        "expected/minimal-introspection.schema_ir.json",
    );
}

#[test]
fn sdl_output_matches_golden_artifact() {
    assert_matches_golden(
        "sdl/minimal.graphql",
        SchemaFormat::Sdl,
        "expected/minimal-sdl.schema_ir.json",
    );
}

#[test]
fn missing_schema_is_a_parse_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("missing-schema.json");
    fs::write(&input, br#"{"data":{}}"#).unwrap();
    let error = build_schema_ir(&S0Options {
        input,
        format: SchemaFormat::Introspection,
        producer: graphql_static_bac::contracts::Producer::current(),
    })
    .unwrap_err();
    assert!(matches!(error, S0Error::Parse(_)));
}

#[test]
fn unknown_introspection_kind_is_a_parse_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("unknown-kind.json");
    fs::write(
        &input,
        br#"{"__schema":{"queryType":null,"mutationType":null,"subscriptionType":null,"types":[{"kind":"ALIEN","name":"Unknown","description":null}]}}"#,
    )
    .unwrap();
    let error = build_schema_ir(&S0Options {
        input,
        format: SchemaFormat::Introspection,
        producer: graphql_static_bac::contracts::Producer::current(),
    })
    .unwrap_err();
    assert!(matches!(error, S0Error::Parse(_)));
}

#[test]
fn missing_input_is_a_source_error() {
    let error = build_schema_ir(&S0Options {
        input: fixture("does-not-exist.graphql"),
        format: SchemaFormat::Sdl,
        producer: graphql_static_bac::contracts::Producer::current(),
    })
    .unwrap_err();
    assert!(matches!(error, S0Error::Source(_)));
}

#[test]
fn writer_failure_does_not_replace_existing_path() {
    let artifact = build_schema_ir(&options("sdl/minimal.graphql", SchemaFormat::Sdl)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let blocker = directory.path().join("not-a-directory");
    fs::write(&blocker, b"keep-me").unwrap();
    let output = blocker.join("schema_ir.json");
    let error = write_schema_ir(&artifact, &output).unwrap_err();
    assert!(matches!(error, S0Error::Write(_)));
    assert_eq!(fs::read(blocker).unwrap(), b"keep-me");
    assert!(!output.exists());
}

#[test]
fn equivalent_sources_keep_source_specific_fingerprints() {
    let introspection = build_schema_ir(&options(
        "introspection/minimal.json",
        SchemaFormat::Introspection,
    ))
    .unwrap();
    let sdl = build_schema_ir(&options("sdl/minimal.graphql", SchemaFormat::Sdl)).unwrap();
    assert_ne!(introspection.schema_fingerprint, sdl.schema_fingerprint);
}

#[test]
fn introspection_list_order_does_not_change_normalized_data() {
    let original = build_schema_ir(&options(
        "introspection/minimal.json",
        SchemaFormat::Introspection,
    ))
    .unwrap();
    let mut source: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture("introspection/minimal.json")).unwrap()).unwrap();
    let types = source["data"]["__schema"]["types"].as_array_mut().unwrap();
    types.reverse();
    for definition in types {
        for key in [
            "fields",
            "inputFields",
            "enumValues",
            "interfaces",
            "possibleTypes",
        ] {
            if let Some(values) = definition[key].as_array_mut() {
                values.reverse();
            }
        }
        if let Some(fields) = definition["fields"].as_array_mut() {
            for field in fields {
                if let Some(arguments) = field["args"].as_array_mut() {
                    arguments.reverse();
                }
            }
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("reordered.json");
    fs::write(&input, serde_json::to_vec(&source).unwrap()).unwrap();
    let reordered = build_schema_ir(&S0Options {
        input,
        format: SchemaFormat::Introspection,
        producer: graphql_static_bac::contracts::Producer::current(),
    })
    .unwrap();
    assert_eq!(original.data, reordered.data);
    assert_ne!(original.schema_fingerprint, reordered.schema_fingerprint);
}

#[test]
fn duplicate_sdl_field_extension_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("duplicate.graphql");
    fs::write(
        &input,
        "type Query { value: String }\nextend type Query { value: Int }\n",
    )
    .unwrap();
    let error = build_schema_ir(&S0Options {
        input,
        format: SchemaFormat::Sdl,
        producer: graphql_static_bac::contracts::Producer::current(),
    })
    .unwrap_err();
    assert!(matches!(error, S0Error::Parse(_)));
}

fn assert_matches_golden(input: &str, format: SchemaFormat, expected: &str) {
    let artifact = build_schema_ir(&options(input, format)).unwrap();
    let actual = serde_json::to_value(artifact).unwrap();
    let expected: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture(expected)).unwrap()).unwrap();
    assert_eq!(actual, expected);
}
