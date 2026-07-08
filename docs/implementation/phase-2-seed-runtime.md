# Phase 2 Seed Runtime

**Status:** Implemented.

**Design authority:** `docs/design/seedFinderPhases.md`.

This subsystem runs after S4. It obtains concrete argument values for S3
routes and verifies that each complete binding set can execute the route far
enough to reach its target type.

It does not classify authorization behavior, compare accounts, project
sensitive fields, or decide whether any result is a vulnerability.

## 1. Objective

Consume:

```text
S0/v1 schema_ir.json
S3/v2 routes.json
S4/v1 seed_plans.json
runtime request template
```

Produce:

```text
seed_runtime.json
```

For every route, the output must answer:

```text
Which complete argument binding sets were obtained?
Where did each value come from?
Which representation was sent to each consumer?
Did the minimal route query reach the expected target type?
Why did unsuccessful strategies fail?
```

The proposed command surface is deliberately outside the numbered static
stages:

```text
graphql-static-bac runtime seed \
  --schema-ir schema_ir.json \
  --routes routes.json \
  --seed-plans seed_plans.json \
  --request-template runtime-request.json \
  --verdict open \
  --verdict unknown \
  --output seed_runtime.json
```

S0 and S3 remain explicit inputs because S4 contains producer operations but
does not contain enough schema and route-witness data to emit consumer
validation operations.

## 2. Scope

### 2.1 Included

- one authentication context per invocation;
- explicit route selection by route ID and/or S3 verdict;
- arbitrary GraphQL HTTP request body shape through JSON Pointer injection;
- credentials embedded directly in headers or request body;
- read-only Query producer execution;
- S4 binding-set scheduling and fallback;
- scalar, list, and branch-aware extraction;
- static binding application;
- producer-job dependency order;
- consumer-specific value representation adapters;
- minimal full-route validation with `__typename` and identity projection;
- execution deduplication;
- runtime fact collection;
- deterministic artifact structure and stable IDs.

### 2.2 Excluded

- bug or authorization oracle;
- attacker/victim comparison;
- sensitive-field projection;
- mutation producers;
- arbitrary custom-scalar synthesis;
- changing server state;
- claiming that a rejected route is secure;
- deleting static candidates based on runtime history.

Runtime history may reorder future attempts. It must not remove S4 plans.

The normal Watchlist invocation selects `open` and `unknown`. Guarded routes
remain available but are not executed unless explicitly requested.

## 3. Request Template

The runtime accepts a structured JSON template:

```json
{
  "request_profile_id": "sorare-test",
  "auth_context_id": "account_b",
  "request": {
    "url": "https://target.example/graphql",
    "method": "POST",
    "headers": {
      "content-type": "application/json",
      "authorization": "Bearer full-token-value",
      "x-api-key": "full-api-key-value"
    },
    "body": {
      "operationName": null,
      "query": null,
      "variables": {},
      "extensions": {}
    }
  },
  "injection": {
    "operation_name_pointer": "/operationName",
    "query_pointer": "/query",
    "variables_pointer": "/variables"
  },
  "limits": {
    "execution_mode": "first_verified",
    "timeout_ms": 15000,
    "max_requests": 500,
    "max_requests_per_route": 3,
    "max_producers_per_route": 3,
    "max_values_per_producer": 3,
    "max_adapter_attempts_per_binding_set": 2,
    "max_values_per_requirement": 100,
    "max_verified_bindings_per_route": 20
  }
}
```

Rules:

- JSON Pointer is used instead of string replacement.
- Unknown body members are preserved.
- Headers, cookies, tokens, and API keys may be stored and emitted verbatim.
- `auth_context_id` is mandatory and identifies the execution context.
- Credentials are not included in stable-ID inputs. Changing only a token
  value must not change plan, execution, or binding identity.
- Changing `auth_context_id`, endpoint, operation inputs, or schema
  fingerprint must change execution identity.
