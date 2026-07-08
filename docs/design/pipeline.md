# GraphQL Static BAC Analysis Framework (v3.2)

Framework phân tích tĩnh sinh **ranked suspect list** cho Broken Access Control (BAC) trong GraphQL
Query APIs, từ introspection/SDL. Hướng bug bounty. Mutation và Subscription được hoãn sang phase
thiết kế sau, không nằm trong scope v3.2.

> **Inspired by** (không phải implementation của): *Taint Analysis for Graph APIs Focusing on
> Broken Access Control* — Lambers et al., ICGT 2024 / arXiv 2501.08947. Paper dùng graph
> transformation rules + Critical Pair Analysis; framework này là **schema-topology heuristic +
> risk scoring**, không claim soundness.

---

## 0. Nguyên tắc nền: RECALL-FIRST

Static **không confirm được gì** từ schema → mục tiêu số một là **giảm false negative xuống thấp
nhất**. Cơ sở là bất đối xứng chi phí:

- **FN ở static = một bug thật sẽ KHÔNG BAO GIỜ được test** (runtime chỉ test cái static phơi ra). Mất vĩnh viễn.
- **FP ở static = một runtime test phí công** — rẻ, bị giới hạn bởi test budget.

→ FN cost ≫ FP cost. Hệ quả thiết kế (bất di bất dịch):

1. **Mọi quyết định "loại candidate" phải đổi thành "giảm điểm" hoặc "gắn cờ nghi ngờ", KHÔNG xoá.**
   Phép loại hợp lệ *duy nhất* là dedup trùng-tuyệt-đối (lossless).
2. **Recall-first BẮT BUỘC đi kèm ranking mạnh.** Nếu không, bottleneck chỉ chuyển từ recall sang
   triage runtime. Châm ngôn: **"không mất gì, rank cho khéo, test top-N."**
3. Dồn công vào **(a) sink/selector detection generous + flag truncation** và **(b) scorer chất
   lượng** — KHÔNG dồn vào filtering tĩnh thông minh (đó là nơi FN sinh ra).

**Epistemics:** output gọi là **Candidate Access Path (CAP)** / BAC hypothesis (KHÔNG "taint flow" —
introspection chỉ chứng minh reachability + có mặt selector, không chứng minh flow). 2 phase: Static
(tài liệu này) → ranked suspect list; Runtime (ngoài phạm vi) → confirm bằng diff-oracle
multi-identity. Agent **prioritize, KHÔNG confirm**.

---

## 1. Nền tảng: Schema IR

Mọi component đọc **IR**, không đọc path string. Parse introspection/SDL **một lần** thành IR đầy đủ.

```
Type        { name, kind: OBJECT|INTERFACE|UNION|INPUT|ENUM|SCALAR,
              fields[], inputFields[], enumValues[], interfaces[], possibleTypes[] }
Field       { name, args[], returnType: TypeRef }
Arg         { name, type: TypeRef, defaultValue }
TypeRef     { name, kind, nonNull, list, ofType }   // đã unwrap NON_NULL/LIST
Roots       { query, mutation, subscription } // v3.2 chỉ phân tích query
```

**Introspection JSON → IR (mapping):**

| Introspection | IR | Ghi chú |
|---------------|-----|--------|
| `types[].kind` | `Type.kind` | |
| `types[].fields[]` | `Type.fields` | OBJECT/INTERFACE |
| `fields[].args[]` `{name,type,defaultValue}` | `Field.args` | **giữ `type` — tool cũ chỉ giữ name** |
| `fields[].type` (unwrap → ofType) | `Field.returnType` | |
| `types[].inputFields[]` | `Type.inputFields` | cho đệ quy lớp C |
| `types[].enumValues[]` | `Type.enumValues` | cho detect policy enum |
| `types[].interfaces[]` / `possibleTypes[]` | tương ứng | |
| `__schema.directives[]` | chỉ *định nghĩa* | |

