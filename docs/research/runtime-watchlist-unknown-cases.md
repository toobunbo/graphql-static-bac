# Runtime Checks: Watchlist Unknown Routes

## Scope

Bon case duoi day dai dien cho bon cau hoi semantic khac nhau ma schema khong
the tu tra loi:

1. mot relation round-trip co tra lai dung object ban dau khong;
2. `possible_types` structural co thuc su kha thi voi resolver cu the khong;
3. selector cua object trung gian co dieu khien sink lien ket khong;
4. transition sang `CurrentUser` co phai hidden self-scope boundary khong.

Tat ca probe la Query/read-only. Khong can test het moi combination ngay tu dau.
Moi case nen co it nhat hai seed hop le neu co the.

Sau khi test, dien JSON trong muc `Runtime result template`. Giu nguyen
`case_id` va `route_id` de join truc tiep voi S3 output.

---

## U01 - Watchlist subscription round-trip identity

### Static route

```json
{
  "route_id": "route:sha256:a54db9ffeef0260f7508f964c7faa60807936179de4cc07030898c7e116cfa48",
  "target_type_id": "type:Watchlist",
  "origin": "traversal",
  "verdict": "unknown",
  "selector": {
    "arg_ref": "arg:MarketRoot.watchlist.id",
    "root_arg_ref": "arg:MarketRoot.watchlist.id",
    "arg_path": "MarketRoot.watchlist.id",
    "input_path": [],
    "type_ref": {
      "display": "ID!",
      "named_type": "ID",
      "named_kind": "SCALAR",
      "wrappers": ["NON_NULL"]
    },
    "classification": "object_selector",
    "confidence": "high",
    "selected_type_id": "type:Watchlist"
  },
  "selector_continuity": "unknown",
  "terminal_semantic_edge_id": "field:EmailSubscription.anySubscribable",
  "boundaries": [],
  "signature": {
    "origin": "traversal",
    "selector_ref": "arg:MarketRoot.watchlist.id",
    "terminal_semantic_edge_id": "field:EmailSubscription.anySubscribable",
    "boundary_families": [],
    "selector_continuity": "unknown",
    "verdict": "unknown"
  },
  "witness": {
    "witness_id": "cap:sha256:9203f922e221e01b9397f376eda96d16f1dc51c141916762d649a693a167cc97",
    "entry_field_id": "field:Query.market",
    "edges": [
      {
        "edge_id": "field:Query.market",
        "kind": "FIELD",
        "source_type_id": "type:Query",
        "field_id": "field:Query.market",
        "target_type_id": "type:MarketRoot"
      },
      {
        "edge_id": "field:MarketRoot.watchlist",
        "kind": "FIELD",
        "source_type_id": "type:MarketRoot",
        "field_id": "field:MarketRoot.watchlist",
        "target_type_id": "type:Watchlist"
      },
      {
        "edge_id": "field:Watchlist.currentUserSubscription",
        "kind": "FIELD",
        "source_type_id": "type:Watchlist",
        "field_id": "field:Watchlist.currentUserSubscription",
        "target_type_id": "type:EmailSubscription"
      },
      {
        "edge_id": "field:EmailSubscription.anySubscribable",
        "kind": "FIELD",
        "source_type_id": "type:EmailSubscription",
        "field_id": "field:EmailSubscription.anySubscribable",
        "target_type_id": "type:WithSubscriptionsInterface"
      },
      {
        "edge_id": "type_condition:WithSubscriptionsInterface->Watchlist",
        "kind": "TYPE_CONDITION",
        "source_type_id": "type:WithSubscriptionsInterface",
        "field_id": null,
        "target_type_id": "type:Watchlist"
      }
    ],
    "field_hop_count": 4,
    "display_projection": "Query.market -> MarketRoot.watchlist -> Watchlist.currentUserSubscription -> EmailSubscription.anySubscribable -> ... on Watchlist"
  }
}
```

### Can xac minh

Voi mot Watchlist ma account hien tai da subscribe:

- `currentUserSubscription` co non-null khong;
- `anySubscribable.__typename` co phai `Watchlist` khong;
- `outer Watchlist.id` co bang `inner Watchlist.id` khong;
- lap lai voi it nhat hai Watchlist ID.

Neu `outer_id == inner_id` tren moi mau, day la relation round-trip
identity-preserving:

```text
Watchlist -> currentUserSubscription -> anySubscribable -> same Watchlist
```

Framework feedback:

- co the them relation policy `identity_round_trip`;
- route nay co the nang `unknown -> open`;
- visibility boundary da gap truoc round-trip co the duoc giu.

### Seed query

Dung query nay de lay Watchlist ma account da subscribe:

```graphql
query WatchlistSubscriptionSeeds {
  currentUser {
    mySubscriptions(first: 50, types: [WATCHLIST]) {
      nodes {
        id
        anySubscribable {
          __typename
          slug
          ... on Watchlist {
            id
            slug
          }
        }
      }
    }
  }
}
```

### Probe query

```graphql
query WatchlistRoundTrip($id: ID!) {
  market {
    watchlist(id: $id) {
      id
      slug
      currentUserSubscription {
        id
        anySubscribable {
          __typename
          slug
          ... on Watchlist {
            id
            slug
          }
        }
      }
    }
  }
}
```

### Runtime result template

```json
{
  "case_id": "watchlist-u01-round-trip",
  "route_id": "route:sha256:a54db9ffeef0260f7508f964c7faa60807936179de4cc07030898c7e116cfa48",
  "requester": null,
  "samples": [
    {
      "input_watchlist_id": null,
      "outer_id": null,
      "outer_slug": null,
      "subscription_id": null,
      "inner_typename": null,
      "inner_id": null,
      "inner_slug": null,
      "same_id": null,
      "same_slug": null,
      "graphql_errors": []
    }
  ],
  "observations": {
    "samples_tested": 0,
    "subscription_non_null_count": 0,
    "inner_watchlist_count": 0,
    "same_id_count": 0,
    "different_id_count": 0
  },
  "conclusion": {
    "runtime_reachable": null,
    "identity_relation": "unknown",
    "selector_controls_sink": null,
    "recommended_verdict": "unknown",
    "recommended_framework_action": null
  }
}
```

---

## U02 - Correlated possible type from `anyCard`

### Static route

```json
{
  "route_id": "route:sha256:b8e4f6abe5965c7ddd78796a1e50e9761442da6e7f98d13952fad4003488134f",
  "target_type_id": "type:Watchlist",
  "origin": "traversal",
  "verdict": "unknown",
  "selector": {
    "arg_ref": "arg:Query.anyCard.assetId",
    "root_arg_ref": "arg:Query.anyCard.assetId",
    "arg_path": "Query.anyCard.assetId",
    "input_path": [],
    "type_ref": {
      "display": "String",
      "named_type": "String",
      "named_kind": "SCALAR",
      "wrappers": []
    },
    "classification": "object_selector",
    "confidence": "high",
    "selected_type_id": "type:AnyCardInterface"
  },
  "selector_continuity": "unknown",
  "terminal_semantic_edge_id": "field:EmailSubscription.anySubscribable",
  "boundaries": [],
  "signature": {
    "origin": "traversal",
    "selector_ref": "arg:Query.anyCard.assetId",
    "terminal_semantic_edge_id": "field:EmailSubscription.anySubscribable",
    "boundary_families": [],
    "selector_continuity": "unknown",
    "verdict": "unknown"
  },
  "witness": {
    "witness_id": "cap:sha256:f6fba8db5cf4b3c98911dce3963f0e86d2d6b2a341f78691875fe3f681583405",
    "entry_field_id": "field:Query.anyCard",
    "edges": [
      {
        "edge_id": "field:Query.anyCard",
        "kind": "FIELD",
        "source_type_id": "type:Query",
        "field_id": "field:Query.anyCard",
        "target_type_id": "type:AnyCardInterface"
      },
      {
        "edge_id": "field:AnyCardInterface.currentUserSubscription",
        "kind": "FIELD",
        "source_type_id": "type:AnyCardInterface",
        "field_id": "field:AnyCardInterface.currentUserSubscription",
        "target_type_id": "type:EmailSubscription"
      },
      {
        "edge_id": "field:EmailSubscription.anySubscribable",
        "kind": "FIELD",
        "source_type_id": "type:EmailSubscription",
        "field_id": "field:EmailSubscription.anySubscribable",
        "target_type_id": "type:WithSubscriptionsInterface"
      },
      {
        "edge_id": "type_condition:WithSubscriptionsInterface->Watchlist",
        "kind": "TYPE_CONDITION",
        "source_type_id": "type:WithSubscriptionsInterface",
        "field_id": null,
        "target_type_id": "type:Watchlist"
      }
    ],
    "field_hop_count": 3,
    "display_projection": "Query.anyCard -> AnyCardInterface.currentUserSubscription -> EmailSubscription.anySubscribable -> ... on Watchlist"
  }
}
```

