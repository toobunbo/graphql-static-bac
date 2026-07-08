# Documentation Map

## Normative specification

- [`spec/architecture.md`](spec/architecture.md): authoritative S0-S5 data contracts and invariants.

## Design rationale

- [`design/pipeline.md`](design/pipeline.md): recall-first model, classifier rationale, scoring, and calibration.
- [`design/routeVerdict.md`](design/routeVerdict.md): constrained route analysis, selector provenance,
  selector continuity, boundaries, route signatures, and Watchlist ground truth.
- [`design/seedFinderPhases.md`](design/seedFinderPhases.md): two-phase Seed
  Finder design covering static harvest-query planning and runtime seed
  validation.

## Implementation

- [`implementation/overview.md`](implementation/overview.md): compact implementation context for all stages.
- [`implementation/repository-layout.md`](implementation/repository-layout.md): source, test, config, and output boundaries.
- [`implementation/stage-0-ir.md`](implementation/stage-0-ir.md): implementation and test plan for S0.
- [`implementation/stage-3-paths.md`](implementation/stage-3-paths.md): implemented
  legacy diagnostic path enumerator.
- [`implementation/stage-3-route-analysis.md`](implementation/stage-3-route-analysis.md):
  implemented production constrained route analysis and verdict compaction.
- [`implementation/stage-4-seed-plans.md`](implementation/stage-4-seed-plans.md):
  implemented static Seed Finder Phase 1, including requirement
  collection, producer search, correlation, dependency DAGs, and query
  emission.
- [`implementation/phase-2-seed-runtime.md`](implementation/phase-2-seed-runtime.md):
  implemented runtime Seed Finder Phase 2, including request-template injection,
  producer execution, extraction, representation adapters, fallback, and
  verified route bindings.

Stages 0, 3, 4, and Seed Runtime Phase 2 are implemented. The S1 and S2
classifiers remain design-only, but S3/S4 consume their typed artifact
contracts. S5 remains deferred.

## Reference and research

- [`reference/graphql-path-enum.md`](reference/graphql-path-enum.md): behavior and gaps of the legacy tool.
- [`research/runtime-cap-c-user-device.md`](research/runtime-cap-c-user-device.md): runtime calibration evidence for the global-ID CAP.
- [`research/runtime-watchlist-unknown-cases.md`](research/runtime-watchlist-unknown-cases.md):
  four runtime probes for resolving representative Watchlist `unknown` routes.
- [`research/runtime-so5-user-group-plan.md`](research/runtime-so5-user-group-plan.md):
  full-route runtime handoff for the public/private `So5UserGroup` anchor.
- [`research/runtime-so5-user-group-selected.md`](research/runtime-so5-user-group-selected.md):
  reduced runtime set containing all 6 open and 15 representative unknown
  routes.
- [`research/runtime-deck-selected.md`](research/runtime-deck-selected.md):
  schema-derived runtime handoff containing all 11 open and 10 representative
  unknown routes for the mutable `Deck.visible` flag.
- [`research/runtime-automation-research-prompt.md`](research/runtime-automation-research-prompt.md):
  research-only prompt for surveying query synthesis, seed discovery,
  multi-user execution, and authorization oracles.
- [`research/field-path-finder-research-prompt.md`](research/field-path-finder-research-prompt.md):
  self-contained research prompt for finding structural paths to exact output
  fields such as `Card.assetId`.
- [`research/s4-watchlist-calibration.md`](research/s4-watchlist-calibration.md):
  final S4 Phase 1 calibration for all 89 Watchlist routes, including the
  global-ID concrete-type audit.
- [`research/phase-2-watchlist-calibration.md`](research/phase-2-watchlist-calibration.md):
  Phase 2 live calibration for the 3 open and 12 unknown Watchlist routes
  under a hard three-request-per-route budget.
- [`research/phase-2-adaptive-scheduler-benchmark.md`](research/phase-2-adaptive-scheduler-benchmark.md):
  live A/B benchmark of exhaustive and first-verified scheduling with
  three-request and ten-request route budgets.
The executable cross-stage contract is stored in
[`../tests/fixtures/user_device/`](../tests/fixtures/user_device/).
