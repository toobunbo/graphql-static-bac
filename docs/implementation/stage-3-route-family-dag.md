# Stage 3 Route-Family DAG Implementation Plan

Status: Superseded by `stage-3-definite-selectors-and-route-family-dag.md`.

The replacement plan first removes `PossibleSelector` from the executable
S3/S4 pipeline, then builds the DAG with definite selector generators only.

Supersedes: `stage-3-route-analysis-scalability.md`

## 1. Problem

The exact-family prototype avoids selector-state explosion, but still
materializes every complete structural path. On the full schema, `Watchlist`
enumeration exceeds 30 seconds even in release mode.

The remaining explosion is structural:

```text
B -> A -> Watchlist
C -> A -> Watchlist
D -> A -> Watchlist
```

The suffix `A -> Watchlist` is repeated once per prefix. S3 must preserve all
three families without serializing three copies of the shared suffix.

The requested target is terminal: the DAG represents alternatives up to the
first arrival at that target. It does not expand away from the target and then
re-enter it.

## 2. Decision

S3/v3 emits a compact route-family DAG instead of a flat `routes[]` list.

- Shared prefixes and suffixes are stored once.
- Exact selector identities are transition generators, not worklist state.
- Concrete routes are expanded lazily by a deterministic iterator.
- S3 completeness means every reachable state and transition is represented.
- It does not mean every concrete family has already been materialized.
- Expansion limits return `budget_exhausted`; they never silently claim
  complete coverage.

A concrete family remains an ordered edge sequence. Compaction changes its
representation, not its identity or coverage.

## 3. Graph Model

### 3.1 Semantic state

An abstract state contains only verdict-relevant facts:

```text
(type_id,
 origin,
 selector_class,
 selector_continuity,
 boundary_state,
 terminal_semantic_edge)
```

`selector_class` is `none | possible | definite`. Exact `selector_id` is not
part of the state.

### 3.2 Transitions

Each transition references one schema edge and applies one semantic effect:

- `pass_through`: preserve active selector provenance.
- `activate_selector`: replace provenance with one selector generated here.
- `type_condition`: traverse interface/union expansion.
- `boundary_update`: update visibility/self-scope facts.
- `continuity_update`: preserve, weaken, or break ownership continuity.

An activation transition stores all exact selector generators that share the
same structural and semantic effect. Lazy expansion chooses one generator and
carries its exact `selector_id` onward.

Required-selector rule:

- If a field has a required selector, omit no-selector pass-through.
- If selectors are optional, emit pass-through and activation choices.

This preserves selector-family correlation without storing
`selector_id x path_family` in S3.

### 3.3 Cycles and the true DAG

The semantic state graph may contain cycles. S3 therefore:

1. Builds the complete semantic state graph.
2. Computes strongly connected components.
3. Condenses them into an acyclic component graph.
4. Stores internal SCC transitions separately as cycle templates.

The serialized top-level graph is a DAG. Lazy expansion applies the existing
edge-simple rule inside each component so expansion remains finite and records
cycle evidence explicitly. Only pre-target cycles are included; target states
are accepting and have no outgoing transitions.

## 4. S3/v3 Contract

```json
{
  "contract_version": "graphql-static-bac/s3-routes/3.0",
  "targets": [
    {
      "target_type_id": "type:Watchlist",
      "dag_id": "dag:sha256:...",
      "entry_component_ids": ["cmp:..."],
      "components": [],
      "transitions": [],
      "terminals": [],
      "selector_facts": [],
      "canonical_witnesses": [],
      "family_cardinality": {
        "status": "not_materialized",
        "lower_bound": 0
      }
    }
  ]
}
```

### 4.1 Components

Each component stores:

- stable `component_id`;
- member semantic states;
- internal transitions;
- cycle templates.

### 4.2 Terminals

Each terminal records:

- terminal state and component;
- `open | unknown | guarded | structurally_infeasible`;
- origin and boundary families;
- selector class and terminal semantic edge;
- one canonical witness for display and regression tests.

The canonical witness is not the completeness representation.

### 4.3 Stable IDs

- `state_id`: hash of canonical abstract semantic state.
- `transition_id`: hash of source, schema edge, mode, and target.
- `selector_id`: existing exact selector occurrence identity.
- `component_id`: hash of sorted member state IDs.
- `dag_id`: hash of target, policy fingerprint, components, and transitions.
- `terminal_id`: hash of terminal semantic state.

Concrete IDs are produced only during lazy expansion:

- `path_family_id`: hash of the ordered schema edge sequence.
- `route_instance_id`: hash of target, selector ID, family ID, and terminal.

Canonical JSON is compact, key-sorted, and array order is explicitly defined.

## 5. Lazy Expansion API

Add a shared `RouteFamilyExpander` used by diagnostics, S4, and runtime:

```text
expand(target_dag, filter, budget, cursor)
    -> iterator<RouteInstance>
    -> ExpansionStatus { complete | budget_exhausted, next_cursor }
```

Properties:

- deterministic depth-first order over sorted transitions;
- carries exact active selector provenance;
- never forms a global selector/family Cartesian product;
- enforces edge-simple traversal inside SCCs;
- filters by verdict, origin, terminal, and selector class;
- supports resumable cursors;
- reports truncation explicitly.

Exact expansion remains proportional to emitted families. That cost is
unavoidable, but it moves to the consumer that needs them and can stream
without retaining all routes in memory.

## 6. S4 and Runtime Migration

S4 consumes a stream of concrete `RouteInstance` values:

```text
DAG -> expand one family -> plan seed -> emit/cache -> discard family
```

Seed requirements depend on the ordered concrete path, so S4 must not plan
from a terminal alone.

Migration rules:

- S4 may filter to `open` and `unknown` before expansion.
- Existing execution dedup remains keyed by canonical operation and bindings.
- Runtime can stop, resume, or bound work using the expansion cursor.
- No adapter may collapse distinct families because they share a terminal or
  verdict.

## 7. Implementation Phases

### Phase 0: Isolate the exact-family prototype

- Preserve it as a diagnostic/reference implementation.
- Stop using it as the production S3 execution path.
- Keep small-fixture comparison tests against it.

### Phase 1: Build the abstract semantic graph

- Reuse the corrected automaton from `stage-3-fix-abstract-automaton.md`.
- Reuse reverse target slicing.
- Run selector-class abstract propagation.
- Retain every valid transition, not only one predecessor witness.
- Store exact selector generators on activation transitions.
- Mark entry states and terminal states.
- Assert that terminal target states have no outgoing transitions.

### Phase 2: Condense cycles

- Implement deterministic SCC discovery.
- Build the component DAG.
- Serialize internal SCC transitions and cycle templates.
- Verify condensation never removes an entry-to-terminal alternative.

### Phase 3: Add the S3/v3 contract

- Add contract structs and canonical ordering.
- Implement stable IDs and policy fingerprinting.
- Emit canonical witnesses independently from family expansion.
- Update CLI validation and artifact readers.

### Phase 4: Implement lazy expansion

- Add deterministic traversal and exact provenance carrying.
- Compute concrete family and route IDs.
- Add budget, cursor, and explicit expansion status.
- Add JSONL output so large expansions can stream to disk.

### Phase 5: Migrate S4

- Replace `Vec<Route>` input with the expander iterator.
- Plan and emit one route at a time.
- Preserve route-local correlation and execution-level dedup.
- Reject legacy flat input unless compatibility mode is explicit.

### Phase 6: Calibrate and benchmark

- Compare small fixtures with the exact-family reference.
- Run full-schema `Watchlist` DAG construction.
- Measure first-route latency, first 100 routes, bounded expansion, memory, and
  serialized DAG size.

## 8. Required Tests

### Structural coverage

- `B/C/D -> A -> sink`: one shared suffix, three expanded families.
- Divergence followed by reconvergence preserves every family.
- Interface and union type-condition branches remain distinct.
- Global-ID `node` and `nodes` origins remain distinct.

### Selector correlation

- A selector introduced only on branch B never appears on C or D.
- Every selector-expanded witness contains its activation transition.
- Required downstream selectors replace prior provenance.
- Optional selectors produce pass-through and activated alternatives.
- Ten thousand equivalent selectors grow generator data, not semantic states.
- Unrelated selectors do not enter the target DAG.

### Cycles

- The serialized component graph is acyclic.
- Internal cycle edges remain represented.
- No target component has an outgoing transition.
- Edge-simple expansion terminates deterministically.
- Cycle templates retain recursive evidence without unbounded unrolling.

### Determinism

- Reordered input maps produce byte-identical S3/v3 output.
- Repeated expansion produces identical route order and IDs.
- Resume from a cursor produces the same suffix as uninterrupted expansion.

### Downstream behavior

- S4 receives complete ordered edges for each emitted route.
- Seed requirements from different families do not leak across routes.
- `open` and `unknown` filtering occurs before expensive expansion.
- `budget_exhausted` is propagated and never reported as complete.

## 9. Acceptance Gates

- Full-schema S3 DAG construction for `Watchlist` completes in release mode
  within 10 seconds and under 512 MiB peak RSS on the current workstation.
- DAG size scales with reachable states/transitions, not family count.
- Small-fixture expansion exactly matches the reference enumerator.
- The B/C/D regression proves no family is lost through compaction.
- Selector provenance remains exact after lazy expansion.
- S4 streams the first 100 `open|unknown` route instances without
  materializing all route families.

## 10. Non-Goals

- Reducing the logical number of route families.
- Grouping families by assumed backend-equivalent behavior.
- Runtime bug classification.
- Silently truncating large expansions.
- Treating one canonical witness as proof that all families were explored.
