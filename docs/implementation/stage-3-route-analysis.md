# Stage 3 Route Analysis Implementation Plan

**Status:** Implemented on June 9, 2026.

The implementation lives in `src/route/`, `src/graph/plumbing.rs`,
`src/stages/s3_routes/`, `src/contracts/s2.rs`, and
`src/contracts/s3_routes.rs`. State, transition, verdict, and witness logic are
kept together in `src/route/worklist.rs` so the fixed-point invariants remain
local.

**Design authority:** `docs/design/routeVerdict.md`.

This milestone replaces production edge-simple path enumeration with constrained
route analysis. The existing enumerator remains available only as a diagnostic
and calibration oracle.

## 1. Objective

For every S1-selected target type, answer:

> Which semantically distinct Query routes can reach this sink, does a
> selector still control the sink, which authorization/visibility boundaries
> are present, and what deterministic witness proves the route?

The production engine must:

- avoid enumerating every simple path;
- preserve structural reachability through OBJECT, INTERFACE, UNION,
  Connection, and Edge nodes;
- consume S2 selector facts;
- classify routes as `open`, `unknown`, or `guarded`;
- collapse irrelevant prefix variants by semantic route signature;
- retain one deterministic structural witness per signature;
- never drop `guarded` or unusual `unknown` routes.

## 2. Architecture decision

The pipeline becomes:

```text
S0 ─┬─> S1
    └─> S2

S0 + S1 + S2 + route policy
            |
            v
     S3 Route Analysis
            |
            v
     S4 Thin Ranking
            |
            v
       S5 Assembly
```

Official S3 is no longer structural-only. It owns route compaction and verdict
facts. S4 must not rediscover routes; it only applies versioned ranking policy
to S3 route output.

The existing `enumerate` command and edge-simple DFS remain as a v1
diagnostic/calibration surface. They are not called by `stage s3` and their
output is not accepted by the future S4 route-ranking implementation.

## 3. Contract migration

This is a breaking S3 artifact change.

- S0, S1, and S2 inputs remain `contract_version: "1.0"`.
- Official S3 route output uses `contract_version: "2.0"`.
- `stage: "S3"` remains unchanged.
- Output filename becomes `routes.json`.
- Legacy `paths.json` remains a diagnostic S3/v1 artifact only.
- The artifact reader must validate an expected `(stage, contract_version)`
  pair instead of globally requiring version `1.0`.

The old `tests/fixtures/user_device/s3_paths.json` is retained as a historical
v1 path-enumerator oracle. A new `s3_routes.json` becomes the current S3 golden
artifact. Downstream v1 S4/S5 fixtures must be marked legacy until their route
contract migration is implemented.

## 4. Inputs

Official semantic S3 consumes:

```text
schema_ir.json   S0/v1
sinks.json       S1/v1
args.json        S2/v1
route policy     route-analysis-v1
```

The runner validates:

- all three artifacts are complete and Query-scoped;
- all fingerprints match;
- every selected S1 target exists in S0 and is an OBJECT, INTERFACE, or UNION;
- every S1 sink ref belongs to its selected target;
- every S2 field, argument, input path, and TypeRef resolves against S0;
- S2 classification and confidence values are closed enums;
- exact policy field IDs exist in S0;
- a selected target produces at least one route.

S3 introduces a read-only typed S2 contract. It does not implement the S2
classifier in this milestone.

## 5. Route policy

Add a versioned, typed policy file:

```text
config/profiles/route-analysis-v1.json
```

Initial shape:

```json
{
  "model_version": "route-analysis-v1",
  "self_scope_root_tokens": ["currentUser", "me", "viewer"],
  "visibility_tokens": ["public"],
  "exact_boundaries": {
    "field:MarketRoot.sorareWatchlists": {
      "family": "visibility",
      "evidence": "system_owned_public_collection"
    }
  }
}
```

Rules:

- generic matching uses the shared identifier tokenizer, not broad substring
  regex;
- self-scope heuristics apply only to Query root fields;
- `current*` in the middle of a route is not automatically a boundary;
- exact domain policy uses stable field IDs;
- policy is an evidence source, not a separate semantic boundary family;
- semantic boundary families in milestone 1 are `self_scope` and `visibility`;
- hash exact policy bytes as `policy_fingerprint` for reproducibility.

## 6. S2 read-only contract

