# Stage 3 Definite Selectors and Route-Family DAG Plan

Status: Deliverable A implemented; Deliverable B planned

Supersedes:

- `stage-3-route-analysis-scalability.md`
- the selector model in `stage-3-route-family-dag.md`

Depends on:

- implemented S2 argument classifier;
- implemented target-absorbing abstract automaton;
- S3/v2.2 canonical route compatibility output.

## 1. Objective

Deliver two changes in order:

1. Remove `PossibleSelector` from the executable S3/S4 route pipeline and retain
   only definite object selectors.
2. Replace flat S3 route materialization with an SCC-condensed route-family DAG
   and lazy route expansion.

The first change reduces selector noise. The second removes structural and
selector duplication from persisted output.

## 2. Current Baseline

Full generated S2 artifact:

| Classification | Count |
|---|---:|
| Total argument records | 6,104 |
| `object_selector` | 876 |
| `possible_selector` | 3,579 |
| `noise` only | 1,649 |
| Current S3 selector feed | 4,455 |

Removing `possible_selector` from S3 leaves 876 executable selectors:

```text
4,455 -> 876
reduction = 3,579 selectors = 80.3%
```

The 3,579 possible selectors are mostly ambiguous values:

- 983 `String`;
- 699 `Boolean`;
- 491 `Int`;
- 367 `Sport`;
- 184 `Rarity`;
- 1,222 enum records overall.

Current full-S2 S3/v2.2 calibration for `Watchlist`:

```text
26,990 canonical routes
193 MiB JSON
50.76 seconds
~1.48 GiB peak RSS
3 OPEN routes remain correct
```

After Deliverable A (`possible_selector` removed from executable S3/S4):

```text
Watchlist calibration: 77 routes, 305,613 bytes, 8.95 seconds, 472,680 KiB RSS
Watchlist full S2:    10,374 routes, 78,208,119 bytes, 14.95 seconds, 812,648 KiB RSS
3 OPEN routes remain correct
```

Compared with the previous full-S2 baseline, this is approximately:

```text
61.6% fewer routes
70.5% lower runtime
46.4% lower peak RSS
```

This output is still large because flat v2.2 materializes combinations of:

```text
canonical witness x selector x terminal semantic state
```

Selector reduction addresses the first multiplier. The DAG addresses the
remaining representation problem.

## 3. Architectural Decision

### 3.1 S2 remains exhaustive

S2 continues recording every argument and may retain
`classification: possible_selector` for audit.

Do not relabel ambiguous arguments as `noise`:

- `sport`, `status`, and arbitrary strings are not proven operational noise;
- S4 still needs complete argument metadata for required literals, enums, and
  input objects;
- changing them to `noise` would destroy information without improving S3.

“Remove PossibleSelector” therefore means:

```text
S2 audit artifact: retained
S3 RouteFacts: excluded
S3 automaton: excluded
S3 verdict/provenance: excluded
S3 route output: excluded
S4 selector seed requirements: excluded
```

Required non-selector arguments remain S4 binding requirements. For example,
`sport: Sport!` may still receive an enum binding; it simply does not claim to
select the target object.

### 3.2 Executable selector rule

An argument is an executable selector only when:

```text
classification contains object_selector
AND confidence is medium or high
```

This includes:

- GraphQL `ID`;
- configured identity scalars such as `UUID`;
- exact names such as `id`, `slug`, `assetId`, `handle`;
- selector suffixes such as `userId`, `deckSlug`;
- authorization-context selectors when also classified as object selectors.

It excludes:

- all `possible_selector`;
- `object_selector` with low confidence caused by selector/noise conflict;
- sole `noise`;
- unrelated argument classifications.

### 3.3 Structural routes are independent from selector eligibility

Removing a possible selector must not remove a structural graph edge:

```text
Query.search(sport:) -> SearchResult -> Watchlist
```

remains structurally reachable. The argument `sport` is merely absent from
selector provenance. If required, S4 handles it as a normal argument binding.

## 4. Deliverable A: Definite-Selector-Only Pipeline

### 4.1 RouteFacts changes

Centralize selector eligibility:

```rust
fn executable_selector_class(argument: &ClassifiedArgument) -> SelectorClass {
    if argument.classifications.contains(&ObjectSelector)
        && matches!(argument.confidence, Medium | High)
    {
        SelectorClass::Definite
    } else {
        SelectorClass::None
    }
}
```

`RouteFacts::build()` must add only executable selectors to:

- `selectors_by_field`;
- `selectors_by_ref`;
- `selectors_by_id`.

Possible selectors must not affect:

- `has_required`;
- no-selector pass-through suppression;
- activation transitions;
- selector provenance;
- route IDs or signatures.

### 4.2 Abstract automaton simplification

The production automaton becomes binary:

```text
selector_class = none | definite
```

Remove from the active implementation:

- `has_possible`;
- `possible_selector_ids`;
- `ActivatePossible`;
- possible-selector transition branches.

For S3/v2.2 deserialization compatibility, the public `SelectorClass::Possible`
enum variant may remain temporarily, but newly generated output must never use
it. Remove the variant from the S3/v3 DAG contract.

### 4.3 Verdict behavior

Verdict rules remain:

```text
boundary present                          -> guarded
definite selector + continuity same       -> open
otherwise                                 -> unknown
```

Routes previously backed only by possible selectors become no-selector
structural routes:

- they remain reachable;
- they normally become `unknown`, unless guarded by a boundary;
- they no longer create selector-specific route variants;
- S4 does not attempt to harvest a semantic selector seed for them.

### 4.4 S4 behavior

S4 still scans all required arguments on the concrete witness.

Binding logic remains separate from selector logic:

| Argument | S3 selector | S4 handling |
|---|---|---|
| `id: ID!` object selector | yes | seed dependency |
| `slug: String!` exact selector | yes | seed dependency |
| `sport: Sport!` | no | enum/static binding |
| `first: Int` | no | bounded pagination |
| unknown required string | no | generated scalar or explicit unresolved |

No required argument may disappear silently because it was removed from
selector provenance.

### 4.5 Phase A implementation steps

1. Add a focused selector-eligibility helper.
2. Filter `RouteFacts` indexes to definite selectors only.
3. Remove possible branches from `FieldAbstractFact`.
4. Remove `ActivatePossible` from the automaton.
5. Keep structural pass-through for fields whose arguments are only possible.
6. Update route contract validation to reject newly emitted possible selectors.
7. Update S4 tests proving non-selector required arguments are still bound.
8. Regenerate full S2/S3 calibration artifacts.
9. Record new runtime, RSS, artifact size, and verdict distribution.

### 4.6 Phase A tests

Classification eligibility:

- `ID/high` -> definite.
- exact `assetId/high` -> definite.
- configured `UUID/medium` -> definite.
- `possible_selector/low` -> excluded.
- `object_selector/low` conflict -> excluded.
- `noise/high` -> excluded.

Structural behavior:

- removing a possible selector does not remove its field edge;
- required possible selector does not suppress no-selector pass-through;
- required enum still appears in S4 requirements;
- input-object possible leaves do not become selector activations.

Watchlist anchors:

- `Query.node(id:)` remains OPEN.
- `Query.nodes(ids:)` remains OPEN.
- `MarketRoot.watchlist(id:)` remains OPEN.
- no route contains a selector classified only as `possible_selector`.

### 4.7 Phase A acceptance gates

- Full selector feed is exactly 876 on the current generated S2 artifact.
- All 3,579 possible selectors are excluded from S3.
- Structural target reachability is unchanged.
- The three Watchlist OPEN anchors are unchanged.
- Every S4 required argument remains bound or explicitly unresolved.
- S3/v2.2 full Watchlist completes without timeout.
- Record a definite-only flat baseline for comparison with the DAG.

Do not pin the new total route count before running the implementation. Pin
semantic anchors and selector eligibility first.

## 5. Deliverable B: Route-Family DAG

### 5.1 Why the DAG is still required

Selector filtering does not solve structural family multiplication:

```text
B -> A -> Watchlist
C -> A -> Watchlist
D -> A -> Watchlist
```

All three families must remain testable, but `A -> Watchlist` should be stored
once.

The DAG also prevents flat duplication when one selector generator is valid
for many continuations.