- `max_requests_per_route` is a hard cap over all HTTP executions for one
  route, including producer harvests and validation representation attempts.
  Reaching the cap yields `budget_exhausted` with `coverage: bounded`; it is
  not evidence that no usable seed exists.
- `first_verified` is the production default. It stops the route after the
  first binding set reaches the target type.
- `exhaustive` retains the calibration behavior and continues until the
  configured binding or request limits.
- producer, value, and adapter limits distribute work across different
  producer paths instead of allowing one large response to consume the route
  budget.

## 4. Runtime Contract

Add `src/contracts/seed_runtime.rs`.

Top-level shape:

```text
SeedRuntimeData
  runtime_model
  request_profile_id
  auth_context_id
  route_bindings: BTreeMap<route_id, RouteRuntimeResult>
  executions: BTreeMap<execution_id, ExecutionRecord>
  runtime_facts
```

### 4.1 Route result

```text
RouteRuntimeResult
  route_id
  target_type_id
  status
  verified_binding_sets
  attempted_plan_ids
  failures
  coverage
```

Closed route statuses:

```text
verified
unresolved
no_seed_required
budget_exhausted
```

`verified` means that at least one complete binding set reached the target type
under this request profile and auth context. It does not mean the route is
authorized correctly.

### 4.2 Binding set

```text
VerifiedBindingSet
  binding_set_id
  source_binding_set_plan_id
  bindings: BTreeMap<arg_ref, RuntimeBinding>
  producer_execution_ids
  validation_execution_id
  validation
```

Each binding records:

```text
RuntimeBinding
  requirement_id
  input_path
  source_value
  consumer_value
  adapter_chain
  producer_job_id
  producer_execution_id
  extraction_provenance
```

Values use `serde_json::Value`, not strings, so scalar, list, enum, Boolean,
numeric, and nested input-object values retain their shapes.

### 4.3 Execution record

```text
ExecutionRecord
  execution_id
  kind: producer | validation
  job_id
  route_id
  request
  response
  status
  extracted_values
  failure
```

The main artifact may contain complete requests and responses. A later
`--evidence-dir` option may move large response bodies into content-addressed
files without changing the logical contract.

### 4.4 Status taxonomy

Producer execution:

```text
not_run
empty
http_error
graphql_error
extraction_failed
extracted
```

Binding-set validation:

```text
not_run
consumer_rejected
target_not_reached
runtime_error
verified
```

Failure codes:

```text
producer_empty
producer_http_error
producer_graphql_error
extraction_path_missing
extraction_shape_mismatch
representation_mismatch
consumer_rejected
target_not_reached
dependency_unresolved
producer_exhausted
coverage_incomplete
request_budget_exhausted
```

These codes describe seed acquisition and route executability only.
`authorization_blocked`, `information_leak`, and `vulnerability` are not
Phase 2 statuses.

## 5. Locked Execution Model

### 5.1 Candidate scheduling

For each route:

1. read executable S4 `binding_set_plans` in deterministic rank order;
2. prefer plans whose producer executions are already cached;
3. prefer standalone and static jobs before threaded jobs;
4. execute jobs in the plan's declared topological order;
5. cap distinct producer paths and values consumed from each producer;
6. assemble complete correlated binding tuples;
7. validate each tuple with a bounded adapter portfolio;
8. in `first_verified`, stop immediately after the first target-reaching
   binding;
9. in `exhaustive`, continue until configured binding or request limits;
10. retain every failed attempt in the artifact.

An empty producer or rejected binding triggers fallback to the next plan or
candidate value. It does not invalidate the static S4 plan.

Phase 2 never performs schema-wide producer-path discovery. Every producer it
executes must have been emitted by S4.

This creates one required S4 handoff correction before the executor is useful:
the bounded S4 portfolio must preserve structurally diverse, direct producer
alternatives. The Watchlist mini test found that `currentUser.cards` returned
all 28 Card seed pairs, while the capped S4 portfolio retained only root
producers that the runtime rejected.

