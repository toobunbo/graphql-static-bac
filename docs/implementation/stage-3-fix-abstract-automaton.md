# Stage 3 Fix: Abstract Automaton

Status: Implemented foundation; selector policy superseded by
`stage-3-definite-selectors-and-route-family-dag.md`.

Next phase: `stage-3-route-family-dag.md`

## 1. Objective

Replace exact structural DFS and selector-identity state propagation with a
finite abstract automaton.

This phase must:

- terminate on the full schema and full S2 output;
- preserve verdict semantics;
- emit only selector/witness combinations that are structurally coherent;
- provide the complete state-transition graph needed by the route-family DAG;
- stop treating canonical witnesses as a complete list of path families.

Route-family expansion remains a later phase.

## 2. Confirmed Root Causes

### 2.1 Structural DFS explosion

`enumerate_structural_families()` uses edge-simple DFS. The current traversal
continues after first reaching the target, so it explores:

```text
Query -> ... -> Watchlist -> ... -> Watchlist
```

On the full schema, this creates long post-target walks through a highly
connected graph. A Watchlist simulation reached millions of iterations and
hundreds of edges in one path while producing almost no useful families.

### 2.2 Selector identity explosion

The current semantic state contains an exact selector occurrence:

```rust
selector_key: Option<(String, Vec<String>)>
```

Every selector therefore multiplies worklist states. With thousands of S2
selectors, state growth depends on selector count instead of semantic classes.

### 2.3 Selector/witness correlation bug in the previous plan

The previous proposal stored:

```text
(type, abstract state) -> selector ID set + one predecessor
```

That is unsound. If selector X arrives through branch B but the canonical
predecessor comes from branch C, materialization can produce:

```text
witness:  C -> A -> Watchlist
selector: X, which only exists on B
```

Correct route counts would not detect this false route.

## 3. Target Semantics

The first arrival at the requested target is an accepting terminal.

**S3 must not expand outgoing edges from a target state.**

For target `Watchlist`:

```text
Query -> ... -> Watchlist
```

is a complete acquisition route. Any suffix after that point describes
navigation away from an already acquired Watchlist, not another way to acquire
the target.

Consequences:

- target re-entry is removed from route discovery;
- post-target cycles cannot cause traversal explosion;
- cycles before first target arrival remain represented;
- SCC/cycle-template handling in the DAG phase still applies to pre-target
  graph regions;
- analysis of a field below Watchlist should use that field's return type as a
  separate target rather than extending a completed Watchlist route.

This rule directly fixes the observed DFS behavior. The abstract fixed-point
also terminates independently because it has a finite state space.

## 4. Architecture

### 4.1 State key

The automaton is keyed by `(type_id, AbstractState)`:

```rust
pub(crate) struct AbstractState {
    pub origin_mode: OriginMode,
    pub selector_class: SelectorClass,
    pub selector_continuity: SelectorContinuity,
    pub boundary_bits: u8,
    pub terminal_semantic_edge_id: Option<String>,
}
```

Exact selector IDs are excluded.

`OriginMode` is finite:

```rust
enum OriginMode {
    Traversal,
    GlobalIdPrefix,
}
```

`GlobalIdPrefix` is retained only while the witness still has the exact direct
global-ID shape. Any subsequent semantic field downgrades it to `Traversal`.
A terminal reached directly through the global-ID field and type condition is
materialized as `origin: global_id`.

Including origin mode prevents traversal and direct global-ID paths from
merging before terminal classification.

### 4.2 Field facts

Precompute facts once:

```rust
struct FieldAbstractFact {
    has_possible: bool,
    has_definite: bool,
    has_required: bool,
    possible_selector_ids: Vec<String>,
    definite_selector_ids: Vec<String>,
}
```

Definitions are exact:

- `has_possible` means class exactly `Possible`;
- `has_definite` means class exactly `Definite`;
- `has_required` means at least one selector occurrence on the field is
  required, and suppresses no-selector pass-through.

### 4.3 Recorded transitions

Phase A retains the complete abstract transition relation:

```rust
struct RecordedTransition {
    transition_id: String,
    source: StateRef,
    target: StateRef,
    edge_index: usize,
    effect: TransitionEffect,
    selector_generator_ids: Vec<String>,
}

enum TransitionEffect {
    PassThrough,
    ActivatePossible,
    ActivateDefinite,
    TypeCondition,
}
```

Activation transitions carry exact selector generators, but generator IDs do
not alter the state graph.

### 4.4 Provenance token

Phase B uses compact provenance tokens:

```rust
enum ProvenanceToken {
    NoSelector,
    ActivationTransition(String),
}
```

The token identifies the exact transition where the currently active selector
was activated. It does not identify one of thousands of equivalent selector
occurrences.

Rules:

- pass-through and type-condition transitions preserve the token;
- activation replaces the token with that transition's ID;
- required selectors remove the no-selector alternative;
- terminal materialization uses only paths carrying the matching token.

All selector IDs generated by one activation transition share a valid route
template. This keeps the automaton compact while preserving selector/witness
correlation.

## 5. Propagation

The caller invokes:

```rust
AbstractAutomaton::propagate(graph, target_type_id, facts)
```

Internally it performs two monotone passes.

### 5.1 Phase A: discover states and transitions

1. Validate that the target is composite.
2. Compute reverse reachability to the target.
3. Initialize the Query root with the empty abstract state.
4. Pop `(type, state)` from the worklist.
5. If `type == target`, mark it terminal and do not expand it.
6. Otherwise, apply every eligible graph edge.
7. Record every valid abstract transition.
8. Enqueue only newly discovered `(type, state)` pairs.

No exact selector identity participates in this pass.

### 5.2 Phase B: propagate provenance templates

