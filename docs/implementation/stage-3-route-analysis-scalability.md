# Stage 3 Route Analysis Scalability Refactor

Status: Superseded by `stage-3-route-family-dag.md`.

The exact-family materialization proposed here exceeded the full-schema
benchmark budget. This document remains as design history and as the reference
behavior for small-fixture equivalence tests.

## 1. Objective

Refactor S3 so that structural graph traversal is independent of the number of
S2 selector facts.

The implementation must preserve the current route semantics:

- every distinct selector route remains visible;
- every distinct structural path family remains visible;
- `possible_selector` facts are not dropped;
- `open`, `unknown`, and `guarded` use the current verdict rules;
- required and optional selector branching remains unchanged;
- cycles, target re-entry, interfaces, unions, and plumbing remain supported;
- one deterministic witness is retained for each exact route signature.

The refactor changes how routes are computed, not what a route means.

## 2. Current Problem

The current product state contains the exact selector identity:

```text
(
  type_id,
  selector_class,
  selector_key,
  selector_continuity,
  boundary_bits,
  terminal_semantic_edge,
  origin
)
```

Every selector therefore creates a separate state that is propagated through
the target-reachable graph.

The full Sorare S2 artifact currently contains:

```text
6,104 classified argument records
876 object_selector records
3,579 possible_selector records
1,649 sole-noise records
```

S2 completes in about one second. S3 route analysis for Watchlist does not
complete in a reasonable calibration window because the same structural
suffix is traversed separately for many selector identities.

The hand-authored Watchlist calibration contained only five selectors, so it
did not expose this scaling problem.

## 3. Locked Semantic Rules

This refactor must preserve the rules in `routeVerdict.md`.

### Selector activation

When a FIELD has selector facts:

- create an activated branch for every exact selector occurrence;
- the new selector replaces earlier selector provenance;
- continuity resets to `same`;
- selector class is `definite` or `possible`;
- existing boundary bits remain set.

### Selector-free invocation

- no selectors on the FIELD: pass through;
- all selectors optional: retain an additional selector-free branch;
- any selector proven required: do not retain the selector-free branch.

### Continuity

- TYPE_CONDITION preserves continuity;
- structural Connection/Edge plumbing preserves continuity;
- an ordinary FIELD without a new selector changes active continuity to
  `unknown`;
- a later selector activation resets continuity to `same`.

### Verdict

```text
boundary present                              -> guarded
definite selector + continuity same           -> open
everything else reachable                     -> unknown
```

`query_unreachable` remains a target-level result.

## 4. New Architecture

S3 will run in two explicit phases.

```text
S0 graph + route facts
        |
        v
Phase A: target-specific abstract route automaton
        |
        v
Phase B: exact selector-provenance projection
        |
        v
Phase C: selector x path-family materialization
        |
        v
S3 routes + deterministic family witnesses
```

Exact selector identity must not be part of the Phase A worklist key.
Exact path-family identity must not be part of the Phase A worklist key either.

## 5. Phase A: Abstract Structural Automaton

### 5.1 Target slice

For one target:

1. Compute the reverse-reachable type set.
2. Retain only graph edges whose destination is in that set.
3. Ignore all S2 selectors on fields outside this target slice.

This immediately prevents unrelated schema arguments from affecting target
analysis.

### 5.2 Abstract state

Use:

```text
AbstractRouteState {
  type_id,
  selector_class,          // none | possible | definite
  selector_continuity,     // not_applicable | same | unknown
  boundary_bits,
  terminal_semantic_edge,
  origin_seed
}
```

The state deliberately does not contain:

```text
arg_ref
root_arg_ref
input_path
selector occurrence ID
```

Consequently, ten thousand selectors of the same class on one FIELD create one
abstract activation transition, not ten thousand graph states.

### 5.3 Abstract transitions

Each FIELD produces at most:

- one pass-through transition;
- one `possible` activation transition if the field has possible selectors;
- one `definite` activation transition if the field has definite selectors.

An activation transition stores a list of exact selector generators attached
to it, but the list does not participate in structural state identity.

TYPE_CONDITION produces one ordinary abstract transition.

### 5.4 Fixed point

Memoize exact `AbstractRouteState` values and keep the canonical witness
template for each state.

Continue processing target states so cycles and target re-entry can still
produce distinct terminal, boundary, or continuity states.

Phase A complexity is bounded by graph size and the finite semantic state
domain, not by the number of selector identities.

## 6. Phase B: Selector Provenance Projection

Phase B treats selector provenance as a reaching-definition problem over the
already-built abstract automaton.

