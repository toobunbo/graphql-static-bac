# graphql-path-enum: Flags and Gaps

This document describes the legacy tool in the parent repository. Source paths
such as `src/graph.rs` are relative to `../`, not to the new framework root.

## How many flags does the tool have?

The tool provides:

- **2 required options**: `-i` and `-t`.
- **2 main optional flags**: `--expand-connections` and `--include-mutations`.
- **2 informational flags**: `-h`/`--help` and `-V`/`--version`.

Basic command:

```bash
graphql-path-enum -i <introspection.json> -t <TYPE_NAME>
```

## 1. `-i`, `--introspect-query-path`

Specifies the introspection JSON file to analyze.

```bash
graphql-path-enum -i introspection.json -t User
```

This option is **required**. The tool does not accept an endpoint URL or a
GraphQL SDL file directly.

## 2. `-t`, `--type`

Specifies the target object type for which paths should be found.

```bash
graphql-path-enum -i introspection.json -t So5UserGroup
```

This option is **required**. Its value must be a GraphQL type name such as
`User`, not a field name such as `user`. Type names are case-sensitive.

Output example:

```text
Query (so5) -> So5Root (so5UserGroup) -> So5UserGroup
```

This means `Query.so5` returns `So5Root`, and then
`So5Root.so5UserGroup` returns the target type `So5UserGroup`.

## 3. `--expand-connections`

Shows the intermediate Relay Connection nodes.

By default, the tool attempts to simplify this path:

```text
Query (users) -> UserConnection (edges) -> UserEdge (node) -> User
```

into:

```text
Query (users) -> User
```

Use the flag to keep the complete structure:

```bash
graphql-path-enum \
  -i introspection.json \
  -t User \
  --expand-connections
```

Use this flag when you need to inspect `Connection`, `Edge`, `node`, or `nodes`
fields. Leave it disabled when you only need short, readable paths.

## 4. `--include-mutations`

By default, the tool only finds paths starting from the query root. This flag
also includes paths starting from the mutation root.

Default output:

```text
Query (...) -> TargetType
```

Output with the flag enabled:

```text
Query (...) -> TargetType
Mutation (...) -> MutationPayload (...) -> TargetType
```

Usage:

```bash
graphql-path-enum \
  -i introspection.json \
  -t User \
  --include-mutations
```

This flag can increase the number of results significantly.

## 5. `-h`, `--help`

Displays the CLI help:

```bash
graphql-path-enum --help
```

## 6. `-V`, `--version`

Displays the tool version:

```bash
graphql-path-enum --version
```

## Combining flags

Search from both Query and Mutation while preserving all Connection nodes:

```bash
graphql-path-enum \
  -i introspection.json \
  -t So5UserGroup \
  --include-mutations \
  --expand-connections
```

## Quick reference

| Flag or option | Required | Purpose |
| --- | --- | --- |
| `-i`, `--introspect-query-path` | Yes | Select the introspection JSON file |
| `-t`, `--type` | Yes | Select the target object type |
| `--expand-connections` | No | Preserve Connection and Edge nodes |
| `--include-mutations` | No | Include paths starting from Mutation |
| `-h`, `--help` | No | Display help |
| `-V`, `--version` | No | Display the version |

# Core Technical Design

## Does it rebuild GraphQL?

Not exactly. The tool does **not** rebuild a GraphQL server, executable schema,
resolvers, arguments, or authorization logic.

It creates a simplified **directed graph of object relationships** from the
introspection JSON:

```text
GraphQL object type = graph node
Object-returning field = directed graph edge
Field name = edge name
```

For example, this schema:

```graphql
type Query {
  user(id: ID!): User
}

type User {
  organization: Organization
}

type Organization {
  id: ID!
}
```

becomes this in-memory graph:

```text
[Query] --user--> [User] --organization--> [Organization]
```

The `id` field is ignored because it returns a scalar rather than an object.

## Processing pipeline

The complete flow is:

```text
CLI arguments
    |
    v
Read introspection JSON
    |
    v
Deserialize the schema with Serde
    |
    v
Convert object types and fields into nodes and edges
    |
    v
Run DFS from Query, and optionally Mutation
    |
    v
Print each discovered field chain
```

In source terms, `src/lib.rs` coordinates three main operations:

```rust
let schema = introspection::Schema::new(path)?;
let graph = graph::Graph::new(schema, show_connections)?;
let results = graph.enumerate_paths_to_target(type_name, include_mutations)?;
```

## Step 1: Deserialize introspection JSON

`src/introspection.rs` defines a small subset of the complete GraphQL
introspection model:

```text
Query
+-- data
    +-- __schema
        +-- queryType
        +-- mutationType
        +-- types[]
            +-- name
            +-- fields[]
                +-- name
                +-- args[]
                +-- type
```

Serde reads the JSON into these Rust structures:

| Rust structure | Introspection data |
| --- | --- |
| `Schema` | Query root, optional Mutation root, and all types |
| `SchemaType` | A named type and its fields |
| `Field` | Field name, arguments, and return type |
| `FieldType` | Return `kind`, `name`, and nested `ofType` |
| `FieldArg` | Argument name |

Unknown JSON properties such as descriptions and deprecation metadata are
ignored. This keeps the internal representation small.

## Step 2: Unwrap GraphQL return types

Introspection represents wrappers through nested `ofType` objects. For example:

```graphql
users: [User!]!
```

is represented conceptually as:

```text
NON_NULL
+-- LIST
    +-- NON_NULL
        +-- OBJECT User
```

`FieldType::get_graph_object_name()` walks down the `ofType` chain until it
finds `kind == "OBJECT"`.

Therefore all of these resolve to the same graph destination:

```graphql
user: User
user: User!
users: [User]
users: [User!]!
```

They all create an edge to the `User` node.

If the final type is a scalar, enum, interface, union, or input object, the
method returns `None`, so no graph edge is created.

## Step 3: Create nodes and edges

The graph uses two vectors:

```rust
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    mutation_node_index: Option<usize>,
}
```

A node stores its type name and outgoing edge indexes:

```rust
pub struct Node {
    pub name: String,
    pub edges: Vec<EdgeIndex>,
}
```

An edge stores the GraphQL field name and destination node index:

```rust
pub struct Edge {
    pub name: String,
    pub destination: NodeIndex,
}
```

For this relationship:

```graphql
type User {
  organization: Organization
}
```

the graph stores approximately:

```text
Node: User
Edge: { name: "organization", destination: OrganizationIndex }
```

Indexes are used instead of Rust references. This avoids self-referential data
structures and makes graph traversal straightforward.

## Step 4: Build the reachable graph recursively

Before graph construction, every introspection type is inserted into a
`HashMap` keyed by type name.

Each map entry has one of two states:

```text
NewNode(type definition)
CachedNode(index in Graph.nodes)
```

Construction starts from the schema's query root:

1. Add the Query type as a graph node.
2. Inspect every Query field.
3. Resolve the object returned by each field.
4. Recursively add that object if it is new.
5. Create an edge from the current node to the destination node.
6. Reuse the existing index if the destination is already cached.

Caching a node **before** recursively processing its fields prevents infinite
recursion while building cyclic schemas.

Example cycle:

```graphql
type User {
  manager: User
}
```

`User` is already cached when `manager` is processed, so the tool creates a
self-edge instead of recursively creating `User` forever:

```text
[User] --manager--> [User]
```

Only object types reachable from Query or Mutation are added to the final
graph. An object that exists in introspection but is unreachable from both roots
is not relevant to root-based path enumeration.

## Step 5: Simplify Relay Connections

Without `--expand-connections`, the tool tries to collapse Relay-style
pagination structures.

Suppose the schema contains:

```graphql
type Query {
  users(
    first: Int
    last: Int
    before: String
    after: String
  ): UserConnection
}

type UserConnection {
  edges: [UserEdge]
  nodes: [User]
  pageInfo: PageInfo!
}

type UserEdge {
  node: User
}
```

The complete graph would be:

```text
Query --users--> UserConnection --edges--> UserEdge --node--> User
```

The tool identifies `UserConnection` using two checks:

1. The return type name ends with `Connection`.
2. The original field has `first`, `last`, `before`, and `after` arguments.

It then inspects the Connection fields and selects an object type that is not
an `Edge` type or `pageInfo`. In this example, it finds `User` through `nodes`
and creates a shortcut:

```text
Query --users--> User
```

The edge keeps the original field name `users`. With
`--expand-connections`, this shortcut logic is disabled and all intermediate
objects remain visible.

## Step 6: Find paths with DFS

After graph construction, the tool locates the node whose name matches the
value passed to `--type`.

It then runs depth-first search from:

- Node index `0`, which is the Query root.
- The stored Mutation node index when `--include-mutations` is enabled.

During traversal:

- `stack` stores the current sequence of edge indexes.
- `visited` tracks edges already explored under one root field to limit cycles.
- `result` stores a copy of `stack` whenever the target node is reached.

Conceptually:

```text
dfs(current, target, stack):
    if current == target:
        save stack
        return

    for each outgoing edge:
        return from this DFS call if the edge is already visited
        mark edge as visited
        push edge onto stack
        dfs(edge.destination, target, stack)
        pop edge from stack
```

The tool creates a fresh `visited` list for each outgoing edge of the root node,
but keeps that list across all sibling branches below that edge. It also never
removes an edge from `visited` during backtracking. This avoids many repeated
traversals, but it is also why the result is not a complete enumeration of every
valid path.

## Step 7: Format the result

A result contains:

```text
(origin node index, list of edge indexes)
```

The printer resolves each index back to its type or field name:

```text
OriginNode (EdgeName) -> DestinationNode
```

For example:

```text
Query (so5) -> So5Root (so5UserGroup) -> So5UserGroup
```

The parentheses always contain a GraphQL **field name**. Names outside the
parentheses are GraphQL **object type names**.

## What information is discarded?

The internal graph intentionally does not preserve most of the GraphQL schema:

| Schema information | Preserved? |
| --- | --- |
| Object type names | Yes |
| Object-returning field names | Yes |
| Query and Mutation roots | Yes |
| Field argument names | Only for Connection detection |
| Argument types and defaults | No |
| Scalar and enum fields | No |
| Input objects | No |
| Interfaces and unions | No |
| Descriptions and deprecations | No |
| Directives | No |
| Resolver implementation | No |
| Authentication and authorization | No |

This is why the tool can discover schema paths efficiently, but cannot generate
complete queries or reproduce runtime behavior.

# Tool Gaps

## 1. It does not guarantee every possible path

The tool uses DFS and marks edges as visited to prevent infinite cycles. The
current `visited` list is not backtracked for each branch, so valid paths may be
skipped when multiple branches converge on the same suffix.

Therefore:

```text
Found 89 ways
```

only means that the tool found 89 paths. It does not prove that the schema has
exactly 89 possible paths.

## 2. Incomplete `INTERFACE` and `UNION` support

The graph only follows fields whose return kind is `OBJECT`. Paths through an
interface, union, or inline fragment may be missing.

Example:

```graphql
type Query {
  node(id: ID!): Node
}

interface Node {
  id: ID!
}
```

The `node` field returns an `INTERFACE`, not an `OBJECT`, so the tool does not
follow it.

## 3. Output is not an executable GraphQL query

The tool only prints a sequence of types and fields:

```text
Query (user) -> User (organization) -> Organization
```

It does not include:

- Required arguments.
- Variables.
- Scalar selection fields.
- Inline fragments.
- Directives.

The user must still build the executable query manually.

## 4. It does not test authorization

The tool does not send requests and cannot inspect runtime resolver behavior. It
cannot determine whether a path is vulnerable to IDOR/BOLA or missing permission
checks.

It only shows that a path exists in the schema.

## 5. Connection detection is heuristic

The tool considers a field to be a Relay Connection when:

- Its return type ends with `Connection`.
- The field has all four arguments: `first`, `last`, `before`, and `after`.

Schemas using different conventions may not be detected or simplified
correctly.

There is also a case-sensitive implementation detail: the code compares an
object type name against `pageInfo`, while the standard object type is normally
named `PageInfo`. If that field is encountered before the item field, the
shortcut may select the wrong destination type.

## 6. It does not accept an endpoint or SDL directly

The only supported input is a GraphQL introspection response in JSON format.
The tool does not:

- Call a GraphQL endpoint.
- Execute an introspection query.
- Read `schema.graphql` directly.

SDL must first be converted into introspection JSON.

## 7. No JSON output

The tool only produces human-readable text. Automation, sorting by path length,
or automatic query generation requires parsing this text, as done by the local
`test_paths.py` script.

## 8. No depth or result limits

The CLI does not provide flags such as:

```text
--max-depth
--max-paths
--shortest-only
```

Large schemas may produce substantial output, especially with
`--include-mutations` or `--expand-connections`.

## 9. No filtering by field or root

The CLI cannot:

- Require paths to pass through a specific field.
- Exclude a type or field.
- Select a root other than Query or Mutation.
- Sort paths by hop count.

External tools or source changes are required for these operations.

## 10. Limited test coverage

The current unit tests mainly cover `LIST` and `NON_NULL` unwrapping and
Connection detection. DFS behavior, cycles, converging paths, Mutation paths,
and CLI output do not have equivalent coverage.

## Short conclusion

The tool does one job well: **finding field chains from Query or Mutation to a
target object type in an introspection schema**.

Its main gaps are incomplete path enumeration, incomplete interface and union
support, no query generation, and no runtime authorization testing.
