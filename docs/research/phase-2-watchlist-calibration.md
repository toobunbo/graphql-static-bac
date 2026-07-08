# Phase 2 Watchlist Calibration

**Date:** 2026-06-14

**Scope:** Seed acquisition and route validation only. No security or
authorization verdict is produced.

## Inputs

- S0: `output/schema_ir.introspection.json`
- S3: `output/routes.watchlist.json`
- S4: `output/seed-plans.watchlist.json`
- Runtime context: Sorare test account B
- Selected routes: 3 `open`, 12 `unknown`
- Hard budget: 3 HTTP requests per route, including harvest and validation

## Open Routes

The run emitted `output/seed-runtime.watchlist-open.json`.

| Route | Result | Verified bindings | Notes |
|---|---:|---:|---|
| `Query.node -> ... on Watchlist` | `verified/bounded` | 1 | Global ID accepted unchanged; first-verified mode stopped immediately. |
| `Query.nodes -> ... on Watchlist` | `verified/bounded` | 1 | List consumer reached Watchlist. |
| `Query.market -> watchlist(id:)` | `budget_exhausted/bounded` | 0 | NBA and BASEBALL harvests were empty; FOOTBALL harvested five IDs on request 3, leaving no validation request. |

The market route did acquire the same five personal Watchlist IDs as the
global-ID routes. Its result is bounded, not unresolved evidence.
The three open routes used eight HTTP requests in total.

## Unknown Routes

The run emitted `output/seed-runtime.watchlist-unknown.json`.

- 11 routes: `budget_exhausted/bounded`
- 1 route: `unresolved/complete`
- 34 HTTP requests total; no route exceeded 3
- 56 unique concrete values harvested
- harvested classes include Card asset IDs, Card slugs,
  AnnouncementNotification IDs, Subscription IDs, and five Watchlist IDs

No unknown route verified within this deliberately small budget. Most routes
spent the remaining requests validating the first candidate values or trying
representation alternatives. This does not contradict the earlier focused
runtime probes that established real Watchlist-reaching witnesses; it shows
that three total requests are insufficient for exhaustive candidate fallback.

## Interpretation

`budget_exhausted` always carries `coverage: bounded`. The artifact preserves
producer responses, extracted values, provenance, attempted plans, and final
validation failures. A later runner can therefore reuse or retry the evidence
with a larger budget without changing S3 or S4.

The three-request policy is suitable as a fast first pass. It is not a
completeness setting, especially where enum fallback consumes multiple
producer requests or the matching seed appears late in a harvested list.