### 6.1 Generator behavior

Every abstract activation transition has one or more exact selector
generators.

Activation:

```text
kill previous selector provenance
generate selectors attached to this transition
```

Pass-through:

```text
preserve active selector provenance
```

A later activation therefore supersedes an earlier selector exactly as the
current engine does.

### 6.2 Batched propagation

Intern selector generator groups and propagate their membership using bitsets
or interned sorted sets.

The worklist is keyed by abstract state. It must not enqueue a separate
structural state for every selector.

Selectors generated by the same activation transition share:

- the same structural prefix;
- the same suffix transfer;
- the same boundary and continuity behavior;
- the same witness template.

They are expanded into separate routes only when materializing target output.

### 6.3 Route materialization

At each reachable target abstract state:

- `selector_class == none`: emit the selector-free route;
- otherwise, expand every reaching exact selector generator;
- copy the exact S2 selector fact into `RouteSelector`;
- derive verdict from the abstract state;
- derive the exact route signature;
- materialize the canonical edge witness only for the final retained route.

Distinct selector identities remain distinct output routes. This phase does not
compact logically similar selectors or remove runtime test cases.

## 7. Phase C: Structural Path-Family Provenance

Abstract-state convergence must not erase structurally different ways to reach
the same state.

Example:

```text
B -> A -> Watchlist
C -> A -> Watchlist
D -> A -> Watchlist
```

If B, C, and D produce the same abstract semantic state at A, Phase A may merge
the state for computation. The output must still preserve three path families.

Losing C and D would not necessarily change the route verdict, but it would
lose runtime coverage and could miss resolver-specific behavior on those
entry/prefix paths.

### 7.1 Family provenance

Propagate structural family provenance separately from the abstract state:

```text
FamilyProvenance {
  entry_field_id,
  prefix_family_id,
  predecessor_family_id,
  transition_edge_id
}
```

Family provenance is stored as interned IDs or persistent DAG nodes. It must not
be copied into `AbstractRouteState` or used to split the Phase A worklist.

The analysis therefore has three independent dimensions:

```text
semantic state       -> finite abstract automaton
selector provenance  -> batched generator sets
family provenance    -> batched persistent prefix DAG
```

### 7.2 Minimum family distinction

Two routes are different families when their ordered canonical graph-edge
sequences differ.

This includes differences in:

- entry FIELD;
- intermediate FIELD;
- Connection/Edge plumbing FIELD;
- TYPE_CONDITION;
- divergence and reconvergence prefixes.

Therefore B/C/D, alternate Connection wrappers, and alternate interface
branches remain distinct executable families even when they produce the same
selector, terminal, continuity, boundary, and verdict.

An optional `semantic_family_id` may later group families for UI display, but
it must never participate in route elimination or runtime coverage selection.

### 7.3 Stable family ID

Define:

```text
path_family_id = family:sha256(
  target_type_id,
  ordered full edge IDs,
  cycle template IDs
)
```

The finite family model follows the legacy structural cycle rule: the same
schema edge may appear at most once in one concrete witness. Repeatable
recursive behavior is represented explicitly by cycle templates rather than
silently collapsed into another prefix.

### 7.4 Family materialization

At each target abstract state:

1. Enumerate reaching selector generators, or the selector-free value.
2. Enumerate reaching path-family IDs.
3. Materialize each valid `(selector, path_family)` pair.
4. Apply the existing exact route signature and verdict rules.
5. Keep one deterministic full witness per exact pair/signature.

The implementation must preserve correlation between selector provenance and
family provenance. It must not compute an unrestricted Cartesian product when
a selector generator is reachable only through some families.

Use a batched relation:

```text
selector_generator -> set<path_family_id>
```

or an equivalent shared representation.

### 7.5 Scope and unavoidable growth

This requirement preserves path families, not every infinitely repeatable
walk.

Cycles remain represented by finite fixed-point states and canonical family
templates. If the schema contains exponentially many genuinely distinct
edge-simple families, output can still be large. S3 must report that
cardinality; it must not silently keep only the shortest family.

## 8. Exact Selector Occurrence Identity

Full recursive S2 exposes a correctness issue independent of performance:
`arg_ref` alone is not globally unique for nested input occurrences.

The current full artifact has 169 per-field collision groups, for example:

```text
input_field:RangeInput.max
  filters.grades.max
  filters.lastFifteenSo5AverageScore.max
  filters.starterOddsBasisPointsRange.max
```

S3 must identify a selector occurrence by:

```text
(
  field_id,
  arg_ref,
  root_arg_ref,
  input_path
)
```

### 8.1 Stable occurrence ID

Add a stable `selector_id`:

```text
direct argument:
  selector_id = arg_ref

nested input occurrence:
  selector_id = selector:sha256(
    field_id,
    arg_ref,
    root_arg_ref,
    input_path
  )
```

Direct Watchlist selectors retain their existing IDs. Nested selectors become
unambiguous.

### 8.2 Contract migration

Publish S3 route contract `2.1`:

- add `selector_id` to `RouteSelector`;
- use `selector_id` in `RouteSignature`;
- add `path_family_id` to `Route` and `RouteSignature`;
- derive `route_id` from both `selector_id` and `path_family_id`;
- retain `arg_ref`, `root_arg_ref`, and `input_path` for query generation.

Update S4, seed runtime, policy oracle, fixtures, and readers to consume S3
`2.1`.

No compatibility shim may silently collapse two selector occurrences back to
the same `arg_ref`.

S3/v2.1 route IDs are intentionally migrated. Existing direct Watchlist
selectors preserve their semantic selector IDs, but their route IDs may change
because path-family identity is now explicit. Differential tests compare
normalized semantics and legacy-family inclusion, not raw v2.0 route IDs.

## 9. Witness Strategy

Do not copy full edge vectors while traversing.

Use:

- an arena of abstract predecessor nodes;
- transition records for FIELD and TYPE_CONDITION edges;
- generator records pointing to activation transitions;
- persistent path-family DAG nodes retained across convergence;
- suffix/predecessor templates shared by selectors in one generator group.

For each final `(selector, path_family, route signature)`, choose the witness by
the existing comparator:

1. fewer FIELD hops;
2. fewer total edges;
3. lexicographically smaller edge-ID sequence;
4. witness ID.

The witness must be a real ordered graph walk and must preserve all plumbing
and TYPE_CONDITION edges required for query generation.

## 10. Complexity Target

Let:

```text
V = target-slice types
E = target-slice edges
A = reachable abstract states
G = selector generator groups
F = structural path families
S = exact selector occurrences
R = emitted routes
```

Current behavior is approximately:

```text
O(S * A) structural states and repeated suffix traversal
```

Target behavior:

```text
Phase A: O(A + abstract transitions)
Phase B/C: O(batched selector-family propagation over abstract transitions)
Output:  O(R)
```

The cost of emitting genuinely distinct routes remains unavoidable. The
refactor removes repeated structural traversal; it does not hide output
cardinality.

## 11. Planned Module Layout

```text
src/route/
  facts.rs
  abstract_state.rs
  automaton.rs
  provenance.rs
  families.rs
  materialize.rs
  witness.rs
  signature.rs
  worklist.rs          # replaced after differential validation
```

Suggested responsibilities:

- `abstract_state.rs`: finite state and transition types.
- `automaton.rs`: target slice and Phase A fixed point.
- `provenance.rs`: generator interning and batched reaching definitions.
- `families.rs`: persistent structural family DAG and family hashing.
- `materialize.rs`: verdict, route signature, route expansion.
- `witness.rs`: predecessor arena and canonical witness selection.

Keep the current engine temporarily as `legacy_product.rs` for differential
tests. Remove it from production only after equivalence gates pass.

## 12. Implementation Phases

### Phase 0: Freeze Semantics and Baselines

- Capture current synthetic Watchlist and UserDevice route outputs.
- Pin the current verdict, continuity, boundary, origin, and witness rules.
- Add B/C/D convergence fixtures proving all path families survive.
- Add selector-occurrence collision tests.
- Record the full-S2 Watchlist timeout baseline.

### Phase 1: Build Abstract Automaton

- Add target-slice indexing.
- Implement `AbstractRouteState`.
- Group selector activations by selector class.
- Preserve required/optional selector-free branching.
- Add abstract-state and transition counters.

### Phase 2: Add Provenance Projection

- Intern exact selector generators.
- Implement kill/gen and pass-through propagation.
- Preserve selector replacement by later fields.

### Phase 3: Add Path-Family Projection

- Build persistent path-family DAG nodes.
- Preserve B/C/D-style families through state convergence.
- Maintain the correlated selector-to-family relation.
- Expand exact selector/family pairs only at target states.

### Phase 4: Witness and Contract Migration

- Add shared predecessor/witness arena.
- Add stable selector occurrence IDs.
- Add stable path-family IDs to route output.
- Publish and migrate S3/v2.1.
- Emit an explicit v2.0-to-v2.1 route migration report for calibration.

