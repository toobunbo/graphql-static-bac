# Repository Layout

`graphql-static-bac/` is the implementation root for the new framework. Its
parent repository remains the untouched legacy `graphql-path-enum` tool and a
source of calibration schemas; new framework code must not be added to the
parent `src/` tree.

```text
graphql-static-bac/
├── config/
│   ├── profiles/              # Versioned scoring and analysis profiles
│   └── lexicons/              # Sink, selector, and sanitizer vocabularies
├── docs/
│   ├── spec/                  # Normative contracts
│   ├── design/                # Rationale and calibration
│   ├── implementation/        # Build plans and module boundaries
│   ├── reference/             # Legacy tool behavior
│   └── research/              # Experimental evidence
├── src/
│   ├── artifact/              # Shared artifact reader/writer and run layout
│   ├── cli/
│   │   └── commands/          # Thin command adapters
│   ├── contracts/             # Serde models shared across stages
│   ├── seed/                  # S4 requirements, producer search, DAGs, emitter
│   ├── pipeline/              # S0-S5 orchestration only
│   ├── schema/                # Introspection/SDL parsing and normalization
│   └── stages/
│       ├── s0_ir/
│       ├── s1_sinks/
│       ├── s2_arguments/
│       ├── s3_paths/
│       ├── s4_seed_plans/
│       └── s5_assembly/
├── tests/
│   ├── contracts/             # Serialization and invariant tests
│   ├── fixtures/              # Source inputs and golden artifacts
│   ├── pipeline/              # Cross-stage tests
│   └── stages/                # Stage-level integration tests
└── output/                    # Generated run artifacts; gitignored later
```

All framework commands and relative paths in the implementation documents are
evaluated from `graphql-static-bac/`.

## Boundaries

- `contracts` contains data shapes, enums, stable IDs, and validation helpers.
  It must not depend on parser or stage implementation modules.
- `schema` converts source formats into the canonical S0 model. Downstream
  stages must not read raw introspection or SDL.
- Each `stages/sN_*` module accepts typed artifacts and returns typed artifacts.
  File I/O belongs to the command/runner boundary, not classification or graph
  logic.
- `artifact` owns JSON loading, contract checks, atomic writes, and common
  envelope handling.
- `pipeline` owns dependency order and run status. It must not duplicate stage
  algorithms.
- `cli` converts arguments into runner options and maps errors to exit codes.

## Fixture policy

`tests/fixtures/user_device/` is the first cross-stage contract fixture. It is
a curated schema subset and proves S0-S5 shape consistency; it is not yet a
parser acceptance fixture. S0 receives separate raw introspection and SDL
fixtures as described in `stage-0-ir.md`.
