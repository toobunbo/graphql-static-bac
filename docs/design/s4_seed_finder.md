# Seed Finder — Kiến trúc ý tưởng (bản nháp)

Stage tìm **đường lấy seed thật** cho fuzzer, gắn ngay **sau S3** của GraphQL Static BAC Framework.
Tài liệu này là *ý tưởng + phương pháp*, không chứa code. Mục tiêu để review: lấy input gì, xử lý
thế nào, ra output gì, và minh hoạ trên 2 path cụ thể.

> Quan hệ với framework cũ: thay cho **S4** (đã quyết bỏ), ta phát triển sang nhánh seed-finding.
> Pipeline đích: `S3 route → seed path → runtime lấy seed → Oracle → runtime bug`.

---

## 1. Mục tiêu

Với một **argument cần seed** (vd `Query.anyCard.assetId`, hoặc selector của một route từ S3 như
`MarketRoot.watchlist.id`), tìm các **đường (path) lấy được một GIÁ TRỊ THẬT** hợp lệ cho argument đó,
kèm đủ thông tin để dựng query chạy được và để runtime harvest.

Không tự bịa giá trị. Chỉ dùng seed thật, lấy từ chính API.

**Phạm vi stage này = đúng một việc: lấy seed *đúng*.** KHÔNG phân loại victim/owner, KHÔNG bug oracle,
KHÔNG quyết định "thế nào là bug" — những thứ đó nằm ở downstream/runtime, tính sau. Mọi producer source
(self-scope hay public) ở đây **ngang hàng**, chỉ là nguồn lấy giá trị; khác nhau ở *độ tin / khả năng chạy*,
không phải "của ai".

**Runtime ở stage kế chỉ là seed-validity check**, không phải oracle:
- argument được API chấp nhận;
- không lỗi type/format;
- consumer trả về object/collection phù hợp;
- nếu có identity field → xác nhận producer và consumer tương ứng.

---

## 2. Nguyên tắc nền

1. **Real-seed-first.** Luôn ưu tiên giá trị thật do API trả về. KHÔNG sinh giá trị giả/random làm seed.
   Thứ tự ưu tiên *nguồn* (rẻ → đắt, an toàn → có side-effect): **đọc self-scope → đọc nhánh khác →
   tạo mới qua mutation**. "Tạo mới" vẫn là seed thật (object thật vừa tạo), chỉ tốn hơn và có write.
2. **Tái dùng engine, không xây engine mới.** Seed-finding = chạy lại engine reachability của S3 ở
   **structural mode** với target là một **field output** (không cần selector, không cần verdict).
3. **Enumerate theo signature, KHÔNG theo simple-path.** Lấy *toàn bộ nhánh nguồn-seed phân biệt*
   (mỗi signature một witness đại diện), không lấy mọi simple-path → tránh explosion (tái dùng đúng
   signature compaction của S3).
4. **Tách "tìm nhánh" khỏi "chọn nhánh".** Stage này ra *toàn bộ* candidate; bộ **filter/ranker nằm
   downstream, mỏng**, đổi policy không đụng stage này.
5. **Defer verification sang runtime.** Static không chứng minh được "seed này đúng entity" → để runtime
   biết. Static chỉ cần đảm bảo **executability** (dựng nổi query để chạy).
6. **Artifact-passing.** Đọc file vào → ghi `seed_artifact.json` ra → test độc lập, như mọi stage khác.

---

## 3. Vị trí trong pipeline

```text
schema_ir.json ─┐
sinks.json ─────┤
args.json ──────┤
routes (S3) ────┘
        │
        ▼
  [S-seed] Seed Finder ──────► seed_artifact.json
        │  (tái dùng engine reachability ở structural mode,
        │   target = field producer suy ra từ selector)
        ▼
  [downstream] Filter/Ranker → chọn 1..n đường dựng query
        ▼
  Query Emitter → runtime (seed-validity check) → seed sẵn dùng
        ▼
  ┄┄ ranh giới stage ┄┄►  [fuzzer downstream: bind seed + oracle]  (ngoài scope)
```

