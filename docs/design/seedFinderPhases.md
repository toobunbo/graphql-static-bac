# Seed Finder - Thiết kế hai phase

Tài liệu này mô tả Seed Finder theo hai phase độc lập:

```text
Phase 1: Static planning
  S3 routes + S0 IR + S2 facts
  -> route-local requirements + correlation constraints
  -> producer-job alternatives + dependency DAG
  -> binding-set plans
  -> GraphQL harvest queries

Phase 2: Runtime harvesting
  harvest queries + token
  -> candidate values
  -> consumer validation
  -> verified seed bindings
```

Seed Finder chỉ giải quyết việc lấy giá trị hợp lệ để thực thi route. Nó không
phân loại owner/victim, không xác định authorization bug và không chứa security
oracle.

---

## 1. Mục tiêu

Với mỗi route S3, tạo được một hoặc nhiều binding hợp lệ cho những argument mà
route thực sự sử dụng.

Ví dụ:

```text
Route:
Query.anyCard(assetId) -> Card.decks -> Deck

Seed requirement:
arg:Query.anyCard.assetId

Producer:
Card.assetId

Harvest:
Query.currentUser
-> CurrentUser.cards
-> AnyCardInterfaceConnection.nodes
-> ... on Card
-> Card.assetId
```

Kết quả cuối của Seed Finder không phải một danh sách scalar rời rạc, mà là các
binding set đã được consumer chấp nhận:

```json
{
  "route_id": "route:sha256:...",
  "bindings": {
    "arg:Query.anyCard.assetId": "..."
  },
  "status": "verified"
}
```

---

## 2. Khái niệm

### Seed requirement

Một argument cần giá trị để query có thể thực thi đúng route:

```text
arg:Query.anyCard.assetId
```

Requirement thuộc về consumer argument, không thuộc về producer field.

### Producer field

Output field có thể cung cấp candidate value:

```text
field:AnyCardInterface.assetId
field:Card.assetId
field:BaseballCard.assetId
field:NBACard.assetId
```

Một requirement có thể có nhiều producer.

### Producer path

Đường từ Query root tới parent type của producer, sau đó append output field:

```text
Query.currentUser
-> CurrentUser.cards
-> AnyCardInterfaceConnection.nodes
-> ... on Card
-> Card.assetId
```

### Binding plan

Cách cung cấp argument cho producer query:

```text
bounded_pagination
schema_default
schema_enum_value
generated_scalar
seed_dependency
unresolved_literal
```

### Binding set

Một tuple giá trị cùng provenance dùng để chạy một route.

Không được trộn độc lập các pool giá trị khi chúng có quan hệ cha-con:

```text
profile_id + deck_slug
```

phải được giữ trong cùng binding set nếu `deck_slug` chỉ hợp lệ dưới
`profile_id` đó.

---

## 3. Input

### S0 Schema IR

Nguồn có thẩm quyền cho:

- field và return type;
- argument và canonical TypeRef;
- required/default state;
- enum values;
- interfaces và possible types;
- Query root;
- Connection/Edge structure.

### S2 argument facts

Chỉ cung cấp semantic classification:

- `object_selector`;
- `possible_selector`;
- `noise`;
- `authz_modifier`;
- confidence.

S2 không thay thế danh sách argument trong S0.

### S3 routes

Cung cấp:

- `route_id`;
- ordered witness edges;
- active selector;
- selector type và selected type;
- entry field;
- target type.

Seed Finder không dùng verdict để quyết route nào được xử lý. Mọi route có thể
chứa argument cần binding.

---

## 4. Phase 1 - Static planning

### 4.1 Thu thập requirement

Với mỗi route:

1. Lấy active selector của route nếu có.
2. Duyệt mọi FIELD edge trong witness.
3. Từ S0, lấy mọi argument `NON_NULL` không có default.
4. Không thêm optional argument nếu route không yêu cầu dùng nó.
5. Deduplicate requirement theo consumer argument và normalized consumer
   TypeRef.

Điểm quan trọng: không được chỉ đọc `route.selector`. Ví dụ:

```text
Query.node(profileId)
-> FootballUserSportProfile.deck(slug)
-> Deck
```

cần cả `Query.node.id` lẫn `FootballUserSportProfile.deck.slug`.

### 4.2 Phân loại binding không cần producer

