# Stage 0 Implementation Plan: Schema IR Converter

**Status:** Implemented on June 8, 2026.

## 1. Objective

Stage 0 reads a GraphQL schema source and emits the canonical `schema_ir.json`
artifact defined by [`../spec/architecture.md`](../spec/architecture.md).

Supported inputs for the first milestone:

- Standard GraphQL introspection response JSON.
- GraphQL SDL.

S0 is syntax and structure normalization only. It must not classify sinks,
arguments, selectors, sanitizers, reachability, or risk.

## 2. Non-goals

- No Query path enumeration.
- No Connection/Edge collapsing.
- No sink or argument lexicon.
- No scoring.
- No schema federation composition.
- No remote endpoint fetching.
- No semantic-equivalence fingerprint across SDL and introspection.

## 3. Required pipeline

```text
input path
   |
   v
read exact bytes ---------------------------> SHA-256 fingerprint
   |
   v
detect/validate source format
   |
   +--> introspection parser --+
   |                            |
   +--> SDL parser -------------+--> normalized schema builder
                                      |
                                      v
                               structural validation
                                      |
                                      v
                              Envelope<SchemaIrData>
                                      |
                                      v
                                atomic JSON write
```

The fingerprint must be computed before UTF-8 decoding or parsing. A trailing
newline therefore changes the fingerprint by design.

## 4. Target modules

```text
src/
├── contracts/
│   ├── mod.rs
│   ├── envelope.rs       # Envelope<T>, status, producer, warning
│   ├── ids.rs            # type/field/arg/input-field ID constructors
│   ├── type_ref.rs       # canonical wrappers and display rendering
│   └── s0.rs             # SchemaIrData and all S0 definitions
├── schema/
│   ├── mod.rs
│   ├── source.rs         # input format and source byte handling
│   ├── introspection.rs  # JSON-specific parser DTOs
│   ├── sdl.rs            # SDL-specific parser adapter
│   ├── normalize.rs      # both formats converge here
│   └── validate.rs       # structural S0 invariants
├── stages/s0_ir/
│   ├── mod.rs
│   └── runner.rs         # library entry point for one conversion
└── artifact/
    ├── mod.rs
    └── writer.rs         # pretty JSON and atomic replace
```

The parser DTOs are private to `schema`. Public consumers receive only the
types in `contracts::s0`.

## 5. Public library API

The first usable API should be equivalent to:

```rust
pub struct S0Options {
    pub input: PathBuf,
    pub format: SchemaFormat, // Auto | Introspection | Sdl
    pub scope: Vec<AnalysisScope>,
    pub producer: Producer,
}

pub fn build_schema_ir(options: &S0Options) -> Result<Envelope<SchemaIrData>, S0Error>;

pub fn write_schema_ir(
    artifact: &Envelope<SchemaIrData>,
    output: &Path,
) -> Result<(), ArtifactWriteError>;
```

Parsing and serialization remain callable independently so tests do not need
to invoke the CLI.

## 6. Canonical data model

Use typed Rust structures and enums with `serde`; do not build the artifact
through `serde_json::Value` maps.

Required top-level structures:

```text
Envelope<SchemaIrData>
SchemaIrData { roots, types }
SchemaRoots { query, mutation, subscription }
TypeDefinition
FieldDefinition
ArgumentDefinition
InputFieldDefinition
EnumValueDefinition
TypeRef
```

Implementation rules:

- Use `BTreeMap` for `types`, `fields`, and `input_fields` so JSON key order is
  deterministic.
- Stable IDs are derived only from canonical GraphQL names:
  `type:User`, `field:Query.user`, `arg:Query.user.id`, and
  `input_field:SearchFilter.ownerId`.
- `TypeKind` is a closed enum covering `OBJECT`, `INTERFACE`, `UNION`,
  `INPUT_OBJECT`, `ENUM`, and `SCALAR`.
- Root names are `Option<String>` and serialize as `null` when absent.
- Preserve type and field descriptions. The current normative examples must be
  amended before implementation if type-level description or applied
  directive fields are added to contract version `1.0`.
- Preserve argument/input default values as canonical GraphQL literal strings,
  or `null` when no default exists.
- Preserve enum values as structured entries, not plain concatenated text.

## 7. Canonical TypeRef algorithm

Walk wrappers from the outside inward until the named type is reached.

```text
NON_NULL(LIST(NON_NULL(User)))
    -> named_type: User
    -> wrappers: [NON_NULL, LIST, NON_NULL]
    -> display: [User!]!
```

The renderer must reconstruct `display` only from `named_type + wrappers`.
Parser-provided display text is not trusted as canonical output.

Invalid wrapper sequences such as `NON_NULL(NON_NULL(User))` fail validation.

## 8. Introspection parsing

Accepted JSON shapes:

```text
{"data":{"__schema":{...}}}   # standard response
{"__schema":{...}}             # extracted compatibility form
```

Rules:

- If neither location contains `__schema`, return `MissingSchema`.
- If GraphQL `errors` exist but usable `data.__schema` also exists, parse the
  schema and add a structured warning.
- Unknown JSON members are ignored.
- Unknown GraphQL kind values fail explicitly; they must not be coerced.
- A type reference chain that ends without a named type fails explicitly.
- Introspection order is not used as identity.

Metadata to retain:

- root operation type names;
- all named types and kinds;
- fields and their arguments;
- complete `NON_NULL`/`LIST` wrapper chains;
- input fields and defaults;
- enum values;
- implemented interfaces;
- interface/union possible types;
- descriptions.

Standard introspection exposes directive definitions but not directives applied
to schema elements. Initial S0 must not invent applied directives.

## 9. SDL parsing

SDL parsing must feed the same normalized builder used by introspection.

Rules:

- Respect an explicit `schema { query: ... }` definition.
- Without an explicit schema definition, use `Query`, `Mutation`, and
  `Subscription` only when matching object types exist.
- Merge valid type/schema extensions before normalization.
- Preserve interfaces, unions, input objects, enum values, defaults, and
  descriptions.
- Reject duplicate incompatible definitions.
- Return parser diagnostics with source line and column.
- Applied directives may only be serialized after their exact S0 JSON contract
  is added to the normative specification. Until then, emit a warning when SDL
  contains applied directives that cannot be represented.

The implementation should use a standards-aware SDL parser. No SDL parsing by
regular expression or line splitting is acceptable.

## 10. Structural validation

Validation runs after normalization and before a `complete` artifact exists.

Required checks:

1. Every map key matches the contained GraphQL name.
2. Every stable ID matches its owner/name tuple.
3. Every `TypeRef.named_type` exists in `types` or is a declared built-in
   scalar.
4. `named_kind` matches the referenced named type kind.
5. Field owner types are `OBJECT` or `INTERFACE`.
6. `input_fields` only belong to `INPUT_OBJECT` types.
7. `enum_values` only belong to `ENUM` types.
8. Interface and possible-type references resolve.
9. Root operation references resolve to `OBJECT` types.
10. `display` exactly matches the TypeRef renderer.

Fatal validation errors return `Err` and must not write a misleading complete
artifact. Failed-run packaging belongs to the later pipeline manifest.

## 11. Serialization and output

- Output is UTF-8 pretty JSON with a final newline.
- Write to a temporary file in the destination directory, flush, then rename.
- Do not leave a partial destination file after parser or validation failure.
- Re-running on identical bytes and producer version must produce byte-identical
  JSON.
- `schema_fingerprint` format is lowercase `sha256:<64 hex chars>`.
- S0 emits `stage: "S0"`, `contract_version: "1.0"`, and
  `status: "complete"` only after validation succeeds.

## 12. CLI milestone

The thin command planned for S0 is:

```bash
graphql-static-bac stage s0 \
  --input tests/fixtures/s0/introspection/minimal.json \
  --format auto \
  --output /tmp/schema_ir.json
```

Exit codes:

```text
0  conversion completed and artifact written
2  invalid CLI usage
3  input read/format detection failure
4  parse failure
5  normalized IR validation failure
6  output write failure
```

The CLI must not contain parser or normalization logic.

## 13. Test layout

```text
tests/
├── contracts/
│   ├── envelope.rs
│   ├── stable_ids.rs
│   └── type_ref.rs
├── fixtures/
│   ├── s0/
│   │   ├── introspection/
│   │   │   ├── minimal.json
│   │   │   ├── complete-types.json
│   │   │   ├── partial-with-errors.json
│   │   │   └── malformed-typeref.json
│   │   ├── sdl/
│   │   │   ├── minimal.graphql
│   │   │   ├── explicit-roots.graphql
│   │   │   ├── extensions.graphql
│   │   │   └── invalid.graphql
│   │   └── expected/
│   │       ├── minimal-introspection.schema_ir.json
│   │       └── minimal-sdl.schema_ir.json
│   └── user_device/           # existing cross-stage subset fixture
└── stages/
    ├── s0_introspection.rs
    ├── s0_sdl.rs
    ├── s0_validation.rs
    └── s0_determinism.rs
```

## 14. Concrete test cases

### Contract unit tests

| ID | Test | Expected result |
| --- | --- | --- |
| C01 | Render `User` | `wrappers=[]`, display `User` |
| C02 | Render `User!` | `[NON_NULL]`, display `User!` |
| C03 | Render `[User!]!` | `[NON_NULL,LIST,NON_NULL]`, exact display |
| C04 | Reject double non-null wrapper | validation error |
| C05 | Build IDs for type/field/arg/input field | exact canonical strings |
| C06 | Serialize missing mutation/subscription roots | explicit JSON `null` |
| C07 | Serialize maps inserted in different orders | byte-identical JSON |