> **Cảnh báo:** introspection chuẩn KHÔNG trả directive *được áp*. Signal `@auth`/`@requiresScope`
> chỉ lấy từ SDL/registry/extension. Không có SDL → bỏ signal directive.

---

## 2. Kiến trúc

```
 Introspection / SDL → Complete Schema IR ──┐
        ├──> Sink Classifier      (tier T1/T2/T3 = label, KHÔNG vào score)
        ├──> Argument Classifier  (3-class)
        └──> Type Graph Builder
                 │
                 ▼
        Query Path Enumerator     (path-local cycle; không depth/path cap; JSON)
                 │
                 ▼
        Annotator + Risk Scorer   (sanitizer = annotate/giảm điểm; KHÔNG drop; KHÔNG prune)
                 │
                 ▼
        JSON suspect artifact (ranked CAPs) → Agent (re-rank/explain, KHÔNG confirm)
```

v3.2 chỉ triển khai read-path từ Query. Mutation/write và Subscription là hai track độc lập sẽ bổ
sung sau, không được suy diễn từ Query path analysis.

---

## 3. Sink Classifier — GENEROUS (recall lever #1)

**Sink bị miss = KHÔNG path nào tới nó → bug vô hình.** Sink thừa chỉ đẻ vài CAP score thấp (rẻ).
Bất đối xứng cực mạnh → **over-include không thương tiếc**.

Facts thô: `has_policy_field`, `has_owner_ref`, `has_sensitive_field`, `in_public_collection`.
Field/type name mơ hồ được giữ trong `weak_signals` để coverage review, nhưng không tự tạo sink nếu
thiếu contextual/co-signal. Đây là trade-off có chủ đích của bug-bounty profile.

Selection universe của v3.2 là **object/field reachable từ Query**, sau khi mở interface/union.
Mutation-only và Subscription-only types không tham gia Query sink count; chúng được báo riêng trong
coverage report để tránh làm sai calibration.

Bug-bounty profile không chọn mọi type. Một type/field được chọn làm sink khi có ít nhất một
evidence đủ mạnh:

- policy/access boundary;
- credential, financial, PII hoặc access-control field;
- owner reference có ngữ cảnh;
- security-sensitive type name;
- auth-sensitive description;
- direct Query selector exposure;
- **globally addressable** qua `Query.node(id:)` / `Query.nodes(ids:)` và `Node.possibleTypes`.

`globally_addressable` là signal chọn sink độc lập, không chỉ là score bonus. Lý do: một type có thể
chỉ xuất hiện qua self-scoped traversal nhưng vẫn truy cập trực tiếp được bằng global ID. Trên schema
calibration hiện tại có 218 `Node` implementers; tool cũ bỏ toàn bộ `Query.node` path vì không theo
INTERFACE, trong đó 107 type từng nằm ngoài classifier thử nghiệm.

| Tier (label) | Điều kiện | Vai trò |
|------|-----------|---------|
| **T1 — Policy** | `has_policy_field` | chọn oracle runtime |
| **T2 — Ownership** | `has_owner_ref` ∧ `has_sensitive_field` | chọn oracle runtime |
| **T3 — Field-level** | có field sensitive | sink là *field* |

> **Tier KHÔNG vào score (đổi từ v3).** Tier chỉ là **label định tuyến oracle ở runtime** — nó
> không dự đoán khả năng có bug, và T2 (PII/tài chính) có khi *impact cao hơn* T1. Mức nghiêm trọng
> để vào một field **`impact` riêng**, sort độc lập với exploitability-score. Một node có thể mang
> nhiều tier.

`in_public_collection` là exposure annotation, không phải điều kiện bắt buộc để tạo T3. Một field
nhạy cảm vẫn là field sink khi object không nằm trong public collection.

`has_policy_field`: boolean (`public`,`private`,`isPublic`,`visible`,`hidden`,`published`,`draft`,
`unlisted`,`shared`); enum `visibility`/`privacy`/`status` values `PUBLIC/PRIVATE/HIDDEN/INTERNAL`.

### Name matching rule: one matcher, two lexical policies