Engine reachability là **lõi dùng chung**: S3 gọi nó với target = sink (BAC mode); S-seed gọi nó với
target = field producer (structural mode). Chỉ thêm một *nguồn target* và phần hậu xử lý seed.

---

## 4. Phân loại nguồn seed (tất cả đều là seed THẬT, ngang hàng)

Phân loại theo **cách lấy giá trị**, KHÔNG theo "của ai". Khác nhau ở *độ tin / khả năng chạy / có side-effect*.

| Mã | Nguồn | Ví dụ | Tính chất |
|----|-------|-------|-----------|
| **B** | Đọc qua guardian root | `currentUser → myWatchlists → … → Watchlist.id` | read, không side-effect, thường non-empty & ổn định |
| **C** | Đọc qua nhánh khác | `publicWatchlists → … → Watchlist.id`, `node(id:)`, cross-object | read, có thể empty/kém ổn định hơn |
| **A** | Tạo mới qua mutation | `createWatchlist(...) → payload.watchlist.id` | **write**, side-effect, đảm bảo tồn tại |

Ghi chú phạm vi: nhánh **A đụng Mutation** — hiện framework để *Query-only*, Mutation deferred. Vì vậy
**MVP = B + C** (harvest bằng query). A được *thiết kế chỗ sẵn* (cùng engine, root = Mutation, đọc field
trong payload) nhưng **bật sau** khi mở scope Mutation. Đây là fallback đảm-bảo-tồn-tại, không phải hàng giả.

Vì sao vẫn giữ **nhiều** nhánh dù không phân loại victim/owner: chúng là các *producer source thay thế* cho
cùng một seed. Nhánh B có thể rỗng lúc runtime (bạn không sở hữu object nào) → cần C hoặc A để cứu. Filter
downstream chọn nhánh theo độ tin/executability, không theo mục đích bug.

---

## 5. Input (đọc gì)

| Input | Lấy gì từ đó |
|-------|-------------|
| `routes` (S3) | danh sách **selector cần seed**: `selector.arg_ref`, `selected_type_id`, `arg_path` (leaf), `type_ref` |
| `schema_ir.json` (S0) | graph để reachability; **nguồn duy nhất cho required-args** (arg nào tồn tại, Non-Null, có default chưa); field args, type_ref, interfaces, possible_types, kind |
| `args.json` (S2) | CHỈ để biết một required-arg **có phải `object_selector` không** → nếu phải thì nó là seed-lồng-seed |
| `sinks.json` (tuỳ) | nếu cần biết target nào thuộc route nào |

Không cần input runtime ở stage này (toàn bộ là static).

---

## 6. Phương pháp (duyệt 1 lần, 5 bước)

> **Batch, KHÔNG per-route.** Nhiều route open/unknown dùng chung một selector → cùng một seed. Vì vậy ta
> duyệt toàn bộ S3 **một lần**, **dedup theo `seed_target_id`**, và reachability **mỗi seed một lần** —
> không lặp lại cho từng route. Hai pass: pass 1 (rẻ) gom & dedup target; pass 2 (đắt) chạy engine cho từng
> target phân biệt.

### Bước 0 — Quét toàn bộ S3 một lần, dedup seed target

Duyệt **tất cả route `open` + `unknown`**. Với mỗi route, lấy selector → suy ra `seed_target` (Bước 1). Gom
vào một dict **khoá theo `seed_target_id`**; mỗi lần gặp lại thì **không tạo group mới**, chỉ append route vào
`consumed_by[]` của group đó. Kết quả: tập **seed target phân biệt** + ánh xạ ngược `target → [route tiêu thụ]`.

- 50 route cùng cần `Watchlist.id` ⇒ **1 group, 1 lần reachability, 1 lần harvest runtime**, 50 back-ref.
- Route mà selector không suy ra được target (noise / không có producer) → đẩy vào `unresolved_targets[]` để
  coverage trung thực (recall-first), không nuốt im.

Đây cũng là chỗ **giải quyết** câu hỏi cũ "map seed ↔ điểm bơm": quan hệ nằm trong `consumed_by[]`.

### Bước 1 — Suy ra seed target từ selector (deterministic, KHÔNG NLP)

Từ một selector của S3:

```text
selected_type_id  + leaf(arg_path)  + type_ref
        │                │               │
        ▼                ▼               ▼
  type cần seed     tên field khớp   kiểu phải tương thích
```

Luật: **seed_target = field output trên `selected_type`, tên khớp leaf, kiểu tương thích.**

- `arg:MarketRoot.watchlist.id` → `type:Watchlist` + leaf `id` + `ID!` ⇒ `field:Watchlist.id`.
- `arg:Query.anyCard.assetId` → `type:AnyCardInterface` + leaf `assetId` + `String` ⇒ `field:AnyCardInterface.assetId`.

**Target là SCALAR → mô hình "reach parent + append".** `Watchlist.id`/`Card.assetId` là *lá*, không phải
node để duyệt tới. Vì vậy: reachability nhắm tới **parent type** (`Watchlist`/`Card`) — đúng việc engine S3
vốn làm — rồi **append output field scalar** (`.id`/`.assetId`) làm terminal read. Reachability target = parent
object type; scalar field chỉ là projection cuối.

**Phân biệt field interface vs field concrete.** Nếu `selected_type` là interface (`AnyCardInterface`), field
producer có thể là:
- field **khai trên interface** (`AnyCardInterface.assetId`) → đọc được trên mọi impl, **KHÔNG cần** `... on Card`;
- field **của concrete** (`Card.assetId`) → **cần** `TYPE_CONDITION` (`... on Card`).

Đây là hai `field_id` khác nhau, quyết định cả (a) chọn đúng producer và (b) sinh query đúng (có/không type-condition).

**Tương thích kiểu:** coi **ID ↔ String hoán đổi được** (như SGAFuzzer nới cho ID-like). Nếu không có field nào
khớp leaf → fallback: field id-ish bất kỳ cùng type → cuối cùng mới cần heuristic (residue nhỏ).

**Mỗi lần suy không phải đúng/sai cứng — phải có evidence + confidence.** Ghi lại `target_derivation`:

```json
{ "method": "exact_leaf_match | type_only | fallback_id_ish | heuristic",
  "evidence": "leaf 'assetId' == field 'AnyCardInterface.assetId'; type String≈String",
  "field_locus": "interface | concrete",
  "confidence": "high | medium | low" }
```

> Đây chính là thứ thay cho bước "dependency object mapping" bằng NLP/fuzzy của SGAFuzzer — nhưng làm
> deterministic vì S2 đã gắn `selected_type_id`. Phần residue (không khớp leaf) mới rơi xuống heuristic.

### Bước 2 — Chạy engine tới parent type + giải required-args TRƯỚC khi compact

Nạp **parent type** của seed_target làm target, chạy engine **structural** (chỉ cần reachable + witness).
Plumbing (Connection/Edge/nodes) và TYPE_CONDITION (`… on Card`) xử như S3 đã làm.

**Quan trọng — thứ tự:** required-args phải được **giải ngay trong lúc traversal**, *trước* khi compact path,
chứ không phải gắn vào sau. Lý do: signature gom path theo (mục 7) có chứa `unresolved_arg_refs`; nếu compact
trước rồi mới tính args, hai path khác nhau về "chạy được / cần seed lồng" đã bị gộp mất witness. Nên mỗi state
trong reachability mang theo tập `unresolved_arg_refs` tích luỹ; compaction tôn trọng tập đó.

### Bước 3 — Required-args mỗi hop (nguồn = S0 IR) + bắt seed-lồng-seed

Đi dọc witness, với mỗi FIELD edge lấy required-args **từ `schema_ir.json` (S0)** — đây là nguồn duy nhất cho
"arg nào Non-Null & chưa có default". Phân loại từng required-arg:

- **pagination** (`first`/`last`/`limit`…) → `binding = generated_default` (vd `first: 20`). Không phải seed.
- **đã có default trong schema (S0 IR)** → bỏ qua.
- **là `object_selector` khác** — kiểm bằng `args.json` (S2) → **seed lồng seed**: đẩy `arg_ref` vào
  `unresolved_arg_refs[]`. Đây là điểm có thể đệ quy → chặn (giới hạn độ sâu / loại nhánh nếu không tự giải quyết).

