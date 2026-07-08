# GraphQL Static BAC — README kiến trúc (context cho implementation)

> **Migration note (June 13, 2026):** các mục S3 enumerator, S4 scorer và S5
> bên dưới mô tả pipeline lịch sử. Production hiện dùng S3 Route Analysis và
> S4 Static Seed Planning. Contract hiện hành nằm tại
> `stage-3-route-analysis.md` và `stage-4-seed-plans.md`.

Tài liệu này cho agent/người implement **đọc lấy context**: mục tiêu từng stage, **input/output rõ ràng**,
và mong muốn của từng bước. **Không chứa code.** Spec chi tiết + golden fixture (`spec_examples/`) là
contract có thẩm quyền khi tài liệu này rút gọn.

**Mục tiêu hệ thống:** từ GraphQL schema (introspection/SDL), sinh **ranked suspect list** các đường
truy cập tới dữ liệu nhạy cảm (Broken Access Control). **Scope hiện tại: Query-only.** Mutation/Subscription
deferred.

**Output cuối:** `suspect_artifact.json` (CAPs đã xếp hạng) + `coverage_report.json`.

---

## Nguyên tắc nền (chi phối mọi stage)

1. **Recall-first.** Static không confirm được gì → ưu tiên không bỏ sót. FN (mất một bug) ≫ FP (một test phí).
   Mọi quyết định "loại" phải đổi thành "giảm điểm/gắn cờ", **không xoá**. Phép xoá hợp lệ duy nhất:
   dedup trùng-tuyệt-đối ở S3.
2. **Deterministic-first.** Các bước phân loại/enumerate là code thuần, kết quả tái lập được. LLM (nếu dùng)
   chỉ xử phần residue mơ hồ, ngoài critical path.
3. **Epistemics.** Output là **Candidate Access Path (CAP)** = *hypothesis*, KHÔNG phải confirmed bug.
   Confirm là việc của runtime phase (ngoài scope).
4. **Artifact-passing.** Mỗi stage **đọc file vào → ghi file ra**. Stage không gọi nhau trong bộ nhớ.
   → mỗi stage **chạy & test độc lập**.

---

## Dòng dữ liệu

```text
introspection.json / schema.graphql
        │
        ▼
   [S0] IR Builder ─────────► schema_ir.json
        │
        ├──► [S1] Sink Classifier ──► sinks.json
        ├──► [S2] Arg Classifier  ──► args.json
        │
        ▼
   [S3] Path Enumerator ──────► paths.json        (cần schema_ir + sinks)
        │
        ▼
   [S4] Scorer ───────────────► scored_caps.json  (cần paths + args + sinks)
        │
        ▼
   [S5] Assembler ────────────► suspect_artifact.json + coverage_report.json
```

**Dependency:** S1, S2 chạy song song sau S0 · S3 cần S0+S1 · S4 cần S1+S2+S3 · S5 cần S0+S1+S3+S4.

**Thứ tự implement đề xuất (xây spine trước, mỗi bước chạy được rồi mới sang bước sau):**

1. **S0** — IR (nền, mọi thứ phụ thuộc).
2. **S1** — Sink Classifier (xác định "đích"; chạy độc lập, output review được ngay).
3. **S3** — Path Enumerator (lõi: vẽ đường tới đích).
4. **S2** — Arg Classifier.
5. **S4** — Scorer.
6. **S5** — Assembler.

> S2/S4/S5 có thể làm sau vì S0→S1→S3 đã đủ để kiểm chứng ý tưởng (tìm được đường tới sink hay chưa).
> Mỗi stage nhận input là file artifact của stage trước → có thể chạy/test riêng bằng artifact mẫu.

---

## Contract dùng chung (mọi artifact tuân theo)

- **Envelope:** mỗi file có `contract_version`, `stage`, `schema_fingerprint`, `scope`, `producer`,
  `status` (`complete|incomplete|failed`), `warnings[]`, `data`. JSON key dùng `snake_case`.
