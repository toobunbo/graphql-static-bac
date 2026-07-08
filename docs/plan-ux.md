# Plan: UX

## Nguyên tắc

1. **Zero config cho 80% use case** — `tophql routes schema.graphql --type Event` chạy được ngay
2. **Output đọc được ngay** — người mới không cần biết gì về pipeline bên trong
3. **Machine-friendly khi cần** — `--format json` cho scripting/integration
4. **Fail rõ ràng** — lỗi phải nói chính xác vấn đề là gì, không phải Rust backtrace

---

## User journey hiện tại vs mục tiêu

### Hiện tại (6 bước, không ai tự làm được)

```bash
# Bước 1: Parse schema
graphql-static-bac stage s0 \
  --input schema.graphql --output /tmp/s0.json

# Bước 2: Classify arguments (cần biết lexicon là gì)
graphql-static-bac stage s2 \
  --schema-ir /tmp/s0.json \
  --policy config/lexicons/argument-classifier-v1.json \
  --output /tmp/s2.json

# Bước 3: Analyze routes (cần biết route policy là gì)
graphql-static-bac route \
  --schema-ir /tmp/s0.json \
  --args /tmp/s2.json \
  --policy config/profiles/route-analysis-v1.json \
  --target Event \
  --output /tmp/routes.json

# Bước 4: Đọc JSON 78MB... → không ai làm
```

### Mục tiêu (1 bước)

```bash
tophql routes schema.graphql --type Event
```

---

## Terminal output design

### Khi chạy (progress)

```
tophql routes schema.graphql --type Event

  Parsing schema...            752 types, 12,847 fields
  Classifying arguments...     2,253 classified  (876 selectors · 1,649 noise · 3,579 filtered)
  Analyzing routes...          313 routes  (6 open · 307 unknown · 0 guarded)
```

Dùng `\r` overwrite trên cùng một dòng — không scroll nhiều.

### Output chính — Table

```
────────────────────────────────────────────────────────────────────────────
  OPEN ROUTES — Event                                            6 routes
────────────────────────────────────────────────────────────────────────────

  #  Path                      Selector                     Type
  ─  ────────────────────────  ───────────────────────────  ─────────────
  1  Query.event               .id                          ID!
  2  Query.events              .ids                         [String!]
  3  Query.events              .slugs                       [String!]
  4  Query.events              .filters.ids                 [ID!]
  5  Query.events              .filters.communityIds        [ID!]
  6  Query.events              .filters.slugs               [String!]

────────────────────────────────────────────────────────────────────────────
  UNKNOWN ROUTES — 307 routes   (use --filter unknown to expand)
────────────────────────────────────────────────────────────────────────────

  Top entry points:
    47 routes via  Document.event
    38 routes via  EventsConnection.nodes
    21 routes via  EventPerson.events
    ...

  Total: 313 routes  ·  6 open  ·  307 unknown  ·  0 guarded
  Time: 1.2s
```

**Quy tắc hiển thị:**
- OPEN routes: luôn hiển thị full, đây là finding chính
- UNKNOWN routes: chỉ hiển thị summary theo entry point, không expand mặc định
- GUARDED routes: không hiển thị mặc định (dùng `--filter guarded` để thấy)
- Với `--filter all`: hiển thị tất cả, paginate nếu > 50 routes

---

## Error messages

### Hiện tại (khó hiểu)

```
route analysis failed: invalid artifact contract: expected stage S0, got S1
```

```
route analysis failed: cross-stage contract violation: S0 and S2 schema fingerprints differ
```

### Mục tiêu (rõ ràng, có action)

```
Error: Cannot read schema
  File: schema.graphql
  Reason: File not found

  Try: tophql routes <path-to-schema.graphql> --type Event
```

```
Error: Unknown type "Events"
  Did you mean "Event"? (close match found)
  Available types: Event, EventPerson, EventsConnection, ...

  Try: tophql routes schema.graphql --type Event
```

```
Error: Type "EventFilter" is not a composite output type (it's an INPUT_OBJECT)
  Route analysis requires Object, Interface, or Union types as targets.

  Try: tophql routes schema.graphql --type Event
```

```
Error: No routes found to "PrivateEvent"
  The type exists but is not reachable from Query root.
  Check if this type is returned by any field accessible from Query.
```

---

## Khi không có flag `--type`

Nếu user quên `--type`, không crash — suggest:

```
Error: Missing --type argument

  Specify which GraphQL type to analyze:
    tophql routes schema.graphql --type <TypeName>

  Object types in this schema (top 10 by field count):
    User, Event, Community, Exhibitor, Document, Planning, ...

  Run without --type to see all types:
    tophql routes schema.graphql --type all    (analyze every type, slow)
    tophql routes schema.graphql --list-types  (just list available types)
```

---

## `--format graphql` output

Dùng để copy-paste thẳng vào GraphQL Playground / Postman:

```graphql
# tophql routes schema.graphql --type Event --filter open --format graphql
# Generated: 2026-06-16
# Target: Event  |  6 open routes found

# ─── Route 1 ─── OPEN ────────────────────────────────────────────────────────
# Path: Query.event → Event
# Selector: id directly identifies Event (confidence: high)

query TestEvent_1($id: ID!) {
  event(id: $id) {
    __typename
    id
  }
}

# ─── Route 4 ─── OPEN ────────────────────────────────────────────────────────
# Path: Query.events → Event
# Selector: filters.ids (nested input object)

query TestEvent_4($ids: [ID!]) {
  events(filters: { ids: $ids }) {
    nodes {
      __typename
      id
    }
  }
}
```

Query được emit trực tiếp từ `witness.edges` — chính xác về structure, đúng với schema. Mỗi route = một query.

---

## `--format md` output — Markdown report

Dùng để paste vào report/issue tracker:

```markdown
# Route Analysis: Event

**Schema:** schema.graphql | **Date:** 2026-06-16 | **Routes:** 313 (6 open)

## Open Routes (High Priority)

| # | Path | Selector | Type |
|---|------|----------|------|
| 1 | `Query.event → Event` | `id` | `ID!` |
| 2 | `Query.events → Event` | `ids` | `[String!]` |
| 3 | `Query.events → Event` | `slugs` | `[String!]` |
| 4 | `Query.events → Event` | `filters.ids` | `[ID!]` |
| 5 | `Query.events → Event` | `filters.communityIds` | `[ID!]` |
| 6 | `Query.events → Event` | `filters.slugs` | `[String!]` |

### Test Queries

**Route 1** — `Query.event(id: ID!)`
```graphql
query { event(id: "<EVENT_ID>") { __typename id } }
```

...

## Unknown Routes (307)

These routes reach Event through traversal. Access control depends on
intermediate field permissions.

Top entry points:
- `Document.event` — 47 routes
- `EventsConnection.nodes` — 38 routes
...
```

---

## `--list-types` helper command

Trước khi biết type nào cần analyze, user cần xem list:

```bash
tophql routes schema.graphql --list-types

Object types in schema.graphql (752 total):

  OUTPUT TYPES (suitable for --type):
    Event, EventPerson, EventsConnection, Community, Exhibitor,
    Document, Planning, User, Profile, ...  (683 types)

  INPUT TYPES (not suitable for --type):
    EventFilter, EventSort, ...  (69 types)
```

---

## Màu sắc

Chỉ khi stdout là terminal (không phải pipe), tắt khi `--no-color` hoặc `NO_COLOR=1`:

| Element | Color |
|---|---|
| "OPEN" label | Green bold |
| "UNKNOWN" label | Yellow |
| "GUARDED" label | Blue |
| Error prefix | Red bold |
| Type names / field paths | Cyan |
| Numbers / counts | White bold |
| Separator lines | Dark gray |

Nếu không muốn thêm crate màu (giữ zero dependency), dùng ANSI codes trực tiếp:

```rust
const GREEN: &str = "\x1b[32;1m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";
```

---

## Không làm (out of scope cho MVP)

- Interactive TUI (arrow keys, filter live) — quá phức tạp
- Web UI / HTML report — phase 2
- Config file (`.tophqlrc`) — không cần ngay
- Shell completion — nice to have
- JSON output với full S3 contract — đã có `--format json` với simplified format; full contract vẫn là domain của `stage s3`
- Progress bar animation (spinner) — optional, chỉ nếu thêm dependency

---

## Scope MVP

Đủ để ship:

| Feature | Priority |
|---|---|
| `tophql routes schema.graphql --type X` chạy được | P0 |
| Table output (OPEN full, UNKNOWN summary) | P0 |
| `--filter open/unknown/all` | P0 |
| Error messages rõ ràng | P0 |
| `--format graphql` (query templates) | P1 |
| `--format json` (simplified) | P1 |
| `--format md` (markdown report) | P2 |
| `--list-types` | P2 |
| Màu sắc terminal | P2 |
| `--out <file>` | P1 |