### Phase 5: Differential Validation

- Run legacy and refactored engines on compact schemas.
- Compare normalized route sets and exact witnesses.
- Replace the production `analyze_target` implementation only after equality.

### Phase 6: Full-Schema Calibration

- Run generated S2 against Watchlist.
- Confirm the old 89-route calibration is a subset, not the expected total.
- Run UserDevice and Deck as independent anchors.
- Record routes, verdict counts, abstract states, generator groups, runtime,
  peak RSS, and output size.

## 13. Test Plan

### 13.1 Semantic Equivalence

- direct required selector;
- optional selector plus selector-free route;
- possible selector never becomes open;
- low-confidence object selector remains possible;
- ordinary FIELD changes continuity to unknown;
- later selector resets continuity to same;
- boundary before and after selector remains guarded;
- TYPE_CONDITION and plumbing preserve continuity;
- global `node` and `nodes` origin remains correct;
- target re-entry and graph cycles terminate;
- required downstream selector kills earlier provenance;
- optional downstream selector permits both old and new provenance routes.

### 13.2 Selector Identity

- the same input field used at two input paths produces two selector IDs;
- the same input type used by two root arguments produces two selector IDs;
- direct argument selector IDs remain equal to their existing `arg_ref`;
- route IDs are deterministic under reordered S0/S2 maps;
- S4 resolves each nested selector using its exact root and input path.

### 13.3 Path Families

- `B -> A -> Watchlist`, `C -> A -> Watchlist`, and
  `D -> A -> Watchlist` emit three families after convergence at A;
- same entry with two distinct semantic prefixes emits two families;
- different TYPE_CONDITION branches emit distinct executable families;
- wrapper/plumbing edge variation emits distinct executable families;
- selector A reachable only through family X is never paired with family Y;
- cycle templates terminate and do not create infinitely many family IDs;
- repeated execution produces identical family IDs and witnesses.

### 13.4 Scalability

- 10,000 unrelated selectors outside the target slice do not change abstract
  state or transition counts.
- 10,000 selectors on one reachable FIELD do not change Phase A state count.
- Selector growth is linear in projection/output, not structural traversal.
- Family growth affects provenance/output, not Phase A state count.
- Repeated execution produces byte-identical output and identical statistics.

### 13.5 Regression Anchors

Hand-authored Watchlist baseline:

```text
89 routes
3 open
12 unknown
74 guarded
```

These routes must all remain present with the same verdict semantics.

Full generated S2:

- `node`, `nodes`, and `market.watchlist(id:)` remain open;
- all old route signatures are a subset of the new result;
- new selector routes are retained rather than grouped away;
- no count is expected to remain exactly 89.

UserDevice:

- global-ID routes remain open;
- current-user routes remain guarded/self-scope.

## 14. Benchmark Gates

Do not assert wall-clock limits in ordinary unit tests. Use deterministic state
counts for CI and a separate release-mode calibration report.

Initial full-schema acceptance gate on the current workstation:

```text
Watchlist + generated S2:
  completes without timeout
  release runtime <= 30 seconds
  peak RSS <= 1 GiB
```

Engineering target after profiling:

```text
release runtime < 10 seconds
peak RSS < 512 MiB
```

If output cardinality alone exceeds these bounds, report it separately from
Phase A traversal cost. Do not truncate routes silently.

## 15. Acceptance Criteria

The refactor is complete when:

1. No Phase A state or worklist key contains exact selector identity.
2. No Phase A state or worklist key contains exact path-family identity.
3. S3 consumes the full generated S2 artifact without hanging.
4. B/C/D-style convergence retains all structural families.
5. The old Watchlist route set is a subset of the full result.
6. Existing verdict, boundary, continuity, and witness semantics are preserved.
7. Nested selector occurrences cannot collide during deduplication.
8. S4 and runtime consumers accept S3/v2.1.
9. Full-schema output is deterministic.
10. Scalability tests demonstrate structural state counts independent of
   selector cardinality.
11. Scalability tests demonstrate Phase A state counts independent of family
    cardinality.
12. Full-S2 Watchlist, UserDevice, and Deck calibration reports are recorded.

## 16. Non-Goals

This refactor will not:

- drop or demote `possible_selector` facts to gain speed;
- group selectors merely because their current route logic looks similar;
- collapse distinct B/C/D-style structural families into one witness;
- change the definitions of open, unknown, or guarded;
- infer resolver or backend authorization semantics;
- add Mutation or Subscription roots;
- generate seeds or run runtime requests;
- truncate output to satisfy a benchmark.
