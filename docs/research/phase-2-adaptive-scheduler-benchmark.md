# Phase 2 Adaptive Scheduler Benchmark

**Date:** 2026-06-14

**Endpoint:** Sorare GraphQL test environment, account B.

**Workload:** Watchlist S3/S4 artifacts. The same compiled binary, endpoint,
authentication context, route order, and input artifacts were used for each
A/B pair.

## Modes

`exhaustive` reproduces the previous runtime behavior:

- continue after a verified binding;
- up to 100 producer paths and values for this benchmark;
- up to 32 adapter combinations.

`first_verified` is the new production default:

- stop after the first target-reaching binding;
- at most 3 producer paths per route;
- at most 3 values from each producer;
- at most 2 adapter combinations per binding set.

## Fast Pass: 3 Requests Per Route

| Workload | Exhaustive | Adaptive | Change |
|---|---:|---:|---:|
| 3 open routes | 8.54s, 9 requests | 7.88s, 8 requests | 7.7% faster, 1 request removed |
| 12 unknown routes | 29.29s, 34 requests | 29.21s, 34 requests | effectively unchanged |
| Combined | 37.83s, 43 requests | 37.09s, 42 requests | 2.0% faster |

The open route count remained unchanged: two verified and one
`budget_exhausted`. Adaptive mode retained one verified binding per successful
route instead of continuing to collect duplicates.

The unknown workload did not improve because no route reached Watchlist within
three requests. Therefore the `first_verified` stopping condition never fired;
the existing request cap remained the dominant bound.

## Production-Shaped Pass: 10 Requests Per Open Route

| Metric | Exhaustive | Adaptive | Change |
|---|---:|---:|---:|
| Wall time | 27.12s | 9.15s | 66.3% faster |
| HTTP requests | 30 | 10 | 66.7% fewer |
| Verified routes | 3 | 3 | unchanged |
| Verified bindings retained | 16 | 3 | one sufficient witness per route |
| Artifact size | 86,501 bytes | 25,916 bytes | 70.0% smaller |

This is the representative production benefit: once a usable seed is found,
the scheduler stops instead of spending the remaining route budget collecting
equivalent bindings.

## Remaining Limits

Adaptive scheduling does not solve two independent search problems:

1. Required enum variants are still represented as separate S4 plans. Empty
   NBA and BASEBALL probes can consume requests before FOOTBALL succeeds.
2. Values inside one producer response retain server order. A useful Card slug
   at position 19 is not reached when only the first three values are sampled.

The next optimizations should therefore be batched enum probing through
GraphQL aliases and route-aware ranking of values inside producer responses.
