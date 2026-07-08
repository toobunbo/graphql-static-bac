# Argument Classifier Lexicon

## Mục đích

S2 (Argument Classifier) phân loại từng argument trong GraphQL schema thành:

| Class | Ý nghĩa |
|---|---|
| `object_selector` | Argument dùng để chọn một object cụ thể (e.g. `id: ID!`, `slug: String`) |
| `possible_selector` | Mơ hồ — có thể là selector, có thể là filter |
| `noise` | Không phải selector (pagination, sorting, flags, locale…) |
| `authz_modifier` | Argument thay đổi auth context (e.g. `viewAs`, `impersonatedUser`) |

Chỉ `object_selector` với confidence ≥ `medium` đi vào S3 route analysis.  
`possible_selector` bị loại khỏi pipeline (Deliverable A).

---

## Cách classifier hoạt động

Với mỗi argument (direct hoặc nested trong InputObject), classifier chạy các rule theo thứ tự ưu tiên:

```
1. Tên có trong definite_noise_names?       → Noise                          (confidence: high)
2. Tên bắt đầu bằng authz_modifier_prefix?  → ObjectSelector + AuthzModifier  (confidence: high)
3. Type là GraphQL scalar ID?               → ObjectSelector                  (confidence: high)
4. Type là identity scalar (UUID, GUID…)?   → ObjectSelector                  (confidence: medium)
5. Tên có trong exact_selector_names?       → ObjectSelector                  (confidence: high)
6. Token cuối tên ∈ selector_suffix_tokens? → ObjectSelector                  (confidence: high)
7. Không match gì cả                        → PossibleSelector                (confidence: low)
```

**Conflict** (vừa noise vừa selector) → confidence: low → bị loại khỏi S3.

### Cách tokenize tên

Argument name được split thành tokens theo camelCase/snake_case trước khi so sánh:
- `eventId` → `["event", "id"]`
- `deck_slug` → `["deck", "slug"]`
- `communityIds` → `["community", "ids"]`
- `viewAsUserId` → `["view", "as", "user", "id"]`

Matching là case-insensitive và strip separator.

---

## Default lexicon: `argument-classifier-v1.json`

```json
{
  "model_version": "argument-classifier-v1",

  "exact_selector_names": [
    "assetId", "assetIds",
    "handle", "handles",
    "id", "ids",
    "slug", "slugs",
    "uuid", "uuids"
  ],

  "selector_suffix_tokens": [
    "id", "ids",
    "slug", "slugs",
    "uuid", "uuids"
  ],

  "authz_modifier_prefixes": [
    "actingUser",
    "impersonatedUser",
    "viewAs"
  ],

  "definite_noise_names": [
    "after", "before", "clientMutationId", "currency", "cursor",
    "first", "format", "last", "limit", "locale",
    "offset", "order", "orderBy", "page", "perPage", "sort", "sortBy"
  ],

  "identity_scalar_names": [
    "GlobalID", "GUID", "RelayID", "UUID"
  ]
}
```

### Ý nghĩa từng field

**`exact_selector_names`**  
Tên argument khớp chính xác (case-insensitive, normalize separator) → luôn là selector.  
Ví dụ: `id`, `slug`, `assetId`, `handle`, `uuid`.

**`selector_suffix_tokens`**  
Token *cuối cùng* trong tên argument → selector.  
Ví dụ: `eventId` (suffix `id`), `deckSlug` (suffix `slug`), `communityIds` (suffix `ids`).

**`authz_modifier_prefixes`**  
*Prefix* của tên argument → argument giả mạo auth context.  
Ví dụ: `viewAsUserId`, `actingUserSlug`, `impersonatedUserHandle`.  
Những argument này vừa là `object_selector` vừa là `authz_modifier`.

**`definite_noise_names`**  
Tên argument chắc chắn KHÔNG phải selector → noise. Ưu tiên cao nhất.  
Ví dụ: `first`, `after`, `orderBy`, `page`, `cursor`.

**`identity_scalar_names`**  
Custom scalar type được coi là identity (tương đương GraphQL `ID`).  
Ví dụ: `UUID`, `GlobalID` — argument có type này là selector với confidence `medium`.

---

## Ví dụ phân loại thực tế (schema2/Event)

| Argument | Type | Signal fired | Class | Confidence |
|---|---|---|---|---|
| `Query.event.id` | `ID!` | `type:graphql_id` | object_selector | high |
| `Query.events.ids` | `[String!]` | `name:exact_selector:ids` | object_selector | high |
| `Query.events.slugs` | `[String!]` | `name:exact_selector:slugs` | object_selector | high |
| `Query.events.filters.ids` | `[ID!]` | `type:graphql_id` | object_selector | high |
| `Query.events.filters.communityIds` | `[ID!]` | `type:graphql_id` | object_selector | high |
| `Query.events.first` | `Int` | `name:definite_noise:first` | noise | high |
| `Query.events.after` | `String` | `name:definite_noise:after` | noise | high |
| `EventFilter.status` | `String` | `fallback:possible_selector` | possible_selector | low |