Mọi lexicon name-based (sink, selector, sanitizer, impact) **cấm substring regex rộng**. Quy trình:

1. Tách `camelCase`, `PascalCase`, `snake_case`, acronym và digit thành token.
2. Normalize morphology nhẹ và explicit: `balances → balance`, `withdrawals → withdrawal`,
   `deposited → deposit`, `devices → device`.
3. Áp policy theo loại từ vựng, không match mọi token theo cùng một cách.
4. Lưu `matched_tokens` và rule evidence để audit.

Không dùng Porter stemming hay full stemming. Chỉ split compound, depluralize/normalize một bảng
variant nhỏ có kiểm soát; stemmer mạnh có thể over-conflate domain terms và tạo false positive mới.

**Policy A — clear domain terms:** `withdrawal`, `balance`, `wallet`, `iban`, `kyc`, `email`, ...

- Sau light normalization, exact token đủ để tạo signal.
- Mục tiêu là chống false negative do plural/inflection.

**Policy B — ambiguous bare terms:** `token`, `code`, `key`, `account`, `user`, `status`.

- Bare token không tự tạo strong signal.
- Chỉ match contextual compound/sequence hoặc khi có co-signal từ type, enum values, description,
  direct selector hay global addressability.
- Ví dụ credential hợp lệ: `apiKey`, `accessToken`, `refreshToken`, `sessionToken`, `privateKey`.
- Ví dụ không phải credential: `boughtSingleSaleTokenOffers`.

Ví dụ:

```text
boughtSingleSaleTokenOffers -> [bought,single,sale,token,offers]
```

Token `token` trong context `sale token offer` là asset/NFT, không phải credential. Ngược lại
`accessToken`, `refreshToken`, `jwtToken` là credential qua contextual sequence.

Calibration Query-reachable hiện tại:

- 790 object types reachable từ Query; 251 object types chỉ nằm ngoài Query reachability.
- Token/context matcher + structural signals chọn 293 sinks, để lại 497 `unselected_types`.
- `Node` hygiene: 218/218 implementers nằm trong selected; `Node ∩ unselected = ∅`.
- Financial exact-token: 91 fields; light morphology: 105 fields; khôi phục 15 fields thuộc
  `CurrentUser`, `TokenBid`, `TokenMyBid`, `UserProfile`.
- 15 restored financial fields kéo thêm **0 type** vào selected vì bốn owner types đã có signal khác;
  dù vậy field evidence được phục hồi để impact/oracle annotation không bị thiếu.
- Tổng morphology diff chỉ thêm `Notifications` vào selected set: 292 → 293.
- Credential contextual matcher còn 17 field; không còn bắt nhầm NFT/asset `TokenOffer` fields.

---

## 4. Argument Classifier (3-class, deterministic)

- **Lớp A (type):** `ID`/`ID!`/`[ID!]`, custom scalar định danh → **object-selector**.
- **Lớp B (name):** selector (`id`,`*Id`,`slug`,`uuid`,`code`,`handle`,`username`,`address`,`ref`,`key`);
  authz-modifier (`role`,`scope`,`viewAs`,`asUser`,`impersonate`); noise (`first`,`after`,`limit`,
  `offset`,`page`,`cursor`,`orderBy`,`sort`,`locale`,`currency`,`format`).
- **Lớp C (đệ quy):** arg input object (`filter`/`where`/`input`) → descend `Type.inputFields`, classify bằng A+B.

Lớp B dùng chung tokenizer/canonicalizer của Stage 1, không chạy substring search trực tiếp. Khi
type signal và name signal xung đột, giữ cả hai trong `signals[]` thay vì ép mất evidence:

```json
{
  "name": "cursor",
  "class": "possible-selector",
  "signals": ["type:ID", "name:known-noise"],
  "confidence": 0.7
}
```

> **Noise lexicon phải CONSERVATIVE (recall):** chỉ chứa thứ **chắc chắn** pagination/format. Tên
> **mơ hồ → KHÔNG cho vào noise**, giữ làm possible-selector. Noise là cái *duy nhất* loại selector,
> nên drop nhầm = FN.

