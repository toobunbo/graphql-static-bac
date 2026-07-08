# Static Pipeline — Input/Output Reference

All artifacts are JSON wrapped in a standard envelope:

```json
{
  "contract_version": "1.0",
  "stage": "S3",
  "schema_fingerprint": "sha256:<hex>",
  "scope": ["query"],
  "status": "complete",
  "producer": { "name": "graphql-static-bac", "version": "0.1.0" },
  "warnings": [],
  "data": { ... }
}
```

`schema_fingerprint` is cross-validated between all artifacts at every stage boundary.

---

## S0 — Schema IR

**File:** `schema_ir.json`  
**Envelope version:** `1.0`, stage `S0`

### `data` structure

```json
{
  "roots": { "query": "Query", "mutation": "Mutation", "subscription": null },
  "types": {
    "Query": {
      "type_id": "type:Query",
      "name": "Query",
      "kind": "OBJECT",
      "fields": {
        "event": {
          "field_id": "field:Query.event",
          "name": "event",
          "return_type": { "named_type": "Event", "named_kind": "OBJECT", "wrappers": [], "display": "Event" },
          "arguments": [
            {
              "arg_id": "arg:Query.event.id",
              "name": "id",
              "type_ref": { "named_type": "ID", "named_kind": "SCALAR", "wrappers": ["NON_NULL"], "display": "ID!" },
              "default_value": null
            }
          ]
        }
      },
      "input_fields": {},
      "enum_values": [],
      "interfaces": [],
      "possible_types": []
    }
  }
}
```

**Key types for the runtime:**
- `type_ref.wrappers`: ordered list of `NON_NULL`, `LIST` — determines whether a field is required and/or returns a list
- `field_id` / `arg_id`: stable string IDs used as keys throughout the pipeline
- `kind`: `OBJECT | INTERFACE | UNION | ENUM | SCALAR | INPUT_OBJECT`

---

## S2 — Argument Classifications

**File:** `args.json`  
**Envelope version:** `1.0`, stage `S2`

### `data` structure

```json
{
  "classifier_model": "argument-classifier-v1",
  "policy_fingerprint": "sha256:<hex>",
  "fields": {
    "field:Query.event": {
      "arguments": [
        {
          "arg_ref": "arg:Query.event.id",
          "root_arg_ref": "arg:Query.event.id",
          "arg_path": "Query.event.id",
          "input_path": [],
          "type_ref": { "named_type": "ID", "named_kind": "SCALAR", "wrappers": ["NON_NULL"], "display": "ID!" },
          "classifications": ["object_selector"],
          "signals": ["id_exact_name", "id_scalar_type"],
          "confidence": "high"
        },
        {
          "arg_ref": "arg:Query.events.filters.ids",
          "root_arg_ref": "arg:Query.events.filters",
          "arg_path": "Query.events.filters.ids",
          "input_path": ["ids"],
          "type_ref": { "named_type": "ID", "named_kind": "SCALAR", "wrappers": ["NON_NULL", "LIST", "NON_NULL"], "display": "[ID!]!" },
          "classifications": ["object_selector"],
          "signals": ["id_scalar_type"],
          "confidence": "high"
        }
      ]
    }
  }
}
```

**Key points:**
- `input_path`: empty for direct arguments; non-empty for nested input object fields (e.g. `filters.ids`)
- `classifications`: can include `object_selector`, `possible_selector`, `noise` — only `object_selector` with confidence ≥ `medium` feeds S3
- `root_arg_ref`: the top-level argument ID (even when `input_path` is non-empty)

---

## S3 — Routes (v2.2, current production format)

**File:** `routes.json`  
**Envelope version:** `2.2`, stage `S3`

### `data` structure

```json
{
  "analysis_model": "abstract-automaton-canonical-v1",
  "policy_fingerprint": "sha256:<hex>",
  "coverage": "canonical_per_provenance",
  "targets": {
    "type:Event": {
      "target_type_id": "type:Event",
      "sink_ref_ids": [],
      "reachability": "reachable",
      "best_verdict": "open",
      "routes": [ ... ]
    }
  }
}
```

### Route object

