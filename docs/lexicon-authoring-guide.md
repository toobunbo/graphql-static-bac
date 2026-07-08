# Lexicon Authoring Guide

Hướng dẫn này dành cho agent tạo file lexicon cho một GraphQL schema mới.
Đọc hết trước khi bắt đầu.

---

## Vai trò của lexicon

Lexicon là bộ quy tắc để `graphql-static-bac` phân loại argument của schema thành:

- `object_selector` — arg mà attacker dùng để nhắm object của victim (ví dụ `userId`, `orderId`)
- `authz_modifier` — arg dùng để leo quyền (ví dụ `viewAs`, `actingUser`)
- `noise` — arg pagination/filter không liên quan đến access control (ví dụ `first`, `limit`)
- `possible_selector` — fallback khi không khớp gì

**Quan trọng:** `possible_selector` và `object_selector` với `confidence: low` đều bị bỏ qua ở bước route analysis. Không có safety net — nếu một selector không được cover trong lexicon, nó biến mất hoàn toàn khỏi kết quả.

---

## Cấu trúc file

```json
{
  "model_version": "argument-classifier-v1",
  "exact_selector_names": [],
  "selector_suffix_tokens": [],
  "authz_modifier_prefixes": [],
  "definite_noise_names": [],
  "identity_scalar_names": []
}
```

`model_version` luôn là `"argument-classifier-v1"`. Không đổi.

---

## Quy trình

### Bước 1 — Thu thập dữ liệu từ schema

Chạy lần lượt:

```bash
# Lấy danh sách tất cả scalar type
grep "^scalar" schema.graphql

# Lấy danh sách tất cả argument name (unique, sorted)
cargo run -- stage s0 --input schema.graphql --output /tmp/ir.json
python3 -c "
import json, sys
ir = json.load(open('/tmp/ir.json'))['data']
args = set()
for t in ir['types'].values():
    for f in t.get('fields', {}).values():
        for a in f.get('arguments', []):
            args.add(a['name'])
print('\n'.join(sorted(args)))
"
```

Từ hai danh sách đó, điền vào 5 field theo quy tắc bên dưới.

---

### Bước 2 — `identity_scalar_names`

Lấy từ output `grep "^scalar"`. Chọn scalar nào **đóng vai định danh object**, không phải scalar dữ liệu.

**Chọn:** `UUID`, `GlobalID`, `GUID`, `RelayID`, `ULID`, `ObjectID`, bất kỳ scalar nào có tên chứa `ID`, `UUID`, `GUID`, `Relay`.

**Bỏ qua:** `DateTime`, `Date`, `JSON`, `Upload`, `URL`, `Email`, `BigDecimal`, `Float`, `Boolean`, tên scalar mang nghĩa giá trị dữ liệu thuần túy.

Khi nghi ngờ, **chọn** — signal này cho `confidence: medium`, tốt hơn bỏ sót.

---

### Bước 3 — `selector_suffix_tokens`

Bắt đầu từ default: `["id", "ids", "slug", "slugs", "uuid", "uuids"]`.

Xem danh sách argument name, tìm thêm suffix nào được dùng như định danh object:
- Nếu schema có nhiều arg kết thúc `*Key`, `*Ref`, `*Code`, `*Handle`, `*Urn` — thêm suffix đó.
- Nếu suffix xuất hiện trong ít hơn 3 field và tên mơ hồ (`name`, `type`, `status`) — **không thêm**, sẽ tạo false positive ồ ạt.

Suffix token là **hậu tố của token cuối cùng sau tokenize** (camelCase được split). Ví dụ: arg `orderId` → tokens `["order", "id"]` → suffix là `id`.

---

### Bước 4 — `exact_selector_names`

Thêm arg name nào **không bị cover bởi suffix** mà vẫn là selector rõ ràng.

Ví dụ điển hình:
- `handle`, `handles` — nếu schema dùng handle thay slug
- `assetId`, `assetIds` — nếu domain dùng "asset" thay "id"
- `nodeId`, `nodeIds` — relay-style node lookup
- Tên arg domain-specific của target mà suffix chưa cover

Dùng exact list khi tên arg là compound đặc thù (`communityProfileId`) mà thêm suffix sẽ quá rộng.

Lưu ý: tokenizer normalize camelCase, nên `asset_id` và `assetId` là duplicate — chỉ ghi một dạng.

---

### Bước 5 — `definite_noise_names`

Bắt đầu từ default: `["after", "before", "clientMutationId", "currency", "cursor", "first", "format", "last", "limit", "locale", "offset", "order", "orderBy", "page", "perPage", "sort", "sortBy"]`.

Xem danh sách argument name của schema, thêm những cái thuộc về:
- **Pagination:** `pageSize`, `endCursor`, `totalCount`, `pageInfo`
- **Search/filter:** `search`, `query`, `filter`, `filters`
- **Locale/format:** `language`, `timezone`, `locale`
- **Boolean flag không liên quan identity:** `isDefault`, `isVisible`, `isActive`

**Không thêm vào noise** nếu arg có thể là selector trong một số context — noise list loại arg hoàn toàn, không có override.

---

### Bước 6 — `authz_modifier_prefixes`

Chỉ thêm nếu schema **có rõ pattern leo quyền**: arg cho phép caller giả danh user khác hoặc chọn policy khác.

Ví dụ: `viewAs`, `actingUser`, `impersonatedUser`, `runAs`, `onBehalfOf`.

Matching là **prefix**, không phải exact: `viewAs` sẽ match `viewAsUserId`, `viewAsRole`...

Nếu không thấy pattern rõ ràng — giữ nguyên default, đừng đoán.

---

## Kiểm tra sau khi tạo

```bash
# Chạy với lexicon mới, so sánh số selector
tophql routes schema.graphql --type <TargetType> --lexicon lexicon.json

# Số selector khi classify
cargo run -- stage s2 --schema-ir /tmp/ir.json --policy lexicon.json --output /tmp/args.json
python3 -c "
import json
data = json.load(open('/tmp/args.json'))['data']
from collections import Counter
classes = []
for f in data['fields'].values():
    for a in f['arguments']:
        classes.extend(a['classifications'])
print(Counter(classes))
"
```

**Dấu hiệu lexicon tốt:**
- Số `object_selector` tăng so với default v1 (cover thêm được selector domain-specific)
- Số `possible_selector` thấp — những gì còn là `possible_selector` là arg thực sự không xác định được
- Không có arg pagination nào lọt vào `object_selector`
- Route count `open` và `unknown` phản ánh đúng attack surface thực tế của target

**Dấu hiệu cần điều chỉnh:**
- `possible_selector` còn nhiều arg trông như selector → thiếu suffix hoặc exact rule
- `object_selector` có arg rõ ràng là filter/pagination → suffix quá rộng hoặc exact list sai
