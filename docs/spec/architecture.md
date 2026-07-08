# GraphQL Static BAC Architecture and Stage Contracts (v4.0)

> **Migration note (June 13, 2026):** production uses S3/v2 Route Analysis and
> S4/v1 Static Seed Planning. The v1 path enumerator and historical scorer/S5
> sections are retained only as reference fixtures.

Tai lieu nay la context chot cho phase implementation cua static framework.

**Scope hien tai:** Query-only. Mutation va Subscription deferred.

**Output static hien tai:** `routes.json` + `seed_plans.json`.

Full machine-readable example cho `UserDevice` nam tai
`tests/fixtures/user_device/`.

JSON snippets trong tai lieu chi minh hoa shape. Golden fixture va
`validate.py` la executable contract co tham quyen khi snippet bi rut gon.

---

## 1. Stage Flow

```text
introspection.json / schema.graphql
              |
              v
       [S0] Schema IR Builder
              |
              +-------------------+
              |                   |
              v                   v
       [S1] Sink Classifier  [S2] Argument Classifier
              |
              v
       [S3] Route Analysis <--------- S2
              |
              v
       [S4] Static Seed Planning <--- S0 + S2
              |
              +--> seed_plans.json
```

Dependency chinh xac:

- S1 va S2 co the chay song song sau S0.
- S3 can S0, S1, S2 va route policy.
- S4 can S0/v1, S2/v1 va S3/v2.
- Phase 2 runtime harvesting doc `seed_plans.json`; no khong nam trong S4.

S0 normalize schema; S1/S2 classify facts; S3 tinh route verdict va compact
theo semantic signature; S4 thu requirement, tim producer, bao toan correlation,
lap dependency DAG va emit GraphQL harvest operations. S4 khong goi runtime va
khong chua security oracle.

---

## 2. Common Envelope

Moi intermediate artifact phai dung cung envelope:

```json
{
  "contract_version": "1.0",
  "stage": "S0",
  "schema_fingerprint": "sha256:...",
  "scope": ["query"],
  "producer": {
    "name": "graphql-static-bac",
    "version": "0.1.0"
  },
  "status": "complete",
  "warnings": [],
  "data": {}
}
```

Invariants:

- JSON keys dung `snake_case` o tat ca stages.
- `contract_version` version hoa data contract, khong phai analyzer version.
- Tat ca files trong cung run phai co cung `schema_fingerprint`.
- `schema_fingerprint` la SHA-256 cua exact input bytes ma S0 parse. No nhan dien
  mot analysis run, khong phai semantic-equivalence hash giua SDL va introspection.
- `status` chi nhan `complete`, `incomplete`, hoac `failed`.
- S4 tu choi S3 artifact co status khac `complete`.
- Partial diagnostic co the duoc ghi khi incomplete, nhung S5 khong duoc dong goi
  no nhu mot complete analysis.
- `confidence` trong machine-readable artifacts chi nhan `low`, `medium`, hoac
  `high`; khong tron numeric probability vao cung contract.
- Moi warning la object co `code`, `message` va optional `details`; khong ghi
  warning bang free-form string.

### Stable IDs

```text
type:UserDevice
field:Query.node
arg:Query.node.id
input_field:SearchFilter.ownerId
sink:UserDevice
sink:UserDevice.userAgent
type_condition:Node->UserDevice
```

`cap_id` duoc tao tu SHA-256 cua canonical tuple:

```text
(target_type_id, ordered edge_id list)
```

Tuple duoc serialize thanh UTF-8 JSON compact, khong whitespace, giu nguyen thu
tu array. Vi du:

```json
["type:UserDevice",["field:Query.node","type_condition:Node->UserDevice"]]
```

`cap_id` la `cap:sha256:` noi voi lowercase hex digest cua chinh byte sequence
tren. Object/map khong duoc tham gia input hash.

Khong dung `cap_1`, `cap_2` lam identity vi enumeration order co the thay doi.

---

## 3. Canonical TypeRef

Hai boolean `non_null` va `list` khong du de bieu dien chinh xac `[User!]!`.
Moi field, argument va input field phai dung TypeRef sau:

```json
{
  "display": "[UserDevice!]!",
  "named_type": "UserDevice",
  "named_kind": "OBJECT",
  "wrappers": ["NON_NULL", "LIST", "NON_NULL"]
}
```

`wrappers` duoc ghi tu ngoai vao trong:

```text
UserDevice      -> []
UserDevice!     -> [NON_NULL]
[UserDevice]    -> [LIST]
[UserDevice!]   -> [LIST, NON_NULL]
[UserDevice!]!  -> [NON_NULL, LIST, NON_NULL]
```

Day la representation canonical; `display` chi de debug va phai tai tao duoc tu
`named_type + wrappers`.

---

