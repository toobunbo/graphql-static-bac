# Stage 3 Implementation Plan: Query Path Enumerator

**Status:** Implemented on June 9, 2026.

Implementation lives under `src/graph/`, `src/stages/s3_paths/`,
`src/contracts/s3.rs`, and the S3 CLI commands. The Watchlist calibration
fixture is under `tests/fixtures/s3/watchlist/`.

**Migration note:** this edge-simple enumerator is retained as a diagnostic and
calibration oracle. The production replacement is implemented in
`stage-3-route-analysis.md`; it compacts prefix variants with a finite
product-graph worklist before output.

## 1. Objective

Stage 3 answers one structural question:

> Given one target object type, which finite structural paths can a GraphQL
> client follow from the Query root to reach that type?

The core API operates on one target type, like the legacy
`graphql-path-enum` tool. The pipeline adapter runs that core for every object
type selected by S1 and packages the results as `paths.json`.

S3 is structural only. It does not decide whether a target is sensitive, whether
an argument is attacker-controlled, whether a path is sanitized, or whether a
path is exploitable.

## 2. Exact meaning of "all paths"

S3 enumerates every finite path from Query to the target under this cycle rule:

```text
the same schema edge_id may appear at most once in one path
```

This is an **edge-simple path** model:

- a type may appear multiple times;
- distinct fields between the same types remain distinct edges;
- the same field or type-condition edge cannot repeat indefinitely;
- recursive families that would require repeating an edge are represented by
  `cycle_templates` where the normative contract permits them.

Therefore "all paths" does not mean infinitely many walks such as:

```text
Query.user -> User.manager -> User.manager -> User.manager -> ...
```

There is no logical `max_depth` or `max_paths` cutoff.

## 3. Non-goals

- No sink classification; that belongs to S1.
- No argument/selector classification; that belongs to S2.
- No sanitizer, flow, ownership, confidence, or score fields; those belong to S4.
- No query document generation.
- No Connection/Edge collapsing in canonical paths.
- No Mutation or Subscription enumeration.
- No runtime requests.
- No parsing of legacy text output in production code.

## 4. Two entry points

### 4.1 Core single-target API

The core must be independently callable without S1:

```rust
pub fn enumerate_target(
    graph: &TypeGraph,
    target_type_id: &str,
) -> Result<TargetEnumeration, EnumerationError>;
```

This is the reusable equivalent of:

```text
graphql-path-enum --type Watchlist
```

but it operates on canonical S0 IR and returns typed CAPs rather than text.

### 4.2 Official S3 stage adapter

The pipeline adapter consumes S0 and S1 artifacts:

```rust
pub fn enumerate_selected_targets(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
) -> Result<Envelope<PathsData>, S3Error>;
```

It builds the graph once, invokes `enumerate_target` for each selected type,
copies that type's sorted `sink_ref_ids`, and emits official `paths.json`.

S1 classification logic does not need to exist before S3. S3 may introduce the
typed read-only S1 contract required to load the existing fixtures, but it must
not implement or duplicate the S1 classifier.

## 5. Prerequisite contract refactor

The current `Envelope::complete` helper hardcodes `stage: "S0"`. Before S3, the
common envelope API must accept a typed stage identifier:

```rust
pub enum StageId { S0, S1, S2, S3, S4 }

Envelope::complete(
    stage: StageId,
    schema_fingerprint: String,
    producer: Producer,
    warnings: Vec<Warning>,
    data: T,
)
```

S0 output must remain byte-compatible apart from tests intentionally updated
for the generalized constructor.

An artifact reader is also required. It must validate:

- JSON shape and typed contract;
- `contract_version`;
- expected stage;
- `scope == ["query"]`;
- input status is `complete`;
- S0 and S1 fingerprints match.

## 6. Target modules

```text
src/
├── artifact/
│   └── reader.rs                 # Typed artifact loading and envelope checks
├── contracts/
│   ├── s1.rs                     # Read-only S1 contract needed by S3
│   └── s3.rs                     # Path/CAP contracts
├── graph/
│   ├── mod.rs
│   ├── builder.rs                # Schema IR -> canonical TypeGraph
│   ├── model.rs                  # Graph nodes and edges
│   └── reachability.rs           # Lossless reverse-reachability index
├── stages/s3_paths/
│   ├── mod.rs
│   ├── enumerator.rs             # Iterative edge-simple DFS
│   ├── global_id.rs              # node/nodes structural seed detection
│   ├── formatter.rs              # display_projection only
│   ├── runner.rs                 # Official S0+S1 stage adapter
│   └── manual.rs                 # Single-target diagnostic report
└── cli/commands/
    ├── enumerate.rs              # Manual target command
    └── s3.rs                     # Official stage command
```