S0 IR cho biết *arg có required không*; S2 chỉ trả lời *arg required đó có phải selector không*. Kết quả mỗi
nhánh: `binding_plan[]` + `unresolved_arg_refs[]`. Rỗng ⇒ **chạy được, tự chứa**.

### Bước 4 — Phát candidate

Mỗi nhánh thành một `seed_candidate` (mục 8), gắn vào group theo `seed_target_id`. KHÔNG chọn ở đây — chọn là
việc của filter downstream. Mỗi candidate phải đủ để runtime **dựng query harvest** (witness + `binding_plan`)
**và rút giá trị** (`read_path` + `read_cardinality`).

---

## 7. Seed-signature & tính đầy đủ ("toàn bộ nhánh")

Để "toàn bộ nhánh" vừa **đủ** vừa **không nổ**, gom theo signature thay vì simple-path. Đề xuất khoá:

```text
seed_signature = (
  source_class,          # B | C | A
  root_class,            # guardian | public | mutation | other
  boundary_summary,      # self_scope | visibility | none
  terminal_field_id,     # field producer chạm tới (vd field:Watchlist.id)
  field_locus,           # interface | concrete (đọc qua type-condition hay không)
  unresolved_arg_refs    # *** TẬP ref cụ thể, KHÔNG phải count ***
)
```

Vì sao `unresolved_arg_refs` (tập ref) chứ KHÔNG phải `count`: hai path cùng *số* arg chưa giải quyết có thể
cần *arg khác nhau* — một cái còn harvest được, một cái không. Count gộp nhầm hai nhánh khác bản chất; tập ref
giữ đúng danh tính. (Và đúng như Bước 2: phải giải args **trước/trong khi** compact thì tập ref này mới có để
đưa vào signature.)

Signature của S3 cũ gom theo đặc trưng *verdict*, KHÔNG có executability → giữ một witness/signature có thể
giấu mất nhánh chạy được. Thêm `unresolved_arg_refs` + `field_locus` vào khoá thì tập nhánh đầy đủ *cả về khả
năng chạy*. (Hoặc giữ top-k witness/signature theo executability.)

"Toàn bộ" vẫn bị chặn bởi `max_depth`/cycle — với seed thì chấp nhận được vì ta vốn thích producer **nông,
đáng tin**.

---

## 8. Output contract — `seed_artifact.json`

Bám envelope + Stable-ID hiện có của framework.

```json
{
  "contract_version": "…",
  "stage": "seed_finder",
  "schema_fingerprint": "…",
  "data": {
    "seed_groups": [
      {
        "seed_target_id": "field:AnyCardInterface.assetId",
        "type_ref": { "display": "String", "named_type": "String" },
        "target_derivation": {
          "method": "exact_leaf_match", "field_locus": "interface",
          "evidence": "leaf 'assetId' == AnyCardInterface.assetId; String≈String",
          "confidence": "high"
        },

        "consumed_by": [
          { "route_id": "route:sha256:f23f…", "verdict": "unknown",
            "selector_ref": "arg:Query.anyCard.assetId", "arg_path": "Query.anyCard.assetId" }
        ],

        "candidates": [
          {
            "candidate_id": "seed:sha256:…",
            "source_class": "B",
            "root_class": "guardian",
            "boundary_summary": "self_scope",
            "field_locus": "concrete",
            "witness": { "...": "… → ... on Card → Card.assetId (giống format witness S3)" },
            "binding_plan": [
              { "field_id": "field:CurrentUser.cards",
                "arg": "first", "type": "Int",
                "binding": "generated_default", "value": 20 }
            ],
            "read_path": "currentUser.cards.nodes[].assetId",
            "read_cardinality": "list",
            "unresolved_arg_refs": [],
            "executable": true,
            "confidence": "structural"
          }
        ]
      }
    ],
    "unresolved_targets": [
      { "route_id": "route:sha256:…", "selector_ref": "arg:…",
        "reason": "no_producer_field | selector_is_noise | type_incompatible" }
    ]
  }
}
```

