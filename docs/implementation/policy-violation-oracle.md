# Policy Violation Oracle Implementation Plan

**Status:** Standalone runtime and explicit-artifact type orchestration
implemented on June 14, 2026. Automatic policy classification remains
deferred.

**Design authority:** `docs/design/Oracle_design.md`, with the architecture
decision that policy classification produces a hypothesis and the Oracle only
tests that hypothesis at runtime.

## 1. Objective

For a target GraphQL type, test whether an observer authentication context can
resolve an object owned by another context while that object matches a
classifier-provided restricted-policy hypothesis.

The Oracle is recall-first. It does not attempt to understand product
semantics. If a classifier marks `hidden: true` as restricted, the Oracle tests
that assumption even when the application uses `hidden` for presentation
rather than authorization.

Positive runtime evidence is emitted as:

```text
policy_violation_candidate
```

This is a review candidate, not a confirmed vulnerability.

## 2. Pipeline Position

```text
S0 Schema IR
  + S3 routes
  + S4 seed plans
  + Phase 2 owner verified bindings
  + policy hypotheses
  + owner request profile
  + observer request profile
        |
        v
Policy Violation Oracle
        |
        +-> policy_oracle_runtime.json
        `-> policy_violation_candidates.json
```

The Oracle does not search for routes or seeds. It reuses:

- the exact S3 `route_id` and witness;
- a Phase 2 `verified_binding_set` obtained under the owner context;
- the binding set's consumer values and provenance.

## 3. Classifier Boundary

The policy classifier owns the hypothesis:

```json
{
  "hypothesis_id": "policy_hypothesis:sha256:...",
  "type_id": "type:Watchlist",
  "field_id": "field:Watchlist.public",
  "policy_class": "boolean_visibility",
  "restricted_values": [false],
  "rule": "public_false"
}
```

The Oracle must not reinterpret or override this classification. It validates
only that:

1. the field exists on the target type;
2. the projected runtime value matches one of `restricted_values`;
3. the observer response contains the same target object identity.

Initial classifier classes may include:

```text
boolean_visibility
boolean_privacy
enum_visibility
enum_privacy
enum_state
```

The Oracle implementation must remain class-agnostic. It compares typed JSON
values supplied by the hypothesis.

## 4. Inputs

```text
schema_ir.json              S0/v1
routes.json                 S3/v2
seed_plans.json             S4/v1
owner_seed_runtime.json     seed_runtime/v1
policy_hypotheses.json      policy-classifier artifact
owner_request_profile.json
observer_request_profile.json
```

Required invariants:

- all static artifacts have the same schema fingerprint;
- every hypothesis type and field resolves in S0;
- the policy field belongs to the hypothesis target type;
- owner and observer profiles have distinct `auth_context_id` values;
- each tested route targets the hypothesis type;
- each tested route has at least one owner `verified_binding_set`;
- the original binding set remains unchanged during observer replay.

The default route selection is `open + unknown`. Guarded routes may be selected
explicitly.

## 5. Runtime Model

### 5.1 Owner calibration

For each selected route and verified owner binding set:

1. reproduce the ordered S3 FIELD and TYPE_CONDITION witness;
2. apply the exact Phase 2 consumer bindings;
3. project `__typename`, an identity field, and the policy field;
4. execute with the owner request profile;
5. collect target instances whose policy value matches the hypothesis;
6. retain each matching target identity as a victim instance.

The initial identity strategy is:

```text
id field on the target type
```

If the target has no stable identity field, the execution is inconclusive.
Structural response position is not sufficient identity evidence.

### 5.2 Route-specific rebinding

The Oracle does not replace arbitrary route arguments with the target ID.

It replays the complete owner binding set. Therefore:

```text
Query.node(Watchlist.id) -> Watchlist
```

replays the Watchlist ID, while:

```text
Query.anyCard(Card.assetId) -> Card.decks -> Deck
```

replays the Card `assetId` that led to the victim Deck.

This preserves correlation, dependency order, input-object paths, and
consumer-specific representations already validated by Phase 2.

### 5.3 Observer replay

For each calibrated victim instance:

1. emit the same route operation;
2. use the same variables and static bindings;
3. execute with the observer request profile;
4. inspect only the GraphQL `data` tree;
5. find target instances by `__typename` and identity;
6. emit a candidate when the victim identity is present.

The observer response does not need to equal the owner response.

## 6. Decision Model

### 6.1 Positive candidate

```text
policy_violation_candidate
```

Required evidence:

- owner resolved a target instance;
- its policy value matched the restricted hypothesis;
- observer replay used the same route and binding set;
- observer response contained the same target identity.

### 6.2 Non-candidate outcomes

```text
no_violation_observed
inconclusive
```

`no_violation_observed` means the replay completed but the victim identity was
not present. It is not a proof that the route is secure.

`inconclusive` covers invalid controls, missing fixtures, transport failures,
budget exhaustion, unsupported identity, or failed route replay.

## 7. Failure Taxonomy

```text
policy_field_missing
policy_field_type_mismatch
owner_seed_unavailable
owner_route_failed
restricted_object_not_found
target_identity_unavailable
route_rebinding_failed
observer_auth_failed
observer_http_error
observer_graphql_error
response_parse_failed
request_budget_exhausted
victim_identity_not_observed
```

Failures are retained in the detailed runtime artifact. They are never written
to the positive-candidate file.

## 8. Output Contracts

### 8.1 Detailed runtime artifact

```text
policy_oracle_runtime.json
```

It retains:

- route and hypothesis IDs;
- owner and observer execution records;
- binding-set identity;
- calibrated victim identities and policy values;
- outcome and failure evidence;
- request-budget and coverage state.

### 8.2 Review artifact

```text
policy_violation_candidates.json
```

Only positive candidates are written:

```json
{
  "candidates": [
    {
      "verdict": "policy_violation_candidate",
      "type": "Watchlist",
      "route_id": "route:sha256:...",
      "auth_context": {
        "owner": "account_b",
        "observer": "account_a"
      },
      "response": {
        "data": {}
      }
    }
  ]
}
```

`route_id` is retained because otherwise the resolver path responsible for the
candidate cannot be recovered reliably.

The review artifact does not contain negative or inconclusive executions.

## 9. Stable Identities

Add stable IDs for:

```text
policy_hypothesis:sha256:...
policy_oracle_run:sha256:...
policy_oracle_execution:sha256:...
policy_candidate:sha256:...
```

Candidate identity is derived from:

```text
schema fingerprint
hypothesis ID
route ID
owner auth_context_id
observer auth_context_id
binding_set_id
victim target identity
```

Credential bytes are excluded.

## 10. Implementation Milestones

### Milestone 0 - Contracts

- define the policy-hypothesis artifact;
- define detailed runtime and candidate contracts;
- add closed enums for outcomes and failure codes;
- validate fingerprints, references, and distinct auth contexts;
- add canonical stable-ID helpers.

### Milestone 1 - Policy-aware operation emitter

- reuse Phase 2 route rendering and binding assembly;
- support custom target projections;
- project `__typename`, identity, and policy field;
- preserve FIELD/TYPE_CONDITION order;
- parse every emitted operation before execution.

The existing Phase 2 validation emitter must continue projecting only identity
evidence. Oracle projection is a separate API.

### Milestone 2 - Owner calibration

- consume owner verified binding sets;
- execute policy-aware operations under owner context;
- extract target instances from nested objects and lists;
- compare policy values as typed JSON;
- record restricted victim identities;
- classify missing restricted fixtures as inconclusive.

### Milestone 3 - Observer replay

- replay identical variables with the observer profile;
- inspect the `data` tree independently of HTTP status;
- match target `__typename` plus identity;
- create positive candidate evidence;
- retain unsuccessful executions in the detailed artifact.

### Milestone 4 - Scheduling and budgets

- default to S3 `open + unknown`;
- permit explicit route IDs and guarded routes;
- cap owner and observer requests per route;
- stop after the first candidate per `(route, hypothesis)` by default;
- preserve an exhaustive calibration mode;
- cache identical owner calibration executions.

### Milestone 5 - Writers and CLI

Add a standalone command:

```text
graphql-static-bac runtime policy-oracle \
  --schema-ir schema_ir.json \
  --routes routes.json \
  --seed-plans seed_plans.json \
  --owner-seeds owner_seed_runtime.json \
  --policy-hypotheses policy_hypotheses.json \
  --owner-request owner-request.json \
  --observer-request observer-request.json \
  --runtime-output policy_oracle_runtime.json \
  --candidates-output policy_violation_candidates.json
