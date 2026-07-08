# Watchlist UNKNOWN Runtime Simulation

Date: 2026-06-13

Scope:

- target type: `Watchlist`;
- auth context: Account B;
- routes: 12 S3 `unknown` routes;
- operations: read-only;
- purpose: seed and route executability validation only;
- security oracle: not applied.

The machine-readable artifact, including reusable seed values, is:

```text
output/runtime-watchlist-unknown.json
```

## Summary

| Result | Routes |
|---|---:|
| Reached `Watchlist` | 4 |
| Prefix executed, but did not reach `Watchlist` | 4 |
| Runtime rejected | 4 |
| Total | 12 |

The four successful routes produced 20 Watchlist occurrences: each route
returned the same five personal Watchlists owned by Account B.

## Seed Coverage

### Watchlist

`currentUser.myWatchlists` returned all five personal Watchlists:

| Property | Count |
|---|---:|
| Total | 5 |
| `public: true` | 3 |
| `public: false` | 2 |

All five seeds were validated successfully:

- `nodes(ids:)` resolved all five from GraphQL global IDs;
- `market.watchlist(id:)` resolved all five from raw UUIDs;
- no null result or GraphQL error occurred.

The required representation is consumer-specific:

```text
Query.node / Query.nodes        Watchlist:<uuid>
MarketRoot.watchlist            <uuid>
```

### Card

`currentUser.cards(first: 100, ownedByMe: true)` reported and returned 28
Cards. `pageInfo.hasNextPage` was false, so coverage was complete for Account
B.

- 28/28 supplied both `assetId` and `slug`;
- one Card had a `currentUserSubscription`;
- that subscription's `anySubscribable` was `Card`;
- zero Card subscriptions pointed to `Watchlist`.

The directly selected S4 producers `anyCard`, `anyCards`, and `cardsWhere`
were rejected at runtime because their resolvers require selectors that the
schema declares nullable. `currentUser.cards` was a valid producer but was
not selected into the capped binding-set alternatives.

### AnnouncementNotification

The first notification page contained six `AnnouncementNotification` IDs.
The selected ID was accepted by both `node` and `nodes`.

### EmailSubscription

One subscription ID was obtained from the subscribed Card. Its returned form
was `Subscription:<uuid>`, and its target was `Card`.

The following representations were all rejected by `node` and `nodes`:

```text
Subscription:<uuid>
EmailSubscription:<uuid>
<uuid>
```

No Watchlist-linked `EmailSubscription` seed was found in all five personal
Watchlists, the first 20 market public Watchlists, the Sorare Watchlist, or
all 28 owned Cards.

## Route Results

| # | Route | Result | Runtime reason |
|---|---|---|---|
| 1 | `market.watchlist -> subscription -> subscriber -> myWatchlists` | Prefix only | Selected Watchlist had no current-user subscription |
| 2 | `anyCard(assetId) -> subscription -> subscriber -> myWatchlists` | Reached Watchlist | Returned all five personal Watchlists |
| 3 | `anyCard(slug) -> subscription -> subscriber -> myWatchlists` | Reached Watchlist | Returned all five personal Watchlists |
| 4 | `node(AnnouncementNotification) -> user -> myWatchlists` | Reached Watchlist | Returned all five personal Watchlists |
| 5 | `nodes(AnnouncementNotification) -> user -> myWatchlists` | Reached Watchlist | Returned all five personal Watchlists |
| 6 | `anyCard(no selector) -> subscriber -> myWatchlists` | Rejected | Runtime requires `slug` or `assetId` |
| 7 | `market.watchlist -> subscription -> anySubscribable -> Watchlist` | Prefix only | Selected Watchlist had no current-user subscription |
| 8 | `anyCard(assetId) -> subscription -> anySubscribable -> Watchlist` | Prefix only | Actual subscription target was `Card` |
| 9 | `anyCard(slug) -> subscription -> anySubscribable -> Watchlist` | Prefix only | Actual subscription target was `Card` |
| 10 | `node(EmailSubscription) -> anySubscribable -> Watchlist` | Rejected | Returned subscription ID is not accepted by `node` |
| 11 | `nodes(EmailSubscription) -> anySubscribable -> Watchlist` | Rejected | Returned subscription ID is not accepted by `nodes` |
| 12 | `anyCard(no selector) -> anySubscribable -> Watchlist` | Rejected | Runtime requires `slug` or `assetId` |

## Framework Implications

1. Phase 2 needs a consumer-specific representation adapter. GraphQL `ID`
   type compatibility alone is insufficient.
2. A producer must be ranked using runtime history. The valid
   `currentUser.cards` producer should outrank repeatedly rejected root
   producers on later executions.
3. Nullable schema arguments may still be resolver-required. A runtime
   rejection should annotate the field contract and prevent retrying the same
   no-selector operation.
4. `Node.possible_types` proves structural reachability, not runtime
   addressability. Phase 2 must record an addressability result per concrete
   type and ID codec.
5. A valid seed for the entry field does not prove the route reaches the
   requested target type. Runtime lineage and type-condition matching remain
   necessary.