Một số argument có thể giải tĩnh:

| Loại | Ví dụ | Cách bind |
|---|---|---|
| Schema default | `sortBy = MOST_FOLLOWED` | omit argument |
| Pagination | `first: Int` | bounded value, ví dụ `20` |
| Required enum | `sport: Sport!` | thử deterministic enum values |
| Boolean | `enabled: Boolean!` | policy-defined candidate set |
| Required selector | `id: ID!` | cần producer |
| Unknown custom scalar | `DateRange!` | unresolved hoặc generator riêng |

Enum values phải giữ thứ tự deterministic. Một enum candidate chưa được coi là
verified cho tới Phase 2.

### 4.3 Derive producer candidates

Từ requirement:

```text
selected_type_id + argument leaf + consumer TypeRef
```

tạo danh sách producer candidates.

Thứ tự evidence:

1. Exact field name trên selected type, type tương thích.
2. Field cùng tên trên interface hoặc concrete implementation liên quan.
3. `ID <-> String` compatibility cho identity-like field.
4. Type-only hoặc ID-like fallback, confidence thấp.

Mỗi derivation phải lưu:

```json
{
  "method": "exact_leaf_match",
  "producer_field_id": "field:Card.assetId",
  "field_locus": "concrete",
  "type_compatibility": "id_to_string",
  "confidence": "high"
}
```

Requirement và producer không dùng chung một ID:

```text
requirement_id = consumer arg + normalized consumer TypeRef
producer_field_id = exact schema field ID
```

### 4.4 Tìm đường tới producer

Scalar/enum output field không phải graph node. Planner thực hiện:

```text
reach parent composite type
-> append exact producer field
```

Ví dụ:

```text
target producer: field:Card.assetId
reachability target: type:Card
terminal projection: Card.assetId
```

Nếu producer nằm trên interface, query trực tiếp field interface. Nếu producer
chỉ nằm trên concrete implementation, witness phải chứa TYPE_CONDITION.

Planner tái sử dụng:

- TypeGraph;
- plumbing detection;
- reverse reachability;
- deterministic witness comparison;
- stable edge IDs.

Nó không gọi nguyên semantic S3 analyzer vì Seed Finder cần state khác.

### 4.5 Argument state trong traversal

Mỗi state mang:

```text
type_id
entry_field_id
binding_plan
unresolved_arg_refs
producer_locus
```

Required arguments được giải trong lúc traversal, trước compaction.

Nếu compact trước rồi mới enrich arguments, một path cần dependency có thể che
mất sibling path tự chạy được.

### 4.6 Seed candidate signature

Một signature tối thiểu:

```text
(
  requirement_id,
  producer_field_id,
  entry_field_id,
  field_locus,
  binding_classes,
  sorted_unresolved_arg_refs,
  read_cardinality
)
```

Giữ một deterministic witness cho mỗi signature. Không dùng riêng
`unresolved_arg_count`, vì hai dependency khác nhau có thể cùng count.

### 4.7 Query emission

Phase 1 dựng GraphQL operation từ:

- ordered witness;
- producer terminal field;
- binding plan;
- TYPE_CONDITION;
- bounded projection;
- operation name.

Ví dụ:

```graphql
query HarvestCardAssetId {
  currentUser {
    cards(first: 20) {
      nodes {
        ... on Card {
          assetId
        }
      }
    }
  }
}
```

Required enum:

```graphql
query HarvestWatchlistId {
  currentUser {
    myWatchlists(sport: FOOTBALL) {
      id
    }
  }
}
```

Mọi emitted document phải được GraphQL parser kiểm tra trước khi ghi artifact.

### 4.8 Correlation constraint và resolution strategy

Correlation constraint và producer strategy là hai trục riêng:

- constraint được suy từ route witness và cố định yêu cầu instance lineage;
- strategy thuộc về producer job và chỉ được chọn sau producer search.

Một constraint có thể được discharge bằng:

```text
joint_co_read
  một query đọc nhiều terminal dưới nearest shared instance anchor

threaded_dependency
  job A harvest value, bind value đó vào required input của job B
```

`independent`, `correlated`, và `dependency` không phải nhãn cố định gắn sớm
vào requirement.

Joint extraction phải giữ branch/index provenance. Hai projection:

```text
profiles[].id
profiles[].decks[].slug
```

không được flatten thành hai pool rồi Cartesian product. Đơn vị extraction là:

```text
profiles[i] -> (profiles[i].id, profiles[i].decks[j].slug)
```

Dependency DAG dùng producer job làm node. Edge ghi rõ:

```text
output field của job A -> input argument của job B
```

Cycle chỉ làm strategy đó infeasible; planner tiếp tục thử alternative khác.

### 4.9 Phase 1 output

Đề xuất artifact:

```text
seed_plans.json
```

Shape rút gọn:

```json
{
  "stage": "S4",
  "data": {
    "planning_model": "seed-planning-v1",
    "routes": {
      "route:sha256:...": {
        "route_id": "route:sha256:...",
        "requirements": [],
        "correlation_constraints": [],
        "producer_jobs": [],
        "dependency_dag": {
          "nodes": [],
          "edges": [],
          "acyclic": true,
          "execution_order": []
        },
        "binding_set_plans": []
      }
    }
  }
}
```

Plan là route-local để giữ correlation. Stable `job_id` vẫn cho phép Phase 2
cache và deduplicate cùng execution giữa nhiều route.

Phase 1 không chứa runtime values.

---

## 5. Phase 2 - Runtime harvesting and validation

### 5.1 Input

- `seed_plans.json`;
- endpoint;
- authentication token/context;
- runtime limits;
- optional HTTP headers.

Phase 2 không cần security policy.

### 5.2 Candidate scheduling

Thử candidate theo policy hiệu năng:

1. `executable == true`;
2. không có seed dependency;
3. read-only trước mutation;
4. ít required bindings hơn;
5. witness ngắn hơn;
6. producer confidence cao hơn;
7. runtime success history.

Ranking chỉ quyết thứ tự chạy, không xóa candidate khỏi artifact.

### 5.3 Execute harvest query

Runner:

1. bind generated literals;
2. gửi harvest operation;
3. normalize GraphQL errors;
4. đọc response theo `read_path`;
5. bỏ null và value sai scalar shape;
6. giữ provenance của từng value.

Kết quả chưa được gọi là verified seed.

### 5.4 Normalize theo consumer TypeRef

Ví dụ:

```text
producer ID! -> consumer String       allowed by compatibility rule
producer ID! -> consumer [ID!]!       aggregate multiple values
producer [ID!] -> consumer ID!        split thành scalar candidates
```

Không tự coercion custom scalar nếu chưa có adapter.

### 5.5 Consumer validation

Candidate value được bind vào consumer argument và chạy một validation
operation tối thiểu.

Validation thành công khi:

- API chấp nhận argument;
- không có type/format error liên quan;
- consumer field trả shape phù hợp;
- nếu có identity companion field, producer và consumer identity tương ứng.

Validation có thể dùng route prefix hoặc full route với projection tối thiểu:

```graphql
query ValidateAnyCardAssetId($assetId: String) {
  anyCard(assetId: $assetId) {
    __typename
    assetId
  }
}
```

Đây là seed-validity check, không phải bug oracle.

### 5.6 Binding-set execution

Phase 2 không tự khám phá correlation. Nó thực thi `binding_set_plan` đã được
Phase 1 lập:

- joint job được extract theo anchor/index descriptor;
- threaded job được chạy theo topological order trong DAG;
- Cartesian product chỉ được phép giữa các dimension independent;
- validation chạy trên binding set hoàn chỉnh, không chạy từng argument rời.

Binding set lưu:

```json
{
  "bindings": {
    "arg:Query.node.id": "profile-id",
    "arg:FootballUserSportProfile.deck.slug": "deck-slug"
  },
  "provenance": {
    "producer_execution_id": "seed_exec:sha256:..."
  }
}
```

Nếu dependency chain tạo hai giá trị trong cùng response branch, extractor phải
giữ chúng thành cùng tuple.

### 5.7 Runtime statuses

Producer execution:

```text
not_run
empty
graphql_error
http_error
extracted
```

`empty` không có nghĩa plan sai; runner có thể thử candidate tiếp theo.

Binding-set validation:

```text
not_run
rejected
verified
```

`verified` là thuộc tính của tuple hoàn chỉnh, không phải producer execution.

### 5.8 Execution dedup

Execution cache key:

```text
hash(
  plan_id,
  endpoint + schema_fingerprint,
  auth_context_id,
  canonical_input_bindings
)
```

Không đưa token vào ID. Cùng operation nhưng khác auth context hoặc input
binding là execution khác.

### 5.9 Phase 2 output

Đề xuất artifact:

```text
verified_seeds.json
```

Shape rút gọn:

```json
{
  "stage": "seed_runtime",
  "data": {
    "route_bindings": [
      {
        "route_id": "route:sha256:...",
        "binding_sets": [
          {
            "binding_set_id": "seed_binding:sha256:...",
            "bindings": {
              "arg:Query.anyCard.assetId": "..."
            },
            "producer_plan_ids": [
              "seed_plan:sha256:..."
            ],
            "validation": {
              "status": "verified",
              "consumer_field_id": "field:Query.anyCard"
            }
          }
        ]
      }
    ],
    "executions": []
  }
}
```

Raw response evidence nên lưu riêng hoặc tham chiếu bằng content hash để tránh
làm artifact chính quá lớn.

---

## 6. Stable IDs

Đề xuất canonical identities:

```text
requirement_id:
  hash(consumer_arg_ref, normalized consumer TypeRef)

constraint_id:
  hash(anchor_type_id, sorted member requirement IDs, dependent field)

job_id:
  hash(strategy, sorted requirement IDs, producer fields,
       ordered edge IDs, canonical bindings, unresolved refs)

execution_id:
  hash(job_id, endpoint + schema fingerprint,
       auth context ID, canonical input bindings)

binding_set_id:
  hash(route_id, sorted arg_ref/value pairs, producer execution IDs)
```

Không đưa token hoặc secret vào hash/artifact.

---

## 7. Invariants

### Phase 1

- Không phát operation nếu GraphQL parser từ chối.
- Required args lấy từ S0, không suy từ S2.
- Mọi unresolved dependency được ghi rõ, không bỏ im.
- Interface và concrete producer là candidate riêng.
- Compaction xảy ra sau khi argument state đã có.
- Không dùng verdict để loại route.
- Correlation constraint tách khỏi producer strategy.
- Mọi executable plan discharge toàn bộ constraint.
- Dependency DAG dùng producer job làm node và phải acyclic.
- Không gọi runtime.

### Phase 2

- Không gọi candidate value là verified trước consumer validation.
- Không mất provenance.
- Không Cartesian product trong correlation group.
- List split phải giữ branch/index provenance.
- Empty/error không xóa plan tĩnh.
- Không đánh giá authorization vulnerability.
- Không ghi token vào artifact hoặc logs.

---

## 8. MVP

### Phase 1 MVP

Hỗ trợ:

- Query root;
- exact leaf match;
- interface/concrete producer;
- `ID <-> String`;
- scalar/list cardinality;
- pagination default;
- enum literals;
- exact required selector dependencies;
- deterministic path/query artifacts.

Chưa hỗ trợ:

- mutation producer;
- arbitrary input-object synthesis;
- custom scalar generators;
- recursive dependency solving hoàn chỉnh.

### Phase 2 MVP

Hỗ trợ:

- một authentication context;
- read-only harvest;
- scalar/list extraction;
- candidate fallback khi empty;
- minimal consumer validation;
- verified binding artifact.

---

## 9. Demo đã xác nhận

Hai query Phase 1 đã được sinh và parse thành công:

```text
output/seed-finder.phase1-demo.json
output/seed-finder.phase1-demo.graphql
tests/seed_phase1_demo_parse.rs
```

Case:

1. `Query.anyCard.assetId <- Card.assetId`
2. `MarketRoot.watchlist.id <- Watchlist.id`, với
   `CurrentUser.myWatchlists.sport` bind từ enum `Sport`.

Demo chưa gọi runtime và chưa xác minh seed value.

---

## 10. Tiêu chí hoàn thành

Phase 1 hoàn thành khi:

- mọi requirement của route được nhận diện;
- producer candidates có evidence;
- candidate paths giữ đúng required bindings;
- query parse được;
- artifact deterministic.

Phase 2 hoàn thành khi:

- runner harvest được candidate values;
- consumer validation phân biệt accepted/rejected;
- binding tuple giữ correlation;
- verified seed artifact join được về `route_id`;
- không có security oracle trong stage.
