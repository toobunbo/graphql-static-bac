# Stage 4 Seed Planning - Phase 1 Implementation Plan

**Status:** Implemented on June 13, 2026.

The implementation lives in `src/contracts/s4_seed.rs`, `src/seed/`,
`src/stages/s4_seed_plans/`, and `src/cli/commands/s4.rs`.

**Design authority:** `docs/design/seedFinderPhases.md`, with the correlation
constraint and producer-job architecture validated by
`tests/seed_correlation_demo.rs`.

This plan implements only static planning. It does not send HTTP requests,
harvest runtime values, validate seeds, or contain a security oracle.

## 1. Objective

For every S3 route, produce one or more deterministic, executable plans for
obtaining all argument values required to execute that route.

The planner consumes:

```text
S0/v1 schema_ir.json
S2/v1 args.json
S3/v2 routes.json
```

and emits:

```text
S4/v1 seed_plans.json
```

The command surface will be:

```text
graphql-static-bac stage s4 \
  --schema-ir schema_ir.json \
  --args args.json \
  --routes routes.json \
  --output seed_plans.json
```

S4 is the static seed planner. Phase 2 runtime harvesting remains a separate
subsystem and is not part of this implementation.

## 2. Locked architecture

### 2.1 Route-local planning

The unit of planning is a route-local binding set, not an isolated
requirement. Each route plan contains:

```text
requirements
correlation_constraints
producer_jobs
dependency_dag
binding_set_plans
```

Two routes may contain equivalent producer jobs. They retain route-local
plans, while stable job IDs allow Phase 2 to deduplicate executions.

### 2.2 Constraint and strategy are separate

A correlation constraint is a structural fact derived from the route witness:
two values must retain the same instance lineage.

A resolution strategy belongs to a producer job:

```text
standalone
joint_co_read
threaded_dependency
static_binding
```

Requirements are not permanently classified as independent, correlated, or
dependent. Producer search may produce multiple strategies, and plan selection
chooses an executable combination.

### 2.3 Correlation preservation

Joint extraction is anchored at the nearest shared response instance, not a
shared GraphQL type or Query root.

For:

```text
usersPaginated.nodes[i].footballUserProfile
  |- id
  `- decks.nodes[j].slug
```

the extractor descriptor must preserve `(i, j)` and produce:

```text
(profile[i].id, profile[i].decks[j].slug)
```

It must never independently flatten both projections and form a Cartesian
product.

### 2.4 Dependency graph

Dependency DAG nodes are producer jobs. An edge identifies the exact output
field of one job and required input argument of another:

```text
producer_job_A
  -- field:Type.id -> arg:Query.lookup.id -->
producer_job_B
```

Cycles make that strategy infeasible. They do not make the requirement
globally unresolved while another acyclic strategy exists.

## 3. Output contract

Add `src/contracts/s4_seed.rs` with the following top-level shape:

```text
SeedPlansData
  planning_model
  routes: BTreeMap<route_id, RouteSeedPlan>

RouteSeedPlan
  route_id
  target_type_id
  requirements
  correlation_constraints
  producer_jobs
  dependency_dag
  binding_set_plans
  unresolved_requirements
```

Required closed enums:

```text
RequirementSource =
  schema_required | route_selector | required_input_field

StaticBindingClass =
  schema_default | bounded_pagination | schema_enum_value |
  generated_boolean | unresolved_literal

ProducerDerivation =
  exact_leaf_match | related_interface_field |
  related_concrete_field | identity_compatible

ProducerStrategy =
  standalone | joint_co_read | threaded_dependency | static_binding