## 4. S0 - Schema IR Builder

**Input:** introspection JSON hoac SDL.

**Output:** `schema_ir.json`.

S0 chi parse va normalize syntax. No khong classify sensitive data, selector hay
sanitizer.

```json
{
  "roots": {
    "query": "Query",
    "mutation": "Mutation",
    "subscription": null
  },
  "types": {
    "UserDevice": {
      "type_id": "type:UserDevice",
      "kind": "OBJECT",
      "description": "A Sorare manager device",
      "interfaces": ["Node"],
      "possible_types": [],
      "fields": {
        "userAgent": {
          "field_id": "field:UserDevice.userAgent",
          "name": "userAgent",
          "arguments": [],
          "return_type": {
            "display": "String!",
            "named_type": "String",
            "named_kind": "SCALAR",
            "wrappers": ["NON_NULL"]
          }
        }
      },
      "input_fields": {},
      "enum_values": []
    }
  }
}
```

S0 invariants:

- Giu argument type, default value, input fields, enum values, interfaces va
  possible types.
- Giu field/type descriptions vi S1 co description-based signals.
- Argument va input-field descriptions cung phai duoc giu khi source cung cap.
- Enum value la object gom `name`, `description`, `is_deprecated` va
  `deprecation_reason`; khong serialize enum bang chuoi don.
- Applied directives chi co khi source la SDL/registry/extension. Standard
  introspection chi cung cap directive definitions.
- Root khong ton tai phai la `null`, khong dung chuoi rong.
- Khong collapse Connection/Edge.

---

## 5. S1 - Sink Classifier

**Input:** `schema_ir.json`.

**Output:** `sinks.json`.

S1 classify cac object/field reachable tu Query theo bug-bounty profile. Moi
selected type co mot type summary va it nhat mot `sink_ref`.

```json
{
  "selected_types": [
    {
      "type_id": "type:UserDevice",
      "type_name": "UserDevice",
      "tiers": ["T3"],
      "impact": "medium",
      "sink_ref_ids": [
        "sink:UserDevice",
        "sink:UserDevice.userAgent"
      ],
      "addressability": {
        "global_id": true,
        "interface": "Node",
        "entry_field_ids": ["field:Query.node", "field:Query.nodes"]
      }
    }
  ],
  "sink_refs": {
    "sink:UserDevice": {
      "sink_ref_id": "sink:UserDevice",
      "kind": "TYPE",
      "type_id": "type:UserDevice",
      "field_id": null,
      "categories": ["device_metadata"],
      "impact": "medium",
      "evidence": [
        {"rule": "implements_node", "interface": "Node"}
      ]
    },
    "sink:UserDevice.userAgent": {
      "sink_ref_id": "sink:UserDevice.userAgent",
      "kind": "FIELD",
      "type_id": "type:UserDevice",
      "field_id": "field:UserDevice.userAgent",
      "categories": ["pii", "device_metadata"],
      "impact": "medium",
      "evidence": [
        {
          "rule": "sensitive_field_context",
          "matched_tokens": ["user", "agent"]
        }
      ]
    }
  },
  "unselected_types": [],
  "query_unreachable_types": []
}
```

S1 invariants:

- `sink_ref.kind` chi nhan `TYPE` hoac `FIELD`.
- `impact` chi nhan `unknown`, `low`, `medium`, `high`, hoac `critical`.
- Type selected chi vi global addressability van phai co TYPE sink ref.
- Field sink co stable `field_id`; khong chi luu field name.
- S3 enumerate mot lan theo target type, khong enumerate lai cho tung field sink.
  `sink_ref_ids` duoc attach vao CAP de consumer biet fields can quan tam.
- Tier la label; impact la field rieng; tier khong vao exploitability score.
- `Node.possibleTypes` giao voi `unselected_types` phai rong.
- `unselected_types` chi chua Query-reachable objects. Query-unreachable objects
  duoc bao rieng.
- Name matcher dung tokenization + light normalization. Clear-domain terms duoc
  exact-token match; ambiguous terms can compound/co-signal.

---

## 6. S2 - Argument Classifier

**Input:** `schema_ir.json`.

**Output:** `args.json`.

A/B/C la detection methods, khong phai classes. Mot argument co the dong thoi
la selector va authz modifier, vi du `viewAsUserId`. Vi vay contract dung
`classifications` array, khong dung mot enum don.

Classification values:

```text
object_selector
authz_modifier
noise
possible_selector
```

Unknown/mixed args duoc giu la `possible_selector`, khong drop.

```json
{
  "fields": {
    "field:Query.node": {
      "arguments": [
        {
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
          "classifications": ["object_selector"],
          "signals": ["type:ID"],
          "confidence": "high"
        }
      ]
    }
  }
}
```

Nested input example:

```json
{
  "arg_ref": "input_field:SearchFilter.ownerId",
  "root_arg_ref": "arg:Query.search.filter",
  "arg_path": "Query.search.filter.ownerId",
  "input_path": ["ownerId"],
  "type_ref": {
    "display": "ID",
    "named_type": "ID",
    "named_kind": "SCALAR",
    "wrappers": []
  },
  "classifications": ["object_selector"]
}
```

S2 invariants:

- `arg_path`, `root_arg_ref`, `input_path` va full TypeRef la bat buoc.
- `classifications` co it nhat mot value va khong co duplicate.
- `noise` chi loai argument khoi S4 khi no la classification duy nhat. Neu
  `noise` xung dot voi selector/authz signal, giu tat ca classifications va ha
  confidence; khong drop.
- Unknown/mixed khong co strong positive signal dung `possible_selector`.
- Recursion vao input object co cycle guard theo input type identity.
- Noise lexicon conservative.
- Khi type/name signals xung dot, giu ca hai trong `signals`; recall-first class
  khong duoc ha thanh noise.

---

## 7. S3 - Query Path Enumerator

**Input:** `schema_ir.json` + `sinks.json`.

**Output:** `paths.json`.

S3 chi chua structural facts. No khong gan selector, sanitizer, flow hay score.

```json
{
  "targets": {
    "type:UserDevice": {
      "target_type_id": "type:UserDevice",
      "sink_ref_ids": [
        "sink:UserDevice",
        "sink:UserDevice.userAgent"
      ],
      "enumeration_status": "complete",
      "caps": [
        {
          "cap_id": "cap:sha256:...",
          "origin": "global_id",
          "entry_field_id": "field:Query.node",
          "target_type_id": "type:UserDevice",
          "edges": [
            {
              "edge_id": "field:Query.node",
              "kind": "FIELD",
              "source_type_id": "type:Query",
              "field_id": "field:Query.node",
              "target_type_id": "type:Node"
            },
            {
              "edge_id": "type_condition:Node->UserDevice",
              "kind": "TYPE_CONDITION",
              "source_type_id": "type:Node",
              "field_id": null,
              "target_type_id": "type:UserDevice"
            }
          ],
          "cycle_templates": [],
          "display_projection": "Query.node -> ... on UserDevice"
        }
      ]
    }
  }
}
```

S3 invariants:

- Path la ordered list cac edges. Terminal type nam trong `target_type_id`; khong
  tao mot FIELD edge gia khong co field name.
- `FIELD` edge luon co source, field va target.
- `TYPE_CONDITION` edge co `field_id: null`.
- Interface/union expansion dung TYPE_CONDITION.
- `origin` chi nhan `traversal` hoac `global_id`.
- Global ID track chi nhan root field co signature structural tuong ung. `node`
  co mot `id` argument mang named type `ID` va tra named type `Node` khong qua
  LIST wrapper. `nodes` co `ids` la LIST cua named type `ID` va return type la
  LIST cua named type `Node`. Nullability khong anh huong detection. No sinh CAP
  rieng cho moi concrete `Node.possible_types`, va sinh rieng cho `node`/`nodes`
  neu ca hai ton tai.
- Canonical graph giu Connection/Edge. `display_projection` khong phai nguon chan
  ly va co the bo qua.
- Cycle detection path-local theo `edge_id`; type co the lap, cung edge khong lap.
- Khi DFS gap mot edge da co trong current path, no dung branch. Neu current type
  la target va CAP cua current path da duoc emit, append cycle template vao
  `cycle_templates` cua CAP do:

```json
{
  "repeated_edge_id": "field:User.manager",
  "cycle_start_index": 1,
  "repeatable_edge_ids": ["field:User.manager"]
}
```

  `cycle_start_index` la zero-based index cua lan xuat hien dau trong CAP edges;
  `repeatable_edge_ids` la cycle segment da co trong path. Template dedup theo
  full object va sort deterministic. Khong co cycle thi `cycle_templates` la `[]`.
- Khong max depth/path cap. Resource exhaustion dat stage/run status incomplete.
- Exact full-path dedup dua tren ordered `edge_id` sequence.
- Enumeration order phai deterministic de snapshot test on dinh.

---

## 8. S4 - Static Seed Planning

**Input:** `schema_ir.json` S0/v1 + `args.json` S2/v1 +
`routes.json` S3/v2.

**Output:** `seed_plans.json` S4/v1.

S4 lap mot hoac nhieu binding-set plan cho tung route:

```text
requirements
correlation_constraints
producer_jobs
dependency_dag
binding_set_plans
unresolved_requirements
```

Producer strategy:

```text
static_binding
standalone
joint_co_read
threaded_dependency
```

S4 invariants:

- Duyet moi FIELD edge trong route witness va thu moi NON_NULL argument khong
  default; active optional selector cung la requirement.