Rule: `known-noise` → khỏi tập selector; `selector`/`authz-mod` → giữ; `UNKNOWN` (scalar opaque,
tên lạ) → **giữ, low-confidence**. Agent (tùy chọn) re-rank UNKNOWN offline, đẩy verdict về lexicon.

---

## 5. Type Graph + Query Path Enumerator

Type graph: node = Type; edge = `field → returnType`. Interface/union expansion dùng edge kind riêng
`TYPE_CONDITION` sang `possibleTypes`, để query generator biết phải sinh inline fragment.

Enumerator (tự viết, KHÔNG subprocess/parse text):
- **Path-local cycle detection theo schema edge identity**, không theo type. Type được phép xuất hiện
  lại trong path (`User.manager -> User`), nhưng cùng edge không được lặp vô hạn.
- **KHÔNG max_depth cap (đổi từ v3).** Depth chỉ là **score penalty** (mục 7), không bao giờ cắt path → không silent FN.
- **KHÔNG `max_paths` cap trong v3.2.** Enumerate toàn bộ simple paths theo cycle policy. Nếu cạn
  resource thì analysis fail/incomplete ở cấp run; không xuất artifact một phần như thể complete.
- Khi gặp repeated edge, emit `cycle_template` annotation. Không thể enumerate vô hạn các walk như
  `User.manager.manager.manager...`; template giữ lại bằng chứng recursive access cho review/runtime.
- Output **JSON** (không text), mỗi path join sẵn metadata từ IR.
- **Dedup CHỈ trùng-tuyệt-đối** (full-path identity). KHÔNG dedup (entry, terminal) — recall-unsafe.

> **Calibration:** `graphql-path-enum` làm **oracle**, không phải dependency. Enumerator của bạn
> phải là **superset** trên cùng schema; thiếu path → DFS có bug.

### Independent global-ID track

`Query.node(id:)` và `Query.nodes(ids:)` không chờ normal traversal tìm tới sink. Với mỗi concrete
type trong `Node.possibleTypes`, sinh CAP độc lập:

```graphql
query Candidate($id: ID!) {
  node(id: $id) {
    ... on UserDevice {
      id
      deviceType
      userAgent
    }
  }
}
```

CAP này có:

```text
edge kind: FIELD(Query.node) -> TYPE_CONDITION(UserDevice)
selector: id
globally_addressable: true
flow: direct-global-id
```

Nó được tạo ngay cả khi normal traversal của `UserDevice` chỉ có
`Query.currentUser.devices`. Đây là một access path riêng, không phải shortcut hay score adjustment
của self-scoped path.

### Ownership ambiguity on indirect CAPs

Static topology không chứng minh identity tại sink còn là identity được chọn ở entry point. Ví dụ:

```text
userById(victim) -> boughtOffers -> owners -> account
```

`account` có thể thuộc seller hoặc bên thứ ba, không phải victim. Mọi indirect CAP phải annotate:

```json
{
  "ownership_continuity": "unknown",
  "identity_transition_edges": ["TokenOffer.owners"],
  "runtime_oracle_hint": "resolve and record the actual sink owner"
}
```

Annotation này không giảm recall và không khẳng định vulnerability; nó đổi cách runtime xác định
ground truth và expected visibility.

---

## 6. Deferred Scope: Mutation and Subscription

v3.2 không enumerate hoặc score Mutation/Subscription.

Các invariants giữ lại cho phase sau:

- Mutation write analysis phải input-driven; `Mutation -> Payload -> Type` không chứng minh write.
- Mutation output disclosure là read track riêng, không được nhập nhằng với write sink.
- Subscription là event/read-disclosure track riêng với identity và tenant oracle khác Query.
- Không tái sử dụng Query score một cách máy móc cho hai root này.

---

## 7. Annotator + Risk Scorer

Score = **chỉ tín hiệu exploitability của path** (tier KHÔNG còn ở đây). Weights gợi ý, tunable:

| Signal | Δ score |
|--------|---------|
| Object-selector reach sink (read) | +3 |
| Independent `node(id:)`/`nodes(ids:)` CAP | +2 |
| Authz-modifier trên path | +2 |
| Direct flow (selector liền sink) | +1 |
| **Self-scope sanitizer** giữa selector→sink (`current*`,`me`,`viewer`,`my*`) | −2 (annotate) |
| **Visibility sanitizer** tại terminal (`public*`,`sorare*`,listing) | −2 (annotate) |
| Depth (mỗi hop > N) | −0.2/hop |
| Selector duy nhất là UNKNOWN | −1 |

- **Sanitizer = annotate + giảm điểm, KHÔNG drop.** Class bug ta săn *chính là* "sanitizer lỗi"
  (`publicWatchlists` rò private) → không được xoá. Path qua sanitizer tụt đáy nhưng vẫn còn.
- **KHÔNG prune gì (đổi từ v3).** Không xoá CAP nào — kể cả không selector/sink yếu thì cũng **xếp
  xuống đáy** (floor score), runtime budget tự quyết cutoff. Static không xoá.
- **`impact`** là field riêng (suy từ tier + category sensitive-field: PII/tài chính/secret = cao),
  sort độc lập với score.
- `ownership_continuity=unknown` là runtime annotation, không tự động cộng/trừ score.

Thứ tự triage mặc định cho bug-bounty profile:

1. direct Query selector CAP;
2. independent global-ID CAP;
3. indirect selector-bearing CAP;
4. sanitizer/self-scoped CAP;
5. sink chỉ có name-based evidence và không có selector.

Name-based sink signals là recall insurance. Chúng không được xếp trên direct/global access surface
chỉ vì type có nhiều keyword nhạy cảm.

Áp Watchlist: 65 path `publicWatchlists` → score thấp (visibility-sanitizer), `watchlist(id:)` +
`node()` → top. Không cái nào bị xoá.

---

## 8. JSON suspect artifact (sketch)

```json
{
  "schema_fingerprint": "sha256:...",
  "analyzer_version": "3.2",
  "scope": ["query"],
  "sink": {
    "type": "UserDevice",
    "tiers": ["T3"],
    "impact": "medium",
    "globally_addressable": true,
    "evidence": [
      {"rule":"sensitive-type-token", "matched_tokens":["user","device"]},
      {"rule":"sensitive-field:pii", "field":"userAgent"},
      {"rule":"implements-node", "interface":"Node"}
    ]
  },
  "caps": [
    {
      "kind": "read",
      "entry": "node",
      "path": [
        {"kind":"FIELD","type":"Query","field":"node",
         "args":[{"name":"id","type":"ID!","class":"object-selector"}]},
        {"kind":"TYPE_CONDITION","type":"Node","on":"UserDevice"}
      ],
      "selectors": ["id"],
      "sanitizers": [],
      "flow": "direct-global-id",
      "seed_requirement": {"kind":"global-id","type":"UserDevice"},
      "ownership_continuity": "unknown",
      "score": 6.0,
      "score_breakdown": [
        {"signal":"object-selector","delta":3},
        {"signal":"global-id-track","delta":2},
        {"signal":"direct-flow","delta":1}
      ],
      "confidence": "high"
    }
  ]
}
```

### Coverage report

Artifact của mỗi run phải kèm coverage report để bước manual review nhìn được vùng classifier bỏ qua:

```json
{
  "profile": "bug-bounty",
  "calibration_snapshot": true,
  "all_object_types_excluding_query": 1041,
  "query_reachable_object_types": 790,
  "query_unreachable_object_types": 251,
  "selected_sinks": 293,
  "unselected_count": 497,
  "unselected_types": [
    {"type":"AlternativeRewardConfig","weak_signals":["domain:reward"],"globally_addressable":false},
    {"type":"Blueprint","weak_signals":[],"globally_addressable":false}
  ],
  "rule_counts": {
    "direct-query-selector-entrypoint": 25,
    "direct-query-selector-concrete-type": 228,
    "globally-addressable": 218,
    "node-implementers-unselected": 0
  },
  "regression": {
    "financial-fields-restored-by-morphology": 15,
    "types-tipped-by-financial-morphology": 0,
    "types-added-by-all-morphology": ["Notifications"]
  }
}
```

