# Runtime Plan - So5UserGroup

Date: June 9, 2026

## Why this target

`So5UserGroup` is a useful second runtime anchor because it:

- implements `Node`;
- exposes `publiclyAccessible: Boolean!`;
- supports direct lookup by `joinSecret` or `slug`;
- appears in requester-scoped `my*` collections and broader `universal*`
  collections;
- exposes fields such as `joinSecret`, `administrator`, `memberships`, and
  `membership(userSlug:)`, making identity and authorization differences
  observable.

This target is independent of the Watchlist behavior while retaining a clear
public/private distinction.

## Static inputs

- Schema IR: `output/schema_ir.introspection.json`
- Selector facts: `output/args.so5_user_group.calibration.json`
- Route policy: `config/profiles/route-analysis-v1.json`
- Full output: `output/routes.so5_user_group.json`

Command:

```bash
cargo run --quiet -- route \
  --schema-ir output/schema_ir.introspection.json \
  --args output/args.so5_user_group.calibration.json \
  --policy config/profiles/route-analysis-v1.json \
  --target So5UserGroup \
  --output output/routes.so5_user_group.json
```

## Static result

```json
{
  "target": "type:So5UserGroup",
  "reachability": "reachable",
  "best_verdict": "open",
  "route_count": 194,
  "verdict_counts": {
    "open": 6,
    "unknown": 88,
    "guarded": 100
  }
}
```

The result is deterministic. Two consecutive runs produced the same SHA-256:

```text
0fce786df9575950bf61b44f736e288ca84b50e7a3d7b79455332d8643f41d75
```

## Open routes

All six `open` routes are direct identity selections:

| Selector | Witness |
|---|---|
| `Query.node(id)` | `Query.node -> ... on So5UserGroup` |
| `Query.nodes(ids)` | `Query.nodes -> ... on So5UserGroup` |
| `Query.userGroup(joinSecret)` | `Query.userGroup -> ... on So5UserGroup` |
| `Query.userGroup(slug)` | `Query.userGroup -> ... on So5UserGroup` |
| `So5Root.so5UserGroup(joinSecret)` | `Query.so5 -> So5Root.so5UserGroup` |
| `So5Root.so5UserGroup(slug)` | `Query.so5 -> So5Root.so5UserGroup` |

Runtime must determine whether a selector resolves the requested group for:

- the owner or a member;
- an authenticated non-member;
- an anonymous requester;
- both `publiclyAccessible=true` and `publiclyAccessible=false` groups.

`open` means selector continuity is statically proven. It does not mean an
authorization issue exists.

## High-value unknown routes

Test every route in the full artifact. Prioritize these direct families first:

```text
Query.so5 -> So5Root.myFeaturedUniversalSo5UserGroup
Query.so5 -> So5Root.myLiveUniversalSo5UserGroup
Query.so5 -> So5Root.mySo5UserGroups -> nodes
Query.so5 -> So5Root.myUserGroups -> nodes -> ... on So5UserGroup
Query.so5 -> So5Root.universalSo5UserGroups -> nodes
Query.so5 -> So5Root.universalUserGroups -> nodes -> ... on So5UserGroup
```

Questions:

- Are `my*` fields requester-scoped hidden boundaries?
- Do `universal*` fields return only publicly accessible/system groups?
- Does `publiclyAccessible=false` disappear for non-members and anonymous
  requesters?
- Are `joinSecret`, membership data, and administrator data projected
  differently according to requester authorization?

The remaining unknown routes reach the group through these semantic terminal
families:

```text
So5Fixture.mySo5UserGroups
So5Fixture.myUserGroups
So5Leaderboard.mySo5UserGroups
So5Leaderboard.myUserGroups
So5Leaderboard.universalSo5UserGroups
So5Leaderboard.universalUserGroups
So5UserGroupMembership.so5UserGroup
So5UserGroupMembership.userGroup
So5UserGroupNotification.so5UserGroup
So5UserGroupNotification.userGroup
```

For each route, verify both runtime feasibility and whether the entry selector
still controls the final `So5UserGroup` identity.

## Guarded routes

The 100 guarded routes remain in the runtime set. Their boundaries are
inherited from route prefixes, not from `So5UserGroup.publiclyAccessible`:

```json
{
  "field:Query.currentUser": 20,
  "field:User.publicWatchlists": 50,
  "field:CurrentUser.publicWatchlists": 10,
  "field:Player.publicWatchlists": 4,
  "field:StarkwarePrivateAccount.publicInfo": 16,
  "field:MarketRoot.sorareWatchlists": 10
}
```

Some routes contain more than one boundary, so the counts above are boundary
occurrences rather than route totals. A guarded route must still be executed;
runtime evidence may confirm the boundary or expose a policy misfire.

## Runtime result format

Record one result object for every `route_id` in
`output/routes.so5_user_group.json`:

```json
{
  "route_id": "route:sha256:...",
  "static_verdict": "open|unknown|guarded",
  "requester": "owner|member|authenticated_non_member|anonymous",
  "seed": {
    "group_id": "So5UserGroup:...",
    "slug": "...",
    "join_secret": "...",
    "publicly_accessible": false
  },
  "runtime": {
    "query_executed": true,
    "prefix_reachable": true,
    "target_reached": true,
    "returned_typename": "So5UserGroup",
    "returned_id": "So5UserGroup:...",
    "same_as_seed": true,
    "graphql_errors": []
  },
  "observed_projection": {
    "join_secret_visible": true,
    "administrator_visible": true,
    "memberships_visible": true
  },
  "classification": {
    "runtime_feasibility": "confirmed|contradicted",
    "selector_controls_sink": "yes|no|unknown",
    "authorization_result": "allowed|denied|not_tested",
    "recommended_static_policy": "none"
  },
  "notes": ""
}
```

Do not rewrite structural reachability from runtime results. Runtime
feasibility and authorization are independent annotations.