`graph` is shared infrastructure for later stages. It must not depend on S1,
S2, S3 output contracts, CLI code, or filesystem I/O.

## 7. S3 contract types

```text
PathsData
  targets: BTreeMap<type_id, TargetPaths>

TargetPaths
  target_type_id
  sink_ref_ids
  enumeration_status
  caps

CandidateAccessPath
  cap_id
  origin
  entry_field_id
  target_type_id
  edges
  cycle_templates
  display_projection

PathEdge
  edge_id
  kind
  source_type_id
  field_id
  target_type_id

CycleTemplate
  repeated_edge_id
  cycle_start_index
  repeatable_edge_ids
```

Closed enums:

```text
CapOrigin        = traversal | global_id
PathEdgeKind     = FIELD | TYPE_CONDITION
EnumerationStatus = complete | incomplete | failed
```

The serialized CAP must not contain any of these S4 fields:

```text
selectors, authz_modifiers, sanitizer_boundaries, flow,
ownership_continuity, ranking_bucket, score, score_breakdown, confidence
```

## 8. Canonical graph construction

Graph nodes represent named GraphQL output-composite types:

```text
OBJECT, INTERFACE, UNION
```

Scalars, enums, and input objects do not become traversal nodes.

### 8.1 FIELD edges

For every field owned by an OBJECT or INTERFACE:

- unwrap its return TypeRef to `named_type`;
- if the named kind is OBJECT, INTERFACE, or UNION, create one FIELD edge;
- ignore LIST/NON_NULL for graph connectivity, but never mutate S0 metadata;
- use the S0 `field_id` as both `edge_id` and `field_id`.

Example:

```text
field:Query.currentUser
  Query -> CurrentUser
```

### 8.2 TYPE_CONDITION edges

For every INTERFACE or UNION possible type, create:

```text
type_condition:<Abstract>-><Concrete>
```

with:

```text
kind            = TYPE_CONDITION
field_id        = null
source_type_id  = type:<Abstract>
target_type_id  = type:<Concrete>
```

No reverse edge is created from an OBJECT to its interface. Such reverse edges
would generate redundant paths that do not correspond to query syntax.

### 8.3 Connection policy

The canonical graph retains all Connection and Edge nodes:

```text
Query.users -> UserConnection -> UserEdge -> User
```

Any collapsed view exists only in the Watchlist legacy-comparison fixture. It
must never alter CAP identity or production graph construction.

### 8.4 Determinism

- node lookup uses stable type IDs;
- outgoing edges are sorted by `edge_id`;
- duplicate edge IDs are graph-construction errors;
- graph construction from semantically identical reordered S0 maps produces the
  same graph.

## 9. Lossless target reachability index

Before enumerating one target, compute the set of nodes that can reach the
target by walking the reverse graph.

During DFS, skip an outgoing edge only when its destination is not in that set.
This is a lossless optimization: such an edge cannot participate in any path to
the target.

The reachability index is not a depth limit, path limit, semantic filter, or
score. Unit tests must compare enumeration with and without this optimization
on synthetic graphs and obtain identical CAP edge sequences.

## 10. Traversal algorithm

Use iterative DFS rather than recursive Rust calls to avoid process stack
overflow on deep schemas.

Per target, maintain:

```text
frame stack
ordered path edge indices
path-local used-edge bitset
CAP map keyed by ordered edge_id sequence
```

Algorithm:

1. Resolve the Query root from S0.
2. Compute reverse reachability to the target.
3. Start from every sorted outgoing Query FIELD edge that can reach the target.
4. On entering a node:
   - if it is the target and the path is non-empty, emit the current CAP;
   - continue traversing after emission so paths that later return to the target
     are not lost.
5. For each sorted outgoing edge:
   - if unused in the current path, mark it, descend, then unmark on backtrack;
   - if already used, stop that branch and apply the cycle rule below.
6. Deduplicate only exact ordered edge sequences.

This corrects the legacy tool's global-visited behavior. Two paths that converge
on the same suffix must both survive because each DFS branch owns its used-edge
state.

## 11. Cycle templates

When an outgoing edge is already present in the current path, the branch stops.
If the current node is the target and a CAP for the current path has already
been emitted, attach:

```json
{
  "repeated_edge_id": "field:User.manager",
  "cycle_start_index": 1,
  "repeatable_edge_ids": ["field:User.manager"]
}
```

Rules:

- `cycle_start_index` is the first index of the repeated edge in CAP edges;
- `repeatable_edge_ids` is the existing path segment from that index onward;
- templates are deduplicated by the full object;
- templates are sorted by start index, repeated edge ID, then repeatable IDs;
- a CAP without a target-ending recursive family has `cycle_templates: []`.

This follows the current normative contract. Broader templates for cycles that
occur before a different terminal target remain outside the first S3 milestone.

## 12. Independent global-ID track

Global-ID detection is structural and exact enough to avoid accidental seeds.

### 12.1 `node`

Recognize `Query.node` when:

- field name is `node`;
- it contains an argument named `id` whose named type is `ID` and has no LIST
  wrapper;
- return named type is `Node` with no LIST wrapper;
- `Node` is an abstract type with the target in `possible_types`.

Nullability does not affect recognition. Additional arguments are retained in
S0 and do not suppress this structural signal.

### 12.2 `nodes`

Recognize `Query.nodes` when:

- field name is `nodes`;
- it contains an argument named `ids` whose named type is `ID` and has exactly
  one LIST wrapper;
- return named type is `Node` with exactly one LIST wrapper;
- the target is in `Node.possible_types`.

### 12.3 Emission and deduplication

Each recognized seed emits an independent direct CAP:

```text
Query.node  -> Node -> TYPE_CONDITION(Target)
Query.nodes -> Node -> TYPE_CONDITION(Target)
```

If both fields exist, the target receives two global-ID CAPs.

Ordinary traversal is still allowed to start from `Query.node` and
`Query.nodes`. This is necessary to retain longer paths such as:

```text
Query.node -> ... on User -> User.publicWatchlists -> Watchlist
```

When traversal discovers the exact same two-edge sequence as an independently
seeded direct global-ID CAP, exact-path dedup keeps one CAP with
`origin: global_id`. Longer paths remain `origin: traversal` and are not
dropped.

## 13. CAP identity and ordering

`cap_id` follows the existing normative rule:

```text
SHA-256(UTF-8 compact JSON [target_type_id, ordered_edge_id_array])
```

The result is:

```text
cap:sha256:<lowercase hex>
```

`entry_field_id` is the first FIELD edge and must belong to Query.

CAP output ordering is deterministic but not a risk ranking:

1. `global_id` before `traversal`;
2. ordered edge-ID sequence lexicographically;
3. `cap_id` as the final tie-breaker.

Targets and sink refs are sorted lexicographically by stable ID.

## 14. Display projection

`display_projection` is generated from canonical edges and is never parsed back
into logic.

Examples:

```text
Query.currentUser -> CurrentUser.devices -> UserDevice
Query.node -> ... on UserDevice
```

Formatting rule:

- FIELD edge: `SourceType.fieldName`;
- TYPE_CONDITION: `... on ConcreteType`;
- if the final edge is FIELD, append the terminal type name;
- if the final edge is TYPE_CONDITION, that segment already names the target.

## 15. Official S3 runner invariants

The stage runner must enforce:

- S0 and S1 artifacts are complete and Query-scoped;
- schema fingerprints match;
- every selected S1 target exists in S0 and is OBJECT;
- every copied sink ref exists in S1 and belongs to that target;
- selected target IDs are unique;
- a target selected as Query-reachable by S1 produces at least one CAP.

The last condition is the cross-stage invariant:

```text
S1 Query-reachable target <=> S3 emits at least one CAP
```

A violation is a contract mismatch, not a valid complete empty target.

## 16. Status and resource behavior

S3 has no default logical path/depth quota. To make failure controlled:

- enumeration is iterative, not recursive;
- large vectors/maps use checked reservation where practical;
- graph or contract errors fail the run;
- controlled allocation/resource/cancellation failure marks the current target
  and stage `incomplete`;
- partial diagnostics may be written, but never with `status: complete`;
- S4 must reject non-complete S3 input.

The initial implementation runs targets serially. Parallel target enumeration
is deferred until memory behavior is measured on the full schema.

## 17. CLI shape

### Manual single-target command

```bash
cargo run -- enumerate \
  --schema-ir output/schema_ir.introspection.json \
  --target Watchlist \
  --output output/paths.watchlist.json
```

This writes a diagnostic single-target report from the core API. It is not an
official S3 pipeline artifact because it has no authoritative S1 sink refs.

### Official stage command

```bash
cargo run -- stage s3 \
  --schema-ir output/schema_ir.introspection.json \
  --sinks output/sinks.json \
  --output output/paths.json
```

Exit-code classes should remain consistent with S0:

```text
0  complete output written
2  invalid CLI usage
3  input read failure
4  artifact JSON/contract failure
5  graph or cross-stage invariant failure
6  output write failure
7  enumeration incomplete/resource failure
```

## 18. Test layout

```text
tests/
├── contracts/
│   └── s3.rs
├── fixtures/s3/
│   ├── graph_cases/
│   │   ├── shared_suffix.schema_ir.json
│   │   ├── abstract_types.schema_ir.json
│   │   ├── cycles.schema_ir.json
│   │   └── connections.schema_ir.json
│   └── watchlist/
│       ├── schema_ir.json
│       ├── sinks.json
│       ├── legacy_paths.txt
│       ├── legacy_paths.canonical.json
│       └── README.md
├── graph.rs
├── stages/
│   └── s3.rs
└── cli_s3.rs
```

The existing `tests/fixtures/user_device/` remains the first exact S0-S5
contract fixture.

## 19. Concrete test cases

### Graph construction

| ID | Test | Expected result |
| --- | --- | --- |
| G01 | Field returns OBJECT | one FIELD edge |
| G02 | Field returns INTERFACE | FIELD to interface plus sorted TYPE_CONDITION edges |
| G03 | Field returns UNION | FIELD to union plus sorted TYPE_CONDITION edges |
| G04 | Field returns scalar/enum | no graph edge |
| G05 | Connection and Edge types | all wrapper nodes retained |
| G06 | Duplicate edge ID | graph-construction error |
| G07 | Reordered S0 maps | byte-equivalent graph serialization/debug snapshot |

### Core enumeration

| ID | Test | Expected result |
| --- | --- | --- |
| E01 | Direct Query field to target | one one-edge traversal CAP |
| E02 | Two branches share a suffix | both paths retained |
| E03 | Type repeats through distinct fields | valid paths retained |
| E04 | Same edge would repeat | branch stops |
| E05 | Target is reached, left, then reached again | both target-ending paths emitted |
| E06 | Exact duplicate edge sequence | one CAP only |
| E07 | Missing target | explicit target error |
| E08 | Non-OBJECT official target | stage validation error |
| E09 | Reverse-reachability optimization toggled in test | identical CAP edge sets |

### Cycle behavior

| ID | Test | Expected result |
| --- | --- | --- |
| C01 | `Query.user -> User` | direct User CAP |
| C02 | `User.manager -> User` | second CAP with manager edge |
| C03 | manager edge repeats at target | one canonical cycle template |
| C04 | multiple repeatable segments | deterministic template ordering |

### Global-ID track

| ID | Test | Expected result |
| --- | --- | --- |
| N01 | valid `node(id: ID): Node` | one direct global-ID CAP |
| N02 | valid `nodes(ids: [ID]): [Node]` | second direct global-ID CAP |
| N03 | nullability variants | still recognized |
| N04 | `node(id: String)` | not recognized |
| N05 | `nodes(ids: ID)` without LIST | not recognized |
| N06 | target absent from `Node.possible_types` | no global-ID CAP |
| N07 | DFS finds the same direct sequence | dedup keeps `origin: global_id` |
| N08 | longer path begins at `node` | retained as traversal CAP |

### Contract and determinism

| ID | Test | Expected result |
| --- | --- | --- |
| D01 | Known target and edge tuple | exact hard-coded `cap_id` |
| D02 | Run same target twice | byte-identical report |
| D03 | Reorder S0 JSON maps | identical CAPs and IDs |
| D04 | Serialize S3 CAP | no S4 semantic/score fields |
| D05 | Mismatched S0/S1 fingerprints | runner rejects input |
| D06 | Incomplete S0 or S1 | runner rejects input |
| D07 | Existing UserDevice fixture | exact 4 CAP contract preserved |

## 20. Watchlist legacy calibration

The current unmodified legacy binary reports:

```text
68 traversal paths to Watchlist
legacy baseline SHA-256:
0cd5f178ba551425b20fe4462651d6eb25b8c5b04bc1f585a656d0fb9a1e9839
```

This number is **not** the expected S3 traversal total. The legacy tool uses a
global visited set, collapses Connection wrappers, and cannot traverse
interfaces/unions. S3 may and is expected to emit additional traversal paths.

### Fixture preparation

During implementation, create a committed Watchlist calibration fixture derived
from the real S0 IR:

1. Freeze the 68 text lines as `legacy_paths.txt`.
2. Resolve each legacy path against S0.
3. Expand every collapsed Connection/Edge segment to canonical S3 edge IDs.
4. Store the resulting 68 ordered edge-ID arrays in
   `legacy_paths.canonical.json`.
