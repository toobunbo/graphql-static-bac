# Research Prompt: GraphQL Field Path Finder

Research and design a reusable **Field Path Finder** for GraphQL schemas.

This is an idea and architecture research task only. Do not implement
production code yet.

## Problem

Given an exact GraphQL output field such as:

```text
Card.assetId
```

find structural paths from selected Query roots to that field:

```text
Query.currentUser
-> CurrentUser.cards
-> CardConnection.nodes
-> ... on Card
-> Card.assetId
```

The primary use case is automatic seed harvesting:

```text
Required argument: Query.anyCard.assetId
-> candidate producer field: Card.assetId
-> find a path to Card.assetId
-> generate and execute a harvesting query
-> use the returned value as the argument seed
```

The component should also be reusable independently for schema exploration and
query generation.

## Scope

### Inputs

```json
{
  "target_field": "Card.assetId",
  "allowed_roots": [
    "Query.currentUser"
  ],
  "root_policy": "guardian|all|explicit",
  "max_depth": 12,
  "result_mode": "shortest|all|ranked"
}
```

Possible root categories:

- guardian roots: `currentUser`, `me`, `viewer`;
- self-scoped fields: `myCards`, `myAccounts`;
- public roots;
- explicit user-selected roots.

### Outputs

Return machine-readable path plans:

```json
{
  "target_field_id": "field:Card.assetId",
  "root_field_id": "field:Query.currentUser",
  "edges": [
    {
      "kind": "FIELD",
      "field_id": "field:Query.currentUser"
    },
    {
      "kind": "FIELD",
      "field_id": "field:CurrentUser.cards"
    },
    {
      "kind": "FIELD",
      "field_id": "field:CardConnection.nodes"
    },
    {
      "kind": "TYPE_CONDITION",
      "source_type": "AnyCardInterface",
      "target_type": "Card"
    },
    {
      "kind": "FIELD",
      "field_id": "field:Card.assetId"
    }
  ],
  "required_arguments": [],
  "field_hop_count": 4,
  "confidence": "structural"
}
```

The output must contain enough information for a separate query generator to
create a valid GraphQL operation.

## Research Questions

### Graph construction

- How should object, interface, union, connection, and edge types be
  represented?
- Should interface and union returns expand through every possible concrete
  type?
- How should Relay `Connection`, `Edge`, `nodes`, and `node` fields be handled?
- Should plumbing fields remain in the path while receiving lower ranking
  cost?
- How should cycles and recursive fields be bounded without losing useful
  paths?

### Root selection

- How can guardian roots such as `currentUser`, `me`, and `viewer` be detected?
- Should detection use field names, return types, descriptions, policy
  configuration, or runtime evidence?
- How should nested self-scoped roots such as `Query.currentUser.cards` be
  represented?
- Should the finder accept only explicit roots initially?

### Exact field matching

- The target is an exact field ID, not only a field name.
- How should interface-declared and concrete implementation fields be related?
- How should duplicate field names on unrelated types be prevented from
  matching?
- Can deprecation and field descriptions improve ranking?

### Required arguments

For every field on a candidate path:

- collect all required arguments;
- preserve full GraphQL type references;
- identify arguments that already have defaults;
- distinguish pagination arguments from identity selectors;
- mark paths that require additional unresolved seeds.

Example:

```json
{
  "field_id": "field:CurrentUser.cards",
  "argument": "first",
  "type": "Int",
  "binding_strategy": "generated_default",
  "suggested_value": 20
}
```

### Ranking paths

Explore ranking signals such as:

1. Guardian or self-scoped root.
2. Fewer unresolved required arguments.
3. Shorter semantic distance.
4. Fewer interface or union expansions.
5. Fewer recursive edges.
6. Direct ownership relation.
7. Non-deprecated fields.
8. Collection paths with bounded pagination.
9. Runtime success history.
10. Stable identity fields available alongside the target.

Do not assume the shortest path is always best.

### Query generation boundary

Determine which responsibilities belong to the finder and which belong to a
query generator.

Possible split:

```text
Field Path Finder:
schema -> structural path plan

Query Generator:
path plan -> GraphQL document + variables

Runtime Harvester:
document + token -> candidate values
```

### Verification

A structurally valid candidate value is not automatically a valid seed.

Example:

```text
Card.assetId
-> bind to Query.anyCard.assetId
-> verify that anyCard returns the same Card identity
```

Research how the finder output should support later verification:

- companion identity fields such as `id`;
- producer provenance;
- expected producer type;
- argument-to-output-field mapping;
- confidence before and after runtime verification.

## Relationship To Type Route Analysis

Keep this component separate from a target-type route analyzer:

```text
Type Route Analyzer:
Query -> target type

Field Path Finder:
selected roots -> exact output field
```

Investigate which graph construction and deterministic hashing utilities can be
shared without coupling their contracts.

## Existing Work To Survey

Research relevant papers and tools involving:

- GraphQL schema traversal;
- GraphQL query generation;
- InQL;
- schema-aware API fuzzing;
- REST API producer-consumer dependency inference;
- stateful API testing;
- data-flow and value-flow graphs;
- property-based query generation;
- shortest-path and constrained-reachability algorithms.

Prefer primary papers, official documentation, and source repositories.

## Deliverable

Produce a concise design research report containing:

1. Problem definition and boundaries.
2. Proposed input/output contracts.
3. Graph and traversal model.
4. Root classification strategy.
5. Required-argument handling.
6. Path ranking strategy.
7. Query-generator integration boundary.
8. Runtime verification requirements.
9. Relevant papers and tools.
10. Two or three architecture alternatives.
11. Major risks and unresolved questions.
12. Small experiments using `Card.assetId`.
13. Recommended MVP scope.

Do not implement the component yet. Clearly distinguish verified facts, design
inferences, and hypotheses requiring experiments.