1. Seed Query root with `NoSelector`.
2. Propagate provenance tokens over Phase A transitions.
3. Activation transitions replace the current token.
4. Re-enqueue a state when its token set grows.
5. Retain predecessor information per `(state, provenance token)`, not one
   predecessor for the entire state.

The number of provenance tokens is bounded by activation transitions in the
abstract graph, not by the number of selector IDs.

### 5.3 Coherent canonical witness

For each terminal `(state, provenance token)`:

1. Reconstruct the predecessor chain for that exact token.
2. Verify every edge is connected and ends at the requested target.
3. For `ActivationTransition(id)`, verify the transition occurs in the witness
   and no later activation overwrites it.
4. Materialize one route for each selector generator on that transition.
5. For `NoSelector`, verify no activation occurs in the witness.
6. Recompute boundaries and origin from witness edges and assert they match the
   terminal abstract state.

This emits a canonical representative, not every structural family.

## 6. Output Contract

The previous plan incorrectly kept S3/v2.1 unchanged. That would imply complete
family coverage while emitting only canonical witnesses.

This fix introduces an explicit compatibility contract:

```text
graphql-static-bac/s3-routes/2.2
analysis_model = "abstract_automaton_canonical"
coverage = "canonical_per_provenance"
```

Properties:

- each emitted route is structurally valid;
- each selector appears on its own witness;
- `path_family_id` hashes that canonical witness only;
- output does not claim to enumerate all path families;
- S4 may use these routes for calibration and canonical execution;
- complete family coverage begins with S3/v3 route-family DAG expansion.

Do not compare v2.2 and v2.1 solely by route count.

## 7. Files

```text
src/route/
  abstract_automaton.rs   new: abstract graph and two-pass propagation
  worklist.rs             use automaton and canonical adapter
  facts.rs                selector lookup by ID and precomputed field facts
  mod.rs                  module exports
  families.rs             retained only as a small-fixture reference

src/contracts/
  s3_routes.rs            add explicit coverage metadata for v2.2
```

The automaton structs should be designed for direct reuse by the S3/v3 DAG
builder. Do not create a second graph representation in the DAG phase.

## 8. Implementation Steps

### Step 1: Freeze regressions

- Add target-absorbing fixture.
- Add selector/witness cross-branch fixture.
- Record the three known Watchlist OPEN selectors and witnesses.
- Add a full-S2 timeout benchmark.

### Step 2: Build abstract facts and states

- Add `OriginMode`.
- Add `AbstractState`.
- Add exact `FieldAbstractFact`.
- Add stable transition IDs.

### Step 3: Implement Phase A

- Discover finite state pairs.
- Record every transition.
- Stop expansion at first target arrival.
- Preserve interface/union type conditions and pre-target cycles.

### Step 4: Implement Phase B

- Propagate `ProvenanceToken`.
- Store predecessor per token.
- Replace token on selector activation.
- Never union selector IDs into a state-level predecessor.

### Step 5: Add coherent materialization

- Reconstruct token-specific witnesses.
- Expand transition generators into exact selectors.
- Assert selector, boundary, origin, and terminal consistency.
- Emit canonical coverage metadata.

### Step 6: Remove production DFS dependency

- Stop calling `enumerate_structural_families()` from production S3.
- Keep it available only for bounded fixtures and equivalence tests.

### Step 7: Validate and benchmark

- Run unit and integration tests.
- Run Watchlist with calibration S2.
- Run Watchlist with full S2.
- Compare semantic signatures, not only counts.
- Capture elapsed time, state count, transition count, and peak RSS.

## 9. Required Tests

### Target handling

- Reaching Watchlist records a terminal.
- No transition has a terminal Watchlist state as its source.
- `Watchlist -> X -> Watchlist` is not emitted as a second acquisition route.
- A cycle before Watchlist remains represented and terminates.

### Selector correlation

Fixture:

```text
B --selector X--> A -> Watchlist
C ----------------> A -> Watchlist
```

Assertions:

- selector X is emitted only with a witness containing branch B;
- branch C never receives selector X;
- route count alone is not used as the assertion.

### Selector semantics

- Required selector suppresses no-selector pass-through.
- Optional selector retains pass-through and activation.
- Later activation replaces earlier provenance.
- Possible and Definite generators remain separate.
- Adding 10,000 selectors to one activation transition does not increase the
  abstract state count.

### Origin and boundaries

- Direct `node` and `nodes` target routes are `global_id`.
- A route starting with `node` but continuing through another semantic field
  is downgraded to `traversal`.
- Materialized boundary families equal boundaries found in witness edges.
- Plumbing does not replace terminal semantic edge or break continuity.

### Watchlist anchors

The following OPEN routes must remain coherent:

- `Query.node(id:) -> ... on Watchlist`
- `Query.nodes(ids:) -> ... on Watchlist`
- `Query.market -> MarketRoot.watchlist(id:) -> Watchlist`

Every emitted selector must reference an argument edge present in its witness.

## 10. Acceptance Gates

- Full-S2 Watchlist analysis terminates within 10 seconds in release mode.
- Peak RSS remains below 512 MiB on the current workstation.
- Abstract state count is independent of exact selector count.
- No target state has outgoing transitions.
- No selector/witness correlation violations are found.
- The three Watchlist OPEN anchors remain present and correctly classified.
- Output explicitly reports canonical, incomplete family coverage.
- The automaton can be consumed directly by the later SCC/DAG builder.

## 11. Non-Goals

- Enumerating every structural path family.
- Preserving target re-entry as a separate acquisition route.
- Claiming that canonical witnesses provide complete route coverage.
- Implementing S4 lazy family expansion.
- Runtime bug classification.