```

The command must write both outputs atomically.

### Milestone 6 - Type pipeline integration

The implemented orchestration command is:

```bash
graphql-static-bac pipeline policy-type \
  --schema-ir schema_ir.json \
  --args args.json \
  --policy route-analysis-v1.json \
  --target Watchlist \
  --policy-hypotheses policy_hypotheses.json \
  --owner-request owner-request.json \
  --observer-request observer-request.json \
  --output-dir output/
```

It runs `S3 -> S4 -> owner seed runtime -> policy violation oracle`, selecting
`open + unknown` routes. The classifier artifact remains an explicit input
until the automatic policy classifier is implemented.

## 11. Test Plan

### Unit tests

- typed Boolean and enum restricted-value matching;
- missing or wrong-type policy fields;
- target identity extraction through objects and lists;
- observer identity match and mismatch;
- GraphQL `data` scanning ignores unrelated `errors` and HTTP status;
- stable candidate IDs and deterministic serialization.

### Query-emitter tests

- direct `Query.node(id) -> Watchlist`;
- `Query.nodes(ids) -> Watchlist`;
- nested selector route;
- TYPE_CONDITION target;
- Connection/Edge plumbing;
- input-object and list-valued bindings;
- route with no target `id` is inconclusive.

### Fake-transport integration tests

- owner restricted + observer same identity -> candidate;
- owner restricted + observer null -> no violation observed;
- owner non-restricted -> no candidate;
- owner control failure -> inconclusive;
- observer GraphQL error -> inconclusive;
- route-specific binding set is byte-for-byte unchanged;
- only positive results appear in the candidate artifact.

### Watchlist golden test

Fixture:

```text
type: Watchlist
policy field: Watchlist.public
restricted value: false
owner: account_b
observer: account_a
```

Test at minimum:

```text
Query.node(id) -> Watchlist
Query.nodes(ids) -> Watchlist
Query.market -> MarketRoot.watchlist(id) -> Watchlist
```

The golden output must contain only routes where the observer response includes
the same restricted Watchlist identity calibrated under the owner context.

## 12. Acceptance Criteria

The milestone is complete when:

- the Oracle consumes classifier hypotheses without semantic reinterpretation;
- every tested route reuses a Phase 2 verified owner binding set;
- indirect routes preserve their original selector inputs and correlation;
- owner calibration proves restricted policy state and target identity;
- observer replay can produce deterministic per-route candidates;
- failures remain distinguishable from negative observations;
- `policy_violation_candidates.json` contains only positive candidates;
- all existing S0/S3/S4/seed-runtime tests remain green.
