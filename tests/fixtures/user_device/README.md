# UserDevice Golden Trace

This fixture is a schema subset derived from `introspection.json`. It fixes the
expected JSON contract for one target across S0-S5.

## S0: Schema IR

Relevant structural facts:

```text
Query.node(id: ID!)              -> Node
Query.nodes(ids: [ID!]!)         -> [Node]!
Query.currentUser                -> CurrentUser
CurrentUser.devices              -> [UserDevice!]
CurrentUser.currentDevice        -> UserDevice
Node TYPE_CONDITION              -> UserDevice
```

`UserDevice` implements `Node`. The fixture keeps full TypeRefs, including list
and nullability wrappers.

## S1: Sink Classification

`UserDevice` is selected for two independent reasons:

- It is globally addressable through the `Node` interface.
- `deviceType`, `os`, `lastUsedAt`, and `userAgent` provide device, activity,
  and PII evidence.

S1 emits one type sink and four field sinks. S3 analyzes the target type only
once and attaches all five sink refs to its route report.

## S2: Argument Classification

```text
Query.node.id    -> [object_selector]
Query.nodes.ids  -> [object_selector]
```

The `classifications` field is an array because a future argument such as
`viewAsUserId` may be both an object selector and an authz modifier.

## S3/v2: Route Analysis

`s3_routes.json` is the current production golden artifact:

```text
Query.node(id:)                 -> open / global_id
Query.nodes(ids:)               -> open / global_id
Query.currentUser.devices       -> guarded / self_scope
Query.currentUser.currentDevice -> guarded / self_scope
```

It pins route signatures, selector provenance, verdicts, deterministic
witnesses, and the `route-analysis-v1` policy fingerprint.

## S3/v1: Legacy Structural CAPs

Four independent CAPs are emitted:

```text
Query.node -> TYPE_CONDITION(UserDevice)
Query.nodes -> TYPE_CONDITION(UserDevice)
Query.currentUser -> CurrentUser.devices
Query.currentUser -> CurrentUser.currentDevice
```

`s3_paths.json` is retained as the diagnostic enumerator oracle. It contains no
selectors, boundaries, flow, or score. Its CAP IDs are SHA-256 hashes of target
type plus ordered edge IDs.

## S4/v1: Legacy Annotation And Score

```text
node/current path          flow               score
Query.node                 direct_global_id    6.0
Query.nodes                direct_global_id    6.0
currentUser.devices        no_selector        -2.0
currentUser.currentDevice  no_selector        -2.0
```

Both `currentUser` paths contain the same single self-scope boundary. The
`currentDevice` field does not create a second penalty because there is no
evidence that it establishes another authorization boundary.

## S5/v1: Legacy Final Artifacts

`s5_suspect_artifact.json` joins S1 sink metadata, S3 edges, and S4 annotations.
`s5_coverage_report.json` records fixture-local coverage counts.

Run the legacy v1 cross-stage contract check:

```bash
python3 tests/fixtures/user_device/validate.py
```

The Rust integration tests validate `s3_routes.json` as the current S3/v2
golden contract.