`unselected_types` là output bắt buộc, không chỉ count. Review nên ưu tiên spot-check:

- globally addressable nhưng chưa được chọn (v3.2 rule mới phải làm tập này rỗng);
- type có weak domain signal;
- type reachable qua direct/short Query path;
- random sample để ước lượng classifier false-negative rate.

Mỗi thay đổi lexicon phải diff `unselected_types` và chạy regression invariants:

```text
Node.possibleTypes ∩ unselected_types == empty
bare ambiguous term alone does not create a strong signal
restored morphology fields remain present
Query-unreachable types do not alter Query calibration counts
```

---

## 9. End-to-End Trace: `UserDevice`

Phần này trace một sink xuyên suốt static pipeline để thấy rõ dữ liệu được biến đổi như thế nào.

### 9.1 Input schema facts

Introspection cung cấp các facts sau:

```graphql
type Query {
  currentUser: CurrentUser
  node(id: ID!): Node
  nodes(ids: [ID!]!): [Node]!
}

type CurrentUser {
  currentDevice: UserDevice
  devices: [UserDevice!]
}

type UserDevice implements Node {
  id: ID!
  deviceType: String!
  lastUsedAt: ISO8601DateTime
  os: String!
  userAgent: String!
}

interface Node {
  id: ID!
}
```

Ngoài ra, `Node.possibleTypes` chứa `UserDevice`.

Từ schema facts này có hai access surface độc lập:

```text
normal traversal: Query.currentUser -> CurrentUser.devices -> UserDevice
global ID:        Query.node(id:) -> ... on UserDevice
```

Không được collapse hai surface thành một path hoặc để score của path này ảnh hưởng path kia.

### 9.2 Parse thành Schema IR

Parser tạo IR tối thiểu tương đương:

```json
{
  "types": {
    "UserDevice": {
      "kind": "OBJECT",
      "interfaces": ["Node"],
      "fields": [
        {"name":"id", "returnType":"ID!"},
        {"name":"deviceType", "returnType":"String!"},
        {"name":"lastUsedAt", "returnType":"ISO8601DateTime"},
        {"name":"os", "returnType":"String!"},
        {"name":"userAgent", "returnType":"String!"}
      ]
    },
    "Node": {
      "kind": "INTERFACE",
      "possibleTypes": ["...", "UserDevice", "..."]
    }
  }
}
```

IR giữ interface relationship. Tool cũ chỉ theo return kind `OBJECT`, nên bỏ
`Query.node -> Node`; enumerator mới không được bỏ edge này.

### 9.3 Stage 1 chọn sink

Tokenizer và classifier xử lý:

```text
UserDevice -> [user, device]
deviceType -> [device, type]
userAgent  -> [user, agent]
```

Áp hai lexical policies:

- `device` là clear PII/device-domain token.
- `user` là ambiguous bare token, không tự tạo signal.
- `user + agent` là contextual sequence, được gắn PII/device metadata.
- `UserDevice implements Node` tạo structural signal `globally_addressable`.

Sink record:

```json
{
  "type": "UserDevice",
  "selected": true,
  "tiers": ["T3"],
  "categories": ["pii"],
  "globally_addressable": true,
  "evidence": [
    {"rule":"sensitive-type-token", "matched_tokens":["device"]},
    {"rule":"sensitive-field:pii", "field":"deviceType", "matched_tokens":["device"]},
    {"rule":"sensitive-field:pii", "field":"userAgent", "matched_sequence":["user","agent"]},
    {"rule":"implements-node", "interface":"Node"}
  ]
}
```

`UserDevice` không thể rơi vào `unselected_types`, vì invariant yêu cầu mọi concrete
`Node.possibleTypes` phải được chọn.