---

## Tạo lexicon cho target cụ thể (`argument-classifier-<target>.json`)

### Khi nào cần lexicon riêng?

Target API có naming convention khác default:

| Trường hợp | Ví dụ | Cần làm |
|---|---|---|
| Dùng `code` hoặc `key` thay `id` | `product(code: String)` | Thêm vào `exact_selector_names` và `selector_suffix_tokens` |
| Dùng `ref` hoặc `token` | `session(token: String)` | Thêm vào `exact_selector_names` |
| Custom scalar identity riêng | `Base58ID`, `HashID` | Thêm vào `identity_scalar_names` |
| Pagination argument tên lạ | `take`, `skip`, `pageSize` | Thêm vào `definite_noise_names` |
| Auth modifier prefix khác | `onBehalfOf`, `asUser` | Thêm vào `authz_modifier_prefixes` |

### Quy trình phân tích schema để tạo lexicon

**Bước 1 — Lấy tất cả argument names và types:**

```python
import json
schema = json.load(open("schema_ir.json"))["data"]

args = {}
for type_name, type_def in schema["types"].items():
    for field_name, field in type_def.get("fields", {}).items():
        for arg in field.get("arguments", []):
            key = arg["name"]
            type_display = arg["type_ref"]["display"]
            args.setdefault(key, set()).add(type_display)

# Xem frequency
from collections import Counter
freq = Counter({k: len(v) for k, v in args.items()})
for name, count in freq.most_common(30):
    print(f"{count:4d}  {name:30s}  {', '.join(sorted(args[name])[:3])}")
```

**Bước 2 — Lấy tất cả custom scalar types:**

```python
scalars = [
    name for name, t in schema["types"].items()
    if t.get("kind") == "SCALAR" and name not in ("String","Int","Float","Boolean","ID")
]
print(scalars)
# → ["UUID", "ISO8601Date", "JSON", "BigDecimal", "Base58ID"...]
# Cái nào là identity? → thêm vào identity_scalar_names
```

**Bước 3 — Phân loại từng argument name lạ:**

```
Heuristic nhanh:
- Kết thúc bằng Id/Key/Code/Ref/Token  → selector_suffix_tokens
- Đứng một mình: "code", "key", "ref", "token", "name", "username" → xem xét exact_selector_names
- "take", "skip", "size", "count", "pageSize", "perPage" → definite_noise_names  
- Mô tả filter: "status", "type", "category", "sport" → để nguyên possible_selector
- Tên dài mô tả: "includeArchived", "withDeleted" → để nguyên possible_selector
```

**Bước 4 — Tạo file:**

```json
{
  "model_version": "argument-classifier-v1",

  "exact_selector_names": [
    "assetId", "assetIds", "handle", "handles",
    "id", "ids", "slug", "slugs", "uuid", "uuids",
    "code", "key", "ref", "token"
  ],

  "selector_suffix_tokens": [
    "id", "ids", "slug", "slugs", "uuid", "uuids",
    "key", "code", "ref", "token"
  ],

  "authz_modifier_prefixes": [
    "actingUser", "impersonatedUser", "viewAs",
    "onBehalfOf", "asUser"
  ],

  "definite_noise_names": [
    "after", "before", "clientMutationId", "currency", "cursor",
    "first", "format", "last", "limit", "locale",
    "offset", "order", "orderBy", "page", "perPage", "sort", "sortBy",
    "take", "skip", "pageSize", "size", "count"
  ],

  "identity_scalar_names": [
    "GlobalID", "GUID", "RelayID", "UUID",
    "Base58ID", "HashID"
  ]
}
```

Lưu thành `argument-classifier-<target_name>.json`.

---

## Constraints bắt buộc

- `model_version` phải là `"argument-classifier-v1"`
- Không entry rỗng trong bất kỳ array nào
- Không duplicate sau khi normalize (case-insensitive, strip separator)
- `definite_noise_names` có ưu tiên tuyệt đối — một tên trong danh sách này không bao giờ là selector dù type là `ID`

---

## Những gì KHÔNG nên thêm vào lexicon

| Argument | Lý do KHÔNG thêm |
|---|---|
| `sport`, `category`, `status` | Là filter/enum, không phải object selector — để `possible_selector` là đúng |
| `locale`, `currency` | Rõ ràng là noise, nhưng không cần thêm — default đã có `currency` |
| `includeArchived`, `withDeleted` | Flag boolean, để `possible_selector` — S3 sẽ loại ra |
| `username` (nếu không chắc) | Có thể là display name, không phải unique selector |

Rule cơ bản: chỉ thêm vào `exact_selector_names`/`selector_suffix_tokens` khi **chắc chắn** argument đó là primary key hoặc unique identifier của một object.