### 5.2 Input graph

Reuse the definite-only abstract automaton. Do not rebuild route semantics in a
second implementation.

Semantic state:

```text
(type_id,
 origin_mode,
 selector_active,
 selector_continuity,
 boundary_bits,
 terminal_semantic_edge_id)
```

`selector_active` is Boolean in S3/v3. Exact selector IDs live in referenced
generator sets.

Target semantics:

- first target arrival is terminal;
- target states have no outgoing transitions;
- target re-entry is not an acquisition route;
- pre-target cycles remain represented.

### 5.3 Compact internal representation

Before serialization, intern repeated values:

- state IDs instead of repeated `StateRef`;
- edge IDs instead of repeated `PathEdge`;
- selector-set IDs instead of repeated selector arrays;
- transition IDs referencing source state, target state, edge, effect, and
  selector set.

Selector sets:

```text
selector_set_id = hash(sorted selector_ids)
```

Many abstract transitions over the same field can then reference one selector
set instead of repeating its members.

### 5.4 SCC condensation

The abstract state graph can contain pre-target cycles, so:

1. Run deterministic Tarjan or Kosaraju SCC discovery.
2. Sort member state and transition IDs canonically.
3. Collapse SCCs into a component DAG.
4. Retain internal SCC transitions in the component.
5. Mark cycle-capable components explicitly.
6. Assert the component graph is acyclic.

Condensation must preserve all entry-to-terminal alternatives.

### 5.5 S3/v3 contract

Envelope:

```json
{
  "contract_version": "3.0",
  "stage": "S3",
  "data": {
    "analysis_model": "route-family-dag-v1",
    "selector_mode": "definite_only",
    "coverage": "complete_graph",
    "targets": {
      "type:Watchlist": {
        "target_type_id": "type:Watchlist",
        "dag_id": "dag:sha256:...",
        "entry_state_id": "state:sha256:...",
        "states": [],
        "edge_table": [],
        "selector_facts": {},
        "selector_sets": {},
        "transitions": [],
        "components": [],
        "component_edges": [],
        "terminals": []
      }
    }
  }
}
```

State records contain semantic facts only.

Transition records contain:

- source and target state IDs;
- schema edge ID;
- transition effect;
- optional selector-set ID.

Terminal records contain:

- terminal state ID;
- verdict;
- origin;
- boundary families;
- selector continuity;
- terminal semantic edge.

The DAG contract does not contain flat `routes[]`.

### 5.6 Stable IDs

- `state_id`: canonical semantic state hash.
- `selector_set_id`: sorted selector ID set hash.
- `transition_id`: source, target, edge, effect, selector-set hash.
- `component_id`: sorted member state/transition hash.
- `terminal_id`: target and terminal semantic state hash.
- `dag_id`: target, policy fingerprint, selector mode, components, and
  component edges hash.

Concrete IDs are computed only during expansion:

- `path_family_id`: ordered schema edge sequence hash.
- `route_instance_id`: target, selector ID, family ID, and terminal hash.

### 5.7 Lazy route expansion

Add:

```text
RouteFamilyExpander::expand(
    target_dag,
    verdict_filter,
    budget,
    cursor
) -> Iterator<RouteInstance>
```

The iterator:

- walks sorted component transitions deterministically;
- traverses internal SCC edges using the edge-simple rule;
- carries one active exact selector ID;
- replaces the active selector on activation;
- never combines a selector with a path that does not contain its activation;
- emits a complete ordered witness for S4;
- computes concrete stable IDs;
- supports resumable cursors;
- reports `complete` or `budget_exhausted`.

No global selector-by-family Cartesian product is allowed.

### 5.8 Canonical witnesses

Canonical witnesses are diagnostic only.

Store at most:

- one no-selector witness per terminal semantic state;
- one definite-selector-class witness per terminal semantic state.

Do not persist one canonical witness per exact selector. Exact selectors remain
referenced through transition selector sets.

### 5.9 S4 migration

Introduce an in-memory `RouteInstance` compatible with current seed planning.

Pipeline:

```text
S3 DAG
  -> filter terminal verdicts
  -> lazily expand one family
  -> build one S4 seed plan
  -> emit/cache
  -> discard route instance
```