### 9.4 Build graph

Normal graph edges:

```text
FIELD Query.currentUser       -> CurrentUser
FIELD CurrentUser.devices     -> UserDevice
FIELD CurrentUser.currentDevice -> UserDevice
```

Global-ID graph edges:

```text
FIELD Query.node(id: ID!) -> Node
TYPE_CONDITION Node       -> UserDevice
```

`TYPE_CONDITION` không phải GraphQL field. Nó biểu diễn requirement phải dùng inline fragment
`... on UserDevice` khi dựng query.

### 9.5 Enumerate CAPs

Enumerator sinh ít nhất ba CAP độc lập:

```text
CAP-A: Query.currentUser -> CurrentUser.devices -> UserDevice
CAP-B: Query.currentUser -> CurrentUser.currentDevice -> UserDevice
CAP-C: Query.node(id:) -> TYPE_CONDITION(UserDevice)
```

`graphql-path-enum` cũ chỉ tìm được CAP-A và CAP-B. Nó tìm được `0` path qua `node(id:)` vì không
theo interface. Independent global-ID track phục hồi CAP-C.

### 9.6 Classify arguments and annotate paths

CAP-A:

```json
{
  "selectors": [],
  "sanitizers": [
    {"kind":"self-scope", "field":"Query.currentUser"}
  ],
  "flow": "none"
}
```

CAP-B:

```json
{
  "selectors": [],
  "sanitizers": [
    {"kind":"self-scope", "field":"Query.currentUser"},
    {"kind":"self-scope", "field":"CurrentUser.currentDevice"}
  ],
  "flow": "none"
}
```

CAP-C:

```json
{
  "selectors": [
    {"field":"Query.node", "arg":"id", "type":"ID!", "class":"object-selector"}
  ],
  "sanitizers": [],
  "flow": "direct-global-id",
  "seed_requirement": {"kind":"global-id", "type":"UserDevice"},
  "ownership_continuity": "unknown"
}
```

CAP-C không kế thừa sanitizer từ CAP-A/B. Chúng là ba access paths riêng.

### 9.7 Score

Với weights hiện tại:

CAP-A:

```text
self-scope sanitizer: -2
total:                -2
```

CAP-B:

```text
self-scope Query.currentUser:          -2
self-scope CurrentUser.currentDevice:  -2
total:                                 -4
```

Penalty thứ hai là kết quả của naming heuristic `current*`. Static chưa chứng minh
`CurrentUser.currentDevice` tái lập thêm một authorization boundary; vì vậy artifact phải giữ
evidence `heuristic:name-prefix`, và weight này cần calibration thay vì coi là semantic fact.

CAP-C:

```text
object-selector:   +3
global-ID track:   +2
direct flow:       +1
total:             +6
```

Ranked order:

```text
1. CAP-C  score +6  Query.node(id:) -> UserDevice
2. CAP-A  score -2  Query.currentUser.devices -> UserDevice
3. CAP-B  score -4  Query.currentUser.currentDevice -> UserDevice
```

Điểm quan trọng: CAP-A/B không bị xoá. Nếu `currentUser` resolver bị lỗi, runtime vẫn có thể kiểm
tra chúng sau CAP-C.

### 9.8 Generate candidate queries

CAP-A:

```graphql
query CurrentUserDevices {
  currentUser {
    devices {
      id
      deviceType
      lastUsedAt
      os
      userAgent
    }
  }
}
```

CAP-C:

```graphql
query UserDeviceByGlobalId($id: ID!) {
  node(id: $id) {
    ... on UserDevice {
      id
      deviceType
      lastUsedAt
      os
      userAgent
    }
  }
}
```

Static phase chỉ sinh query candidate. Nó chưa có giá trị `$id` và chưa biết requester nào được phép
đọc device nào.

### 9.9 Static artifact output

Artifact rút gọn:

```json
{
  "sink": {
    "type": "UserDevice",
    "tiers": ["T3"],
    "impact": "medium",
    "globally_addressable": true
  },
  "caps": [
    {
      "id": "user-device:node",
      "entry": "node",
      "flow": "direct-global-id",
      "selectors": ["id"],
      "sanitizers": [],
      "seed_requirement": {"kind":"global-id", "type":"UserDevice"},
      "score": 6.0
    },
    {
      "id": "user-device:current-user-devices",
      "entry": "currentUser",
      "flow": "none",
      "selectors": [],
      "sanitizers": ["currentUser"],
      "score": -2.0
    },
    {
      "id": "user-device:current-device",
      "entry": "currentUser",
      "flow": "none",
      "selectors": [],
      "sanitizers": ["currentUser", "currentDevice"],
      "score": -4.0
    }
  ]
}
```

### 9.10 Handoff to runtime

Runtime phase cần ground truth trước khi test CAP-C:

1. Identity A lấy device ID thuộc A qua `currentUser.devices`.
2. Identity B gọi `node(id: A_DEVICE_ID)` với inline fragment `UserDevice`.
3. So sánh response của owner A, non-owner B và anonymous nếu hợp lệ.
4. Ghi nhận `null`, authorization error, masked fields hay full object.

Expected secure behavior phụ thuộc product policy, nhưng một kết quả như sau là suspect mạnh:

```text
A (owner):     full UserDevice
B (non-owner): full UserDevice
anonymous:     full UserDevice
```

Kết quả static không được gọi đây là vulnerability. Chỉ runtime diff-oracle cùng policy/ground truth
mới xác nhận BAC.

### 9.11 Observed calibration run

CAP-C đã được chạy read-only với hai test identities và anonymous control. Seed lấy từ Identity B
qua browser-session `currentUser.currentDevice`.

```text
Owner B:     full UserDevice, 0 errors, no masking
Non-owner A: full UserDevice, 0 errors, no masking
Anonymous:   full UserDevice, 0 errors, no masking
```

Ba response có `.data.node` giống hệt nhau. Runtime status hiện là
`strong-bac-suspect`; confirmation gate còn lại là expected visibility policy của `UserDevice`.
Chi tiết nằm trong [`../research/runtime-cap-c-user-device.md`](../research/runtime-cap-c-user-device.md).

---

## 10. Changelog v3.1 → v3.2

1. Scope hiện tại chốt **Query-only**; Mutation/Subscription deferred thành track độc lập.
2. Bỏ `max_paths`; run phải complete theo cycle policy hoặc báo fail/incomplete ở cấp run.
3. Cấm substring lexicon rộng; thêm tokenization, canonical morphology và contextual sequence.
4. `globally_addressable` trở thành sink-selection signal và sinh CAP `node(id:)` độc lập.
5. Thêm `TYPE_CONDITION` edge để biểu diễn interface/union và sinh inline fragment.
6. Thêm `ownership_continuity`/identity-transition annotation cho indirect CAP.
7. Front-load direct selectors và global-ID CAP trước name-based sink evidence.
8. Coverage report bắt buộc xuất full `unselected_types` để manual review vùng mù.
9. T2 không loại type có policy; T3 không phụ thuộc `in_public_collection`.
10. Matcher chia clear-domain và ambiguous-context policy; chỉ dùng light morphology, không stemming.
11. Sink calibration chỉ tính Query-reachable types; `unselected_types` trở thành regression surface.

---

## 11. Giới hạn

- Static không thấy resolver → FP (arg `id` có thể bị ép khớp requester). Runtime confirm.
- Sanitizer/policy nhận diện bằng tên → có thể sai; UNKNOWN giữ low-score để bù.
- Directive signal phụ thuộc SDL.
- "All paths" nghĩa là toàn bộ simple edge-path theo cycle policy, không phải vô hạn cyclic walks.
- Global-ID CAP chứng minh type addressable về mặt schema; runtime vẫn cần seed ID hợp lệ của type đó.
- Bug-bounty sink profile cố ý chấp nhận false negative; full `unselected_types` là coverage control.
- Output = **ranked suspect list**, KHÔNG phải confirmed vulnerabilities.