Đủ cho runtime:
- **Dựng query harvest** ← `witness` + `binding_plan` (+ `field_locus` để biết có cần `... on Card` không).
- **Rút giá trị** ← `read_path` (vị trí trong response) + `read_cardinality` (`list` → connection trả nhiều, lấy một/nhiều).
- **Bơm seed vào điểm tiêu thụ** ← `consumed_by[].arg_path`. Mỗi route tiêu thụ chọn candidate từ cùng pool theo
  **độ tin / executability** (không theo mục đích bug): ưu tiên nhánh `executable` + nguồn ổn định.

`confidence: structural` ở static; runtime nâng/hạ bằng **seed-validity check** (arg được chấp nhận, không lỗi
type/format, consumer trả object/collection phù hợp, identity field khớp nếu có) — KHÔNG phải oracle.

---

## 9. Ví dụ chi tiết

### 9.1 — Target `field:Watchlist.id` (minh hoạ "toàn bộ nhánh" + filter)

**Input:** selector `arg:MarketRoot.watchlist.id` (route 1, `open`), `selected_type_id: type:Watchlist`,
leaf `id`, `ID!`.

**Bước 1:** seed_target = `field:Watchlist.id`.

**Bước 2 + 3:** engine ra (≥) các nhánh sau, gom theo signature (parent = `Watchlist`, append `.id`):

| # | source | witness (rút gọn) | required-args | unresolved_arg_refs | executable |
|---|--------|-------------------|---------------|---------------------|------------|
| B | guardian/self_scope | `currentUser → myWatchlists → nodes → Watchlist.id` | `myWatchlists(first:20)` | `[]` | ✅ |
| C1 | public/visibility | `market → publicWatchlists → nodes → Watchlist.id` | `publicWatchlists(first:20)` | `[]` | ✅ |
| C2 | global node | `node(id:) → … on Watchlist → Watchlist.id` | `node(id: ???)` | `[arg:Query.node.id]` | ❌ |
| A | mutation (deferred) | `createWatchlist(...) → payload.watchlist → Watchlist.id` | input createWatchlist | tuỳ | (write) |

Quan sát:
- **C2 tự mâu thuẫn**: muốn lấy một `id` lại cần sẵn một `id` → `unresolved_arg_refs=[arg:Query.node.id]` → loại (hoặc hạ đáy).
- **B và C1 ngang hàng** — đều là producer hợp lệ, đều `executable`. Khác nhau ở độ tin (B self-scope thường non-empty hơn).
- **A** deferred theo scope Query-only; để dành bootstrap khi cả B lẫn C1 rỗng lúc runtime.

**Filter downstream chọn theo độ tin / executability** (KHÔNG theo mục đích bug):
- loại C2 (không executable);
- ưu tiên B (nguồn ổn định) → nếu runtime B rỗng thì rơi sang C1 → cuối cùng A.

**Query dựng từ B:**

```graphql
query { currentUser { myWatchlists(first: 20) { nodes { id } } } }
```

→ runtime trả danh sách `id` thật → seed-validity check → bơm vào `market.watchlist(id: <seed>)` ở các route tiêu thụ.

### 9.2 — Target `field:AnyCardInterface.assetId` (minh hoạ type-compat + interface expansion)

**Input:** selector `arg:Query.anyCard.assetId` (route 3), `selected_type_id: type:AnyCardInterface`,
leaf `assetId`, **`String`** (không phải `ID!`).

**Bước 1:** seed_target = `field:AnyCardInterface.assetId`. Selector `String`, producer `assetId: String`
→ tương thích. (Nếu selector là `ID!` mà producer `String` → vẫn coi hợp lệ nhờ luật ID↔String.)

**Bước 2:** nhánh self-scope, có **TYPE_CONDITION** vì đi qua interface:

```text
Query.currentUser
  → CurrentUser.cards
  → CardConnection.nodes
  → ... on Card                # TYPE_CONDITION: AnyCardInterface → Card
  → Card.assetId               # field producer (String) ✓
```

**Bước 3:** `cards(first: 20)` là pagination → generated_default. `unresolved_arg_refs: []` → executable.

**Field locus:** `assetId` ở đây đọc trên **concrete** (`Card.assetId`) nên witness có `... on Card`. Nếu
`assetId` được khai ngay trên interface `AnyCardInterface` thì có nhánh `field_locus: interface` **không cần**
`... on Card` — engine giữ cả hai nếu cùng reachable.

