# Route Verdict qua Selector Provenance + Selector Continuity

Chiến lược cho S3 Route Analysis: với mỗi route tới sink, quyết verdict **`open | unknown | guarded`**
bằng cách theo dõi **selector provenance** và **selector continuity** xuyên suốt route.
`query_unreachable` là **target-level** (S1), KHÔNG phải route verdict.

**Nguyên tắc nền:** recall-first. Tách *"path có tồn tại"* (structural, chắc chắn — luôn xuất hiện)
khỏi *"selector có thực sự điều khiển sink"* (semantic, thường không chứng minh được → `unknown`).
Giữ mọi route lạ làm chứng cứ; **không nâng nhầm thành `open`**.

---

## 1. Selector provenance

Mỗi selector tạo một provenance khi xuất hiện:

```text
Query.anyCard(assetId)     -> selects AnyCard
Query.node(id)             -> selects returned Node object
MarketRoot.watchlist(id)   -> selects returned Watchlist
```

State tối thiểu mang theo route:

```text
selector_provenance:
  arg_ref            # vd arg:MarketRoot.watchlist.id
  selected_type_id   # type object mà selector đang điều khiển
  classification     # object_selector | possible_selector
  confidence         # low | medium | high  (từ S2)

selector_continuity:
  same | unknown | not_applicable
```

---

## 2. Cập nhật selector continuity tại mỗi edge

`selector_continuity` trả lời:

> Selector ban đầu có còn điều khiển lựa chọn sink cuối hay không?

Nó không khẳng định hai GraphQL object là cùng identity. Khi đi qua một edge,
cập nhật quan hệ điều khiển giữa selector và object/tập kết quả trả về:

| Loại edge | Ví dụ | Continuity |
|---|---|---|
| **Direct terminal selection** | `market.watchlist(id) -> Watchlist` | **same** (selector nằm ngay field sinh sink) |
| **Same identity (TYPE_CONDITION)** | `node(id) -> Node -> ...on Watchlist` | **same** (refine cùng object) |
| **Plumbing (Connection/Edge)** | `publicWatchlists -> WatchlistConnection -> .nodes -> Watchlist` | **giữ selector continuity** (unwrap tập kết quả; không assert object identity) |
| **Identity transition** | `Card.subscription -> Subscription`, `User.account -> Account` | **unknown** (selector chọn parent, chưa chứng minh chọn child) |
| **Collection expansion** | `users(slugs) -> User -> publicWatchlists` | **unknown** (selector chọn User, không trực tiếp chọn Watchlist) |

