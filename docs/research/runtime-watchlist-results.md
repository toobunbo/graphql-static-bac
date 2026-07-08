# Runtime Results — Watchlist Unknown Cases

**Date:** 2026-06-09
**Accounts:** A = `trnhtinthnh` (attacker), B = `phmgiahuy` (victim)
**Endpoint:** `https://api.sorare.com/graphql`

---

## Summary

| Case | Route | Verdict | Security issue |
|---|---|---|---|
| U01 | `market.watchlist → currentUserSubscription → anySubscribable → Watchlist` | `open` | None |
| U02 | `anyCard → currentUserSubscription → anySubscribable → Watchlist` | `infeasible` | None |
| U03 | `node(EmailSubscription.id) → anySubscribable → Watchlist` | `infeasible` | None |
| U04 | `market.watchlist → currentUserSubscription → subscriber → CurrentUser → myWatchlists` | `guarded` | None |

---

## U01 — Watchlist subscription round-trip identity

**Route:** `route:sha256:a54db9...`
**Probe:** `WatchlistRoundTrip($id)` chạy bởi Account A (1 seed — watchlist công khai của B).

**Kết quả:** `outer.id == inner.id` trên tất cả mẫu. `currentUserSubscription → anySubscribable` luôn trả về chính Watchlist ban đầu.

**Kết luận:** Route identity-preserving. Không có issue — selector controls sink theo đúng nghĩa quan hệ round-trip. Đề xuất framework: thêm relation policy `identity_round_trip`, nâng verdict `unknown → open`.

---

## U02 — Correlated possible type from `anyCard`

**Routes:** `route:sha256:b8e4f6...` (assetId), `route:sha256:440f1b...` (slug)
**Probe:** `AnyCardSubscriptionTarget` — 2 card × 2 selector = 4 mẫu, chạy bởi Account B.

**Kết quả:** `inner.__typename` luôn là `Card` (cùng object với outer), không bao giờ là `Watchlist`. `... on Watchlist` không kha thi vì subscription của một Card luôn trỏ lại chính Card đó.

**Kết luận:** Static analyzer bị false positive do union type `WithSubscriptionsInterface`. Đề xuất framework: thêm correlated possible-type narrowing — khi entry field là `AnyCardInterface`, nhánh `Watchlist` của `WithSubscriptionsInterface` là infeasible. Đóng cả 2 route.

---

## U03 — Subscription global ID controls associated Watchlist

**Route:** `route:sha256:05b697...`
**Probe:** `node(id: Subscription:uuid)` — chạy bởi Account A (owner), Account B (cross-account), anonymous.

**Kết quả:** `node()` từ chối mọi real subscription UUID với lỗi `Invalid ID starting with Subscription:`. Ngược lại, fake string như `nonexistent-uuid` trả `NOT_FOUND` bình thường — tức là `Subscription` type có trong allowlist nhưng real UUID bị một lớp validation thứ hai chặn (nghi vấn server re-interprets hex UUID bytes).

**Kết luận:** Route infeasible tại runtime — `EmailSubscription` không resolve được qua `node()` với ID thực. `type_condition Node→EmailSubscription` trong witness không bao giờ đạt được. Đề xuất framework: đánh dấu infeasible, ghi chú discrepancy giữa `EmailSubscription implements Node` trong schema và behavior thực của resolver.

---

## U04 — Hidden self-scope through subscription subscriber

**Route:** `route:sha256:6aa6be...`
**Probe:** `SubscriberScope($id, $sport)` — 2 seed (2 watchlist khác nhau), chạy bởi Account A.

**Kết quả:**
- `subscriber.__typename` = `CurrentUser` trên cả 2 mẫu.
- `subscriber.id` == `currentUser.id` trên cả 2 mẫu.
- `subscriber.id` không thay đổi khi đổi input Watchlist ID.
- `subscriber.myWatchlists` == `currentUser.myWatchlists` (baseline) — không phụ thuộc vào selector.

**Kết luận:** `EmailSubscription.subscriber → CurrentUser` là hidden self-scope boundary — selector continuity bị cắt tại đây. Output `myWatchlists` chỉ reflect data của requester hiện tại. Không có BAC issue. Đề xuất framework: thêm self-scope policy cho edge này, nâng verdict `unknown → guarded`.

---

## Framework implications

| Observation | Action |
|---|---|
| U01: round-trip identity confirmed | Thêm `identity_round_trip` relation policy |
| U02: correlated possible type = Card only | Thêm possible-type narrowing cho `AnyCardInterface → anySubscribable` |
| U03: `EmailSubscription` không resolve qua `node()` | Ghi chú Node interface discrepancy; mark infeasible |
| U04: subscriber luôn là current requester | Thêm self-scope policy cho `EmailSubscription.subscriber → CurrentUser` |
