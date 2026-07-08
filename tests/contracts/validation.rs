use std::path::Path;

use graphql_static_bac::contracts::Producer;
use graphql_static_bac::schema::{validate_schema_ir, SchemaFormat};
use graphql_static_bac::stages::s0_ir::{build_schema_ir, S0Options};

fn schema() -> graphql_static_bac::contracts::SchemaIrData {
    build_schema_ir(&S0Options {
        input: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/s0/sdl/minimal.graphql"),
        format: SchemaFormat::Sdl,
        producer: Producer::current(),
    })
    .unwrap()
    .data
}

#[test]
fn rejects_invalid_stable_id() {
    let mut schema = schema();
    schema.types.get_mut("User").unwrap().type_id = "type:Other".to_string();
    assert!(validate_schema_ir(&schema).is_err());
}

#[test]
fn rejects_non_canonical_type_ref_display() {
    let mut schema = schema();
    schema
        .types
        .get_mut("Query")
        .unwrap()
        .fields
        .get_mut("node")
        .unwrap()
        .return_type
        .display = "Wrong".to_string();
    assert!(validate_schema_ir(&schema).is_err());
}

#[test]
fn rejects_missing_root_type() {
    let mut schema = schema();
    schema.roots.query = Some("MissingQuery".to_string());
    assert!(validate_schema_ir(&schema).is_err());
}