```json
{
  "route_id": "route:sha256:<hex>",
  "path_family_id": "family:sha256:<hex>",
  "target_type_id": "type:Event",
  "origin": "traversal",
  "verdict": "open",
  "selector": {
    "selector_id": "arg:Query.event.id",
    "arg_ref": "arg:Query.event.id",
    "root_arg_ref": "arg:Query.event.id",
    "arg_path": "Query.event.id",
    "input_path": [],
    "type_ref": { "named_type": "ID", "named_kind": "SCALAR", "wrappers": ["NON_NULL"], "display": "ID!" },
    "classification": "object_selector",
    "confidence": "high",
    "selected_type_id": "type:Event"
  },
  "selector_continuity": "same",
  "terminal_semantic_edge_id": "field:Query.event",
  "boundaries": [],
  "signature": {
    "origin": "traversal",
    "selector_id": "arg:Query.event.id",
    "path_family_id": "family:sha256:<hex>",
    "terminal_semantic_edge_id": "field:Query.event",
    "boundary_families": [],
    "selector_continuity": "same",
    "verdict": "open"
  },
  "witness": {
    "witness_id": "cap:sha256:<hex>",
    "entry_field_id": "field:Query.event",
    "field_hop_count": 1,
    "display_projection": "Query.event -> Event",
    "edges": [
      {
        "edge_id": "field:Query.event",
        "kind": "field",
        "source_type_id": "type:Query",
        "field_id": "field:Query.event",
        "target_type_id": "type:Event"
      }
    ],
    "cycle_templates": []
  }
}
```

### Verdict semantics

| Verdict | Meaning for runtime |
|---|---|
| `open` | Selector directly identifies the target. High exploitability signal. |
| `unknown` | Target reachable but selector continuity broken (intermediate traversal). Needs runtime confirmation. |
| `guarded` | Path crosses a self-scope or visibility boundary. Requires auth context switching to test. |

### Key route fields for runtime

- `witness.edges`: ordered concrete path from Query root to target type. Each edge has `field_id` (for field edges) or is a type condition (for interface/union branches).
- `selector`: the argument that selects the target object. `arg_ref` + `input_path` identify exactly which argument to bind.
- `terminal_semantic_edge_id`: the field that "owns" the target semantically — used to build the route validation query.
- `origin`: `global_id` (via `node(id:)` / `nodes(ids:)`) vs `traversal` (arbitrary path).

### Witness with type conditions (interface/union example)

```json
"edges": [
  { "edge_id": "field:Query.node",        "kind": "field",          "source_type_id": "type:Query",   "target_type_id": "type:Node" },
  { "edge_id": "type_condition:Watchlist", "kind": "type_condition", "source_type_id": "type:Node",    "target_type_id": "type:Watchlist", "field_id": null }
]
```

Type condition edges have `field_id: null` and map to `... on TypeName` fragments in the emitted query.

### Event-specific routes (schema2)

313 routes total:
- **6 open**: `Query.event(id:)`, `Query.events(ids:)`, `Query.events(slugs:)`, `Query.events(filters.ids:)`, `Query.events(filters.communityIds:)`, `Query.events(filters.slugs:)`
- **307 unknown**: traversal paths through Connection types, Document types, etc., reaching Event via indirect fields
- **0 guarded**: no self-scope or visibility boundaries in schema2 for Event

---

## S4 — Seed Plans

**File:** `seed-plans.json`  
**Envelope version:** `1.0`, stage `S4`

### `data` structure

```json
{
  "planning_model": "seed-planning-v1",
  "routes": {
    "route:sha256:<hex>": { ... RouteSeedPlan ... }
  }
}
```

### RouteSeedPlan

```json
{
  "route_id": "route:sha256:<hex>",
  "target_type_id": "type:Event",
  "portfolio_truncated": false,
  "requirements": [ ... ],
  "correlation_constraints": [ ... ],
  "producer_jobs": [ ... ],
  "dependency_dag": { ... },
  "binding_set_plans": [ ... ],
  "unresolved_requirements": [ ... ]
}
```

### SeedRequirement

One per argument that must be bound to execute the route:

```json
{
  "requirement_id": "seed_req:sha256:<hex>",
  "consumer_arg_ref": "arg:Query.event.id",
  "root_arg_ref": "arg:Query.event.id",
  "consumer_field_id": "field:Query.event",
  "input_path": [],
  "leaf_name": "id",
  "type_ref": { "named_type": "ID", "named_kind": "SCALAR", "wrappers": ["NON_NULL"], "display": "ID!" },
  "source": "route_selector",
  "selected_type_id": "type:Event",
  "static_bindings": [],
  "producer_candidates": [ ... ]
}
```

**`source` values:**
- `route_selector` — this argument IS the selector that identifies the target object
- `schema_required` — non-optional argument on the path (not a selector, e.g. pagination)
- `required_input_field` — required field inside an input object

**`static_bindings`**: pre-resolved values that don't need a producer query:
```json
{ "arg_ref": "arg:Query.events.first", "input_path": [], "class": "bounded_pagination", "value": "20" }
```

Static binding classes: `bounded_pagination`, `schema_enum_value`, `generated_boolean`, `schema_default`, `unresolved_literal`.

### ProducerJob

A GraphQL query that extracts seed values for one or more requirements:

```json
{
  "job_id": "seed_job:sha256:<hex>",
  "strategy": "standalone",
  "producer_priority": 2,
  "covers_requirements": ["seed_req:sha256:<hex>"],
  "producer_field_ids": ["field:Query.events"],
  "entry_field_id": "field:Query.events",
  "witness": {
    "edges": [ ... ],
    "terminal_field_ids": ["field:Event.id"]
  },
  "static_bindings": [],
  "unresolved_arg_refs": [],
  "extraction": {
    "anchor": null,
    "members": {
      "seed_req:sha256:<hex>": {
        "response_path": "events.nodes[].id",
        "relative_path": "events.nodes[].id",
        "cardinality": "many"
      }
    }
  },
  "operation_name": "Harvest_25bf1ff11104bb83",
  "operation": "query Harvest_25bf1ff11104bb83 {\n  events(first: 20) {\n    nodes {\n      id\n    }\n  }\n}",
  "executable": true,
  "rejection_reasons": []
}
```

**`executable: false`** means the job has unresolved arguments (e.g. requires a seed itself) or failed query emission.

**`strategy` values:**
- `standalone` — one producer job covers one or more independent requirements
- `joint_co_read` — single query fetches multiple correlated values from the same object instance

### Correlation constraints (joint co-read)

When a route requires two arguments that must come from the *same object instance*:

```json
{
  "constraint_id": "corr:sha256:<hex>",
  "members": ["seed_req:sha256:profile_id", "seed_req:sha256:deck_slug"],
  "anchor_type_id": "type:FootballUserSportProfile",
  "basis": {
    "kind": "route_lineage",
    "selector_requirement_id": "seed_req:sha256:profile_id",
    "dependent_requirement_id": "seed_req:sha256:deck_slug",
    "dependent_field_id": "field:FootballUserSportProfile.deck"
  },
  "discharged_by_job_ids": ["seed_job:sha256:<hex>"]
}
```

A joint co-read job's extraction plan has an **anchor** — the shared parent object from which both values are read:

```json
"extraction": {
  "anchor": {
    "type_id": "type:FootballUserSportProfile",
    "response_path": "usersPaginated.nodes[].footballUserProfile",
    "instance_rule": "nearest_shared_instance"
  },
  "members": {
    "seed_req:profile_id": { "response_path": "...", "relative_path": "id", "cardinality": "one" },
    "seed_req:deck_slug":  { "response_path": "...", "relative_path": "decks[].slug", "cardinality": "many" }
  }
}
```

### DependencyDag

Defines execution order when one producer job's output feeds another:

```json
{
  "nodes": ["seed_job:sha256:A", "seed_job:sha256:B"],
  "edges": [
    {
      "dependency_id": "dep:sha256:<hex>",
      "from_job_id": "seed_job:sha256:A",
      "to_job_id": "seed_job:sha256:B",
      "output_field_id": "field:SomeType.id",
      "input_arg_ref": "arg:SomeType.items.id"
    }
  ],
  "acyclic": true,
  "execution_order": ["seed_job:sha256:A", "seed_job:sha256:B"]
}
```