Default production selection is `open + unknown`. Guarded expansion remains
available explicitly.

Migration sequence:

1. Keep S3/v2.2 reader for existing artifacts.
2. Add S3/v3 DAG reader.
3. Add a `RouteSource` adapter for flat v2.2 and lazy v3.
4. Reuse current S4 planning logic on each `RouteInstance`.
5. Move execution dedup to canonical operation plus bindings.
6. Stop accepting v2.2 as the production default after calibration parity.

### 5.10 Runtime behavior

Runtime remains route-centered:

- request a type;
- select `open|unknown`;
- expand one route family;
- resolve its seed plan;
- execute it;
- checkpoint the expansion cursor.

Runtime limits apply to emitted route instances, not DAG nodes. Any limit must
produce bounded coverage metadata.

## 6. Deliverable B Implementation Steps

### Phase B1: Intern the automaton

- Replace repeated state/transition payloads with interned IDs.
- Add selector-set interning.
- Verify byte-stable canonical ordering.

### Phase B2: Condense SCCs

- Implement deterministic SCC discovery.
- Build and validate the component DAG.
- Preserve internal transitions and cycle capability.

### Phase B3: Add S3/v3 contracts

- Add DAG contract structs and validation.
- Add stable IDs and fingerprints.
- Emit `selector_mode: definite_only`.
- Add CLI serialization and artifact reading.

### Phase B4: Add lazy expansion

- Implement deterministic expansion.
- Add selector provenance carrying.
- Add edge-simple SCC traversal.
- Add budget and resumable cursor.
- Add optional JSONL diagnostic output.

### Phase B5: Migrate S4/runtime

- Add `RouteSource`.
- Stream route instances into S4.
- Filter `open|unknown` before expansion.
- Preserve seed correlation and execution dedup.

### Phase B6: Remove flat production output

- Keep v2.2 only as a compatibility/reference mode.
- Make v3 DAG the default S3 output.
- Document explicit compatibility flags and removal timeline.

## 7. DAG Tests

Structural families:

- `B/C/D -> A -> sink` stores one shared suffix and expands three families.
- Divergence/reconvergence preserves every family.
- Interface and union type conditions remain distinct.
- `node` and `nodes` remain distinct global-ID origins.

Selector provenance:

- selector on B never appears on C or D;
- every selector route contains its activation transition;
- later selector activation replaces earlier provenance;
- no possible selector appears in selector sets;
- required definite selector suppresses no-selector activation choice;
- required non-selector does not suppress structural pass-through.

Cycles:

- target states have no outgoing transitions;
- component graph is acyclic;
- internal SCC transitions remain represented;
- edge-simple expansion terminates;
- expansion cursor resumes deterministically.

Downstream:

- each S4 route receives complete ordered edges;
- required enums and literals are still bound;
- seed requirements do not leak between route instances;
- `budget_exhausted` propagates to runtime coverage.

## 8. End-to-End Acceptance Gates

Selector reduction:

- 4,455 current selectors become exactly 876 executable selectors.
- Zero possible selector reaches S3 transitions or S4 selector requirements.
- Watchlist OPEN anchors remain exactly `node`, `nodes`, and
  `market.watchlist(id:)`.

DAG correctness:

- small-fixture full expansion equals the exact reference family set;
- B/C/D regression proves no family loss;
- selector/witness correlation has zero violations;
- repeated runs produce byte-identical DAGs and route-instance order.

Performance:

- full-schema Watchlist DAG build completes within 10 seconds in release mode;
- peak RSS is below 512 MiB on the current workstation;
- serialized DAG is at least 4x smaller than the definite-only flat v2.2
  baseline;
- first 100 `open|unknown` route instances stream without materializing the
  complete family set.

If the first implementation misses a performance gate, preserve correctness
and report the measured bottleneck. Do not truncate or collapse route families.

## 9. Non-Goals

- Converting ambiguous arguments into semantically proven noise.
- Dropping required non-selector arguments from query generation.
- Grouping routes by assumed backend-equivalent behavior.
- Runtime bug classification or policy oracle changes.
- Silent expansion limits.
- Treating a canonical witness as complete family coverage.