- **`schema_fingerprint`:** SHA-256 của **đúng bytes input** S0 parse. Nhận diện một run; SDL vs introspection
  của cùng schema → fingerprint khác. Mọi file trong cùng run phải cùng fingerprint.
- **Stable IDs:** `type:User`, `field:Query.node`, `arg:Query.node.id`, `input_field:SearchFilter.ownerId`,
  `sink:UserDevice`, `sink:UserDevice.userAgent`, `type_condition:Node->UserDevice`. Mọi cross-stage join
  dùng các ID này.
- **`cap_id`:** content-hash của `(target_type_id, [edge_id...])` — KHÔNG dùng `cap_1/cap_2` (order đổi).
- **TypeRef:** `{display, named_type, named_kind, wrappers[]}`, `wrappers` ghi ngoài-vào-trong
  (vd `[User!]!` → `["NON_NULL","LIST","NON_NULL"]`). `display` chỉ để debug.
- **`confidence`:** chỉ `low|medium|high` (không trộn số). **`impact`:** `unknown|low|medium|high|critical`,
  sort độc lập với score.

---

## S0 — Schema IR Builder

**Mong muốn:** một hình dạng schema sạch, format-agnostic, để mọi stage sau dựa vào (không stage nào
phải đụng introspection/SDL thô).

**Input:** `introspection.json` **hoặc** `schema.graphql` (SDL).

**Output:** `schema_ir.json` — `roots{query,mutation,subscription}` + `types` map. Mỗi type:
`{type_id, kind, interfaces, possible_types, fields{}, input_fields{}, enum_values[]}`. Mỗi field:
`{field_id, name, arguments[], return_type: TypeRef, description?}`.

**Rules:**
- Giữ **đủ** metadata: arg type, default value, input_fields, enum_values, interfaces, possible_types, description.
- Unwrap NON_NULL/LIST thành `TypeRef.wrappers`.
- Root không tồn tại → `null` (không chuỗi rỗng).
- KHÔNG collapse Connection/Edge. KHÔNG classify gì.
- Applied directive chỉ có khi input là SDL/registry; introspection chuẩn không có.
- **[determinism]** emit key của map theo thứ tự sorted.

---

## S1 — Sink Classifier

**Mong muốn:** xác định "đích" — type/field nhạy cảm đáng test — generous (thà thừa còn hơn sót), và
phơi luôn phần không chọn để review bù sót.

**Input:** `schema_ir.json`.

**Output:** `sinks.json` — `selected_types[]` (mỗi cái: `type_id`, `tiers`, `impact`, `sink_ref_ids`,
`addressability`), `sink_refs{}` (mỗi `SinkRef`: `kind: TYPE|FIELD`, `type_id`, `field_id|null`,
`categories`, `impact`, `evidence[]`), `unselected_types[]`, `query_unreachable_types[]`.

**Rules:**
- **SinkRef tách TYPE vs FIELD.** Một type có thể có nhiều field-sink. Path reach *type*; oracle nhắm *SinkRef*.
- Chọn sink khi ≥1 strong signal: policy / credential / financial / PII / access-control / owner-ref-ngữ-cảnh /
  sensitive-type-name / auth-description / direct-query-selector / **globally_addressable** (implements `Node`).
- **Name matcher:** tokenize → light normalization (depluralize) → two-policy: *clear-domain* (`withdrawal`,
  `balance`...) exact-token; *ambiguous bare* (`token`,`key`,`code`...) chỉ khi compound/co-signal. **Cấm substring rộng.**
  Lưu `matched_tokens`.
- Tier là **label** (T1 policy · T2 owner+sensitive · T3 field-sensitive), KHÔNG vào score. `impact` riêng.
- `unselected_types` là **output bắt buộc** (vùng review FN), không chỉ count; chỉ chứa Query-reachable objects.
- Invariant cứng: `Node.possible_types ∩ unselected_types == ∅`.
- **[reachability — fix]** "Query-reachable" định nghĩa = *∃ CAP dưới mô hình edge của S3, gồm cả global-ID track*.
  Type chỉ addressable qua `node()` vẫn tính reachable. Phải đảm bảo **S1-reachable ⟺ S3 sinh ≥1 CAP**
  (S1 và S3 dùng chung định nghĩa graph).