**[Guardrail #2 — Plumbing trong suốt]** `Connection`/`Edge`/`.nodes`/`.edges`/`.node` KHÔNG phải identity
transition — chúng chỉ unwrap. Nhận diện plumbing theo **cấu trúc** (`*Connection` có `edges`/`nodes`,
`*Edge` có `node`), KHÔNG theo tên. Lưu ý: `Query.node` là **entrypoint có ý nghĩa** (chỗ selector sống),
không phải plumbing — đừng skip nhầm.

**[Milestone-1 conservative]** Chỉ kết luận **same** cho **direct field selection** + **TYPE_CONDITION**
(+ plumbing giữ selector continuity). Mọi object-to-object transition khác đặt **unknown**. Milestone đầu
không dùng `changed`; nếu sau này có structural/policy evidence rõ ràng thì mở rộng contract có version.

---

## 3. Boundary detection (conservative)

- **Self-scope:** root auth-bound (`currentUser` / `me` / `viewer`) không có identity-selector → danh tính
  khoá vào token.
- **Visibility:** field tên `public*` / listing-collection trả về tập đã lọc.
- **Domain policy:** field do config/policy profile khai báo là public/system-owned. Với schema calibration
  hiện tại, `field:MarketRoot.sorareWatchlists` thuộc visibility boundary theo policy, không suy rộng mọi
  field bắt đầu bằng `sorare*`.

Boundary semantic family vẫn là `self_scope | visibility`. `policy` là nguồn evidence, không phải family
thứ ba; output boundary ghi riêng `source: heuristic | policy`.

**[Guardrail #3 — Boundary neo theo ROOT, không fire bừa giữa path]** Self-scope fire *chắc chắn* ở
**root** auth-bound. Field `current*` nằm **giữa** đường (vd `Card.currentUserSubscription`) → coi là
**continuity-unknown**, KHÔNG assert là boundary. (Vì sao: `myWatchlists` neo `Query.currentUser` = guarded;
`anyCard -> currentUserSubscription` giữa path = unknown.)

Boundary detection skip plumbing (terminal semantic của `publicWatchlists -> ...nodes` là `publicWatchlists`).
Boundary là **heuristic, không phải bằng chứng** — luôn `guarded`, không bao giờ "chết".

---

## 4. Verdict — cây quyết định CÓ THỨ TỰ

**[Guardrail #1]** Đánh giá theo thứ tự (dừng ở nhánh đầu khớp):

```text
1. Có self-scope/visibility boundary trên đường selector→sink
   (hoặc self-scope root không selector)?          -> guarded
2. Có `object_selector` reach sink, continuity = same,
   confidence = medium|high, không boundary?          -> open
3. Còn lại (selector nhưng continuity = unknown,
   HOẶC không selector và không boundary):           -> unknown
```

**[Guardrail #4]** `classification` và `confidence` là hai trục độc lập. `possible_selector` ở bất kỳ
confidence nào, hoặc `object_selector` có confidence thấp, **không** lên `open` → coi là `unknown`.

---

## 5. Roll-up tại sink

**[Guardrail #5]** Một sink có nhiều route khác verdict:
- Ưu tiên triage của sink = verdict tốt nhất trong các route: **`open > unknown > guarded`**.
- Không gộp mọi route cùng verdict thành một. Giữ một deterministic witness cho mỗi **route signature**:

```text
(
  origin,
  selector_ref,
  terminal_semantic_edge,
  boundary_families,
  selector_continuity,
  verdict
)
```

  Nhờ vậy `node`, `nodes`, và `market.watchlist(id:)` đều được giữ dù cùng verdict `open`.
- Sink-level chỉ roll-up để ưu tiên triage; route-level vẫn giữ witness cho mọi signature phân biệt.
- **`guarded` không bao giờ bị drop.**

---

## 6. Terminal semantic edge và plumbing

Canonical structural graph vẫn giữ Connection/Edge để query generation có đủ edge thật. Route verdict dùng
một projection semantic riêng:

- `Connection`, `Edge`, `.nodes`, `.edges`, và Edge `.node` được đánh dấu plumbing bằng cấu trúc schema;
- bỏ qua plumbing khi tìm terminal semantic edge và boundary;
- không bỏ `Query.node`/`Query.nodes`, vì đó là selector entrypoint có ý nghĩa.

Ví dụ:

```text
User.publicWatchlists -> WatchlistConnection.nodes -> Watchlist
```

có terminal semantic edge:

```text
field:User.publicWatchlists
```

không phải `field:WatchlistConnection.nodes`.

---

## 7. Áp vào Watchlist (ground truth)

| Route | Phân tích | Verdict |
|---|---|---|
| `node(id) -> Node -> Watchlist` | selector `id`, TYPE_CONDITION giữ identity → same; no boundary | **open** |
| `nodes(ids) -> Node -> Watchlist` | như trên | **open** |
| `market.watchlist(id) -> Watchlist` | selector ngay terminal field → same; no boundary | **open** |
| `currentUser.myWatchlists -> Watchlist` | self-scope boundary (root `currentUser`, no selector) | **guarded/self_scope** |
| `user(slug).publicWatchlists -> Watchlist` | visibility boundary (boundary thắng dù continuity unknown) | **guarded/visibility** |
| `market.sorareWatchlists -> Watchlist` | domain policy đánh dấu public/system-owned | **guarded/visibility** |
| `anyCard(assetId) -> currentUserSubscription -> anySubscribable -> Watchlist` | Card→Subscription là identity transition → continuity unknown; mid-path `current*` KHÔNG fire boundary | **unknown** |

`anyCard` là ground-truth structural path — **vẫn phải xuất hiện**, nhưng verdict `unknown` (chưa rõ selector
Card còn điều khiển Watchlist cuối), không phải `open`.

---

## 8. Product state hữu hạn và dominance

Milestone đầu dùng finite abstraction, không mang tập provenance/boundary không giới hạn:

```text
state = (
  type_id,
  selector_class,       # none | possible | definite
  selector_ref,         # none hoặc một stable arg_ref
  selector_continuity,  # not_applicable | same | unknown
  boundary_families,    # bitset: self_scope | visibility
  terminal_semantic_edge
)
```

`selector_class`:

- `definite`: `object_selector` confidence medium/high;
- `possible`: `possible_selector`, hoặc `object_selector` confidence low;
- `none`: chưa gặp selector.

Dominance chỉ áp dụng khi hai state ở cùng `type_id`, cùng `selector_ref`, cùng terminal semantic context:

- cùng semantic state: giữ witness canonical nhỏ nhất;
- boundary bitset nhỏ hơn không được dùng để xoá state có boundary lớn hơn, vì cần giữ witness `guarded`;
- `same` không xoá `unknown`, vì chúng tạo verdict khác;
- state khác `selector_ref` không dominance nhau, để giữ `node`, `nodes`, và selector entrypoint khác nhau.

Worklist memoize full finite state. Mỗi transition chỉ tạo state trong lattice trên, nên fixed-point hội tụ.
Trong cùng một route signature, witness canonical được chọn theo số FIELD hop ít nhất, rồi tổng edge ít
nhất, rồi ordered edge-ID sequence lexicographic. Hop chỉ chọn witness dễ đọc/test hơn; **không tham gia
quyết verdict**.

### Transition khi gặp selector

Khi FIELD edge có selector facts từ S2:

- tạo một successor state cho mỗi `selector_ref`;
- provenance mới trỏ tới argument đó và `selected_type_id` là return type của FIELD;
- continuity mới là `same`, vì selector nằm ngay field đang chọn object/tập kết quả trả về;
- selector mới không bị hạ thành `unknown` chỉ vì route trước đó đã đổi identity;
- nếu selector argument optional, giữ thêm nhánh không dùng selector;
- nếu selector argument required, không tạo nhánh selector-free cho invocation đó;
- nhiều selector cùng field được giữ thành các state riêng trong milestone đầu, không gộp thành một set vô hạn.

TYPE_CONDITION và plumbing sau đó giữ continuity. FIELD object-to-object không có selector mới chuyển
continuity hiện tại thành `unknown`.

---

## 9. Triển khai hai giai đoạn

1. **Chưa có S2 — structural mode:** worklist chỉ chứng minh route **reachable** và lưu witness. Chưa phân
   loại verdict (chưa có selector/boundary facts).
2. **Có S2 — semantic mode:** **chạy lại** cùng engine product-graph worklist với state giàu
   (selector_provenance, boundary, selector_continuity) để phân loại `open | unknown | guarded`.

**[Quan trọng]** KHÔNG được chỉ enrich witness structural cũ. Witness đầu tiên có thể là `publicWatchlists`
(guarded) trong khi route khác tới cùng sink là `market.watchlist(id)` (open). Phải **re-run** worklist với
state semantic, không gắn nhãn lên witness cũ.

---

## 10. Điều kiện để engine đúng (checklist)

- Product state **hữu hạn**, memoize full state và dùng dominance bảo toàn verdict → fixed-point hội tụ.
- Một **deterministic witness** cho mỗi route signature, không chỉ mỗi verdict.
- `query_unreachable` xử ở **target-level** (S1), không trộn vào route verdict.
- Selector continuity: chỉ `same` cho direct selection + TYPE_CONDITION; plumbing giữ continuity; còn lại
  `unknown`.
- Boundary: heuristic → luôn `guarded`, **không bao giờ chết**; neo theo root; skip plumbing.
- Domain-specific visibility phải đến từ config/policy stable ID; không suy rộng bằng prefix thương hiệu.
- Terminal semantic edge bỏ plumbing nhưng canonical witness vẫn giữ đầy đủ structural edges.
- Sink roll-up `open > unknown > guarded`; giữ witness mọi verdict; `guarded` không drop.
- Semantic mode **re-run** sau S2, không enrich witness cũ.

**Stage migration (June 13, 2026):**
`S0 → (S1 ∥ S2) → S3 Route Analysis → S4 Static Seed Planning`.
Ý tưởng S4 thin ranking cũ không còn là stage production; S3 đã giữ verdict và
deterministic triage ordering, còn S4 hiện lập harvest/binding plans.
