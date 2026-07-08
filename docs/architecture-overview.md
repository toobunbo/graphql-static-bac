# GraphQL Static BAC — Architecture Overview

## Purpose

This framework performs **static Broken Access Control (BAC) analysis** on GraphQL APIs.
It does not confirm bugs — it identifies candidate routes that *may* expose unauthorized
object access, then ranks and filters them by semantic evidence. Exploitability requires
a live runtime execution phase that is currently being rebuilt.

---

## High-Level Pipeline

```
Schema (SDL / Introspection JSON)
          │
          ▼
     ┌─────────┐
     │   S0    │  Schema IR — normalize, fingerprint
     └────┬────┘
          │ schema_ir.json
          ├────────────────────────┐
          ▼                        ▼
     ┌─────────┐            ┌─────────┐
     │   S1    │            │   S2    │  Argument Classifier
     │  Sinks  │            └────┬────┘
     └────┬────┘                 │ args.json
          │ sinks.json           │
          └──────────┬───────────┘
                     │
                     ▼
               ┌──────────┐
               │    S3    │  Route Analysis  ← primary static output
               └────┬─────┘
                    │ routes.json (v2.2 flat OR v3 DAG)
                    ▼
               ┌──────────┐
               │    S4    │  Seed Planning — generates GraphQL queries
               └────┬─────┘
                    │ seed-plans.json
                    ▼
            ┌──────────────┐
            │   Runtime    │  Execute queries, harvest seed values  ← REBUILD TARGET
            └──────┬───────┘
                   │ seed-runtime.json
                   ▼
            ┌──────────────┐
            │Policy Oracle │  Compare victim vs attacker response → verdict
            └──────────────┘
```

---

## Stage Descriptions

### S0 — Schema IR

Parses SDL or introspection JSON into a normalized internal representation.
Computes a stable `schema_fingerprint` (SHA-256 of raw input bytes) used to
cross-validate all downstream artifacts.

Output contains: all types (Object, Interface, Union, Enum, Scalar, InputObject),
fields, arguments with their TypeRefs, enum values, implemented interfaces, schema roots.

### S1 — Sinks (Sensitive Data Classification)

**Not** produced by this codebase — provided externally by a sensitive-data scanner
that inspects the schema for PII / privileged types and fields.

Key fields consumed by S3:
- `selected_types[].type_id` — the GraphQL type that is the analysis target
- `selected_types[].sink_ref_ids` — references to specific sensitive fields within the type
- `sink_refs[].field_id` — optional: a single field within the type (null = entire type)

Additional metadata (not currently consumed by S3/S4, relevant to Policy Oracle):
`tiers`, `impact`, `categories`, `evidence[]`, `addressability`, `profile`.

S1 answers: *"what are we looking for?"*

### S2 — Argument Classifier

For every argument on every field in the schema, classifies it as:

| Class | Meaning |
|---|---|
| `object_selector` | argument selects the specific object instance (e.g. `id: ID!`, `slug: String!`) |
| `possible_selector` | ambiguous — might select an object or might be a filter |
| `noise` | not a selector (pagination, sorting, flags, etc.) |

Also computes `confidence` (high / medium / low) and records `signals` (which lexicon
rules fired). Only `object_selector` with confidence ≥ medium feeds into S3 as a
**definite selector** (Deliverable A removed `possible_selector` from the pipeline).

S2 answers: *"which arguments identify a specific object?"*

### S3 — Route Analysis

Core static analysis stage. Finds all paths from the GraphQL query root to each
target type, classifies each path's security verdict, and records selector provenance.

**Abstract automaton**: runs a BFS-based fixed-point over an abstract state space
`(type_id, origin_mode, selector_class, selector_continuity, boundary_bits,
terminal_semantic_edge_id)`. The state space is bounded and independent of selector
count (Deliverable A guarantee).

**Verdict classification**:
- `open` — target is reachable with a definite selector and `selector_continuity = same`
  (the selector argument directly selects the target object, no intermediate ownership break)
- `unknown` — target is reachable but selector continuity is broken or absent
  (may still be exploitable; requires runtime confirmation)
- `guarded` — path crosses a visibility or self-scope boundary (requires auth context
  to be the owning user; lower exploitability without auth context switching)

**Output formats**:
- `v2.2` (current default): flat `routes[]` list, one canonical witness per
  `(terminal_abstract_state, provenance_token)` pair. Coverage label:
  `canonical_per_provenance`.
- `v3 DAG` (new, Deliverable B): SCC-condensed abstract state graph. No flat routes.
  States, transitions, selector sets, components, terminals stored once. Lazy expansion
  via `expand_routes()` produces all structural families on demand.

**Key invariants**:
- Target is absorbing: no transitions out of target states.
- Selector provenance is coherent: a selector only appears on witnesses that contain
  its activation transition.
- No `PossibleSelector` in any S3 output (Deliverable A).

S3 answers: *"how can the target be reached, and what is the access control verdict?"*

### S4 — Seed Planning

For each route from S3, computes a **seed plan**: the set of GraphQL queries that,
when executed, will yield values to bind the route's required arguments.