---

## S2 — Argument Classifier

**Mong muốn:** biết arg nào cho attacker "nhắm" vào object của victim (selector), arg nào vô hại (noise).

**Input:** `schema_ir.json`.

**Output:** `args.json` — keyed `field_id` → `arguments[]`. Mỗi arg: `{arg_ref, root_arg_ref, arg_path,
input_path[], type_ref, classifications[], signals[], confidence}`.

**Rules:**
- A/B/C là **method**, không phải class: A (type ID-ish) · B (name lexicon, **chung tokenizer S1**) ·
  C (đệ quy vào input object, cycle-guard theo input-type identity).
- `classifications[]` là **array** (1 arg có thể vừa `object_selector` vừa `authz_modifier`, vd `viewAsUserId`).
  Values: `object_selector | authz_modifier | noise | possible_selector`.
- **Provenance bắt buộc:** `arg_path` đầy đủ kể cả lồng (`Query.search.filter.ownerId`), `root_arg_ref`, `input_path`.
- `noise` chỉ loại arg khi là classification **duy nhất**; xung đột signal → giữ tất cả, hạ confidence, không drop.
- Noise lexicon **conservative**; mơ hồ → `possible_selector`.

---

## S3 — Query Path Enumerator

**Mong muốn:** với mỗi sink, liệt kê **mọi đường** từ `Query` tới nó + track `node(id:)` độc lập. Chỉ structural,
không score.

**Input:** `schema_ir.json` + `sinks.json`.

**Output:** `paths.json` — keyed `target_type_id` → `{sink_ref_ids, enumeration_status, caps[]}`. Mỗi CAP:
`{cap_id, origin: traversal|global_id, entry_field_id, edges[], cycle_templates[], display_projection}`.
Mỗi edge: `{edge_id, kind: FIELD|TYPE_CONDITION, source_type_id, field_id|null, target_type_id}`.

**Rules:**
- Graph edge = `field → return_type`; interface/union → edge `TYPE_CONDITION` sang `possible_types`.
  **Giữ Connection/Edge, không collapse** (`display_projection` chỉ để đọc, không phải nguồn chân lý).
- **Cycle path-local theo `edge_id`** (type lặp được, cùng edge không lặp). Gặp repeated edge tại target →
  ghi `cycle_template`.
- **KHÔNG max_depth/max_paths cap.** Cạn resource → `status=incomplete` ở cấp run, KHÔNG xuất artifact
  một phần như thể đầy đủ.
- **Global-ID track độc lập:** mỗi concrete type ∈ `Node.possible_types` sinh CAP `node()`/`nodes()` riêng,
  kể cả khi traversal chỉ self-scoped. Detect bằng structural signature (`node(id:ID)→Node`, `nodes(ids:[ID])→[Node]`).
- Dedup **chỉ trùng-tuyệt-đối** (theo ordered `edge_id` sequence).
- **[determinism — fix]** iterate edge theo thứ tự sorted → `paths.json` snapshot ổn định (fixture cần).
- Calibration: superset của `graphql-path-enum` trên cùng schema.

---

## Historical S4 — Annotator + Scorer

Mục này chỉ mô tả pipeline v1. Production S4 hiện là Static Seed Planning;
xem `stage-4-seed-plans.md`.

**Mong muốn:** biến đống đường thô thành danh sách **xếp hạng** theo khả năng khai thác; annotate, không xoá.

**Input:** `paths.json` + `args.json` + `sinks.json`.

**Output:** `scored_caps.json` — keyed `cap_id` → `{target_type_id, target_sink_ref_ids, selectors[],
authz_modifiers[], sanitizer_boundaries[], flow, ranking_bucket, edge_count, field_hop_count, score,
score_breakdown[], ownership_continuity, confidence}`.