### BindingSetPlan

One executable plan = an ordered set of producer jobs whose combined output satisfies all requirements:

```json
{
  "binding_set_plan_id": "seed_binding_plan:sha256:<hex>",
  "selected_job_ids": ["seed_job:sha256:<hex>"],
  "discharged_constraint_ids": ["corr:sha256:<hex>"],
  "execution_order": ["seed_job:sha256:<hex>"],
  "status": "executable",
  "unresolved_requirement_ids": []
}
```

**`status` values:** `executable` | `partially_executable` | `unresolvable`

### Unresolved requirements

When S4 could not find any producer for a requirement:

```json
{
  "requirement_id": "seed_req:sha256:<hex>",
  "reason": "no_producer_path",
  "details": "no path found from Query to type:SomeType within budget"
}
```

---

## What the Runtime Receives Per Route

For each route the runtime should execute:

```
route (from S3):
  - witness.edges         → defines the GraphQL selection set to build
  - selector.arg_ref      → which variable to inject the seed value into
  - selector.input_path   → nested path if inside input object
  - verdict               → priority (open first, then unknown)

seed_plan (from S4, keyed by route.route_id):
  - binding_set_plans[0]  → first executable plan (try in priority order)
    - execution_order     → run producer jobs in this order
    - selected_job_ids    → which jobs to run

  per producer_job:
    - operation           → ready-to-send GraphQL query string
    - extraction.members  → response_path → where to find the value
    - extraction.anchor   → for joint co-read: shared parent object path
    - static_bindings     → pre-bound variables (pagination, enums)

  per job output:
    - extracted values (list) → must be adapted to consumer type
    - cardinality: "one" | "many" → take first vs. all values
```

**Happy path execution sequence:**

```
1. Pick route with verdict = open
2. Take binding_set_plan[0] (executable)
3. Run jobs in execution_order:
   a. Send producer_job.operation to endpoint
   b. Walk response via extraction.member.response_path
   c. Collect raw values (may be list)
4. For each candidate seed value:
   a. Apply adapter chain (identity / global_id_to_payload / ...)
   b. Build route validation query from witness.edges
   c. Inject adapted value at selector.arg_ref (+ input_path)
   d. Send validation query
   e. Check response contains target type (__typename == "Event")
5. Record: verified_binding_set if step (e) passes
```

---

## Concrete Example — Event (schema2)

### Route: `Query.event(id: ID!)` — verdict open

```json
{
  "route_id": "route:sha256:...",
  "verdict": "open",
  "origin": "traversal",
  "selector": { "arg_ref": "arg:Query.event.id", "input_path": [], "type_ref": "ID!" },
  "terminal_semantic_edge_id": "field:Query.event",
  "witness": {
    "display_projection": "Query.event -> Event",
    "edges": [
      { "edge_id": "field:Query.event", "kind": "field",
        "source_type_id": "type:Query", "field_id": "field:Query.event",
        "target_type_id": "type:Event" }
    ]
  }
}
```

**Seed plan for this route:**

- Requirement: `arg:Query.event.id` (type `ID!`, source `route_selector`)
- Producer: `query { events(first: 20) { nodes { id } } }`
- Extraction: `events.nodes[].id`, cardinality `many`
- Validation query: `query { event(id: $seed1) { __typename id } }`

**Runtime execution:**
1. Execute producer → get list of Event IDs
2. For each ID: call `event(id: $id)` → verify `__typename == "Event"`
3. If verified → confirmed access to Event object with that ID

### Route: `Query.events(filters: { ids: [ID!]! })` — verdict open, nested input

```json
{
  "selector": {
    "arg_ref": "arg:Query.events.filters.ids",
    "root_arg_ref": "arg:Query.events.filters",
    "input_path": ["ids"],
    "type_ref": "[ID!]!"
  }
}
```

The runtime must build: `query { events(filters: { ids: [$seed1] }) { ... } }` — not just `events(ids: [$seed1])`.
The `input_path` tells the runtime to wrap the value in `{ ids: ... }` under the `filters` argument.
