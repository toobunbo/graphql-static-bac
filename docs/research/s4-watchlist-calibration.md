# S4 Seed Planning - Watchlist Calibration

**Date:** June 13, 2026  
**Scope:** Static planning only; no endpoint or runtime validation was used.

## Inputs

```text
output/schema_ir.introspection.json
output/args.watchlist.calibration.json
output/routes.watchlist.json
```

Output:

```text
output/seed-plans.watchlist.json
```

## Summary

| Metric | Result |
|---|---:|
| S3 routes | 89 |
| Open / unknown / guarded | 3 / 12 / 74 |
| Global-ID / traversal | 2 / 87 |
| Routes with an executable S4 plan | 89 |
| Routes unresolved | 0 |
| Requirements | 115 |
| Binding-set plans | 1,042 |
| Producer jobs | 2,344 |
| Unique producer jobs | 526 |
| Emitted operations | 2,194 |
| Unique operations | 487 |
| Correlation constraints | 0 |
| Output size | 6.4 MB |
| Runtime | 3.51 s |
| Peak RSS | 60,472 KB |

All 1,042 emitted binding-set plans have status `executable`.

Requirement distribution:

| Requirements per route | Routes |
|---:|---:|
| 0 | 10 |
| 1 | 44 |
| 2 | 34 |
| 3 | 1 |

Requirement source:

```text
route_selector:   63
schema_required:  52
```

The 10 routes without requirements still receive one executable empty binding
plan because their route witness needs no argument value.

## Binding alternatives

S4 emits at most 16 deterministic binding-set plans per route:

| Plans per route | Routes |
|---:|---:|
| 1 | 10 |
| 3 | 12 |
| 9 | 10 |
| 15 | 6 |
| 16 | 51 |

These alternatives preserve enum and producer fallback. Phase 2 can try the
next plan when a producer execution is empty without deriving a new static
plan.

The primary plans contain:

```text
standalone producer jobs: 65
static binding jobs:      50
```

The count exceeds 89 because a route may need both a harvested value and a
static enum binding.

## OPEN routes

### `Query.node(id) -> Watchlist`

```graphql
query Harvest_56c40ac3c51df01e {
  market {
    sorareWatchlists(sport: NBA) {
      id
    }
  }
}
```

Requirement:

```text
arg:Query.node.id -> selected type: Watchlist
producer: field:Watchlist.id
```

### `Query.nodes(ids) -> Watchlist`

```graphql
query Harvest_09ece6bb1eeb5c28 {
  currentUser {
    myWatchlists(sport: NBA) {
      id
    }
  }
}
```

Requirement:

```text
arg:Query.nodes.ids -> selected type: Watchlist
producer: field:Watchlist.id
```

The scalar IDs are aggregated for the list consumer by Phase 2.

### `Query.market.watchlist(id) -> Watchlist`

```graphql
query Harvest_19858bf6b0c0e822 {
  currentUser {
    myWatchlists(sport: NBA) {
      id
    }
  }
}
```

Requirement:

```text
arg:MarketRoot.watchlist.id -> selected type: Watchlist
producer: field:Watchlist.id
```

Each OPEN route has 15 or 16 static alternatives, including other reachable
Watchlist producers and enum values.

## Important audit correction

The first implementation used S2's broad selected type `Node` for
`Query.node.id`. It could therefore choose an ID from another Node
implementation, such as `Announcement.id`. That ID is valid for `node(id:)`
but does not execute the Watchlist type condition.

The corrected rule is:

```text
consumer selected type =
  concrete type reached immediately after the consumer FIELD and its
  following TYPE_CONDITION edges in the route witness
```

After the correction:

- both global-ID OPEN routes use `type:Watchlist`;
- their producers are `field:Watchlist.id`;
- a regression test checks the same invariant on `UserDevice`.

## Correlation interpretation

Watchlist produces zero correlation constraints. Its multi-requirement routes
combine identity producers with independent enum filters such as `sport`.
Static enum filters do not impose same-instance lineage.

This is expected. The joint correlation engine is exercised by the Deck
calibration, where `profile.id + deck.slug` must share one profile anchor.

## Determinism

Two independent final runs produced byte-identical output.

```text
SHA-256:
011045b98fbdd167d94c5dbba269e66161a23843023debc4c4ceb633eb0f5cb2
```

Every emitted GraphQL operation is parsed by `graphql-parser` before the S4
artifact is written.

## Conclusion

For the Watchlist anchor, Phase 1 is complete:

- every S3 route has at least one executable static binding-set plan;
- all required arguments and active selectors are represented;
- enum and producer fallback are explicit;
- global-ID seeds are constrained to Watchlist identity;
- the artifact is deterministic and bounded.

`executable` is a static property. It does not prove that a producer returns
data for a particular account or that the consumer accepts the harvested
value. Those checks belong to runtime Seed Finder Phase 2.