Companion selector co cung witness:

```json
{
  "route_id": "route:sha256:440f1b92cc6730335dfe4eeeeaa7b78cf75794293e4ec9326ade009cfb6eb27e",
  "selector_ref": "arg:Query.anyCard.slug"
}
```

### Can xac minh

Voi Card ma account da subscribe, test rieng `assetId` va `slug`:

- outer `anyCard.__typename`, `id`, `assetId`, `slug`;
- inner `anySubscribable.__typename`, `slug`;
- inner co bao gio la `Watchlist` khong;
- neu inner cung concrete Card type, slug/identity co khop outer khong.

Hai ket qua quan trong:

1. Inner luon la chinh Card ban dau:
   `... on Watchlist` khong kha thi cho edge sequence nay. Framework can
   relation-aware possible-type narrowing.
2. Inner co the la Watchlist:
   route structural la runtime-feasible; tiep tuc xac minh assetId/slug co
   dieu khien Watchlist nao duoc tra ve.

### Probe query

Chi truyen mot trong hai variable moi lan.

```graphql
query AnyCardSubscriptionTarget($assetId: String, $slug: String) {
  anyCard(assetId: $assetId, slug: $slug) {
    __typename
    id
    assetId
    slug
    currentUserSubscription {
      id
      anySubscribable {
        __typename
        slug
        ... on Watchlist {
          id
          slug
        }
      }
    }
  }
}
```

### Runtime result template

```json
{
  "case_id": "watchlist-u02-any-card-correlated-type",
  "route_ids": [
    "route:sha256:b8e4f6abe5965c7ddd78796a1e50e9761442da6e7f98d13952fad4003488134f",
    "route:sha256:440f1b92cc6730335dfe4eeeeaa7b78cf75794293e4ec9326ade009cfb6eb27e"
  ],
  "requester": null,
  "samples": [
    {
      "selector": "assetId",
      "selector_value": null,
      "outer_typename": null,
      "outer_id": null,
      "outer_asset_id": null,
      "outer_slug": null,
      "subscription_id": null,
      "inner_typename": null,
      "inner_id": null,
      "inner_slug": null,
      "same_concrete_type": null,
      "same_slug": null,
      "graphql_errors": []
    }
  ],
  "observations": {
    "samples_tested": 0,
    "subscription_non_null_count": 0,
    "inner_watchlist_count": 0,
    "inner_matches_outer_type_count": 0,
    "inner_matches_outer_slug_count": 0
  },
  "conclusion": {
    "runtime_reachable_as_watchlist": null,
    "correlated_possible_type": "unknown",
    "selector_controls_sink": null,
    "recommended_verdict": "unknown",
    "recommended_framework_action": null
  }
}
```

---

## U03 - Subscription global ID controls associated Watchlist

### Static route

