#!/usr/bin/env python3
"""Validate the UserDevice golden contract across S0-S5."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any


BASE = Path(__file__).resolve().parent
FILES = {
    "s0": "s0_schema_ir.json",
    "s1": "s1_sinks.json",
    "s2": "s2_args.json",
    "s3": "s3_paths.json",
    "s4": "s4_scored_caps.json",
    "artifact": "s5_suspect_artifact.json",
    "coverage": "s5_coverage_report.json",
}


def load(name: str) -> dict[str, Any]:
    with (BASE / FILES[name]).open(encoding="utf-8") as handle:
        return json.load(handle)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def cap_id(target_type_id: str, edge_ids: list[str]) -> str:
    canonical = json.dumps(
        [target_type_id, edge_ids],
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    return f"cap:sha256:{hashlib.sha256(canonical).hexdigest()}"


def walk(value: Any):
    yield value
    if isinstance(value, dict):
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)


def render_type_ref(type_ref: dict[str, Any]) -> str:
    rendered = type_ref["named_type"]
    for wrapper in reversed(type_ref["wrappers"]):
        if wrapper == "NON_NULL":
            rendered += "!"
        elif wrapper == "LIST":
            rendered = f"[{rendered}]"
        else:
            raise AssertionError(f"unknown TypeRef wrapper: {wrapper}")
    return rendered


def normalize_introspection_type_ref(raw: dict[str, Any]) -> dict[str, Any]:
    wrappers: list[str] = []
    current = raw
    while current["kind"] in {"NON_NULL", "LIST"}:
        wrappers.append(current["kind"])
        current = current["ofType"]
    normalized = {
        "display": "",
        "named_type": current["name"],
        "named_kind": current["kind"],
        "wrappers": wrappers,
    }
    normalized["display"] = render_type_ref(normalized)
    return normalized


def main() -> None:
    docs = {name: load(name) for name in FILES}

    fingerprints = {doc["schema_fingerprint"] for doc in docs.values()}
    require(len(fingerprints) == 1, "schema_fingerprint differs across stages")
    source_path = BASE.parents[3] / "introspection.json"
    source_bytes = source_path.read_bytes()
    source_fingerprint = f"sha256:{hashlib.sha256(source_bytes).hexdigest()}"
    require(fingerprints == {source_fingerprint}, "fixture fingerprint differs from introspection.json")

    for index, name in enumerate(("s0", "s1", "s2", "s3", "s4")):
        doc = docs[name]
        require(doc["contract_version"] == "1.0", f"{name}: contract version")
        require(doc["stage"] == f"S{index}", f"{name}: wrong stage")
        require(doc["scope"] == ["query"], f"{name}: scope must be query-only")
        require(doc["status"] == "complete", f"{name}: fixture must be complete")

    for node in walk(docs):
        if isinstance(node, dict) and {"display", "named_type", "wrappers"} <= node.keys():
            require(node["display"] == render_type_ref(node), f"invalid TypeRef: {node}")

    schema_types = docs["s0"]["data"]["types"]
    source_schema = json.loads(source_bytes)["data"]["__schema"]
    source_types = {item["name"]: item for item in source_schema["types"]}
    source_roots = {
        "query": source_schema["queryType"]["name"] if source_schema["queryType"] else None,
        "mutation": source_schema["mutationType"]["name"] if source_schema["mutationType"] else None,
        "subscription": source_schema["subscriptionType"]["name"] if source_schema["subscriptionType"] else None,
    }
    require(docs["s0"]["data"]["roots"] == source_roots, "S0 root types differ from source")

    for type_name, fixture_type in schema_types.items():
        source_type = source_types[type_name]
        require(fixture_type["kind"] == source_type["kind"], f"S0 kind mismatch: {type_name}")
        require(
            fixture_type["description"] == source_type.get("description"),
            f"S0 type description mismatch: {type_name}",
        )
        source_interfaces = {item["name"] for item in source_type.get("interfaces") or []}
        source_possible_types = {item["name"] for item in source_type.get("possibleTypes") or []}
        require(set(fixture_type["interfaces"]) <= source_interfaces, f"S0 interface mismatch: {type_name}")
        require(set(fixture_type["possible_types"]) <= source_possible_types, f"S0 possible type mismatch: {type_name}")
        source_fields = {item["name"]: item for item in source_type.get("fields") or []}
        for field_name, fixture_field in fixture_type["fields"].items():
            source_field = source_fields[field_name]
            require(
                fixture_field["return_type"] == normalize_introspection_type_ref(source_field["type"]),
                f"S0 return TypeRef mismatch: {type_name}.{field_name}",
            )
            source_args = {item["name"]: item for item in source_field["args"]}
            require(set(source_args) == {item["name"] for item in fixture_field["arguments"]}, f"S0 args mismatch: {type_name}.{field_name}")
            for fixture_arg in fixture_field["arguments"]:
                source_arg = source_args[fixture_arg["name"]]
                require(
                    fixture_arg["description"] == source_arg.get("description"),
                    f"S0 argument description mismatch: {type_name}.{field_name}.{fixture_arg['name']}",
                )
                require(
                    fixture_arg["type_ref"] == normalize_introspection_type_ref(source_arg["type"]),
                    f"S0 arg TypeRef mismatch: {type_name}.{field_name}.{fixture_arg['name']}",
                )

    type_ids = {type_def["type_id"] for type_def in schema_types.values()}
    fields = {
        field["field_id"]: field
        for type_def in schema_types.values()
        for field in type_def["fields"].values()
    }
    schema_arg_ids = {
        argument["arg_id"]
        for field in fields.values()
        for argument in field["arguments"]
    }

    sink_data = docs["s1"]["data"]
    sink_refs = sink_data["sink_refs"]
    require(
        set(sink_refs) == {sink["sink_ref_id"] for sink in sink_refs.values()},
        "S1 sink_ref map keys differ from sink_ref_id values",
    )
    for selected in sink_data["selected_types"]:
        require(selected["type_id"] in type_ids, "S1 selected type missing from S0")
        require(set(selected["sink_ref_ids"]) <= set(sink_refs), "S1 dangling sink ref")
    for sink in sink_refs.values():
        require(sink["type_id"] in type_ids, "S1 sink type missing from S0")
        require(sink["field_id"] is None or sink["field_id"] in fields, "S1 sink field missing from S0")
    selected_type_names = {selected["type_name"] for selected in sink_data["selected_types"]}
    node_possible_types = set(schema_types["Node"]["possible_types"])
    require(node_possible_types <= selected_type_names, "S1 dropped a Node implementer")

    classified_arguments = {
        argument["arg_ref"]: argument
        for field in docs["s2"]["data"]["fields"].values()
        for argument in field["arguments"]
    }
    require(set(classified_arguments) <= schema_arg_ids, "S2 argument missing from S0")
    for field in docs["s2"]["data"]["fields"].values():
        for argument in field["arguments"]:
            classifications = argument["classifications"]
            require(classifications, "S2 classifications must not be empty")
            require(len(classifications) == len(set(classifications)), "duplicate S2 classification")

    target = docs["s3"]["data"]["targets"]["type:UserDevice"]
    require(set(target["sink_ref_ids"]) == set(sink_refs), "S3 sink refs differ from S1")
    structural_caps = {cap["cap_id"]: cap for cap in target["caps"]}
    require(len(structural_caps) == 4, "S3 must emit four UserDevice CAPs")

    seen_paths: set[tuple[str, ...]] = set()
    forbidden_s3_keys = {"flow", "score", "selectors", "sanitizer_boundaries"}
    for cap in structural_caps.values():
        require(not (forbidden_s3_keys & cap.keys()), "S3 contains semantic fields")
        edges = cap["edges"]
        edge_ids = [edge["edge_id"] for edge in edges]
        require(tuple(edge_ids) not in seen_paths, "S3 contains duplicate path")
        seen_paths.add(tuple(edge_ids))
        require(cap["cap_id"] == cap_id(cap["target_type_id"], edge_ids), "unstable CAP ID")
        require(isinstance(cap["cycle_templates"], list), "S3 cycle_templates must be an array")
        cycle_keys = [json.dumps(item, sort_keys=True) for item in cap["cycle_templates"]]
        require(len(cycle_keys) == len(set(cycle_keys)), "S3 duplicate cycle template")
        require(edges[-1]["target_type_id"] == cap["target_type_id"], "S3 wrong terminal type")
        for left, right in zip(edges, edges[1:]):
            require(left["target_type_id"] == right["source_type_id"], "disconnected S3 edges")
        for edge in edges:
            require(edge["source_type_id"] in type_ids, "S3 source type missing from S0")
            require(edge["target_type_id"] in type_ids, "S3 target type missing from S0")
            if edge["kind"] == "FIELD":
                require(edge["field_id"] == edge["edge_id"] in fields, "invalid FIELD edge")
            elif edge["kind"] == "TYPE_CONDITION":
                require(edge["field_id"] is None, "TYPE_CONDITION must not have field_id")
            else:
                raise AssertionError(f"unknown edge kind: {edge['kind']}")

    scored_caps = docs["s4"]["data"]["caps"]
    require(set(scored_caps) == set(structural_caps), "S3/S4 CAP sets differ")
    for key, cap in scored_caps.items():
        require(key == cap["cap_id"], "S4 map key differs from cap_id")
        require(set(cap["target_sink_ref_ids"]) == set(target["sink_ref_ids"]), "S4 sink refs differ from S3")
        require(
            all(selector["arg_ref"] in classified_arguments for selector in cap["selectors"]),
            "S4 selector missing from S2",
        )
        for selector in cap["selectors"]:
            source = classified_arguments[selector["arg_ref"]]
            require(selector["classification"] in source["classifications"], "S4 selector role mismatch")
            for field in ("root_arg_ref", "arg_path", "input_path", "type_ref"):
                require(selector[field] == source[field], f"S4 selector changed {field}")
        boundary_ids = [item["boundary_id"] for item in cap["sanitizer_boundaries"]]
        require(len(boundary_ids) == len(set(boundary_ids)), "S4 duplicate sanitizer boundary")
        score_sum = sum(item["delta"] for item in cap["score_breakdown"])
        require(abs(score_sum - cap["score"]) < 1e-9, "S4 score breakdown mismatch")
        structural_edges = structural_caps[key]["edges"]
        require(cap["edge_count"] == len(structural_edges), "S4 edge count mismatch")
        field_hops = sum(edge["kind"] == "FIELD" for edge in structural_edges)
        require(cap["field_hop_count"] == field_hops, "S4 field hop count mismatch")
        require(
            all(modifier["arg_ref"] in classified_arguments for modifier in cap["authz_modifiers"]),
            "S4 authz modifier missing from S2",
        )

    expected = {
        "Query.node -> ... on UserDevice": ("direct_global_id", 6.0),
        "Query.nodes -> ... on UserDevice": ("direct_global_id", 6.0),
        "Query.currentUser -> CurrentUser.devices -> UserDevice": ("no_selector", -2.0),
        "Query.currentUser -> CurrentUser.currentDevice -> UserDevice": ("no_selector", -2.0),
    }
    actual = {
        cap["display_projection"]: (scored_caps[cap_id_]["flow"], scored_caps[cap_id_]["score"])
        for cap_id_, cap in structural_caps.items()
    }
    require(actual == expected, f"unexpected UserDevice simulation: {actual}")

    ordered_final_caps = [
        cap
        for sink_type in docs["artifact"]["sink_types"]
        for cap in sink_type["caps"]
    ]
    final_caps = {
        cap["cap_id"]: cap
        for cap in ordered_final_caps
    }
    final_sink_refs = {
        sink["sink_ref_id"]: sink
        for sink_type in docs["artifact"]["sink_types"]
        for sink in sink_type["sink_refs"]
    }
    require(final_sink_refs == sink_refs, "S5 changed S1 sink refs")
    require(set(final_caps) == set(structural_caps), "S5 artifact CAP set differs")
    for key, cap in final_caps.items():
        require(cap["edges"] == structural_caps[key]["edges"], "S5 changed structural edges")
        require(cap["cycle_templates"] == structural_caps[key]["cycle_templates"], "S5 changed cycle templates")
        require(cap["score"] == scored_caps[key]["score"], "S5 changed score")
        require(cap["flow"] == scored_caps[key]["flow"], "S5 changed flow")
        require(cap["authz_modifiers"] == scored_caps[key]["authz_modifiers"], "S5 changed authz modifiers")
        require(cap["edge_count"] == scored_caps[key]["edge_count"], "S5 changed edge count")
        require(cap["field_hop_count"] == scored_caps[key]["field_hop_count"], "S5 changed field hops")

    bucket_order = {name: index for index, name in enumerate(
        ("direct", "global_id", "indirect", "self_scoped", "name_only")
    )}
    expected_order = sorted(
        ordered_final_caps,
        key=lambda cap: (
            -cap["score"],
            bucket_order[cap["ranking_bucket"]],
            cap["field_hop_count"],
            cap["cap_id"],
        ),
    )
    require(ordered_final_caps == expected_order, "S5 CAP sort order mismatch")

    coverage = docs["coverage"]
    require(coverage["selected_type_count"] == len(sink_data["selected_types"]), "coverage type count")
    require(coverage["selected_sink_ref_count"] == len(sink_refs), "coverage sink count")
    require(coverage["unselected_type_count"] == len(coverage["unselected_types"]), "coverage unselected count")
    require(
        coverage["query_reachable_type_count"]
        == coverage["selected_type_count"] + coverage["unselected_type_count"],
        "coverage reachable count mismatch",
    )
    require(coverage["invariants"]["node_implementers_unselected"] == 0, "Node implementer dropped")

    print("UserDevice golden contract: PASS")
    print("CAPs: 4 (2 global_id @ 6.0, 2 self_scoped @ -2.0)")
    print(f"Schema fingerprint: {next(iter(fingerprints))}")


if __name__ == "__main__":
    main()
