# Stage 2 Argument Classifier Implementation Plan

Status: Implemented (June 14, 2026)

Implementation result:

- `stage s2` generates deterministic, policy-fingerprinted artifacts.
- Query, Mutation, Subscription, and ordinary output fields are covered.
- Recursive input-object leaves and cycles are represented explicitly.
- Existing S2 fixtures remain readable.
- Generated S2 facts preserve the synthetic Watchlist selector ground truth.
- Full Sorare S0 calibration completes in about one second and emits 6,104
  classified records across 10,030 fields.

Downstream observation:

- The full calibration contains 3,579 low-confidence
  `possible_selector` records.
- Feeding every possible selector into the current S3 product-state engine can
  be substantially slower than using the old hand-selected calibration file.
- This is an S3 scalability issue exposed by complete S2 coverage, not a reason
  to silently discard ambiguous S2 arguments.

Execution-policy update:

- S2 continues retaining `possible_selector` as audit evidence.
- The executable S3/S4 selector feed now excludes it under
  `stage-3-definite-selectors-and-route-family-dag.md`.
- Required ambiguous arguments remain available to S4 as ordinary
  literal/enum/input bindings.

## 1. Objective

Stage 2 consumes the complete `S0/v1` schema IR and emits a deterministic
`S2/v1` argument-classification artifact.

It must classify every field argument, including leaves nested inside input
objects, into facts that S3 and S4 can consume without manual
`args.*.calibration.json` files.

S2 is root-agnostic: it classifies fields belonging to Query, Mutation,
Subscription, and ordinary output types. Reachability and operation-root
selection remain responsibilities of later stages.

## 2. Classification Contract

The existing classifications remain authoritative:

- `object_selector`: strong evidence that the value selects an object or
  object set.
- `authz_modifier`: the value may change the identity or authorization context
  under which data is resolved.
- `noise`: high-confidence operational input that does not select the sink.
- `possible_selector`: insufficient evidence to safely classify the value as
  selector or noise.

The classifier is recall-first. Unknown semantic arguments are retained as
`possible_selector`; they are never silently converted to `noise`.

### 2.1 Decision Matrix

| Evidence | Classifications | Confidence |
|---|---|---|
| GraphQL `ID` or list of `ID` | `object_selector` | high |
| Exact selector name such as `id`, `slug`, `assetId` | `object_selector` | high |
| Selector suffix such as `userId`, `notificationIds` | `object_selector` | high |
| Explicit identity-like custom scalar | `object_selector` | medium |
| Exact operational noise name | `noise` | high |
| Selector and noise evidence conflict | preserve both | low |
| Authorization-context selector | `object_selector`, `authz_modifier` | high or medium |
| Unknown scalar, enum, or semantic argument | `possible_selector` | low |

Examples:

- `id: ID!` -> `object_selector/high`
- `assetId: String!` -> `object_selector/high`
- `viewAsUserId: ID!` -> `object_selector + authz_modifier/high`
- `first: Int` -> `noise/high`
- `cursor: ID` -> `object_selector + noise/low`
- `sport: Sport` -> `possible_selector/low`

`noise` excludes an argument only when it is the sole classification.

## 3. Evidence Sources

### 3.1 Type Evidence

Strong selector evidence:

- `ID` through any `NON_NULL` or `LIST` wrappers.
- Explicitly configured identity scalars such as `UUID` or `GlobalID`.

Type alone does not prove selector status for:

- `String`, numeric, Boolean, or enum values.
- Arbitrary custom scalars.
- Input objects before their leaves are inspected.

### 3.2 Name Evidence

Use tokenized identifiers, never broad substring matching.

Initial selector vocabulary:

- exact: `id`, `ids`, `uuid`, `slug`, `handle`, `assetId`
- suffix tokens: `id`, `ids`, `uuid`, `slug`
- authorization sequences: `viewAs`, `impersonatedUser`, `actingUser`

Initial definite-noise vocabulary:

- pagination: `first`, `last`, `after`, `before`, `limit`, `offset`, `page`,
  `perPage`
- ordering: `order`, `orderBy`, `sort`, `sortBy`
- presentation: `locale`, `currency`, `format`

Names such as `sport`, `status`, `category`, `type`, and `mode` are not
definite noise. They remain possible selectors unless stronger evidence exists.

### 3.3 Recursive Input Objects

For a scalar or enum root argument, emit one classified argument with an empty
`input_path`.

For an input-object argument:

1. Traverse input fields recursively, including input objects inside lists.
2. Emit one classified record per scalar or enum leaf.
3. Preserve `root_arg_ref`.
4. Set `input_path` relative to the root argument.
5. Build a canonical `arg_path`, for example
   `Query.search.filter.ownerId`.
6. Use path-local input-type cycle detection.
7. On a recursive cycle, emit an explicit `possible_selector` truncation record
   with a cycle signal instead of silently dropping the branch.

There will be no arbitrary recursion-depth cutoff.

## 4. Deterministic Identifier Tokenization

Extract the tokenizer currently embedded in `src/route/facts.rs` into a shared
module used by both S2 and S3.

It must deterministically split:

- camelCase
- PascalCase
- snake_case
- acronym boundaries
- letter/digit boundaries

The extraction must preserve existing S3 route-fact behavior.

## 5. Versioned Policy

Create:

`config/lexicons/argument-classifier-v1.json`

The policy contains:

- exact selector names
- selector suffix tokens
- authorization modifier patterns
- definite-noise names
- identity scalar names

Extend `ArgumentsData` with optional, backward-compatible metadata:

- `classifier_model`
- `policy_fingerprint`

Existing manually authored `S2/v1` fixtures without these fields must remain
readable. Newly generated artifacts always populate both fields.

Every emitted signal uses a stable rule identifier, for example:

- `type:graphql_id`
- `name:selector_suffix:id`
- `name:definite_noise:first`
- `input:cycle_truncated`

## 6. Implementation Layout

Planned modules:

- `src/identifier/mod.rs`
  - shared identifier tokenizer
- `src/argument/policy.rs`
  - policy loading, validation, and fingerprinting
- `src/argument/index.rs`
  - S0 type and input-object lookup indexes
- `src/argument/classifier.rs`
  - type/name evidence and decision matrix
- `src/argument/recursive.rs`
  - recursive input-object expansion
- `src/argument/validation.rs`
  - artifact completeness and invariant checks
- `src/stages/s2_arguments/runner.rs`
  - stage orchestration and artifact emission
- `src/cli/commands/s2.rs`
  - CLI integration

## 7. CLI

Planned command:

```bash
graphql-static-bac stage s2 \
  --schema-ir output/schema.ir.json \
  --policy config/lexicons/argument-classifier-v1.json \
  --output output/args.json
```

The runner will:

1. Read and validate the complete S0 artifact.
2. Load and validate the S2 policy.
3. Classify all field arguments.
4. Validate complete coverage.
5. Write a canonical S2 artifact using the S0 schema fingerprint.

## 8. Output Invariants

The implementation must enforce:

- Every scalar or enum field argument has exactly one emitted record.
- Every reachable input-object leaf has exactly one emitted record.
- Recursive input cycles produce an explicit truncation record.
- No classified record has an empty `classifications` list.
- Field map keys and all field/argument/type references resolve in S0.
- `root_arg_ref`, `input_path`, and `arg_path` are mutually consistent.
- Classifications and signals are sorted and deduplicated.
- Records use deterministic ordering by field, root argument, and input path.
- The same S0 bytes and policy bytes produce byte-identical S2 output.
- A sole `noise` classification can only come from a definite-noise rule.
- Mixed selector/noise evidence is retained and assigned low confidence.

## 9. Implementation Phases

### Phase 1: Contract and Shared Foundations

- Add backward-compatible classifier metadata to `ArgumentsData`.
- Extract and regression-test the shared identifier tokenizer.
- Add and validate the versioned classifier policy.
- Build S0 field, scalar, and input-object indexes.

### Phase 2: Root Argument Classification

- Implement wrapper-aware type inspection.
- Implement exact-name and token-based rules.
- Implement conflict preservation and confidence calculation.
- Classify scalar and enum root arguments.

### Phase 3: Recursive Input Classification

- Traverse nested input objects and lists.
- Emit canonical leaf paths.
- Add path-local cycle detection and explicit cycle records.
- Verify every root argument is fully accounted for.

### Phase 4: Stage Runner and CLI

- Add `stage s2`.
- Add canonical artifact writing and fingerprints.
- Add clear validation and malformed-policy errors.

### Phase 5: Calibration and Pipeline Integration

- Generate S2 artifacts for Watchlist, Deck, UserDevice, and existing fixtures.
- Remove S3/S4 reliance on manually authored calibration artifacts.
- Re-run S3 route verdicts and S4 seed plans against generated S2 data.
- Document intentional output differences before updating golden fixtures.

## 10. Test Plan

### 10.1 Unit Tests

Tokenizer:

- `assetId`
- `notificationIDs`
- `URLValue`
- `per_page`
- `card2Id`

Classification:

- `id: ID!` -> selector/high
- `ids: [ID!]!` -> selector/high
- `assetId: String!` -> selector/high
- `viewAsUserId: ID!` -> selector + authz/high
- `first: Int` -> noise/high
- `cursor: ID` -> selector + noise/low
- `sport: Sport` -> possible/low
- unknown custom scalar -> possible/low
- configured `UUID` scalar -> selector/medium

Recursive inputs:

- nested `filter.ownerId`
- list of input objects
- nullable recursive input cycle
- the same input type reached through independent branches

### 10.2 Integration Tests

Use a compact schema containing Query, Mutation, and Subscription fields to
prove S2 classifies all operation roots without reachability filtering.

Verify:

- canonical IDs and paths
- complete argument coverage
- byte-identical output across repeated runs
- policy changes alter the policy fingerprint
- old S2 fixtures without metadata still deserialize

### 10.3 Calibration Tests

Watchlist must detect selectors for at least:

- `Query.node(id:)`
- `Query.nodes(ids:)`
- `Query.anyCard(assetId:)`
- `Market.watchlist(id:)`

Additional anchors:

- UserDevice global-ID routes
- Deck `slug` and parent selectors
- So5Lineup `id`

After replacing manual S2 fixtures:

- Existing S3 and S4 tests must pass.
- Definite open routes must not be demoted by S2.
- Unknown semantic arguments must remain visible as possible selectors.

### 10.4 Full-Schema Checks

- Report total root arguments and expanded input leaves.
- Report counts by classification and confidence.
- Assert zero silently unclassified arguments.
- Audit all sole-noise classifications.
- Measure runtime and artifact size.

## 11. Acceptance Criteria

S2 is complete when:

1. A full S0 artifact can be converted to S2 with one CLI command.
2. All field arguments and recursive input leaves are classified.
3. Output is deterministic and policy-fingerprinted.
4. Existing S3/S4 consumers accept generated S2 output.
5. Manual argument calibration files are no longer required for normal runs.
6. Query, Mutation, and Subscription arguments are already present for future
   multi-root S3 support.
7. Ambiguous semantics remain recall-safe as `possible_selector`.

## 12. Non-Goals

This stage will not:

- infer resolver implementation or authorization behavior
- determine whether a route is vulnerable
- use runtime responses
- use an LLM or network service
- invent values for arguments
- rank routes or generate executable queries
- treat arbitrary enum or Boolean arguments as definite noise