```json
{
  "route_id": "route:sha256:05b697c2729135202ccd53132e68e6a2ce6603af6188e8f5449b5d22a6511c56",
  "target_type_id": "type:Watchlist",
  "origin": "traversal",
  "verdict": "unknown",
  "selector": {
    "arg_ref": "arg:Query.node.id",
    "root_arg_ref": "arg:Query.node.id",
    "arg_path": "Query.node.id",
    "input_path": [],
    "type_ref": {
      "display": "ID!",
      "named_type": "ID",
      "named_kind": "SCALAR",
      "wrappers": ["NON_NULL"]
    },
    "classification": "object_selector",
    "confidence": "high",
    "selected_type_id": "type:Node"
  },
  "selector_continuity": "unknown",
  "terminal_semantic_edge_id": "field:EmailSubscription.anySubscribable",
  "boundaries": [],
  "signature": {
    "origin": "traversal",
    "selector_ref": "arg:Query.node.id",
    "terminal_semantic_edge_id": "field:EmailSubscription.anySubscribable",
    "boundary_families": [],
    "selector_continuity": "unknown",
    "verdict": "unknown"
  },
  "witness": {
    "witness_id": "cap:sha256:cc9cdf1cec6e6831545a7a4a622ccd9e3199b1496bf22d35fb0bb662f89d6a6d",
    "entry_field_id": "field:Query.node",
    "edges": [
      {
        "edge_id": "field:Query.node",
        "kind": "FIELD",
        "source_type_id": "type:Query",
        "field_id": "field:Query.node",
        "target_type_id": "type:Node"
      },
      {
        "edge_id": "type_condition:Node->EmailSubscription",
        "kind": "TYPE_CONDITION",
        "source_type_id": "type:Node",
        "field_id": null,
        "target_type_id": "type:EmailSubscription"
      },
      {
        "edge_id": "field:EmailSubscription.anySubscribable",
        "kind": "FIELD",
        "source_type_id": "type:EmailSubscription",
        "field_id": "field:EmailSubscription.anySubscribable",
        "target_type_id": "type:WithSubscriptionsInterface"
      },
      {
        "edge_id": "type_condition:WithSubscriptionsInterface->Watchlist",
        "kind": "TYPE_CONDITION",
        "source_type_id": "type:WithSubscriptionsInterface",
        "field_id": null,
        "target_type_id": "type:Watchlist"
      }
    ],
    "field_hop_count": 2,
    "display_projection": "Query.node -> ... on EmailSubscription -> EmailSubscription.anySubscribable -> ... on Watchlist"
  }
}
```

### Can xac minh

Lay it nhat hai `EmailSubscription.id` co `anySubscribable.__typename ==
Watchlist`, sau do goi `node(id:)` bang tung subscription ID:

- `node.__typename` co la `EmailSubscription` khong;
- `anySubscribable` co tra dung Watchlist cua seed subscription khong;
- thay subscription ID co lam Watchlist sink thay doi tuong ung khong;
- neu co account thu hai, subscription ID cua account A co doc duoc boi B hay
  anonymous khong.

Case nay khong hoi "same object identity" vi EmailSubscription va Watchlist la
hai object khac nhau. No hoi relation co functional/deterministic khong:

```text
EmailSubscription ID -> exactly one associated Watchlist
```

Neu co, selector thuc su dieu khien sink qua mot derived relation. Framework can
them continuity/provenance `derived` hoac `functional_relation`, thay vi de
chung vao `unknown`.

### Seed query

```graphql
query WatchlistSubscriptionNodeSeeds {
  currentUser {
    mySubscriptions(first: 50, types: [WATCHLIST]) {
      nodes {
        id
        anySubscribable {
          __typename
          ... on Watchlist {
            id
            slug
          }
        }
      }
    }
  }
}
```

### Probe query

```graphql
query WatchlistFromSubscriptionNode($id: ID!) {
  node(id: $id) {
    __typename
    id
    ... on EmailSubscription {
      slug
      anySubscribable {
        __typename
        slug
        ... on Watchlist {
          id
          slug
        }
      }
    }
  }
}
```

### Runtime result template