**Rules:**
- Join arg facts lên mỗi FIELD edge: `object_selector`/`possible_selector` → `selectors`; `authz_modifier` →
  `authz_modifiers`. Cùng arg có thể ở cả hai.
- `flow`: `direct_global_id` (origin=global_id) · `no_selector` (selectors rỗng) · `direct` (selector trên
  terminal FIELD edge; bỏ qua TYPE_CONDITION cuối) · `indirect` (selector ở field sớm hơn).
- **Sanitizer phạt theo `boundary_id`, KHÔNG theo số field khớp.** `currentUser.currentDevice` = một boundary
  self-scope → −2 một lần, không −4.
- **Score breakdown phải sum đúng = score.** Mỗi positive signal cộng **một lần/CAP** dù nhiều arg cùng class.
  Depth = `−0.2 * max(0, field_hop_count − 3)`. **KHÔNG prune** (CAP điểm âm vẫn giữ).
- Rubric `bug-bounty-v1` (có version): `+3 object_selector · +2 global_id · +2 authz_modifier · +1 direct_flow ·
  −2/sanitizer boundary · −0.2/hop>3 · −1 possible-selector-only`.
- Indirect CAP đổi identity → `ownership_continuity: unknown` (annotation, không tự cộng/trừ score).
- **[ranking_bucket — fix]** mapping rõ:
  `origin=global_id → global_id` · `flow=direct → direct` · `flow=indirect → indirect` ·
  `flow=no_selector + có sanitizer → self_scoped` · `flow=no_selector + không sanitizer → name_only`.

---

## Historical S5 — Artifact Assembler

**Mong muốn:** một file tự-đủ để triage (không cần đọc file trung gian) + báo cáo coverage.

**Input:** `scored_caps.json` + `paths.json` (edges) + `sinks.json` + S0 metadata.

**Output:**
- `suspect_artifact.json` — envelope (`artifact_version`, `schema_fingerprint`, `analyzer_version`,
  `scoring_profile`, `profile`, `scope`, `status`, `warnings`) + `sink_types[]` (mỗi cái gồm sink_refs + caps
  đầy đủ: canonical edges + semantic annotations + score_breakdown).
- `coverage_report.json` — `query_reachable_type_count`, `selected_type_count`, `selected_sink_ref_count`,
  `unselected_type_count`, `unselected_types[]`, `query_unreachable_type_count/[]`, `invariants`.

**Rules:**
- Join theo `cap_id`: edges (S3) + score (S4) + sink_refs (S1).
- **Sort deterministic:** `score desc → ranking_bucket order → field_hop_count asc → cap_id asc`.
  Bucket order: `direct < global_id < indirect < self_scoped < name_only`.
- Coverage `*_type_count` chỉ đếm **OBJECT types** trong Query universe (không trộn scalar/enum/input/interface/union).
- `unselected_types` + query-unreachable list xuất **đầy đủ** (không chỉ count).
- `warnings[]` cấp run: missing-SDL-directives, incomplete-enumeration, parser-recovery, contract-mismatch.

---

## Deferred (ngoài scope hiện tại)

Mutation write analysis (input-driven) · Mutation output disclosure · Subscription · Runtime seed harvesting ·
Agent re-ranking · profiles `broad`/`full`. **Không tái dùng Query score máy móc cho Mutation/Subscription.**

---

## Lưu ý cho người implement

- Bắt đầu S0 → S1 → S3; mỗi stage đọc artifact file của stage trước nên test riêng được bằng input mẫu.
- Golden fixture `tests/fixtures/user_device/` + `validate.py` là contract test đầu tiên — implement xong một
  stage thì chạy fixture để kiểm output khớp shape.
- Ba điểm `[fix]` (reachability ⟺ CAP · ranking_bucket mapping · sorted iteration) là contract giữa stage —
  chốt sớm vì chúng ảnh hưởng fixture.