5. Build a deterministic schema subset containing all required canonical edges,
   plus `Query.node`, `Query.nodes`, `Node`, and `Watchlist`.

Production S3 never reads the legacy text file. It exists only as a regression
oracle.

### Watchlist acceptance assertions

1. All 68 canonicalized legacy edge sequences are present among S3 traversal
   CAPs.
2. No assertion requires `S3 traversal count == 68`.
3. No assertion requires `total CAP count == 70`.
4. One exact global-ID CAP exists for `Query.node`.
5. One exact global-ID CAP exists for `Query.nodes`.
6. Both direct global CAPs end with
   `type_condition:Node->Watchlist` and use `origin: global_id`.
7. Their CAP IDs match the normative content-hash rule.
8. Repeated runs produce byte-identical output.
9. Output contains no selector, sanitizer, flow, or score fields.

The required relation is:

```text
legacy 68 canonical paths subset-of S3 traversal paths
```

not equality.

## 21. Performance checks

Performance tests are measurements, not recall-changing cutoffs:

- graph build time and edge count on the full 1,665-type introspection IR;
- reverse-reachable node/edge count for Watchlist and UserDevice;
- CAP count, maximum edge count, and peak resident memory per target;
- deterministic output size;
- serial multi-target behavior on the UserDevice fixture and a small sink set.

No optimization is accepted unless CAP edge-sequence equivalence is proven on
the synthetic and Watchlist fixtures.

## 22. Implementation sequence

1. Generalize the envelope stage constructor and add typed artifact reader.
2. Add read-only S1 and full S3 contracts.
3. Implement canonical graph model and builder.
4. Implement reverse reachability.
5. Implement iterative edge-simple DFS and exact-path dedup.
6. Implement CAP IDs, deterministic ordering, and display projection.
7. Implement cycle templates.
8. Implement structural `node`/`nodes` detection and direct CAP merge rules.
9. Add the manual single-target API and CLI.
10. Add the official S0+S1 S3 runner and CLI.
11. Make the existing UserDevice S3 fixture executable against the implementation.
12. Build and freeze the Watchlist legacy calibration fixture.
13. Run full formatting, lint, unit, golden, calibration, and legacy regression tests.

## 23. Definition of done

S3 is complete when:

- the single-target core returns every edge-simple Query path under the defined
  cycle policy;
- OBJECT, INTERFACE, and UNION traversal works through canonical edges;
- Connection/Edge nodes remain in canonical paths;
- valid `node` and `nodes` signatures create separate direct global-ID CAPs;
- longer traversal paths starting from those root fields are not suppressed;
- exact-path dedup and CAP hashing match the normative contract;
- the official S3 adapter preserves S1 sink refs and cross-stage invariants;
- the UserDevice fixture still produces 4 CAPs: 2 global-ID and 2 traversal;
- all 68 frozen Watchlist legacy paths are covered after canonical expansion;
- Watchlist has both direct global-ID CAPs;
- no test treats 68, 70, or any other current count as the total S3 result;
- output is deterministic and contains no S4 semantic fields;
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, and the cross-stage fixture validator pass.

## 24. Implementation result

The implementation provides:

- typed S0/S1 artifact loading and common stage-aware envelopes;
- canonical OBJECT/INTERFACE/UNION graph construction;
- sorted FIELD and TYPE_CONDITION edges;
- iterative path-local edge-simple DFS;
- deterministic cycle templates, CAP hashes, ordering, and display projection;
- structural `node` and `nodes` detection with direct-CAP deduplication;
- manual single-target and official S0+S1 CLI entry points;
- executable UserDevice, graph, cycle, global-ID, CLI, and Watchlist tests.

The frozen 68-path Watchlist baseline expands to a 77-type, 115-edge
calibration subset. S3 emits 2,440 traversal CAPs plus two direct global-ID
CAPs on that subset. This is intentionally a superset assertion, not a fixed
production count.

The full real-schema reverse-reachable subgraph is much larger: approximately
817 nodes and 3,670 edges for Watchlist. Since the normative model requires all
edge-simple paths and forbids logical path/depth quotas, full-schema output can
grow combinatorially. The implementation does not silently truncate it;
reverse reachability remains the only production pruning and is covered by a
losslessness test.

A bounded debug-build measurement on the full Watchlist IR did not finish
within 5 seconds and reached approximately 56 MiB resident memory. The process
was externally stopped and, because output is written atomically only after
enumeration completes, no partial artifact was produced.