**Requirements**: collected from `route.witness.edges` — every required argument on
every field in the witness path, plus the route's selector argument.

**Producer candidates**: fields in the schema that return a value compatible with
the requirement type (exact match, interface compatibility, related concrete field).

**Strategies**:
- `standalone` — one producer query, one requirement covered
- `joint_co_read` — single query covers multiple correlated requirements
  (e.g. fetch `profile.id` AND `profile.deck.slug` in the same query to satisfy
  a correlation constraint requiring both to come from the same Profile instance)

**Correlation constraints**: when a route has multiple requirements that must be
satisfied by values from the *same object instance* (e.g. `node(id:)` + `deck(slug:)`
where slug must belong to the profile selected by id), a correlation constraint
is recorded and discharged by a joint co-read job.

**Output**: per-route `binding_set_plans[]` — each plan is an ordered list of
producer job IDs plus a dependency DAG. A plan is `executable` if all requirements
are resolved and all jobs have emitted GraphQL operation strings.

S4 answers: *"what queries do I run to get the seed values needed to test this route?"*

### Runtime (current, to be rebuilt)

The existing runtime (`seed-runtime/v1`) is a Rust implementation that:

1. Selects routes by verdict filter (`open`, `unknown`, `guarded`)
2. For each route, iterates its `binding_set_plans` in priority order
3. Executes each producer job's GraphQL operation against the target endpoint
4. Extracts values from responses using `extraction.members[].response_path`
5. Applies adapter chains (`identity`, `global_id_to_payload`, etc.) to transform
   raw response values into argument-compatible values
6. Executes the route validation query with bound arguments
7. Checks whether the response reaches the target type (`__typename` matching)
8. Records `verified_binding_sets` for confirmed reachability

**Known limitations driving rebuild**:
- Adapter chain is static (hardcoded adapter types); cannot handle novel value shapes
- No auth context switching (tests only one auth context per execution)
- No differential comparison between attacker/victim contexts (required for BAC confirmation)
- Extraction path resolution is brittle for complex nested/list responses
- No retry / rate-limit handling
- Coverage metadata is coarse; no per-route budget accounting tied to DAG expansion

### Policy Oracle

Post-runtime stage. Takes seed-runtime results (verified binding sets) and re-executes
routes under a *different* auth context (victim) to detect whether the attacker's seed
values expose data that should be restricted.

**Not rebuilt yet.** Depends on runtime producing clean per-route binding sets with
clear provenance so the oracle can compare responses.

---

## Code Layout (src/)

```
src/
  contracts/      ← all artifact schemas (S0-S4, runtime, oracle)
    s0.rs         schema IR types
    s1.rs         sinks/selected types
    s2.rs         argument classifications
    s3.rs         path enumeration (legacy)
    s3_routes.rs  route contract v2.2 types
    s3_dag.rs     route contract v3 DAG types  ← new (Deliverable B)
    s4_seed.rs    seed plan types
    seed_runtime.rs runtime result types
  graph/          ← TypeGraph: adjacency, reachability, plumbing detection
  route/
    abstract_automaton.rs  ← core BFS automaton, phase A/B propagation
    dag.rs                 ← SCC condensation, RouteFamilyDag  ← new
    worklist.rs            ← canonical traces + expand_routes()  ← new
    facts.rs               ← RouteFacts: selector/boundary/plumbing indexes
    families.rs            ← reference DFS enumerator (diagnostic only)
  seed/
    planner.rs    ← plan_seed_routes, requirement collection, job building
    search.rs     ← BFS producer path search
    emitter.rs    ← GraphQL operation string emission
    correlation.rs ← joint co-read constraint derivation
    dependency.rs ← job dependency DAG
  runtime/
    engine.rs     ← run_seed_runtime (to be rebuilt)
    adapters.rs   ← value adapter chain
    extractor.rs  ← response path extraction
    transport.rs  ← HTTP transport trait
  stages/         ← CLI-facing runners for each stage
  cli/            ← argument parsing, command dispatch
```

---

## What the Runtime Rebuild Needs

The rebuilt runtime receives:

**Inputs** (all stable, well-tested):
- `schema_ir.json` (S0) — for query validation and typename resolution
- `routes.json` (S3 v2.2 or v3 DAG expanded) — routes with verdicts, witnesses, selectors
- `seed-plans.json` (S4) — per-route producer jobs with emitted GraphQL operation strings
- `request_profile` — HTTP endpoint, auth headers, request template

**What it must produce**:
- For each route: attempt its binding set plans in order
- Execute producer queries → extract seed values → bind arguments
- Execute route validation query under *attacker* context → check typename
- Execute same route under *victim* context → compare responses
- Emit structured result: verified binding sets, execution records, coverage metadata

**Design considerations for new runtime**:
- Auth context switching is first-class (attacker vs victim credentials)
- Differential response comparison is the BAC signal (not just typename check)
- Budget is keyed on route instances, not abstract plans
- Rate-limit and retry should be transparent to the route execution logic
- Extraction must handle list-of-lists, optional chains, interface/union type conditions
- Adapter chain should be extensible (not hardcoded enum)
- The v3 DAG lazy expansion (`expand_routes()`) can stream routes without materializing all