```json
{
  "case_id": "watchlist-u03-subscription-node-derived-selector",
  "route_id": "route:sha256:05b697c2729135202ccd53132e68e6a2ce6603af6188e8f5449b5d22a6511c56",
  "samples": [
    {
      "seed_owner": null,
      "requester": null,
      "subscription_id": null,
      "expected_watchlist_id": null,
      "node_typename": null,
      "returned_watchlist_id": null,
      "returned_watchlist_slug": null,
      "matches_seed_watchlist": null,
      "graphql_errors": []
    }
  ],
  "observations": {
    "samples_tested": 0,
    "node_resolved_count": 0,
    "watchlist_returned_count": 0,
    "matches_seed_count": 0,
    "cross_account_readable_count": 0,
    "anonymous_readable_count": 0
  },
  "conclusion": {
    "runtime_reachable": null,
    "relation_is_functional": null,
    "selector_controls_sink": null,
    "authorization_boundary": "unknown",
    "recommended_verdict": "unknown",
    "recommended_framework_action": null
  }
}
```

---

## U04 - Hidden self-scope through subscription subscriber

### Static route

```json
{
  "route_id": "route:sha256:6aa6bed9b8448d844c3c6f11509f4be19cf83b51f2a7771e3ed2b25a53de2527",
  "target_type_id": "type:Watchlist",
  "origin": "traversal",
  "verdict": "unknown",
  "selector": {
    "arg_ref": "arg:MarketRoot.watchlist.id",
    "root_arg_ref": "arg:MarketRoot.watchlist.id",
    "arg_path": "MarketRoot.watchlist.id",
    "input_path": [],
    "type_ref": {
      "display": "ID!",
      "named_type": "ID",
      "named_kind": "SCALAR",
      "wrappers": ["NON_NULL"]
    },
    "classification": "object_selector",
    "confidence": "high",
    "selected_type_id": "type:Watchlist"
  },
  "selector_continuity": "unknown",
  "terminal_semantic_edge_id": "field:CurrentUser.myWatchlists",
  "boundaries": [],
  "signature": {
    "origin": "traversal",
    "selector_ref": "arg:MarketRoot.watchlist.id",
    "terminal_semantic_edge_id": "field:CurrentUser.myWatchlists",
    "boundary_families": [],
    "selector_continuity": "unknown",
    "verdict": "unknown"
  },
  "witness": {
    "witness_id": "cap:sha256:a0d384bb4ca6fb572959b9114f16790c0c1faa75da1eb45092a761bbce3b0f16",
    "entry_field_id": "field:Query.market",
    "edges": [
      {
        "edge_id": "field:Query.market",
        "kind": "FIELD",
        "source_type_id": "type:Query",
        "field_id": "field:Query.market",
        "target_type_id": "type:MarketRoot"
      },
      {
        "edge_id": "field:MarketRoot.watchlist",
        "kind": "FIELD",
        "source_type_id": "type:MarketRoot",
        "field_id": "field:MarketRoot.watchlist",
        "target_type_id": "type:Watchlist"
      },
      {
        "edge_id": "field:Watchlist.currentUserSubscription",
        "kind": "FIELD",
        "source_type_id": "type:Watchlist",
        "field_id": "field:Watchlist.currentUserSubscription",
        "target_type_id": "type:EmailSubscription"
      },
      {
        "edge_id": "field:EmailSubscription.subscriber",
        "kind": "FIELD",
        "source_type_id": "type:EmailSubscription",
        "field_id": "field:EmailSubscription.subscriber",
        "target_type_id": "type:Subscriber"
      },
      {
        "edge_id": "type_condition:Subscriber->CurrentUser",
        "kind": "TYPE_CONDITION",
        "source_type_id": "type:Subscriber",
        "field_id": null,
        "target_type_id": "type:CurrentUser"
      },
      {
        "edge_id": "field:CurrentUser.myWatchlists",
        "kind": "FIELD",
        "source_type_id": "type:CurrentUser",
        "field_id": "field:CurrentUser.myWatchlists",
        "target_type_id": "type:Watchlist"
      }
    ],
    "field_hop_count": 5,
    "display_projection": "Query.market -> MarketRoot.watchlist -> Watchlist.currentUserSubscription -> EmailSubscription.subscriber -> ... on CurrentUser -> CurrentUser.myWatchlists -> Watchlist"
  }
}
```