### Introspection tests

| ID | Test | Expected result |
| --- | --- | --- |
| I01 | Parse standard `data.__schema` response | complete S0 artifact |
| I02 | Parse direct `__schema` compatibility object | same normalized data shape |
| I03 | Preserve nested argument wrapper `[ID!]!` | exact canonical TypeRef |
| I04 | Preserve input object fields and defaults | exact IDs, defaults, kinds |
| I05 | Preserve interface implementations and possible types | resolved names in IR |
| I06 | Preserve union possible types | resolved names in IR |
| I07 | Preserve enum values and descriptions | structured enum entries |
| I08 | Response has errors plus usable schema | complete artifact plus warning |
| I09 | Missing `__schema` | `MissingSchema`, no output file |
| I10 | Truncated TypeRef chain | parse/validation error, no output |
| I11 | Unknown kind | explicit unsupported-kind error |

### SDL tests

| ID | Test | Expected result |
| --- | --- | --- |
| D01 | Conventional `Query` without schema definition | Query root inferred |
| D02 | Explicit custom root `RootQuery` | exact explicit root retained |
| D03 | Missing Mutation/Subscription types | both roots serialize as `null` |
| D04 | Type and schema extensions | merged once without duplicate fields |
| D05 | Interface and union declarations | complete relationship metadata |
| D06 | Input defaults and enum values | canonical literals retained |
| D07 | Applied SDL directive before contract support | structured omission warning |
| D08 | Duplicate incompatible field definition | validation failure |
| D09 | Invalid SDL syntax | line/column diagnostic, no output |

### Fingerprint and determinism tests

| ID | Test | Expected result |
| --- | --- | --- |
| F01 | Convert same bytes twice | identical fingerprint and output bytes |
| F02 | Add one trailing newline | different fingerprint |
| F03 | Semantically equal SDL and introspection | fingerprints intentionally differ |
| F04 | Change input map/list order where semantics are set-like | normalized ordering stays deterministic |
| F05 | Writer failure | no partial destination replacement |

### Golden acceptance tests

1. Convert `minimal.json` and compare the complete artifact to
   `minimal-introspection.schema_ir.json`.
2. Convert `minimal.graphql` and compare to `minimal-sdl.schema_ir.json`, with
   the expected source-specific fingerprint.
3. Run the existing cross-stage validator:

```bash
python3 tests/fixtures/user_device/validate.py
```

The existing `UserDevice` S0 file contains fixture-only subset metadata and is
not a parser golden. It remains the S0-S5 contract fixture until a raw source
schema is added for it.

## 15. Implementation sequence

1. Add contract structs and serialization tests.
2. Implement stable IDs and TypeRef rendering/validation.
3. Implement exact-byte source loading and fingerprinting.
4. Implement introspection DTOs and normalization.
5. Add introspection fixtures and golden test.
6. Implement SDL adapter into the same normalized builder.
7. Add SDL fixtures and golden test.
8. Implement structural validation.
9. Implement atomic artifact writer.
10. Add the thin S0 CLI command and exit-code tests.
11. Run formatting, linting, unit tests, golden tests, and the existing
    `UserDevice` validator.

## 16. Definition of done

S0 is complete when:

- both supported source formats produce contract-valid typed artifacts;
- all metadata required by S1-S3 is retained;
- all TypeRef and stable-ID invariants are enforced;
- identical input bytes produce byte-identical output;
- invalid input cannot leave a complete or partial artifact;
- the S0 test matrix passes;
- `python3 tests/fixtures/user_device/validate.py` still passes;
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test` pass.

## 17. Implementation result

Implemented components:

- typed envelope, stable IDs, canonical TypeRef, and S0 contracts;
- standard introspection response and direct `__schema` compatibility parsing;
- SDL definitions, type extensions, schema extensions, and directive-only
  federation-style schema extensions;
- root inference, interface/union relationships, descriptions, defaults,
  deprecation metadata, and applied-directive omission warnings;
- structural validation, exact-byte SHA-256 fingerprinting, deterministic JSON,
  and atomic output replacement;
- CLI command `graphql-static-bac stage s0` with documented exit codes;
- contract, parser, validation, determinism, golden, and CLI tests.

Calibration against the repository schema succeeded for both source formats:

```text
introspection.json: 1665 types, complete, 0 warnings
schema.graphql:      1652 types, complete, 1 applied-directive warning
```

The 13-type difference is expected: introspection contains the five built-in
scalars plus eight `__*` introspection-system types that are not declared in the
SDL. The two outputs otherwise contain the same application type names and the
same Query/Mutation/Subscription roots.