**Query:**

```graphql
query { currentUser { cards(first: 20) { nodes { ... on Card { assetId } } } } }
```

Đây chính là ví dụ gốc bạn nêu ở đầu (`Card.assetId`), giờ ra từ pipeline một cách tự nhiên: reach parent
`Card` + append `assetId`, interface→concrete qua TYPE_CONDITION, plumbing, pagination-default — tái dùng engine S3.

---

## 10. Filter downstream (mỏng, ngoài scope stage này)

Chỉ là policy trên `candidates[]`, không đụng engine. Tín hiệu gợi ý (đều về *lấy seed đúng*, không về bug):

1. `executable == true` (loại nhánh còn `unresolved_arg_refs`).
2. Độ tin của nguồn: guardian/self_scope thường non-empty hơn public.
3. Không side-effect (B/C) trước, có write (A) sau.
4. `field_locus: interface` (đọc thẳng) gọn hơn `concrete` (cần `... on Card`) khi cả hai cùng được.
5. Pagination bị chặn; witness ngắn hơn khi hoà.

---

## 11. Tái dùng vs viết mới

**Tái dùng nguyên:** engine reachability (structural mode), witness format, signature compaction, xử lý
plumbing/TYPE_CONDITION, S0 IR, `args.json`, envelope/Stable-ID.

**Viết mới (nhỏ):**
1. **Batch scan + dedup** — duyệt toàn bộ route open/unknown 1 lần, gom theo `seed_target_id`, dựng `consumed_by[]` (pass 1).
2. **Seed-target deriver** — selector → field producer (reach-parent + append scalar; leaf-match + ID↔String + fallback; interface vs concrete; kèm `target_derivation` evidence/confidence).
3. **Arg-resolution trong traversal** — giải required-args (nguồn **S0 IR**) + bắt seed-lồng-seed → `unresolved_arg_refs`, **trước khi compact**.
4. **Seed-signature** — gồm `unresolved_arg_refs` (tập ref) + `field_locus` để đủ về executability.
5. **(sau) Query emitter** — witness + binding_plan + field_locus → GraphQL string + `read_path`.
6. **(sau) A — mutation producer** — khi mở scope Mutation.

---

## 12. Rủi ro & câu hỏi mở

- **Scope Mutation:** nhánh A là nguồn bootstrap mạnh nhất nhưng đang deferred. Quyết: MVP B+C, hay kéo
  một phần Mutation-create lên sớm chỉ để tạo seed?
- **Producer rỗng lúc runtime:** B (và cả C) có thể trả `[]`. Cần A để cứu → lý do giữ đủ nhánh.
- **Completeness theo executability:** đã chốt hướng — đưa `unresolved_arg_refs` (tập ref, không phải count)
  vào seed-signature, và giải args **trước khi compact**. Còn lại: có cần giữ top-k witness/signature không?
- **Type-compat:** ID↔String đủ chưa? Còn enum/custom-scalar làm selector thì sao?
- **Leaf không khớp field nào:** khi tên arg ≠ tên field output trên type → tới đâu thì dừng fallback, khi nào
  chấp nhận residue cần heuristic? (gắn `target_derivation.confidence`)
- **Interface vs concrete:** khi `assetId` có cả ở interface lẫn concrete — luôn ưu tiên interface-locus, hay
  giữ cả hai nhánh? Có schema nào field chỉ ở concrete (cần TYPE_CONDITION) không thể tránh?
- **~~Map seed ↔ điểm bơm~~ (đã chốt):** dedup theo `seed_target_id` + `consumed_by[]`.
- **Dedup key:** chỉ `seed_target_id`, hay `(seed_target_id, type_ref)` khi nhiều consumer khai báo kiểu lệch nhau?

---

*Trạng thái: bản nháp ý tưởng để review. Chưa cố định contract. Phân biệt rõ: leaf-match + reuse-engine là
**suy luận thiết kế chắc**; ngưỡng type-compat, chính sách A/scope, và mức completeness là **câu hỏi cần chốt
hoặc thực nghiệm**.*