### Can xac minh

Dung hai Watchlist khac nhau ma cung requester da subscribe:

- `subscriber.__typename` co luon la `CurrentUser` khong;
- `subscriber.id` co luon bang `currentUser.id` khong;
- khi thay input Watchlist ID, subscriber ID co thay doi khong;
- tap `subscriber.myWatchlists` co thay doi theo input Watchlist ID khong;
- hash/sorted ID set cua `subscriber.myWatchlists` co bang
  `Query.currentUser.myWatchlists` khong.

Neu subscriber luon la current requester va output `myWatchlists` khong phu
thuoc selector Watchlist ban dau:

- selector continuity thuc su bi cat;
- `EmailSubscription.subscriber -> CurrentUser` la hidden self-scope boundary;
- route nen chuyen `unknown -> guarded/self_scope`.

Neu subscriber co the thay doi theo Watchlist ID va mo ra account context khac,
day la route can uu tien cao vi selector co the dieu khien account-level sink.

### Probe query

Chay query hai lan voi hai Watchlist ID, hoac alias hai ID trong cung request.

```graphql
query SubscriberScope($id: ID!, $sport: Sport!) {
  currentUser {
    id
    baseline: myWatchlists(sport: $sport) {
      id
      slug
    }
  }
  market {
    watchlist(id: $id) {
      id
      slug
      currentUserSubscription {
        id
        subscriber {
          __typename
          ... on CurrentUser {
            id
            selected: myWatchlists(sport: $sport) {
              id
              slug
            }
          }
        }
      }
    }
  }
}
```

### Runtime result template

```json
{
  "case_id": "watchlist-u04-subscriber-self-scope",
  "route_id": "route:sha256:6aa6bed9b8448d844c3c6f11509f4be19cf83b51f2a7771e3ed2b25a53de2527",
  "requester": null,
  "samples": [
    {
      "input_watchlist_id": null,
      "input_watchlist_slug": null,
      "current_user_id": null,
      "subscriber_typename": null,
      "subscriber_id": null,
      "subscriber_is_current_user": null,
      "baseline_watchlist_ids": [],
      "selected_watchlist_ids": [],
      "same_watchlist_id_set": null,
      "graphql_errors": []
    }
  ],
  "observations": {
    "samples_tested": 0,
    "subscriber_current_user_count": 0,
    "subscriber_changed_with_input_count": 0,
    "same_watchlist_set_count": 0,
    "different_watchlist_set_count": 0
  },
  "conclusion": {
    "runtime_reachable": null,
    "selector_controls_subscriber": null,
    "selector_controls_sink": null,
    "hidden_boundary": "unknown",
    "recommended_verdict": "unknown",
    "recommended_framework_action": null
  }
}
```

---

## Result interpretation

| Case | Observation | Framework implication |
| --- | --- | --- |
| U01 | outer and inner Watchlist IDs always equal | Add identity round-trip rule; route may become `open` |
| U01 | IDs differ | Keep `unknown`; selector-to-final-sink relation needs more analysis |
| U02 | inner always same Card type/object | Narrow correlated possible types; Watchlist branch is runtime-infeasible for this edge sequence |
| U02 | inner can be Watchlist | Keep route and assess whether `assetId`/`slug` controls returned Watchlist |
| U03 | subscription ID deterministically selects associated Watchlist | Add derived/functional selector continuity |
| U03 | cross-account/anonymous read blocked | Attach authorization boundary evidence without removing structural route |
| U04 | subscriber always equals current requester | Add self-scope policy for `EmailSubscription.subscriber -> CurrentUser`; verdict becomes `guarded` |
| U04 | subscriber changes with selected Watchlist | Selector may control account context; prioritize for deeper BAC testing |

