# Watchlist Legacy Calibration

This fixture freezes the 68 paths emitted by the unmodified legacy
`graphql-path-enum` binary for `Watchlist`.

`legacy_paths.canonical.json` expands every legacy Connection collapse through
the canonical `Connection.nodes` edge. `schema_ir.json` is a deterministic S0
subset containing those edges plus the direct `Query.node` and `Query.nodes`
global-ID tracks.

The legacy text SHA-256, including one trailing newline per path, is:

```text
0cd5f178ba551425b20fe4462651d6eb25b8c5b04bc1f585a656d0fb9a1e9839
```

S3 acceptance is subset-based: all 68 canonical paths must be present. The
total S3 path count is intentionally not fixed.

Rebuild from the local real-schema inputs with:

```bash
python3 scripts/build_watchlist_fixture.py
```