Cardinality = one | optional | many
PlanStatus = executable | unresolved
```

Every unsupported required input must appear in
`unresolved_requirements`; it must not disappear from the artifact.

## 4. Implemented milestones

### Milestone 0 - Contract and design synchronization

Implement:

- update `docs/design/seedFinderPhases.md` from the old
  requirement-centric candidate shape to route-local binding-set plans;
- define the S4/v1 Rust contract;
- add stable ID namespaces for requirement, constraint, job, dependency edge,
  and binding-set plan;
- add contract serialization and validation tests.

Stable IDs use compact canonical JSON with array order preserved and sorted map
keys:

```text
seed_req:sha256:...
seed_constraint:sha256:...
seed_job:sha256:...
seed_dependency:sha256:...
seed_binding_plan:sha256:...
```

Acceptance tests:

- identical logical input serializes to identical bytes and IDs;
- changing ordered witness edges changes a job ID;
- reordering set-like members does not change a constraint ID;
- malformed references and duplicate IDs are rejected.

### Milestone 1 - Requirement collection and static bindings

Add `src/seed/requirements.rs` and indexes resolving route edge IDs back to S0
field definitions.

For every route:

1. inspect every FIELD edge in witness order;
2. collect every `NON_NULL` argument without a default;
3. include the active optional selector used by the route;
4. recurse through required input-object fields;
5. deduplicate by consumer argument plus input path and normalized TypeRef.

Resolve non-producer bindings:

- schema default: omit;
- pagination: bounded configured value;
- enum: deterministic ordered alternatives;
- Boolean: configured candidate alternatives;
- unsupported scalar/input object: explicit `unresolved_literal`.

Pagination is the only optional argument added for runtime bounding when it was
not present in the route.

Acceptance tests:

- `Query.node(id) -> FootballUserSportProfile.deck(slug)` yields both `id` and
  optional route selector `slug`;
- unrelated optional arguments are not collected;
- required enum alternatives preserve schema order;
- required nested input fields retain their complete input path;
- unsupported required input objects are reported as unresolved.

### Milestone 2 - Producer derivation

Add `src/seed/producers.rs`.

Build producer alternatives from:

```text
selected type + consumer argument leaf + consumer TypeRef
```

Automatic producer evidence is limited to:

1. exact leaf and compatible type on selected type;
2. exact leaf on a related interface;
3. exact leaf on a related concrete implementation;
4. `ID <-> String` for identity-like names.

Pure type-only matching is emitted only as a non-executable suggestion, not an
automatic binding source in the MVP.

Acceptance tests:

- `anyCard(assetId)` derives `AnyCardInterface.assetId` and compatible concrete
  fields;
- `watchlist(id)` derives `Watchlist.id`;
- unrelated String fields are not auto-bound to `assetId`;
- interface and concrete producers remain distinct alternatives;
- candidate ordering and evidence are deterministic.

### Milestone 3 - Argument-aware producer path search

Add:

```text
src/seed/search.rs
src/seed/state.rs
src/seed/signature.rs
```

Reuse `TypeGraph`, plumbing detection, reverse reachability, and stable graph
edge IDs. Do not call semantic route verdict analysis.

Search targets the producer's parent composite type, then appends the exact
scalar/enum producer field as a terminal projection.

The worklist state carries argument information before compaction:

```text
type_id
entry_field_id
ordered_witness
static_bindings
unresolved_arg_refs
producer_field_id
producer_locus
```

Compaction signature includes exact unresolved refs and binding alternatives,
not only their counts.

Acceptance tests:

- Connection/Edge plumbing remains in the query witness;
- concrete producers add the required TYPE_CONDITION;
- interface producers do not add an unnecessary TYPE_CONDITION;
- a self-cycle such as `node(id) -> Node.id` remains unresolved;
- a longer self-contained producer is retained over an unresolved short path;
- converging graph branches remain deterministic and cycle-safe.

### Milestone 4 - Correlation constraints and joint co-read

Add:

```text
src/seed/correlation.rs
src/seed/joint.rs
```

First derive route correlation constraints from requirement positions and
selected-object lineage. Then search for a producer job that can co-read all
constraint members beneath one nearest shared instance anchor.

Joint jobs emit an extraction descriptor:

```text
anchor response path
anchor list dimensions
member relative paths
member cardinalities
fan-out rule
```

Acceptance tests:

- the profile/deck demo produces three correlated tuples, not six Cartesian
  pairs;
- one anchor with multiple decks fans out while repeating only that anchor's
  profile ID;
- null anchors and empty child lists are representable without corrupting
  sibling branches;
- a shared Query root alone is not accepted as an instance anchor;
- a correlation constraint remains unresolved when no valid joint or threaded
  strategy exists.

### Milestone 5 - Threaded dependencies and plan selection

Add:

```text
src/seed/dependency.rs
src/seed/select.rs
```

When a producer job itself has a required selector argument, search whether an
output from another route-local job can bind it. Materialize a typed job edge,
build the DAG, reject cyclic combinations, and topologically order each
executable plan.

Plan selection:

1. chooses producer job alternatives covering every requirement;
2. verifies every correlation constraint is discharged;
3. rejects cycles and missing input bindings;
4. permits Cartesian composition only across independent dimensions;
5. may emit multiple ranked executable plans;
6. records why every failed alternative was rejected.

MVP supports route-local dependency chains. Recursive sub-requirements created
only inside a producer remain explicitly unresolved.

Acceptance tests:

- a two-job `A output -> B input` chain has the correct execution order;
- a self-cycle is rejected while an acyclic alternative survives;
- a joint strategy and threaded fallback may coexist as alternatives;
- validation is defined over a complete binding-set plan, not one argument;
- unresolved leaf requirements remain visible in output.

### Milestone 6 - Query emission, CLI, and integration

Add:

```text
src/seed/emitter.rs
src/stages/s4_seed_plans/runner.rs
src/cli/commands/s4.rs
```

The emitter consumes a producer job plan and renders:

- operation name;
- ordered field selections;
- bounded pagination and enum literals;
- variables for threaded inputs;
- inline fragments for concrete producers;
- terminal projections;
- branch-aware extraction descriptors.

Every emitted operation must parse with `graphql-parser` before the artifact is
written.

The S4 runner validates:

- S0 and S2 are v1, S3 is v2, and all fingerprints match;
- every route witness edge resolves against S0;
- every selector and argument reference resolves against S0/S2;
- all selected jobs and dependency edges reference existing IDs;
- every executable binding-set plan covers all requirements and constraints.

Acceptance tests:

- CLI writes an S4/v1 complete artifact atomically;
- wrong stage/version/fingerprint inputs fail without replacing output;
- repeated runs produce byte-identical output;
- all emitted GraphQL operations parse;
- existing S0 and S3 tests remain unchanged.

## 5. Calibration fixtures

Use focused fixtures before the full schema:

1. `AnyCard.assetId`: one independent producer and interface/concrete locus.
2. `Watchlist.id`: self-scope/public producers plus the unresolved global-ID
   self-cycle.
3. `FootballUserSportProfile.id + Deck.slug`: joint co-read and branch
   correlation.
4. Synthetic threaded dependency: producer B requires the ID emitted by
   producer A.
5. Synthetic cycle: A requires B and B requires A.

After focused fixtures pass, run the planner over the existing Deck and
Watchlist S3 artifacts. Calibration checks completeness and determinism; it
does not claim runtime executability or seed validity.

## 6. Module layout

Implemented layout:

```text
src/contracts/s4_seed.rs
src/seed/
  mod.rs
  index.rs
  planner.rs
  requirements.rs
  producers.rs
  search.rs
  correlation.rs
  dependency.rs
  emitter.rs
  ids.rs
  validation.rs
src/stages/s4_seed_plans/
  mod.rs
  runner.rs
src/cli/commands/s4.rs
```

The obsolete `src/stages/s4_scoring/` placeholder has been removed.

## 7. Calibration results

Final static calibration:

| Target | S3 routes | Executable plans | Runtime | Peak RSS |
|---|---:|---:|---:|---:|
| Watchlist | 89 | 89 | 3.51 s | 60,472 KB |
| Deck | 867 | 867 | 14.59 s | 191,168 KB |

Both artifacts were reproduced byte-for-byte on a second run. These results
prove static plan coverage and determinism only; no endpoint was called.

## 8. Completion criteria

Phase 1 is complete when:

- every S3 route has a route-local seed plan or explicit unresolved reason;
- every route requirement is represented;
- producer derivations carry evidence and confidence;
- correlation constraints are discharged by joint or threaded strategies;
- dependency DAGs are acyclic and topologically ordered;
- emitted harvest operations parse;
- artifacts and stable IDs are deterministic;
- no runtime endpoint, token, extracted value, validation result, or security
  oracle appears in S4.
