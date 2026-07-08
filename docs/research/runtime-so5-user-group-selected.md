# Selected Runtime Routes - So5UserGroup

Date: June 9, 2026

This subset contains all 6 `open` routes and 15 representative `unknown`
routes selected from the full 194-route S3 artifact.

Machine-readable artifact:

```text
output/runtime.so5_user_group.selected_routes.json
```

Each case includes the original `route_id`, selector state, terminal semantic
edge, and complete witness from S3.

## Open - test all 6

| Case | Selector | Witness |
|---|---|---|
| SG-O01 | `Query.node(id)` | `node -> ... on So5UserGroup` |
| SG-O02 | `Query.nodes(ids)` | `nodes -> ... on So5UserGroup` |
| SG-O03 | `Query.userGroup(joinSecret)` | `userGroup -> ... on So5UserGroup` |
| SG-O04 | `Query.userGroup(slug)` | `userGroup -> ... on So5UserGroup` |
| SG-O05 | `So5Root.so5UserGroup(joinSecret)` | `so5 -> so5UserGroup` |
| SG-O06 | `So5Root.so5UserGroup(slug)` | `so5 -> so5UserGroup` |

For each selector, compare:

- owner or group member;
- authenticated non-member;
- anonymous requester;
- `publiclyAccessible=true` and `publiclyAccessible=false` seeds.

Record whether the selected identity is returned and whether sensitive
projections such as `joinSecret`, `administrator`, and membership data differ
by requester.

## Unknown - selected 15

| Case | Behavior covered | Witness |
|---|---|---|
| SG-U01 | optional selectors omitted | `Query.userGroup()` |
| SG-U02 | featured requester-derived group | `so5.myFeaturedUniversalSo5UserGroup` |
| SG-U03 | live requester-derived group | `so5.myLiveUniversalSo5UserGroup` |
| SG-U04 | direct concrete `my` collection | `so5.mySo5UserGroups.nodes` |
| SG-U05 | direct interface `my` collection | `so5.myUserGroups.nodes -> So5UserGroup` |
| SG-U06 | direct concrete universal collection | `so5.universalSo5UserGroups.nodes` |
| SG-U07 | direct interface universal collection | `so5.universalUserGroups.nodes -> So5UserGroup` |
| SG-U08 | fixture selector to concrete `my` collection | `node(So5Fixture).mySo5UserGroups.nodes` |
| SG-U09 | fixture selector to interface `my` collection | `node(So5Fixture).myUserGroups.nodes -> So5UserGroup` |
| SG-U10 | leaderboard selector to `my` collection | `node(So5Leaderboard).mySo5UserGroups.nodes` |
| SG-U11 | leaderboard selector to universal collection | `node(So5Leaderboard).universalSo5UserGroups` |
| SG-U12 | group-membership-group round-trip | `userGroup(slug).membership(userSlug).so5UserGroup` |
| SG-U13 | membership lookup to parent group | `so5UserGroupMembership(id).so5UserGroup` |
| SG-U14 | global-ID membership to parent group | `node(So5UserGroupMembership).so5UserGroup` |
| SG-U15 | global-ID notification to parent group | `node(So5UserGroupNotification).so5UserGroup` |

## Main questions

1. Do `my*` fields reset identity to the current requester?
2. Do `universal*` fields contain only public or system-managed groups?
3. Does selecting a fixture or leaderboard control the final group set?
4. Does `membership -> so5UserGroup` preserve a stable parent identity?
5. Are membership and notification types genuinely supported by `node()`?
6. Can a private group or its `joinSecret` be projected by a non-member?

## Request batching

The 15 unknown routes can be covered by fewer GraphQL operations:

- SG-U02 and SG-U03 can share one `so5` query.
- SG-U04 through SG-U07 can share one direct collection query.
- SG-U08 and SG-U09 can share one fixture query.
- SG-U10 and SG-U11 can share one leaderboard query.
- SG-O03 and SG-O04 can be aliases in one query.
- SG-O05 and SG-O06 can be aliases in one query.

Keep one result object per `case_id` even when multiple cases share a request.
This preserves the join back to the S3 `route_id`.

## Result fields

At minimum, record:

```json
{
  "case_id": "SG-U01",
  "route_id": "route:sha256:...",
  "requester": "owner|member|authenticated_non_member|anonymous",
  "seed_visibility": "public|private|unknown",
  "runtime_feasibility": "confirmed|contradicted",
  "target_reached": true,
  "returned_ids": [],
  "selector_controls_sink": "yes|no|unknown",
  "authorization_result": "allowed|denied|not_tested",
  "graphql_errors": [],
  "notes": ""
}
```

