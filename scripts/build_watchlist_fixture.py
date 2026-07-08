#!/usr/bin/env python3
"""Build the committed Watchlist S3 calibration fixture."""

from __future__ import annotations

import copy
import hashlib
import json
import re
import subprocess
from pathlib import Path


CRATE = Path(__file__).resolve().parents[1]
LEGACY_ROOT = CRATE.parent
OUTPUT = CRATE / "tests/fixtures/s3/watchlist"
EXPECTED_HASH = "0cd5f178ba551425b20fe4462651d6eb25b8c5b04bc1f585a656d0fb9a1e9839"


def main() -> None:
    result = subprocess.run(
        [
            str(LEGACY_ROOT / "target/release/graphql-path-enum"),
            "-i",
            str(LEGACY_ROOT / "introspection.json"),
            "-t",
            "Watchlist",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    lines = [line for line in result.stdout.splitlines() if line.startswith("- Query")]
    legacy_text = "".join(f"{line}\n" for line in lines)
    digest = hashlib.sha256(legacy_text.encode()).hexdigest()
    assert len(lines) == 68, len(lines)
    assert digest == EXPECTED_HASH, digest

    source = json.loads(
        (CRATE / "output/schema_ir.introspection.json").read_text(encoding="utf-8")
    )
    types = source["data"]["types"]
    used_fields: set[tuple[str, str]] = set()
    used_types = {"Query", "Node", "Watchlist"}
    canonical_paths: list[list[str]] = []

    for line in lines:
        legacy_steps = re.findall(r"(\w+) \((\w+)\)", line)
        edge_ids: list[str] = []
        for index, (owner, field_name) in enumerate(legacy_steps):
            shown_target = (
                legacy_steps[index + 1][0]
                if index + 1 < len(legacy_steps)
                else "Watchlist"
            )
            field = types[owner]["fields"][field_name]
            actual_target = field["return_type"]["named_type"]
            edge_ids.append(field["field_id"])
            used_fields.add((owner, field_name))
            used_types.update((owner, actual_target, shown_target))

            if actual_target != shown_target:
                nodes = types[actual_target]["fields"]["nodes"]
                assert nodes["return_type"]["named_type"] == shown_target
                edge_ids.append(nodes["field_id"])
                used_fields.add((actual_target, "nodes"))

        canonical_paths.append(edge_ids)

    for field_name in ("node", "nodes"):
        used_fields.add(("Query", field_name))
        used_types.add(types["Query"]["fields"][field_name]["return_type"]["named_type"])

    subset_types = {}
    for type_name in sorted(used_types):
        definition = copy.deepcopy(types[type_name])
        definition["fields"] = {
            field_name: field
            for field_name, field in definition["fields"].items()
            if (type_name, field_name) in used_fields
        }
        definition["interfaces"] = [
            interface
            for interface in definition["interfaces"]
            if interface in used_types
        ]
        if type_name == "Node":
            definition["possible_types"] = ["Watchlist"]
        elif definition["kind"] in {"INTERFACE", "UNION"}:
            definition["possible_types"] = [
                possible
                for possible in definition["possible_types"]
                if possible in used_types
            ]
        subset_types[type_name] = definition

    schema_subset = {
        key: copy.deepcopy(value)
        for key, value in source.items()
        if key != "data"
    }
    schema_subset["data"] = {
        "roots": {"query": "Query", "mutation": None, "subscription": None},
        "types": subset_types,
    }

    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / "legacy_paths.txt").write_text(legacy_text, encoding="utf-8")
    write_json(OUTPUT / "legacy_paths.canonical.json", canonical_paths)
    write_json(OUTPUT / "schema_ir.json", schema_subset)


def write_json(path: Path, value: object) -> None:
    path.write_text(
        json.dumps(value, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