Add `src/contracts/s2.rs` for the existing fixture shape:

```text
ArgumentsData
  fields: BTreeMap<field_id, ClassifiedField>

ClassifiedField
  arguments: Vec<ClassifiedArgument>

ClassifiedArgument
  arg_ref
  root_arg_ref
  arg_path
  input_path
  type_ref
  classifications
  signals
  confidence
```

Closed enums:

```text
ArgumentClassification =
  object_selector | authz_modifier | noise | possible_selector

Confidence = low | medium | high
```

Build a `SelectorIndex` keyed by `field_id`. It retains only
`object_selector` and `possible_selector` facts for route analysis, but the
typed artifact reader validates every classification.

Selector abstraction:

```text
definite = object_selector with medium/high confidence
possible = possible_selector at any confidence
           OR object_selector with low confidence
none     = no active selector provenance
```

## 7. S3/v2 output contract

Add a new contract module rather than overloading `CandidateAccessPath`:

```text
RoutesData
  analysis_model
  policy_fingerprint
  targets: BTreeMap<type_id, TargetRoutes>

TargetRoutes
  target_type_id
  sink_ref_ids
  reachability
  best_verdict
  routes

Route
  route_id
  target_type_id
  origin
  verdict
  selector
  selector_continuity
  terminal_semantic_edge_id
  boundaries
  signature
  witness

RouteWitness
  witness_id
  entry_field_id
  edges
  field_hop_count
  display_projection
```

Closed enums:

```text
RouteVerdict       = open | unknown | guarded
SelectorClass      = none | possible | definite
SelectorContinuity = not_applicable | same | unknown
BoundaryFamily     = self_scope | visibility
BoundarySource     = heuristic | policy
RouteOrigin        = traversal | global_id
Reachability       = reachable | query_unreachable
```

`selector` is null when no selector provenance is active. Otherwise it copies:

```text
arg_ref, root_arg_ref, arg_path, input_path,
type_ref, classification, confidence, selected_type_id
```

Each boundary records:

```text
family, source, root_edge_id, evidence
```

Boundary arrays are sorted and deduplicated. No S4 score, ranking bucket, or
score breakdown appears in S3/v2.

## 8. Stable identities

### 8.1 Route signature

Canonical route signature:

```text
[
  target_type_id,
  origin,
  selector_ref_or_null,
  terminal_semantic_edge_id,
  sorted_boundary_families,
  selector_continuity,
  verdict
]
```

Serialize as compact UTF-8 JSON with array order preserved. `route_id` is:

```text
route:sha256:<lowercase hex>
```

Distinct `node`, `nodes`, and `market.watchlist(id:)` routes remain distinct
because their selector refs and terminal semantic edges differ.

### 8.2 Witness identity

`witness_id` keeps the existing path identity rule:

```text
SHA-256(compact JSON [target_type_id, ordered_edge_id_array])
```

Changing the chosen prefix witness may change `witness_id` without changing
`route_id`. This separates semantic route identity from presentation evidence.

## 9. Graph annotations

Keep the canonical TypeGraph unchanged for structural connectivity. Add
derived indexes:

```text
PlumbingIndex
SelectorIndex
BoundaryIndex
ReverseReachability
```

### 9.1 Plumbing detection

Mark only internal wrapper fields as plumbing:

- Connection `.nodes`;
- Connection `.edges`;
- Edge `.node`.

Detection is structural:

- a Connection candidate is an OBJECT with `pageInfo` and `nodes` or `edges`;
- an Edge candidate is an OBJECT with a composite `node` field;
- the semantic field returning a Connection is not plumbing;
- `Query.node` and `Query.nodes` are never plumbing.

Names may support evidence but cannot establish plumbing alone. False-positive
types ending in `Connection`/`Edge` must remain ordinary semantic nodes.

### 9.2 Terminal semantic edge

The state tracks the most recent non-plumbing FIELD edge. TYPE_CONDITION and
plumbing do not replace it.

Example:

```text
User.publicWatchlists
  -> WatchlistConnection.nodes
  -> Watchlist
```

has terminal semantic edge `field:User.publicWatchlists`.

## 10. Product state

Milestone-1 state:

```text
RouteState
  type_id
  selector_class
  selector_ref
  selector_continuity
  boundary_bits
  terminal_semantic_edge_id
  origin_seed
```

`boundary_bits` is a fixed bitset:

```text
SELF_SCOPE | VISIBILITY
```

The worklist memoizes the complete semantic state. Partial-order pruning is not
used in milestone 1 because `same` versus `unknown`, boundary-free versus
guarded, and different selector refs can produce different route signatures.

Dominance is therefore deliberately strict:

- only identical semantic states dominate each other;
- for an identical state, retain the better witness;
- no-boundary state does not erase a boundary state;
- `same` does not erase `unknown`;
- different selector refs never dominate one another.

This exact-state memoization is finite and recall-safe.

## 11. Transition rules

Process sorted outgoing graph edges.

### 11.1 FIELD without a new selector

- update terminal semantic edge if the FIELD is not plumbing;
- preserve continuity through plumbing;
- otherwise, if a selector is active, change continuity to `unknown`;
- if no selector is active, continuity remains `not_applicable`;
- detect and OR any boundary bits.

### 11.2 FIELD with selector facts

For every selector fact, create a successor state:

- replace provenance with that selector;
- set `selected_type_id` to the FIELD return type;
- set continuity to `same`;
- set class to `definite` or `possible`;
- retain route boundary bits; a new selector does not erase prior boundaries.

Invocation branching:

- direct required selector: do not create a selector-free invocation branch;
- optional selector: also retain the transition without using it;
- nested input selector: retain a selector-free branch unless the complete
  root/input path is proven required from S0;
- multiple selectors create separate states in milestone 1.

GraphQL requiredness is `NON_NULL` with no default value. For nested input
paths, every path component must be required before suppressing the
selector-free branch.

### 11.3 TYPE_CONDITION

- preserve selector provenance and continuity;
- preserve boundaries and terminal semantic edge;
- refine only the graph type.

### 11.4 Global-ID origin

Reuse structural `node`/`nodes` validation:

- exact direct `Query.node/nodes -> Node -> TYPE_CONDITION(target)` routes use
  `origin: global_id`;
- longer routes beginning with `node`/`nodes` remain traversal routes;
- global-ID does not bypass selector or boundary rules.

## 12. Verdict function

Evaluate in this order:

```text
if boundary_bits != empty:
    guarded
else if selector_class == definite
        and selector_continuity == same:
    open
else:
    unknown
```

Consequences:

- a possible or low-confidence selector never becomes `open`;
- no-selector/no-boundary routes are `unknown`;
- boundary heuristics never make a route unreachable;
- hop count never changes verdict;
- unusual interface routes remain visible as `unknown`.

Target-level `query_unreachable` is separate. The official S3 runner treats an
S1-selected target with no reachable route as a cross-stage contract error.

## 13. Worklist and witness selection

Use a deterministic fixed-point worklist, not recursive DFS.

Per target:

1. Compute reverse-reachable nodes.
2. Seed Query root state.
3. Explore only edges whose destination can reach the target.
4. Memoize complete RouteState.
5. When the target is reached, derive verdict and route signature.
6. Continue processing target states so cycles/re-entry can expose a different
   semantic state.
7. Deduplicate by route signature.

For identical semantic state/signature, select witness by:

1. fewer FIELD hops;
2. fewer total edges;
3. ordered edge-ID sequence lexicographically;
4. witness ID as final tie-breaker.

This ordering selects a concise witness only after semantic equivalence is
established. It is never used to prefer a guarded route over an open route.

Store witnesses through predecessor arena indices while exploring. Materialize
edge arrays only for final routes to avoid copying full prefixes per state.

## 14. Target and route ordering

Target map uses stable type IDs.

Route output ordering:

1. verdict: `open`, `unknown`, `guarded`;
2. origin: `global_id`, `traversal`;
3. terminal semantic edge ID;
4. selector ref, null last;
5. route ID.

This is deterministic triage ordering, not S4 risk scoring.

## 15. Core APIs

Shared engine:

```rust
pub fn analyze_target(
    graph: &TypeGraph,
    target_type_id: &str,
    facts: &RouteFacts,
) -> Result<TargetRoutes, RouteAnalysisError>;
```

Structural diagnostic mode:

```rust
pub fn analyze_reachability(
    graph: &TypeGraph,
    target_type_id: &str,
) -> Result<StructuralRouteReport, RouteAnalysisError>;
```

Official adapter:

```rust
pub fn analyze_selected_targets(
    schema: &Envelope<SchemaIrData>,
    sinks: &Envelope<SinksData>,
    args: &Envelope<ArgumentsData>,
    policy: &RoutePolicy,
) -> Result<Envelope<RoutesData>, S3RouteError>;
```

The graph and indexes are built once per run. Targets execute serially in the
first implementation.

## 16. Module layout

```text
src/
├── contracts/
│   ├── s2.rs
│   └── s3_routes.rs
├── graph/
│   └── plumbing.rs
├── route/
│   ├── mod.rs
│   ├── facts.rs
│   ├── policy.rs
│   ├── signature.rs
│   ├── state.rs
│   ├── transition.rs
│   ├── verdict.rs
│   ├── witness.rs
│   └── worklist.rs
├── stages/
│   ├── s3_paths/                # legacy diagnostic/calibration
│   └── s3_routes/
│       ├── mod.rs
│       └── runner.rs
└── cli/commands/
    ├── enumerate.rs             # legacy diagnostic
    ├── route.rs                 # manual structural/semantic target
    └── s3.rs                    # official semantic route stage
```

`route` may depend on canonical graph and typed facts, but not on CLI or file
I/O. `graph` must not depend on S2/S3 contracts.

## 17. CLI

Manual structural reachability:

```bash
cargo run -- route \
  --schema-ir output/schema_ir.introspection.json \
  --target Watchlist \
  --output output/route.watchlist.structural.json
```

Manual semantic route analysis:

```bash
cargo run -- route \
  --schema-ir output/schema_ir.introspection.json \
  --args output/args.json \
  --policy config/profiles/route-analysis-v1.json \
  --target Watchlist \
  --output output/route.watchlist.json
```

Official stage:

```bash
cargo run -- stage s3 \
  --schema-ir output/schema_ir.introspection.json \
  --sinks output/sinks.json \
  --args output/args.json \
  --policy config/profiles/route-analysis-v1.json \
  --output output/routes.json
```

Keep the current `enumerate` CLI for legacy calibration, but label it
diagnostic in help and documentation.

Exit classes remain:

```text
0 complete output written
2 invalid CLI usage
3 input read failure
4 JSON/artifact/policy contract failure
5 graph, facts, or cross-stage invariant failure
6 output write failure
7 controlled incomplete/resource failure
```

## 18. Test plan

### Contract and hashing

| ID | Test | Expected |
|---|---|---|
| C01 | Deserialize current S2 fixture | exact typed facts |
| C02 | S3/v2 route JSON | no score/ranking fields |
| C03 | Known route signature | exact pinned `route_id` |
| C04 | Known witness path | existing pinned path hash |
| C05 | Same signature, different witness | same route ID |
| C06 | Reordered input maps | byte-identical route output |
| C07 | S3/v1 passed to v2 consumer | explicit version rejection |

### Plumbing and terminal semantics

| ID | Test | Expected |
|---|---|---|
| P01 | Structural Connection with `.nodes` | internal edge marked plumbing |
| P02 | Connection through `.edges.node` | both internal edges plumbing |
| P03 | Object name ends in Connection but lacks shape | not plumbing |
| P04 | Ordinary field named `node` | not plumbing |
| P05 | `Query.node` | selector entry, never plumbing |
| P06 | `publicWatchlists -> Connection.nodes` | terminal semantic edge is `publicWatchlists` |

### Selector transitions

| ID | Test | Expected |
|---|---|---|
| S01 | Direct required selector | definite/same; no selector-free branch |
| S02 | Optional selector | selector and selector-free states |
| S03 | Possible selector | unknown, never open |
| S04 | Low-confidence object selector | unknown |
| S05 | New selector after unknown transition | continuity resets to same |
| S06 | Multiple selectors on one field | separate route states |
| S07 | Nested optional selector | selector-free branch retained |

### Continuity and boundaries

| ID | Test | Expected |
|---|---|---|
| V01 | TYPE_CONDITION | continuity preserved |
| V02 | Connection plumbing | selector continuity preserved |
| V03 | Ordinary object transition | same becomes unknown |
| V04 | Query.currentUser | self_scope bit |
| V05 | mid-path currentUserSubscription | no boundary; continuity unknown |
| V06 | terminal `public*` | visibility bit |
| V07 | exact sorare policy field | guarded/visibility with policy source |
| V08 | unrelated `sorare*` field | no automatic boundary |

### Worklist and compaction