S4 must therefore:

- rank direct executable producer paths before longer paths;
- retain diversity by ordered producer witness, not only terminal field type;
- avoid allowing many equivalent `anyCard`/`anyCards` alternatives to consume
  the whole route plan cap;
- include `currentUser.cards -> nodes -> assetId/slug` in the Watchlist Card
  producer portfolio;
- keep the existing global plan cap, but expose a warning when candidates were
  truncated.

If every emitted producer is exhausted, Phase 2 reports
`producer_exhausted`; it must not silently invent a producer.

### 5.2 Execution identity

```text
execution_id = hash(
  kind,
  job_or_validation_operation_id,
  endpoint,
  schema_fingerprint,
  auth_context_id,
  canonical_input_bindings
)
```

Credential bytes are deliberately excluded.

The cache is scoped to one invocation in the MVP. A previous runtime artifact
may later be supplied as read-only scheduling history.

### 5.3 Extraction

The extractor must parse S4 response paths structurally:

```text
currentUser.cards.nodes[].assetId
```

Supported tokens:

```text
object member
list expansion []
optional/null branch
branch anchor and element index
```

It must not flatten correlated fields independently. Joint jobs use the S4
anchor descriptor and emit tuples per shared branch instance.

Every value retains:

```text
response path
branch path
list indices
producer execution ID
producer field ID
```

### 5.4 Static and threaded bindings

- Static jobs produce values without HTTP execution.
- Threaded jobs receive values from predecessor producer jobs.
- Dependency edges map an exact output field to an exact input argument.
- Missing predecessor output yields `dependency_unresolved`.
- A runtime dependency failure affects that strategy only; the scheduler may
  continue with another binding-set plan.

### 5.5 Representation adapters

Adapters are consumer-specific and validation-driven.

MVP adapters:

```text
identity
scalar_to_singleton_list
list_to_scalar_candidates
global_id_to_payload
ID_String_passthrough
```

`global_id_to_payload` converts:

```text
Watchlist:<uuid> -> <uuid>
```

It is attempted only when:

- the source value has a single type prefix;
- identity validation was rejected;
- the consumer argument is ID/String-compatible.

An adapter is considered successful only after consumer validation. Successful
and rejected adapters are recorded as runtime facts keyed by:

```text
consumer_arg_ref
producer_field_id
source value shape/prefix
```

No adapter is inferred solely from matching GraphQL scalar names.

### 5.6 Consumer validation query

Add a dedicated runtime validation emitter. It consumes S0, the S3 route
witness, and a complete binding set.

The operation:

- reproduces ordered S3 FIELD and TYPE_CONDITION edges;
- binds every route requirement and static binding;
- uses variables for harvested values;
- preserves enum literals and nested input paths;
- projects only `__typename` and an identity field when available;
- does not project policy, PII, financial, or other bug-oracle fields;
- parses with `graphql-parser` before execution.

Validation succeeds only when:

1. GraphQL accepts all bound arguments;
2. the route reaches a value whose `__typename` matches the S3 target type;
3. correlation and dependency provenance remain intact.

Null intermediate fields or a different concrete type yield
`target_not_reached`, not `consumer_rejected`.

GraphQL errors are recorded without security interpretation.

### 5.7 Pagination and coverage

The MVP executes the bounded operation emitted by S4. It records:

```text
complete
bounded
unknown
```

as seed coverage.

It must not claim complete coverage for a connection when the S4 operation
does not expose `pageInfo`.

Relay pagination, operation rewriting, and exhaustive list harvesting form a
separate post-MVP milestone. They are not required to verify at least one
binding per route.

## 6. Module Layout

Implemented layout:

```text
src/contracts/seed_runtime.rs
src/runtime/
  mod.rs
  config.rs
  request_adapter.rs
  transport.rs
  engine.rs
  extractor.rs
  adapters.rs
  validation.rs
  ids.rs
src/stages/seed_runtime/
  mod.rs
  runner.rs
src/main.rs
```

`runtime` owns pure orchestration and typed transport boundaries.
`stages/seed_runtime` owns artifact loading, cross-input validation, and atomic
output writing. The CLI remains a thin adapter.

The blocking transport invokes `curl`; the runtime core depends on a
`GraphqlTransport` trait so unit tests do not call an external endpoint.

## 7. Implemented Scope

### 7.1 S4 runtime handoff

- producer portfolios retain structurally diverse paths;
- authenticated inventory producers rank before indirect alternatives;
- enum/static dimensions retain representative alternatives;
- `portfolio_truncated` explicitly marks bounded S4 search;
- Watchlist regression coverage retains `currentUser.cards`.

### 7.2 Contracts and request execution

- typed request-template and runtime-output contracts;
- stable IDs for execution and verified binding sets;
- JSON Pointer operation injection;
- header/body preservation;
- blocking `curl` transport with timeout;
- normalized transport, HTTP, GraphQL, and extraction failures;
- global and per-route request budgets;
- atomic artifact output.

### 7.3 Extraction and binding assembly

- response-path tokenizer;
- object/list/null traversal;
- branch/index provenance;
- joint extraction anchored to shared branch instances;
- topological producer execution;
- exact dependency output-to-input binding;
- bounded independent-dimension product;
- correlation tuple preservation;
- complete binding-set construction.

### 7.4 Scheduling and validation

- deterministic route, plan, and job iteration;
- invocation-local execution deduplication;
- producer fallback with complete attempt records;
- `first_verified` and `exhaustive` execution modes;
- producer-path, producer-value, and adapter-attempt budgets;
- ordered adapter candidates;
- identity, scalar/list, and global-ID payload adapters;
- adapter runtime facts;
- consumer-specific failed-adapter fallback;
- minimal validation operations from S3 witnesses;
- variable definitions and nested argument values;
- inline fragments from TYPE_CONDITION edges;
- target `__typename` verification;
- distinction between consumer rejection and target-not-reached.

### 7.5 Runner and verification

- cross-input stage/version/fingerprint validation;
- `runtime seed` CLI;
- route selection by repeated `--verdict` and `--route-id` arguments;
- unit coverage for adapter fallback and joint correlation extraction;
- planner regression coverage for authenticated inventory producers;
- live Watchlist calibration for all three open and twelve unknown routes.

Live endpoint checks are calibration commands, not default `cargo test` cases.
The bounded three-request calibration is recorded in
`docs/research/phase-2-watchlist-calibration.md`.
The scheduler A/B benchmark is recorded in
`docs/research/phase-2-adaptive-scheduler-benchmark.md`.

## 8. Acceptance Criteria

Phase 2 is complete when:

- every S4 route receives a deterministic runtime result;
- every attempted producer has an execution record;
- every extracted value retains provenance;
- complete binding tuples preserve correlation;
- candidate fallback works across empty, GraphQL-error, extraction, adapter,
  and validation failures;
- consumer-specific representation adapters are validated, not guessed;
- verified bindings reach the S3 target type;
- unresolved routes retain exact failure evidence;
- request templates may contain complete credentials;
- credentials do not influence stable IDs;
- the artifact contains no security verdict or bug oracle;
- Watchlist calibration reproduces the expected seed and failure classes.

## 9. Deferred Work

- Relay pagination and exhaustive producer harvesting;
- batched enum probing with GraphQL aliases;
- runtime ranking of values inside large producer responses;
- mutation-based seed creation;
- recursive producer discovery generated from runtime hidden requirements;
- custom scalar adapters;
- multiple auth contexts in one invocation;
- cross-run runtime-history scheduling;
- cross-account execution;
- policy projections and bug oracles;
- persistent runtime learning database.