- Required input object duoc recurse; moi static binding giu `input_path`.
- Exact leaf producer va ID/String identity compatibility co evidence ro rang.
- Search target parent composite type, sau do append exact scalar/enum field.
- Argument state duoc dua vao worklist truoc compaction.
- Correlation constraint la fact route-lineage; producer strategy la lua chon
  rieng cua search.
- Joint co-read dung nearest shared instance anchor va giu branch/index
  provenance; khong flatten thanh Cartesian product.
- Dependency DAG dung producer job lam node va output-field -> input-argument
  lam typed edge.
- Moi executable binding-set plan cover tat ca requirement, discharge tat ca
  constraint, va co execution order acyclic.
- Emit toi da 16 binding-set plan deterministic cho moi route de Phase 2 fallback
  khi enum/producer candidate empty.
- Moi emitted GraphQL operation phai parse duoc truoc khi artifact duoc ghi.
- S4 khong goi endpoint, khong chua token/runtime value, validation status hoac
  security oracle.

Stable ID namespaces:

```text
seed_req:sha256:...
seed_constraint:sha256:...
seed_job:sha256:...
seed_dependency:sha256:...
seed_binding_plan:sha256:...
```

Canonical details va implementation invariants nam tai
`docs/implementation/stage-4-seed-plans.md`.

---

## 9. Historical S5 - Artifact Assembler

Section nay mo ta fixture v1 cu va khong nam trong production pipeline hien tai.
S5 se duoc redesign sau runtime Seed Finder Phase 2.

**Input:** S0 metadata + `sinks.json` + `paths.json` + `scored_caps.json`.

**Output:** `suspect_artifact.json` + `coverage_report.json`.

Final artifact phai self-contained cho triage: consumer khong can doc
intermediate files de hieu sink, path, selector va score. Query generator deferred
van duoc phep doc S0 de lay tat ca required non-selector arguments.

```json
{
  "artifact_version": "1.0",
  "schema_fingerprint": "sha256:...",
  "analyzer_version": "0.1.0",
  "scoring_profile": "bug-bounty-v1",
  "profile": "bug-bounty",
  "scope": ["query"],
  "status": "complete",
  "warnings": [],
  "sink_types": [
    {
      "type_id": "type:UserDevice",
      "type_name": "UserDevice",
      "sink_refs": [],
      "caps": []
    }
  ]
}
```

Coverage count phai tach ro:

```json
{
  "query_reachable_type_count": 790,
  "selected_type_count": 293,
  "selected_sink_ref_count": 293,
  "unselected_type_count": 497,
  "unselected_types": [],
  "query_unreachable_type_count": 251,
  "query_unreachable_types": [],
  "invariants": {
    "node_implementers_unselected": 0
  }
}
```

S5 invariants:

- `selected_type_count` va `selected_sink_ref_count` khong duoc dung chung ten
  `selected_sinks`.
- Cac `*_type_count` trong coverage chi dem OBJECT types trong Query analysis
  universe; khong tron scalar, enum, input object, interface hoac union.
- Final CAP phai chua canonical edges, semantic annotations va score breakdown.
- Sort deterministic: score desc, ranking bucket order o S4, `field_hop_count`
  asc, cap ID asc.
- `unselected_types` va Query-unreachable list phai xuat day du.
- Warnings cap run gom missing SDL directives, incomplete enumeration, parser
  recovery, hoac contract mismatch.

---

## 10. Historical UserDevice Simulation Fixture

Fixture files:

```text
tests/fixtures/user_device/
  s0_schema_ir.json
  s1_sinks.json
  s2_args.json
  s3_paths.json
  s4_scored_caps.json
  s5_suspect_artifact.json
  s5_coverage_report.json
  validate.py
  README.md
```

Expected CAPs:

| CAP | Origin | Flow | Score |
| --- | --- | --- | ---: |
| `Query.node -> UserDevice` | global ID | `direct_global_id` | 6.0 |
| `Query.nodes -> UserDevice` | global ID | `direct_global_id` | 6.0 |
| `Query.currentUser.devices` | traversal | `no_selector` | -2.0 |
| `Query.currentUser.currentDevice` | traversal | `no_selector` | -2.0 |

Hai path `currentUser` cung chi co mot sanitizer boundary. Fixture nay la golden
contract test dau tien cua implementation.

Chay fixture contract:

```bash
python3 tests/fixtures/user_device/validate.py
```

---

## 11. Deferred

- Mutation write analysis.
- Mutation output disclosure.
- Subscription/event disclosure.
- Runtime seed harvesting and consumer validation.
- Runtime authorization oracle.
- Agent re-ranking/explanation.
- `broad` va `full` profiles.

Khong tai su dung Query score mot cach may moc cho Mutation/Subscription.