| ID | Test | Expected |
|---|---|---|
| W01 | Shared suffix/prefix variants | one witness per equal signature |
| W02 | Graph cycle | fixed-point terminates |
| W03 | Short guarded and long open route | both retained; sink best=open |
| W04 | Same signature with two witnesses | shortest FIELD-hop witness selected |
| W05 | Distinct selector refs | no dominance |
| W06 | Boundary/no-boundary states | no dominance |
| W07 | Same/unknown continuity | no dominance |
| W08 | Run twice | byte-identical artifact |

### Watchlist ground truth

Create a deterministic fixture containing the required real-schema structures,
S1 target, S2 facts, and policy:

```text
tests/fixtures/s3_routes/watchlist/
  schema_ir.json
  sinks.json
  args.json
  policy.json
  expected.routes.json
```

Assertions:

| Route | Expected verdict |
|---|---|
| `Query.node(id) -> ... on Watchlist` | open |
| `Query.nodes(ids) -> ... on Watchlist` | open |
| `Query.market -> MarketRoot.watchlist(id)` | open |
| `Query.currentUser -> CurrentUser.myWatchlists` | guarded/self_scope |
| semantic `publicWatchlists` routes | guarded/visibility |
| `MarketRoot.sorareWatchlists` | guarded/visibility, policy evidence |
| `Query.anyCard -> ... -> WithSubscriptionsInterface -> Watchlist` | unknown |

Additional acceptance:

- all canonical witnesses retain real Connection/TYPE_CONDITION edges;
- `publicWatchlists` terminal is not `.nodes`;
- node, nodes, and market.watchlist remain three distinct open route IDs;
- guarded routes are present even when the target best verdict is open;
- no output count is tied to 68, 2,440, or another path-enumerator count.

### UserDevice migration

Expected route set remains four semantic routes:

```text
Query.node             open/global_id
Query.nodes            open/global_id
Query.currentUser.devices
                       guarded/self_scope
Query.currentUser.currentDevice
                       guarded/self_scope
```

The new fixture pins route IDs, verdicts, boundaries, selector provenance, and
witness edges.

## 19. Performance acceptance

Measure on the full 1,665-type introspection IR:

- graph and index build time;
- reverse-reachable nodes/edges;
- worklist states processed;
- transition count;
- final route signature count;
- peak resident memory;
- output size.

Required behavior:

- full Watchlist semantic analysis completes without enumerating/materializing
  the 2,440 calibration CAPs;
- runtime scales with reachable product states, not the number of simple paths;
- no logical depth/path cutoff;
- no partial artifact is written as `complete`;
- repeated runs produce byte-identical output.

Performance thresholds are recorded after the first implementation rather than
guessed in advance. Correctness tests must not use wall-clock assertions.

## 20. Migration sequence

1. Generalize artifact reader/version validation for S3/v2.
2. Add typed S2 and S3 route contracts.
3. Add typed policy loader and policy fingerprint.
4. Implement structural plumbing detection and tests.
5. Build SelectorIndex and BoundaryIndex with cross-artifact validation.
6. Implement route state, transitions, exact-state memoization, and witness
   arena.
7. Implement verdict, signature hashing, roll-up, and deterministic ordering.
8. Add manual `route` CLI.
9. Switch official `stage s3` to S0+S1+S2+policy and `routes.json`.
10. Keep legacy `enumerate` isolated from the official runner.
11. Add Watchlist and UserDevice route fixtures.
12. Update architecture, implementation overview, README, and fixture validator.
13. Mark old S3/S4/S5 v1 fixtures as legacy until their corresponding route
    migration is complete.
14. Run full-schema performance calibration and record measurements.

## 21. Definition of done

The milestone is complete when:

- official S3 does not call the edge-simple enumerator;
- the product worklist terminates by finite semantic-state memoization;
- route IDs derive from semantic signatures and witness IDs derive from paths;
- selectors, continuity, boundaries, and terminal semantic edges follow the
  design rules;
- Watchlist produces the agreed open/unknown/guarded ground truth;
- the unusual `anyCard` interface route remains present as unknown;
- public/self-scoped routes remain present as guarded;
- prefix explosion is compacted before output;
- full Watchlist analysis completes on the real IR without path materialization;
- S0 and legacy enumerator regressions remain green;
- formatting, strict Clippy, all tests, golden validation, and `git diff
  --check` pass.
